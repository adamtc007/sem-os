//! Evidence strategy definition for the semantic registry.
//!
//! An evidence strategy governs a concrete satisfaction pattern for a proof
//! obligation, such as a single acceptable document, OR alternatives, or a
//! small AND bundle.

use serde::{Deserialize, Serialize};

use crate::proof_obligation_def::ProofStrength;

/// One governed component within an evidence strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceStrategyComponent {
    /// Document type FQN required or referenced by this component.
    pub document_type_fqn: String,
    /// Role in the strategy, e.g. `primary`, `supporting`, `compensating`.
    pub role: String,
    /// Whether the component is required.
    #[serde(default = "default_true")]
    pub required: bool,
    /// Freshness override in days.
    #[serde(default)]
    pub freshness_days: Option<u32>,
    /// Whether this component must be notarised.
    #[serde(default)]
    pub must_be_notarised: bool,
    /// Whether this component must be legalised.
    #[serde(default)]
    pub must_be_legalised: bool,
    /// Attribute FQNs proven by this component.
    #[serde(default)]
    pub attributes_proven: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// Body of an `evidence_strategy_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceStrategyDefBody {
    /// Fully qualified name, e.g. `"doc.evidence_strategy.identity.passport"`
    pub fqn: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the strategy.
    pub description: String,
    /// Owning proof obligation FQN, if directly bound.
    #[serde(default)]
    pub obligation_fqn: Option<String>,
    /// Priority relative to sibling strategies.
    #[serde(default)]
    pub priority: i32,
    /// Achieved proof strength if this strategy is satisfied.
    pub proof_strength: ProofStrength,
    /// Components participating in the strategy.
    #[serde(default)]
    pub components: Vec<EvidenceStrategyComponent>,
    /// Optional extra conditions or labels.
    #[serde(default)]
    pub extra_conditions: Vec<String>,
    /// Optional downgrade note if this strategy is weaker than the preferred path.
    #[serde(default)]
    pub strength_downgrade_note: Option<String>,
    /// Whether this strategy is enabled for consumption.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evidence_strategy_def_serde() {
        let val = EvidenceStrategyDefBody {
            fqn: "doc.evidence_strategy.identity.passport".into(),
            name: "Passport Only".into(),
            description: "Single-document primary identity strategy".into(),
            obligation_fqn: Some("doc.proof_obligation.identity.primary".into()),
            priority: 10,
            proof_strength: ProofStrength::Primary,
            components: vec![EvidenceStrategyComponent {
                document_type_fqn: "doc.passport".into(),
                role: "primary".into(),
                required: true,
                freshness_days: Some(3650),
                must_be_notarised: false,
                must_be_legalised: false,
                attributes_proven: vec!["entity.full-name".into(), "entity.date-of-birth".into()],
            }],
            extra_conditions: vec!["jurisdiction in [GB, IE]".into()],
            strength_downgrade_note: None,
            enabled: true,
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: EvidenceStrategyDefBody = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.proof_strength, ProofStrength::Primary);
        assert_eq!(back.components.len(), 1);
        assert_eq!(json["components"][0]["role"], "primary");
    }

    #[test]
    fn test_evidence_strategy_defaults() {
        let val: EvidenceStrategyDefBody = serde_json::from_value(serde_json::json!({
            "fqn": "doc.evidence_strategy.test",
            "name": "Test",
            "description": "Test strategy",
            "proof_strength": "supporting"
        }))
        .unwrap();
        assert!(val.obligation_fqn.is_none());
        assert_eq!(val.priority, 0);
        assert!(val.components.is_empty());
        assert!(val.extra_conditions.is_empty());
        assert!(val.enabled);
    }
}
