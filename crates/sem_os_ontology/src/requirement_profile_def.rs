//! Requirement profile definition for the semantic registry.
//!
//! A requirement profile governs which proof obligations apply for a
//! document-relevant context such as KYC onboarding, based on entity,
//! jurisdiction, client type, and context scope.

use serde::{Deserialize, Serialize};

/// Body of a `requirement_profile_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementProfileDefBody {
    /// Fully qualified name, e.g. `"doc.requirement_profile.kyc.individual.uk"`
    pub fqn: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the profile.
    pub description: String,
    /// Applicable entity types.
    #[serde(default)]
    pub entity_types: Vec<String>,
    /// Applicable jurisdictions.
    #[serde(default)]
    pub jurisdictions: Vec<String>,
    /// Applicable client types.
    #[serde(default)]
    pub client_types: Vec<String>,
    /// Contexts in which this profile applies.
    #[serde(default)]
    pub contexts: Vec<String>,
    /// Effective-from date in ISO-8601 form, if bounded.
    #[serde(default)]
    pub effective_from: Option<String>,
    /// Effective-to date in ISO-8601 form, if bounded.
    #[serde(default)]
    pub effective_to: Option<String>,
    /// Referenced proof obligation FQNs governed by this profile.
    #[serde(default)]
    pub obligation_fqns: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requirement_profile_def_serde() {
        let val = RequirementProfileDefBody {
            fqn: "doc.requirement_profile.kyc.individual.uk".into(),
            name: "UK Individual KYC Profile".into(),
            description: "Core KYC obligation profile for UK individuals".into(),
            entity_types: vec!["entity.natural-person".into()],
            jurisdictions: vec!["GB".into()],
            client_types: vec!["institutional".into()],
            contexts: vec!["kyc_workstream".into()],
            effective_from: Some("2026-01-01".into()),
            effective_to: None,
            obligation_fqns: vec![
                "doc.proof_obligation.identity.primary".into(),
                "doc.proof_obligation.address.current".into(),
            ],
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: RequirementProfileDefBody = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.fqn, val.fqn);
        assert_eq!(back.obligation_fqns.len(), 2);
        assert_eq!(json["contexts"][0], "kyc_workstream");
    }

    #[test]
    fn test_requirement_profile_defaults() {
        let val: RequirementProfileDefBody = serde_json::from_value(serde_json::json!({
            "fqn": "doc.requirement_profile.test",
            "name": "Test",
            "description": "Test profile"
        }))
        .unwrap();
        assert!(val.entity_types.is_empty());
        assert!(val.jurisdictions.is_empty());
        assert!(val.client_types.is_empty());
        assert!(val.contexts.is_empty());
        assert!(val.obligation_fqns.is_empty());
        assert!(val.effective_from.is_none());
    }
}
