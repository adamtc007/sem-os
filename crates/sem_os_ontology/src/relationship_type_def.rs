//! Relationship type definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

/// Body of a `relationship_type_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipTypeDefBody {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    pub source_entity_type_fqn: String,
    pub target_entity_type_fqn: String,
    pub cardinality: RelationshipCardinality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directionality: Option<Directionality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inverse_fqn: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
}

/// Directionality of a relationship.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Directionality {
    Forward,
    Reverse,
    Bidirectional,
}

/// Cardinality of a relationship.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipCardinality {
    OneToOne,
    OneToMany,
    ManyToMany,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = RelationshipTypeDefBody {
            fqn: "rel.parent_child".into(),
            name: "Parent-Child".into(),
            description: "Corporate hierarchy".into(),
            domain: "entity".into(),
            source_entity_type_fqn: "entity.organization".into(),
            target_entity_type_fqn: "entity.organization".into(),
            cardinality: RelationshipCardinality::OneToMany,
            edge_class: Some("ownership".into()),
            directionality: Some(Directionality::Forward),
            inverse_fqn: Some("rel.child_parent".into()),
            constraints: vec!["no_self_reference".into()],
        };
        let json = serde_json::to_value(&val).unwrap();
        // Check rename_all snake_case on enums
        assert_eq!(json["cardinality"], "one_to_many");
        assert_eq!(json["directionality"], "forward");
        // Check all Directionality variants serialize correctly
        let bidir = serde_json::to_value(Directionality::Bidirectional).unwrap();
        assert_eq!(bidir, "bidirectional");
        let rev = serde_json::to_value(Directionality::Reverse).unwrap();
        assert_eq!(rev, "reverse");
        // Round-trip
        let back: RelationshipTypeDefBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }
}
