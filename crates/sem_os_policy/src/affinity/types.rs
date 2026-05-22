//! AffinityGraph types — bidirectional verb↔data index.
//!
//! All types are pure value types with no DB dependency.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

// ── Core graph ──────────────────────────────────────────────────

/// Pre-computed bidirectional index of verb↔data relationships,
/// built entirely from active registry snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffinityGraph {
    /// All edges in the graph.
    pub edges: Vec<AffinityEdge>,

    /// Forward index: verb FQN → data assets it touches.
    pub verb_to_data: HashMap<String, Vec<usize>>,

    /// Reverse index: data ref key → verbs that touch it.
    pub data_to_verb: HashMap<String, Vec<usize>>,

    /// Entity FQN → physical table mapping.
    pub entity_to_table: HashMap<String, TableRef>,

    /// Physical table key → entity FQN mapping.
    pub table_to_entity: HashMap<String, String>,

    /// Attribute FQN → physical column mapping.
    pub attribute_to_column: HashMap<String, ColumnRef>,

    /// Derivation lineage edges (attribute→attribute).
    pub derivation_edges: Vec<DerivationEdge>,

    /// Entity↔entity relationships from RelationshipTypeDefs.
    pub entity_relationships: Vec<EntityRelationship>,

    /// All declared verb FQNs from VerbContract snapshots, including verbs
    /// that produced zero affinity edges.
    pub known_verbs: HashSet<String>,
}

/// A single verb↔data relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffinityEdge {
    pub verb_fqn: String,
    pub data_ref: DataRef,
    pub affinity_kind: AffinityKind,
    pub provenance: AffinityProvenance,
}

// ── Edge classification ─────────────────────────────────────────

/// How a verb relates to a data asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AffinityKind {
    /// Verb produces an entity (from VerbContractBody.produces).
    Produces,
    /// Verb consumes an entity (from VerbContractBody.consumes).
    Consumes,
    /// Verb reads from a table (CRUD select or reads_from).
    CrudRead,
    /// Verb inserts into a table (CRUD insert or writes_to).
    CrudInsert,
    /// Verb updates a table (CRUD update).
    CrudUpdate,
    /// Verb deletes from a table (CRUD delete).
    CrudDelete,
    /// Verb looks up an entity via an argument.
    ArgLookup { arg_name: String },
    /// Verb produces an attribute value (from AttributeSource.producing_verb).
    ProducesAttribute,
    /// Verb consumes an attribute value (from AttributeSink.consuming_verb).
    ConsumesAttribute { arg_name: String },
}

/// Where an edge was derived from — provenance chain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AffinityProvenance {
    /// From VerbContractBody.produces.
    VerbProduces,
    /// From VerbContractBody.consumes.
    VerbConsumes,
    /// From VerbContractBody.crud_mapping.
    VerbCrudMapping,
    /// From VerbContractBody.args[].lookup.
    VerbArgLookup,
    /// From AttributeDefBody.source.producing_verb.
    AttributeSource,
    /// From AttributeDefBody.sinks[].consuming_verb.
    AttributeSink,
    /// From DerivationSpecBody (lineage).
    DerivationSpec,
    /// From VerbContractBody.reads_from / writes_to (supplemental).
    VerbDataFootprint,
}

// ── Data references ─────────────────────────────────────────────

/// Unified reference to a data asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DataRef {
    Table(TableRef),
    Column(ColumnRef),
    EntityType { fqn: String },
    Attribute { fqn: String },
}

impl DataRef {
    /// Stable string key for indexing.
    pub fn index_key(&self) -> String {
        match self {
            DataRef::Table(t) => format!("table:{}:{}", t.schema, t.table),
            DataRef::Column(c) => format!("column:{}:{}:{}", c.schema, c.table, c.column),
            DataRef::EntityType { fqn } => format!("entity_type:{fqn}"),
            DataRef::Attribute { fqn } => format!("attribute:{fqn}"),
        }
    }
}

/// Physical table reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TableRef {
    pub schema: String,
    pub table: String,
}

