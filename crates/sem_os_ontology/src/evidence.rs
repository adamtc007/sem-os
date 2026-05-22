//! Evidence requirement types for the semantic registry.
//!
//! An evidence requirement specifies what documents and observations
//! must be collected to substantiate a registry claim. This is the
//! bridge between the Proof Rule (governance) and the document/observation
//! collection pipeline.
//!
//! Key invariant: if an evidence requirement references a `TrustClass::Proof`
//! attribute, the evidence requirement itself must be governed-tier.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_one() -> u32 {
    1
}

/// Body for an evidence requirement snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRequirementBody {
    /// Fully qualified name, e.g. `"kyc.identity-evidence"`
    pub fqn: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Entity type this requirement applies to
    pub target_entity_type: String,
    /// Context that triggers this requirement (e.g., `onboarding`, `periodic_review`)
    #[serde(default)]
    pub trigger_context: Option<String>,
    /// Required documents
    #[serde(default)]
    pub required_documents: Vec<RequiredDocument>,
    /// Required observations (data points from external sources)
    #[serde(default)]
    pub required_observations: Vec<RequiredObservation>,
    /// Whether ALL items are required (true) or ANY (false)
    #[serde(default = "default_true")]
    pub all_required: bool,
}

/// A document required as evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredDocument {
    /// FQN of the document type definition
    pub document_type_fqn: String,
    /// How many copies/instances are needed
    #[serde(default = "default_one")]
    pub min_count: u32,
    /// Maximum age in days (None = no expiry check)
    #[serde(default)]
    pub max_age_days: Option<u32>,
    /// Acceptable alternatives (any one suffices)
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// Whether this specific document is mandatory
    #[serde(default = "default_true")]
    pub mandatory: bool,
}

/// An observation (data point) required as evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredObservation {
    /// FQN of the observation definition
    pub observation_def_fqn: String,
    /// Minimum confidence score required (0.0 to 1.0)
    #[serde(default)]
    pub min_confidence: Option<f64>,
    /// Maximum age in days
    #[serde(default)]
    pub max_age_days: Option<u32>,
    /// Whether this observation is mandatory
    #[serde(default = "default_true")]
    pub mandatory: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = EvidenceRequirementBody {
            fqn: "kyc.identity_evidence".into(),
            name: "Identity Evidence".into(),
            description: "Required identity docs".into(),
            target_entity_type: "natural_person".into(),
            trigger_context: Some("onboarding".into()),
            required_documents: vec![RequiredDocument {
                document_type_fqn: "doc.passport".into(),
                min_count: 1,
                max_age_days: Some(365),
                alternatives: vec!["doc.national_id".into()],
                mandatory: true,
            }],
            required_observations: vec![RequiredObservation {
                observation_def_fqn: "obs.identity_check".into(),
                min_confidence: Some(0.85),
                max_age_days: Some(90),
                mandatory: false,
            }],
            all_required: false,
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: EvidenceRequirementBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn defaults_all_required_and_mandatory() {
        let doc: RequiredDocument = serde_json::from_value(serde_json::json!({
            "document_type_fqn": "doc.x"
        }))
        .unwrap();
        assert!(doc.mandatory);
        assert_eq!(doc.min_count, 1);

        let body: EvidenceRequirementBody = serde_json::from_value(serde_json::json!({
            "fqn": "f", "name": "n", "description": "d", "target_entity_type": "t"
        }))
        .unwrap();
        assert!(body.all_required);
    }
}
