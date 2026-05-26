//! Public output types for taxonomy projection.

use serde::{Deserialize, Serialize};

pub(crate) use sem_os_ontology::membership::MembershipKind;

/// Top-level metadata for a single taxonomy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyMeta {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification_axis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
}

/// A single member of a taxonomy node — a registry object classified here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberSummary {
    pub fqn: String,
    /// Registry object type: `attribute_def`, `verb_contract`, `entity_type_def`, etc.
    pub object_type: String,
    pub kind: MembershipKind,
}

/// A node in the taxonomy tree with its children and direct members.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyTreeNode {
    pub fqn: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub sort_order: i32,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub labels: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<MemberSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TaxonomyTreeNode>,
}

/// A fully resolved taxonomy tree — metadata header + recursive node tree.
///
/// Serializes cleanly to YAML or JSON for UI consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyTree {
    pub meta: TaxonomyMeta,
    /// Nodes without a declared root are gathered under a synthetic root
    /// named after the taxonomy fqn.
    pub root: TaxonomyTreeNode,
    /// Total node count (excluding synthetic root when taxonomy has a declared root).
    pub node_count: usize,
    /// Total member count across all nodes.
    pub member_count: usize,
}

impl TaxonomyTree {
    /// Serialize to YAML.
    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
