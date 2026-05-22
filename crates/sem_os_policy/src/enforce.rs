//! ABAC enforcement helpers for Semantic Registry tool handlers.
//!
//! Wraps the pure `evaluate_abac()` function into an enforcement layer
//! that tool handlers call before returning snapshot data.

use serde_json::json;

use crate::abac::{
    evaluate_abac, evaluate_abac_with_evidence_grade, AccessDecision, AccessPurpose, ActorContext,
};
use sem_os_ontology::attribute_def::AttributeDefBody;
use sem_os_types::{ObjectType, SecurityLabel, SnapshotRow};

/// Result of an enforcement check on a single snapshot.
pub enum EnforceResult {
    /// Access allowed — return full data.
    Allow,
    /// Access allowed but some fields must be masked.
    AllowWithMasking { masked_fields: Vec<String> },
    /// Access denied — return redacted stub.
    Deny { reason: String },
}

/// Check whether the actor may read a snapshot.
///
/// Parses the `security_label` JSONB from the `SnapshotRow` and evaluates
/// ABAC rules. Falls back to deny if the label is unparseable.
pub fn enforce_read(actor: &ActorContext, row: &SnapshotRow) -> EnforceResult {
    if row.object_type == ObjectType::AttributeDef {
        return enforce_attribute_read(actor, row);
    }

    let label: SecurityLabel = match serde_json::from_value(row.security_label.clone()) {
        Ok(l) => l,
        Err(_) => {
            // Unparseable label — conservative deny
            return EnforceResult::Deny {
                reason: "Security label unparseable — access denied by default".into(),
            };
        }
    };

    match evaluate_abac(actor, &label, AccessPurpose::Operations) {
        AccessDecision::Allow => EnforceResult::Allow,
        AccessDecision::AllowWithMasking { masked_fields } => {
            EnforceResult::AllowWithMasking { masked_fields }
        }
        AccessDecision::Deny { reason } => EnforceResult::Deny { reason },
    }
}

/// Check whether the actor may read an attribute definition, including
/// evidence-grade restrictions.
///
/// # Examples
///
/// ```
/// use sem_os_core::abac::{AccessDecision, ActorContext};
/// use sem_os_core::enforce::{enforce_attribute_read, EnforceResult};
/// use sem_os_core::types::{ChangeType, Classification, GovernanceTier, ObjectType, SecurityLabel, SnapshotRow, SnapshotStatus, TrustClass};
///
/// let row = SnapshotRow {
///     snapshot_id: uuid::Uuid::new_v4(),
///     snapshot_set_id: None,
///     object_type: ObjectType::AttributeDef,
///     object_id: uuid::Uuid::new_v4(),
///     version_major: 1,
///     version_minor: 0,
///     status: SnapshotStatus::Active,
///     governance_tier: GovernanceTier::Operational,
///     trust_class: TrustClass::Convenience,
///     security_label: serde_json::to_value(SecurityLabel {
///         classification: Classification::Internal,
///         pii: false,
///         jurisdictions: vec![],
///         purpose_limitation: vec![],
///         handling_controls: vec![],
///     }).unwrap(),
///     effective_from: chrono::Utc::now(),
///     effective_until: None,
///     predecessor_id: None,
///     change_type: ChangeType::Created,
///     change_rationale: None,
///     created_by: "test".into(),
///     approved_by: None,
///     definition: serde_json::json!({
///         "fqn": "test.attr",
///         "name": "test",
///         "description": "test",
///         "domain": "test",
///         "data_type": "string",
///         "evidence_grade": "none"
///     }),
///     created_at: chrono::Utc::now(),
/// };
/// let actor = ActorContext {
///     actor_id: "user-1".into(),
///     roles: vec!["analyst".into()],
///     department: None,
///     clearance: Some(Classification::Internal),
///     jurisdictions: vec![],
/// };
///
/// assert!(matches!(enforce_attribute_read(&actor, &row), EnforceResult::Allow));
/// ```
pub fn enforce_attribute_read(actor: &ActorContext, row: &SnapshotRow) -> EnforceResult {
    let label: SecurityLabel = match serde_json::from_value(row.security_label.clone()) {
        Ok(l) => l,
        Err(_) => {
            return EnforceResult::Deny {
                reason: "Security label unparseable — access denied by default".into(),
            };
        }
    };

    let body: AttributeDefBody = match serde_json::from_value(row.definition.clone()) {
        Ok(b) => b,
        Err(_) => {
            return EnforceResult::Deny {
                reason: "Attribute definition unparseable — access denied by default".into(),
            };
        }
    };

    match evaluate_abac_with_evidence_grade(
        actor,
        &label,
        AccessPurpose::Operations,
        body.evidence_grade,
    ) {
        AccessDecision::Allow => EnforceResult::Allow,
        AccessDecision::AllowWithMasking { masked_fields } => {
            EnforceResult::AllowWithMasking { masked_fields }
        }
        AccessDecision::Deny { reason } => EnforceResult::Deny { reason },
    }
}

/// Check whether the actor may read a row identified only by its raw security_label JSON.
///
/// Used by search handlers that don't load full `SnapshotRow` objects.
pub fn enforce_read_label(
    actor: &ActorContext,
    security_label: &serde_json::Value,
) -> EnforceResult {
    let label: SecurityLabel = match serde_json::from_value(security_label.clone()) {
        Ok(l) => l,
        Err(_) => {
            return EnforceResult::Deny {
                reason: "Security label unparseable — access denied by default".into(),
            };
        }
    };

    match evaluate_abac(actor, &label, AccessPurpose::Operations) {
        AccessDecision::Allow => EnforceResult::Allow,
        AccessDecision::AllowWithMasking { masked_fields } => {
            EnforceResult::AllowWithMasking { masked_fields }
        }
        AccessDecision::Deny { reason } => EnforceResult::Deny { reason },
    }
}

