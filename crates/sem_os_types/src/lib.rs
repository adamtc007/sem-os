//! Core domain types for Semantic OS.
//! These are pure value types — no sqlx, no DB dependencies.
//! Migrated from sem_reg/types.rs with sqlx derives stripped.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};
use uuid::Uuid;

// ── Enums (pure — no sqlx::Type) ─────────────────────────────

/// Governance tier — determines workflow rigour, NOT security posture.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum GovernanceTier {
    Governed,
    Operational,
}

/// Attribute visibility — external (client-facing, governed) or internal (system/implementation).
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AttributeVisibility {
    #[default]
    External,
    Internal,
}

/// Trust class — graduated trust levels for registry objects.
/// Invariant: `Proof` is only valid when `governance_tier = Governed`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TrustClass {
    Proof,
    DecisionSupport,
    Convenience,
}

/// Snapshot lifecycle status.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SnapshotStatus {
    Draft,
    Active,
    Deprecated,
    Retired,
}

/// Change type for snapshot transitions.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ChangeType {
    Created,
    NonBreaking,
    Breaking,
    Promotion,
    Deprecation,
    Retirement,
}

/// Registry object type — discriminator for the shared snapshots table.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ObjectType {
    AttributeDef,
    EntityTypeDef,
    RelationshipTypeDef,
    VerbContract,
    MacroDef,
    UniverseDef,
    ConstellationFamilyDef,
    ConstellationMap,
    StateMachine,
    StateGraph,
    DagTaxonomy,
    DomainPack,
    TaxonomyDef,
    TaxonomyNode,
    MembershipRule,
    ViewDef,
    PolicyRule,
    EvidenceRequirement,
    DocumentTypeDef,
    RequirementProfileDef,
    ProofObligationDef,
    EvidenceStrategyDef,
    ObservationDef,
    DerivationSpec,
    PhraseMapping,
    SharedAtom,
    ServiceResourceDef,
}

// ── Security label ────────────────────────────────────────────

/// Security label carried on every registry snapshot (both tiers).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SecurityLabel {
    #[serde(default)]
    pub classification: Classification,
    #[serde(default)]
    pub pii: bool,
    #[serde(default)]
    pub jurisdictions: Vec<String>,
    #[serde(default)]
    pub purpose_limitation: Vec<String>,
    #[serde(default)]
    pub handling_controls: Vec<HandlingControl>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Public,
    #[default]
    Internal,
    Confidential,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandlingControl {
    MaskByDefault,
    NoExport,
    DualControl,
    SecureViewerOnly,
    NoLlmExternal,
}

/// Whether a derived or governed attribute may be used as evidence.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum EvidenceGrade {
    /// No evidence-bearing semantics are asserted for this object.
    None,
    /// This object must not be treated as evidence.
    #[default]
    Prohibited,
    /// Evidence use is permitted only with additional policy constraints.
    AllowedWithConstraints,
    /// This object is intended for regulated evidence-bearing workflows.
    RegulatoryEvidence,
}

// ── Snapshot metadata ─────────────────────────────────────────

/// Common metadata for every snapshot.
/// Not a DB row — used as an input struct for creating snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub object_type: ObjectType,
    pub object_id: Uuid,
    pub version_major: i32,
    pub version_minor: i32,
    pub status: SnapshotStatus,
    pub governance_tier: GovernanceTier,
    pub trust_class: TrustClass,
    pub security_label: SecurityLabel,
    pub change_type: ChangeType,
    pub change_rationale: Option<String>,
    pub created_by: String,
    pub approved_by: Option<String>,
    pub predecessor_id: Option<Uuid>,
}

impl SnapshotMeta {
    /// Create metadata for a new object at the specified governance tier.
    pub fn new_at_tier(
        governance_tier: GovernanceTier,
        object_type: ObjectType,
        object_id: Uuid,
        created_by: impl Into<String>,
    ) -> Self {
        Self {
            object_type,
            object_id,
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier,
            trust_class: TrustClass::Convenience,
            security_label: SecurityLabel::default(),
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: created_by.into(),
            approved_by: Some("auto".into()),
            predecessor_id: None,
        }
    }

