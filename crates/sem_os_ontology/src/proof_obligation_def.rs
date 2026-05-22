//! Proof obligation definition for the semantic registry.
//!
//! A proof obligation is the governed unit of document evidence demand that
//! runtime requirement computation consumes from a published SemOS snapshot.

use serde::{Deserialize, Serialize};

/// Required proof strength.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProofStrength {
    Primary,
    Secondary,
    Supporting,
}

/// Body of a `proof_obligation_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofObligationDefBody {
    /// Fully qualified name, e.g. `"doc.proof_obligation.identity.primary"`
    pub fqn: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the obligation.
    pub description: String,
    /// Category bucket for rollups such as identity/financial/compliance.
    pub category: String,
    /// Required proof strength.
    pub strength_required: ProofStrength,
    /// Whether the obligation is mandatory.
    #[serde(default)]
    pub is_mandatory: bool,
    /// Freshness requirement in days, if applicable.
    #[serde(default)]
    pub freshness_days: Option<u32>,
    /// Whether legalisation is required.
    #[serde(default)]
    pub legalisation_required: bool,
    /// Whether notarisation is required.
    #[serde(default)]
    pub notarisation_required: bool,
    /// Strategy FQNs that can satisfy this obligation.
    #[serde(default)]
    pub evidence_strategy_fqns: Vec<String>,
    /// Optional condition expressions or labels.
    #[serde(default)]
    pub conditions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_obligation_def_serde() {
        let val = ProofObligationDefBody {
            fqn: "doc.proof_obligation.identity.primary".into(),
            name: "Primary Identity Obligation".into(),
            description: "Requires primary-grade proof of identity".into(),
            category: "identity".into(),
            strength_required: ProofStrength::Primary,
            is_mandatory: true,
            freshness_days: Some(3650),
            legalisation_required: false,
            notarisation_required: false,
            evidence_strategy_fqns: vec!["doc.evidence_strategy.identity.passport".into()],
            conditions: vec!["client_type != omnibus".into()],
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: ProofObligationDefBody = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.strength_required, ProofStrength::Primary);
        assert_eq!(back.evidence_strategy_fqns.len(), 1);
        assert_eq!(json["category"], "identity");
    }

    #[test]
    fn test_proof_obligation_defaults() {
        let val: ProofObligationDefBody = serde_json::from_value(serde_json::json!({
            "fqn": "doc.proof_obligation.test",
            "name": "Test",
            "description": "Test obligation",
            "category": "identity",
            "strength_required": "supporting"
        }))
        .unwrap();
        assert!(!val.is_mandatory);
        assert!(val.freshness_days.is_none());
        assert!(val.evidence_strategy_fqns.is_empty());
        assert!(val.conditions.is_empty());
    }
}
