//! Security label inheritance for the semantic registry.
//!
//! Pure functions — no database dependency. Computes inherited labels
//! when outputs are derived from multiple inputs (e.g., derivation specs,
//! verb side-effects).
//!
//! Default policy: **most restrictive wins**.
//! - Classification: highest ordinal
//! - PII: true if ANY input has PII
//! - Jurisdictions: union
//! - Purpose limitation: intersection (empty = no restriction)
//! - Handling controls: union

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use sem_os_types::{Classification, HandlingControl, SecurityLabel};

// ── Supporting types ──────────────────────────────────────────

/// Override declaration for less-restrictive labels on derived outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SecurityLabelOverride {
    /// Human-readable justification for the override.
    pub(crate) rationale: String,
    /// Whether a data steward has approved this override.
    pub(crate) steward_approved: bool,
    /// Which fields are being overridden (for audit trail).
    pub(crate) override_fields: Vec<OverrideField>,
}

/// Which aspect of the label is being overridden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OverrideField {
    Classification,
    Pii,
    Jurisdictions,
    PurposeLimitation,
    HandlingControls,
}

/// Errors from security inheritance computation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SecurityInheritanceError {
    /// Override not approved by steward.
    StewardApprovalRequired {
        field: OverrideField,
        message: String,
    },
    /// PII → non-PII override without justification is forbidden.
    PiiDowngradeRequiresJustification,
    /// Cannot override to a lower classification without steward approval.
    ClassificationDowngradeUnapproved {
        inherited: Classification,
        requested: Classification,
    },
    /// Empty rationale on override.
    EmptyRationale,
}

impl std::fmt::Display for SecurityInheritanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StewardApprovalRequired { field, message } => {
                write!(f, "Steward approval required for {:?}: {}", field, message)
            }
            Self::PiiDowngradeRequiresJustification => {
                write!(
                    f,
                    "Cannot downgrade PII to non-PII without steward justification"
                )
            }
            Self::ClassificationDowngradeUnapproved {
                inherited,
                requested,
            } => {
                write!(
                    f,
                    "Classification downgrade from {:?} to {:?} requires steward approval",
                    inherited, requested
                )
            }
            Self::EmptyRationale => {
                write!(f, "Override rationale must not be empty")
            }
        }
    }
}

impl std::error::Error for SecurityInheritanceError {}

