//! MermaidRenderer — generates Mermaid diagram syntax from DiagramModel.
//!
//! Three render modes:
//! - `render_erd()` — erDiagram syntax (entity blocks + relationships)
//! - `render_verb_flow()` — graph LR syntax (verb → data asset flow)
//! - `render_domain_map()` — graph TD syntax (domain subgraphs)

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::affinity::discovery::DiscoveryResponse;
use crate::affinity::AffinityGraph;
use crate::diagram::model::{
    DiagramEntity, DiagramModel, GovernanceLevel, RelationshipKind, RenderOptions,
};

/// Sanitize a string for use as a Mermaid node ID.
/// Replaces non-alphanumeric characters with underscores.
pub fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── ERD Renderer ───────────────────────────────────────────────

/// Render an entity-relationship diagram in Mermaid `erDiagram` syntax.
///
/// Features:
/// - Entity blocks with columns (PK/FK annotated)
/// - Relationships with cardinality notation
/// - Governance level comments
/// - Verb surface annotations (when `show_verb_surface` is true)
pub fn render_erd(model: &DiagramModel, options: &RenderOptions) -> String {
    let mut out = String::with_capacity(4096);
    writeln!(out, "erDiagram").unwrap();

    // Render entities
    for entity in &model.entities {
        let id = sanitize_id(&entity.table_name);

        // Governance annotation as comment
        if options.show_governance {
            let gov = match entity.governance_level {
                GovernanceLevel::Full => "FULL",
                GovernanceLevel::Partial => "PARTIAL",
                GovernanceLevel::None => "NONE",
            };
            writeln!(out, "    %% Governance: {gov}").unwrap();
            if let Some(ref fqn) = entity.entity_type_fqn {
                writeln!(out, "    %% Entity type: {fqn}").unwrap();
            }
        }

        // Verb surface annotations
        if options.show_verb_surface && !entity.verb_surface.is_empty() {
            write!(out, "    %% Verbs:").unwrap();
            for vs in &entity.verb_surface {
                if options.show_affinity_kind {
                    write!(out, " {}({})", vs.verb_fqn, vs.relation).unwrap();
                } else {
                    write!(out, " {}", vs.verb_fqn).unwrap();
                }
            }
            writeln!(out).unwrap();
        }

        writeln!(out, "    {id} {{").unwrap();

        if options.include_columns {
            for attr in &entity.attributes {
                let sql_type = sanitize_id(&attr.sql_type);
                let mut markers = Vec::new();
                if attr.is_pk {
                    markers.push("PK");
                }
                if attr.is_fk {
                    markers.push("FK");
                }
                let marker_str = if markers.is_empty() {
                    String::new()
                } else {
                    format!(" \"{}\"", markers.join(","))
                };
                writeln!(out, "        {sql_type} {}{marker_str}", attr.name).unwrap();
            }
        }

        writeln!(out, "    }}").unwrap();
    }

    // Render relationships
    for rel in &model.relationships {
        let from_id = sanitize_id(table_name_from_key(&rel.from_entity));
        let to_id = sanitize_id(table_name_from_key(&rel.to_entity));

        let arrow = match rel.kind {
            RelationshipKind::ForeignKey => {
                match rel.cardinality.as_deref() {
                    Some("1:1") => "||--||",
                    Some("1:N") | Some("N:1") => "||--o{",
                    Some("M:N") => "}o--o{",
                    _ => "||--o{", // Default FK cardinality
                }
            }
            RelationshipKind::EntityRelationship => "..|>",
        };

        let label = match (&rel.fk_column, &rel.kind) {
            (Some(col), RelationshipKind::ForeignKey) => col.clone(),
            (_, RelationshipKind::EntityRelationship) => {
                rel.cardinality.clone().unwrap_or_default()
            }
            _ => String::new(),
        };

        writeln!(out, "    {from_id} {arrow} {to_id} : \"{label}\"").unwrap();
    }

    out
}

// ── Verb Flow Renderer ─────────────────────────────────────────

