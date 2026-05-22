//! Governance gates — **report-only by default**, promote to enforce later.
//!
//! These gates enforce governance-tier policies:
//! 1. Taxonomy membership — governed objects must belong to ≥1 taxonomy
//! 2. Stewardship — governed objects must have a steward (changed_by)
//! 3. Evidence grade policy — governed derivations with AllowedWithConstraints
//!    need passing tests + policy link

use sem_os_ontology::derivation_spec::DerivationSpecBody;
use sem_os_types::EvidenceGrade;
use sem_os_types::{GovernanceTier, SnapshotRow};

use super::{GateFailure, GateSeverity};

// ── Gate 1: Taxonomy membership ──────────────────────────────

/// Check that governed objects belong to at least one taxonomy node.
///
/// - Governed tier with zero memberships → Error
/// - Operational tier with zero memberships → Warning (informational)
pub fn check_taxonomy_membership(
    object_fqn: &str,
    tier: GovernanceTier,
    memberships: &[String],
) -> Vec<GateFailure> {
    if memberships.is_empty() {
        let (severity, message) = match tier {
            GovernanceTier::Governed => (
                GateSeverity::Error,
                format!(
                    "Governed object '{}' has no taxonomy membership — \
                     assign it to at least one taxonomy node",
                    object_fqn
                ),
            ),
            GovernanceTier::Operational => (
                GateSeverity::Warning,
                format!(
                    "Operational object '{}' has no taxonomy membership — \
                     consider classifying it",
                    object_fqn
                ),
            ),
        };
        vec![GateFailure {
            gate_name: "taxonomy_membership".into(),
            severity,
            object_type: "snapshot".into(),
            object_fqn: Some(object_fqn.into()),
            snapshot_id: None,
            message,
            remediation_hint: Some(
                "Add a MembershipRule linking this object to a taxonomy node".into(),
            ),
        }]
    } else {
        vec![]
    }
}

// ── Gate 2: Stewardship ──────────────────────────────────────

/// Check that governed objects have a steward (non-empty `created_by`).
///
/// - Governed tier with empty/placeholder steward → Error
/// - Operational tier → pass (no steward requirement)
pub fn check_stewardship(snapshot: &SnapshotRow, tier: GovernanceTier) -> Vec<GateFailure> {
    match tier {
        GovernanceTier::Operational => vec![],
        GovernanceTier::Governed => {
            let steward = snapshot.created_by.trim();
            if steward.is_empty() || steward == "system" || steward == "unknown" {
                vec![GateFailure::error(
                    "stewardship",
                    snapshot.object_type.to_string(),
                    format!(
                        "Governed snapshot {} has no identifiable steward (created_by = '{}')",
                        snapshot.snapshot_id, snapshot.created_by,
                    ),
                )
                .with_snapshot_id(snapshot.snapshot_id)
                .with_hint(
                    "Set created_by to the steward's identity (e.g., email or service account)",
                )]
            } else {
                vec![]
            }
        }
    }
}

// ── Gate 3: Evidence grade policy ────────────────────────────

/// Check that governed derivations with `AllowedWithConstraints` evidence grade
/// have passing tests and a policy link.
///
/// - `EvidenceGrade::None` / `EvidenceGrade::Prohibited` → always passes
/// - constrained or regulatory evidence grades on governed tier → need tests + policy
/// - Operational tier → pass (no evidence policy enforcement)
pub fn check_evidence_grade_policy(
    derivation: &DerivationSpecBody,
    tier: GovernanceTier,
    has_passing_tests: bool,
    has_policy_link: bool,
) -> Vec<GateFailure> {
    match tier {
        GovernanceTier::Operational => vec![],
        GovernanceTier::Governed => match derivation.evidence_grade {
            EvidenceGrade::None | EvidenceGrade::Prohibited => vec![],
            EvidenceGrade::AllowedWithConstraints | EvidenceGrade::RegulatoryEvidence => {
                let mut failures = Vec::new();
                let grade = derivation.evidence_grade.to_string();

                if !has_passing_tests {
                    failures.push(
                        GateFailure::error(
                            "evidence_grade_policy",
                            "derivation_spec",
                            format!(
                                "Governed derivation '{}' with {} \
                                 evidence grade must have passing test cases",
                                derivation.fqn, grade
                            ),
                        )
                        .with_fqn(&derivation.fqn)
                        .with_hint(
                            "Add test cases to the derivation spec's `tests` field \
                             and ensure they pass",
                        ),
                    );
                }

                if !has_policy_link {
                    failures.push(
                        GateFailure::error(
                            "evidence_grade_policy",
                            "derivation_spec",
                            format!(
                                "Governed derivation '{}' with {} \
                                 evidence grade must reference a policy rule",
                                derivation.fqn, grade
                            ),
                        )
                        .with_fqn(&derivation.fqn)
                        .with_hint(
                            "Link this derivation to a PolicyRule that authorizes \
                             evidence-grade use",
                        ),
                    );
                }

                failures
            }
        },
    }
}