impl TableRef {
    pub fn new(schema: impl Into<String>, table: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            table: table.into(),
        }
    }

    /// Stable string key for lookups.
    pub fn key(&self) -> String {
        format!("{}:{}", self.schema, self.table)
    }
}

/// Physical column reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColumnRef {
    pub schema: String,
    pub table: String,
    pub column: String,
}

impl ColumnRef {
    pub fn new(
        schema: impl Into<String>,
        table: impl Into<String>,
        column: impl Into<String>,
    ) -> Self {
        Self {
            schema: schema.into(),
            table: table.into(),
            column: column.into(),
        }
    }
}

// ── Lineage ─────────────────────────────────────────────────────

/// Attribute-level lineage edge from derivation specs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivationEdge {
    pub from_attribute: String,
    pub to_attribute: String,
    pub spec_fqn: String,
}

// ── Relationships (Pass 5) ──────────────────────────────────────

/// Entity↔entity relationship captured from RelationshipTypeDefs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRelationship {
    pub fqn: String,
    pub source_entity_type_fqn: String,
    pub target_entity_type_fqn: String,
    pub cardinality: String,
    pub edge_class: Option<String>,
    pub directionality: Option<String>,
}

// ── Query result types ──────────────────────────────────────────

/// Query result: a verb with its edge context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbAffinity {
    pub verb_fqn: String,
    pub affinity_kind: AffinityKind,
    pub provenance: AffinityProvenance,
}

/// Query result: a data asset with its edge context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataAffinity {
    pub data_ref: DataRef,
    pub affinity_kind: AffinityKind,
    pub provenance: AffinityProvenance,
}