/// Build a redacted stub for a denied snapshot — safe to return to the caller.
pub fn redacted_stub(row: &SnapshotRow, reason: &str) -> serde_json::Value {
    let fqn = row
        .definition
        .get("fqn")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    json!({
        "snapshot_id": row.snapshot_id,
        "object_type": row.object_type.to_string(),
        "fqn": fqn,
        "redacted": true,
        "reason": reason,
    })
}

/// Filter a list of snapshot rows by ABAC enforcement.
///
/// Returns `(allowed, redacted)` — allowed rows pass through, denied rows
/// become redacted stubs. Callers decide whether to include stubs.
pub fn filter_by_abac<'a>(
    actor: &ActorContext,
    rows: &'a [SnapshotRow],
) -> (Vec<&'a SnapshotRow>, Vec<serde_json::Value>) {
    let mut allowed = Vec::new();
    let mut redacted = Vec::new();

    for row in rows {
        match enforce_read(actor, row) {
            EnforceResult::Allow | EnforceResult::AllowWithMasking { .. } => {
                allowed.push(row);
            }
            EnforceResult::Deny { reason } => {
                redacted.push(redacted_stub(row, &reason));
            }
        }
    }

    (allowed, redacted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abac::ActorContext;
    use sem_os_types::*;
    use uuid::Uuid;

    fn make_row(
        classification: Classification,
        pii: bool,
        handling: Vec<HandlingControl>,
    ) -> SnapshotRow {
        SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Convenience,
            security_label: serde_json::to_value(&SecurityLabel {
                classification,
                pii,
                jurisdictions: vec![],
                purpose_limitation: vec![],
                handling_controls: handling,
            })
            .unwrap(),
            effective_from: chrono::Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            definition: serde_json::json!({
                "fqn": "test.attr",
                "name": "Test Attribute",
                "description": "Test attribute used by enforcement unit tests",
                "domain": "test",
                "data_type": "string",
                "evidence_grade": "none"
            }),
            created_at: chrono::Utc::now(),
        }
    }

    fn unprivileged_actor() -> ActorContext {
        ActorContext {
            actor_id: "test_user".into(),
            roles: vec![],
            department: None,
            clearance: Some(Classification::Internal),
            jurisdictions: vec![],
        }
    }

    fn privileged_actor() -> ActorContext {
        ActorContext {
            actor_id: "admin".into(),
            roles: vec!["compliance_officer".into()],
            department: None,
            clearance: Some(Classification::Restricted),
            jurisdictions: vec!["US".into(), "EU".into()],
        }
    }

    #[test]
    fn test_allow_internal_for_internal_clearance() {
        let actor = unprivileged_actor();
        let row = make_row(Classification::Internal, false, vec![]);
        match enforce_read(&actor, &row) {
            EnforceResult::Allow => {}
            other => panic!("Expected Allow, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn test_deny_restricted_for_internal_clearance() {
        let actor = unprivileged_actor();
        let row = make_row(
            Classification::Restricted,
            false,
            vec![HandlingControl::NoLlmExternal],
        );
        match enforce_read(&actor, &row) {
            EnforceResult::Deny { .. } => {}
            other => panic!("Expected Deny, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn test_allow_restricted_for_restricted_clearance() {
        let actor = privileged_actor();
        let row = make_row(Classification::Restricted, false, vec![]);
        match enforce_read(&actor, &row) {
            EnforceResult::Allow => {}
            other => panic!("Expected Allow, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn test_redacted_stub_contains_fqn() {
        let row = make_row(Classification::Internal, false, vec![]);
        let stub = redacted_stub(&row, "test reason");
        assert_eq!(stub["redacted"], true);
        assert_eq!(stub["fqn"], "test.attr");
        assert_eq!(stub["reason"], "test reason");
    }

    #[test]
    fn test_filter_by_abac_splits_correctly() {
        let actor = unprivileged_actor();
        let allowed_row = make_row(Classification::Internal, false, vec![]);
        let denied_row = make_row(
            Classification::Restricted,
            false,
            vec![HandlingControl::NoLlmExternal],
        );
        let rows = vec![allowed_row, denied_row];

        let (allowed, redacted) = filter_by_abac(&actor, &rows);
        assert_eq!(allowed.len(), 1);
        assert_eq!(redacted.len(), 1);
        assert_eq!(redacted[0]["redacted"], true);
    }

    #[test]
    fn test_unparseable_label_denied() {
        let mut row = make_row(Classification::Internal, false, vec![]);
        row.security_label = serde_json::json!("not_a_valid_label");
        let actor = privileged_actor();
        match enforce_read(&actor, &row) {
            EnforceResult::Deny { reason } => {
                assert!(reason.contains("unparseable"));
            }
            other => panic!(
                "Expected Deny for bad label, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn test_enforce_read_label_allow_public() {
        let actor = unprivileged_actor();
        let label = serde_json::json!({
            "classification": "public",
            "handling_controls": [],
            "jurisdictions": []
        });
        assert!(matches!(
            enforce_read_label(&actor, &label),
            EnforceResult::Allow
        ));
    }

    #[test]
    fn test_enforce_read_label_deny_unparseable() {
        let actor = unprivileged_actor();
        let label = serde_json::json!("not_a_valid_label");
        assert!(matches!(
            enforce_read_label(&actor, &label),
            EnforceResult::Deny { .. }
        ));
    }
}