// ── Gate 4: Regulatory linkage ────────────────────────────────

/// Check that governed snapshots referencing regulatory concepts have valid linkage.
///
/// If a governed snapshot's definition contains `regulatory_references` or
/// `regulation_ids`, those references should be non-empty and well-formed.
/// Operational-tier snapshots skip this check.
pub fn check_regulatory_linkage(snapshot: &SnapshotRow, tier: GovernanceTier) -> Vec<GateFailure> {
    match tier {
        GovernanceTier::Operational => vec![],
        GovernanceTier::Governed => {
            let mut failures = Vec::new();
            let def = &snapshot.definition;

            // Check if definition references regulatory concepts but provides no linkage
            let has_regulatory_field =
                def.get("regulatory_references").is_some() || def.get("regulation_ids").is_some();

            if has_regulatory_field {
                let refs_empty = def
                    .get("regulatory_references")
                    .and_then(|v| v.as_array())
                    .is_none_or(|a| a.is_empty());
                let ids_empty = def
                    .get("regulation_ids")
                    .and_then(|v| v.as_array())
                    .is_none_or(|a| a.is_empty());

                if refs_empty && ids_empty {
                    failures.push(
                        GateFailure::error(
                            "regulatory_linkage",
                            snapshot.object_type.to_string(),
                            format!(
                                "Governed snapshot {} declares regulatory fields but all are empty",
                                snapshot.snapshot_id,
                            ),
                        )
                        .with_snapshot_id(snapshot.snapshot_id)
                        .with_hint(
                            "Populate regulatory_references or regulation_ids, \
                             or remove the empty fields",
                        ),
                    );
                }
            }

            failures
        }
    }
}

// ── Gate 5: Review cycle compliance ──────────────────────────

