use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed body for `ObjectType::ServiceResourceDef`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceResourceDefBody {
    pub srdef_id: String,
    pub code: String,
    pub name: String,
    pub resource_type: String,
    pub purpose: Option<String>,
    pub provisioning_strategy: String,
    pub owner_principal_fqn: String,
    #[serde(default)]
    pub triggered_by_services: Vec<String>,
    #[serde(default)]
    pub attributes: Vec<ServiceResourceAttributeRequirement>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub dimensions: ServiceResourceDimensions,
    #[serde(default)]
    pub binding_policy: Value,
}

/// Attribute requirement slice inside a service-resource definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceResourceAttributeRequirement {
    pub attr_id: String,
    pub requirement: String,
    #[serde(default)]
    pub source_policy: Vec<String>,
    #[serde(default)]
    pub constraints: Value,
    #[serde(default)]
    pub evidence_policy: Value,
    pub default_value: Option<Value>,
    pub condition: Option<String>,
    pub description: Option<String>,
}

/// Instantiation dimensions for a service-resource definition.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceResourceDimensions {
    pub per_market: bool,
    pub per_currency: bool,
    pub per_counterparty: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_resource_def_body_round_trips() {
        let body = ServiceResourceDefBody {
            srdef_id: "SRDEF::CUSTODY::Account::custody_cash".to_string(),
            code: "custody_cash".to_string(),
            name: "Cash Custody Account".to_string(),
            resource_type: "Account".to_string(),
            purpose: Some("Hold cash balances".to_string()),
            provisioning_strategy: "request".to_string(),
            owner_principal_fqn: "resource_owner:CUSTODY".to_string(),
            triggered_by_services: vec!["CASH_MGMT".to_string()],
            attributes: vec![ServiceResourceAttributeRequirement {
                attr_id: "settlement_currency".to_string(),
                requirement: "required".to_string(),
                source_policy: vec!["cbu".to_string()],
                constraints: serde_json::json!({ "type": "string" }),
                evidence_policy: serde_json::json!({}),
                default_value: None,
                condition: None,
                description: None,
            }],
            depends_on: vec![],
            dimensions: ServiceResourceDimensions {
                per_market: false,
                per_currency: true,
                per_counterparty: false,
            },
            binding_policy: serde_json::json!({}),
        };

        let json = serde_json::to_value(&body).unwrap();
        let round: ServiceResourceDefBody = serde_json::from_value(json).unwrap();
        assert_eq!(round, body);
    }
}
