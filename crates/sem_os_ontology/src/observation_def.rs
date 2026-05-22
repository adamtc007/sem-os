//! Observation definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

fn default_none() -> String {
    "none".into()
}

/// Body of an `observation_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationDefBody {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub observation_type: String,
    #[serde(default)]
    pub source_verb_fqn: Option<String>,
    #[serde(default)]
    pub extraction_rules: Vec<ExtractionRule>,
    #[serde(default)]
    pub requires_human_review: bool,
}

/// A rule for extracting observation data into an attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionRule {
    pub target_attribute_fqn: String,
    pub source_path: String,
    #[serde(default = "default_none")]
    pub transform: String,
    #[serde(default)]
    pub confidence: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = ObservationDefBody {
            fqn: "kyc.identity_check".into(),
            name: "Identity Check".into(),
            description: "Verify identity docs".into(),
            observation_type: "document_review".into(),
            source_verb_fqn: Some("kyc.verify-identity".into()),
            extraction_rules: vec![ExtractionRule {
                target_attribute_fqn: "entity.name".into(),
                source_path: "$.name".into(),
                transform: "uppercase".into(),
                confidence: Some(0.95),
            }],
            requires_human_review: true,
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: ObservationDefBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn default_none_transform() {
        let rule: ExtractionRule = serde_json::from_value(serde_json::json!({
            "target_attribute_fqn": "a.b",
            "source_path": "$.x"
        }))
        .unwrap();
        assert_eq!(rule.transform, "none");
    }
}
