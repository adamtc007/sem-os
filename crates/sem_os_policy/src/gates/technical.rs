//! Technical gates — apply to BOTH governance tiers.
//!
//! These gates enforce structural correctness and integrity:
//! 1. Type correctness — verb I/O types match attribute definitions
//! 2. Dependency correctness — no cycles, no unknown attribute refs
//! 3. Security label presence — all objects must have a SecurityLabel
//! 4. Verb surface disclosure — every attribute a verb reads/writes is in its I/O
//! 5. Snapshot integrity — predecessor references and version monotonicity
//! 6. Orphan attributes — attributes not consumed by any verb

use std::collections::HashSet;

use sem_os_ontology::verb_contract::VerbContractBody;
use sem_os_types::{GovernanceTier, SnapshotRow};

use super::{GateFailure, GateSeverity};

// ── Gate 1: Type correctness ─────────────────────────────────

/// Check that a verb's I/O argument types match the attribute and entity type dictionaries.
///
/// Validates three surfaces:
/// 1. `produces.entity_type` — must resolve to a known entity type FQN
/// 2. `consumes` — each consumed FQN must exist in the attribute dictionary
/// 3. `args[].lookup.entity_type` — entity type references should resolve
pub fn check_type_correctness(
    verb: &VerbContractBody,
    known_attribute_fqns: &HashSet<String>,
    known_entity_type_fqns: &HashSet<String>,
) -> Vec<GateFailure> {
    let mut failures = Vec::new();

    // Check args with entity type lookups
    for arg in &verb.args {
        if let Some(ref lookup) = arg.lookup {
            let entity_type_fqn = format!("entity.{}", lookup.entity_type);
            if !known_entity_type_fqns.contains(&entity_type_fqn)
                && !known_entity_type_fqns.contains(&lookup.entity_type)
            {
                failures.push(
                    GateFailure::warning(
                        "type_correctness",
                        "verb_contract",
                        format!(
                            "Verb '{}' arg '{}' has lookup referencing entity type '{}' \
                             which is not in the entity type registry",
                            verb.fqn, arg.name, lookup.entity_type
                        ),
                    )
                    .with_fqn(&verb.fqn)
                    .with_hint("Register the entity type via the scanner or publish it manually"),
                );
            }
        }
    }

    // Check produces: if a verb produces an entity type, verify it exists
    if let Some(ref produces) = verb.produces {
        let produced_fqn = format!("entity.{}", produces.entity_type);
        if !known_entity_type_fqns.contains(&produced_fqn)
            && !known_entity_type_fqns.contains(&produces.entity_type)
        {
            failures.push(
                GateFailure::warning(
                    "type_correctness",
                    "verb_contract",
                    format!(
                        "Verb '{}' produces entity type '{}' which is not in the entity type registry",
                        verb.fqn, produces.entity_type
                    ),
                )
                .with_fqn(&verb.fqn)
                .with_hint("Register the entity type or check for typos in the produces spec"),
            );
        }
    }

    // Check consumes: verify consumed attribute FQNs exist
    for consumed in &verb.consumes {
        let consumed_fqn = format!("{}.{}", verb.domain, consumed);
        if !known_attribute_fqns.contains(&consumed_fqn) && !known_attribute_fqns.contains(consumed)
        {
            failures.push(
                GateFailure::warning(
                    "type_correctness",
                    "verb_contract",
                    format!(
                        "Verb '{}' consumes '{}' which is not in the attribute dictionary",
                        verb.fqn, consumed
                    ),
                )
                .with_fqn(&verb.fqn),
            );
        }
    }

    failures
}

// ── Gate 2: Dependency correctness ───────────────────────────

/// Check that derivation specs have no cycles and no unknown attribute references.
///
/// Delegates to `check_derivation_cycle` and `check_derivation_type_compatibility`
/// from `gates/mod.rs`.
pub fn check_dependency_correctness(
    derivation_specs: &[sem_os_ontology::derivation_spec::DerivationSpecBody],
    known_attribute_fqns: &HashSet<String>,
) -> Vec<GateFailure> {
    let mut failures = Vec::new();

    // Cycle detection
    failures.extend(super::check_derivation_cycle(derivation_specs));

    // Type compatibility for each spec
    for spec in derivation_specs {
        failures.extend(super::check_derivation_type_compatibility(
            spec,
            known_attribute_fqns,
        ));
    }

    failures
}