/// Warning from verb-security compatibility checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecurityWarning {
    pub(crate) kind: SecurityWarningKind,
    pub(crate) message: String,
    pub(crate) severity: WarningSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SecurityWarningKind {
    /// Verb purpose doesn't match output purpose — potential label laundering.
    PurposeMismatch,
    /// Output has lower classification than verb context.
    ClassificationDowngrade,
    /// Handling controls present on verb but absent on output.
    MissingHandlingControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WarningSeverity {
    Low,
    Medium,
    High,
}

// ── Core functions ────────────────────────────────────────────

/// Compute the inherited security label from multiple inputs.
///
/// Policy: most restrictive wins.
/// - Classification: highest ordinal
/// - PII: true if any input has PII
/// - Jurisdictions: union of all
/// - Purpose limitation: intersection (empty input = no restriction, so skip)
/// - Handling controls: union of all
///
/// If `inputs` is empty, returns `default_label()`.
pub(crate) fn compute_inherited_label(inputs: &[SecurityLabel]) -> SecurityLabel {
    if inputs.is_empty() {
        return default_label();
    }

    let classification = inputs
        .iter()
        .map(|l| l.classification)
        .max_by_key(classification_level)
        .unwrap_or_default();

    let pii = inputs.iter().any(|l| l.pii);

    // Union of jurisdictions (deduplicated)
    let mut jurisdictions: Vec<String> = inputs
        .iter()
        .flat_map(|l| l.jurisdictions.iter().cloned())
        .collect();
    jurisdictions.sort();
    jurisdictions.dedup();

    // Intersection of purpose limitations.
    // Empty purpose_limitation means "no restriction" — skip those inputs.
    let restricted_inputs: Vec<&Vec<String>> = inputs
        .iter()
        .map(|l| &l.purpose_limitation)
        .filter(|p| !p.is_empty())
        .collect();

    let purpose_limitation = if restricted_inputs.is_empty() {
        vec![] // No restriction from any input
    } else {
        // Start with first restricted set, intersect with the rest
        let mut result = restricted_inputs[0].clone();
        for other in &restricted_inputs[1..] {
            result.retain(|p| other.contains(p));
        }
        result.sort();
        result
    };

    // Union of handling controls (deduplicated)
    let mut handling_controls: Vec<HandlingControl> = inputs
        .iter()
        .flat_map(|l| l.handling_controls.iter().cloned())
        .collect();
    handling_controls.sort_by_key(handling_control_ordinal);
    handling_controls.dedup_by_key(|c| handling_control_ordinal(c));

    SecurityLabel {
        classification,
        pii,
        jurisdictions,
        purpose_limitation,
        handling_controls,
    }
}

/// Compute inherited label with a declared override for less-restrictive output.
///
/// Returns error if override violates hard constraints:
/// - Empty rationale is always rejected
/// - Steward approval is required for classification downgrade or PII removal
pub(crate) fn compute_inherited_label_with_override(
    inputs: &[SecurityLabel],
    declared_override: &SecurityLabelOverride,
) -> Result<SecurityLabel, SecurityInheritanceError> {
    if declared_override.rationale.trim().is_empty() {
        return Err(SecurityInheritanceError::EmptyRationale);
    }

    let inherited = compute_inherited_label(inputs);
    let mut result = inherited.clone();

    for field in &declared_override.override_fields {
        match field {
            OverrideField::Classification => {
                if !declared_override.steward_approved {
                    return Err(
                        SecurityInheritanceError::ClassificationDowngradeUnapproved {
                            inherited: inherited.classification,
                            requested: inherited.classification,
                        },
                    );
                }
            }
            OverrideField::Pii => {
                if inherited.pii && !declared_override.steward_approved {
                    return Err(SecurityInheritanceError::PiiDowngradeRequiresJustification);
                }
                if declared_override.steward_approved {
                    result.pii = false;
                }
            }
            OverrideField::Jurisdictions => {
                if !declared_override.steward_approved {
                    return Err(SecurityInheritanceError::StewardApprovalRequired {
                        field: OverrideField::Jurisdictions,
                        message: "Jurisdiction override requires steward approval".into(),
                    });
                }
            }
            OverrideField::PurposeLimitation => {
                if !declared_override.steward_approved {
                    return Err(SecurityInheritanceError::StewardApprovalRequired {
                        field: OverrideField::PurposeLimitation,
                        message: "Purpose limitation override requires steward approval".into(),
                    });
                }
            }
            OverrideField::HandlingControls => {
                if !declared_override.steward_approved {
                    return Err(SecurityInheritanceError::StewardApprovalRequired {
                        field: OverrideField::HandlingControls,
                        message: "Handling controls override requires steward approval".into(),
                    });
                }
            }
        }
    }

    Ok(result)
}

/// Validate that a verb's security label is compatible with its output label.
///
/// Prevents "label laundering" — a verb under purpose P producing output
/// for a different purpose without explicit authorization.
pub(crate) fn validate_verb_security_compatibility(
    verb_label: &SecurityLabel,
    output_label: &SecurityLabel,
) -> Vec<SecurityWarning> {
    let mut warnings = Vec::new();

    // 1. Purpose mismatch
    if !verb_label.purpose_limitation.is_empty() && !output_label.purpose_limitation.is_empty() {
        let extra_purposes: Vec<&String> = output_label
            .purpose_limitation
            .iter()
            .filter(|p| !verb_label.purpose_limitation.contains(p))
            .collect();
        if !extra_purposes.is_empty() {
            warnings.push(SecurityWarning {
                kind: SecurityWarningKind::PurposeMismatch,
                message: format!(
                    "Output declares purposes {:?} not present in verb's purpose limitation",
                    extra_purposes
                ),
                severity: WarningSeverity::High,
            });
        }
    }

    // 2. Classification downgrade
    if classification_level(&output_label.classification)
        < classification_level(&verb_label.classification)
    {
        warnings.push(SecurityWarning {
            kind: SecurityWarningKind::ClassificationDowngrade,
            message: format!(
                "Output classification {:?} is lower than verb classification {:?}",
                output_label.classification, verb_label.classification
            ),
            severity: WarningSeverity::Medium,
        });
    }

    // 3. Missing handling controls
    for control in &verb_label.handling_controls {
        if !output_label.handling_controls.contains(control) {
            warnings.push(SecurityWarning {
                kind: SecurityWarningKind::MissingHandlingControl,
                message: format!(
                    "Verb requires handling control {:?} but output lacks it",
                    control
                ),
                severity: WarningSeverity::Medium,
            });
        }
    }

    warnings
}

/// Default security label: Internal classification, no PII, no restrictions.
pub(crate) fn default_label() -> SecurityLabel {
    SecurityLabel::default()
}

/// Named label templates for common security postures.
pub(crate) fn label_template(name: &str) -> Option<SecurityLabel> {
    match name {
        "standard_pii_eu" => Some(SecurityLabel {
            classification: Classification::Confidential,
            pii: true,
            jurisdictions: vec!["EU".into()],
            purpose_limitation: vec!["operations".into(), "audit".into()],
            handling_controls: vec![HandlingControl::MaskByDefault],
        }),
        "standard_financial_global" => Some(SecurityLabel {
            classification: Classification::Confidential,
            pii: false,
            jurisdictions: vec![],
            purpose_limitation: vec![],
            handling_controls: vec![HandlingControl::NoLlmExternal],
        }),
        "sanctions_restricted" => Some(SecurityLabel {
            classification: Classification::Restricted,
            pii: true,
            jurisdictions: vec![],
            purpose_limitation: vec!["operations".into()],
            handling_controls: vec![
                HandlingControl::NoExport,
                HandlingControl::DualControl,
                HandlingControl::NoLlmExternal,
            ],
        }),
        "operational_internal" => Some(SecurityLabel {
            classification: Classification::Internal,
            pii: false,
            jurisdictions: vec![],
            purpose_limitation: vec![],
            handling_controls: vec![],
        }),
        _ => None,
    }
}

// ── Helpers ───────────────────────────────────────────────────

/// Numeric level for classification comparison (higher = more restrictive).
fn classification_level(c: &Classification) -> u8 {
    match c {
        Classification::Public => 0,
        Classification::Internal => 1,
        Classification::Confidential => 2,
        Classification::Restricted => 3,
    }
}

/// Ordinal for dedup of handling controls.
fn handling_control_ordinal(c: &HandlingControl) -> u8 {
    match c {
        HandlingControl::MaskByDefault => 0,
        HandlingControl::NoExport => 1,
        HandlingControl::DualControl => 2,
        HandlingControl::SecureViewerOnly => 3,
        HandlingControl::NoLlmExternal => 4,
    }
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn internal_label() -> SecurityLabel {
        SecurityLabel {
            classification: Classification::Internal,
            pii: false,
            jurisdictions: vec!["LU".into()],
            purpose_limitation: vec![],
            handling_controls: vec![],
        }
    }

    fn confidential_pii_label() -> SecurityLabel {
        SecurityLabel {
            classification: Classification::Confidential,
            pii: true,
            jurisdictions: vec!["DE".into()],
            purpose_limitation: vec!["operations".into(), "audit".into()],
            handling_controls: vec![HandlingControl::MaskByDefault],
        }
    }

    fn restricted_label() -> SecurityLabel {
        SecurityLabel {
            classification: Classification::Restricted,
            pii: true,
            jurisdictions: vec!["US".into()],
            purpose_limitation: vec!["operations".into()],
            handling_controls: vec![HandlingControl::NoExport, HandlingControl::DualControl],
        }
    }

    // ── compute_inherited_label tests ─────────────────────────

    #[test]
    fn test_empty_inputs_returns_default() {
        let result = compute_inherited_label(&[]);
        assert_eq!(result.classification, Classification::Internal);
        assert!(!result.pii);
        assert!(result.jurisdictions.is_empty());
        assert!(result.purpose_limitation.is_empty());
        assert!(result.handling_controls.is_empty());
    }

    #[test]
    fn test_single_input_passthrough() {
        let input = confidential_pii_label();
        let result = compute_inherited_label(std::slice::from_ref(&input));
        assert_eq!(result.classification, Classification::Confidential);
        assert!(result.pii);
        assert_eq!(result.jurisdictions, vec!["DE"]);
        assert_eq!(result.purpose_limitation, vec!["audit", "operations"]);
        assert_eq!(
            result.handling_controls,
            vec![HandlingControl::MaskByDefault]
        );
    }

    #[test]
    fn test_classification_takes_highest() {
        let result = compute_inherited_label(&[
            internal_label(),
            confidential_pii_label(),
            restricted_label(),
        ]);
        assert_eq!(result.classification, Classification::Restricted);
    }

    #[test]
    fn test_pii_true_if_any_input() {
        let result = compute_inherited_label(&[internal_label(), confidential_pii_label()]);
        assert!(result.pii);
    }

    #[test]
    fn test_pii_false_if_none() {
        let mut a = internal_label();
        a.pii = false;
        let mut b = internal_label();
        b.jurisdictions = vec!["IE".into()];
        b.pii = false;
        let result = compute_inherited_label(&[a, b]);
        assert!(!result.pii);
    }

    #[test]
    fn test_jurisdictions_union_deduplicated() {
        let mut a = internal_label();
        a.jurisdictions = vec!["LU".into(), "DE".into()];
        let mut b = internal_label();
        b.jurisdictions = vec!["DE".into(), "US".into()];
        let result = compute_inherited_label(&[a, b]);
        assert_eq!(result.jurisdictions, vec!["DE", "LU", "US"]);
    }

    #[test]
    fn test_purpose_limitation_intersection() {
        let result = compute_inherited_label(&[confidential_pii_label(), restricted_label()]);
        assert_eq!(result.purpose_limitation, vec!["operations"]);
    }

    #[test]
    fn test_purpose_limitation_empty_means_no_restriction() {
        let result = compute_inherited_label(&[internal_label(), confidential_pii_label()]);
        assert_eq!(result.purpose_limitation, vec!["audit", "operations"]);
    }

    #[test]
    fn test_handling_controls_union() {
        let result = compute_inherited_label(&[confidential_pii_label(), restricted_label()]);
        assert!(result
            .handling_controls
            .contains(&HandlingControl::MaskByDefault));
        assert!(result
            .handling_controls
            .contains(&HandlingControl::NoExport));
        assert!(result
            .handling_controls
            .contains(&HandlingControl::DualControl));
    }

    // ── compute_inherited_label_with_override tests ───────────

    #[test]
    fn test_override_empty_rationale_rejected() {
        let override_decl = SecurityLabelOverride {
            rationale: "".into(),
            steward_approved: true,
            override_fields: vec![OverrideField::Classification],
        };
        let result =
            compute_inherited_label_with_override(&[confidential_pii_label()], &override_decl);
        assert!(matches!(
            result,
            Err(SecurityInheritanceError::EmptyRationale)
        ));
    }

    #[test]
    fn test_override_classification_without_approval_rejected() {
        let override_decl = SecurityLabelOverride {
            rationale: "Business need".into(),
            steward_approved: false,
            override_fields: vec![OverrideField::Classification],
        };
        let result =
            compute_inherited_label_with_override(&[confidential_pii_label()], &override_decl);
        assert!(matches!(
            result,
            Err(SecurityInheritanceError::ClassificationDowngradeUnapproved { .. })
        ));
    }

    #[test]
    fn test_override_pii_without_approval_rejected() {
        let override_decl = SecurityLabelOverride {
            rationale: "Aggregated output".into(),
            steward_approved: false,
            override_fields: vec![OverrideField::Pii],
        };
        let result =
            compute_inherited_label_with_override(&[confidential_pii_label()], &override_decl);
        assert!(matches!(
            result,
            Err(SecurityInheritanceError::PiiDowngradeRequiresJustification)
        ));
    }

    #[test]
    fn test_override_pii_with_approval_succeeds() {
        let override_decl = SecurityLabelOverride {
            rationale: "Aggregated output, no individual data".into(),
            steward_approved: true,
            override_fields: vec![OverrideField::Pii],
        };
        let result =
            compute_inherited_label_with_override(&[confidential_pii_label()], &override_decl);
        let label = result.unwrap();
        assert!(!label.pii);
        assert_eq!(label.classification, Classification::Confidential);
    }

    // ── validate_verb_security_compatibility tests ────────────

    #[test]
    fn test_compatibility_no_warnings_when_matching() {
        let verb = confidential_pii_label();
        let output = confidential_pii_label();
        let warnings = validate_verb_security_compatibility(&verb, &output);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_compatibility_purpose_mismatch_detected() {
        let verb = SecurityLabel {
            purpose_limitation: vec!["operations".into()],
            ..confidential_pii_label()
        };
        let output = SecurityLabel {
            purpose_limitation: vec!["operations".into(), "analytics".into()],
            ..confidential_pii_label()
        };
        let warnings = validate_verb_security_compatibility(&verb, &output);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, SecurityWarningKind::PurposeMismatch);
        assert_eq!(warnings[0].severity, WarningSeverity::High);
    }

    #[test]
    fn test_compatibility_classification_downgrade_detected() {
        let verb = SecurityLabel {
            classification: Classification::Confidential,
            ..default_label()
        };
        let output = SecurityLabel {
            classification: Classification::Internal,
            ..default_label()
        };
        let warnings = validate_verb_security_compatibility(&verb, &output);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].kind,
            SecurityWarningKind::ClassificationDowngrade
        );
    }

    #[test]
    fn test_compatibility_missing_handling_control_detected() {
        let verb = SecurityLabel {
            handling_controls: vec![HandlingControl::NoExport, HandlingControl::DualControl],
            ..default_label()
        };
        let output = SecurityLabel {
            handling_controls: vec![HandlingControl::NoExport],
            ..default_label()
        };
        let warnings = validate_verb_security_compatibility(&verb, &output);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].kind,
            SecurityWarningKind::MissingHandlingControl
        );
    }

    // ── label_template tests ─────────────────────────────────

    #[test]
    fn test_template_standard_pii_eu() {
        let label = label_template("standard_pii_eu").unwrap();
        assert_eq!(label.classification, Classification::Confidential);
        assert!(label.pii);
        assert_eq!(label.jurisdictions, vec!["EU"]);
    }

    #[test]
    fn test_template_sanctions_restricted() {
        let label = label_template("sanctions_restricted").unwrap();
        assert_eq!(label.classification, Classification::Restricted);
        assert!(label.handling_controls.contains(&HandlingControl::NoExport));
        assert!(label
            .handling_controls
            .contains(&HandlingControl::DualControl));
    }

    #[test]
    fn test_template_unknown_returns_none() {
        assert!(label_template("nonexistent").is_none());
    }

    // ── default_label tests ──────────────────────────────────

    #[test]
    fn test_default_label() {
        let label = default_label();
        assert_eq!(label.classification, Classification::Internal);
        assert!(!label.pii);
        assert!(label.jurisdictions.is_empty());
        assert!(label.purpose_limitation.is_empty());
        assert!(label.handling_controls.is_empty());
    }
}