/// Check that governed snapshots have not exceeded their review cycle deadline.
///
/// If a snapshot has an `effective_until` date, it must not be in the past.
/// Governed snapshots without `effective_until` get a Warning suggesting one be set.
pub fn check_review_cycle_compliance(
    snapshot: &SnapshotRow,
    tier: GovernanceTier,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<GateFailure> {
    match tier {
        GovernanceTier::Operational => vec![],
        GovernanceTier::Governed => {
            let mut failures = Vec::new();

            if let Some(until) = snapshot.effective_until {
                if until < now {
                    failures.push(
                        GateFailure::error(
                            "review_cycle_compliance",
                            snapshot.object_type.to_string(),
                            format!(
                                "Governed snapshot {} has expired (effective_until = {})",
                                snapshot.snapshot_id, until,
                            ),
                        )
                        .with_snapshot_id(snapshot.snapshot_id)
                        .with_hint(
                            "Publish a successor snapshot or extend the effective_until date",
                        ),
                    );
                }
            } else {
                // No review deadline — warn for governed objects
                failures.push(
                    GateFailure::warning(
                        "review_cycle_compliance",
                        snapshot.object_type.to_string(),
                        format!(
                            "Governed snapshot {} has no effective_until — \
                             consider setting a review cycle deadline",
                            snapshot.snapshot_id,
                        ),
                    )
                    .with_snapshot_id(snapshot.snapshot_id)
                    .with_hint("Set effective_until to enforce periodic review"),
                );
            }

            failures
        }
    }
}

// ── Gate 6: Version consistency ──────────────────────────────

/// Check that version is monotonically increasing within the object lineage.
///
/// When a predecessor exists, the new snapshot's version must be strictly greater.
/// This is a stricter version of the simple `version_monotonicity` gate — it
/// also checks that the version tuple strictly increases (not just >=).
pub fn check_version_consistency(
    snapshot: &SnapshotRow,
    predecessor: Option<&SnapshotRow>,
) -> Vec<GateFailure> {
    if let Some(pred) = predecessor {
        let new_ver = (snapshot.version_major, snapshot.version_minor);
        let old_ver = (pred.version_major, pred.version_minor);

        if new_ver <= old_ver {
            vec![GateFailure::error(
                "version_consistency",
                snapshot.object_type.to_string(),
                format!(
                    "Version {}.{} must be strictly greater than predecessor {}.{}",
                    snapshot.version_major,
                    snapshot.version_minor,
                    pred.version_major,
                    pred.version_minor,
                ),
            )
            .with_snapshot_id(snapshot.snapshot_id)
            .with_hint("Bump the version_major or version_minor before publishing")]
        } else {
            vec![]
        }
    } else {
        vec![]
    }
}

// ── Gate 7: Continuation completeness ────────────────────────

/// Check that breaking changes have a migration path documented in `change_rationale`.
///
/// When `change_type` is `Breaking`, `change_rationale` must be non-empty and
/// describe the migration path for consumers of the previous version.
pub fn check_continuation_completeness(snapshot: &SnapshotRow) -> Vec<GateFailure> {
    if snapshot.change_type == sem_os_types::ChangeType::Breaking {
        let rationale = snapshot.change_rationale.as_deref().unwrap_or("").trim();
        if rationale.is_empty() {
            vec![GateFailure::error(
                "continuation_completeness",
                snapshot.object_type.to_string(),
                format!(
                    "Snapshot {} is a breaking change but has no change_rationale",
                    snapshot.snapshot_id,
                ),
            )
            .with_snapshot_id(snapshot.snapshot_id)
            .with_hint(
                "Provide a change_rationale describing the migration path \
                 for consumers of the previous version",
            )]
        } else {
            vec![]
        }
    } else {
        vec![]
    }
}

// ── Gate 8: Macro expansion integrity ────────────────────────

/// Check that macro-expanded verb contracts reference valid primitive verbs.
///
/// If a verb contract has `behavior: template` and its definition contains
/// an `expands_to` field, each referenced verb FQN must exist in the
/// known verb set.
pub fn check_macro_expansion_integrity(
    verb_definition: &serde_json::Value,
    verb_fqn: &str,
    known_verb_fqns: &std::collections::HashSet<String>,
) -> Vec<GateFailure> {
    let mut failures = Vec::new();

    // Check if this is a template/macro verb
    let behavior = verb_definition
        .get("behavior")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if behavior != "template" && behavior != "macro" {
        return failures;
    }

    // Extract expansion targets from the definition
    if let Some(expands_to) = verb_definition
        .get("expands_to")
        .or_else(|| verb_definition.get("expansion"))
    {
        let target_verbs: Vec<&str> = match expands_to {
            serde_json::Value::Array(arr) => arr
                .iter()
                .filter_map(|step| step.get("verb").and_then(|v| v.as_str()))
                .collect(),
            _ => vec![],
        };

        for target_verb in target_verbs {
            if !known_verb_fqns.contains(target_verb) {
                failures.push(
                    GateFailure::warning(
                        "macro_expansion_integrity",
                        "verb_contract",
                        format!(
                            "Template verb '{}' references '{}' in its expansion, \
                             but that verb is not registered",
                            verb_fqn, target_verb,
                        ),
                    )
                    .with_fqn(verb_fqn)
                    .with_hint("Register the referenced verb or fix the expansion target"),
                );
            }
        }
    }

    failures
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sem_os_ontology::derivation_spec::*;
    use sem_os_types::*;
    use uuid::Uuid;

    fn mock_snapshot(tier: GovernanceTier, created_by: &str) -> SnapshotRow {
        SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: tier,
            trust_class: TrustClass::Convenience,
            security_label: serde_json::json!({"classification": "internal"}),
            effective_from: chrono::Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: created_by.into(),
            approved_by: None,
            definition: serde_json::json!({}),
            created_at: chrono::Utc::now(),
        }
    }

    fn sample_derivation(evidence_grade: EvidenceGrade) -> DerivationSpecBody {
        DerivationSpecBody {
            fqn: "kyc.risk_score_derived".into(),
            name: "Risk Score".into(),
            description: "Derived risk score".into(),
            output_attribute_fqn: "kyc.risk_score".into(),
            inputs: vec![DerivationInput {
                attribute_fqn: "kyc.raw_score".into(),
                role: "primary".into(),
                required: true,
            }],
            expression: DerivationExpression::FunctionRef {
                ref_name: "compute_risk_score".into(),
            },
            null_semantics: NullSemantics::default(),
            freshness_rule: None,
            security_inheritance: SecurityInheritanceMode::default(),
            evidence_grade,
            tests: vec![],
        }
    }

    // ── Taxonomy membership ───────────────────────────────────

    #[test]
    fn test_taxonomy_governed_no_membership_error() {
        let failures = check_taxonomy_membership("kyc.risk_score", GovernanceTier::Governed, &[]);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].severity, GateSeverity::Error);
        assert!(failures[0].message.contains("no taxonomy membership"));
    }

    #[test]
    fn test_taxonomy_operational_no_membership_warning() {
        let failures =
            check_taxonomy_membership("cbu.temp_field", GovernanceTier::Operational, &[]);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].severity, GateSeverity::Warning);
    }

    #[test]
    fn test_taxonomy_with_membership_passes() {
        let failures = check_taxonomy_membership(
            "kyc.risk_score",
            GovernanceTier::Governed,
            &["kyc.risk_taxonomy.scores".into()],
        );
        assert!(failures.is_empty());
    }

    // ── Stewardship ───────────────────────────────────────────

    #[test]
    fn test_stewardship_governed_with_steward_passes() {
        let snapshot = mock_snapshot(GovernanceTier::Governed, "alice@example.com");
        let failures = check_stewardship(&snapshot, GovernanceTier::Governed);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_stewardship_governed_empty_fails() {
        let snapshot = mock_snapshot(GovernanceTier::Governed, "");
        let failures = check_stewardship(&snapshot, GovernanceTier::Governed);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("no identifiable steward"));
    }

    #[test]
    fn test_stewardship_governed_system_fails() {
        let snapshot = mock_snapshot(GovernanceTier::Governed, "system");
        let failures = check_stewardship(&snapshot, GovernanceTier::Governed);
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn test_stewardship_operational_skips() {
        let snapshot = mock_snapshot(GovernanceTier::Operational, "");
        let failures = check_stewardship(&snapshot, GovernanceTier::Operational);
        assert!(failures.is_empty());
    }

    // ── Evidence grade policy ─────────────────────────────────

    #[test]
    fn test_evidence_prohibited_always_passes() {
        let spec = sample_derivation(EvidenceGrade::Prohibited);
        let failures = check_evidence_grade_policy(&spec, GovernanceTier::Governed, false, false);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_evidence_allowed_governed_missing_both() {
        let spec = sample_derivation(EvidenceGrade::AllowedWithConstraints);
        let failures = check_evidence_grade_policy(&spec, GovernanceTier::Governed, false, false);
        assert_eq!(failures.len(), 2);
        let has_test_failure = failures.iter().any(|f| f.message.contains("passing test"));
        let has_policy_failure = failures.iter().any(|f| f.message.contains("policy rule"));
        assert!(has_test_failure);
        assert!(has_policy_failure);
    }

    #[test]
    fn test_evidence_allowed_governed_with_both() {
        let spec = sample_derivation(EvidenceGrade::AllowedWithConstraints);
        let failures = check_evidence_grade_policy(&spec, GovernanceTier::Governed, true, true);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_evidence_allowed_operational_skips() {
        let spec = sample_derivation(EvidenceGrade::AllowedWithConstraints);
        let failures =
            check_evidence_grade_policy(&spec, GovernanceTier::Operational, false, false);
        assert!(failures.is_empty());
    }

    // ── Regulatory linkage ────────────────────────────────────

    #[test]
    fn test_regulatory_linkage_operational_skips() {
        let mut snapshot = mock_snapshot(GovernanceTier::Operational, "test");
        snapshot.definition =
            serde_json::json!({"regulatory_references": [], "regulation_ids": []});
        let failures = check_regulatory_linkage(&snapshot, GovernanceTier::Operational);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_regulatory_linkage_governed_empty_refs_fails() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.definition =
            serde_json::json!({"regulatory_references": [], "regulation_ids": []});
        let failures = check_regulatory_linkage(&snapshot, GovernanceTier::Governed);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].gate_name, "regulatory_linkage");
        assert!(failures[0].message.contains("empty"));
    }

    #[test]
    fn test_regulatory_linkage_governed_with_refs_passes() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.definition = serde_json::json!({"regulatory_references": ["MiFID II Art.25"]});
        let failures = check_regulatory_linkage(&snapshot, GovernanceTier::Governed);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_regulatory_linkage_no_regulatory_fields_passes() {
        let snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        let failures = check_regulatory_linkage(&snapshot, GovernanceTier::Governed);
        assert!(failures.is_empty());
    }

    // ── Review cycle compliance ───────────────────────────────

    #[test]
    fn test_review_cycle_operational_skips() {
        let snapshot = mock_snapshot(GovernanceTier::Operational, "test");
        let failures = check_review_cycle_compliance(
            &snapshot,
            GovernanceTier::Operational,
            chrono::Utc::now(),
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn test_review_cycle_governed_expired_fails() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.effective_until = Some(chrono::Utc::now() - chrono::Duration::days(1));
        let failures =
            check_review_cycle_compliance(&snapshot, GovernanceTier::Governed, chrono::Utc::now());
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].severity, GateSeverity::Error);
        assert!(failures[0].message.contains("expired"));
    }

    #[test]
    fn test_review_cycle_governed_future_passes() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.effective_until = Some(chrono::Utc::now() + chrono::Duration::days(30));
        let failures =
            check_review_cycle_compliance(&snapshot, GovernanceTier::Governed, chrono::Utc::now());
        assert!(failures.is_empty());
    }

    #[test]
    fn test_review_cycle_governed_no_deadline_warns() {
        let snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        let failures =
            check_review_cycle_compliance(&snapshot, GovernanceTier::Governed, chrono::Utc::now());
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].severity, GateSeverity::Warning);
        assert!(failures[0].message.contains("no effective_until"));
    }

    // ── Version consistency ───────────────────────────────────

    #[test]
    fn test_version_consistency_no_predecessor_passes() {
        let snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        let failures = check_version_consistency(&snapshot, None);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_version_consistency_strictly_greater_passes() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.version_major = 2;
        snapshot.version_minor = 0;
        let mut pred = mock_snapshot(GovernanceTier::Governed, "test");
        pred.version_major = 1;
        pred.version_minor = 5;
        let failures = check_version_consistency(&snapshot, Some(&pred));
        assert!(failures.is_empty());
    }

    #[test]
    fn test_version_consistency_equal_fails() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.version_major = 1;
        snapshot.version_minor = 0;
        let mut pred = mock_snapshot(GovernanceTier::Governed, "test");
        pred.version_major = 1;
        pred.version_minor = 0;
        let failures = check_version_consistency(&snapshot, Some(&pred));
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("strictly greater"));
    }

    #[test]
    fn test_version_consistency_less_fails() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.version_major = 1;
        snapshot.version_minor = 0;
        let mut pred = mock_snapshot(GovernanceTier::Governed, "test");
        pred.version_major = 2;
        pred.version_minor = 0;
        let failures = check_version_consistency(&snapshot, Some(&pred));
        assert_eq!(failures.len(), 1);
    }

    // ── Continuation completeness ─────────────────────────────

    #[test]
    fn test_continuation_non_breaking_passes() {
        let snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        let failures = check_continuation_completeness(&snapshot);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_continuation_breaking_no_rationale_fails() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.change_type = ChangeType::Breaking;
        snapshot.change_rationale = None;
        let failures = check_continuation_completeness(&snapshot);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("breaking change"));
        assert!(failures[0].message.contains("no change_rationale"));
    }

    #[test]
    fn test_continuation_breaking_empty_rationale_fails() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.change_type = ChangeType::Breaking;
        snapshot.change_rationale = Some("   ".into());
        let failures = check_continuation_completeness(&snapshot);
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn test_continuation_breaking_with_rationale_passes() {
        let mut snapshot = mock_snapshot(GovernanceTier::Governed, "test");
        snapshot.change_type = ChangeType::Breaking;
        snapshot.change_rationale =
            Some("Removed field X; consumers should use field Y instead".into());
        let failures = check_continuation_completeness(&snapshot);
        assert!(failures.is_empty());
    }

    // ── Macro expansion integrity ─────────────────────────────

    #[test]
    fn test_macro_expansion_non_template_skips() {
        let def = serde_json::json!({"behavior": "plugin"});
        let known = std::collections::HashSet::new();
        let failures = check_macro_expansion_integrity(&def, "cbu.create", &known);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_macro_expansion_known_targets_passes() {
        let def = serde_json::json!({
            "behavior": "template",
            "expands_to": [
                {"verb": "cbu.create", "args": {}},
                {"verb": "entity.create", "args": {}}
            ]
        });
        let known: std::collections::HashSet<String> = ["cbu.create", "entity.create"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let failures = check_macro_expansion_integrity(&def, "onboarding.setup", &known);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_macro_expansion_unknown_target_warns() {
        let def = serde_json::json!({
            "behavior": "template",
            "expands_to": [
                {"verb": "cbu.create", "args": {}},
                {"verb": "nonexistent.verb", "args": {}}
            ]
        });
        let known: std::collections::HashSet<String> =
            ["cbu.create"].iter().map(|s| s.to_string()).collect();
        let failures = check_macro_expansion_integrity(&def, "onboarding.setup", &known);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("nonexistent.verb"));
        assert_eq!(failures[0].severity, GateSeverity::Warning);
    }

    #[test]
    fn test_macro_expansion_no_expands_to_passes() {
        let def = serde_json::json!({"behavior": "template"});
        let known = std::collections::HashSet::new();
        let failures = check_macro_expansion_integrity(&def, "onboarding.setup", &known);
        assert!(failures.is_empty());
    }
}
