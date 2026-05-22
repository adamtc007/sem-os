//! AffinityGraph builder — 5-pass construction from registry snapshots.
//!
//! Entry point: `AffinityGraph::build(snapshots)` takes a flat list of active
//! `SnapshotRow` (all 13 object types), deserializes the relevant body types,
//! and runs 5 passes to produce a complete bidirectional verb↔data index.

use std::collections::HashMap;

use crate::affinity::{
    AffinityEdge, AffinityGraph, AffinityKind, AffinityProvenance, ColumnRef, DataRef,
    DerivationEdge, EntityRelationship, TableRef,
};
use sem_os_ontology::attribute_def::AttributeDefBody;
use sem_os_ontology::derivation_spec::DerivationSpecBody;
use sem_os_ontology::entity_type_def::EntityTypeDefBody;
use sem_os_ontology::relationship_type_def::RelationshipTypeDefBody;
use sem_os_ontology::verb_contract::VerbContractBody;
use sem_os_types::{ObjectType, SnapshotRow};

impl AffinityGraph {
    /// Build an AffinityGraph from a flat list of active registry snapshots.
    ///
    /// Filters by relevant object types, deserializes JSONB definitions into
    /// typed bodies, and runs 5 passes. Returns an empty graph (no error) when
    /// no snapshots are provided or none match relevant types.
    pub fn build(snapshots: &[SnapshotRow]) -> Self {
        let mut edges: Vec<AffinityEdge> = Vec::new();
        let mut known_verbs: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut entity_to_table: HashMap<String, TableRef> = HashMap::new();
        let mut table_to_entity: HashMap<String, String> = HashMap::new();
        let mut attribute_to_column: HashMap<String, ColumnRef> = HashMap::new();
        let mut derivation_edges: Vec<DerivationEdge> = Vec::new();
        let mut entity_relationships: Vec<EntityRelationship> = Vec::new();

        // ── Pass 1: VerbContracts → Forward edges ──────────────────
        for row in snapshots
            .iter()
            .filter(|r| r.object_type == ObjectType::VerbContract)
        {
            let Ok(body) = row.parse_definition::<VerbContractBody>() else {
                continue;
            };
            known_verbs.insert(body.fqn.clone());
            pass1_verb_contract(&body, &mut edges);
        }

        // ── Pass 2: EntityTypeDefs → Entity↔table bimaps ──────────
        for row in snapshots
            .iter()
            .filter(|r| r.object_type == ObjectType::EntityTypeDef)
        {
            let Ok(body) = row.parse_definition::<EntityTypeDefBody>() else {
                continue;
            };
            pass2_entity_type_def(&body, &mut entity_to_table, &mut table_to_entity);
        }

        // ── Pass 3: AttributeDefs → Reverse edges ─────────────────
        for row in snapshots
            .iter()
            .filter(|r| r.object_type == ObjectType::AttributeDef)
        {
            let Ok(body) = row.parse_definition::<AttributeDefBody>() else {
                continue;
            };
            pass3_attribute_def(&body, &mut edges, &mut attribute_to_column);
        }

        // ── Pass 4: DerivationSpecs → Lineage edges ───────────────
        for row in snapshots
            .iter()
            .filter(|r| r.object_type == ObjectType::DerivationSpec)
        {
            let Ok(body) = row.parse_definition::<DerivationSpecBody>() else {
                continue;
            };
            pass4_derivation_spec(&body, &mut derivation_edges);
        }

        // ── Pass 5: RelationshipTypeDefs → Entity↔entity ──────────
        for row in snapshots
            .iter()
            .filter(|r| r.object_type == ObjectType::RelationshipTypeDef)
        {
            let Ok(body) = row.parse_definition::<RelationshipTypeDefBody>() else {
                continue;
            };
            pass5_relationship_type_def(&body, &mut entity_relationships);
        }

        // ── Build bidirectional indexes ────────────────────────────
        let mut verb_to_data: HashMap<String, Vec<usize>> = HashMap::new();
        let mut data_to_verb: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, edge) in edges.iter().enumerate() {
            verb_to_data
                .entry(edge.verb_fqn.clone())
                .or_default()
                .push(idx);
            data_to_verb
                .entry(edge.data_ref.index_key())
                .or_default()
                .push(idx);
        }

