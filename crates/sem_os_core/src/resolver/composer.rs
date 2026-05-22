use dsl_core::{
    config::dag::{
        load_domain_pack_owned_dags, ClosureType,
        CompletenessAssertionConfig as DagCompletenessAssertionConfig, Dag, EligibilityConstraint,
        LoadedDag, PredicateBinding, Slot as DagSlot, SlotStateMachine,
    },
    resolver::{
        compute_version_hash, ResolvedSlot, ResolvedSource, ResolvedTemplate, ResolvedTransition,
        ResolverProvenance, ShapeRef, SlotProvenance, WorkspaceId,
    },
};
use crate::resolver::shape_rule::{
    load_shape_rules_from_dir, LoadedShapeRule, SlotGateMetadataRefinement, StructuralFacts,
};
use anyhow::{Context, Result};
use dsl_types::constellation_map_def as core_map;
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("workspace not found: {0}")]
    WorkspaceNotFound(String),
    #[error("constellation not found: {0}")]
    ConstellationNotFound(String),
    #[error("shape rule not found: {0}")]
    ShapeRuleNotFound(String),
    #[error("ambiguous vector composition for slot {slot} field {field}: replacement and additive values are both authored in {shape}")]
    AmbiguousVectorComposition {
        slot: String,
        field: String,
        shape: String,
    },
    #[error("shape rule {shape} authors unsupported Phase 2 directive '{directive}'")]
    UnsupportedShapeDirective { shape: String, directive: String },
    #[error(
        "ambiguous shape refinement for slot {slot} field {field}: same-level sources {sources:?}"
    )]
    AmbiguousShapeRefinement {
        slot: String,
        field: String,
        sources: Vec<String>,
    },
    #[error("shape rule inheritance cycle detected: {cycle_path:?}")]
    ShapeRuleCycle { cycle_path: Vec<String> },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct LoadedConstellationMap {
    pub source_path: PathBuf,
    pub body: core_map::ConstellationMapDefBody,
    pub legacy_stack_before: Vec<String>,
    pub legacy_stack_after: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolverInputs {
    pub dag_taxonomies: BTreeMap<String, LoadedDag>,
    pub constellation_maps: BTreeMap<String, LoadedConstellationMap>,
    pub shape_rules: BTreeMap<String, LoadedShapeRule>,
    pub state_machine_paths: Vec<PathBuf>,
    pub shared_atom_paths: Vec<PathBuf>,
    pub seed_root: PathBuf,
}

impl ResolverInputs {
    pub fn from_seed_root(seed_root: impl Into<PathBuf>) -> Result<Self> {
        let seed_root = seed_root.into();
        let config_root = seed_root
            .parent()
            .with_context(|| format!("seed root {} has no config parent", seed_root.display()))?;
        let dag_taxonomies = load_domain_pack_owned_dags(config_root)?;
        let constellation_maps =
            load_constellation_maps_from_dir(&seed_root.join("constellation_maps"))?;
        let shape_rules = load_shape_rules_from_dir(&seed_root.join("shape_rules"))?;
        let state_machine_paths = load_yaml_paths_from_dir(&seed_root.join("state_machines"))?;
        let shared_atom_paths = load_yaml_paths_from_dir(&seed_root.join("shared_atoms"))?;
        Ok(Self {
            dag_taxonomies,
            constellation_maps,
            shape_rules,
            state_machine_paths,
            shared_atom_paths,
            seed_root,
        })
    }

    pub fn from_workspace_config_dir(config_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::from_seed_root(config_dir.into().join("sem_os_seeds"))
    }

    pub fn default_from_cargo_manifest() -> Result<Self> {
        Self::from_workspace_config_dir(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config"),
        )
    }

    pub fn legacy_constellation_stack(
        &self,
        constellation_id: &str,
    ) -> Result<Vec<core_map::ConstellationMapDefBody>, ResolveError> {
        let target = self
            .constellation_maps
            .get(constellation_id)
            .ok_or_else(|| ResolveError::ConstellationNotFound(constellation_id.to_string()))?;

        let mut stack = Vec::new();
        for id in &target.legacy_stack_before {
            push_if_present(&mut stack, &self.constellation_maps, id);
        }
        stack.push(target.body.clone());
        for id in &target.legacy_stack_after {
            push_if_present(&mut stack, &self.constellation_maps, id);
        }
        Ok(stack)
    }
}

fn load_yaml_paths_from_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }

    for entry in std::fs::read_dir(dir).with_context(|| format!("cannot read {dir:?}"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("yaml") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

#[derive(Debug, Default, Deserialize)]
struct SeedLegacyStack {
    #[serde(default)]
    before: Vec<String>,
    #[serde(default)]
    after: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SeedConstellationMap {
    constellation: String,
    #[serde(default)]
    description: Option<String>,
    jurisdiction: String,
    #[serde(default)]
    legacy_stack: SeedLegacyStack,
    #[serde(default)]
    slots: BTreeMap<String, core_map::SlotDef>,
}

pub fn load_constellation_maps_from_dir(
    dir: &Path,
) -> Result<BTreeMap<String, LoadedConstellationMap>> {
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("cannot read {dir:?}"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("yaml") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read constellation map {path:?}"))?;
        let seed: SeedConstellationMap = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse constellation map {path:?}"))?;
        let body = core_map::ConstellationMapDefBody {
            fqn: seed.constellation.clone(),
            constellation: seed.constellation,
            description: seed.description,
            jurisdiction: seed.jurisdiction,
            slots: seed.slots,
        };
        out.insert(
            body.constellation.clone(),
            LoadedConstellationMap {
                source_path: path,
                body,
                legacy_stack_before: seed.legacy_stack.before,
                legacy_stack_after: seed.legacy_stack.after,
            },
        );
    }
    Ok(out)
}

pub fn resolve_template(
    composite_shape: impl Into<ShapeRef>,
    workspace: impl Into<WorkspaceId>,
    inputs: &ResolverInputs,
) -> Result<ResolvedTemplate, ResolveError> {
    let composite_shape = composite_shape.into();
    let workspace = workspace.into();
    let loaded_dag = inputs
        .dag_taxonomies
        .get(&workspace)
        .ok_or_else(|| ResolveError::WorkspaceNotFound(workspace.clone()))?;
    let leaf = inputs
        .constellation_maps
        .get(&composite_shape)
        .ok_or_else(|| ResolveError::ConstellationNotFound(composite_shape.clone()))?;

    let legacy_stack = inputs.legacy_constellation_stack(&composite_shape)?;
    let shape_chain = inputs.shape_rule_chain(&composite_shape)?;
    reject_unsupported_shape_directives(&shape_chain)?;
    inputs.reject_ambiguous_shape_refinements(&composite_shape)?;
    let structural_facts = compose_structural_facts(&shape_chain);
    let mut slot_ids = BTreeSet::new();
    for slot in &loaded_dag.dag.slots {
        slot_ids.insert(slot.id.clone());
    }
    for slot_id in leaf.body.slots.keys() {
        slot_ids.insert(slot_id.clone());
    }
    for rule in &shape_chain {
        for slot_id in rule.body.slots.keys() {
            slot_ids.insert(slot_id.clone());
        }
    }

    let slots = slot_ids
        .into_iter()
        .map(|slot_id| {
            compose_slot(
                &slot_id,
                loaded_dag.dag.slots.iter().find(|slot| slot.id == slot_id),
                leaf.body.slots.get(&slot_id),
                &shape_chain,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let transitions = compose_transitions(&loaded_dag.dag);

    let mut version_paths = vec![loaded_dag.source_path.as_path(), leaf.source_path.as_path()];
    for rule in &shape_chain {
        version_paths.push(rule.source_path.as_path());
    }
    for path in &inputs.state_machine_paths {
        version_paths.push(path.as_path());
    }
    for path in &inputs.shared_atom_paths {
        version_paths.push(path.as_path());
    }
    let version = compute_version_hash(&version_paths, &composite_shape, &workspace);

    Ok(ResolvedTemplate {
        workspace,
        composite_shape,
        structural_facts,
        slots,
        transitions,
        version,
        generated_at: chrono::Utc::now().to_rfc3339(),
        generated_from: ResolverProvenance {
            dag_paths: vec![loaded_dag.source_path.to_string_lossy().to_string()],
            constellation_paths: vec![leaf.source_path.to_string_lossy().to_string()],
            shape_rule_paths: shape_chain
                .iter()
                .map(|rule| rule.source_path.to_string_lossy().to_string())
                .collect(),
            legacy_constellation_stack: legacy_stack,
        },
    })
}

fn compose_structural_facts(shape_chain: &[&LoadedShapeRule]) -> StructuralFacts {
    let mut out = StructuralFacts::default();
    for rule in shape_chain {
        let facts = &rule.body.structural_facts;
        if facts.jurisdiction.is_some() {
            out.jurisdiction = facts.jurisdiction.clone();
        }
        if facts.structure_type.is_some() {
            out.structure_type = facts.structure_type.clone();
        }
        if facts.trading_profile_type.is_some() {
            out.trading_profile_type = facts.trading_profile_type.clone();
        }
        append_unique(
            &mut out.allowed_structure_types,
            &facts.allowed_structure_types,
        );
        append_unique(&mut out.document_bundles, &facts.document_bundles);
        append_unique(&mut out.required_roles, &facts.required_roles);
        append_unique(&mut out.optional_roles, &facts.optional_roles);
        append_unique(&mut out.deferred_roles, &facts.deferred_roles);
    }
    out
}

fn append_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

fn reject_unsupported_shape_directives(
    shape_chain: &[&LoadedShapeRule],
) -> Result<(), ResolveError> {
    for rule in shape_chain {
        for (directive, authored) in [
            (
                "tighten_constraint",
                !rule.body.tighten_constraint.is_empty(),
            ),
            ("add_constraint", !rule.body.add_constraint.is_empty()),
            (
                "replace_constraint",
                !rule.body.replace_constraint.is_empty(),
            ),
            ("insert_between", !rule.body.insert_between.is_empty()),
            ("add_branch", !rule.body.add_branch.is_empty()),
            ("add_terminal", !rule.body.add_terminal.is_empty()),
            ("refine_reducer", !rule.body.refine_reducer.is_empty()),
            ("raw_add", !rule.body.raw_add.is_empty()),
            ("raw_remove", !rule.body.raw_remove.is_empty()),
        ] {
            if authored {
                return Err(ResolveError::UnsupportedShapeDirective {
                    shape: rule.body.shape.clone(),
                    directive: directive.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn compose_slot(
    id: &str,
    dag_slot: Option<&DagSlot>,
    constellation_slot: Option<&core_map::SlotDef>,
    shape_chain: &[&LoadedShapeRule],
) -> Result<ResolvedSlot, ResolveError> {
    let mut provenance = SlotProvenance::default();

    let mut slot = ResolvedSlot {
        id: id.to_string(),
        state_machine: sourced_option(
            &mut provenance,
            "state_machine",
            ResolvedSource::DagTaxonomy,
            dag_state_machine_id(dag_slot),
        )
        .or_else(|| {
            sourced_option(
                &mut provenance,
                "state_machine",
                ResolvedSource::ConstellationMap,
                constellation_slot.and_then(|slot| slot.state_machine.clone()),
            )
        }),
        predicate_bindings: dag_slot
            .and_then(|slot| match &slot.state_machine {
                Some(SlotStateMachine::Structured(machine)) => {
                    source(
                        &mut provenance,
                        "predicate_bindings",
                        ResolvedSource::DagTaxonomy,
                    );
                    Some(machine.predicate_bindings.clone())
                }
                _ => None,
            })
            .unwrap_or_default(),
        table: sourced_option(
            &mut provenance,
            "table",
            ResolvedSource::ConstellationMap,
            constellation_slot.and_then(|slot| slot.table.clone()),
        ),
        pk: sourced_option(
            &mut provenance,
            "pk",
            ResolvedSource::ConstellationMap,
            constellation_slot.and_then(|slot| slot.pk.clone()),
        ),
        join: sourced_option(
            &mut provenance,
            "join",
            ResolvedSource::ConstellationMap,
            constellation_slot.and_then(|slot| slot.join.clone()),
        ),
        entity_kinds: sourced_vec(
            &mut provenance,
            "entity_kinds",
            ResolvedSource::ConstellationMap,
            constellation_slot.map(|slot| &slot.entity_kinds),
        ),
        cardinality: constellation_slot.map(|slot| {
            source(
                &mut provenance,
                "cardinality",
                ResolvedSource::ConstellationMap,
            );
            slot.cardinality
        }),
        depends_on: sourced_vec(
            &mut provenance,
            "depends_on",
            ResolvedSource::ConstellationMap,
            constellation_slot.map(|slot| &slot.depends_on),
        ),
        placeholder: sourced_option(
            &mut provenance,
            "placeholder",
            ResolvedSource::ConstellationMap,
            constellation_slot.and_then(|slot| slot.placeholder.clone()),
        ),
        overlays: sourced_vec(
            &mut provenance,
            "overlays",
            ResolvedSource::ConstellationMap,
            constellation_slot.map(|slot| &slot.overlays),
        ),
        edge_overlays: sourced_vec(
            &mut provenance,
            "edge_overlays",
            ResolvedSource::ConstellationMap,
            constellation_slot.map(|slot| &slot.edge_overlays),
        ),
        verbs: sourced_map(
            &mut provenance,
            "verbs",
            ResolvedSource::ConstellationMap,
            constellation_slot.map(|slot| &slot.verbs),
        ),
        children: sourced_map(
            &mut provenance,
            "children",
            ResolvedSource::ConstellationMap,
            constellation_slot.map(|slot| &slot.children),
        ),
        max_depth: sourced_option(
            &mut provenance,
            "max_depth",
            ResolvedSource::ConstellationMap,
            constellation_slot.and_then(|slot| slot.max_depth),
        ),
        closure: gate_option(
            &mut provenance,
            "closure",
            constellation_slot.and_then(|slot| convert_closure(slot.closure.as_ref())),
            dag_slot.and_then(|slot| slot.closure.clone()),
        ),
        eligibility: gate_option(
            &mut provenance,
            "eligibility",
            constellation_slot.and_then(|slot| convert_eligibility(slot.eligibility.as_ref())),
            dag_slot.and_then(|slot| slot.eligibility.clone()),
        ),
        cardinality_max: gate_option(
            &mut provenance,
            "cardinality_max",
            constellation_slot.and_then(|slot| slot.cardinality_max),
            dag_slot.and_then(|slot| slot.cardinality_max),
        ),
        entry_state: gate_option(
            &mut provenance,
            "entry_state",
            constellation_slot.and_then(|slot| slot.entry_state.clone()),
            dag_slot.and_then(|slot| slot.entry_state.clone()),
        ),
        attachment_predicates: gate_vec(
            &mut provenance,
            "attachment_predicates",
            dag_slot.map(|slot| &slot.attachment_predicates),
            constellation_slot.map(|slot| &slot.attachment_predicates),
        ),
        addition_predicates: gate_vec(
            &mut provenance,
            "addition_predicates",
            dag_slot.map(|slot| &slot.addition_predicates),
            constellation_slot.map(|slot| &slot.addition_predicates),
        ),
        aggregate_breach_checks: gate_vec(
            &mut provenance,
            "aggregate_breach_checks",
            dag_slot.map(|slot| &slot.aggregate_breach_checks),
            constellation_slot.map(|slot| &slot.aggregate_breach_checks),
        ),
        role_guard: gate_option(
            &mut provenance,
            "role_guard",
            constellation_slot.and_then(|slot| convert_role_guard(slot.role_guard.as_ref())),
            dag_slot.and_then(|slot| slot.role_guard.clone()),
        ),
        justification_required: gate_option(
            &mut provenance,
            "justification_required",
            constellation_slot.and_then(|slot| slot.justification_required),
            dag_slot.and_then(|slot| slot.justification_required),
        ),
        audit_class: gate_option(
            &mut provenance,
            "audit_class",
            constellation_slot.and_then(|slot| slot.audit_class.clone()),
            dag_slot.and_then(|slot| slot.audit_class.clone()),
        ),
        completeness_assertion: gate_option(
            &mut provenance,
            "completeness_assertion",
            constellation_slot.and_then(|slot| slot.completeness_assertion.clone()),
            dag_slot.and_then(|slot| {
                slot.completeness_assertion
                    .as_ref()
                    .map(convert_dag_completeness)
            }),
        ),
        provenance,
    };

    for rule in shape_chain {
        if let Some(refinement) = rule.body.slots.get(id) {
            apply_slot_refinement(&mut slot, refinement, &rule.body.shape)?;
        }
    }

    Ok(slot)
}

impl ResolverInputs {
    fn shape_rule_chain(
        &self,
        composite_shape: &str,
    ) -> Result<Vec<&LoadedShapeRule>, ResolveError> {
        let Some(leaf) = self.shape_rules.get(composite_shape) else {
            return Ok(Vec::new());
        };

        let mut out = Vec::new();
        let mut visiting = Vec::new();
        let mut emitted = BTreeSet::new();
        self.push_shape_rule_ancestors(leaf, &mut out, &mut visiting, &mut emitted)?;
        out.push(leaf);
        Ok(out)
    }

    fn push_shape_rule_ancestors<'a>(
        &'a self,
        rule: &'a LoadedShapeRule,
        out: &mut Vec<&'a LoadedShapeRule>,
        visiting: &mut Vec<String>,
        emitted: &mut BTreeSet<String>,
    ) -> Result<(), ResolveError> {
        if let Some(cycle_start) = visiting.iter().position(|shape| shape == &rule.body.shape) {
            let mut cycle_path = visiting[cycle_start..].to_vec();
            cycle_path.push(rule.body.shape.clone());
            return Err(ResolveError::ShapeRuleCycle { cycle_path });
        }

        visiting.push(rule.body.shape.clone());
        for ancestor in &rule.body.extends {
            let loaded = self
                .shape_rules
                .get(ancestor)
                .ok_or_else(|| ResolveError::ShapeRuleNotFound(ancestor.clone()))?;
            self.push_shape_rule_ancestors(loaded, out, visiting, emitted)?;
            if emitted.insert(loaded.body.shape.clone()) {
                out.push(loaded);
            }
        }
        visiting.pop();
        Ok(())
    }

    fn reject_ambiguous_shape_refinements(&self, shape: &str) -> Result<(), ResolveError> {
        let Some(rule) = self.shape_rules.get(shape) else {
            return Ok(());
        };

        let mut visited = BTreeSet::new();
        self.reject_ambiguous_shape_refinements_for_rule(rule, &mut visited)
    }

    fn reject_ambiguous_shape_refinements_for_rule(
        &self,
        rule: &LoadedShapeRule,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), ResolveError> {
        if !visited.insert(rule.body.shape.clone()) {
            return Ok(());
        }

        for ancestor in &rule.body.extends {
            let loaded = self
                .shape_rules
                .get(ancestor)
                .ok_or_else(|| ResolveError::ShapeRuleNotFound(ancestor.clone()))?;
            self.reject_ambiguous_shape_refinements_for_rule(loaded, visited)?;
        }

        for (left_index, left_shape) in rule.body.extends.iter().enumerate() {
            let left = self
                .shape_rules
                .get(left_shape)
                .ok_or_else(|| ResolveError::ShapeRuleNotFound(left_shape.clone()))?;
            for right_shape in rule.body.extends.iter().skip(left_index + 1) {
                let right = self
                    .shape_rules
                    .get(right_shape)
                    .ok_or_else(|| ResolveError::ShapeRuleNotFound(right_shape.clone()))?;
                reject_sibling_slot_conflicts(left, right)?;
            }
        }

        Ok(())
    }
}

fn reject_sibling_slot_conflicts(
    left: &LoadedShapeRule,
    right: &LoadedShapeRule,
) -> Result<(), ResolveError> {
    for (slot_id, left_refinement) in &left.body.slots {
        let Some(right_refinement) = right.body.slots.get(slot_id) else {
            continue;
        };
        reject_refinement_conflict(slot_id, left_refinement, right_refinement, left, right)?;
    }
    Ok(())
}

fn reject_refinement_conflict(
    slot_id: &str,
    left: &SlotGateMetadataRefinement,
    right: &SlotGateMetadataRefinement,
    left_rule: &LoadedShapeRule,
    right_rule: &LoadedShapeRule,
) -> Result<(), ResolveError> {
    let sources = || vec![left_rule.body.shape.clone(), right_rule.body.shape.clone()];

    reject_option_conflict(slot_id, "closure", &left.closure, &right.closure, &sources)?;
    reject_option_conflict(
        slot_id,
        "eligibility",
        &left.eligibility,
        &right.eligibility,
        &sources,
    )?;
    reject_option_conflict(
        slot_id,
        "cardinality_max",
        &left.cardinality_max,
        &right.cardinality_max,
        &sources,
    )?;
    reject_option_conflict(
        slot_id,
        "entry_state",
        &left.entry_state,
        &right.entry_state,
        &sources,
    )?;
    reject_vector_replacement_conflict(
        slot_id,
        "attachment_predicates",
        &left.attachment_predicates,
        &left.additive_attachment_predicates,
        &right.attachment_predicates,
        &right.additive_attachment_predicates,
        &sources,
    )?;
    reject_vector_replacement_conflict(
        slot_id,
        "addition_predicates",
        &left.addition_predicates,
        &left.additive_addition_predicates,
        &right.addition_predicates,
        &right.additive_addition_predicates,
        &sources,
    )?;
    reject_vector_replacement_conflict(
        slot_id,
        "aggregate_breach_checks",
        &left.aggregate_breach_checks,
        &left.additive_aggregate_breach_checks,
        &right.aggregate_breach_checks,
        &right.additive_aggregate_breach_checks,
        &sources,
    )?;
    reject_option_conflict(
        slot_id,
        "role_guard",
        &left.role_guard,
        &right.role_guard,
        &sources,
    )?;
    reject_option_conflict(
        slot_id,
        "justification_required",
        &left.justification_required,
        &right.justification_required,
        &sources,
    )?;
    reject_option_conflict(
        slot_id,
        "audit_class",
        &left.audit_class,
        &right.audit_class,
        &sources,
    )?;
    reject_option_conflict(
        slot_id,
        "completeness_assertion",
        &left.completeness_assertion,
        &right.completeness_assertion,
        &sources,
    )?;
    reject_predicate_binding_conflict(
        slot_id,
        &left.predicate_bindings,
        &right.predicate_bindings,
        &sources,
    )?;

    Ok(())
}

fn reject_option_conflict<T: Serialize>(
    slot_id: &str,
    field: &str,
    left: &Option<T>,
    right: &Option<T>,
    sources: &impl Fn() -> Vec<String>,
) -> Result<(), ResolveError> {
    let (Some(left), Some(right)) = (left, right) else {
        return Ok(());
    };

    let left = serde_yaml::to_value(left).map_err(anyhow::Error::from)?;
    let right = serde_yaml::to_value(right).map_err(anyhow::Error::from)?;
    if left != right {
        return Err(ResolveError::AmbiguousShapeRefinement {
            slot: slot_id.to_string(),
            field: field.to_string(),
            sources: sources(),
        });
    }
    Ok(())
}

fn reject_vector_replacement_conflict(
    slot_id: &str,
    field: &str,
    left_replacement: &[String],
    left_additive: &[String],
    right_replacement: &[String],
    right_additive: &[String],
    sources: &impl Fn() -> Vec<String>,
) -> Result<(), ResolveError> {
    let left_authors_replacement = !left_replacement.is_empty();
    let right_authors_replacement = !right_replacement.is_empty();
    let left_authors_additive = !left_additive.is_empty();
    let right_authors_additive = !right_additive.is_empty();

    let ambiguous = (left_authors_replacement
        && right_authors_replacement
        && left_replacement != right_replacement)
        || (left_authors_replacement && right_authors_additive)
        || (left_authors_additive && right_authors_replacement);

    if ambiguous {
        return Err(ResolveError::AmbiguousShapeRefinement {
            slot: slot_id.to_string(),
            field: field.to_string(),
            sources: sources(),
        });
    }
    Ok(())
}

fn reject_predicate_binding_conflict(
    slot_id: &str,
    left: &[PredicateBinding],
    right: &[PredicateBinding],
    sources: &impl Fn() -> Vec<String>,
) -> Result<(), ResolveError> {
    for left_binding in left {
        for right_binding in right {
            if left_binding.entity == right_binding.entity && left_binding != right_binding {
                return Err(ResolveError::AmbiguousShapeRefinement {
                    slot: slot_id.to_string(),
                    field: "predicate_bindings".to_string(),
                    sources: sources(),
                });
            }
        }
    }
    Ok(())
}

fn apply_slot_refinement(
    slot: &mut ResolvedSlot,
    refinement: &SlotGateMetadataRefinement,
    shape: &str,
) -> Result<(), ResolveError> {
    if let Some(value) = &refinement.closure {
        slot.closure = Some(value.clone());
        source(&mut slot.provenance, "closure", ResolvedSource::ShapeRule);
    }
    if let Some(value) = &refinement.eligibility {
        slot.eligibility = Some(value.clone());
        source(
            &mut slot.provenance,
            "eligibility",
            ResolvedSource::ShapeRule,
        );
    }
    if let Some(value) = refinement.cardinality_max {
        slot.cardinality_max = Some(value);
        source(
            &mut slot.provenance,
            "cardinality_max",
            ResolvedSource::ShapeRule,
        );
    }
    if let Some(value) = &refinement.entry_state {
        slot.entry_state = Some(value.clone());
        source(
            &mut slot.provenance,
            "entry_state",
            ResolvedSource::ShapeRule,
        );
    }
    apply_vector_refinement(
        &slot.id,
        shape,
        "attachment_predicates",
        &mut slot.attachment_predicates,
        &refinement.attachment_predicates,
        &refinement.additive_attachment_predicates,
        &mut slot.provenance,
    )?;
    apply_vector_refinement(
        &slot.id,
        shape,
        "addition_predicates",
        &mut slot.addition_predicates,
        &refinement.addition_predicates,
        &refinement.additive_addition_predicates,
        &mut slot.provenance,
    )?;
    apply_vector_refinement(
        &slot.id,
        shape,
        "aggregate_breach_checks",
        &mut slot.aggregate_breach_checks,
        &refinement.aggregate_breach_checks,
        &refinement.additive_aggregate_breach_checks,
        &mut slot.provenance,
    )?;
    if let Some(value) = &refinement.role_guard {
        slot.role_guard = Some(value.clone());
        source(
            &mut slot.provenance,
            "role_guard",
            ResolvedSource::ShapeRule,
        );
    }
    if let Some(value) = refinement.justification_required {
        slot.justification_required = Some(value);
        source(
            &mut slot.provenance,
            "justification_required",
            ResolvedSource::ShapeRule,
        );
    }
    if let Some(value) = &refinement.audit_class {
        slot.audit_class = Some(value.clone());
        source(
            &mut slot.provenance,
            "audit_class",
            ResolvedSource::ShapeRule,
        );
    }
    if let Some(value) = &refinement.completeness_assertion {
        slot.completeness_assertion = Some(value.clone());
        source(
            &mut slot.provenance,
            "completeness_assertion",
            ResolvedSource::ShapeRule,
        );
    }
    apply_predicate_binding_refinement(slot, &refinement.predicate_bindings, shape)?;
    Ok(())
}

fn apply_predicate_binding_refinement(
    slot: &mut ResolvedSlot,
    refinements: &[PredicateBinding],
    shape: &str,
) -> Result<(), ResolveError> {
    if refinements.is_empty() {
        return Ok(());
    }

    for refinement in refinements {
        match slot
            .predicate_bindings
            .iter()
            .position(|binding| binding.entity == refinement.entity)
        {
            Some(index) if slot.predicate_bindings[index] == *refinement => {}
            Some(index) if slot.predicate_bindings[index].replaceable_by_shape => {
                slot.predicate_bindings[index] = refinement.clone();
            }
            Some(_) => {
                return Err(ResolveError::AmbiguousShapeRefinement {
                    slot: slot.id.clone(),
                    field: "predicate_bindings".to_string(),
                    sources: vec![shape.to_string()],
                });
            }
            None => slot.predicate_bindings.push(refinement.clone()),
        }
    }
    source(
        &mut slot.provenance,
        "predicate_bindings",
        ResolvedSource::ShapeRule,
    );
    Ok(())
}

fn apply_vector_refinement(
    slot_id: &str,
    shape: &str,
    field: &str,
    target: &mut Vec<String>,
    replacement: &[String],
    additive: &[String],
    provenance: &mut SlotProvenance,
) -> Result<(), ResolveError> {
    if !replacement.is_empty() && !additive.is_empty() {
        return Err(ResolveError::AmbiguousVectorComposition {
            slot: slot_id.to_string(),
            field: field.to_string(),
            shape: shape.to_string(),
        });
    }
    if !replacement.is_empty() {
        *target = replacement.to_vec();
        source(provenance, field, ResolvedSource::ShapeRule);
    }
    if !additive.is_empty() {
        target.extend(additive.iter().cloned());
        source(provenance, field, ResolvedSource::ShapeRule);
    }
    Ok(())
}

fn source(provenance: &mut SlotProvenance, field: &str, source: ResolvedSource) {
    provenance.field_sources.insert(field.to_string(), source);
}

fn sourced_option<T>(
    provenance: &mut SlotProvenance,
    field: &str,
    resolved_source: ResolvedSource,
    value: Option<T>,
) -> Option<T> {
    if value.is_some() {
        source(provenance, field, resolved_source);
    }
    value
}

fn sourced_vec<T: Clone>(
    provenance: &mut SlotProvenance,
    field: &str,
    resolved_source: ResolvedSource,
    value: Option<&Vec<T>>,
) -> Vec<T> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    source(provenance, field, resolved_source);
    value.clone()
}

fn sourced_map<K: Ord + Clone, V: Clone>(
    provenance: &mut SlotProvenance,
    field: &str,
    resolved_source: ResolvedSource,
    value: Option<&BTreeMap<K, V>>,
) -> BTreeMap<K, V> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return BTreeMap::new();
    };
    source(provenance, field, resolved_source);
    value.clone()
}

fn gate_option<T>(
    provenance: &mut SlotProvenance,
    field: &str,
    constellation: Option<T>,
    dag: Option<T>,
) -> Option<T> {
    sourced_option(
        provenance,
        field,
        ResolvedSource::ConstellationMap,
        constellation,
    )
    .or_else(|| sourced_option(provenance, field, ResolvedSource::DagTaxonomy, dag))
}

fn dag_state_machine_id(slot: Option<&DagSlot>) -> Option<String> {
    slot.and_then(|slot| match &slot.state_machine {
        Some(SlotStateMachine::Structured(machine)) => Some(machine.id.clone()),
        Some(SlotStateMachine::Reference(reference)) => Some(reference.clone()),
        None => None,
    })
}

fn compose_transitions(dag: &Dag) -> Vec<ResolvedTransition> {
    let mut out = Vec::new();
    for slot in &dag.slots {
        let Some(SlotStateMachine::Structured(machine)) = &slot.state_machine else {
            continue;
        };
        for transition in &machine.transitions {
            let destination_green_when = machine
                .states
                .iter()
                .find(|state| state.id == transition.to)
                .and_then(|state| state.green_when.clone());
            out.push(ResolvedTransition {
                slot_id: slot.id.clone(),
                from: render_yaml_value(&transition.from),
                to: transition.to.clone(),
                via: transition.via.as_ref().map(render_yaml_value),
                destination_green_when,
            });
        }
    }
    out
}

fn render_yaml_value(value: &YamlValue) -> String {
    match value {
        YamlValue::String(value) => value.clone(),
        YamlValue::Sequence(values) => values
            .iter()
            .map(render_yaml_value)
            .collect::<Vec<_>>()
            .join(","),
        YamlValue::Number(value) => value.to_string(),
        YamlValue::Bool(value) => value.to_string(),
        YamlValue::Null => String::new(),
        YamlValue::Mapping(_) | YamlValue::Tagged(_) => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

fn gate_vec(
    provenance: &mut SlotProvenance,
    field: &str,
    dag: Option<&Vec<String>>,
    constellation: Option<&Vec<String>>,
) -> Vec<String> {
    if let Some(values) = constellation.filter(|values| !values.is_empty()) {
        source(provenance, field, ResolvedSource::ConstellationMap);
        return values.clone();
    }
    if let Some(values) = dag.filter(|values| !values.is_empty()) {
        source(provenance, field, ResolvedSource::DagTaxonomy);
        return values.clone();
    }
    Vec::new()
}

fn convert_closure(value: Option<&core_map::ClosureType>) -> Option<ClosureType> {
    Some(match value? {
        core_map::ClosureType::Open => ClosureType::Open,
        core_map::ClosureType::ClosedBounded => ClosureType::ClosedBounded,
        core_map::ClosureType::ClosedUnbounded => ClosureType::ClosedUnbounded,
    })
}

fn convert_eligibility(
    value: Option<&core_map::EligibilityConstraint>,
) -> Option<EligibilityConstraint> {
    Some(match value? {
        core_map::EligibilityConstraint::EntityKinds { entity_kinds } => {
            EligibilityConstraint::EntityKinds {
                entity_kinds: entity_kinds.clone(),
            }
        }
        core_map::EligibilityConstraint::ShapeTaxonomyPosition {
            shape_taxonomy_position,
        } => EligibilityConstraint::ShapeTaxonomyPosition {
            shape_taxonomy_position: shape_taxonomy_position.clone(),
        },
    })
}

fn convert_role_guard(
    value: Option<&core_map::RoleGuard>,
) -> Option<dsl_core::config::dag::RoleGuard> {
    value.map(|guard| dsl_core::config::dag::RoleGuard {
        any_of: guard.any_of.clone(),
        all_of: guard.all_of.clone(),
    })
}

fn convert_dag_completeness(
    value: &DagCompletenessAssertionConfig,
) -> core_map::CompletenessAssertionConfig {
    core_map::CompletenessAssertionConfig {
        predicate: value.predicate.clone(),
        description: value.description.clone(),
        extra: value.extra.clone(),
    }
}

fn push_if_present(
    stack: &mut Vec<core_map::ConstellationMapDefBody>,
    maps: &BTreeMap<String, LoadedConstellationMap>,
    id: &str,
) {
    if let Some(map) = maps.get(id) {
        stack.push(map.body.clone());
    }
}
