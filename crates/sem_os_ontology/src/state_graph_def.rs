//! State graph definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

/// Body of a `state_graph` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateGraphDefBody {
    pub fqn: String,
    pub graph_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub entity_types: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub gates: Vec<GraphGate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphNode {
    pub node_id: String,
    pub name: String,
    pub node_type: NodeType,
    pub lane: String,
    #[serde(default)]
    pub satisfied_when: Vec<SignalCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Entry,
    Milestone,
    Gate,
    Terminal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub edge_id: String,
    pub from_node: String,
    pub to_node: String,
    pub verb_ids: Vec<String>,
    pub edge_type: EdgeType,
    #[serde(default)]
    pub condition: Vec<SignalCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    Advance,
    Revert,
    Conditional,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphGate {
    pub gate_node: String,
    pub required_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalCondition {
    pub signal: String,
    pub description: String,
}
