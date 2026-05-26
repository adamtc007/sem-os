//! AffinityGraph query methods — 10 navigation and governance queries.
//!
//! All methods are pure lookups against the pre-computed graph. No DB access.

use std::collections::{HashMap, HashSet};

use super::types::*;

// ── 6 Core Queries ─────────────────────────────────────────────

impl AffinityGraph {
    /// Find all verbs that read/write/produce/consume a given table.
    pub fn verbs_for_table(&self, schema: &str, table: &str) -> Vec<VerbAffinity> {
        let key = format!("table:{schema}:{table}");
        self.verbs_for_data_key(&key)
    }

    /// Find all verbs that produce or consume a given attribute.
    pub fn verbs_for_attribute(&self, attr_fqn: &str) -> Vec<VerbAffinity> {
        let key = format!("attribute:{attr_fqn}");
        self.verbs_for_data_key(&key)
    }

    /// Find all verbs operating on a given entity type (via entity→table→verbs + direct edges).
    pub fn verbs_for_entity_type(&self, entity_fqn: &str) -> Vec<VerbAffinity> {
        let mut results = Vec::new();
        let mut seen = HashSet::new();

        // Direct entity type edges
        let direct_key = format!("entity_type:{entity_fqn}");
        for va in self.verbs_for_data_key(&direct_key) {
            let dedup = format!("{}:{:?}", va.verb_fqn, va.affinity_kind);
            if seen.insert(dedup) {
                results.push(va);
            }
        }

        // Transitive: entity → table → verbs
        if let Some(table_ref) = self.entity_to_table.get(entity_fqn) {
            let table_key = format!("table:{}:{}", table_ref.schema, table_ref.table);
            for va in self.verbs_for_data_key(&table_key) {
                let dedup = format!("{}:{:?}", va.verb_fqn, va.affinity_kind);
                if seen.insert(dedup) {
                    results.push(va);
                }
            }
        }

        results
    }

    /// Find all data assets a verb touches (tables, columns, attributes, entities).
    pub fn data_for_verb(&self, verb_fqn: &str) -> Vec<DataAffinity> {
        let Some(edge_indices) = self.verb_to_data.get(verb_fqn) else {
            return Vec::new();
        };
        edge_indices
            .iter()
            .filter_map(|&idx| self.edges.get(idx))
            .map(|edge| DataAffinity {
                data_ref: edge.data_ref.clone(),
                affinity_kind: edge.affinity_kind.clone(),
                provenance: edge.provenance.clone(),
            })
            .collect()
    }

    /// Transitive data footprint: collect all data assets a verb touches,
    /// following arg lookups up to `depth` hops.
    pub fn data_footprint(&self, verb_fqn: &str, depth: u32) -> DataFootprint {
        let mut footprint = DataFootprint::default();
        let mut visited_verbs = HashSet::new();
        self.collect_footprint(verb_fqn, depth, &mut footprint, &mut visited_verbs);
        footprint
    }

    /// Find verbs sharing data dependencies (same table/attribute) with the given verb.
    pub fn adjacent_verbs(&self, verb_fqn: &str) -> Vec<(String, Vec<DataRef>)> {
        // Collect all data ref keys this verb touches
        let Some(edge_indices) = self.verb_to_data.get(verb_fqn) else {
            return Vec::new();
        };

        let my_data_keys: HashSet<String> = edge_indices
            .iter()
            .filter_map(|&idx| self.edges.get(idx))
            .map(|edge| edge.data_ref.index_key())
            .collect();

        // For each data key, find other verbs that also touch it
        let mut neighbor_data: HashMap<String, Vec<DataRef>> = HashMap::new();
        for data_key in &my_data_keys {
            if let Some(verb_indices) = self.data_to_verb.get(data_key) {
                for &idx in verb_indices {
                    if let Some(edge) = self.edges.get(idx) {
                        if edge.verb_fqn != verb_fqn {
                            neighbor_data
                                .entry(edge.verb_fqn.clone())
                                .or_default()
                                .push(edge.data_ref.clone());
                        }
                    }
                }
            }
        }

        // Deduplicate data refs per neighbor
        let mut results: Vec<(String, Vec<DataRef>)> = neighbor_data
            .into_iter()
            .map(|(verb, refs)| {
                let mut unique_keys = HashSet::new();
                let deduped: Vec<DataRef> = refs
                    .into_iter()
                    .filter(|r| unique_keys.insert(r.index_key()))
                    .collect();
                (verb, deduped)
            })
            .collect();
        results.sort_by(|a, b| a.0.cmp(&b.0));
        results
    }
}

