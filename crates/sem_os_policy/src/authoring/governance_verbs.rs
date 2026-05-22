//! Governance verb service — orchestrates the 7 governance verbs.
//!
//! | Verb                   | Status transition               | Key logic                        |
//! |------------------------|---------------------------------|----------------------------------|
//! | propose_change_set     | → Draft                        | Parse bundle, content_hash       |
//! | validate_change_set    | Draft → Validated/Rejected      | Run Stage 1, save report         |
//! | dry_run_change_set     | Validated → DryRunPassed/Failed | Run Stage 2, save report         |
//! | plan_publish           | (read-only)                     | Diff against active              |
//! | publish_snapshot_set   | DryRunPassed → Published        | Advisory lock, drift, apply, pub |
//! | rollback_snapshot_set  | (pointer revert)                | Revert active_snapshot_set       |
//! | diff_change_sets       | (read-only)                     | Structural diff                  |
//!
//! See: docs/semantic_os_research_governed_boundary_v0.4.md §6.3

use chrono::Utc;
use uuid::Uuid;

use sem_os_core::error::SemOsError;

use super::ports::Result;
use sem_os_core::principal::Principal;

use super::bundle::BundleContents;
use super::canonical_hash::{compute_content_hash, HASH_VERSION};
use super::diff::{diff_changesets, summarize_changeset};
use super::ports::{AuthoringStore, ScratchSchemaRunner};
use super::types::*;
use super::validate_stage1::validate_stage1;
use super::validate_stage2::validate_stage2;

/// Central governance verb service.
pub struct GovernanceVerbService<'a> {
    authoring_store: &'a dyn AuthoringStore,
    scratch_runner: &'a dyn ScratchSchemaRunner,
}

impl<'a> GovernanceVerbService<'a> {
    pub fn new(
        authoring_store: &'a dyn AuthoringStore,
        scratch_runner: &'a dyn ScratchSchemaRunner,
    ) -> Self {
        Self {
            authoring_store,
            scratch_runner,
        }
    }

    // ── 1. propose_change_set ─────────────────────────────────────

    /// Create a new ChangeSet from a parsed bundle. Content-addressed idempotent:
    /// if a ChangeSet with the same (hash_version, content_hash) exists in a
    /// non-terminal state, returns the existing one.
    pub async fn propose(
        &self,
        bundle: &BundleContents,
        principal: &Principal,
    ) -> Result<ChangeSetFull> {
        let content_hash = compute_content_hash(&bundle.manifest, &bundle.artifacts);

        // Idempotent check
        if let Some(existing) = self
            .authoring_store
            .find_by_content_hash(HASH_VERSION, &content_hash)
            .await?
        {
            super::metrics::emit_propose(existing.change_set_id, &existing.title, true);
            return Ok(existing);
        }

        // Create new ChangeSet in Draft status
        let cs = self
            .authoring_store
            .create_change_set(
                principal,
                &bundle.manifest.title,
                bundle.manifest.rationale.as_deref(),
                &content_hash,
                HASH_VERSION,
                &bundle.manifest.depends_on,
                bundle.manifest.supersedes,
            )
            .await?;

        // Insert artifacts
        self.authoring_store
            .insert_artifacts(cs.change_set_id, &bundle.artifacts)
            .await?;

        super::metrics::emit_propose(cs.change_set_id, &cs.title, false);
        Ok(cs)
    }

    // ── 2. validate_change_set ────────────────────────────────────

    /// Run Stage 1 (pure) validation. Transitions Draft → Validated or Rejected.
    pub async fn validate(&self, change_set_id: Uuid) -> Result<ValidationReport> {
        let cs = self.authoring_store.get_change_set(change_set_id).await?;

        // Guard: must be in Draft status
        if cs.status != ChangeSetStatus::Draft {
            return Err(SemOsError::InvalidInput(format!(
                "Cannot validate ChangeSet in status '{}' — must be Draft",
                cs.status
            )));
        }

        let artifacts = self.authoring_store.get_artifacts(change_set_id).await?;

        let report = validate_stage1(&cs_to_manifest(&cs), &artifacts);

        // Save report
        let report_json = serde_json::to_value(&report)
            .map_err(|e| SemOsError::InvalidInput(format!("Failed to serialize report: {e}")))?;
        self.authoring_store
            .insert_validation_report(
                change_set_id,
                ValidationStage::Validate,
                report.ok,
                &report_json,
            )
            .await?;

        // Transition status
        let new_status = if report.ok {
            ChangeSetStatus::Validated
        } else {
            ChangeSetStatus::Rejected
        };

        self.authoring_store
            .update_change_set_status(change_set_id, new_status)
            .await?;

        super::metrics::emit_validate(change_set_id, &report);
        super::metrics::emit_status_transition(change_set_id, ChangeSetStatus::Draft, new_status);
        Ok(report)
    }