// ── Gate 3: Security label presence ──────────────────────────

/// Check that a snapshot has a valid (non-empty) security label.
pub fn check_security_label_presence(snapshot: &SnapshotRow) -> Vec<GateFailure> {
    // Parse the security label from JSONB
    match snapshot.parse_security_label() {
        Ok(_label) => {
            // Label exists and is parseable — pass
            vec![]
        }
        Err(e) => {
            vec![GateFailure::error(
                "security_label_presence",
                snapshot.object_type.to_string(),
                format!(
                    "Snapshot {} has invalid or missing security label: {}",
                    snapshot.snapshot_id, e
                ),
            )
            .with_snapshot_id(snapshot.snapshot_id)]
        }
    }
}

// ── Gate 4: Verb surface disclosure ──────────────────────────

/// Check that every attribute a verb reads/writes appears in its I/O surface
/// (args + produces + consumes).
pub fn check_verb_surface_disclosure(
    verb: &VerbContractBody,
    _known_attribute_fqns: &HashSet<String>,
    attribute_sinks_for_verb: &[String],
    attribute_sources_from_verb: &[String],
) -> Vec<GateFailure> {
    let mut failures = Vec::new();

    // Build the verb's declared I/O surface
    let mut declared_surface: HashSet<String> = HashSet::new();

    for arg in &verb.args {
        declared_surface.insert(format!("{}.{}", verb.domain, arg.name));
        declared_surface.insert(arg.name.clone());
    }

    if let Some(ref produces) = verb.produces {
        declared_surface.insert(format!("{}.{}", verb.domain, produces.entity_type));
        declared_surface.insert(produces.entity_type.clone());
    }

    for consumed in &verb.consumes {
        declared_surface.insert(format!("{}.{}", verb.domain, consumed));
        declared_surface.insert(consumed.clone());
    }

    // Check sinks: attributes that declare this verb as a consumer (data the verb writes to)
    for sink_attr_fqn in attribute_sinks_for_verb {
        if !declared_surface.contains(sink_attr_fqn) {
            let short_name = sink_attr_fqn
                .split('.')
                .next_back()
                .unwrap_or(sink_attr_fqn.as_str());
            if !declared_surface.contains(short_name) {
                failures.push(
                    GateFailure::error(
                        "verb_surface_disclosure",
                        "verb_contract",
                        format!(
                            "Attribute '{}' declares verb '{}' as a consumer (sink), \
                             but the verb does not include it in its I/O surface \
                             (args, produces, or consumes)",
                            sink_attr_fqn, verb.fqn
                        ),
                    )
                    .with_fqn(&verb.fqn)
                    .with_hint(
                        "Add this attribute to the verb's consumes list \
                         or update the attribute's sinks",
                    ),
                );
            }
        }
    }

    // Check sources: attributes that declare this verb as their producer
    for source_attr_fqn in attribute_sources_from_verb {
        if !declared_surface.contains(source_attr_fqn) {
            let short_name = source_attr_fqn
                .split('.')
                .next_back()
                .unwrap_or(source_attr_fqn.as_str());
            if !declared_surface.contains(short_name) {
                failures.push(
                    GateFailure::warning(
                        "verb_surface_disclosure",
                        "verb_contract",
                        format!(
                            "Attribute '{}' declares verb '{}' as its producing verb, \
                             but the verb does not include it in its I/O surface",
                            source_attr_fqn, verb.fqn
                        ),
                    )
                    .with_fqn(&verb.fqn)
                    .with_hint(
                        "Update the verb's produces spec or add the attribute to its args list",
                    ),
                );
            }
        }
    }

    failures
}

// ── Gate 5: Snapshot integrity ────────────────────────────────

