//! Stage 2 (DB-backed) dry-run validation.
//! Validates: schema safety (scratch apply), compatibility checks, dependency statuses.
//! See: docs/semantic_os_research_governed_boundary_v0.4.md §6.2 Stage 2

use uuid::Uuid;

use super::diff::summarize_changeset;
use super::errors::*;
use super::ports::{AuthoringStore, ScratchSchemaRunner};
use super::types::*;

/// Run Stage 2 (DB-backed) dry-run validation.
///
/// Two phases:
///   4. Schema safety: apply migrations in scratch schema, check for forbidden DDL
///   5. Compatibility: diff against active snapshot set, check dependencies
pub async fn validate_stage2(
    change_set_id: Uuid,
    manifest: &ChangeSetManifest,
    artifacts: &[ChangeSetArtifact],
    scratch_runner: &dyn ScratchSchemaRunner,
    authoring_store: &dyn AuthoringStore,
) -> DryRunReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut scratch_apply_ms = None;

    // Phase 4: Schema safety
    let migration_artifacts: Vec<&ChangeSetArtifact> = artifacts
        .iter()
        .filter(|a| a.artifact_type == ArtifactType::MigrationSql)
        .collect();

    let down_migration_artifacts: Vec<&ChangeSetArtifact> = artifacts
        .iter()
        .filter(|a| a.artifact_type == ArtifactType::MigrationDownSql)
        .collect();

    if !migration_artifacts.is_empty() {
        validate_schema_safety(
            &migration_artifacts,
            &down_migration_artifacts,
            scratch_runner,
            &mut errors,
            &mut warnings,
            &mut scratch_apply_ms,
        )
        .await;
    }

    // Phase 5: Compatibility checks
    validate_compatibility(
        change_set_id,
        manifest,
        authoring_store,
        &mut errors,
        &mut warnings,
    )
    .await;

    // Compute diff summary for the changeset's artifacts
    let diff_summary = Some(summarize_changeset(artifacts));

    let ok = errors.is_empty();
    DryRunReport {
        ok,
        errors,
        warnings,
        scratch_schema_apply_ms: scratch_apply_ms,
        diff_summary,
    }
}

// ── Phase 4: Schema safety ───────────────────────────────────────

async fn validate_schema_safety(
    migrations: &[&ChangeSetArtifact],
    down_migrations: &[&ChangeSetArtifact],
    scratch_runner: &dyn ScratchSchemaRunner,
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<ValidationError>,
    scratch_apply_ms: &mut Option<u64>,
) {
    // Collect forward migration SQL in ordinal order
    let mut sorted_migrations: Vec<&&ChangeSetArtifact> = migrations.iter().collect();
    sorted_migrations.sort_by_key(|a| a.ordinal);

    // Build (path, sql) tuples for scratch runner
    let migration_tuples: Vec<(String, String)> = sorted_migrations
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

    let mut sorted_downs: Vec<&&ChangeSetArtifact> = down_migrations.iter().collect();
    sorted_downs.sort_by_key(|a| a.ordinal);
    let down_tuples: Vec<(String, String)> = sorted_downs
        .iter()
        .map(|a| {
            (
                a.path
                    .clone()
                    .unwrap_or_else(|| format!("down_ordinal_{}", a.ordinal)),
                a.content.clone(),
            )
        })
        .collect();

    // Check for non-transactional DDL (hard error in Stage 2)
    for migration in &sorted_migrations {
        let upper = migration.content.to_uppercase();
        if upper.contains("CONCURRENTLY") {
            errors.push(ValidationError {
                code: D_SCHEMA_NON_TRANSACTIONAL_DDL.to_string(),
                severity: ErrorSeverity::Error,
                message: format!(
                    "Migration contains CONCURRENTLY which cannot run in a transaction: {}",
                    migration.path.as_deref().unwrap_or("unknown")
                ),
                artifact_path: migration.path.clone(),
                line: find_concurrently_line(&migration.content),
                context: None,
            });
        }
    }

    // If we have non-transactional DDL errors, skip scratch apply
    if errors
        .iter()
        .any(|e| e.code == D_SCHEMA_NON_TRANSACTIONAL_DDL)
    {
        return;
    }

    // Run migrations in scratch schema
    let result = scratch_runner
        .run_scratch_migrations(&migration_tuples, &down_tuples)
        .await;

    match result {
        Ok(run_result) => {
            *scratch_apply_ms = Some(run_result.apply_ms);

            // Report apply errors
            for err_msg in &run_result.apply_errors {
                errors.push(ValidationError {
                    code: D_SCHEMA_APPLY_FAILED.to_string(),
                    severity: ErrorSeverity::Error,
                    message: format!("Scratch schema apply failed: {err_msg}"),
                    artifact_path: None,
                    line: None,
                    context: None,
                });
            }

            // Check for down migrations
            if down_migrations.is_empty() && !migrations.is_empty() {
                warnings.push(ValidationError {
                    code: D_SCHEMA_DOWN_MISSING.to_string(),
                    severity: ErrorSeverity::Warning,
                    message: "No down migration provided for forward migration(s)".to_string(),
                    artifact_path: None,
                    line: None,
                    context: None,
                });
            }

            // Report down migration errors
            for err_msg in &run_result.down_errors {
                warnings.push(ValidationError {
                    code: D_SCHEMA_DOWN_FAILED.to_string(),
                    severity: ErrorSeverity::Warning,
                    message: format!("Down migration failed in scratch: {err_msg}"),
                    artifact_path: None,
                    line: None,
                    context: None,
                });
            }
        }
        Err(e) => {
            errors.push(ValidationError {
                code: D_SCHEMA_APPLY_FAILED.to_string(),
                severity: ErrorSeverity::Error,
                message: format!("Scratch schema runner error: {e}"),
                artifact_path: None,
                line: None,
                context: None,
            });
        }
    }
}