    // ── 3. dry_run_change_set ─────────────────────────────────────

    /// Run Stage 2 (DB-backed) dry-run. Transitions Validated → DryRunPassed or DryRunFailed.
    pub async fn dry_run(&self, change_set_id: Uuid) -> Result<DryRunReport> {
        let cs = self.authoring_store.get_change_set(change_set_id).await?;

        // Guard: must be in Validated status
        if cs.status != ChangeSetStatus::Validated {
            return Err(SemOsError::InvalidInput(format!(
                "Cannot dry-run ChangeSet in status '{}' — must be Validated",
                cs.status
            )));
        }

        let artifacts = self.authoring_store.get_artifacts(change_set_id).await?;
        let manifest = cs_to_manifest(&cs);

        let report = validate_stage2(
            change_set_id,
            &manifest,
            &artifacts,
            self.scratch_runner,
            self.authoring_store,
        )
        .await;

        // Record evaluated_against_snapshot_set_id for drift detection
        if let Some(snapshot_set_id) = self.authoring_store.get_active_snapshot_set_id().await? {
            self.authoring_store
                .set_evaluated_against(change_set_id, snapshot_set_id)
                .await?;
        }

        // Save report
        let report_json = serde_json::to_value(&report).map_err(|e| {
            SemOsError::InvalidInput(format!("Failed to serialize dry-run report: {e}"))
        })?;
        self.authoring_store
            .insert_validation_report(
                change_set_id,
                ValidationStage::DryRun,
                report.ok,
                &report_json,
            )
            .await?;

        // Transition status
        let new_status = if report.ok {
            ChangeSetStatus::DryRunPassed
        } else {
            ChangeSetStatus::DryRunFailed
        };

        self.authoring_store
            .update_change_set_status(change_set_id, new_status)
            .await?;

        super::metrics::emit_dry_run(change_set_id, &report);
        super::metrics::emit_status_transition(
            change_set_id,
            ChangeSetStatus::Validated,
            new_status,
        );
        Ok(report)
    }

    // ── 4. plan_publish (read-only) ───────────────────────────────

    /// Generate a publish plan with blast-radius analysis for a ChangeSet.
    /// Does not modify state — read-only.
    pub async fn plan_publish(&self, change_set_id: Uuid) -> Result<PublishPlan> {
        let cs = self.authoring_store.get_change_set(change_set_id).await?;

        if cs.status != ChangeSetStatus::DryRunPassed {
            return Err(SemOsError::InvalidInput(format!(
                "Cannot plan publish for ChangeSet in status '{}' — must be DryRunPassed",
                cs.status
            )));
        }

        let artifacts = self.authoring_store.get_artifacts(change_set_id).await?;
        let diff = summarize_changeset(&artifacts);

        // Count migration artifacts
        let migration_count = artifacts
            .iter()
            .filter(|a| a.artifact_type == ArtifactType::MigrationSql)
            .count();
        let down_migration_count = artifacts
            .iter()
            .filter(|a| a.artifact_type == ArtifactType::MigrationDownSql)
            .count();

        // Collect distinct artifact types
        let mut affected_types: Vec<String> = artifacts
            .iter()
            .map(|a| a.artifact_type.to_string())
            .collect();
        affected_types.sort();
        affected_types.dedup();

        // Check for stale dry-run (drift detection preview)
        let current_active = self.authoring_store.get_active_snapshot_set_id().await?;
        let stale_dry_run = match (cs.evaluated_against_snapshot_set_id, current_active) {
            (Some(evaluated), Some(active)) => evaluated != active,
            (None, _) => true,       // No evaluation recorded = stale
            (Some(_), None) => true, // Active gone = stale
        };

        super::metrics::emit_plan_publish(change_set_id, &diff);

        Ok(PublishPlan {
            change_set_id,
            status: cs.status,
            has_breaking_changes: !diff.breaking_changes.is_empty(),
            breaking_change_count: diff.breaking_changes.len(),
            diff,
            migration_count,
            down_migration_count,
            affected_artifact_types: affected_types,
            supersedes: cs.supersedes_change_set_id,
            depends_on: cs.depends_on,
            stale_dry_run,
            evaluated_against_snapshot_set_id: cs.evaluated_against_snapshot_set_id,
            current_active_snapshot_set_id: current_active,
        })
    }

