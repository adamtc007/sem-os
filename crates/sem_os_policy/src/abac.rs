//! Attribute-Based Access Control (ABAC) for the semantic registry.
//!
//! ABAC evaluates access decisions based on:
//! - **Actor context** (who is requesting: role, department, clearance)
//! - **Object security label** (classification, PII, jurisdictions)
//! - **Access purpose** (why they need it: operations, audit, analytics)

use serde::{Deserialize, Serialize};

use sem_os_types::{Classification, EvidenceGrade, SecurityLabel};

/// Context about the actor requesting access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorContext {
    /// Actor identifier (user ID, service account, etc.)
    pub actor_id: String,
    /// Actor roles (e.g., `["analyst", "compliance_officer"]`)
    pub roles: Vec<String>,
    /// Actor's department or organizational unit
    #[serde(default)]
    pub department: Option<String>,
    /// Actor's security clearance level
    #[serde(default)]
    pub clearance: Option<Classification>,
    /// Actor's jurisdictions (what jurisdictions they are cleared for)
    #[serde(default)]
    pub jurisdictions: Vec<String>,
}

/// The purpose for which access is being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessPurpose {
    /// Day-to-day operational use
    Operations,
    /// Compliance and audit
    Audit,
    /// Analytics and reporting
    Analytics,
    /// Administrative actions (publishing, configuration)
    Administration,
    /// Read-only inspection
    Inspection,
}

/// The result of an ABAC access decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessDecision {
    /// Full access granted
    Allow,
    /// Access denied
    Deny {
        /// Reason for denial
        reason: String,
    },
    /// Access granted with field-level masking
    AllowWithMasking {
        /// Attribute FQNs that must be masked
        masked_fields: Vec<String>,
    },
}

impl AccessDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow | Self::AllowWithMasking { .. })
    }
}

/// Evaluate ABAC access decision.
///
/// This is a pure function — no database calls.
pub fn evaluate_abac(
    actor: &ActorContext,
    label: &SecurityLabel,
    purpose: AccessPurpose,
) -> AccessDecision {
    // 1. Classification check
    if let Some(ref actor_clearance) = actor.clearance {
        if !clearance_sufficient(actor_clearance, &label.classification) {
            return AccessDecision::Deny {
                reason: format!(
                    "Actor clearance {:?} insufficient for object classification {:?}",
                    actor_clearance, label.classification
                ),
            };
        }
    } else {
        // No clearance specified — only allow Public objects
        if label.classification != Classification::Public {
            return AccessDecision::Deny {
                reason: format!(
                    "Actor has no clearance; object requires {:?}",
                    label.classification
                ),
            };
        }
    }

    // 2. Jurisdiction check
    if !label.jurisdictions.is_empty() {
        let has_jurisdiction = label
            .jurisdictions
            .iter()
            .any(|j| actor.jurisdictions.contains(j));
        if !has_jurisdiction {
            return AccessDecision::Deny {
                reason: format!(
                    "Actor not cleared for object jurisdictions: {:?}",
                    label.jurisdictions
                ),
            };
        }
    }

    // 3. Purpose limitation check
    if !label.purpose_limitation.is_empty() {
        let purpose_str = format!("{:?}", purpose).to_lowercase();
        if !label.purpose_limitation.contains(&purpose_str)
            && !label.purpose_limitation.contains(&"*".to_string())
        {
            return AccessDecision::Deny {
                reason: format!(
                    "Access purpose {:?} not in allowed purposes: {:?}",
                    purpose, label.purpose_limitation
                ),
            };
        }
    }

    // 4. PII check: if object has PII and purpose is analytics, mask PII fields
    if label.pii && purpose == AccessPurpose::Analytics {
        return AccessDecision::AllowWithMasking {
            masked_fields: vec!["*pii*".into()],
        };
    }

    AccessDecision::Allow
}

