//! Authoring pipeline types — ChangeSet lifecycle, artifacts, reports.
//! Pure value types — no sqlx, no DB dependencies.
//! See: docs/semantic_os_research_governed_boundary_v0.4.md §3, §6.1

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};
use uuid::Uuid;

// ── ChangeSet status (canonical 9-state, lives in sem_os_types) ───

// sem_os_core-split v1 Phase 2.5 (2026-05-14): ChangeSetStatus enum +
// impl relocated to `sem_os_types`. The enum is foundational vocabulary
// (lifecycle states, derive macros only) — having it here forced the
// `Changeset` struct in `sem_os_types` to reach up into the policy
// tier. Re-exported here to preserve `sem_os_core::authoring::types::
// ChangeSetStatus` paths for existing consumers.
pub use sem_os_types::ChangeSetStatus;

// ── Artifact type ──────────────────────────────────────────────

/// Type discriminator for artifacts in a ChangeSet bundle.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ArtifactType {
    MigrationSql,
    MigrationDownSql,
    VerbYaml,
    AttributeJson,
    TaxonomyJson,
    DocJson,
}

// ── ChangeSet (full row) ───────────────────────────────────────

/// A ChangeSet — an immutable bundle of artifacts with content-addressed idempotency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSetFull {
    pub change_set_id: Uuid,
    pub status: ChangeSetStatus,
    pub content_hash: String,
    pub hash_version: String,
    pub title: String,
    pub rationale: Option<String>,
    pub created_by: String,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub supersedes_change_set_id: Option<Uuid>,
    pub superseded_by: Option<Uuid>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub depends_on: Vec<Uuid>,
    pub evaluated_against_snapshot_set_id: Option<Uuid>,
}

// ── ChangeSet artifact ─────────────────────────────────────────

/// A single artifact within a ChangeSet bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSetArtifact {
    pub artifact_id: Uuid,
    pub change_set_id: Uuid,
    pub artifact_type: ArtifactType,
    pub ordinal: i32,
    pub path: Option<String>,
    pub content: String,
    pub content_hash: String,
    pub metadata: Option<serde_json::Value>,
}

// ── ChangeSet manifest (input for propose) ─────────────────────

/// Input manifest for `propose_change_set`.
/// Parsed from the `changeset.yaml` file in the bundle root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSetManifest {
    pub title: String,
    pub rationale: Option<String>,
    pub depends_on: Vec<Uuid>,
    pub supersedes: Option<Uuid>,
    pub artifacts: Vec<ArtifactManifestEntry>,
}

/// An entry in the manifest describing an artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactManifestEntry {
    pub artifact_type: ArtifactType,
    pub path: String,
    pub content_hash: Option<String>,
}

// ── Validation report ──────────────────────────────────────────

/// Stage discriminator for validation reports.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ValidationStage {
    Validate,
    DryRun,
}

/// A validation error with structured code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: String,
    pub severity: ErrorSeverity,
    pub message: String,
    pub artifact_path: Option<String>,
    pub line: Option<u32>,
    /// Optional structured context for diagnostics (e.g., dependency IDs,
    /// conflicting values). Skipped if None in serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Severity for validation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
}

/// Stage 1 (pure) validation report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct ValidationReport {
    pub ok: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationError>,
}

impl ValidationReport {
    pub fn empty_ok() -> Self {
        Self {
            ok: true,
            errors: vec![],
            warnings: vec![],
        }
    }
}

/// Stage 2 (DB-backed) dry-run report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct DryRunReport {
    pub ok: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationError>,
    pub scratch_schema_apply_ms: Option<u64>,
    pub diff_summary: Option<DiffSummary>,
}

// ── Diff summary ───────────────────────────────────────────────

/// Structural diff summary between two artifact sets or against active snapshot set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSummary {
    pub added: Vec<DiffEntry>,
    pub modified: Vec<DiffEntry>,
    pub removed: Vec<DiffEntry>,
    pub breaking_changes: Vec<DiffEntry>,
}

/// A single entry in a diff summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    pub fqn: String,
    pub object_type: String,
    pub detail: Option<String>,
}

// ── Publish plan (blast-radius analysis) ──────────────────────

/// Publish plan with blast-radius analysis.
/// Returned by `plan_publish` — read-only, does not modify state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishPlan {
    /// The ChangeSet being planned.
    pub change_set_id: Uuid,
    /// Current status of the ChangeSet.
    pub status: ChangeSetStatus,
    /// Structural diff summary.
    pub diff: DiffSummary,
    /// Number of forward DDL migrations in the bundle.
    pub migration_count: usize,
    /// Number of down (rollback) migrations in the bundle.
    pub down_migration_count: usize,
    /// Whether any breaking changes were detected.
    pub has_breaking_changes: bool,
    /// Count of breaking changes.
    pub breaking_change_count: usize,
    /// Distinct artifact types affected.
    pub affected_artifact_types: Vec<String>,
    /// Whether this ChangeSet supersedes another.
    pub supersedes: Option<Uuid>,
    /// IDs of ChangeSets this one depends on.
    pub depends_on: Vec<Uuid>,
    /// Whether a stale dry-run was detected (evaluated_against != current active).
    pub stale_dry_run: bool,
    /// The snapshot set the dry-run was evaluated against.
    pub evaluated_against_snapshot_set_id: Option<Uuid>,
    /// The current active snapshot set (for drift comparison).
    pub current_active_snapshot_set_id: Option<Uuid>,
}

