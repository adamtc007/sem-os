//! Diagram enrichment — merges physical schema with AffinityGraph intelligence.
//!
//! The `build_diagram_model()` function takes raw table metadata and an
//! AffinityGraph and produces an enriched `DiagramModel` suitable for rendering.

use std::collections::{HashMap, HashSet};

use crate::affinity::{AffinityGraph, AffinityKind};
use crate::diagram::model::{
    DiagramAttribute, DiagramEntity, DiagramMetadata, DiagramModel, DiagramRelationship,
    GovernanceLevel, RelationshipKind, RenderOptions, TableInput, VerbSurfaceEntry,
};

/// Build an enriched diagram model from physical tables and an AffinityGraph.
///
/// Pipeline:
/// 1. For each table → resolve entity type via `graph.table_to_entity`
/// 2. Get verb surface via `graph.verbs_for_table()`
/// 3. Annotate columns with attribute FQNs via `graph.attribute_to_column`
/// 4. Classify governance level (Full/Partial/None)
/// 5. Build relationships (FK + entity-level)
/// 6. Apply filters from RenderOptions
/// 7. Sort deterministically
pub fn build_diagram_model(
    tables: &[TableInput],
    graph: &AffinityGraph,
    options: &RenderOptions,
) -> DiagramModel {
    let total_tables = tables.len();

    // Build reverse lookup: "schema:table:column" → attribute FQN
    let column_to_attribute: HashMap<String, String> = graph
        .attribute_to_column
        .iter()
        .map(|(fqn, col_ref)| {
            let key = format!("{}:{}:{}", col_ref.schema, col_ref.table, col_ref.column);
            (key, fqn.clone())
        })
        .collect();

    // Step 1-4: Build enriched entities
    let mut entities: Vec<DiagramEntity> = tables
        .iter()
        .map(|table| {
            let table_key = format!("{}:{}", table.schema, table.table_name);

            // Step 1: Resolve entity type
            let entity_type_fqn = graph.table_to_entity.get(&table_key).cloned();

            // Step 2: Get verb surface
            let verb_affinities = graph.verbs_for_table(&table.schema, &table.table_name);
            let verb_surface: Vec<VerbSurfaceEntry> = verb_affinities
                .iter()
                .map(|va| VerbSurfaceEntry {
                    verb_fqn: va.verb_fqn.clone(),
                    relation: affinity_kind_label(&va.affinity_kind),
                })
                .collect();

            // Step 3: Annotate columns
            let pk_set: HashSet<&str> = table.primary_keys.iter().map(|s| s.as_str()).collect();
            let fk_set: HashSet<&str> = table
                .foreign_keys
                .iter()
                .map(|fk| fk.from_column.as_str())
                .collect();

            let attributes: Vec<DiagramAttribute> = table
                .columns
                .iter()
                .map(|col| {
                    let col_key = format!("{}:{}:{}", table.schema, table.table_name, col.name);
                    let attribute_fqn = column_to_attribute.get(&col_key).cloned();

                    // Find producing/consuming verbs for this attribute
                    let (producing_verbs, consuming_verbs) = if let Some(ref fqn) = attribute_fqn {
                        let attr_verbs = graph.verbs_for_attribute(fqn);
                        let mut producing = Vec::new();
                        let mut consuming = Vec::new();
                        for va in &attr_verbs {
                            match va.affinity_kind {
                                AffinityKind::ProducesAttribute => {
                                    producing.push(va.verb_fqn.clone());
                                }
                                AffinityKind::ConsumesAttribute { .. } => {
                                    consuming.push(va.verb_fqn.clone());
                                }
                                _ => {}
                            }
                        }
                        (producing, consuming)
                    } else {
                        (Vec::new(), Vec::new())
                    };

                    DiagramAttribute {
                        name: col.name.clone(),
                        sql_type: col.sql_type.clone(),
                        is_nullable: col.is_nullable,
                        is_pk: pk_set.contains(col.name.as_str()),
                        is_fk: fk_set.contains(col.name.as_str()),
                        attribute_fqn,
                        producing_verbs,
                        consuming_verbs,
                    }
                })
                .collect();

            // Step 4: Classify governance level
            let has_entity = entity_type_fqn.is_some();
            let has_verbs = !verb_surface.is_empty();
            let governance_level = match (has_entity, has_verbs) {
                (true, true) => GovernanceLevel::Full,
                (true, false) | (false, true) => GovernanceLevel::Partial,
                (false, false) => GovernanceLevel::None,
            };

            DiagramEntity {
                schema: table.schema.clone(),
                table_name: table.table_name.clone(),
                entity_type_fqn,
                attributes,
                primary_keys: table.primary_keys.clone(),
                verb_surface,
                governance_level,
            }
        })
        .collect();

    // Step 5: Build relationships
    let mut relationships: Vec<DiagramRelationship> = Vec::new();

    // 5a: FK relationships from physical schema
    for table in tables {
        let from_entity = format!("{}:{}", table.schema, table.table_name);
        for fk in &table.foreign_keys {
            let to_entity = format!("{}:{}", fk.target_schema, fk.target_table);
            relationships.push(DiagramRelationship {
                from_entity: from_entity.clone(),
                to_entity,
                kind: RelationshipKind::ForeignKey,
                cardinality: Some("N:1".into()),
                fk_column: Some(fk.from_column.clone()),
            });
        }
    }

    // 5b: Entity-level relationships from AffinityGraph
    let included_entities: HashSet<String> = entities
        .iter()
        .filter_map(|e| e.entity_type_fqn.clone())
        .collect();

    for rel in &graph.entity_relationships {
        // Only include relationships where both entities are in the model
        if included_entities.contains(&rel.source_entity_type_fqn)
            && included_entities.contains(&rel.target_entity_type_fqn)
        {
            // Resolve entity FQNs to table keys for the relationship
            let from_entity = graph
                .entity_to_table
                .get(&rel.source_entity_type_fqn)
                .map(|t| t.key())
                .unwrap_or_else(|| rel.source_entity_type_fqn.clone());
            let to_entity = graph
                .entity_to_table
                .get(&rel.target_entity_type_fqn)
                .map(|t| t.key())
                .unwrap_or_else(|| rel.target_entity_type_fqn.clone());

            relationships.push(DiagramRelationship {
                from_entity,
                to_entity,
                kind: RelationshipKind::EntityRelationship,
                cardinality: Some(rel.cardinality.clone()),
                fk_column: None,
            });
        }
    }

    // Step 6: Apply filters
    if let Some(ref schema_filter) = options.schema_filter {
        let allowed: HashSet<&str> = schema_filter.iter().map(|s| s.as_str()).collect();
        entities.retain(|e| allowed.contains(e.schema.as_str()));
    }

    if let Some(ref domain_filter) = options.domain_filter {
        entities.retain(|e| e.table_name.starts_with(domain_filter.as_str()));
    }

    if let Some(max) = options.max_tables {
        entities.truncate(max);
    }

    // Filter relationships to only include entities still in the model
    let entity_keys: HashSet<String> = entities.iter().map(|e| e.sort_key()).collect();
    relationships
        .retain(|r| entity_keys.contains(&r.from_entity) && entity_keys.contains(&r.to_entity));

    // Step 7: Sort deterministically
    entities.sort_by_key(|a| a.sort_key());
    relationships
        .sort_by(|a, b| (&a.from_entity, &a.to_entity).cmp(&(&b.from_entity, &b.to_entity)));

    let included_tables = entities.len();
    let filtered = included_tables < total_tables;

    DiagramModel {
        entities,
        relationships,
        metadata: DiagramMetadata {
            total_tables,
            included_tables,
            filtered,
        },
    }
}