/// Evaluate ABAC with an additional evidence-grade constraint.
///
/// This preserves the base security-label decision from [`evaluate_abac`] and
/// then applies evidence-plane narrowing for attribute semantics:
///
/// - `None` / `Prohibited`: no extra restriction
/// - `AllowedWithConstraints`: analytics requires stewardship/compliance privilege
/// - `RegulatoryEvidence`: analytics and administration require stewardship/compliance privilege
///
/// # Examples
///
/// ```
/// use sem_os_core::abac::{evaluate_abac_with_evidence_grade, AccessDecision, AccessPurpose, ActorContext};
/// use sem_os_core::types::{Classification, EvidenceGrade, SecurityLabel};
///
/// let actor = ActorContext {
///     actor_id: "user-1".into(),
///     roles: vec!["analyst".into()],
///     department: None,
///     clearance: Some(Classification::Confidential),
///     jurisdictions: vec![],
/// };
/// let label = SecurityLabel {
///     classification: Classification::Confidential,
///     pii: false,
///     jurisdictions: vec![],
///     purpose_limitation: vec!["analytics".into()],
///     handling_controls: vec![],
/// };
///
/// let decision = evaluate_abac_with_evidence_grade(
///     &actor,
///     &label,
///     AccessPurpose::Analytics,
///     EvidenceGrade::AllowedWithConstraints,
/// );
/// assert!(matches!(decision, AccessDecision::Deny { .. }));
/// ```
pub fn evaluate_abac_with_evidence_grade(
    actor: &ActorContext,
    label: &SecurityLabel,
    purpose: AccessPurpose,
    evidence_grade: EvidenceGrade,
) -> AccessDecision {
    let base = evaluate_abac(actor, label, purpose);
    if !base.is_allowed() {
        return base;
    }

    match evidence_grade {
        EvidenceGrade::None | EvidenceGrade::Prohibited => base,
        EvidenceGrade::AllowedWithConstraints => {
            if purpose == AccessPurpose::Analytics && !has_evidence_steward_privilege(actor) {
                AccessDecision::Deny {
                    reason: "Evidence grade requires steward/compliance privilege for analytics"
                        .into(),
                }
            } else {
                base
            }
        }
        EvidenceGrade::RegulatoryEvidence => {
            if matches!(
                purpose,
                AccessPurpose::Analytics | AccessPurpose::Administration
            ) && !has_evidence_steward_privilege(actor)
            {
                AccessDecision::Deny {
                    reason:
                        "Regulatory evidence requires steward/compliance privilege for this purpose"
                            .into(),
                }
            } else {
                base
            }
        }
    }
}

fn has_evidence_steward_privilege(actor: &ActorContext) -> bool {
    actor.roles.iter().any(|role| {
        let role = role.to_lowercase();
        role.contains("steward")
            || role == "compliance_officer"
            || role == "compliance-manager"
            || role == "regulatory_officer"
    })
}

/// Check if an actor's clearance is sufficient for an object's classification.
fn clearance_sufficient(actor: &Classification, object: &Classification) -> bool {
    classification_level(actor) >= classification_level(object)
}