        AffinityGraph {
            edges,
            verb_to_data,
            data_to_verb,
            entity_to_table,
            table_to_entity,
            attribute_to_column,
            derivation_edges,
            entity_relationships,
            known_verbs,
        }
    }
}

// ── Pass 1: VerbContracts ──────────────────────────────────────────

fn pass1_verb_contract(body: &VerbContractBody, edges: &mut Vec<AffinityEdge>) {
    let verb_fqn = &body.fqn;

    // produces → EntityType edge
    if let Some(ref produces) = body.produces {
        edges.push(AffinityEdge {
            verb_fqn: verb_fqn.clone(),
            data_ref: DataRef::EntityType {
                fqn: produces.entity_type.clone(),
            },
            affinity_kind: AffinityKind::Produces,
            provenance: AffinityProvenance::VerbProduces,
        });
    }

    // consumes → EntityType edges
    for entity_fqn in &body.consumes {
        edges.push(AffinityEdge {
            verb_fqn: verb_fqn.clone(),
            data_ref: DataRef::EntityType {
                fqn: entity_fqn.clone(),
            },
            affinity_kind: AffinityKind::Consumes,
            provenance: AffinityProvenance::VerbConsumes,
        });
    }

    // crud_mapping → Table edge
    if let Some(ref crud) = body.crud_mapping {
        let kind = match crud.operation.as_str() {
            "select" | "read" => Some(AffinityKind::CrudRead),
            "insert" | "create" => Some(AffinityKind::CrudInsert),
            "update" => Some(AffinityKind::CrudUpdate),
            "delete" => Some(AffinityKind::CrudDelete),
            _ => None,
        };
        if let (Some(kind), Some(table)) = (kind, crud.table.as_ref()) {
            edges.push(AffinityEdge {
                verb_fqn: verb_fqn.clone(),
                data_ref: DataRef::Table(TableRef::new(
                    crud.schema.as_deref().unwrap_or(""),
                    table.as_str(),
                )),
                affinity_kind: kind,
                provenance: AffinityProvenance::VerbCrudMapping,
            });
        }
    }

    // args[].lookup → Table edges
    for arg in &body.args {
        if let Some(ref lookup) = arg.lookup {
            edges.push(AffinityEdge {
                verb_fqn: verb_fqn.clone(),
                data_ref: DataRef::Table(TableRef::new(
                    lookup.schema.as_deref().unwrap_or(""),
                    lookup.table.as_str(),
                )),
                affinity_kind: AffinityKind::ArgLookup {
                    arg_name: arg.name.clone(),
                },
                provenance: AffinityProvenance::VerbArgLookup,
            });
        }
    }

    // reads_from → supplemental CrudRead edges
    for table_name in &body.reads_from {
        edges.push(AffinityEdge {
            verb_fqn: verb_fqn.clone(),
            data_ref: DataRef::Table(TableRef::new("", table_name.as_str())),
            affinity_kind: AffinityKind::CrudRead,
            provenance: AffinityProvenance::VerbDataFootprint,
        });
    }

    // writes_to → supplemental CrudInsert edges
    for table_name in &body.writes_to {
        edges.push(AffinityEdge {
            verb_fqn: verb_fqn.clone(),
            data_ref: DataRef::Table(TableRef::new("", table_name.as_str())),
            affinity_kind: AffinityKind::CrudInsert,
            provenance: AffinityProvenance::VerbDataFootprint,
        });
    }
}

// ── Pass 2: EntityTypeDefs ─────────────────────────────────────────

fn pass2_entity_type_def(
    body: &EntityTypeDefBody,
    entity_to_table: &mut HashMap<String, TableRef>,
    table_to_entity: &mut HashMap<String, String>,
) {
    if let Some(ref db) = body.db_table {
        let table_ref = TableRef::new(&db.schema, &db.table);
        let key = table_ref.key();
        entity_to_table.insert(body.fqn.clone(), table_ref);
        table_to_entity.insert(key, body.fqn.clone());
    }
}

// ── Pass 3: AttributeDefs ──────────────────────────────────────────

