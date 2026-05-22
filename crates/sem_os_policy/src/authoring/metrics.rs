//! Structured tracing events for governance verb observability.
//!
//! Each governance verb emits a structured tracing event on completion.
//! These events are consumed by any tracing subscriber (stdout, OTLP, etc.)
//! for dashboards, alerting, and audit.
//!
//! Event naming convention: `authoring.<verb>` with structured fields.

use uuid::Uuid;

use super::types::{ChangeSetStatus, DiffSummary, DryRunReport, ValidationReport};

/// Emit a structured tracing event for `propose_change_set`.
pub fn emit_propose(change_set_id: Uuid, title: &str, idempotent_hit: bool) {
    tracing::info!(
        target: "authoring.propose",
        %change_set_id,
        title,
        idempotent_hit,
        "changeset proposed"
    );
}

/// Emit a structured tracing event for `validate_change_set`.
pub fn emit_validate(change_set_id: Uuid, report: &ValidationReport) {
    let error_count = report.errors.len();
    let warning_count = report.warnings.len();
    tracing::info!(
        target: "authoring.validate",
        %change_set_id,
        ok = report.ok,
        error_count,
        warning_count,
        "changeset validated"
    );
}

/// Emit a structured tracing event for `dry_run_change_set`.
pub fn emit_dry_run(change_set_id: Uuid, report: &DryRunReport) {
    let error_count = report.errors.len();
    let warning_count = report.warnings.len();
    let apply_ms = report.scratch_schema_apply_ms.unwrap_or(0);
    tracing::info!(
        target: "authoring.dry_run",
        %change_set_id,
        ok = report.ok,
        error_count,
        warning_count,
        apply_ms,
        "changeset dry-run completed"
    );
}

/// Emit a structured tracing event for `plan_publish`.
pub fn emit_plan_publish(change_set_id: Uuid, diff: &DiffSummary) {
    let added = diff.added.len();
    let modified = diff.modified.len();
    let removed = diff.removed.len();
    let breaking = diff.breaking_changes.len();
    tracing::info!(
        target: "authoring.plan_publish",
        %change_set_id,
        added,
        modified,
        removed,
        breaking,
        "publish plan generated"
    );
}

/// Emit a structured tracing event for `publish_snapshot_set`.
pub fn emit_publish(change_set_id: Uuid, batch_id: Uuid, publisher: &str) {
    tracing::info!(
        target: "authoring.publish",
        %change_set_id,
        %batch_id,
        publisher,
        "changeset published"
    );
}

/// Emit a structured tracing event for `publish_batch`.
pub fn emit_publish_batch(batch_id: Uuid, count: usize, publisher: &str) {
    tracing::info!(
        target: "authoring.publish_batch",
        %batch_id,
        changeset_count = count,
        publisher,
        "batch published"
    );
}

/// Emit a structured tracing event for `diff_change_sets`.
pub fn emit_diff(base_id: Uuid, target_id: Uuid, diff: &DiffSummary) {
    let added = diff.added.len();
    let modified = diff.modified.len();
    let removed = diff.removed.len();
    tracing::info!(
        target: "authoring.diff",
        %base_id,
        %target_id,
        added,
        modified,
        removed,
        "changeset diff computed"
    );
}

/// Emit a structured tracing event for status transitions.
pub fn emit_status_transition(change_set_id: Uuid, from: ChangeSetStatus, to: ChangeSetStatus) {
    tracing::info!(
        target: "authoring.status_transition",
        %change_set_id,
        from = from.as_ref(),
        to = to.as_ref(),
        "changeset status transition"
    );
}

/// Emit a warning event for governance verb errors.
pub fn emit_governance_error(verb: &str, change_set_id: Option<Uuid>, error: &str) {
    tracing::warn!(
        target: "authoring.error",
        verb,
        change_set_id = change_set_id.map(|id| id.to_string()).as_deref(),
        error,
        "governance verb error"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_propose_does_not_panic() {
        emit_propose(Uuid::new_v4(), "Test title", false);
    }

    #[test]
    fn test_emit_validate_does_not_panic() {
        emit_validate(Uuid::new_v4(), &ValidationReport::empty_ok());
    }

    #[test]
    fn test_emit_status_transition_does_not_panic() {
        emit_status_transition(
            Uuid::new_v4(),
            ChangeSetStatus::Draft,
            ChangeSetStatus::Validated,
        );
    }
}
