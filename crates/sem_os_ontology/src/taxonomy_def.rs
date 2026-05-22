//! Taxonomy definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Body of a `taxonomy_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyDefBody {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    #[serde(default)]
    pub root_node_fqn: Option<String>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub classification_axis: Option<String>,
}

/// Body of a `taxonomy_node` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyNodeBody {
    pub fqn: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub taxonomy_fqn: String,
    #[serde(default)]
    pub parent_fqn: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = TaxonomyDefBody {
            fqn: "domain.kyc".into(),
            name: "KYC Domain".into(),
            description: "KYC taxonomy".into(),
            domain: "kyc".into(),
            root_node_fqn: Some("domain.kyc.root".into()),
            max_depth: Some(4),
            classification_axis: Some("risk_tier".into()),
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: TaxonomyDefBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);

        // TaxonomyNodeBody with BTreeMap labels
        let mut labels = BTreeMap::new();
        labels.insert("en".into(), "KYC Root".into());
        labels.insert("de".into(), "KYC Wurzel".into());
        let node = TaxonomyNodeBody {
            fqn: "domain.kyc.root".into(),
            name: "KYC Root".into(),
            description: Some("Top-level node".into()),
            taxonomy_fqn: "domain.kyc".into(),
            parent_fqn: None,
            sort_order: 0,
            labels,
        };
        let nj = serde_json::to_value(&node).unwrap();
        assert_eq!(nj["labels"]["de"], "KYC Wurzel");
        let nb: TaxonomyNodeBody = serde_json::from_value(nj.clone()).unwrap();
        let nj2 = serde_json::to_value(&nb).unwrap();
        assert_eq!(nj, nj2);
    }
}