    // ── 5. publish_snapshot_set ───────────────────────────────────

    /// Publish a ChangeSet. Transitions DryRunPassed → Published.
    ///
    /// Steps:
    ///   1. Verify status == DryRunPassed
    ///   2. Acquire advisory lock (single-publisher gate)
    ///   3. Drift detection: evaluated_against must match current active
    ///   4. Apply DDL migrations (forward-only)
    ///   5. Transition → Published
    ///   6. Handle supersession chain
    ///   7. Create snapshot set + write audit entry
    pub async fn publish(&self, change_set_id: Uuid, publisher: &str) -> Result<PublishBatch> {
        let start = std::time::Instant::now();
        let cs = self.authoring_store.get_change_set(change_set_id).await?;

        // Step 1: Status guard
        if cs.status != ChangeSetStatus::DryRunPassed {
            return Err(SemOsError::InvalidInput(format!(
                "Cannot publish ChangeSet in status '{}' — must be DryRunPassed",
                cs.status
            )));
        }

        // Step 2: Acquire advisory lock for single-publisher access
        let lock_acquired = self.authoring_store.try_acquire_publish_lock().await?;
        if !lock_acquired {
            self.emit_publish_failure(
                change_set_id,
                super::errors::PUBLISH_LOCK_CONTENTION,
                "Another publish is in progress",
                start.elapsed().as_millis() as u64,
            )
            .await;
            return Err(SemOsError::Conflict(
                "Could not acquire publish lock — another publish is in progress".to_string(),
            ));
        }

        // Step 3: Drift detection — evaluated_against must match current active
        let current_active = self.authoring_store.get_active_snapshot_set_id().await?;
        if let Some(evaluated_against) = cs.evaluated_against_snapshot_set_id {
            if current_active != Some(evaluated_against) {
                self.emit_publish_failure(
                    change_set_id,
                    super::errors::PUBLISH_DRIFT_DETECTED,
                    &format!(
                        "Dry-run was evaluated against snapshot set {:?}, \
                         but current active is {:?}. Re-run dry-run first.",
                        evaluated_against, current_active
                    ),
                    start.elapsed().as_millis() as u64,
                )
                .await;
                return Err(SemOsError::Conflict(format!(
                    "Snapshot set drift detected: dry-run evaluated against {}, \
                     but current active is {}. Re-run dry-run.",
                    evaluated_against,
                    current_active
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "<none>".to_string())
                )));
            }
        }

        // Step 4: Apply DDL migrations (forward-only, against real database)
        let artifacts = self.authoring_store.get_artifacts(change_set_id).await?;
        let mut migration_artifacts: Vec<&ChangeSetArtifact> = artifacts
            .iter()
            .filter(|a| a.artifact_type == ArtifactType::MigrationSql)
            .collect();
        migration_artifacts.sort_by_key(|a| a.ordinal);

        if !migration_artifacts.is_empty() {
            let migration_tuples: Vec<(String, String)> = migration_artifacts
                .iter()
                .map(|a| {
                    (
                        a.path
                            .clone()
                            .unwrap_or_else(|| format!("ordinal_{}", a.ordinal)),
                        a.content.clone(),
                    )
                })
                .collect();

            self.authoring_store
                .apply_migrations(&migration_tuples)
                .await?;
        }

        // Step 5: Transition to Published
        self.authoring_store
            .update_change_set_status(change_set_id, ChangeSetStatus::Published)
            .await?;

        // Step 6: Handle supersession
        if let Some(supersedes_id) = cs.supersedes_change_set_id {
            self.authoring_store
                .mark_superseded(supersedes_id, change_set_id)
                .await?;
        }

        // Step 7: Create snapshot set + write audit entry + publish batch record
        let snapshot_set_id = self
            .authoring_store
            .create_and_activate_snapshot_set(&[change_set_id], publisher)
            .await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        let batch = PublishBatch {
            batch_id: Uuid::new_v4(),
            change_set_ids: vec![change_set_id],
            snapshot_set_id,
            published_at: Utc::now(),
            publisher: publisher.to_string(),
        };

        self.authoring_store.insert_publish_batch(&batch).await?;

