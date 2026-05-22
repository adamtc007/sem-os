//! Diagram model types — pure value types for diagram generation.
//!
//! These types represent the enriched diagram model that combines physical
//! schema information with AffinityGraph intelligence. No DB dependency.

use serde::{Deserialize, Serialize};

// ── Input types (caller provides) ───────────────────────────────

/// Physical table metadata provided by the caller (e.g., from `extract_schema()`).
///
/// This is the pure-value equivalent of the monolith's `TableExtract`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInput {
    /// Schema name (e.g. "ob-poc").
    pub schema: String,
    /// Table name (e.g. "cbus").
    pub table_name: String,
    /// Columns in this table.
    pub columns: Vec<ColumnInput>,
    /// Primary key column names.
    pub primary_keys: Vec<String>,
    /// Foreign key relationships.
    pub foreign_keys: Vec<ForeignKeyInput>,
}

/// Physical column metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInput {
    /// Column name.
    pub name: String,
    /// SQL data type (e.g. "uuid", "text").
    pub sql_type: String,
    /// Whether the column is nullable.
    pub is_nullable: bool,
}

/// Foreign key relationship from physical schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyInput {
    /// Column in the source table.
    pub from_column: String,
    /// Schema of the target table.
    pub target_schema: String,
    /// Target table name.
    pub target_table: String,
    /// Target column name.
    pub target_column: String,
}

// ── Enriched diagram model (output) ─────────────────────────────

/// Top-level enriched diagram model combining physical schema with
/// AffinityGraph intelligence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramModel {
    /// Enriched entities (one per table).
    pub entities: Vec<DiagramEntity>,
    /// Relationships between entities (FK + entity-level).
    pub relationships: Vec<DiagramRelationship>,
    /// Metadata about the diagram.
    pub metadata: DiagramMetadata,
}

/// Metadata about the generated diagram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramMetadata {
    /// Total tables in input (before filtering).
    pub total_tables: usize,
    /// Tables included in the model (after filtering).
    pub included_tables: usize,
    /// Whether any filters were applied.
    pub filtered: bool,
}

/// An enriched entity in the diagram, combining a physical table
/// with its registry entity type and verb surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramEntity {
    /// Schema name.
    pub schema: String,
    /// Table name.
    pub table_name: String,
    /// Matched entity type FQN (if any).
    pub entity_type_fqn: Option<String>,
    /// Columns with enrichment.
    pub attributes: Vec<DiagramAttribute>,
    /// Primary key column names.
    pub primary_keys: Vec<String>,
    /// Verbs that operate on this table (verb surface).
    pub verb_surface: Vec<VerbSurfaceEntry>,
    /// Governance level based on registry coverage.
    pub governance_level: GovernanceLevel,
}

/// An enriched column/attribute in the diagram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramAttribute {
    /// Column name.
    pub name: String,
    /// SQL data type.
    pub sql_type: String,
    /// Whether nullable.
    pub is_nullable: bool,
    /// Whether this is a primary key column.
    pub is_pk: bool,
    /// Whether this is a foreign key column.
    pub is_fk: bool,
    /// Matched attribute FQN from registry (if any).
    pub attribute_fqn: Option<String>,
    /// Verbs that produce this attribute.
    pub producing_verbs: Vec<String>,
    /// Verbs that consume this attribute.
    pub consuming_verbs: Vec<String>,
}

/// A verb that operates on a table in the diagram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbSurfaceEntry {
    /// Verb FQN (e.g. "cbu.create").
    pub verb_fqn: String,
    /// How the verb relates to this table (e.g. "produces", "crud_read").
    pub relation: String,
}

/// A relationship between two entities in the diagram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramRelationship {
    /// Source entity (schema:table).
    pub from_entity: String,
    /// Target entity (schema:table).
    pub to_entity: String,
    /// Relationship type.
    pub kind: RelationshipKind,
    /// Cardinality label (e.g. "1:N", "M:N").
    pub cardinality: Option<String>,
    /// FK column name (for FK relationships).
    pub fk_column: Option<String>,
}

/// How two entities are related in the diagram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    /// Foreign key relationship from physical schema.
    ForeignKey,
    /// Entity-level relationship from RelationshipTypeDef.
    EntityRelationship,
}

/// Governance classification based on registry coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceLevel {
    /// Table has matching entity type AND verb surface.
    Full,
    /// Table has some verb affinity but no entity type (or vice versa).
    Partial,
    /// Table has no registry representation.
    None,
}

// ── Render options ──────────────────────────────────────────────

/// Options controlling diagram rendering.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenderOptions {
    /// Filter to specific schema(s).
    pub schema_filter: Option<Vec<String>>,
    /// Filter to tables matching a domain prefix.
    pub domain_filter: Option<String>,
    /// Whether to include columns in the diagram.
    pub include_columns: bool,
    /// Whether to show governance level annotations.
    pub show_governance: bool,
    /// Maximum number of tables to include.
    pub max_tables: Option<usize>,
    /// Output format (currently only "mermaid").
    pub format: Option<String>,
    /// Whether to show verb surface annotations.
    pub show_verb_surface: bool,
    /// Whether to include affinity kind labels in verb annotations.
    pub show_affinity_kind: bool,
}