fn pass3_attribute_def(
    body: &AttributeDefBody,
    edges: &mut Vec<AffinityEdge>,
    attribute_to_column: &mut HashMap<String, ColumnRef>,
) {
    let attr_fqn = &body.fqn;

    if let Some(ref source) = body.source {
        // producing_verb → ProducesAttribute edge
        if let Some(ref producing_verb) = source.producing_verb {
            edges.push(AffinityEdge {
                verb_fqn: producing_verb.clone(),
                data_ref: DataRef::Attribute {
                    fqn: attr_fqn.clone(),
                },
                affinity_kind: AffinityKind::ProducesAttribute,
                provenance: AffinityProvenance::AttributeSource,
            });
        }

        // source (schema, table, column) → attribute_to_column mapping
        if let (Some(ref schema), Some(ref table), Some(ref column)) =
            (&source.schema, &source.table, &source.column)
        {
            attribute_to_column.insert(
                attr_fqn.clone(),
                ColumnRef::new(schema.as_str(), table.as_str(), column.as_str()),
            );
        }
    }

    // sinks → ConsumesAttribute edges
    for sink in &body.sinks {
        edges.push(AffinityEdge {
            verb_fqn: sink.consuming_verb.clone(),
            data_ref: DataRef::Attribute {
                fqn: attr_fqn.clone(),
            },
            affinity_kind: AffinityKind::ConsumesAttribute {
                arg_name: sink.arg_name.clone(),
            },
            provenance: AffinityProvenance::AttributeSink,
        });
    }
}

// ── Pass 4: DerivationSpecs ────────────────────────────────────────

fn pass4_derivation_spec(body: &DerivationSpecBody, derivation_edges: &mut Vec<DerivationEdge>) {
    for input in &body.inputs {
        derivation_edges.push(DerivationEdge {
            from_attribute: input.attribute_fqn.clone(),
            to_attribute: body.output_attribute_fqn.clone(),
            spec_fqn: body.fqn.clone(),
        });
    }
}

// ── Pass 5: RelationshipTypeDefs ───────────────────────────────────