/// Render a verb-centric data flow diagram in Mermaid `graph LR` syntax.
///
/// Shows a center verb node connected to all data assets it touches,
/// with edges labeled by AffinityKind. When `depth > 1`, shows
/// adjacent verbs sharing the same data assets.
pub fn render_verb_flow(verb_fqn: &str, graph: &AffinityGraph, depth: u32) -> String {
    let mut out = String::with_capacity(2048);
    writeln!(out, "graph LR").unwrap();

    let verb_id = sanitize_id(verb_fqn);
    writeln!(out, "    {verb_id}[\"{verb_fqn}\"]").unwrap();

    // Direct data assets
    let data_assets = graph.data_for_verb(verb_fqn);
    let mut data_nodes: BTreeSet<String> = BTreeSet::new();

    for da in &data_assets {
        let data_key = da.data_ref.index_key();
        let data_id = sanitize_id(&data_key);
        let label = data_ref_label(&da.data_ref);

        if data_nodes.insert(data_key.clone()) {
            writeln!(out, "    {data_id}[(\"{label}\")]").unwrap();
        }

        let edge_label = affinity_kind_short(&da.affinity_kind);
        writeln!(out, "    {verb_id} -->|{edge_label}| {data_id}").unwrap();
    }

    // Adjacent verbs at depth > 1
    if depth > 1 {
        let adjacent = graph.adjacent_verbs(verb_fqn);
        for (adj_verb, shared_refs) in &adjacent {
            let adj_id = sanitize_id(adj_verb);
            writeln!(out, "    {adj_id}[\"{adj_verb}\"]").unwrap();

            for data_ref in shared_refs {
                let data_key = data_ref.index_key();
                let data_id = sanitize_id(&data_key);
                let label = data_ref_label(data_ref);

                if data_nodes.insert(data_key.clone()) {
                    writeln!(out, "    {data_id}[(\"{label}\")]").unwrap();
                }

                writeln!(out, "    {adj_id} -.-> {data_id}").unwrap();
            }
        }
    }

    out
}

// ── Domain Map Renderer ────────────────────────────────────────

/// Render a domain map diagram in Mermaid `graph TD` syntax.
///
/// Groups tables by domain prefix into subgraphs, with cross-domain
/// FK edges shown as connections between subgraphs.
pub fn render_domain_map(model: &DiagramModel, _options: &RenderOptions) -> String {
    let mut out = String::with_capacity(2048);
    writeln!(out, "graph TD").unwrap();

    // Group entities by domain (prefix before first underscore or dot)
    let mut domains: BTreeMap<String, Vec<&DiagramEntity>> = BTreeMap::new();
    for entity in &model.entities {
        let domain = extract_domain(&entity.table_name);
        domains.entry(domain).or_default().push(entity);
    }

    // Render subgraphs
    for (domain, entities) in &domains {
        let domain_id = sanitize_id(domain);
        writeln!(out, "    subgraph {domain_id}[\"{domain}\"]").unwrap();

        for entity in entities {
            let entity_id = sanitize_id(&entity.sort_key());
            let verb_count = entity.verb_surface.len();
            let badge = if verb_count > 0 {
                format!(" ({verb_count}v)")
            } else {
                String::new()
            };
            writeln!(out, "        {entity_id}[\"{}{badge}\"]", entity.table_name).unwrap();
        }

        writeln!(out, "    end").unwrap();
    }

    // Cross-domain edges
    for rel in &model.relationships {
        if rel.kind == RelationshipKind::ForeignKey {
            let from_id = sanitize_id(&rel.from_entity);
            let to_id = sanitize_id(&rel.to_entity);
            let label = rel.fk_column.as_deref().unwrap_or("");
            writeln!(out, "    {from_id} -->|{label}| {to_id}").unwrap();
        }
    }

    out
}

// ── Discovery Map Renderer ───────────────────────────────────

/// Render an utterance -> intent -> verb -> data discovery map in Mermaid `graph TD` syntax.
pub fn render_discovery_map(
    utterance: &str,
    discovery: &DiscoveryResponse,
    graph: &AffinityGraph,
) -> String {
    let mut out = String::with_capacity(4096);
    writeln!(out, "graph TD").unwrap();

    let utterance_id = sanitize_id(&format!("utterance:{utterance}"));
    writeln!(out, "    {utterance_id}[\"Utterance: {utterance}\"]").unwrap();

    for intent in &discovery.intent_matches {
        let intent_id = sanitize_id(&format!("intent:{}:{}", intent.verb, intent.score));
        writeln!(
            out,
            "    {intent_id}[\"Intent: {} ({:.2})\"]",
            intent.verb, intent.score
        )
        .unwrap();
        writeln!(out, "    {utterance_id} --> {intent_id}").unwrap();

        let verb_id = sanitize_id(&format!("verb:{}", intent.verb));
        writeln!(out, "    {verb_id}[\"{}\"]", intent.verb).unwrap();
        writeln!(out, "    {intent_id} --> {verb_id}").unwrap();

        let mut seen_data = BTreeSet::new();
        for data in graph.data_for_verb(&intent.verb) {
            let key = data.data_ref.index_key();
            if !seen_data.insert(key.clone()) {
                continue;
            }
            let data_id = sanitize_id(&format!("data:{key}"));
            let label = data_ref_label(&data.data_ref);
            writeln!(out, "    {data_id}[(\"{label}\")]").unwrap();
            writeln!(out, "    {verb_id} --> {data_id}").unwrap();
        }
    }

    out
}