// ── Convenience constructors ────────────────────────────────────

impl DiagramModel {
    /// Create an empty diagram model.
    pub fn empty() -> Self {
        Self {
            entities: Vec::new(),
            relationships: Vec::new(),
            metadata: DiagramMetadata {
                total_tables: 0,
                included_tables: 0,
                filtered: false,
            },
        }
    }
}

impl DiagramEntity {
    /// Stable sort key for deterministic output.
    pub fn sort_key(&self) -> String {
        format!("{}:{}", self.schema, self.table_name)
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governance_level_serde_round_trip() {
        let levels = vec![
            GovernanceLevel::Full,
            GovernanceLevel::Partial,
            GovernanceLevel::None,
        ];
        for level in &levels {
            let json = serde_json::to_value(level).unwrap();
            let back: GovernanceLevel = serde_json::from_value(json).unwrap();
            assert_eq!(&back, level);
        }
    }

    #[test]
    fn relationship_kind_serde_round_trip() {
        let kinds = vec![
            RelationshipKind::ForeignKey,
            RelationshipKind::EntityRelationship,
        ];
        for kind in &kinds {
            let json = serde_json::to_value(kind).unwrap();
            let back: RelationshipKind = serde_json::from_value(json).unwrap();
            assert_eq!(&back, kind);
        }
    }

    #[test]
    fn diagram_model_empty() {
        let model = DiagramModel::empty();
        assert!(model.entities.is_empty());
        assert!(model.relationships.is_empty());
        assert_eq!(model.metadata.total_tables, 0);
    }

    #[test]
    fn diagram_entity_sort_key() {
        let entity = DiagramEntity {
            schema: "ob-poc".into(),
            table_name: "cbus".into(),
            entity_type_fqn: None,
            attributes: vec![],
            primary_keys: vec![],
            verb_surface: vec![],
            governance_level: GovernanceLevel::None,
        };
        assert_eq!(entity.sort_key(), "ob-poc:cbus");
    }

    #[test]
    fn render_options_default() {
        let opts = RenderOptions::default();
        assert!(opts.schema_filter.is_none());
        assert!(opts.domain_filter.is_none());
        assert!(!opts.include_columns);
        assert!(!opts.show_governance);
        assert!(opts.max_tables.is_none());
    }

    #[test]
    fn table_input_serde_round_trip() {
        let input = TableInput {
            schema: "ob-poc".into(),
            table_name: "cbus".into(),
            columns: vec![ColumnInput {
                name: "cbu_id".into(),
                sql_type: "uuid".into(),
                is_nullable: false,
            }],
            primary_keys: vec!["cbu_id".into()],
            foreign_keys: vec![ForeignKeyInput {
                from_column: "apex_entity_id".into(),
                target_schema: "ob-poc".into(),
                target_table: "entities".into(),
                target_column: "entity_id".into(),
            }],
        };
        let json = serde_json::to_value(&input).unwrap();
        let back: TableInput = serde_json::from_value(json).unwrap();
        assert_eq!(back.table_name, "cbus");
        assert_eq!(back.columns.len(), 1);
        assert_eq!(back.foreign_keys.len(), 1);
    }

    #[test]
    fn diagram_model_serde_round_trip() {
        let model = DiagramModel {
            entities: vec![DiagramEntity {
                schema: "ob-poc".into(),
                table_name: "cbus".into(),
                entity_type_fqn: Some("cbu".into()),
                attributes: vec![DiagramAttribute {
                    name: "cbu_id".into(),
                    sql_type: "uuid".into(),
                    is_nullable: false,
                    is_pk: true,
                    is_fk: false,
                    attribute_fqn: None,
                    producing_verbs: vec!["cbu.create".into()],
                    consuming_verbs: vec![],
                }],
                primary_keys: vec!["cbu_id".into()],
                verb_surface: vec![VerbSurfaceEntry {
                    verb_fqn: "cbu.create".into(),
                    relation: "produces".into(),
                }],
                governance_level: GovernanceLevel::Full,
            }],
            relationships: vec![DiagramRelationship {
                from_entity: "ob-poc:cbus".into(),
                to_entity: "ob-poc:entities".into(),
                kind: RelationshipKind::ForeignKey,
                cardinality: Some("N:1".into()),
                fk_column: Some("apex_entity_id".into()),
            }],
            metadata: DiagramMetadata {
                total_tables: 1,
                included_tables: 1,
                filtered: false,
            },
        };
        let json = serde_json::to_value(&model).unwrap();
        let back: DiagramModel = serde_json::from_value(json).unwrap();
        assert_eq!(back.entities.len(), 1);
        assert_eq!(back.entities[0].governance_level, GovernanceLevel::Full);
        assert_eq!(back.relationships.len(), 1);
    }
}
