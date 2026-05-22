//! Document type definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

/// Body of a `document_type_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentTypeDefBody {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub category: String,
    #[serde(default)]
    pub max_age_days: Option<u32>,
    #[serde(default)]
    pub accepted_formats: Vec<String>,
    #[serde(default)]
    pub extraction_rules: Vec<DocumentExtractionRule>,
}

/// A rule for extracting data from a document into an attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentExtractionRule {
    pub target_attribute_fqn: String,
    pub method: String,
    #[serde(default)]
    pub min_confidence: Option<f64>,
    #[serde(default)]
    pub requires_review: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = DocumentTypeDefBody {
            fqn: "doc.passport".into(),
            name: "Passport".into(),
            description: "Government-issued passport".into(),
            category: "identity".into(),
            max_age_days: Some(3650),
            accepted_formats: vec!["pdf".into(), "jpeg".into()],
            extraction_rules: vec![DocumentExtractionRule {
                target_attribute_fqn: "entity.nationality".into(),
                method: "ocr".into(),
                min_confidence: Some(0.9),
                requires_review: true,
            }],
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: DocumentTypeDefBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn defaults_empty_lists() {
        let val: DocumentTypeDefBody = serde_json::from_value(serde_json::json!({
            "fqn": "f", "name": "n", "description": "d", "category": "c"
        }))
        .unwrap();
        assert!(val.accepted_formats.is_empty());
        assert!(val.extraction_rules.is_empty());
        assert!(val.max_age_days.is_none());
    }
}