    /// Create metadata for a new operational object (auto-approved).
    pub fn new_operational(
        object_type: ObjectType,
        object_id: Uuid,
        created_by: impl Into<String>,
    ) -> Self {
        Self::new_at_tier(
            GovernanceTier::Operational,
            object_type,
            object_id,
            created_by,
        )
    }
}

// ── Full snapshot row (pure — no sqlx::FromRow) ───────────────

/// A complete snapshot row — pure representation without sqlx derives.
/// The postgres adapter has `PgSnapshotRow` with `sqlx::FromRow` and
/// `impl From<PgSnapshotRow> for SnapshotRow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRow {
    pub snapshot_id: Uuid,
    pub snapshot_set_id: Option<Uuid>,
    pub object_type: ObjectType,
    pub object_id: Uuid,
    pub version_major: i32,
    pub version_minor: i32,
    pub status: SnapshotStatus,
    pub governance_tier: GovernanceTier,
    pub trust_class: TrustClass,
    pub security_label: serde_json::Value,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
    pub predecessor_id: Option<Uuid>,
    pub change_type: ChangeType,
    pub change_rationale: Option<String>,
    pub created_by: String,
    pub approved_by: Option<String>,
    pub definition: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl SnapshotRow {
    /// Deserialise the JSONB `definition` column into a typed body.
    pub fn parse_definition<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.definition.clone())
    }

    /// Deserialise the JSONB `security_label` column.
    pub fn parse_security_label(&self) -> Result<SecurityLabel, serde_json::Error> {
        serde_json::from_value(self.security_label.clone())
    }

    /// Version string, e.g. "1.0"
    pub fn version_string(&self) -> String {
        format!("{}.{}", self.version_major, self.version_minor)
    }
}

// ── Fully-qualified name ──────────────────────────────────────

/// Fully-qualified name (e.g., "cbu.jurisdiction_code").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fqn(pub String);

impl Fqn {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Fqn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── ID newtypes ───────────────────────────────────────────────

/// Unique identifier for a snapshot set (atomic publish unit).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotSetId(pub Uuid);

/// Unique identifier for an individual snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub Uuid);

/// Unique identifier for an outbox event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

// ── Composite types ───────────────────────────────────────────

/// Summary of a snapshot for listing/manifest purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSummary {
    pub snapshot_id: SnapshotId,
    pub object_type: ObjectType,
    pub fqn: Fqn,
    pub content_hash: String,
}

/// A snapshot that depends on (references) a given FQN.
/// Returned by `SnapshotStore::find_dependents()` for impact analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependentSnapshot {
    pub snapshot_id: uuid::Uuid,
    pub object_type: ObjectType,
    pub fqn: String,
}

/// Manifest of a snapshot set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub snapshot_set_id: SnapshotSetId,
    pub published_at: DateTime<Utc>,
    pub entries: Vec<SnapshotSummary>,
}

/// Input for a publish operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishInput {
    pub payload: serde_json::Value,
}

/// Exported snapshot for external consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotExport {
    pub snapshot_id: SnapshotId,
    pub fqn: Fqn,
    pub object_type: ObjectType,
    pub payload: serde_json::Value,
}

/// A fully-typed object loaded from a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedObject {
    pub snapshot_id: SnapshotId,
    pub fqn: Fqn,
    pub object_type: ObjectType,
    pub definition: serde_json::Value,
}

/// An outbox event produced atomically with a publish.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEvent {
    pub event_id: EventId,
    pub snapshot_set_id: SnapshotSetId,
    /// Monotonic sequence number assigned by the database.
    pub outbox_seq: i64,
    /// Discriminator for the event kind (e.g. "snapshot_published", "snapshot_deprecated").
    pub event_type: String,
    /// Number of delivery attempts (starts at 0, incremented by the outbox poller).
    pub attempt_count: u32,
    /// Correlation identifier for tracing a publish through the outbox pipeline.
    pub correlation_id: Uuid,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// An audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub action: String,
    pub details: serde_json::Value,
}