// ── 4 Governance Queries ───────────────────────────────────────

impl AffinityGraph {
    /// Tables with no verb affinity (ungoverned data).
    ///
    /// Takes `known_tables` as parameter — the caller provides the physical
    /// schema tables (e.g. from `extract_schema()`). The AffinityGraph itself
    /// doesn't know all physical tables.
    pub fn orphan_tables(&self, known_tables: &[TableRef]) -> Vec<TableRef> {
        known_tables
            .iter()
            .filter(|t| {
                let key = format!("table:{}:{}", t.schema, t.table);
                !self.data_to_verb.contains_key(&key)
            })
            .cloned()
            .collect()
    }

    /// Verbs with no data affinity (disconnected operations).
    pub fn orphan_verbs(&self) -> Vec<String> {
        let verbs_with_data: HashSet<&str> = self
            .verb_to_data
            .iter()
            .filter(|(_, edges)| !edges.is_empty())
            .map(|(verb, _)| verb.as_str())
            .collect();

        let mut orphaned: Vec<String> = self
            .known_verbs
            .iter()
            .filter(|v| !verbs_with_data.contains(v.as_str()))
            .cloned()
            .collect();
        orphaned.sort();
        orphaned
    }

    /// Attributes with source but no sinks (written but never read).
    pub fn write_only_attributes(&self) -> Vec<String> {
        // Collect attributes that are produced (have ProducesAttribute edges)
        let mut produced: HashSet<String> = HashSet::new();
        let mut consumed: HashSet<String> = HashSet::new();

        for edge in &self.edges {
            match (&edge.data_ref, &edge.affinity_kind) {
                (DataRef::Attribute { fqn }, AffinityKind::ProducesAttribute) => {
                    produced.insert(fqn.clone());
                }
                (DataRef::Attribute { fqn }, AffinityKind::ConsumesAttribute { .. }) => {
                    consumed.insert(fqn.clone());
                }
                _ => {}
            }
        }

        let mut result: Vec<String> = produced.difference(&consumed).cloned().collect();
        result.sort();
        result
    }

    /// Attributes with sinks but no source (read before written).
    pub fn read_before_write_attributes(&self) -> Vec<String> {
        let mut produced: HashSet<String> = HashSet::new();
        let mut consumed: HashSet<String> = HashSet::new();

        for edge in &self.edges {
            match (&edge.data_ref, &edge.affinity_kind) {
                (DataRef::Attribute { fqn }, AffinityKind::ProducesAttribute) => {
                    produced.insert(fqn.clone());
                }
                (DataRef::Attribute { fqn }, AffinityKind::ConsumesAttribute { .. }) => {
                    consumed.insert(fqn.clone());
                }
                _ => {}
            }
        }

        let mut result: Vec<String> = consumed.difference(&produced).cloned().collect();
        result.sort();
        result
    }
}

// ── Private helpers ────────────────────────────────────────────

impl AffinityGraph {
    /// Look up verbs for a given data index key.
    fn verbs_for_data_key(&self, key: &str) -> Vec<VerbAffinity> {
        let Some(edge_indices) = self.data_to_verb.get(key) else {
            return Vec::new();
        };
        edge_indices
            .iter()
            .filter_map(|&idx| self.edges.get(idx))
            .map(|edge| VerbAffinity {
                verb_fqn: edge.verb_fqn.clone(),
                affinity_kind: edge.affinity_kind.clone(),
                provenance: edge.provenance.clone(),
            })
            .collect()
    }

