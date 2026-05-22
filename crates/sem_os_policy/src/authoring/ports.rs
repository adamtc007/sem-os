//! Storage port traits for the authoring pipeline.
//! Implemented by sem_os_postgres — core logic depends only on these traits.

use async_trait::async_trait;
use uuid::Uuid;

use sem_os_core::error::SemOsError;
use sem_os_core::principal::Principal;

use super::types::*;

pub type Result<T> = std::result::Result<T, SemOsError>;

/// Storage operations for the authoring pipeline.
///
/// Operates on the same `sem_reg.changesets` table as `ChangesetStore`
/// but with extended columns (content_hash, depends_on, etc.) and
/// the `sem_reg_authoring.*` tables (artifacts, reports, audit, batches).
#[async_trait]
pub trait AuthoringStore: Send + Sync {
    // ── ChangeSet CRUD ─────────────────────────────────────────

    /// Create a new ChangeSet in Draft status.
    /// Returns the created ChangeSet with its assigned ID.
    #[allow(clippy::too_many_arguments)]
    async fn create_change_set(
        &self,
        principal: &Principal,
        title: &str,
        rationale: Option<&str>,
        content_hash: &str,
        hash_version: &str,
        depends_on: &[Uuid],
        supersedes: Option<Uuid>,
    ) -> Result<ChangeSetFull>;

    /// Load a ChangeSet by ID.
    async fn get_change_set(&self, change_set_id: Uuid) -> Result<ChangeSetFull>;

    /// Find a ChangeSet by content hash (idempotent propose).
    /// Returns None if no active (non-rejected, non-superseded) match.
    async fn find_by_content_hash(
        &self,
        hash_version: &str,
        content_hash: &str,
    ) -> Result<Option<ChangeSetFull>>;

    /// Update a ChangeSet's status.
    async fn update_change_set_status(
        &self,
        change_set_id: Uuid,
        new_status: ChangeSetStatus,
    ) -> Result<()>;

    /// Record the snapshot set ID that was active when dry-run was evaluated.
    async fn set_evaluated_against(&self, change_set_id: Uuid, snapshot_set_id: Uuid)
        -> Result<()>;

    /// Mark a ChangeSet as superseded by another.
    async fn mark_superseded(&self, change_set_id: Uuid, superseded_by: Uuid) -> Result<()>;

    /// List ChangeSets with optional filters.
    async fn list_change_sets(
        &self,
        status: Option<ChangeSetStatus>,
        limit: i64,
    ) -> Result<Vec<ChangeSetFull>>;

    // ── Artifacts ──────────────────────────────────────────────

    /// Insert artifacts for a ChangeSet (batch).
    async fn insert_artifacts(
        &self,
        change_set_id: Uuid,
        artifacts: &[ChangeSetArtifact],
    ) -> Result<()>;

    /// Load all artifacts for a ChangeSet.
    async fn get_artifacts(&self, change_set_id: Uuid) -> Result<Vec<ChangeSetArtifact>>;

    // ── Validation reports ─────────────────────────────────────

    /// Append a validation report.
    async fn insert_validation_report(
        &self,
        change_set_id: Uuid,
        stage: ValidationStage,
        ok: bool,
        report: &serde_json::Value,
    ) -> Result<Uuid>;

    /// Get validation reports for a ChangeSet.
    async fn get_validation_reports(
        &self,
        change_set_id: Uuid,
    ) -> Result<Vec<(Uuid, ValidationStage, bool, serde_json::Value)>>;

    // ── Governance audit log ───────────────────────────────────

    /// Append a governance audit entry.
    async fn insert_audit_entry(&self, entry: &GovernanceAuditEntry) -> Result<()>;

    // ── Publish batches ────────────────────────────────────────

    /// Record a batch publish operation.
    async fn insert_publish_batch(&self, batch: &PublishBatch) -> Result<()>;

    // ── Snapshot set queries ─────────────────────────────────────

    /// Get the current active snapshot set ID.
    /// Used by dry-run to record `evaluated_against_snapshot_set_id`,
    /// and by publish for drift detection.
    async fn get_active_snapshot_set_id(&self) -> Result<Option<Uuid>>;

    // ── Publish support ───────────────────────────────────────────

    /// Acquire an advisory lock for exclusive publish access.
    /// Returns `true` if the lock was acquired, `false` if contended.
    /// The lock is released when the underlying transaction commits/rolls back.
    async fn try_acquire_publish_lock(&self) -> Result<bool>;

    /// Apply forward DDL migration SQL against the real database (not scratch).
    /// Used during publish to apply schema changes.
    async fn apply_migrations(&self, migrations: &[(String, String)]) -> Result<()>;

    /// Create a new snapshot set and set it as the active snapshot set.
    /// Returns the new snapshot set ID.
    async fn create_and_activate_snapshot_set(
        &self,
        change_set_ids: &[Uuid],
        publisher: &str,
    ) -> Result<Uuid>;

    // ── Health queries ───────────────────────────────────────────

    /// Count ChangeSets grouped by status.
    async fn count_by_status(&self) -> Result<Vec<(ChangeSetStatus, i64)>>;

    /// Find ChangeSets where `evaluated_against_snapshot_set_id` does not
    /// match the current active snapshot set (stale dry-runs).
    async fn find_stale_dry_runs(&self) -> Result<Vec<ChangeSetFull>>;
}

/// Scratch schema runner for Stage 2 dry-run validation.
///
/// Creates a temporary schema, applies migrations within a transaction,
/// then rolls back. Implemented in sem_os_postgres.
#[async_trait]
pub trait ScratchSchemaRunner: Send + Sync {
    /// Apply migration SQL files in a scratch schema within a transaction,
    /// then ROLLBACK. Returns timing and any errors.
    ///
    /// - `migrations`: ordered list of (path, sql_content) pairs
    /// - `down_migrations`: ordered list of (path, sql_content) for cleanup validation
    ///
    /// Returns (apply_ms, errors)
    async fn run_scratch_migrations(
        &self,
        migrations: &[(String, String)],
        down_migrations: &[(String, String)],
    ) -> Result<ScratchRunResult>;
}

/// Result of a scratch schema migration run.
#[derive(Debug, Clone)]
pub struct ScratchRunResult {
    /// Time in milliseconds to apply forward migrations.
    pub apply_ms: u64,
    /// Errors encountered during apply (empty = success).
    pub apply_errors: Vec<String>,
    /// Errors encountered during down-migration validation (empty = success).
    pub down_errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratch_run_result_default() {
        let result = ScratchRunResult {
            apply_ms: 42,
            apply_errors: vec![],
            down_errors: vec![],
        };
        assert_eq!(result.apply_ms, 42);
        assert!(result.apply_errors.is_empty());
    }
}
