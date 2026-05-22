//! Verb contract body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Body of a `verb_contract` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbContractBody {
    pub fqn: String,
    pub domain: String,
    pub action: String,
    pub description: String,
    pub behavior: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<VerbArgDef>,
    #[serde(default)]
    pub returns: Option<VerbReturnSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preconditions: Vec<VerbPrecondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub postconditions: Vec<String>,
    #[serde(default)]
    pub produces: Option<VerbProducesSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invocation_phrases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject_kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phase_tags: Vec<String>,
    /// Safety tier for routing and confirmation policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harm_class: Option<HarmClass>,
    /// Normalized action family for deterministic intent routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_class: Option<ActionClass>,
    /// Required lifecycle states extracted from verb lifecycle metadata.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub precondition_states: Vec<String>,
    #[serde(default = "default_true")]
    pub requires_subject: bool,
    #[serde(default)]
    pub produces_focus: bool,
    #[serde(default)]
    pub metadata: Option<VerbContractMetadata>,
    /// CRUD table/schema/operation mapping (when behavior = "crud").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crud_mapping: Option<VerbCrudMapping>,
    /// Tables this verb reads from (populated from domain metadata verb_data_footprint).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads_from: Vec<String>,
    /// Tables this verb writes to (populated from domain metadata verb_data_footprint).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes_to: Vec<String>,
    /// Typed output declarations for forward-reference resolution in runbook plans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<VerbOutput>,
    /// Shared atom paths this verb produces/mutates (cross-workspace consistency).
    /// When non-empty, successful execution triggers shared fact version recording.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produces_shared_facts: Vec<String>,
}

/// Safety tier for routing and confirmation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarmClass {
    ReadOnly,
    Reversible,
    Irreversible,
    Destructive,
}

/// Normalized action family for deterministic intent routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    List,
    Read,
    Search,
    Describe,
    Create,
    Update,
    Delete,
    Assign,
    Remove,
    Import,
    Compute,
    Review,
    Approve,
    Reject,
    Execute,
}

/// CRUD table/operation mapping captured from verb YAML.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerbCrudMapping {
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_column: Option<String>,
    /// Column name for RETURNING clause (INSERT/UPSERT).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returning: Option<String>,
    /// Columns for ON CONFLICT (UPSERT).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflict_keys: Vec<String>,
    /// Named constraint for ON CONFLICT (when conflict_keys has computed columns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_constraint: Option<String>,
    /// Junction table name (LINK/UNLINK operations).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub junction: Option<String>,
    /// Source column in junction table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_col: Option<String>,
    /// Target column in junction table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_col: Option<String>,
    /// Role table (ROLE_LINK/ROLE_UNLINK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_table: Option<String>,
    /// Role column name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_col: Option<String>,
    /// Foreign key column (LIST_BY_FK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fk_col: Option<String>,
    /// Filter column (LIST_BY_FK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_col: Option<String>,
    /// Primary table for join queries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_table: Option<String>,
    /// Join table for SELECT_WITH_JOIN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_table: Option<String>,
    /// Join column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_col: Option<String>,
}

/// Definition of a verb argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbArgDef {
    pub name: String,
    pub arg_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub lookup: Option<VerbArgLookup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_values: Option<Vec<String>>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Database column this argument maps to (for CRUD operations).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maps_to: Option<String>,
}

/// Entity lookup configuration for a verb argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbArgLookup {
    pub table: String,
    pub entity_type: String,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub search_key: Option<String>,
    #[serde(default)]
    pub primary_key: Option<String>,
}

/// Return type specification for a verb.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbReturnSpec {
    #[serde(rename = "type")]
    pub return_type: String,
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
}

/// A precondition that must be met before execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbPrecondition {
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// What a verb produces on success.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbProducesSpec {
    #[serde(rename = "type")]
    pub entity_type: String,
    #[serde(default)]
    pub resolved: bool,
}

/// Typed output declaration for forward-reference resolution in multi-workspace plans.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerbOutput {
    /// Output field name (e.g. "created_cbu_id").
    pub field_name: String,
    /// Output type — "uuid", "record", etc.
    pub output_type: String,
    /// Entity kind this output refers to (e.g. "cbu", "entity").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<String>,
    /// Human description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Optional metadata attached to a verb contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbContractMetadata {
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub source_of_truth: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub noun: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject_kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phase_tags: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = VerbContractBody {
            fqn: "cbu.create".into(),
            domain: "cbu".into(),
            action: "create".into(),
            description: "Create a CBU".into(),
            behavior: "plugin".into(),
            args: vec![VerbArgDef {
                name: "name".into(),
                arg_type: "string".into(),
                required: true,
                description: Some("CBU name".into()),
                lookup: None,
                valid_values: None,
                default: None,
                maps_to: None,
            }],
            returns: Some(VerbReturnSpec {
                return_type: "uuid".into(),
                schema: None,
            }),
            preconditions: vec![VerbPrecondition {
                kind: "requires_scope".into(),
                value: "cbu".into(),
                description: None,
            }],
            postconditions: vec![],
            produces: Some(VerbProducesSpec {
                entity_type: "cbu".into(),
                resolved: true,
            }),
            consumes: vec![],
            invocation_phrases: vec!["create cbu".into()],
            subject_kinds: vec!["cbu".into()],
            phase_tags: vec![],
            harm_class: Some(HarmClass::Reversible),
            action_class: Some(ActionClass::Create),
            precondition_states: vec![],
            requires_subject: true,
            produces_focus: false,
            metadata: Some(VerbContractMetadata {
                tier: Some("intent".into()),
                source_of_truth: None,
                scope: None,
                noun: None,
                tags: vec![],
                subject_kinds: vec![],
                phase_tags: vec![],
            }),
            crud_mapping: Some(VerbCrudMapping {
                operation: "insert".into(),
                table: Some("cbus".into()),
                schema: Some("ob-poc".into()),
                key_column: None,
                ..Default::default()
            }),
            reads_from: vec![],
            writes_to: vec!["cbus".into()],
            outputs: vec![VerbOutput {
                field_name: "created_cbu_id".into(),
                output_type: "uuid".into(),
                entity_kind: Some("cbu".into()),
                description: Some("ID of the newly created CBU".into()),
            }],
            produces_shared_facts: vec![],
        };
        let json = serde_json::to_value(&val).unwrap();
        // Check #[serde(rename = "type")] on returns and produces
        assert_eq!(json["returns"]["type"], "uuid");
        assert_eq!(json["produces"]["type"], "cbu");
        // Check default_true(): requires_subject defaults to true
        let minimal: VerbContractBody = serde_json::from_str(
            r#"{"fqn":"x","domain":"x","action":"x","description":"x","behavior":"x"}"#,
        )
        .unwrap();
        assert!(minimal.requires_subject);
        assert_eq!(minimal.harm_class, None);
        assert_eq!(minimal.action_class, None);
        assert!(minimal.precondition_states.is_empty());
        // Round-trip
        let back: VerbContractBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }
}
