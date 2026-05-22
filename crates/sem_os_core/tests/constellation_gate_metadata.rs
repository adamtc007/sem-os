use sem_os_ontology::constellation_map_def::{
    Cardinality, ClosureType, ConstellationMapDefBody, EligibilityConstraint,
};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(serde::Deserialize)]
struct SeedConstellationMap {
    slots: BTreeMap<String, sem_os_ontology::constellation_map_def::SlotDef>,
}

fn constellation_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/sem_os_seeds/constellation_maps")
}

#[test]
fn slot_def_gate_metadata_batch_1a_round_trips() {
    let yaml = r#"
fqn: demo.map
constellation: demo.map
description: Demo map
jurisdiction: ALL
slots:
  investment_manager:
    type: entity
    entity_kinds: [company]
    cardinality: optional
    closure: closed_unbounded
    eligibility:
      shape_taxonomy_position: cbu.lux.ucits.sicav
    cardinality_max: 3
    entry_state: DISCOVERED
"#;

    let body: ConstellationMapDefBody =
        serde_yaml::from_str(yaml).expect("constellation gate metadata parses");
    let slot = &body.slots["investment_manager"];

    assert_eq!(slot.cardinality, Cardinality::Optional);
    assert_eq!(slot.closure, Some(ClosureType::ClosedUnbounded));
    assert_eq!(
        slot.eligibility,
        Some(EligibilityConstraint::ShapeTaxonomyPosition {
            shape_taxonomy_position: "cbu.lux.ucits.sicav".to_string()
        })
    );
    assert_eq!(slot.cardinality_max, Some(3));
    assert_eq!(slot.entry_state.as_deref(), Some("DISCOVERED"));

    let round_trip = serde_yaml::to_string(&body).expect("serializes");
    let reparsed: ConstellationMapDefBody =
        serde_yaml::from_str(&round_trip).expect("round-trip parses");
    assert_eq!(
        reparsed.slots["investment_manager"].closure,
        Some(ClosureType::ClosedUnbounded)
    );
}

#[test]
fn slot_def_gate_metadata_batch_1a_defaults_absent_fields() {
    let yaml = r#"
fqn: demo.map
constellation: demo.map
jurisdiction: ALL
slots:
  cbu:
    type: cbu
    cardinality: root
"#;

    let body: ConstellationMapDefBody = serde_yaml::from_str(yaml).expect("slot parses");
    let slot = &body.slots["cbu"];

    assert_eq!(slot.closure, None);
    assert_eq!(slot.eligibility, None);
    assert_eq!(slot.cardinality_max, None);
    assert_eq!(slot.entry_state, None);
}

#[test]
fn slot_def_predicate_vectors_batch_1b_round_trips() {
    let yaml = r#"
fqn: demo.map
constellation: demo.map
jurisdiction: ALL
slots:
  vehicle:
    type: entity
    cardinality: optional
    attachment_predicates:
      - "candidate.state = VALIDATED"
    addition_predicates:
      - "this.service.status = ACTIVE"
    aggregate_breach_checks:
      - "count(vehicle) <= 3"
"#;

    let body: ConstellationMapDefBody =
        serde_yaml::from_str(yaml).expect("predicate vectors parse");
    let slot = &body.slots["vehicle"];

    assert_eq!(
        slot.attachment_predicates,
        vec!["candidate.state = VALIDATED".to_string()]
    );
    assert_eq!(
        slot.addition_predicates,
        vec!["this.service.status = ACTIVE".to_string()]
    );
    assert_eq!(
        slot.aggregate_breach_checks,
        vec!["count(vehicle) <= 3".to_string()]
    );

    let round_trip = serde_yaml::to_string(&body).expect("serializes");
    let reparsed: ConstellationMapDefBody =
        serde_yaml::from_str(&round_trip).expect("round-trip parses");
    assert_eq!(
        reparsed.slots["vehicle"].attachment_predicates,
        vec!["candidate.state = VALIDATED".to_string()]
    );
}

#[test]
fn slot_def_predicate_vectors_batch_1b_defaults_absent_fields() {
    let yaml = r#"
fqn: demo.map
constellation: demo.map
jurisdiction: ALL
slots:
  cbu:
    type: cbu
    cardinality: root
"#;

    let body: ConstellationMapDefBody = serde_yaml::from_str(yaml).expect("slot parses");
    let slot = &body.slots["cbu"];

    assert!(slot.attachment_predicates.is_empty());
    assert!(slot.addition_predicates.is_empty());
    assert!(slot.aggregate_breach_checks.is_empty());
    assert!(slot.additive_attachment_predicates.is_empty());
    assert!(slot.additive_addition_predicates.is_empty());
    assert!(slot.additive_aggregate_breach_checks.is_empty());
}

