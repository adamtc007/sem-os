//! Discovery universe definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

/// Body of a `universe_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniverseDefBody {
    pub fqn: String,
    pub universe_id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub domains: Vec<UniverseDomain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_entry_domain: Option<String>,
}

/// Discovery-stage domain cluster inside a universe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniverseDomain {
    pub domain_id: String,
    pub label: String,
    pub description: String,
    #[serde(default)]
    pub objective_tags: Vec<String>,
    #[serde(default)]
    pub utterance_signals: Vec<UtteranceSignal>,
    #[serde(default)]
    pub candidate_entity_kinds: Vec<String>,
    #[serde(default)]
    pub candidate_family_ids: Vec<String>,
    #[serde(default)]
    pub required_grounding_inputs: Vec<GroundingInput>,
    #[serde(default)]
    pub entry_questions: Vec<EntryQuestion>,
    #[serde(default)]
    pub allowed_discovery_actions: Vec<String>,
}

/// Weighted utterance cue used to navigate the discovery universe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtteranceSignal {
    pub signal_type: String,
    pub pattern: String,
    pub weight: f64,
}

/// Input Sem OS still needs before grounding can advance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingInput {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    pub input_type: String,
}

/// Human-facing clarification prompt for the discovery stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryQuestion {
    pub question_id: String,
    pub prompt: String,
    pub maps_to: String,
    pub priority: u8,
}