    /// Recursively collect data footprint, following arg lookups.
    fn collect_footprint(
        &self,
        verb_fqn: &str,
        remaining_depth: u32,
        footprint: &mut DataFootprint,
        visited_verbs: &mut HashSet<String>,
    ) {
        if !visited_verbs.insert(verb_fqn.to_string()) {
            return; // Already visited — avoid cycles.
        }

        let Some(edge_indices) = self.verb_to_data.get(verb_fqn) else {
            return;
        };

        for &idx in edge_indices {
            let Some(edge) = self.edges.get(idx) else {
                continue;
            };

            match &edge.data_ref {
                DataRef::Table(t) => {
                    footprint.tables.insert(t.key(), t.clone());
                }
                DataRef::Column(c) => {
                    let key = format!("{}:{}:{}", c.schema, c.table, c.column);
                    footprint.columns.insert(key, c.clone());
                }
                DataRef::Attribute { fqn } => {
                    footprint.attributes.insert(fqn.clone());
                }
                DataRef::EntityType { fqn } => {
                    footprint.entity_types.insert(fqn.clone());
                    // Also include the entity's table if known
                    if let Some(table_ref) = self.entity_to_table.get(fqn) {
                        footprint.tables.insert(table_ref.key(), table_ref.clone());
                    }
                }
            }

            // Follow arg lookups transitively at depth > 1
            if remaining_depth > 1 {
                if let AffinityKind::ArgLookup { .. } = &edge.affinity_kind {
                    // Find verbs that write to the same data asset
                    let data_key = edge.data_ref.index_key();
                    if let Some(writer_indices) = self.data_to_verb.get(&data_key) {
                        for &widx in writer_indices {
                            if let Some(writer_edge) = self.edges.get(widx) {
                                if writer_edge.verb_fqn != verb_fqn {
                                    self.collect_footprint(
                                        &writer_edge.verb_fqn,
                                        remaining_depth - 1,
                                        footprint,
                                        visited_verbs,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Build a test graph with known edges for query testing.
    fn test_graph() -> AffinityGraph {
        let edges = vec![
            // cbu.create → Produces EntityType(cbu) + CrudInsert Table(ob-poc, cbus)
            AffinityEdge {
                verb_fqn: "cbu.create".into(),
                data_ref: DataRef::EntityType { fqn: "cbu".into() },
                affinity_kind: AffinityKind::Produces,
                provenance: AffinityProvenance::VerbProduces,
            },
            AffinityEdge {
                verb_fqn: "cbu.create".into(),
                data_ref: DataRef::Table(TableRef::new("ob-poc", "cbus")),
                affinity_kind: AffinityKind::CrudInsert,
                provenance: AffinityProvenance::VerbCrudMapping,
            },
            // cbu.list → CrudRead Table(ob-poc, cbus)
            AffinityEdge {
                verb_fqn: "cbu.list".into(),
                data_ref: DataRef::Table(TableRef::new("ob-poc", "cbus")),
                affinity_kind: AffinityKind::CrudRead,
                provenance: AffinityProvenance::VerbCrudMapping,
            },
            // cbu.create → ArgLookup Table(ob-poc, entities)
            AffinityEdge {
                verb_fqn: "cbu.create".into(),
                data_ref: DataRef::Table(TableRef::new("ob-poc", "entities")),
                affinity_kind: AffinityKind::ArgLookup {
                    arg_name: "depositary".into(),
                },
                provenance: AffinityProvenance::VerbArgLookup,
            },
            // entity.create → CrudInsert Table(ob-poc, entities)
            AffinityEdge {
                verb_fqn: "entity.create".into(),
                data_ref: DataRef::Table(TableRef::new("ob-poc", "entities")),
                affinity_kind: AffinityKind::CrudInsert,
                provenance: AffinityProvenance::VerbCrudMapping,
            },
            // cbu.create → ProducesAttribute cbu.name
            AffinityEdge {
                verb_fqn: "cbu.create".into(),
                data_ref: DataRef::Attribute {
                    fqn: "cbu.name".into(),
                },
                affinity_kind: AffinityKind::ProducesAttribute,
                provenance: AffinityProvenance::AttributeSource,
            },
            // cbu.list → ConsumesAttribute cbu.name
            AffinityEdge {
                verb_fqn: "cbu.list".into(),
                data_ref: DataRef::Attribute {
                    fqn: "cbu.name".into(),
                },
                affinity_kind: AffinityKind::ConsumesAttribute {
                    arg_name: "filter".into(),
                },
                provenance: AffinityProvenance::AttributeSink,
            },
            // cbu.create → ProducesAttribute cbu.jurisdiction_code (write-only: no consumer)
            AffinityEdge {
                verb_fqn: "cbu.create".into(),
                data_ref: DataRef::Attribute {
                    fqn: "cbu.jurisdiction_code".into(),
                },
                affinity_kind: AffinityKind::ProducesAttribute,
                provenance: AffinityProvenance::AttributeSource,
            },
            // cbu.read → ConsumesAttribute cbu.client_label (read-before-write: no producer)
            AffinityEdge {
                verb_fqn: "cbu.read".into(),
                data_ref: DataRef::Attribute {
                    fqn: "cbu.client_label".into(),
                },
                affinity_kind: AffinityKind::ConsumesAttribute {
                    arg_name: "label".into(),
                },
                provenance: AffinityProvenance::AttributeSink,
            },
        ];

        // Build bidirectional indexes
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

        let mut entity_to_table = HashMap::new();
        entity_to_table.insert("cbu".to_string(), TableRef::new("ob-poc", "cbus"));

        let mut table_to_entity = HashMap::new();
        table_to_entity.insert("ob-poc:cbus".to_string(), "cbu".to_string());

        AffinityGraph {
            edges,
            verb_to_data,
            data_to_verb,
            entity_to_table,
            table_to_entity,
            attribute_to_column: HashMap::new(),
            derivation_edges: vec![],
            entity_relationships: vec![],
            known_verbs: HashSet::new(),
        }
    }

    // ── Core query tests ───────────────────────────────────────

    #[test]
    fn test_verbs_for_table_finds_crud() {
        let graph = test_graph();
        let verbs = graph.verbs_for_table("ob-poc", "cbus");
        let fqns: Vec<&str> = verbs.iter().map(|v| v.verb_fqn.as_str()).collect();
        assert!(fqns.contains(&"cbu.create"), "should find insert verb");
        assert!(fqns.contains(&"cbu.list"), "should find read verb");
    }

    #[test]
    fn test_verbs_for_table_finds_lookup() {
        let graph = test_graph();
        let verbs = graph.verbs_for_table("ob-poc", "entities");
        let fqns: Vec<&str> = verbs.iter().map(|v| v.verb_fqn.as_str()).collect();
        assert!(fqns.contains(&"cbu.create"), "should find arg lookup verb");
        assert!(fqns.contains(&"entity.create"), "should find insert verb");
    }

    #[test]
    fn test_verbs_for_attribute_both_directions() {
        let graph = test_graph();
        let verbs = graph.verbs_for_attribute("cbu.name");
        let fqns: Vec<&str> = verbs.iter().map(|v| v.verb_fqn.as_str()).collect();
        assert!(fqns.contains(&"cbu.create"), "should find producer");
        assert!(fqns.contains(&"cbu.list"), "should find consumer");
    }

    #[test]
    fn test_verbs_for_entity_type_via_table() {
        let graph = test_graph();
        let verbs = graph.verbs_for_entity_type("cbu");
        let fqns: Vec<&str> = verbs.iter().map(|v| v.verb_fqn.as_str()).collect();
        // Should find both direct entity type edges AND transitive table edges
        assert!(fqns.contains(&"cbu.create"), "via Produces + CrudInsert");
        assert!(fqns.contains(&"cbu.list"), "via CrudRead on cbus table");
    }

    #[test]
    fn test_data_for_verb() {
        let graph = test_graph();
        let data = graph.data_for_verb("cbu.create");
        assert!(
            data.len() >= 3,
            "cbu.create touches entity, table, lookup, attrs"
        );

        let has_entity = data
            .iter()
            .any(|d| matches!(&d.data_ref, DataRef::EntityType { fqn } if fqn == "cbu"));
        assert!(has_entity, "should include entity type");

        let has_table = data.iter().any(|d| {
            matches!(&d.data_ref, DataRef::Table(t) if t.schema == "ob-poc" && t.table == "cbus")
        });
        assert!(has_table, "should include table");
    }

    #[test]
    fn test_data_footprint_depth_1() {
        let graph = test_graph();
        let fp = graph.data_footprint("cbu.create", 1);
        // At depth 1, just direct assets
        assert!(fp.tables.contains_key("ob-poc:cbus"));
        assert!(fp.tables.contains_key("ob-poc:entities"));
        assert!(fp.entity_types.contains("cbu"));
        assert!(fp.attributes.contains("cbu.name"));
    }

    #[test]
    fn test_data_footprint_depth_2() {
        let graph = test_graph();
        let fp = graph.data_footprint("cbu.create", 2);
        // At depth 2, follows arg lookup on entities → entity.create's data
        // entity.create writes to entities table (already included)
        assert!(fp.tables.contains_key("ob-poc:entities"));
        assert!(fp.tables.contains_key("ob-poc:cbus"));
    }

    #[test]
    fn test_adjacent_verbs_shared_table() {
        let graph = test_graph();
        let adjacent = graph.adjacent_verbs("cbu.create");
        let neighbor_fqns: Vec<&str> = adjacent.iter().map(|(v, _)| v.as_str()).collect();

        assert!(
            neighbor_fqns.contains(&"cbu.list"),
            "cbu.list shares cbus table"
        );
        assert!(
            neighbor_fqns.contains(&"entity.create"),
            "entity.create shares entities table"
        );

        // Check shared data refs
        let cbu_list_entry = adjacent.iter().find(|(v, _)| v == "cbu.list").unwrap();
        let shared_refs: Vec<String> = cbu_list_entry.1.iter().map(|r| r.index_key()).collect();
        assert!(shared_refs.contains(&"table:ob-poc:cbus".to_string()));
    }

    // ── Governance query tests ─────────────────────────────────

    #[test]
    fn test_orphan_tables() {
        let graph = test_graph();
        let known = vec![
            TableRef::new("ob-poc", "cbus"),
            TableRef::new("ob-poc", "entities"),
            TableRef::new("ob-poc", "holdings"), // No verb touches this
        ];
        let orphans = graph.orphan_tables(&known);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].table, "holdings");
    }

    #[test]
    fn test_write_only_attributes() {
        let graph = test_graph();
        let write_only = graph.write_only_attributes();
        assert!(
            write_only.contains(&"cbu.jurisdiction_code".to_string()),
            "jurisdiction_code is produced but never consumed"
        );
        assert!(
            !write_only.contains(&"cbu.name".to_string()),
            "cbu.name has both producer and consumer"
        );
    }

    #[test]
    fn test_read_before_write_attributes() {
        let graph = test_graph();
        let rbw = graph.read_before_write_attributes();
        assert!(
            rbw.contains(&"cbu.client_label".to_string()),
            "client_label is consumed but never produced"
        );
        assert!(
            !rbw.contains(&"cbu.name".to_string()),
            "cbu.name has a producer"
        );
    }

    // ── Edge cases ─────────────────────────────────────────────

    #[test]
    fn test_verbs_for_nonexistent_table() {
        let graph = test_graph();
        let verbs = graph.verbs_for_table("unknown", "table");
        assert!(verbs.is_empty());
    }

    #[test]
    fn test_data_for_nonexistent_verb() {
        let graph = test_graph();
        let data = graph.data_for_verb("nonexistent.verb");
        assert!(data.is_empty());
    }

    #[test]
    fn test_adjacent_verbs_nonexistent() {
        let graph = test_graph();
        let adj = graph.adjacent_verbs("nonexistent.verb");
        assert!(adj.is_empty());
    }

    #[test]
    fn test_orphan_verbs_empty_graph() {
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
        assert!(graph.orphan_verbs().is_empty());
    }
}