/// Transitive data reach of a verb.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataFootprint {
    pub tables: BTreeMap<String, TableRef>,
    pub columns: BTreeMap<String, ColumnRef>,
    pub attributes: HashSet<String>,
    pub entity_types: HashSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_ref_hash_eq() {
        let a = TableRef::new("ob-poc", "cbus");
        let b = TableRef::new("ob-poc", "cbus");
        let c = TableRef::new("ob-poc", "entities");
        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut set = HashSet::new();
        set.insert(a.clone());
        set.insert(b);
        assert_eq!(set.len(), 1);
        set.insert(c);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn column_ref_hash_eq() {
        let a = ColumnRef::new("ob-poc", "cbus", "name");
        let b = ColumnRef::new("ob-poc", "cbus", "name");
        let c = ColumnRef::new("ob-poc", "cbus", "jurisdiction_code");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn data_ref_hash_eq() {
        let t1 = DataRef::Table(TableRef::new("ob-poc", "cbus"));
        let t2 = DataRef::Table(TableRef::new("ob-poc", "cbus"));
        let e1 = DataRef::EntityType { fqn: "cbu".into() };
        let e2 = DataRef::EntityType { fqn: "cbu".into() };
        let a1 = DataRef::Attribute {
            fqn: "cbu.name".into(),
        };
        assert_eq!(t1, t2);
        assert_eq!(e1, e2);
        assert_ne!(t1, e1);
        assert_ne!(e1, a1);
    }

    #[test]
    fn data_ref_index_key() {
        assert_eq!(
            DataRef::Table(TableRef::new("ob-poc", "cbus")).index_key(),
            "table:ob-poc:cbus"
        );
        assert_eq!(
            DataRef::Column(ColumnRef::new("ob-poc", "cbus", "name")).index_key(),
            "column:ob-poc:cbus:name"
        );
        assert_eq!(
            DataRef::EntityType { fqn: "cbu".into() }.index_key(),
            "entity_type:cbu"
        );
        assert_eq!(
            DataRef::Attribute {
                fqn: "cbu.name".into()
            }
            .index_key(),
            "attribute:cbu.name"
        );
    }

    #[test]
    fn affinity_kind_serde_round_trip() {
        let kinds = vec![
            AffinityKind::Produces,
            AffinityKind::Consumes,
            AffinityKind::CrudRead,
            AffinityKind::CrudInsert,
            AffinityKind::CrudUpdate,
            AffinityKind::CrudDelete,
            AffinityKind::ArgLookup {
                arg_name: "counterparty".into(),
            },
            AffinityKind::ProducesAttribute,
            AffinityKind::ConsumesAttribute {
                arg_name: "code".into(),
            },
        ];
        for kind in &kinds {
            let json = serde_json::to_value(kind).unwrap();
            let back: AffinityKind = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(&back, kind);
        }
    }

    #[test]
    fn affinity_provenance_serde_round_trip() {
        let provenances = vec![
            AffinityProvenance::VerbProduces,
            AffinityProvenance::VerbConsumes,
            AffinityProvenance::VerbCrudMapping,
            AffinityProvenance::VerbArgLookup,
            AffinityProvenance::AttributeSource,
            AffinityProvenance::AttributeSink,
            AffinityProvenance::DerivationSpec,
            AffinityProvenance::VerbDataFootprint,
        ];
        for prov in &provenances {
            let json = serde_json::to_value(prov).unwrap();
            let back: AffinityProvenance = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(&back, prov);
        }
    }

    #[test]
    fn data_ref_serde_round_trip() {
        let refs = vec![
            DataRef::Table(TableRef::new("ob-poc", "cbus")),
            DataRef::Column(ColumnRef::new("ob-poc", "cbus", "name")),
            DataRef::EntityType { fqn: "cbu".into() },
            DataRef::Attribute {
                fqn: "cbu.name".into(),
            },
        ];
        for dr in &refs {
            let json = serde_json::to_value(dr).unwrap();
            let back: DataRef = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(&back, dr);
        }
    }

    #[test]
    fn affinity_edge_serde_round_trip() {
        let edge = AffinityEdge {
            verb_fqn: "cbu.create".into(),
            data_ref: DataRef::Table(TableRef::new("ob-poc", "cbus")),
            affinity_kind: AffinityKind::CrudInsert,
            provenance: AffinityProvenance::VerbCrudMapping,
        };
        let json = serde_json::to_value(&edge).unwrap();
        let back: AffinityEdge = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn derivation_edge_serde_round_trip() {
        let edge = DerivationEdge {
            from_attribute: "holding.amount".into(),
            to_attribute: "cbu.total_aum".into(),
            spec_fqn: "derived.total_aum".into(),
        };
        let json = serde_json::to_value(&edge).unwrap();
        let back: DerivationEdge = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn entity_relationship_serde_round_trip() {
        let rel = EntityRelationship {
            fqn: "rel.parent_child".into(),
            source_entity_type_fqn: "entity.organization".into(),
            target_entity_type_fqn: "entity.organization".into(),
            cardinality: "one_to_many".into(),
            edge_class: Some("ownership".into()),
            directionality: Some("forward".into()),
        };
        let json = serde_json::to_value(&rel).unwrap();
        let back: EntityRelationship = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn verb_affinity_serde_round_trip() {
        let va = VerbAffinity {
            verb_fqn: "cbu.create".into(),
            affinity_kind: AffinityKind::Produces,
            provenance: AffinityProvenance::VerbProduces,
        };
        let json = serde_json::to_value(&va).unwrap();
        let back: VerbAffinity = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn data_footprint_default() {
        let fp = DataFootprint::default();
        assert!(fp.tables.is_empty());
        assert!(fp.columns.is_empty());
        assert!(fp.attributes.is_empty());
        assert!(fp.entity_types.is_empty());
    }

    #[test]
    fn affinity_graph_empty() {
        let graph = AffinityGraph {
            edges: vec![],
            verb_to_data: HashMap::new(),
            data_to_verb: HashMap::new(),
            entity_to_table: HashMap::new(),
            table_to_entity: HashMap::new(),
            attribute_to_column: HashMap::new(),
            derivation_edges: vec![],
            entity_relationships: vec![],
            known_verbs: HashSet::new(),
        };
        assert!(graph.edges.is_empty());
        let json = serde_json::to_value(&graph).unwrap();
        let back: AffinityGraph = serde_json::from_value(json).unwrap();
        assert!(back.edges.is_empty());
    }
}
