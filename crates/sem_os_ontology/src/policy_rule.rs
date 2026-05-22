//! Policy rule body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

fn default_priority() -> i32 {
    100
}

fn default_true() -> bool {
    true
}

/// Body of a `policy_rule` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRuleBody {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default)]
    pub predicates: Vec<PolicyPredicate>,
    #[serde(default)]
    pub actions: Vec<PolicyAction>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// A predicate that determines when a policy applies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyPredicate {
    pub kind: String,
    pub field: String,
    pub operator: String,
    pub value: serde_json::Value,
}

/// An action taken when a policy's predicates match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAction {
    pub kind: String,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default)]
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = PolicyRuleBody {
            fqn: "policy.sanctions_screen".into(),
            name: "Sanctions Screening".into(),
            description: "Require screening before onboarding".into(),
            domain: "kyc".into(),
            scope: Some("onboarding".into()),
            priority: 50,
            predicates: vec![PolicyPredicate {
                kind: "entity_attribute".into(),
                field: "entity.jurisdiction_code".into(),
                operator: "in".into(),
                value: serde_json::json!(["US", "GB", "EU"]),
            }],
            actions: vec![PolicyAction {
                kind: "require_screening".into(),
                params: serde_json::json!({"provider": "refinitiv"}),
                description: Some("Run sanctions check".into()),
            }],
            enabled: true,
        };
        let json = serde_json::to_value(&val).unwrap();
        // Check default_priority gives 100
        let minimal: PolicyRuleBody =
            serde_json::from_str(r#"{"fqn":"x","name":"x","description":"x","domain":"x"}"#)
                .unwrap();
        assert_eq!(minimal.priority, 100);
        // Check default_true gives true for enabled
        assert!(minimal.enabled);
        // Round-trip
        let back: PolicyRuleBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }
}
