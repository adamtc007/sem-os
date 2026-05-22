//! ACP projection envelope types.
//!
//! ACP is the rich agent-editor discovery projection surface. These value
//! types keep projection shape, classification, redactions, and provenance
//! explicit without granting mutation authority.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain_pack::ClassificationLimit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpProjectionKind {
    PackManifest,
    ProbeCatalogue,
    DiscoverySurface,
    WorkspaceState,
    Dag,
    GraphScene,
    VerbSurface,
    TransitionSurface,
    LanguagePack,
    Governance,
    EvidenceSchema,
    AffinityGraph,
    Lineage,
    DerivationRegistry,
    Materiality,
    Policy,
}

impl AcpProjectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PackManifest => "pack_manifest",
            Self::ProbeCatalogue => "probe_catalogue",
            Self::DiscoverySurface => "discovery_surface",
            Self::WorkspaceState => "workspace_state",
            Self::Dag => "dag",
            Self::GraphScene => "graph_scene",
            Self::VerbSurface => "verb_surface",
            Self::TransitionSurface => "transition_surface",
            Self::LanguagePack => "language_pack",
            Self::Governance => "governance",
            Self::EvidenceSchema => "evidence_schema",
            Self::AffinityGraph => "affinity_graph",
            Self::Lineage => "lineage",
            Self::DerivationRegistry => "derivation_registry",
            Self::Materiality => "materiality",
            Self::Policy => "policy",
        }
    }
}

impl std::str::FromStr for AcpProjectionKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pack_manifest" => Ok(Self::PackManifest),
            "probe_catalogue" => Ok(Self::ProbeCatalogue),
            "discovery_surface" => Ok(Self::DiscoverySurface),
            "workspace_state" => Ok(Self::WorkspaceState),
            "dag" => Ok(Self::Dag),
            "graph_scene" => Ok(Self::GraphScene),
            "verb_surface" => Ok(Self::VerbSurface),
            "transition_surface" => Ok(Self::TransitionSurface),
            "language_pack" => Ok(Self::LanguagePack),
            "governance" => Ok(Self::Governance),
            "evidence_schema" => Ok(Self::EvidenceSchema),
            "affinity_graph" => Ok(Self::AffinityGraph),
            "lineage" => Ok(Self::Lineage),
            "derivation_registry" => Ok(Self::DerivationRegistry),
            "materiality" => Ok(Self::Materiality),
            "policy" => Ok(Self::Policy),
            other => Err(format!("unknown ACP projection kind {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpProjectionSubject {
    pub subject_kind: String,
    pub subject_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpProjectionRedaction {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcpProjectionEnvelope {
    pub projection_kind: AcpProjectionKind,
    pub session_id: Uuid,
    pub pack_id: String,
    pub classification: ClassificationLimit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<AcpProjectionSubject>,
    #[serde(default)]
    pub snapshot_refs: Vec<String>,
    pub payload: Value,
    #[serde(default)]
    pub redactions: Vec<AcpProjectionRedaction>,
    pub projection_hash: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcpProjectionEnvelopeInput {
    pub projection_kind: AcpProjectionKind,
    pub session_id: Uuid,
    pub pack_id: String,
    pub classification: ClassificationLimit,
    pub subject: Option<AcpProjectionSubject>,
    pub snapshot_refs: Vec<String>,
    pub payload: Value,
    pub redactions: Vec<AcpProjectionRedaction>,
}

impl AcpProjectionEnvelope {
    pub fn new(input: AcpProjectionEnvelopeInput) -> Self {
        let projection_hash = projection_hash(&input);
        Self {
            projection_kind: input.projection_kind,
            session_id: input.session_id,
            pack_id: input.pack_id,
            classification: input.classification,
            subject: input.subject,
            snapshot_refs: input.snapshot_refs,
            payload: input.payload,
            redactions: input.redactions,
            projection_hash,
            generated_at: Utc::now(),
        }
    }
}

fn projection_hash(input: &AcpProjectionEnvelopeInput) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.projection_kind.as_str().as_bytes());
    hasher.update(input.session_id.as_bytes());
    hasher.update(input.pack_id.as_bytes());
    hasher.update(format!("{:?}", input.classification).as_bytes());
    if let Some(subject) = input.subject.as_ref() {
        hasher.update(subject.subject_kind.as_bytes());
        hasher.update(subject.subject_id.as_bytes());
    }
    for snapshot_ref in &input.snapshot_refs {
        hasher.update(snapshot_ref.as_bytes());
    }
    hasher.update(serde_json::to_vec(&input.payload).unwrap_or_default());
    hasher.update(serde_json::to_vec(&input.redactions).unwrap_or_default());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_hash_is_stable_for_same_payload() {
        let payload = serde_json::json!({"a": 1});
        let first = AcpProjectionEnvelope::new(AcpProjectionEnvelopeInput {
            projection_kind: AcpProjectionKind::PackManifest,
            session_id: Uuid::nil(),
            pack_id: "pack".into(),
            classification: ClassificationLimit::Internal,
            subject: None,
            snapshot_refs: vec!["snap-1".into()],
            payload: payload.clone(),
            redactions: vec![],
        });
        let second = AcpProjectionEnvelope::new(AcpProjectionEnvelopeInput {
            projection_kind: AcpProjectionKind::PackManifest,
            session_id: Uuid::nil(),
            pack_id: "pack".into(),
            classification: ClassificationLimit::Internal,
            subject: None,
            snapshot_refs: vec!["snap-1".into()],
            payload,
            redactions: vec![],
        });

        assert_eq!(first.projection_hash, second.projection_hash);
    }
}
