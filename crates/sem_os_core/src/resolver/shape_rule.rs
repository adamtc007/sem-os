use dsl_core::config::dag::{ClosureType, EligibilityConstraint, PredicateBinding, RoleGuard};
use anyhow::{Context, Result};
use dsl_types::constellation_map_def::{AuditClass, CompletenessAssertionConfig};
pub use dsl_types::resolver_facts::StructuralFacts;
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct LoadedShapeRule {
    pub source_path: PathBuf,
    pub body: ShapeRule,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShapeRule {
    pub shape: String,

    #[serde(default)]
    pub workspace: Option<String>,

    #[serde(default)]
    pub extends: Vec<String>,

    #[serde(default)]
    pub structural_facts: StructuralFacts,

    #[serde(default)]
    pub slots: BTreeMap<String, SlotGateMetadataRefinement>,

    #[serde(default)]
    pub tighten_constraint: Vec<TightenConstraint>,

    #[serde(default)]
    pub add_constraint: Vec<AddConstraint>,

    #[serde(default)]
    pub replace_constraint: Vec<ReplaceConstraint>,

    #[serde(default)]
    pub insert_between: Vec<InsertBetween>,

    #[serde(default)]
    pub add_branch: Vec<AddBranch>,

    #[serde(default)]
    pub add_terminal: Vec<AddTerminal>,

    #[serde(default)]
    pub refine_reducer: Vec<RefineReducer>,

    #[serde(default)]
    pub raw_add: Vec<RawStateEdit>,

    #[serde(default)]
    pub raw_remove: Vec<RawStateEdit>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SlotGateMetadataRefinement {
    #[serde(default)]
    pub closure: Option<ClosureType>,

    #[serde(default)]
    pub eligibility: Option<EligibilityConstraint>,

    #[serde(default)]
    pub cardinality_max: Option<u64>,

    #[serde(default)]
    pub entry_state: Option<String>,

    #[serde(default)]
    pub attachment_predicates: Vec<String>,

    #[serde(
        default,
        rename = "+attachment_predicates",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub additive_attachment_predicates: Vec<String>,

    #[serde(default)]
    pub addition_predicates: Vec<String>,

    #[serde(
        default,
        rename = "+addition_predicates",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub additive_addition_predicates: Vec<String>,

    #[serde(default)]
    pub aggregate_breach_checks: Vec<String>,

    #[serde(
        default,
        rename = "+aggregate_breach_checks",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub additive_aggregate_breach_checks: Vec<String>,

    #[serde(default)]
    pub role_guard: Option<RoleGuard>,

    #[serde(default)]
    pub justification_required: Option<bool>,

    #[serde(default)]
    pub audit_class: Option<AuditClass>,

    #[serde(default)]
    pub completeness_assertion: Option<CompletenessAssertionConfig>,

    #[serde(default)]
    pub predicate_bindings: Vec<PredicateBinding>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TightenConstraint {
    pub name: String,
    #[serde(default)]
    pub source_state: Option<Vec<String>>,
    #[serde(default)]
    pub source_predicate: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddConstraint {
    pub name: String,
    #[serde(flatten)]
    pub body: BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReplaceConstraint {
    pub name: String,
    #[serde(flatten)]
    pub body: BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InsertBetween {
    pub from: String,
    pub to: String,
    pub via: Vec<String>,
    #[serde(default)]
    pub enter_verb: Option<String>,
    #[serde(default)]
    pub exit_verb: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddBranch {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub verbs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddTerminal {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub verbs: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RefineReducer {
    #[serde(default)]
    pub conditions: BTreeMap<String, YamlValue>,
    #[serde(default)]
    pub rules: BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawStateEdit {
    pub rationale: String,
    #[serde(flatten)]
    pub body: BTreeMap<String, YamlValue>,
}

pub fn load_shape_rules_from_dir(dir: &Path) -> Result<BTreeMap<String, LoadedShapeRule>> {
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return Ok(out);
    }

    for entry in std::fs::read_dir(dir).with_context(|| format!("cannot read {dir:?}"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("yaml") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read shape rule {path:?}"))?;
        let body: ShapeRule = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse shape rule {path:?}"))?;
        validate_shape_rule(&body, &path)?;
        out.insert(
            body.shape.clone(),
            LoadedShapeRule {
                source_path: path,
                body,
            },
        );
    }

    Ok(out)
}

fn validate_shape_rule(body: &ShapeRule, path: &Path) -> Result<()> {
    validate_no_placeholder(&body.shape, path, "shape")?;
    validate_structural_fact_string(
        body.structural_facts.jurisdiction.as_deref(),
        path,
        &body.shape,
        "structural_facts.jurisdiction",
    )?;
    validate_structural_fact_string(
        body.structural_facts.structure_type.as_deref(),
        path,
        &body.shape,
        "structural_facts.structure_type",
    )?;
    validate_structural_fact_string(
        body.structural_facts.trading_profile_type.as_deref(),
        path,
        &body.shape,
        "structural_facts.trading_profile_type",
    )?;
    validate_structural_fact_values(
        &body.structural_facts.allowed_structure_types,
        path,
        &body.shape,
        "structural_facts.allowed_structure_types",
    )?;
    validate_structural_fact_values(
        &body.structural_facts.document_bundles,
        path,
        &body.shape,
        "structural_facts.document_bundles",
    )?;
    validate_structural_fact_values(
        &body.structural_facts.required_roles,
        path,
        &body.shape,
        "structural_facts.required_roles",
    )?;
    validate_structural_fact_values(
        &body.structural_facts.optional_roles,
        path,
        &body.shape,
        "structural_facts.optional_roles",
    )?;
    validate_structural_fact_values(
        &body.structural_facts.deferred_roles,
        path,
        &body.shape,
        "structural_facts.deferred_roles",
    )?;
    validate_role_partition(body, path)?;
    Ok(())
}

fn validate_structural_fact_string(
    value: Option<&str>,
    path: &Path,
    shape: &str,
    field: &str,
) -> Result<()> {
    if let Some(value) = value {
        validate_no_placeholder(value, path, &format!("{shape}.{field}"))?;
    }
    Ok(())
}

fn validate_structural_fact_values(
    values: &[String],
    path: &Path,
    shape: &str,
    field: &str,
) -> Result<()> {
    for value in values {
        validate_no_placeholder(value, path, &format!("{shape}.{field}"))?;
    }
    Ok(())
}

fn validate_no_placeholder(value: &str, path: &Path, field: &str) -> Result<()> {
    if value.contains("${arg.") {
        anyhow::bail!(
            "shape rule {path:?} has unresolved template placeholder in {field}: {value}"
        );
    }
    Ok(())
}

fn validate_role_partition(body: &ShapeRule, path: &Path) -> Result<()> {
    let required = body
        .structural_facts
        .required_roles
        .iter()
        .collect::<BTreeSet<_>>();
    for role in &body.structural_facts.optional_roles {
        if required.contains(role) {
            anyhow::bail!(
                "shape rule {path:?} lists role '{role}' as both required and optional for {}",
                body.shape
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_required_optional_role_overlap() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("bad.yaml"),
            r#"
shape: struct.bad
structural_facts:
  required_roles: [depositary]
  optional_roles: [depositary]
slots: {}
"#,
        )
        .expect("write fixture");

        let err = load_shape_rules_from_dir(dir.path()).expect_err("overlap is invalid");
        assert!(err.to_string().contains("both required and optional"));
    }

    #[test]
    fn rejects_structural_fact_placeholders() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("bad.yaml"),
            r#"
shape: struct.bad
structural_facts:
  jurisdiction: "${arg.master_jurisdiction.internal}"
slots: {}
"#,
        )
        .expect("write fixture");

        let err = load_shape_rules_from_dir(dir.path()).expect_err("placeholder is invalid");
        assert!(err.to_string().contains("unresolved template placeholder"));
    }
}