#[test]
fn slot_def_additive_predicate_vectors_batch_1b_parse_for_validation() {
    let yaml = r#"
fqn: demo.map
constellation: demo.map
jurisdiction: ALL
slots:
  vehicle:
    type: entity
    cardinality: optional
    +attachment_predicates:
      - "candidate.risk_rating != HIGH"
    +addition_predicates:
      - "this.category = FUND"
    +aggregate_breach_checks:
      - "count(vehicle) <= 4"
"#;

    let body: ConstellationMapDefBody =
        serde_yaml::from_str(yaml).expect("additive predicate vectors parse");
    let slot = &body.slots["vehicle"];

    assert_eq!(
        slot.additive_attachment_predicates,
        vec!["candidate.risk_rating != HIGH".to_string()]
    );
    assert_eq!(
        slot.additive_addition_predicates,
        vec!["this.category = FUND".to_string()]
    );
    assert_eq!(
        slot.additive_aggregate_breach_checks,
        vec!["count(vehicle) <= 4".to_string()]
    );
}

#[test]
fn slot_def_discretionary_metadata_batch_1c_round_trips() {
    let yaml = r#"
fqn: demo.map
constellation: demo.map
description: Demo map
jurisdiction: ALL
slots:
  waiver:
    type: entity
    cardinality: optional
    role_guard:
      any_of: [compliance_officer, operations_lead]
    justification_required: true
    audit_class: discretionary_override
    completeness_assertion:
      predicate: "all expected vehicles reviewed"
      description: Vehicle population reviewed by operations
      evidence_kind: manual_attestation
"#;

    let body: ConstellationMapDefBody =
        serde_yaml::from_str(yaml).expect("discretionary metadata parses");
    let slot = &body.slots["waiver"];

    let role_guard = slot.role_guard.as_ref().expect("role guard present");
    assert_eq!(
        role_guard.any_of,
        vec![
            "compliance_officer".to_string(),
            "operations_lead".to_string()
        ]
    );
    assert_eq!(slot.justification_required, Some(true));
    assert_eq!(slot.audit_class.as_deref(), Some("discretionary_override"));
    let completeness = slot
        .completeness_assertion
        .as_ref()
        .expect("completeness assertion present");
    assert_eq!(
        completeness.predicate.as_deref(),
        Some("all expected vehicles reviewed")
    );
    assert!(completeness.extra.contains_key("evidence_kind"));

    let round_trip = serde_yaml::to_string(&body).expect("serializes");
    let reparsed: ConstellationMapDefBody =
        serde_yaml::from_str(&round_trip).expect("round-trip parses");
    assert_eq!(
        reparsed.slots["waiver"]
            .role_guard
            .as_ref()
            .expect("role guard present")
            .any_of,
        vec![
            "compliance_officer".to_string(),
            "operations_lead".to_string()
        ]
    );
}

#[test]
fn slot_def_discretionary_metadata_batch_1c_defaults_absent_fields() {
    let yaml = r#"
fqn: demo.map
constellation: demo.map
jurisdiction: ALL
slots:
  cbu:
    type: cbu
    cardinality: root
"#;

    let body: ConstellationMapDefBody = serde_yaml::from_str(yaml).expect("slot parses");
    let slot = &body.slots["cbu"];

    assert!(slot.role_guard.is_none());
    assert_eq!(slot.justification_required, None);
    assert_eq!(slot.audit_class, None);
    assert!(slot.completeness_assertion.is_none());
}

#[test]
fn existing_constellation_maps_parse_with_batch_1a_defaults() {
    let mut count = 0usize;
    for entry in fs::read_dir(constellation_dir()).expect("constellation dir readable") {
        let path = entry.expect("dir entry").path();
        let is_yaml = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| matches!(ext, "yaml" | "yml"));
        if !is_yaml {
            continue;
        }
        let contents = fs::read_to_string(&path).expect("map readable");
        let parsed: SeedConstellationMap =
            serde_yaml::from_str(&contents).unwrap_or_else(|err| panic!("{path:?}: {err}"));
        assert!(
            !parsed.slots.is_empty(),
            "{path:?}: expected at least one slot"
        );
        count += 1;
    }

    assert!(
        count > 0,
        "expected to parse at least one seed constellation map"
    );
}
