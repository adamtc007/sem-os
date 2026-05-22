//! View definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Body of a `view_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewDefBody {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    pub base_entity_type: String,
    #[serde(default)]
    pub columns: Vec<ViewColumn>,
    #[serde(default)]
    pub filters: Vec<ViewFilter>,
    #[serde(default)]
    pub sort_order: Vec<ViewSortField>,
    #[serde(default)]
    pub includes_operational: bool,
}

/// A column in a view definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewColumn {
    pub attribute_fqn: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub format: Option<String>,
}

/// A filter applied to a view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewFilter {
    pub attribute_fqn: String,
    pub operator: String,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default = "default_true")]
    pub removable: bool,
}

/// A sort field in a view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewSortField {
    pub attribute_fqn: String,
    #[serde(default)]
    pub direction: SortDirection,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}