fn pass5_relationship_type_def(
    body: &RelationshipTypeDefBody,
    entity_relationships: &mut Vec<EntityRelationship>,
) {
    entity_relationships.push(EntityRelationship {
        fqn: body.fqn.clone(),
        source_entity_type_fqn: body.source_entity_type_fqn.clone(),
        target_entity_type_fqn: body.target_entity_type_fqn.clone(),
        cardinality: format!("{:?}", body.cardinality).to_lowercase(),
        edge_class: body.edge_class.clone(),
        directionality: body
            .directionality
            .as_ref()
            .map(|d| format!("{d:?}").to_lowercase()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    use sem_os_ontology::derivation_spec::DerivationInput;
    use sem_os_types::{ChangeType, GovernanceTier, SnapshotStatus, TrustClass};

    /// Helper: create a minimal SnapshotRow for testing.
    fn snap(object_type: ObjectType, definition: serde_json::Value) -> SnapshotRow {
        SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Convenience,
            security_label: serde_json::json!({}),
            effective_from: Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            definition,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_build_empty_snapshots() {
        let graph = AffinityGraph::build(&[]);
        assert!(graph.edges.is_empty());
        assert!(graph.verb_to_data.is_empty());
        assert!(graph.data_to_verb.is_empty());
        assert!(graph.entity_to_table.is_empty());
        assert!(graph.derivation_edges.is_empty());
        assert!(graph.entity_relationships.is_empty());
    }

    #[test]
    fn test_pass1_verb_produces() {
        let verb = serde_json::to_value(VerbContractBody {
            fqn: "cbu.create".into(),
            domain: "cbu".into(),
            action: "create".into(),
            description: "Create a CBU".into(),
            behavior: "plugin".into(),
            args: vec![],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: Some(sem_os_ontology::verb_contract::VerbProducesSpec {
                entity_type: "cbu".into(),
                resolved: true,
            }),
            consumes: vec![],
            invocation_phrases: vec![],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: true,
            produces_focus: false,
            metadata: None,
            crud_mapping: None,
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[snap(ObjectType::VerbContract, verb)]);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].verb_fqn, "cbu.create");
        assert_eq!(graph.edges[0].affinity_kind, AffinityKind::Produces);
        assert_eq!(
            graph.edges[0].data_ref,
            DataRef::EntityType { fqn: "cbu".into() }
        );
        assert_eq!(graph.edges[0].provenance, AffinityProvenance::VerbProduces);
        // Check forward index
        assert!(graph.verb_to_data.contains_key("cbu.create"));
        assert_eq!(graph.verb_to_data["cbu.create"], vec![0]);
    }

    #[test]
    fn test_pass1_crud_mapping() {
        let verb = serde_json::to_value(VerbContractBody {
            fqn: "cbu.list".into(),
            domain: "cbu".into(),
            action: "list".into(),
            description: "List CBUs".into(),
            behavior: "crud".into(),
            args: vec![],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec![],
            invocation_phrases: vec![],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: false,
            produces_focus: false,
            metadata: None,
            crud_mapping: Some(sem_os_ontology::verb_contract::VerbCrudMapping {
                operation: "select".into(),
                table: Some("cbus".into()),
                schema: Some("ob-poc".into()),
                key_column: None,
                ..Default::default()
            }),
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[snap(ObjectType::VerbContract, verb)]);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].affinity_kind, AffinityKind::CrudRead);
        assert_eq!(
            graph.edges[0].data_ref,
            DataRef::Table(TableRef::new("ob-poc", "cbus"))
        );
        assert_eq!(
            graph.edges[0].provenance,
            AffinityProvenance::VerbCrudMapping
        );
    }

    #[test]
    fn test_pass1_arg_lookup() {
        let verb = serde_json::to_value(VerbContractBody {
            fqn: "isda.create".into(),
            domain: "isda".into(),
            action: "create".into(),
            description: "Create ISDA".into(),
            behavior: "plugin".into(),
            args: vec![sem_os_ontology::verb_contract::VerbArgDef {
                name: "counterparty".into(),
                arg_type: "uuid".into(),
                required: true,
                description: None,
                lookup: Some(sem_os_ontology::verb_contract::VerbArgLookup {
                    table: "entities".into(),
                    entity_type: "entity".into(),
                    schema: Some("ob-poc".into()),
                    search_key: None,
                    primary_key: None,
                }),
                valid_values: None,
                default: None,
                maps_to: None,
            }],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec![],
            invocation_phrases: vec![],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: true,
            produces_focus: false,
            metadata: None,
            crud_mapping: None,
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[snap(ObjectType::VerbContract, verb)]);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(
            graph.edges[0].affinity_kind,
            AffinityKind::ArgLookup {
                arg_name: "counterparty".into()
            }
        );
        assert_eq!(
            graph.edges[0].data_ref,
            DataRef::Table(TableRef::new("ob-poc", "entities"))
        );
    }

    #[test]
    fn test_pass1_reads_from_writes_to() {
        let verb = serde_json::to_value(VerbContractBody {
            fqn: "cbu.create".into(),
            domain: "cbu".into(),
            action: "create".into(),
            description: "Create".into(),
            behavior: "plugin".into(),
            args: vec![],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec![],
            invocation_phrases: vec![],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: true,
            produces_focus: false,
            metadata: None,
            crud_mapping: None,
            reads_from: vec!["entities".into()],
            writes_to: vec!["cbus".into()],
            outputs: vec![],
            produces_shared_facts: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[snap(ObjectType::VerbContract, verb)]);
        assert_eq!(graph.edges.len(), 2);
        // reads_from → CrudRead with VerbDataFootprint provenance
        assert_eq!(graph.edges[0].affinity_kind, AffinityKind::CrudRead);
        assert_eq!(
            graph.edges[0].provenance,
            AffinityProvenance::VerbDataFootprint
        );
        // writes_to → CrudInsert with VerbDataFootprint provenance
        assert_eq!(graph.edges[1].affinity_kind, AffinityKind::CrudInsert);
        assert_eq!(
            graph.edges[1].provenance,
            AffinityProvenance::VerbDataFootprint
        );
    }

    #[test]
    fn test_pass2_entity_table_bimap() {
        let entity = serde_json::to_value(EntityTypeDefBody {
            fqn: "cbu".into(),
            name: "Client Business Unit".into(),
            description: "Atomic trading unit".into(),
            domain: "cbu".into(),
            db_table: Some(sem_os_ontology::entity_type_def::DbTableMapping {
                schema: "ob-poc".into(),
                table: "cbus".into(),
                primary_key: "cbu_id".into(),
                name_column: Some("name".into()),
            }),
            lifecycle_states: vec![],
            required_attributes: vec![],
            optional_attributes: vec![],
            parent_type: None,
            governance_tier: None,
            security_classification: None,
            pii: None,
            read_by_verbs: vec![],
            written_by_verbs: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[snap(ObjectType::EntityTypeDef, entity)]);
        // entity→table
        let table_ref = graph.entity_to_table.get("cbu").unwrap();
        assert_eq!(table_ref.schema, "ob-poc");
        assert_eq!(table_ref.table, "cbus");
        // table→entity (bidirectional)
        assert_eq!(graph.table_to_entity.get("ob-poc:cbus").unwrap(), "cbu");
    }

    #[test]
    fn test_pass3_attribute_source_sink() {
        let attr = serde_json::to_value(AttributeDefBody {
            fqn: "cbu.jurisdiction_code".into(),
            name: "jurisdiction_code".into(),
            description: "ISO jurisdiction".into(),
            domain: "cbu".into(),
            data_type: sem_os_ontology::attribute_def::AttributeDataType::String,
            evidence_grade: sem_os_types::EvidenceGrade::None,
            source: Some(sem_os_ontology::attribute_def::AttributeSource {
                producing_verb: Some("cbu.create".into()),
                schema: Some("ob-poc".into()),
                table: Some("cbus".into()),
                column: Some("jurisdiction_code".into()),
                derived: false,
            }),
            constraints: None,
            sinks: vec![sem_os_ontology::attribute_def::AttributeSink {
                consuming_verb: "session.load-jurisdiction".into(),
                arg_name: "code".into(),
            }],
            category: None,
            validation_rules: None,
            applicability: None,
            is_required: None,
            default_value: None,
            group_id: None,
            is_derived: None,
            derivation_spec_fqn: None,
            visibility: None,
        })
        .unwrap();

        let graph = AffinityGraph::build(&[snap(ObjectType::AttributeDef, attr)]);
        // ProducesAttribute edge
        assert_eq!(graph.edges.len(), 2);
        assert_eq!(graph.edges[0].verb_fqn, "cbu.create");
        assert_eq!(
            graph.edges[0].affinity_kind,
            AffinityKind::ProducesAttribute
        );
        assert_eq!(
            graph.edges[0].provenance,
            AffinityProvenance::AttributeSource
        );
        // ConsumesAttribute edge
        assert_eq!(graph.edges[1].verb_fqn, "session.load-jurisdiction");
        assert_eq!(
            graph.edges[1].affinity_kind,
            AffinityKind::ConsumesAttribute {
                arg_name: "code".into()
            }
        );
        // attribute_to_column mapping
        let col = graph
            .attribute_to_column
            .get("cbu.jurisdiction_code")
            .unwrap();
        assert_eq!(col.schema, "ob-poc");
        assert_eq!(col.table, "cbus");
        assert_eq!(col.column, "jurisdiction_code");
    }

    #[test]
    fn test_pass4_derivation_lineage() {
        let spec = serde_json::to_value(DerivationSpecBody {
            fqn: "derived.total_aum".into(),
            name: "Total AUM".into(),
            description: "Sum of holdings".into(),
            output_attribute_fqn: "cbu.total_aum".into(),
            inputs: vec![
                DerivationInput {
                    attribute_fqn: "holding.amount".into(),
                    role: "addend".into(),
                    required: true,
                },
                DerivationInput {
                    attribute_fqn: "holding.currency".into(),
                    role: "currency".into(),
                    required: true,
                },
            ],
            expression: sem_os_ontology::derivation_spec::DerivationExpression::FunctionRef {
                ref_name: "sum_holdings".into(),
            },
            null_semantics: Default::default(),
            freshness_rule: None,
            security_inheritance: Default::default(),
            evidence_grade: Default::default(),
            tests: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[snap(ObjectType::DerivationSpec, spec)]);
        assert_eq!(graph.derivation_edges.len(), 2);
        assert_eq!(graph.derivation_edges[0].from_attribute, "holding.amount");
        assert_eq!(graph.derivation_edges[0].to_attribute, "cbu.total_aum");
        assert_eq!(graph.derivation_edges[0].spec_fqn, "derived.total_aum");
        assert_eq!(graph.derivation_edges[1].from_attribute, "holding.currency");
    }

    #[test]
    fn test_pass5_relationship() {
        let rel = serde_json::to_value(RelationshipTypeDefBody {
            fqn: "rel.parent_child".into(),
            name: "Parent-Child".into(),
            description: "Corporate hierarchy".into(),
            domain: "entity".into(),
            source_entity_type_fqn: "entity.organization".into(),
            target_entity_type_fqn: "entity.organization".into(),
            cardinality: sem_os_ontology::relationship_type_def::RelationshipCardinality::OneToMany,
            edge_class: Some("ownership".into()),
            directionality: Some(sem_os_ontology::relationship_type_def::Directionality::Forward),
            inverse_fqn: None,
            constraints: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[snap(ObjectType::RelationshipTypeDef, rel)]);
        assert_eq!(graph.entity_relationships.len(), 1);
        let er = &graph.entity_relationships[0];
        assert_eq!(er.fqn, "rel.parent_child");
        assert_eq!(er.source_entity_type_fqn, "entity.organization");
        assert_eq!(er.target_entity_type_fqn, "entity.organization");
        assert_eq!(er.edge_class.as_deref(), Some("ownership"));
        assert_eq!(er.directionality.as_deref(), Some("forward"));
    }

    #[test]
    fn test_bidirectional_index() {
        // Two verbs sharing the same table
        let verb1 = serde_json::to_value(VerbContractBody {
            fqn: "cbu.create".into(),
            domain: "cbu".into(),
            action: "create".into(),
            description: "Create".into(),
            behavior: "crud".into(),
            args: vec![],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec![],
            invocation_phrases: vec![],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: true,
            produces_focus: false,
            metadata: None,
            crud_mapping: Some(sem_os_ontology::verb_contract::VerbCrudMapping {
                operation: "insert".into(),
                table: Some("cbus".into()),
                schema: Some("ob-poc".into()),
                key_column: None,
                ..Default::default()
            }),
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        })
        .unwrap();

        let verb2 = serde_json::to_value(VerbContractBody {
            fqn: "cbu.list".into(),
            domain: "cbu".into(),
            action: "list".into(),
            description: "List".into(),
            behavior: "crud".into(),
            args: vec![],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec![],
            invocation_phrases: vec![],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: false,
            produces_focus: false,
            metadata: None,
            crud_mapping: Some(sem_os_ontology::verb_contract::VerbCrudMapping {
                operation: "select".into(),
                table: Some("cbus".into()),
                schema: Some("ob-poc".into()),
                key_column: None,
                ..Default::default()
            }),
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[
            snap(ObjectType::VerbContract, verb1),
            snap(ObjectType::VerbContract, verb2),
        ]);

        assert_eq!(graph.edges.len(), 2);

        // Forward: each verb maps to its edges
        assert_eq!(graph.verb_to_data["cbu.create"].len(), 1);
        assert_eq!(graph.verb_to_data["cbu.list"].len(), 1);

        // Reverse: table maps to both verbs
        let table_key = DataRef::Table(TableRef::new("ob-poc", "cbus")).index_key();
        let verb_indices = &graph.data_to_verb[&table_key];
        assert_eq!(verb_indices.len(), 2);
    }

    #[test]
    fn test_invalid_definition_skipped() {
        // A snapshot with object_type VerbContract but invalid definition
        let bad = snap(
            ObjectType::VerbContract,
            serde_json::json!({"not_a_verb": true}),
        );
        // Should not panic — just skip the bad row
        let graph = AffinityGraph::build(&[bad]);
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn test_multi_pass_integration() {
        // Build a graph with data from all 5 passes
        let verb = serde_json::to_value(VerbContractBody {
            fqn: "cbu.create".into(),
            domain: "cbu".into(),
            action: "create".into(),
            description: "Create".into(),
            behavior: "plugin".into(),
            args: vec![],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: Some(sem_os_ontology::verb_contract::VerbProducesSpec {
                entity_type: "cbu".into(),
                resolved: true,
            }),
            consumes: vec![],
            invocation_phrases: vec![],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: true,
            produces_focus: false,
            metadata: None,
            crud_mapping: Some(sem_os_ontology::verb_contract::VerbCrudMapping {
                operation: "insert".into(),
                table: Some("cbus".into()),
                schema: Some("ob-poc".into()),
                key_column: None,
                ..Default::default()
            }),
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        })
        .unwrap();

        let entity = serde_json::to_value(EntityTypeDefBody {
            fqn: "cbu".into(),
            name: "CBU".into(),
            description: "CBU".into(),
            domain: "cbu".into(),
            db_table: Some(sem_os_ontology::entity_type_def::DbTableMapping {
                schema: "ob-poc".into(),
                table: "cbus".into(),
                primary_key: "cbu_id".into(),
                name_column: None,
            }),
            lifecycle_states: vec![],
            required_attributes: vec!["cbu.name".into()],
            optional_attributes: vec![],
            parent_type: None,
            governance_tier: None,
            security_classification: None,
            pii: None,
            read_by_verbs: vec![],
            written_by_verbs: vec![],
        })
        .unwrap();

        let attr = serde_json::to_value(AttributeDefBody {
            fqn: "cbu.name".into(),
            name: "name".into(),
            description: "CBU name".into(),
            domain: "cbu".into(),
            data_type: sem_os_ontology::attribute_def::AttributeDataType::String,
            evidence_grade: sem_os_types::EvidenceGrade::None,
            source: Some(sem_os_ontology::attribute_def::AttributeSource {
                producing_verb: Some("cbu.create".into()),
                schema: Some("ob-poc".into()),
                table: Some("cbus".into()),
                column: Some("name".into()),
                derived: false,
            }),
            constraints: None,
            sinks: vec![],
            category: None,
            validation_rules: None,
            applicability: None,
            is_required: None,
            default_value: None,
            group_id: None,
            is_derived: None,
            derivation_spec_fqn: None,
            visibility: None,
        })
        .unwrap();

        let deriv = serde_json::to_value(DerivationSpecBody {
            fqn: "derived.total_aum".into(),
            name: "Total AUM".into(),
            description: "Sum".into(),
            output_attribute_fqn: "cbu.total_aum".into(),
            inputs: vec![DerivationInput {
                attribute_fqn: "holding.amount".into(),
                role: "addend".into(),
                required: true,
            }],
            expression: sem_os_ontology::derivation_spec::DerivationExpression::FunctionRef {
                ref_name: "sum".into(),
            },
            null_semantics: Default::default(),
            freshness_rule: None,
            security_inheritance: Default::default(),
            evidence_grade: Default::default(),
            tests: vec![],
        })
        .unwrap();

        let rel = serde_json::to_value(RelationshipTypeDefBody {
            fqn: "rel.parent_child".into(),
            name: "PC".into(),
            description: "PC".into(),
            domain: "entity".into(),
            source_entity_type_fqn: "entity.org".into(),
            target_entity_type_fqn: "entity.org".into(),
            cardinality: sem_os_ontology::relationship_type_def::RelationshipCardinality::OneToMany,
            edge_class: None,
            directionality: None,
            inverse_fqn: None,
            constraints: vec![],
        })
        .unwrap();

        let graph = AffinityGraph::build(&[
            snap(ObjectType::VerbContract, verb),
            snap(ObjectType::EntityTypeDef, entity),
            snap(ObjectType::AttributeDef, attr),
            snap(ObjectType::DerivationSpec, deriv),
            snap(ObjectType::RelationshipTypeDef, rel),
        ]);

        // Verb edges: Produces + CrudInsert + ProducesAttribute = 3
        assert_eq!(graph.edges.len(), 3);
        // Entity→table mapping
        assert!(graph.entity_to_table.contains_key("cbu"));
        assert!(graph.table_to_entity.contains_key("ob-poc:cbus"));
        // Attribute→column mapping
        assert!(graph.attribute_to_column.contains_key("cbu.name"));
        // Derivation edges
        assert_eq!(graph.derivation_edges.len(), 1);
        // Entity relationships
        assert_eq!(graph.entity_relationships.len(), 1);
        // Bidirectional index
        assert_eq!(graph.verb_to_data["cbu.create"].len(), 3);
    }
}