// ── Governance audit ───────────────────────────────────────────

/// Result of a governance verb invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum AuditResult {
    Success { detail: Option<String> },
    Failure { code: String, message: String },
}

// AgentMode re-exported from agent_mode.rs (single canonical definition)
pub use super::agent_mode::AgentMode;

/// A governance audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceAuditEntry {
    pub entry_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub verb: String,
    pub agent_session_id: Option<Uuid>,
    pub agent_mode: Option<AgentMode>,
    pub change_set_id: Option<Uuid>,
    pub snapshot_set_id: Option<Uuid>,
    pub active_snapshot_set_id: Uuid,
    pub result: AuditResult,
    pub duration_ms: u64,
    pub metadata: Option<serde_json::Value>,
}

// ── Publish batch ──────────────────────────────────────────────

/// Record of a batch publish operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishBatch {
    pub batch_id: Uuid,
    pub change_set_ids: Vec<Uuid>,
    pub snapshot_set_id: Uuid,
    pub published_at: DateTime<Utc>,
    pub publisher: String,
}

// ── Health types ──────────────────────────────────────────────

/// Health check: active snapshot set info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSnapshotHealth {
    pub active_snapshot_set_id: Option<Uuid>,
    pub object_count: i64,
}

/// Health check: pending ChangeSet counts by status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingChangeSetsHealth {
    pub counts: Vec<StatusCount>,
    pub total_pending: i64,
}

/// A (status, count) pair for health reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

/// Health check: stale dry-run info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaleDryRunsHealth {
    pub stale_count: i64,
    pub stale_change_set_ids: Vec<Uuid>,
}

/// Health check: outbox projection lag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionLagHealth {
    pub unprocessed_events: i64,
    pub oldest_pending_age_seconds: Option<i64>,
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_set_status_round_trip() {
        for status in [
            ChangeSetStatus::Draft,
            ChangeSetStatus::UnderReview,
            ChangeSetStatus::Approved,
            ChangeSetStatus::Validated,
            ChangeSetStatus::Rejected,
            ChangeSetStatus::DryRunPassed,
            ChangeSetStatus::DryRunFailed,
            ChangeSetStatus::Published,
            ChangeSetStatus::Superseded,
        ] {
            let s: &str = status.as_ref();
            assert_eq!(
                s.parse::<ChangeSetStatus>().ok(),
                Some(status),
                "round-trip failed for {s}"
            );
        }
        // Alias: "in_review" should parse as UnderReview
        assert_eq!(
            "in_review".parse::<ChangeSetStatus>().ok(),
            Some(ChangeSetStatus::UnderReview)
        );
    }

    #[test]
    fn test_artifact_type_round_trip() {
        for at in [
            ArtifactType::MigrationSql,
            ArtifactType::MigrationDownSql,
            ArtifactType::VerbYaml,
            ArtifactType::AttributeJson,
            ArtifactType::TaxonomyJson,
            ArtifactType::DocJson,
        ] {
            let s: &str = at.as_ref();
            assert_eq!(
                s.parse::<ArtifactType>().ok(),
                Some(at),
                "round-trip failed for {s}"
            );
        }
    }

    #[test]
    fn test_terminal_statuses() {
        assert!(ChangeSetStatus::Published.is_terminal());
        assert!(ChangeSetStatus::Rejected.is_terminal());
        assert!(ChangeSetStatus::Superseded.is_terminal());
        assert!(ChangeSetStatus::DryRunFailed.is_terminal());
        assert!(!ChangeSetStatus::Draft.is_terminal());
        assert!(!ChangeSetStatus::UnderReview.is_terminal());
        assert!(!ChangeSetStatus::Approved.is_terminal());
        assert!(!ChangeSetStatus::Validated.is_terminal());
        assert!(!ChangeSetStatus::DryRunPassed.is_terminal());
    }

    #[test]
    fn test_validation_report_empty_ok() {
        let report = ValidationReport::empty_ok();
        assert!(report.ok);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_audit_result_serde() {
        let success = AuditResult::Success {
            detail: Some("all good".into()),
        };
        let json = serde_json::to_value(&success).unwrap();
        assert_eq!(json["status"], "success");

        let failure = AuditResult::Failure {
            code: "PUBLISH:DRIFT_DETECTED".into(),
            message: "snapshot set changed".into(),
        };
        let json = serde_json::to_value(&failure).unwrap();
        assert_eq!(json["status"], "failure");
        assert_eq!(json["code"], "PUBLISH:DRIFT_DETECTED");
    }
}