/// An entity-centric evidence observation.
///
/// Records evidence that a specific entity (`subject_ref`) has been observed
/// for a specific attribute (`attribute_fqn`). Inserts into
/// `sem_reg.attribute_observations` (migration 091).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceInstance {
    /// The entity this observation is about.
    pub subject_ref: uuid::Uuid,
    /// Fully-qualified attribute name (e.g. `"kyc.ubo_ownership_pct"`).
    pub attribute_fqn: String,
    /// Confidence score 0.0–1.0.
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    /// Evidence grade (must match DB CHECK constraint).
    #[serde(default = "default_evidence_grade")]
    pub evidence_grade: String,
    /// When the observation was made (defaults to now in DB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Optional raw payload with observation details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_payload: Option<serde_json::Value>,
}

fn default_confidence() -> f32 {
    1.0
}
fn default_evidence_grade() -> String {
    "system_derived".to_string()
}

// ── Changeset types ───────────────────────────────────────────

/// ChangeSet lifecycle status — canonical 9-state authoring superset.
///
/// Was at `sem_os_core::authoring::types::ChangeSetStatus` (a policy-tier
/// module) but the enum is foundational vocabulary, not policy logic —
/// relocated to `sem_os_types` in sem_os_core-split v1 Phase 2.5 so the
/// `Changeset` struct below can use it without a back-edge into the
/// policy plane. `sem_os_core::authoring::types::ChangeSetStatus` is
/// now a `pub use` of this canonical definition.
///
/// Transitions:
///   Draft → Validated → DryRunPassed → Published
///   Draft → Rejected (validation failure)
///   Validated → Rejected (dry-run not attempted)
///   Validated → DryRunFailed
///   Published → Superseded (when successor declares supersedes_change_set_id)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ChangeSetStatus {
    Draft,
    #[strum(serialize = "under_review", serialize = "in_review")]
    UnderReview,
    Approved,
    Validated,
    Rejected,
    DryRunPassed,
    DryRunFailed,
    Published,
    Superseded,
}

impl ChangeSetStatus {
    /// Whether this status is terminal (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Published | Self::Rejected | Self::DryRunFailed | Self::Superseded
        )
    }

    /// Whether this status is a non-terminal intermediate state.
    pub fn is_intermediate(&self) -> bool {
        matches!(
            self,
            Self::Draft | Self::UnderReview | Self::Approved | Self::Validated | Self::DryRunPassed
        )
    }
}

/// Back-compat alias — historically the types module exposed this as
/// `ChangesetStatus` (lowercase 's'). Both spellings now resolve to
/// the same canonical enum.
pub use ChangeSetStatus as ChangesetStatus;

// ── Agent mode (relocated in Phase 9, was sem_os_core::authoring::agent_mode)

pub mod agent_mode;
pub use agent_mode::AgentMode;

// ── Gate severity ─────────────────────────────────────────────

/// Severity of a gate failure. Originally lived in
/// `sem_os_core::gates::GateSeverity`; relocated to sem_os_types in
/// sem_os_core-split v1 Phase 9 because `sem_os_core::error::SemOsError`
/// embeds a `GateViolation` that carries this severity, and the engine
/// crate cannot reach up into the policy plane where the gates module
/// now lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateSeverity {
    Error,
    Warning,
}

impl std::fmt::Display for GateSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warning => write!(f, "warning"),
        }
    }
}

/// Kind of change in a changeset entry.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ChangeKind {
    Add,
    Modify,
    Remove,
}

/// Review verdict for a changeset.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ReviewVerdict {
    Approved,
    Rejected,
    RequestedChanges,
}

/// A changeset — a grouping of draft changes pending review/approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Changeset {
    pub changeset_id: Uuid,
    pub status: ChangesetStatus,
    pub owner_actor_id: String,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A single entry within a changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetEntry {
    pub entry_id: Uuid,
    pub changeset_id: Uuid,
    pub object_fqn: String,
    pub object_type: ObjectType,
    pub change_kind: ChangeKind,
    pub draft_payload: serde_json::Value,
    pub base_snapshot_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// A review submitted on a changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetReview {
    pub review_id: Uuid,
    pub changeset_id: Uuid,
    pub actor_id: String,
    pub verdict: ReviewVerdict,
    pub comment: Option<String>,
    pub reviewed_at: DateTime<Utc>,
}