/// Human-readable label for an AffinityKind.
fn affinity_kind_label(kind: &AffinityKind) -> String {
    match kind {
        AffinityKind::Produces => "produces".into(),
        AffinityKind::Consumes => "consumes".into(),
        AffinityKind::CrudRead => "crud_read".into(),
        AffinityKind::CrudInsert => "crud_insert".into(),
        AffinityKind::CrudUpdate => "crud_update".into(),
        AffinityKind::CrudDelete => "crud_delete".into(),
        AffinityKind::ArgLookup { ref arg_name } => format!("arg_lookup:{arg_name}"),
        AffinityKind::ProducesAttribute => "produces_attribute".into(),
        AffinityKind::ConsumesAttribute { ref arg_name } => {
            format!("consumes_attribute:{arg_name}")
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affinity::{
        AffinityEdge, AffinityProvenance, ColumnRef, EntityRelationship, TableRef,
    };
    use crate::diagram::model::{ColumnInput, ForeignKeyInput};

    fn sample_graph() -> AffinityGraph {
        let mut graph = AffinityGraph {
            edges: vec![AffinityEdge {
                verb_fqn: "cbu.create".into(),
                data_ref: crate::affinity::DataRef::Table(TableRef::new("ob-poc", "cbus")),
                affinity_kind: AffinityKind::CrudInsert,
                provenance: AffinityProvenance::VerbCrudMapping,
            }],
            verb_to_data: HashMap::new(),
            data_to_verb: HashMap::new(),
            entity_to_table: HashMap::new(),
            table_to_entity: HashMap::new(),
            attribute_to_column: HashMap::new(),
            derivation_edges: vec![],
            entity_relationships: vec![],
            known_verbs: HashSet::new(),
        };
        graph.known_verbs.insert("cbu.create".into());

        // Entity↔table bimaps
        graph
            .entity_to_table
            .insert("cbu".into(), TableRef::new("ob-poc", "cbus"));
        graph
            .table_to_entity
            .insert("ob-poc:cbus".into(), "cbu".into());

        // Attribute↔column mapping
        graph
            .attribute_to_column
            .insert("cbu.name".into(), ColumnRef::new("ob-poc", "cbus", "name"));

        // Build indexes
        graph.verb_to_data.insert("cbu.create".into(), vec![0]);
        graph
            .data_to_verb
            .insert("table:ob-poc:cbus".into(), vec![0]);

        graph
    }

    fn sample_tables() -> Vec<TableInput> {
        vec![
            TableInput {
                schema: "ob-poc".into(),
                table_name: "cbus".into(),
                columns: vec![
                    ColumnInput {
                        name: "cbu_id".into(),
                        sql_type: "uuid".into(),
                        is_nullable: false,
                    },
                    ColumnInput {
                        name: "name".into(),
                        sql_type: "text".into(),
                        is_nullable: false,
                    },
                    ColumnInput {
                        name: "apex_entity_id".into(),
                        sql_type: "uuid".into(),
                        is_nullable: true,
                    },
                ],
                primary_keys: vec!["cbu_id".into()],
                foreign_keys: vec![ForeignKeyInput {
                    from_column: "apex_entity_id".into(),
                    target_schema: "ob-poc".into(),
                    target_table: "entities".into(),
                    target_column: "entity_id".into(),
                }],
            },
            TableInput {
                schema: "ob-poc".into(),
                table_name: "entities".into(),
                columns: vec![ColumnInput {
                    name: "entity_id".into(),
                    sql_type: "uuid".into(),
                    is_nullable: false,
                }],
                primary_keys: vec!["entity_id".into()],
                foreign_keys: vec![],
            },
        ]
    }

    #[test]
    fn enrichment_registered_entity() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        let cbu_entity = model
            .entities
            .iter()
            .find(|e| e.table_name == "cbus")
            .expect("cbus entity");

        assert_eq!(cbu_entity.entity_type_fqn, Some("cbu".into()));
        assert_eq!(cbu_entity.governance_level, GovernanceLevel::Full);
        assert!(!cbu_entity.verb_surface.is_empty());
    }

    #[test]
    fn enrichment_unregistered_table() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        let entity_table = model
            .entities
            .iter()
            .find(|e| e.table_name == "entities")
            .expect("entities table");

        assert_eq!(entity_table.entity_type_fqn, None);
        assert_eq!(entity_table.governance_level, GovernanceLevel::None);
        assert!(entity_table.verb_surface.is_empty());
    }

    #[test]
    fn enrichment_verb_surface_populated() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        let cbu_entity = model
            .entities
            .iter()
            .find(|e| e.table_name == "cbus")
            .expect("cbus entity");

        assert!(cbu_entity
            .verb_surface
            .iter()
            .any(|v| v.verb_fqn == "cbu.create"));
    }

    #[test]
    fn enrichment_attribute_fqn_populated() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        let cbu_entity = model
            .entities
            .iter()
            .find(|e| e.table_name == "cbus")
            .expect("cbus entity");

        let name_attr = cbu_entity
            .attributes
            .iter()
            .find(|a| a.name == "name")
            .expect("name attribute");

        assert_eq!(name_attr.attribute_fqn, Some("cbu.name".into()));
    }

    #[test]
    fn enrichment_pk_fk_flags() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        let cbu_entity = model
            .entities
            .iter()
            .find(|e| e.table_name == "cbus")
            .expect("cbus entity");

        let pk_col = cbu_entity
            .attributes
            .iter()
            .find(|a| a.name == "cbu_id")
            .expect("cbu_id");
        assert!(pk_col.is_pk);
        assert!(!pk_col.is_fk);

        let fk_col = cbu_entity
            .attributes
            .iter()
            .find(|a| a.name == "apex_entity_id")
            .expect("apex_entity_id");
        assert!(!fk_col.is_pk);
        assert!(fk_col.is_fk);
    }

    #[test]
    fn enrichment_fk_relationships() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        assert!(!model.relationships.is_empty());
        let fk_rel = model
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::ForeignKey)
            .expect("FK relationship");
        assert_eq!(fk_rel.from_entity, "ob-poc:cbus");
        assert_eq!(fk_rel.to_entity, "ob-poc:entities");
        assert_eq!(fk_rel.fk_column, Some("apex_entity_id".into()));
    }

    #[test]
    fn enrichment_entity_relationships() {
        let mut graph = sample_graph();
        // Add entity-level relationship
        graph
            .table_to_entity
            .insert("ob-poc:entities".into(), "entity".into());
        graph
            .entity_to_table
            .insert("entity".into(), TableRef::new("ob-poc", "entities"));
        graph.entity_relationships.push(EntityRelationship {
            fqn: "rel.cbu_apex".into(),
            source_entity_type_fqn: "cbu".into(),
            target_entity_type_fqn: "entity".into(),
            cardinality: "many_to_one".into(),
            edge_class: Some("ownership".into()),
            directionality: Some("forward".into()),
        });

        let tables = sample_tables();
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        let entity_rel = model
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::EntityRelationship)
            .expect("Entity relationship");
        assert_eq!(entity_rel.from_entity, "ob-poc:cbus");
        assert_eq!(entity_rel.to_entity, "ob-poc:entities");
    }

    #[test]
    fn enrichment_schema_filter() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions {
            schema_filter: Some(vec!["other-schema".into()]),
            ..RenderOptions::default()
        };

        let model = build_diagram_model(&tables, &graph, &options);

        assert!(model.entities.is_empty());
        assert!(model.metadata.filtered);
        assert_eq!(model.metadata.total_tables, 2);
        assert_eq!(model.metadata.included_tables, 0);
    }

    #[test]
    fn enrichment_max_tables() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions {
            max_tables: Some(1),
            ..RenderOptions::default()
        };

        let model = build_diagram_model(&tables, &graph, &options);

        assert_eq!(model.entities.len(), 1);
        assert!(model.metadata.filtered);
    }

    #[test]
    fn enrichment_deterministic_sort() {
        let graph = sample_graph();
        let tables = sample_tables();
        let options = RenderOptions::default();

        let model1 = build_diagram_model(&tables, &graph, &options);
        let model2 = build_diagram_model(&tables, &graph, &options);

        let keys1: Vec<_> = model1.entities.iter().map(|e| e.sort_key()).collect();
        let keys2: Vec<_> = model2.entities.iter().map(|e| e.sort_key()).collect();
        assert_eq!(keys1, keys2);
    }

    #[test]
    fn enrichment_empty_input() {
        let graph = sample_graph();
        let tables: Vec<TableInput> = vec![];
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        assert!(model.entities.is_empty());
        assert!(model.relationships.is_empty());
        assert_eq!(model.metadata.total_tables, 0);
        assert!(!model.metadata.filtered);
    }

    #[test]
    fn governance_level_classification() {
        let mut graph = sample_graph();
        // Add an entity type for "entities" table but no verbs
        graph
            .table_to_entity
            .insert("ob-poc:entities".into(), "entity".into());

        let tables = sample_tables();
        let options = RenderOptions::default();

        let model = build_diagram_model(&tables, &graph, &options);

        let cbu = model
            .entities
            .iter()
            .find(|e| e.table_name == "cbus")
            .unwrap();
        assert_eq!(cbu.governance_level, GovernanceLevel::Full);

        let ent = model
            .entities
            .iter()
            .find(|e| e.table_name == "entities")
            .unwrap();
        assert_eq!(ent.governance_level, GovernanceLevel::Partial);
    }
}