/// Check that a new snapshot correctly references its predecessor.
pub fn check_snapshot_integrity(
    snapshot: &SnapshotRow,
    predecessor: Option<&SnapshotRow>,
) -> Vec<GateFailure> {
    let mut failures = Vec::new();

    if let Some(pred) = predecessor {
        // Version monotonicity
        let new_version = (snapshot.version_major, snapshot.version_minor);
        let old_version = (pred.version_major, pred.version_minor);
        if new_version < old_version {
            failures.push(
                GateFailure::error(
                    "snapshot_integrity",
                    snapshot.object_type.to_string(),
                    format!(
                        "Version {}.{} is less than predecessor {}.{}",
                        snapshot.version_major,
                        snapshot.version_minor,
                        pred.version_major,
                        pred.version_minor,
                    ),
                )
                .with_snapshot_id(snapshot.snapshot_id),
            );
        }

        // Object type must match
        if snapshot.object_type != pred.object_type {
            failures.push(
                GateFailure::error(
                    "snapshot_integrity",
                    snapshot.object_type.to_string(),
                    format!(
                        "Snapshot object_type {:?} does not match predecessor {:?}",
                        snapshot.object_type, pred.object_type,
                    ),
                )
                .with_snapshot_id(snapshot.snapshot_id),
            );
        }

        // Object ID must match
        if snapshot.object_id != pred.object_id {
            failures.push(
                GateFailure::error(
                    "snapshot_integrity",
                    snapshot.object_type.to_string(),
                    format!(
                        "Snapshot object_id {} does not match predecessor {}",
                        snapshot.object_id, pred.object_id,
                    ),
                )
                .with_snapshot_id(snapshot.snapshot_id),
            );
        }
    }

    failures
}

// ── Gate 6: Orphan attributes ────────────────────────────────

