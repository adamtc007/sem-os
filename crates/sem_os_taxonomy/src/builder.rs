//! Tree builder — 3-pass construction from registry snapshots.
//!
//! Pass 1: collect TaxonomyDef snapshots → index by fqn.
//! Pass 2: collect TaxonomyNode snapshots → index by (taxonomy_fqn, node_fqn).
//! Pass 3: collect MembershipRule snapshots → attach members to nodes.
//! Assemble: sort nodes by sort_order, link children to parents recursively.

use std::collections::HashMap;

use sem_os_ontology::{
    membership::MembershipRuleBody,
    taxonomy_def::{TaxonomyDefBody, TaxonomyNodeBody},
};
use sem_os_types::{ObjectType, SnapshotRow};

use crate::types::{MemberSummary, TaxonomyMeta, TaxonomyTree, TaxonomyTreeNode};

/// Build the taxonomy tree for a single taxonomy FQN from a snapshot slice.
///
/// Returns `None` if no `TaxonomyDef` snapshot with that FQN is found.
pub fn build_taxonomy_tree(snapshots: &[SnapshotRow], taxonomy_fqn: &str) -> Option<TaxonomyTree> {
    let defs = collect_defs(snapshots);
    let def = defs.get(taxonomy_fqn)?;

    let nodes = collect_nodes_for(snapshots, taxonomy_fqn);
    let members = collect_members_for(snapshots, taxonomy_fqn);

    Some(assemble(def, nodes, members))
}

/// Build taxonomy trees for every TaxonomyDef found in the snapshot slice.
pub fn build_all_taxonomy_trees(snapshots: &[SnapshotRow]) -> Vec<TaxonomyTree> {
    let defs = collect_defs(snapshots);
    let mut trees: Vec<TaxonomyTree> = defs
        .keys()
        .filter_map(|fqn| build_taxonomy_tree(snapshots, fqn))
        .collect();
    trees.sort_by(|a, b| a.meta.fqn.cmp(&b.meta.fqn));
    trees
}

// ── Passes ─────────────────────────────────────────────────────────────────

fn collect_defs(snapshots: &[SnapshotRow]) -> HashMap<String, TaxonomyDefBody> {
    snapshots
        .iter()
        .filter(|r| r.object_type == ObjectType::TaxonomyDef)
        .filter_map(|r| r.parse_definition::<TaxonomyDefBody>().ok())
        .map(|b| (b.fqn.clone(), b))
        .collect()
}

fn collect_nodes_for(
    snapshots: &[SnapshotRow],
    taxonomy_fqn: &str,
) -> Vec<TaxonomyNodeBody> {
    snapshots
        .iter()
        .filter(|r| r.object_type == ObjectType::TaxonomyNode)
        .filter_map(|r| r.parse_definition::<TaxonomyNodeBody>().ok())
        .filter(|b| b.taxonomy_fqn == taxonomy_fqn)
        .collect()
}

fn collect_members_for(
    snapshots: &[SnapshotRow],
    taxonomy_fqn: &str,
) -> Vec<MembershipRuleBody> {
    snapshots
        .iter()
        .filter(|r| r.object_type == ObjectType::MembershipRule)
        .filter_map(|r| r.parse_definition::<MembershipRuleBody>().ok())
        .filter(|b| b.taxonomy_fqn == taxonomy_fqn)
        .collect()
}

// ── Assembly ────────────────────────────────────────────────────────────────

