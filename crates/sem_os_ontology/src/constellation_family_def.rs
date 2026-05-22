//! Constellation family definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

/// Body of a `constellation_family_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstellationFamilyDefBody {
    pub fqn: String,
    pub family_id: String,
    pub label: String,
    pub description: String,
    pub domain_id: String,
    #[serde(default)]
    pub selection_rules: Vec<SelectionRule>,
    #[serde(default)]
    pub constellation_refs: Vec<ConstellationRef>,
    #[serde(default)]
    pub candidate_jurisdictions: Vec<String>,
    #[serde(default)]
    pub candidate_entity_kinds: Vec<String>,
    pub grounding_threshold: GroundingThreshold,
}

/// A concrete constellation that this family can narrow into.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstellationRef {
    pub constellation_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jurisdiction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
}

/// Deterministic authored narrowing rule from family to constellation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionRule {
    pub condition: String,
    pub target_constellation: String,
    pub priority: u16,
}

/// Inputs required before Sem OS may treat a family as grounded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingThreshold {
    #[serde(default)]
    pub required_input_keys: Vec<String>,
    #[serde(default)]
    pub requires_entity_instance: bool,
    #[serde(default)]
    pub allows_draft_instance: bool,
}