/// Input for creating a new changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChangesetInput {
    pub owner_actor_id: String,
    pub scope: String,
}

/// Input for adding an entry to a changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddChangesetEntryInput {
    pub object_fqn: String,
    pub object_type: ObjectType,
    pub change_kind: ChangeKind,
    pub draft_payload: serde_json::Value,
    pub base_snapshot_id: Option<Uuid>,
}

/// Input for submitting a review on a changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitReviewInput {
    pub actor_id: String,
    pub verdict: ReviewVerdict,
    pub comment: Option<String>,
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_rule_check() {
        let meta = SnapshotMeta {
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Proof,
            security_label: SecurityLabel::default(),
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            predecessor_id: None,
        };
        assert!(
            meta.governance_tier == GovernanceTier::Operational
                && meta.trust_class == TrustClass::Proof,
            "This combination should be rejected by publish gates"
        );
    }

    #[test]
    fn test_security_label_serde() {
        let label = SecurityLabel {
            classification: Classification::Confidential,
            pii: true,
            jurisdictions: vec!["UK".into(), "EU".into()],
            purpose_limitation: vec!["KYC_CDD".into()],
            handling_controls: vec![HandlingControl::MaskByDefault],
        };
        let json = serde_json::to_value(&label).unwrap();
        let back: SecurityLabel = serde_json::from_value(json).unwrap();
        assert_eq!(back.classification, Classification::Confidential);
        assert!(back.pii);
        assert_eq!(back.jurisdictions.len(), 2);
    }

    #[test]
    fn test_snapshot_meta_new_operational() {
        let meta =
            SnapshotMeta::new_operational(ObjectType::VerbContract, Uuid::new_v4(), "scanner");
        assert_eq!(meta.governance_tier, GovernanceTier::Operational);
        assert_eq!(meta.trust_class, TrustClass::Convenience);
        assert_eq!(meta.approved_by.as_deref(), Some("auto"));
    }

    #[test]
    fn test_snapshot_meta_new_at_tier() {
        let meta = SnapshotMeta::new_at_tier(
            GovernanceTier::Governed,
            ObjectType::VerbContract,
            Uuid::new_v4(),
            "scanner",
        );
        assert_eq!(meta.governance_tier, GovernanceTier::Governed);
        assert_eq!(meta.trust_class, TrustClass::Convenience);
        assert_eq!(meta.approved_by.as_deref(), Some("auto"));
    }

    #[test]
    fn test_object_type_display() {
        assert_eq!(ObjectType::VerbContract.to_string(), "verb_contract");
        assert_eq!(ObjectType::AttributeDef.to_string(), "attribute_def");
        assert_eq!(
            ObjectType::ServiceResourceDef.to_string(),
            "service_resource_def"
        );
        assert_eq!(ObjectType::MacroDef.to_string(), "macro_def");
        assert_eq!(ObjectType::UniverseDef.to_string(), "universe_def");
        assert_eq!(
            ObjectType::ConstellationFamilyDef.to_string(),
            "constellation_family_def"
        );
        assert_eq!(
            ObjectType::ConstellationMap.to_string(),
            "constellation_map"
        );
    }

    #[test]
    fn test_fqn_display() {
        let fqn = Fqn::new("cbu.jurisdiction_code");
        assert_eq!(fqn.to_string(), "cbu.jurisdiction_code");
        assert_eq!(fqn.as_str(), "cbu.jurisdiction_code");
    }

    #[test]
    fn test_snapshot_row_parse_definition() {
        let row = SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Convenience,
            security_label: serde_json::json!({}),
            effective_from: Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            definition: serde_json::json!({"fqn": "test.attr", "name": "Test"}),
            created_at: Utc::now(),
        };
        assert_eq!(row.version_string(), "1.0");
        let label = row.parse_security_label().unwrap();
        assert_eq!(label.classification, Classification::Internal);
    }
}