// ── Helpers ────────────────────────────────────────────────────

/// Extract table name from a "schema:table" key.
fn table_name_from_key(key: &str) -> &str {
    key.split(':').nth(1).unwrap_or(key)
}

/// Extract domain prefix from a table name (e.g., "cbus" → "cbu", "kyc_cases" → "kyc").
fn extract_domain(table_name: &str) -> String {
    // Try underscore split first, then use full name
    if let Some(pos) = table_name.find('_') {
        table_name[..pos].to_string()
    } else {
        // Strip trailing 's' for plurals (cbus → cbu)
        if table_name.ends_with('s') && table_name.len() > 1 {
            table_name[..table_name.len() - 1].to_string()
        } else {
            table_name.to_string()
        }
    }
}

/// Short label for a DataRef.
fn data_ref_label(data_ref: &crate::affinity::DataRef) -> String {
    use crate::affinity::DataRef;
    match data_ref {
        DataRef::Table(t) => format!("{}:{}", t.schema, t.table),
        DataRef::Column(c) => format!("{}.{}", c.table, c.column),
        DataRef::EntityType { fqn } => format!("entity:{fqn}"),
        DataRef::Attribute { fqn } => format!("attr:{fqn}"),
    }
}

/// Short label for an AffinityKind (for edge labels).
fn affinity_kind_short(kind: &crate::affinity::AffinityKind) -> String {
    use crate::affinity::AffinityKind;
    match kind {
        AffinityKind::Produces => "produces".into(),
        AffinityKind::Consumes => "consumes".into(),
        AffinityKind::CrudRead => "reads".into(),
        AffinityKind::CrudInsert => "inserts".into(),
        AffinityKind::CrudUpdate => "updates".into(),
        AffinityKind::CrudDelete => "deletes".into(),
        AffinityKind::ArgLookup { .. } => "lookup".into(),
        AffinityKind::ProducesAttribute => "produces_attr".into(),
        AffinityKind::ConsumesAttribute { .. } => "consumes_attr".into(),
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affinity::{AffinityEdge, AffinityKind, AffinityProvenance, DataRef, TableRef};
    use crate::diagram::model::{DiagramMetadata, DiagramRelationship, VerbSurfaceEntry};
    use std::collections::{HashMap, HashSet};

    fn sample_model() -> DiagramModel {
        DiagramModel {
            entities: vec![
                DiagramEntity {
                    schema: "ob-poc".into(),
                    table_name: "cbus".into(),
                    entity_type_fqn: Some("cbu".into()),
                    attributes: vec![
                        crate::diagram::model::DiagramAttribute {
                            name: "cbu_id".into(),
                            sql_type: "uuid".into(),
                            is_nullable: false,
                            is_pk: true,
                            is_fk: false,
                            attribute_fqn: None,
                            producing_verbs: vec![],
                            consuming_verbs: vec![],
                        },
                        crate::diagram::model::DiagramAttribute {
                            name: "name".into(),
                            sql_type: "text".into(),
                            is_nullable: false,
                            is_pk: false,
                            is_fk: false,
                            attribute_fqn: Some("cbu.name".into()),
                            producing_verbs: vec!["cbu.create".into()],
                            consuming_verbs: vec![],
                        },
                    ],
                    primary_keys: vec!["cbu_id".into()],
                    verb_surface: vec![VerbSurfaceEntry {
                        verb_fqn: "cbu.create".into(),
                        relation: "crud_insert".into(),
                    }],
                    governance_level: GovernanceLevel::Full,
                },
                DiagramEntity {
                    schema: "ob-poc".into(),
                    table_name: "entities".into(),
                    entity_type_fqn: None,
                    attributes: vec![],
                    primary_keys: vec!["entity_id".into()],
                    verb_surface: vec![],
                    governance_level: GovernanceLevel::None,
                },
            ],
            relationships: vec![DiagramRelationship {
                from_entity: "ob-poc:cbus".into(),
                to_entity: "ob-poc:entities".into(),
                kind: RelationshipKind::ForeignKey,
                cardinality: Some("N:1".into()),
                fk_column: Some("apex_entity_id".into()),
            }],
            metadata: DiagramMetadata {
                total_tables: 2,
                included_tables: 2,
                filtered: false,
            },
        }
    }

    fn sample_graph() -> AffinityGraph {
        let mut graph = AffinityGraph {
            edges: vec![AffinityEdge {
                verb_fqn: "cbu.create".into(),
                data_ref: DataRef::Table(TableRef::new("ob-poc", "cbus")),
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
        graph.verb_to_data.insert("cbu.create".into(), vec![0]);
        graph
            .data_to_verb
            .insert("table:ob-poc:cbus".into(), vec![0]);
        graph.known_verbs.insert("cbu.create".into());
        graph
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize_id("ob-poc"), "ob_poc");
        assert_eq!(sanitize_id("schema:table"), "schema_table");
        assert_eq!(sanitize_id("simple"), "simple");
        assert_eq!(sanitize_id("has.dot"), "has_dot");
    }

    #[test]
    fn erd_basic() {
        let model = sample_model();
        let options = RenderOptions {
            include_columns: true,
            ..RenderOptions::default()
        };

        let output = render_erd(&model, &options);

        assert!(output.starts_with("erDiagram\n"));
        assert!(output.contains("cbus {"));
        assert!(output.contains("entities {"));
        assert!(output.contains("uuid cbu_id \"PK\""));
        assert!(output.contains("text name"));
    }

    #[test]
    fn erd_with_governance() {
        let model = sample_model();
        let options = RenderOptions {
            show_governance: true,
            ..RenderOptions::default()
        };

        let output = render_erd(&model, &options);

        assert!(output.contains("Governance: FULL"));
        assert!(output.contains("Entity type: cbu"));
        assert!(output.contains("Governance: NONE"));
    }

    #[test]
    fn erd_with_verbs() {
        let model = sample_model();
        let options = RenderOptions {
            show_verb_surface: true,
            ..RenderOptions::default()
        };

        let output = render_erd(&model, &options);

        assert!(output.contains("Verbs: cbu.create"));
    }

    #[test]
    fn erd_relationships() {
        let model = sample_model();
        let options = RenderOptions::default();

        let output = render_erd(&model, &options);

        assert!(output.contains("cbus ||--o{ entities"));
        assert!(output.contains("apex_entity_id"));
    }

    #[test]
    fn verb_flow_basic() {
        let graph = sample_graph();

        let output = render_verb_flow("cbu.create", &graph, 1);

        assert!(output.starts_with("graph LR\n"));
        assert!(output.contains("cbu_create[\"cbu.create\"]"));
        assert!(output.contains("inserts"));
    }

    #[test]
    fn domain_map_basic() {
        let model = sample_model();
        let options = RenderOptions::default();

        let output = render_domain_map(&model, &options);

        assert!(output.starts_with("graph TD\n"));
        assert!(output.contains("subgraph"));
        assert!(output.contains("end"));
    }

    #[test]
    fn deterministic_output() {
        let model = sample_model();
        let options = RenderOptions {
            include_columns: true,
            show_governance: true,
            show_verb_surface: true,
            ..RenderOptions::default()
        };

        let output1 = render_erd(&model, &options);
        let output2 = render_erd(&model, &options);
        assert_eq!(output1, output2);
    }

    #[test]
    fn empty_model() {
        let model = DiagramModel::empty();
        let options = RenderOptions::default();

        let erd = render_erd(&model, &options);
        assert!(erd.starts_with("erDiagram\n"));
        // Should have just the header, no entities
        assert_eq!(erd.trim(), "erDiagram");

        let dm = render_domain_map(&model, &options);
        assert!(dm.starts_with("graph TD\n"));
    }

    #[test]
    fn extract_domain_works() {
        assert_eq!(extract_domain("cbus"), "cbu");
        assert_eq!(extract_domain("kyc_cases"), "kyc");
        assert_eq!(extract_domain("entities"), "entitie");
        assert_eq!(extract_domain("trading_profiles"), "trading");
    }

    #[test]
    fn table_name_from_key_works() {
        assert_eq!(table_name_from_key("ob-poc:cbus"), "cbus");
        assert_eq!(table_name_from_key("schema:table"), "table");
        assert_eq!(table_name_from_key("nodelim"), "nodelim");
    }
}
