//! Entity type definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

/// Body of an `entity_type_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTypeDefBody {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    #[serde(default)]
    pub db_table: Option<DbTableMapping>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lifecycle_states: Vec<LifecycleStateDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_attributes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub optional_attributes: Vec<String>,
    #[serde(default)]
    pub parent_type: Option<String>,
    /// Governance tier from domain metadata (governed/operational).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_tier: Option<String>,
    /// Security classification from domain metadata (public/internal/confidential/restricted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_classification: Option<String>,
    /// Whether this entity contains PII data.
    #[serde(default)]
    pub pii: Option<bool>,
    /// Verb FQNs that read from this entity's table (reverse index from verb_data_footprint).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_by_verbs: Vec<String>,
    /// Verb FQNs that write to this entity's table (reverse index from verb_data_footprint).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub written_by_verbs: Vec<String>,
}

/// Database table mapping for entity storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTableMapping {
    pub schema: String,
    pub table: String,
    pub primary_key: String,
    #[serde(default)]
    pub name_column: Option<String>,
}

/// A state in an entity's lifecycle state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleStateDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<LifecycleTransition>,
    #[serde(default)]
    pub terminal: bool,
}

/// A valid state transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleTransition {
    pub to: String,
    #[serde(default)]
    pub trigger_verb: Option<String>,
    #[serde(default)]
    pub guard: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = EntityTypeDefBody {
            fqn: "cbu".into(),
            name: "Client Business Unit".into(),
            description: "Atomic trading unit".into(),
            domain: "cbu".into(),
            db_table: Some(DbTableMapping {
                schema: "ob-poc".into(),
                table: "cbus".into(),
                primary_key: "cbu_id".into(),
                name_column: Some("name".into()),
            }),
            lifecycle_states: vec![
                LifecycleStateDef {
                    name: "draft".into(),
                    description: Some("Initial state".into()),
                    transitions: vec![LifecycleTransition {
                        to: "active".into(),
                        trigger_verb: Some("cbu.activate".into()),
                        guard: Some("has_depositary".into()),
                    }],
                    terminal: false,
                },
                LifecycleStateDef {
                    name: "active".into(),
                    description: None,
                    transitions: vec![],
                    terminal: true,
                },
            ],
            required_attributes: vec!["cbu.name".into(), "cbu.jurisdiction_code".into()],
            optional_attributes: vec!["cbu.client_label".into()],
            parent_type: None,
            governance_tier: Some("governed".into()),
            security_classification: Some("internal".into()),
            pii: Some(false),
            read_by_verbs: vec!["cbu.list".into(), "cbu.read".into()],
            written_by_verbs: vec!["cbu.create".into()],
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: EntityTypeDefBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }
}
