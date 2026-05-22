//! Retention and cleanup logic for the authoring pipeline.
//!
//! Archives terminal ChangeSets based on age:
//! - `Rejected` / `DryRunFailed`: archived after 90 days
//! - Orphan `Draft` / `Validated` (no activity): archived after 30 days
//!
//! Archival moves rows to `sem_reg_authoring.change_sets_archive` and
//! deletes from the main table.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Configuration for the cleanup policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPolicy {
    /// Days after which Rejected/DryRunFailed ChangeSets are archived.
    pub terminal_retention_days: u32,
    /// Days after which orphan Draft/Validated ChangeSets are archived.
    pub orphan_retention_days: u32,
}

impl Default for CleanupPolicy {
    fn default() -> Self {
        Self {
            terminal_retention_days: 90,
            orphan_retention_days: 30,
        }
    }
}

/// Result of a cleanup run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    /// Number of terminal ChangeSets archived.
    pub terminal_archived: u32,
    /// Number of orphan ChangeSets archived.
    pub orphan_archived: u32,
    /// Timestamp when cleanup was performed.
    pub cleaned_at: DateTime<Utc>,
}

/// Port trait for cleanup storage operations.
///
/// Implemented by sem_os_postgres.
#[async_trait]
pub trait CleanupStore: Send + Sync {
    /// Archive terminal ChangeSets (Rejected/DryRunFailed) older than the cutoff.
    /// Returns the count of archived rows.
    async fn archive_terminal_changesets(&self, cutoff: DateTime<Utc>)
        -> super::ports::Result<u32>;

    /// Archive orphan ChangeSets (Draft/Validated with no updates) older than the cutoff.
    /// Returns the count of archived rows.
    async fn archive_orphan_changesets(&self, cutoff: DateTime<Utc>) -> super::ports::Result<u32>;
}

/// Run the cleanup process according to the given policy.
pub async fn run_cleanup(
    store: &dyn CleanupStore,
    policy: &CleanupPolicy,
) -> super::ports::Result<CleanupReport> {
    let now = Utc::now();

    let terminal_cutoff = now - chrono::Duration::days(i64::from(policy.terminal_retention_days));
    let orphan_cutoff = now - chrono::Duration::days(i64::from(policy.orphan_retention_days));

    let terminal_archived = store.archive_terminal_changesets(terminal_cutoff).await?;
    let orphan_archived = store.archive_orphan_changesets(orphan_cutoff).await?;

    tracing::info!(
        target: "authoring.cleanup",
        terminal_archived,
        orphan_archived,
        terminal_cutoff = %terminal_cutoff,
        orphan_cutoff = %orphan_cutoff,
        "cleanup completed"
    );

    Ok(CleanupReport {
        terminal_archived,
        orphan_archived,
        cleaned_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleanup_policy_defaults() {
        let policy = CleanupPolicy::default();
        assert_eq!(policy.terminal_retention_days, 90);
        assert_eq!(policy.orphan_retention_days, 30);
    }

    #[test]
    fn test_cleanup_report_serde() {
        let report = CleanupReport {
            terminal_archived: 5,
            orphan_archived: 3,
            cleaned_at: Utc::now(),
        };
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["terminal_archived"], 5);
        assert_eq!(json["orphan_archived"], 3);
    }
}
