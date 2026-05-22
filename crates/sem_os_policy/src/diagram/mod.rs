//! Diagram model + renderers — pure value types for schema-driven diagram generation.
//!
//! Combines physical schema metadata (tables, columns, FKs) with AffinityGraph
//! intelligence (verb surface, entity types, governance) to produce enriched
//! diagram models and Mermaid syntax output.

pub mod enrichment;
pub mod mermaid;
pub mod model;

pub use enrichment::build_diagram_model;
pub use mermaid::{
    render_discovery_map, render_domain_map, render_erd, render_verb_flow, sanitize_id,
};
pub use model::{
    ColumnInput, DiagramAttribute, DiagramEntity, DiagramMetadata, DiagramModel,
    DiagramRelationship, ForeignKeyInput, GovernanceLevel, RelationshipKind, RenderOptions,
    TableInput, VerbSurfaceEntry,
};