/// Check for attributes not consumed by any verb.
///
/// - Governed orphans → Error (every governed attribute should be traceable)
/// - Operational orphans → Warning (informational)
pub fn check_orphan_attributes(
    attribute_fqn: &str,
    tier: GovernanceTier,
    consuming_verbs: &[String],
) -> Vec<GateFailure> {
    if consuming_verbs.is_empty() {
        let severity = match tier {
            GovernanceTier::Governed => GateSeverity::Error,
            GovernanceTier::Operational => GateSeverity::Warning,
        };
        let failure = GateFailure {
            gate_name: "orphan_attributes".into(),
            severity,
            object_type: "attribute_def".into(),
            object_fqn: Some(attribute_fqn.into()),
            snapshot_id: None,
            message: format!("Attribute '{}' is not consumed by any verb", attribute_fqn),
            remediation_hint: Some(
                "Add this attribute to a verb's args or consumes list, or deprecate it".into(),
            ),
        };
        vec![failure]
    } else {
        vec![]
    }
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sem_os_ontology::verb_contract::*;
    use sem_os_types::*;
    use uuid::Uuid;

    fn sample_verb() -> VerbContractBody {
        VerbContractBody {
            fqn: "cbu.create".into(),
            domain: "cbu".into(),
            action: "create".into(),
            description: "Create a CBU".into(),
            behavior: "plugin".into(),
            args: vec![VerbArgDef {
                name: "name".into(),
                arg_type: "string".into(),
                required: true,
                description: Some("CBU name".into()),
                lookup: None,
                valid_values: None,
                default: None,
                maps_to: None,
            }],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec![],
            invocation_phrases: vec![],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: true,
            produces_focus: false,
            metadata: None,
            crud_mapping: None,
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        }
    }

    fn mock_snapshot(major: i32, minor: i32) -> SnapshotRow {
        SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: major,
            version_minor: minor,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Convenience,
            security_label: serde_json::json!({"classification": "internal"}),
            effective_from: chrono::Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            definition: serde_json::json!({}),
            created_at: chrono::Utc::now(),
        }
    }

    // ── Type correctness ──────────────────────────────────────

    #[test]
    fn test_type_correctness_no_consumes_passes() {
        let verb = sample_verb();
        let attrs: HashSet<String> = HashSet::new();
        let entities: HashSet<String> = HashSet::new();
        let failures = check_type_correctness(&verb, &attrs, &entities);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_type_correctness_unknown_consumed_warns() {
        let mut verb = sample_verb();
        verb.consumes = vec!["unknown_type".into()];
        let attrs: HashSet<String> = HashSet::new();
        let entities: HashSet<String> = HashSet::new();
        let failures = check_type_correctness(&verb, &attrs, &entities);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("consumes"));
    }

    #[test]
    fn test_type_correctness_known_consumed_passes() {
        let mut verb = sample_verb();
        verb.consumes = vec!["entity".into()];
        let mut attrs: HashSet<String> = HashSet::new();
        attrs.insert("cbu.entity".into());
        let entities: HashSet<String> = HashSet::new();
        let failures = check_type_correctness(&verb, &attrs, &entities);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_type_correctness_unknown_produces_warns() {
        let mut verb = sample_verb();
        verb.produces = Some(VerbProducesSpec {
            entity_type: "nonexistent".into(),
            resolved: false,
        });
        let attrs: HashSet<String> = HashSet::new();
        let entities: HashSet<String> = HashSet::new();
        let failures = check_type_correctness(&verb, &attrs, &entities);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("produces entity type"));
    }

    #[test]
    fn test_type_correctness_known_produces_passes() {
        let mut verb = sample_verb();
        verb.produces = Some(VerbProducesSpec {
            entity_type: "cbu".into(),
            resolved: false,
        });
        let attrs: HashSet<String> = HashSet::new();
        let mut entities: HashSet<String> = HashSet::new();
        entities.insert("entity.cbu".into());
        let failures = check_type_correctness(&verb, &attrs, &entities);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_type_correctness_unknown_lookup_entity_type_warns() {
        let mut verb = sample_verb();
        verb.args[0].lookup = Some(VerbArgLookup {
            table: "entities".into(),
            entity_type: "missing_type".into(),
            schema: None,
            search_key: None,
            primary_key: None,
        });
        let attrs: HashSet<String> = HashSet::new();
        let entities: HashSet<String> = HashSet::new();
        let failures = check_type_correctness(&verb, &attrs, &entities);
        assert_eq!(failures.len(), 1);
        assert!(failures[0]
            .message
            .contains("lookup referencing entity type"));
    }

    // ── Dependency correctness ────────────────────────────────

    #[test]
    fn test_dependency_correctness_no_specs_passes() {
        let failures = check_dependency_correctness(&[], &HashSet::new());
        assert!(failures.is_empty());
    }

    #[test]
    fn test_dependency_correctness_catches_cycle() {
        use sem_os_ontology::derivation_spec::*;
        use sem_os_types::EvidenceGrade;

        let specs = vec![
            DerivationSpecBody {
                fqn: "d1".into(),
                name: "d1".into(),
                description: "test".into(),
                output_attribute_fqn: "a".into(),
                inputs: vec![DerivationInput {
                    attribute_fqn: "b".into(),
                    role: "in".into(),
                    required: true,
                }],
                expression: DerivationExpression::FunctionRef {
                    ref_name: "f".into(),
                },
                null_semantics: NullSemantics::default(),
                freshness_rule: None,
                security_inheritance: SecurityInheritanceMode::default(),
                evidence_grade: EvidenceGrade::default(),
                tests: vec![],
            },
            DerivationSpecBody {
                fqn: "d2".into(),
                name: "d2".into(),
                description: "test".into(),
                output_attribute_fqn: "b".into(),
                inputs: vec![DerivationInput {
                    attribute_fqn: "a".into(),
                    role: "in".into(),
                    required: true,
                }],
                expression: DerivationExpression::FunctionRef {
                    ref_name: "f".into(),
                },
                null_semantics: NullSemantics::default(),
                freshness_rule: None,
                security_inheritance: SecurityInheritanceMode::default(),
                evidence_grade: EvidenceGrade::default(),
                tests: vec![],
            },
        ];
        let failures = check_dependency_correctness(&specs, &HashSet::new());
        assert!(!failures.is_empty());
        let has_cycle = failures.iter().any(|f| f.gate_name == "derivation_cycle");
        assert!(has_cycle);
    }

    // ── Security label presence ───────────────────────────────

    #[test]
    fn test_security_label_presence_valid() {
        let snapshot = mock_snapshot(1, 0);
        let failures = check_security_label_presence(&snapshot);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_security_label_presence_invalid() {
        let mut snapshot = mock_snapshot(1, 0);
        snapshot.security_label = serde_json::json!("not_a_valid_label");
        let failures = check_security_label_presence(&snapshot);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("invalid"));
    }

    // ── Verb surface disclosure ───────────────────────────────

    #[test]
    fn test_verb_surface_disclosure_no_sinks_passes() {
        let verb = sample_verb();
        let known: HashSet<String> = HashSet::new();
        let failures = check_verb_surface_disclosure(&verb, &known, &[], &[]);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_verb_surface_disclosure_undeclared_sink_is_error() {
        let verb = sample_verb();
        let known: HashSet<String> = HashSet::new();
        let sinks = vec!["cbu.hidden_field".to_string()];
        let failures = check_verb_surface_disclosure(&verb, &known, &sinks, &[]);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("consumer (sink)"));
        assert_eq!(failures[0].gate_name, "verb_surface_disclosure");
        assert_eq!(failures[0].severity, GateSeverity::Error);
    }

    #[test]
    fn test_verb_surface_disclosure_declared_sink_passes() {
        let verb = sample_verb();
        let known: HashSet<String> = HashSet::new();
        let sinks = vec!["cbu.name".to_string()];
        let failures = check_verb_surface_disclosure(&verb, &known, &sinks, &[]);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_verb_surface_disclosure_undeclared_source_warns() {
        let verb = sample_verb();
        let known: HashSet<String> = HashSet::new();
        let sources = vec!["cbu.secret_output".to_string()];
        let failures = check_verb_surface_disclosure(&verb, &known, &[], &sources);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("producing verb"));
    }

    // ── Snapshot integrity ────────────────────────────────────

    #[test]
    fn test_snapshot_integrity_no_predecessor_passes() {
        let snapshot = mock_snapshot(1, 0);
        let failures = check_snapshot_integrity(&snapshot, None);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_snapshot_integrity_version_monotonicity() {
        let snapshot = mock_snapshot(1, 0);
        let pred = mock_snapshot(2, 0);
        let failures = check_snapshot_integrity(&snapshot, Some(&pred));
        assert!(!failures.is_empty());
        assert!(failures[0].message.contains("Version"));
    }

    #[test]
    fn test_snapshot_integrity_type_mismatch() {
        let snapshot = mock_snapshot(2, 0);
        let mut pred = mock_snapshot(1, 0);
        pred.object_type = ObjectType::VerbContract;
        let failures = check_snapshot_integrity(&snapshot, Some(&pred));
        assert!(!failures.is_empty());
        assert!(failures[0].message.contains("object_type"));
    }

    #[test]
    fn test_snapshot_integrity_id_mismatch() {
        let snapshot = mock_snapshot(2, 0);
        let pred = mock_snapshot(1, 0);
        let failures = check_snapshot_integrity(&snapshot, Some(&pred));
        assert!(!failures.is_empty());
        assert!(failures[0].message.contains("object_id"));
    }

    #[test]
    fn test_snapshot_integrity_valid_successor() {
        let object_id = Uuid::new_v4();
        let mut snapshot = mock_snapshot(2, 0);
        snapshot.object_id = object_id;
        let mut pred = mock_snapshot(1, 0);
        pred.object_id = object_id;
        let failures = check_snapshot_integrity(&snapshot, Some(&pred));
        assert!(failures.is_empty());
    }

    // ── Orphan attributes ─────────────────────────────────────

    #[test]
    fn test_orphan_governed_is_error() {
        let failures = check_orphan_attributes("kyc.risk_score", GovernanceTier::Governed, &[]);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].severity, GateSeverity::Error);
    }

    #[test]
    fn test_orphan_operational_is_warning() {
        let failures = check_orphan_attributes("cbu.temp_field", GovernanceTier::Operational, &[]);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].severity, GateSeverity::Warning);
    }

    #[test]
    fn test_non_orphan_passes() {
        let failures =
            check_orphan_attributes("cbu.name", GovernanceTier::Governed, &["cbu.create".into()]);
        assert!(failures.is_empty());
    }
}