fn assemble(
    def: &TaxonomyDefBody,
    mut nodes: Vec<TaxonomyNodeBody>,
    members: Vec<MembershipRuleBody>,
) -> TaxonomyTree {
    // Sort nodes by sort_order for stable child ordering.
    nodes.sort_by_key(|n| (n.sort_order, n.fqn.clone()));

    // Build member index: node_fqn → Vec<MemberSummary>
    let mut member_index: HashMap<String, Vec<MemberSummary>> = HashMap::new();
    for rule in &members {
        member_index
            .entry(rule.node_fqn.clone())
            .or_default()
            .push(MemberSummary {
                fqn: rule.target_fqn.clone(),
                object_type: rule.target_type.clone(),
                kind: rule.membership_kind,
            });
    }

    // Build flat node map and collect total counts.
    let node_count = nodes.len();
    let member_count: usize = member_index.values().map(|v| v.len()).collect::<Vec<_>>().iter().sum();

    // Convert bodies to TaxonomyTreeNode (leaves first, no children yet).
    let mut node_map: HashMap<String, TaxonomyTreeNode> = nodes
        .iter()
        .map(|b| {
            let node = TaxonomyTreeNode {
                fqn: b.fqn.clone(),
                name: b.name.clone(),
                description: b.description.clone(),
                sort_order: b.sort_order,
                labels: b.labels.clone(),
                members: member_index.remove(&b.fqn).unwrap_or_default(),
                children: Vec::new(),
            };
            (b.fqn.clone(), node)
        })
        .collect();

    // Determine root fqn: use declared root_node_fqn, or find nodes with no parent.
    let declared_root = def.root_node_fqn.as_deref();

    // Collect child relationships: (parent_fqn, child_fqn) ordered by sort_order.
    let mut parent_children: HashMap<String, Vec<String>> = HashMap::new();
    let mut root_fqns: Vec<String> = Vec::new();

    for node in &nodes {
        match &node.parent_fqn {
            Some(p) if !p.is_empty() => {
                parent_children
                    .entry(p.clone())
                    .or_default()
                    .push(node.fqn.clone());
            }
            _ => root_fqns.push(node.fqn.clone()),
        }
    }

    // Recursively attach children (post-order, consuming from node_map).
    fn attach_children(
        fqn: &str,
        node_map: &mut HashMap<String, TaxonomyTreeNode>,
        parent_children: &HashMap<String, Vec<String>>,
    ) -> TaxonomyTreeNode {
        let mut node = node_map.remove(fqn).unwrap_or_else(|| TaxonomyTreeNode {
            fqn: fqn.to_owned(),
            name: fqn.to_owned(),
            description: None,
            sort_order: 0,
            labels: Default::default(),
            members: Vec::new(),
            children: Vec::new(),
        });
        if let Some(children_fqns) = parent_children.get(fqn) {
            node.children = children_fqns
                .iter()
                .map(|c| attach_children(c, node_map, parent_children))
                .collect();
        }
        node
    }

    let root = match declared_root {
        Some(r) if node_map.contains_key(r) => {
            attach_children(r, &mut node_map, &parent_children)
        }
        _ => {
            // No declared root or not found — build a synthetic root collecting
            // all actual root-level nodes as children.
            let mut children: Vec<TaxonomyTreeNode> = root_fqns
                .iter()
                .map(|r| attach_children(r, &mut node_map, &parent_children))
                .collect();
            // Absorb any nodes that remain (orphans — parent declared but parent
            // snapshot missing). Attach them at root level rather than losing them.
            children.extend(node_map.drain().map(|(_, n)| n));
            children.sort_by_key(|n| (n.sort_order, n.fqn.clone()));

            TaxonomyTreeNode {
                fqn: def.fqn.clone(),
                name: def.name.clone(),
                description: Some(def.description.clone()),
                sort_order: 0,
                labels: Default::default(),
                members: Vec::new(),
                children,
            }
        }
    };

    TaxonomyTree {
        meta: TaxonomyMeta {
            fqn: def.fqn.clone(),
            name: def.name.clone(),
            description: def.description.clone(),
            domain: def.domain.clone(),
            classification_axis: def.classification_axis.clone(),
            max_depth: def.max_depth,
        },
        root,
        node_count,
        member_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sem_os_ontology::membership::MembershipKind;
    use chrono::Utc;
    use sem_os_types::{
        ChangeType, GovernanceTier, SnapshotStatus, TrustClass,
    };
    use uuid::Uuid;

    fn snapshot(object_type: ObjectType, body: serde_json::Value) -> SnapshotRow {
        SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Governed,
            trust_class: TrustClass::Convenience,
            security_label: serde_json::Value::Null,
            effective_from: Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            definition: body,
            created_at: Utc::now(),
        }
    }

    fn kyc_snapshots() -> Vec<SnapshotRow> {
        vec![
            // TaxonomyDef
            snapshot(
                ObjectType::TaxonomyDef,
                serde_json::json!({
                    "fqn": "tax.kyc",
                    "name": "KYC Taxonomy",
                    "description": "KYC risk classification",
                    "domain": "kyc",
                    "root_node_fqn": "tax.kyc.root",
                    "max_depth": 3
                }),
            ),
            // Nodes
            snapshot(
                ObjectType::TaxonomyNode,
                serde_json::json!({
                    "fqn": "tax.kyc.root",
                    "name": "KYC Root",
                    "taxonomy_fqn": "tax.kyc",
                    "sort_order": 0
                }),
            ),
            snapshot(
                ObjectType::TaxonomyNode,
                serde_json::json!({
                    "fqn": "tax.kyc.high_risk",
                    "name": "High Risk",
                    "taxonomy_fqn": "tax.kyc",
                    "parent_fqn": "tax.kyc.root",
                    "sort_order": 1
                }),
            ),
            snapshot(
                ObjectType::TaxonomyNode,
                serde_json::json!({
                    "fqn": "tax.kyc.standard",
                    "name": "Standard",
                    "taxonomy_fqn": "tax.kyc",
                    "parent_fqn": "tax.kyc.root",
                    "sort_order": 2
                }),
            ),
            // MembershipRule
            snapshot(
                ObjectType::MembershipRule,
                serde_json::json!({
                    "fqn": "mem.kyc.high_risk.attr",
                    "name": "PEP status → High Risk",
                    "taxonomy_fqn": "tax.kyc",
                    "node_fqn": "tax.kyc.high_risk",
                    "membership_kind": "direct",
                    "target_type": "attribute_def",
                    "target_fqn": "attr.entity.pep_status"
                }),
            ),
        ]
    }

    #[test]
    fn builds_tree_for_known_taxonomy() {
        let snapshots = kyc_snapshots();
        let tree = build_taxonomy_tree(&snapshots, "tax.kyc").expect("tree should build");
        assert_eq!(tree.meta.fqn, "tax.kyc");
        assert_eq!(tree.node_count, 3);
        assert_eq!(tree.member_count, 1);
    }

    #[test]
    fn root_node_has_two_children() {
        let snapshots = kyc_snapshots();
        let tree = build_taxonomy_tree(&snapshots, "tax.kyc").unwrap();
        assert_eq!(tree.root.fqn, "tax.kyc.root");
        assert_eq!(tree.root.children.len(), 2);
    }

    #[test]
    fn children_sorted_by_sort_order() {
        let snapshots = kyc_snapshots();
        let tree = build_taxonomy_tree(&snapshots, "tax.kyc").unwrap();
        let fqns: Vec<&str> = tree.root.children.iter().map(|n| n.fqn.as_str()).collect();
        assert_eq!(fqns, ["tax.kyc.high_risk", "tax.kyc.standard"]);
    }

    #[test]
    fn member_attached_to_correct_node() {
        let snapshots = kyc_snapshots();
        let tree = build_taxonomy_tree(&snapshots, "tax.kyc").unwrap();
        let high_risk = tree
            .root
            .children
            .iter()
            .find(|n| n.fqn == "tax.kyc.high_risk")
            .unwrap();
        assert_eq!(high_risk.members.len(), 1);
        assert_eq!(high_risk.members[0].fqn, "attr.entity.pep_status");
        assert_eq!(high_risk.members[0].kind, MembershipKind::Direct);
    }

    #[test]
    fn unknown_taxonomy_returns_none() {
        let snapshots = kyc_snapshots();
        assert!(build_taxonomy_tree(&snapshots, "tax.nonexistent").is_none());
    }

    #[test]
    fn build_all_returns_one_tree() {
        let snapshots = kyc_snapshots();
        let trees = build_all_taxonomy_trees(&snapshots);
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].meta.fqn, "tax.kyc");
    }

    #[test]
    fn yaml_emit_round_trips() {
        let snapshots = kyc_snapshots();
        let tree = build_taxonomy_tree(&snapshots, "tax.kyc").unwrap();
        let yaml = tree.to_yaml().unwrap();
        assert!(yaml.contains("KYC Taxonomy"));
        assert!(yaml.contains("High Risk"));
        assert!(yaml.contains("attr.entity.pep_status"));
        let back: TaxonomyTree = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.meta.fqn, tree.meta.fqn);
        assert_eq!(back.node_count, tree.node_count);
    }

    #[test]
    fn json_emit_is_valid() {
        let snapshots = kyc_snapshots();
        let tree = build_taxonomy_tree(&snapshots, "tax.kyc").unwrap();
        let json = tree.to_json().unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["meta"]["fqn"], "tax.kyc");
        assert_eq!(val["node_count"], 3);
    }

    #[test]
    fn synthetic_root_used_when_no_declared_root() {
        let mut snapshots = kyc_snapshots();
        // Replace TaxonomyDef with one that has no root_node_fqn
        snapshots[0] = snapshot(
            ObjectType::TaxonomyDef,
            serde_json::json!({
                "fqn": "tax.kyc",
                "name": "KYC Taxonomy",
                "description": "KYC risk classification",
                "domain": "kyc"
            }),
        );
        // Remove declared parent from nodes so they become root-level
        snapshots[1] = snapshot(
            ObjectType::TaxonomyNode,
            serde_json::json!({
                "fqn": "tax.kyc.high_risk",
                "name": "High Risk",
                "taxonomy_fqn": "tax.kyc",
                "sort_order": 0
            }),
        );
        snapshots[2] = snapshot(
            ObjectType::TaxonomyNode,
            serde_json::json!({
                "fqn": "tax.kyc.standard",
                "name": "Standard",
                "taxonomy_fqn": "tax.kyc",
                "sort_order": 1
            }),
        );
        // Remove the original root node snapshot (index 3 is now high_risk, etc.)
        let snapshots: Vec<_> = snapshots.into_iter().take(4).collect();
        let tree = build_taxonomy_tree(&snapshots, "tax.kyc").unwrap();
        // Synthetic root has the taxonomy fqn
        assert_eq!(tree.root.fqn, "tax.kyc");
        assert_eq!(tree.root.children.len(), 2);
    }
}