fn find_concurrently_line(content: &str) -> Option<u32> {
    for (i, line) in content.lines().enumerate() {
        if line.to_uppercase().contains("CONCURRENTLY") {
            return Some((i + 1) as u32);
        }
    }
    None
}

// ── Phase 5: Compatibility checks ────────────────────────────────

async fn validate_compatibility(
    change_set_id: Uuid,
    manifest: &ChangeSetManifest,
    authoring_store: &dyn AuthoringStore,
    errors: &mut Vec<ValidationError>,
    _warnings: &mut Vec<ValidationError>,
) {
    // Check dependency statuses
    for dep_id in &manifest.depends_on {
        match authoring_store.get_change_set(*dep_id).await {
            Ok(dep_cs) => {
                match dep_cs.status {
                    ChangeSetStatus::Published => {
                        // Good — dependency is published
                    }
                    ChangeSetStatus::Rejected | ChangeSetStatus::DryRunFailed => {
                        errors.push(ValidationError {
                            code: D_COMPAT_DEPENDENCY_FAILED.to_string(),
                            severity: ErrorSeverity::Error,
                            message: format!(
                                "Dependency {dep_id} is in failed state: {}",
                                dep_cs.status
                            ),
                            artifact_path: None,
                            line: None,
                            context: None,
                        });
                    }
                    _ => {
                        errors.push(ValidationError {
                            code: D_COMPAT_DEPENDENCY_UNPUBLISHED.to_string(),
                            severity: ErrorSeverity::Error,
                            message: format!(
                                "Dependency {dep_id} is not yet published (status: {})",
                                dep_cs.status
                            ),
                            artifact_path: None,
                            line: None,
                            context: None,
                        });
                    }
                }
            }
            Err(e) => {
                // NotFound → missing dependency; other errors → report as-is
                let err_str = e.to_string();
                if err_str.contains("not found") || err_str.contains("NotFound") {
                    errors.push(ValidationError {
                        code: V_REF_MISSING_DEPENDENCY.to_string(),
                        severity: ErrorSeverity::Error,
                        message: format!("Dependency ChangeSet {dep_id} not found"),
                        artifact_path: None,
                        line: None,
                        context: None,
                    });
                } else {
                    errors.push(ValidationError {
                        code: D_SCHEMA_APPLY_FAILED.to_string(),
                        severity: ErrorSeverity::Error,
                        message: format!("Error checking dependency {dep_id}: {e}"),
                        artifact_path: None,
                        line: None,
                        context: None,
                    });
                }
            }
        }
    }

    // Check for supersession conflicts
    if let Some(supersedes_id) = manifest.supersedes {
        match authoring_store.get_change_set(supersedes_id).await {
            Ok(target) => {
                if target.superseded_by.is_some() {
                    errors.push(ValidationError {
                        code: D_COMPAT_SUPERSESSION_CONFLICT.to_string(),
                        severity: ErrorSeverity::Error,
                        message: format!(
                            "ChangeSet {supersedes_id} is already superseded by {:?}",
                            target.superseded_by
                        ),
                        artifact_path: None,
                        line: None,
                        context: None,
                    });
                }
                if !target.status.is_terminal() {
                    errors.push(ValidationError {
                        code: D_COMPAT_SUPERSESSION_CONFLICT.to_string(),
                        severity: ErrorSeverity::Error,
                        message: format!(
                            "Cannot supersede ChangeSet {supersedes_id} — it is not in a terminal state ({})",
                            target.status
                        ),
                        artifact_path: None,
                        line: None,
                        context: None,
                    });
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("not found") || err_str.contains("NotFound") {
                    errors.push(ValidationError {
                        code: V_REF_MISSING_DEPENDENCY.to_string(),
                        severity: ErrorSeverity::Error,
                        message: format!("Supersedes target ChangeSet {supersedes_id} not found"),
                        artifact_path: None,
                        line: None,
                        context: None,
                    });
                } else {
                    errors.push(ValidationError {
                        code: D_SCHEMA_APPLY_FAILED.to_string(),
                        severity: ErrorSeverity::Error,
                        message: format!("Error checking supersession target: {e}"),
                        artifact_path: None,
                        line: None,
                        context: None,
                    });
                }
            }
        }
    }

    let _ = change_set_id; // Used for future drift detection
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_concurrently_detection() {
        let line =
            find_concurrently_line("SELECT 1;\nCREATE INDEX CONCURRENTLY idx ON t(c);\nSELECT 2;");
        assert_eq!(line, Some(2));
    }

    #[test]
    fn test_concurrently_case_insensitive() {
        let line = find_concurrently_line("create index concurrently idx on t(c);");
        assert_eq!(line, Some(1));
    }
}
