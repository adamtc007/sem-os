//! State machine definition body types — pure value types, no DB dependency.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Body of a `state_machine` registry snapshot.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateMachineDefBody {
    pub fqn: String,
    pub state_machine: String,
    #[serde(default)]
    pub description: Option<String>,
    pub states: Vec<String>,
    pub initial: String,
    #[serde(default)]
    pub transitions: Vec<TransitionDef>,
    #[serde(default)]
    pub reducer: Option<ReducerDef>,
}

/// Transition definition.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TransitionDef {
    pub from: String,
    pub to: String,
    pub verbs: Vec<String>,
}

/// Reducer section of the state machine.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerDef {
    #[serde(default)]
    pub overlay_sources: HashMap<String, OverlaySourceDef>,
    #[serde(default)]
    pub conditions: HashMap<String, ConditionDef>,
    #[serde(default)]
    pub rules: Vec<RuleDef>,
}

/// Overlay source definition.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverlaySourceDef {
    pub table: String,
    pub join: String,
    pub provides: Vec<String>,
    #[serde(default)]
    pub cardinality: Option<String>,
}

/// Condition definition.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConditionDef {
    pub expr: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parameterized: bool,
}

/// Reducer rule definition.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RuleDef {
    pub state: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub excludes: Vec<String>,
    #[serde(default)]
    pub consistency_check: Option<ConsistencyCheckDef>,
}

/// Consistency warning definition.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConsistencyCheckDef {
    pub warn_unless: String,
    pub warning: String,
}