/// Map classification to a numeric level for comparison.
fn classification_level(c: &Classification) -> u8 {
    match c {
        Classification::Public => 0,
        Classification::Internal => 1,
        Classification::Confidential => 2,
        Classification::Restricted => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public_label() -> SecurityLabel {
        SecurityLabel {
            classification: Classification::Public,
            pii: false,
            jurisdictions: vec![],
            purpose_limitation: vec![],
            handling_controls: vec![],
        }
    }

    fn confidential_pii_label() -> SecurityLabel {
        SecurityLabel {
            classification: Classification::Confidential,
            pii: true,
            jurisdictions: vec!["LU".into(), "DE".into()],
            purpose_limitation: vec!["operations".into(), "audit".into()],
            handling_controls: vec![],
        }
    }

    fn analyst() -> ActorContext {
        ActorContext {
            actor_id: "user-1".into(),
            roles: vec!["analyst".into()],
            department: Some("compliance".into()),
            clearance: Some(Classification::Confidential),
            jurisdictions: vec!["LU".into(), "DE".into(), "IE".into()],
        }
    }

    fn steward() -> ActorContext {
        ActorContext {
            actor_id: "user-3".into(),
            roles: vec!["data_steward".into()],
            department: Some("governance".into()),
            clearance: Some(Classification::Restricted),
            jurisdictions: vec!["LU".into(), "DE".into(), "IE".into()],
        }
    }

    fn uncleared_actor() -> ActorContext {
        ActorContext {
            actor_id: "user-2".into(),
            roles: vec!["viewer".into()],
            department: None,
            clearance: None,
            jurisdictions: vec![],
        }
    }

    #[test]
    fn test_public_access_allowed() {
        let result = evaluate_abac(
            &uncleared_actor(),
            &public_label(),
            AccessPurpose::Operations,
        );
        assert_eq!(result, AccessDecision::Allow);
    }

    #[test]
    fn test_confidential_denied_without_clearance() {
        let result = evaluate_abac(
            &uncleared_actor(),
            &confidential_pii_label(),
            AccessPurpose::Operations,
        );
        assert!(matches!(result, AccessDecision::Deny { .. }));
    }

    #[test]
    fn test_confidential_allowed_with_clearance() {
        let result = evaluate_abac(
            &analyst(),
            &confidential_pii_label(),
            AccessPurpose::Operations,
        );
        assert_eq!(result, AccessDecision::Allow);
    }

    #[test]
    fn test_pii_masked_for_analytics() {
        let mut label = confidential_pii_label();
        label.purpose_limitation = vec!["operations".into(), "analytics".into()];
        let result = evaluate_abac(&analyst(), &label, AccessPurpose::Analytics);
        assert!(matches!(result, AccessDecision::AllowWithMasking { .. }));
    }

    #[test]
    fn test_purpose_limitation_denied() {
        let result = evaluate_abac(
            &analyst(),
            &confidential_pii_label(),
            AccessPurpose::Analytics,
        );
        assert!(matches!(result, AccessDecision::Deny { reason } if reason.contains("purpose")));
    }

    #[test]
    fn test_jurisdiction_denied() {
        let mut actor = analyst();
        actor.jurisdictions = vec!["US".into()];
        let result = evaluate_abac(&actor, &confidential_pii_label(), AccessPurpose::Operations);
        assert!(
            matches!(result, AccessDecision::Deny { reason } if reason.contains("jurisdiction"))
        );
    }

    #[test]
    fn test_insufficient_clearance() {
        let mut actor = analyst();
        actor.clearance = Some(Classification::Internal);
        let result = evaluate_abac(&actor, &confidential_pii_label(), AccessPurpose::Operations);
        assert!(matches!(result, AccessDecision::Deny { reason } if reason.contains("clearance")));
    }

    #[test]
    fn test_access_decision_is_allowed() {
        assert!(AccessDecision::Allow.is_allowed());
        assert!(AccessDecision::AllowWithMasking {
            masked_fields: vec![]
        }
        .is_allowed());
        assert!(!AccessDecision::Deny {
            reason: "test".into()
        }
        .is_allowed());
    }

    #[test]
    fn none_grade_no_escalation() {
        let mut label = confidential_pii_label();
        label.purpose_limitation = vec!["analytics".into()];
        let result = evaluate_abac_with_evidence_grade(
            &analyst(),
            &label,
            AccessPurpose::Analytics,
            EvidenceGrade::None,
        );
        assert!(matches!(result, AccessDecision::AllowWithMasking { .. }));
    }

    #[test]
    fn prohibited_grade_no_escalation() {
        let result = evaluate_abac_with_evidence_grade(
            &analyst(),
            &confidential_pii_label(),
            AccessPurpose::Operations,
            EvidenceGrade::Prohibited,
        );
        assert_eq!(result, AccessDecision::Allow);
    }

    #[test]
    fn allowed_with_constraints_blocks_analytics_for_analyst() {
        let mut label = confidential_pii_label();
        label.purpose_limitation = vec!["analytics".into()];
        let result = evaluate_abac_with_evidence_grade(
            &analyst(),
            &label,
            AccessPurpose::Analytics,
            EvidenceGrade::AllowedWithConstraints,
        );
        assert!(matches!(result, AccessDecision::Deny { .. }));
    }

    #[test]
    fn allowed_with_constraints_permits_steward_analytics() {
        let mut label = confidential_pii_label();
        label.purpose_limitation = vec!["analytics".into()];
        let result = evaluate_abac_with_evidence_grade(
            &steward(),
            &label,
            AccessPurpose::Analytics,
            EvidenceGrade::AllowedWithConstraints,
        );
        assert!(matches!(result, AccessDecision::AllowWithMasking { .. }));
    }

    #[test]
    fn regulatory_evidence_blocks_admin_for_analyst() {
        let mut label = confidential_pii_label();
        label.purpose_limitation = vec!["administration".into()];
        let result = evaluate_abac_with_evidence_grade(
            &analyst(),
            &label,
            AccessPurpose::Administration,
            EvidenceGrade::RegulatoryEvidence,
        );
        assert!(matches!(result, AccessDecision::Deny { .. }));
    }

    #[test]
    fn regulatory_evidence_permits_steward_admin() {
        let mut label = confidential_pii_label();
        label.purpose_limitation = vec!["administration".into()];
        let result = evaluate_abac_with_evidence_grade(
            &steward(),
            &label,
            AccessPurpose::Administration,
            EvidenceGrade::RegulatoryEvidence,
        );
        assert_eq!(result, AccessDecision::Allow);
    }
}