        self.authoring_store
            .insert_audit_entry(&GovernanceAuditEntry {
                entry_id: Uuid::new_v4(),
                timestamp: Utc::now(),
                verb: "publish_snapshot_set".to_string(),
                agent_session_id: None,
                agent_mode: None,
                change_set_id: Some(change_set_id),
                snapshot_set_id: Some(snapshot_set_id),
                active_snapshot_set_id: snapshot_set_id,
                result: AuditResult::Success { detail: None },
                duration_ms,
                metadata: None,
            })
            .await?;

        super::metrics::emit_publish(change_set_id, batch.batch_id, publisher);
        super::metrics::emit_status_transition(
            change_set_id,
            ChangeSetStatus::DryRunPassed,
            ChangeSetStatus::Published,
        );
        Ok(batch)
    }

    // ── 6. publish_batch ──────────────────────────────────────────

    /// Publish multiple ChangeSets atomically in topological order.
    ///
    /// Steps:
    ///   1. Topological sort by depends_on
    ///   2. Verify all are DryRunPassed
    ///   3. Acquire advisory lock
    ///   4. Apply DDL migrations in topological order
    ///   5. Transition all → Published
    ///   6. Create single snapshot set for the batch
    pub async fn publish_batch(
        &self,
        change_set_ids: &[Uuid],
        publisher: &str,
    ) -> Result<PublishBatch> {
        let start = std::time::Instant::now();

        // Step 1: Topological sort by depends_on
        let sorted = topological_sort(change_set_ids, self.authoring_store).await?;

        // Step 2: Verify all are DryRunPassed
        for cs_id in &sorted {
            let cs = self.authoring_store.get_change_set(*cs_id).await?;
            if cs.status != ChangeSetStatus::DryRunPassed {
                return Err(SemOsError::InvalidInput(format!(
                    "ChangeSet {} is not DryRunPassed (status: {})",
                    cs_id, cs.status
                )));
            }
        }

        // Step 3: Acquire advisory lock
        let lock_acquired = self.authoring_store.try_acquire_publish_lock().await?;
        if !lock_acquired {
            return Err(SemOsError::Conflict(
                "Could not acquire publish lock — another publish is in progress".to_string(),
            ));
        }

        // Step 4: Apply DDL migrations in topological order
        for cs_id in &sorted {
            let artifacts = self.authoring_store.get_artifacts(*cs_id).await?;
            let mut migration_artifacts: Vec<&ChangeSetArtifact> = artifacts
                .iter()
                .filter(|a| a.artifact_type == ArtifactType::MigrationSql)
                .collect();
            migration_artifacts.sort_by_key(|a| a.ordinal);

            if !migration_artifacts.is_empty() {
                let migration_tuples: Vec<(String, String)> = migration_artifacts
                    .iter()
                    .map(|a| {
                        (
                            a.path
                                .clone()
                                .unwrap_or_else(|| format!("ordinal_{}", a.ordinal)),
                            a.content.clone(),
                        )
                    })
                    .collect();

                self.authoring_store
                    .apply_migrations(&migration_tuples)
                    .await?;
            }
        }

        // Step 5: Transition all → Published
        for cs_id in &sorted {
            self.authoring_store
                .update_change_set_status(*cs_id, ChangeSetStatus::Published)
                .await?;
        }

        // Step 6: Create single snapshot set for the batch
        let snapshot_set_id = self
            .authoring_store
            .create_and_activate_snapshot_set(&sorted, publisher)
            .await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        let batch = PublishBatch {
            batch_id: Uuid::new_v4(),
            change_set_ids: sorted.clone(),
            snapshot_set_id,
            published_at: Utc::now(),
            publisher: publisher.to_string(),
        };

        self.authoring_store.insert_publish_batch(&batch).await?;

        self.authoring_store
            .insert_audit_entry(&GovernanceAuditEntry {
                entry_id: Uuid::new_v4(),
                timestamp: Utc::now(),
                verb: "publish_batch".to_string(),
                agent_session_id: None,
                agent_mode: None,
                change_set_id: None,
                snapshot_set_id: Some(snapshot_set_id),
                active_snapshot_set_id: snapshot_set_id,
                result: AuditResult::Success {
                    detail: Some(format!("{} ChangeSets published", sorted.len())),
                },
                duration_ms,
                metadata: None,
            })
            .await?;

        super::metrics::emit_publish_batch(batch.batch_id, sorted.len(), publisher);
        Ok(batch)
    }

    // ── 7. diff_change_sets (read-only) ───────────────────────────

    /// Compute structural diff between two ChangeSets.
    pub async fn diff(&self, base_id: Uuid, target_id: Uuid) -> Result<DiffSummary> {
        let base_artifacts = self.authoring_store.get_artifacts(base_id).await?;
        let target_artifacts = self.authoring_store.get_artifacts(target_id).await?;
        let diff = diff_changesets(&base_artifacts, &target_artifacts);
        super::metrics::emit_diff(base_id, target_id, &diff);
        Ok(diff)
    }

    // ── Internal helpers ─────────────────────────────────────────

    /// Record a publish failure in the governance audit log.
    async fn emit_publish_failure(
        &self,
        change_set_id: Uuid,
        error_code: &str,
        message: &str,
        duration_ms: u64,
    ) {
        let active_snapshot = self
            .authoring_store
            .get_active_snapshot_set_id()
            .await
            .ok()
            .flatten()
            .unwrap_or_else(Uuid::nil);

        let _ = self
            .authoring_store
            .insert_audit_entry(&GovernanceAuditEntry {
                entry_id: Uuid::new_v4(),
                timestamp: Utc::now(),
                verb: "publish_snapshot_set".to_string(),
                agent_session_id: None,
                agent_mode: None,
                change_set_id: Some(change_set_id),
                snapshot_set_id: None,
                active_snapshot_set_id: active_snapshot,
                result: AuditResult::Failure {
                    code: error_code.to_string(),
                    message: message.to_string(),
                },
                duration_ms,
                metadata: None,
            })
            .await;
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// Reconstruct a manifest from a stored ChangeSetFull.
fn cs_to_manifest(cs: &ChangeSetFull) -> ChangeSetManifest {
    ChangeSetManifest {
        title: cs.title.clone(),
        rationale: cs.rationale.clone(),
        depends_on: cs.depends_on.clone(),
        supersedes: cs.supersedes_change_set_id,
        artifacts: vec![], // Artifacts loaded separately
    }
}

/// Simple topological sort for batch publish.
/// Returns an ordering where dependencies come before dependents.
async fn topological_sort(ids: &[Uuid], store: &dyn AuthoringStore) -> Result<Vec<Uuid>> {
    use std::collections::{HashMap, HashSet, VecDeque};

    let id_set: HashSet<Uuid> = ids.iter().copied().collect();
    let mut deps_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let mut in_degree: HashMap<Uuid, usize> = HashMap::new();

    // Initialize
    for &id in ids {
        in_degree.insert(id, 0);
        deps_map.insert(id, vec![]);
    }

    // Build dependency graph (only within the batch)
    for &id in ids {
        let cs = store.get_change_set(id).await?;
        for dep in &cs.depends_on {
            if id_set.contains(dep) {
                deps_map.entry(*dep).or_default().push(id);
                *in_degree.entry(id).or_default() += 1;
            }
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<Uuid> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut sorted = Vec::with_capacity(ids.len());

    while let Some(id) = queue.pop_front() {
        sorted.push(id);
        if let Some(dependents) = deps_map.get(&id) {
            for &dep in dependents {
                let deg = in_degree.get_mut(&dep).ok_or_else(|| {
                    SemOsError::Internal(anyhow::anyhow!(
                        "topo sort invariant violated: dep {dep} not in in_degree map"
                    ))
                })?;
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(dep);
                }
            }
        }
    }

    if sorted.len() != ids.len() {
        return Err(SemOsError::InvalidInput(
            "Circular dependency detected in batch publish".to_string(),
        ));
    }

    Ok(sorted)
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cs_to_manifest_round_trip() {
        let cs = ChangeSetFull {
            change_set_id: Uuid::new_v4(),
            status: ChangeSetStatus::Draft,
            content_hash: "abc".to_string(),
            hash_version: "v1".to_string(),
            title: "Test title".to_string(),
            rationale: Some("Test rationale".to_string()),
            created_by: "test".to_string(),
            scope: "authoring".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            supersedes_change_set_id: None,
            superseded_by: None,
            superseded_at: None,
            depends_on: vec![Uuid::new_v4()],
            evaluated_against_snapshot_set_id: None,
        };

        let manifest = cs_to_manifest(&cs);
        assert_eq!(manifest.title, cs.title);
        assert_eq!(manifest.rationale, cs.rationale);
        assert_eq!(manifest.depends_on, cs.depends_on);
        assert_eq!(manifest.supersedes, cs.supersedes_change_set_id);
    }
}
