//! Context-policy enforcement for prompt assembly.
//!
//! Domain Packs declare the maximum classification that may enter prompt
//! context. This module keeps that enforcement pure and testable so ACP/Sage
//! adapters can consume already-redacted discovery context.

use crate::domain_pack::{
    ClassificationLimit, ContextClassificationPolicy, DiscoveryObservation, DiscoveryResponse,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptContextAssembly {
    pub included: Vec<PromptContextObservation>,
    pub redacted: Vec<PromptContextRedaction>,
    pub context_hash: String,
    pub external_llm_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptContextObservation {
    pub key: String,
    pub value: serde_json::Value,
    pub classification: ClassificationLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptContextRedaction {
    pub key: String,
    pub reason: PromptContextRedactionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptContextRedactionReason {
    ClassificationLimitExceeded,
    RequiredRedaction,
}

pub fn assemble_prompt_context(
    policy: &ContextClassificationPolicy,
    discovery: &DiscoveryResponse,
) -> PromptContextAssembly {
    let mut included = Vec::new();
    let mut redacted = Vec::new();

    for observation in &discovery.observations {
        let classification = observation
            .classification
            .unwrap_or(ClassificationLimit::Internal);

        if classification_rank(classification)
            > classification_rank(policy.max_prompt_classification)
        {
            redacted.push(redaction(
                observation,
                PromptContextRedactionReason::ClassificationLimitExceeded,
            ));
            continue;
        }

        if requires_redaction(policy, observation) {
            redacted.push(redaction(
                observation,
                PromptContextRedactionReason::RequiredRedaction,
            ));
            continue;
        }

        included.push(PromptContextObservation {
            key: observation.key.clone(),
            value: observation.value.clone(),
            classification,
        });
    }

    included.sort_by(|left, right| left.key.cmp(&right.key));
    redacted.sort_by(|left, right| left.key.cmp(&right.key));

    let context_hash = hash_included_context(&included);
    let external_llm_allowed = policy.allow_external_llm
        && policy.max_prompt_classification <= ClassificationLimit::Internal
        && redacted
            .iter()
            .all(|r| r.reason != PromptContextRedactionReason::ClassificationLimitExceeded);

    PromptContextAssembly {
        included,
        redacted,
        context_hash,
        external_llm_allowed,
    }
}

fn requires_redaction(
    policy: &ContextClassificationPolicy,
    observation: &DiscoveryObservation,
) -> bool {
    let key = observation.key.to_ascii_lowercase();
    policy.required_redactions.iter().any(|needle| {
        let needle = needle.to_ascii_lowercase();
        !needle.is_empty() && key.contains(&needle)
    })
}

fn redaction(
    observation: &DiscoveryObservation,
    reason: PromptContextRedactionReason,
) -> PromptContextRedaction {
    PromptContextRedaction {
        key: observation.key.clone(),
        reason,
    }
}

fn hash_included_context(included: &[PromptContextObservation]) -> String {
    let bytes = serde_json::to_vec(included).expect("prompt context observations serialize");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

const fn classification_rank(classification: ClassificationLimit) -> u8 {
    match classification {
        ClassificationLimit::Public => 0,
        ClassificationLimit::Internal => 1,
        ClassificationLimit::Confidential => 2,
        ClassificationLimit::Restricted => 3,
    }
}

impl PartialOrd for ClassificationLimit {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(classification_rank(*self).cmp(&classification_rank(*other)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain_pack::{DiscoveryProvenance, DiscoverySubject};

    fn policy() -> ContextClassificationPolicy {
        ContextClassificationPolicy {
            max_prompt_classification: ClassificationLimit::Internal,
            allow_external_llm: true,
            required_redactions: vec!["pii".to_string(), "date_of_birth".to_string()],
        }
    }

    fn discovery() -> DiscoveryResponse {
        DiscoveryResponse {
            probe_id: "kyc-case.read-evidence-summary".to_string(),
            subject: DiscoverySubject {
                subject_kind: "kyc_case".to_string(),
                subject_id: "case-123".to_string(),
            },
            observations: vec![
                DiscoveryObservation {
                    key: "case.status".to_string(),
                    value: serde_json::json!("DISCOVERY"),
                    classification: Some(ClassificationLimit::Internal),
                },
                DiscoveryObservation {
                    key: "case.sanctions_summary".to_string(),
                    value: serde_json::json!("potential hit"),
                    classification: Some(ClassificationLimit::Confidential),
                },
                DiscoveryObservation {
                    key: "customer.pii.date_of_birth".to_string(),
                    value: serde_json::json!("1970-01-01"),
                    classification: Some(ClassificationLimit::Internal),
                },
            ],
            provenance: vec![DiscoveryProvenance {
                source: "test-fixture".to_string(),
                snapshot_ref: Some("snapshot-1".to_string()),
            }],
            first_class_state_mutated: false,
        }
    }

    #[test]
    fn prompt_context_redacts_by_classification_and_required_terms() {
        let assembly = assemble_prompt_context(&policy(), &discovery());

        assert_eq!(assembly.included.len(), 1);
        assert_eq!(assembly.included[0].key, "case.status");
        assert_eq!(assembly.redacted.len(), 2);
        assert!(assembly.redacted.iter().any(|r| {
            r.key == "case.sanctions_summary"
                && r.reason == PromptContextRedactionReason::ClassificationLimitExceeded
        }));
        assert!(assembly.redacted.iter().any(|r| {
            r.key == "customer.pii.date_of_birth"
                && r.reason == PromptContextRedactionReason::RequiredRedaction
        }));
    }

    #[test]
    fn prompt_context_hash_is_deterministic_after_sorting() {
        let first = discovery();
        let mut second = discovery();
        second.observations.reverse();

        let first = assemble_prompt_context(&policy(), &first);
        let second = assemble_prompt_context(&policy(), &second);

        assert_eq!(first.context_hash, second.context_hash);
    }

    #[test]
    fn external_llm_is_refused_when_prompt_limit_is_too_high() {
        let mut policy = policy();
        policy.max_prompt_classification = ClassificationLimit::Confidential;

        let assembly = assemble_prompt_context(&policy, &discovery());

        assert!(!assembly.external_llm_allowed);
    }
}
