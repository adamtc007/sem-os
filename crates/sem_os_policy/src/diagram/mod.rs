//! Diagram model + renderers — pure value types for schema-driven diagram generation.
//!
//! Combines physical schema metadata (tables, columns, FKs) with AffinityGraph
//! intelligence (verb surface, entity types, governance) to produce enriched
//! diagram models and Mermaid syntax output.

pub(crate) mod enrichment; // implementation — consumers use re-exported build_diagram_model
pub(crate) mod mermaid;    // implementation — consumers use re-exported render_* fns
pub mod model;             // accessed directly by ob-poc at diagram::model::{ColumnInput, ...}

pub use enrichment::build_diagram_model;
pub use mermaid::{
    render_discovery_map, render_domain_map, render_erd, render_verb_flow, sanitize_id,
};
pub use model::{
    ColumnInput, DiagramAttribute, DiagramEntity, DiagramMetadata, DiagramModel,
    DiagramRelationship, ForeignKeyInput, GovernanceLevel, RelationshipKind, RenderOptions,
    TableInput, VerbSurfaceEntry,
};
