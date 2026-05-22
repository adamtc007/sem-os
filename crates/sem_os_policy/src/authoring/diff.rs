//! Diff tooling for ChangeSets.
//! Structural diff between two artifact sets, or against the active snapshot set.
//! See: docs/semantic_os_research_governed_boundary_v0.4.md §6.2

use std::collections::HashMap;

use super::types::*;

/// Compute a structural diff between two sets of artifacts.
pub fn diff_changesets(base: &[ChangeSetArtifact], target: &[ChangeSetArtifact]) -> DiffSummary {
    let base_map = build_artifact_index(base);
    let target_map = build_artifact_index(target);

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut removed = Vec::new();
    let mut breaking_changes = Vec::new();

    // Items in target but not in base → added
    // Items in both but different hash → modified
    for (key, target_artifact) in &target_map {
        match base_map.get(key) {
            None => {
                added.push(DiffEntry {
                    fqn: key.clone(),
                    object_type: target_artifact.artifact_type.to_string(),
                    detail: Some("New artifact".to_string()),
                });
            }
            Some(base_artifact) => {
                if base_artifact.content_hash != target_artifact.content_hash {
                    let is_breaking = detect_breaking_change(base_artifact, target_artifact);
                    let entry = DiffEntry {
                        fqn: key.clone(),
                        object_type: target_artifact.artifact_type.to_string(),
                        detail: Some(format!(
                            "Hash changed: {} → {}",
                            &base_artifact.content_hash[..8.min(base_artifact.content_hash.len())],
                            &target_artifact.content_hash
                                [..8.min(target_artifact.content_hash.len())]
                        )),
                    };
                    if is_breaking {
                        breaking_changes.push(entry);
                    } else {
                        modified.push(entry);
                    }
                }
            }
        }
    }

    // Items in base but not in target → removed (always breaking)
    for (key, base_artifact) in &base_map {
        if !target_map.contains_key(key) {
            let entry = DiffEntry {
                fqn: key.clone(),
                object_type: base_artifact.artifact_type.to_string(),
                detail: Some("Removed".to_string()),
            };
            removed.push(entry.clone());
            breaking_changes.push(entry);
        }
    }

    DiffSummary {
        added,
        modified,
        removed,
        breaking_changes,
    }
}

/// Build an index of artifacts by their logical key (type + path or ordinal).
fn build_artifact_index(artifacts: &[ChangeSetArtifact]) -> HashMap<String, &ChangeSetArtifact> {
    let mut index = HashMap::new();
    for artifact in artifacts {
        let key = artifact_key(artifact);
        index.insert(key, artifact);
    }
    index
}

/// Compute a stable logical key for an artifact.
fn artifact_key(artifact: &ChangeSetArtifact) -> String {
    match &artifact.path {
        Some(path) => format!("{}:{}", artifact.artifact_type, path),
        None => format!("{}:#{}", artifact.artifact_type, artifact.ordinal),
    }
}

/// Heuristic detection of breaking changes.
///
/// Currently detects:
/// - SQL migrations that contain DROP TABLE / DROP COLUMN
/// - Removed artifacts (always breaking)
/// - Attribute type changes
fn detect_breaking_change(_base: &ChangeSetArtifact, target: &ChangeSetArtifact) -> bool {
    match target.artifact_type {
        ArtifactType::MigrationSql => {
            let upper = target.content.to_uppercase();
            upper.contains("DROP TABLE")
                || upper.contains("DROP COLUMN")
                || upper.contains("ALTER COLUMN")
                || upper.contains("RENAME TABLE")
        }
        ArtifactType::AttributeJson => {
            // Check if data_type changed between base and target
            let base_type = extract_json_field(&_base.content, "data_type");
            let target_type = extract_json_field(&target.content, "data_type");
            base_type.is_some() && target_type.is_some() && base_type != target_type
        }
        _ => false,
    }
}

fn extract_json_field(json_content: &str, field: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(json_content)
        .ok()?
        .get(field)?
        .as_str()
        .map(String::from)
}

/// Compute a summary of a single ChangeSet's artifacts for planning purposes.
pub fn summarize_changeset(artifacts: &[ChangeSetArtifact]) -> DiffSummary {
    let added: Vec<DiffEntry> = artifacts
        .iter()
        .map(|a| DiffEntry {
            fqn: artifact_key(a),
            object_type: a.artifact_type.to_string(),
            detail: a.path.clone(),
        })
        .collect();

    let breaking_changes: Vec<DiffEntry> = artifacts
        .iter()
        .filter(|a| {
            a.artifact_type == ArtifactType::MigrationSql && {
                let upper = a.content.to_uppercase();
                upper.contains("DROP TABLE") || upper.contains("DROP COLUMN")
            }
        })
        .map(|a| DiffEntry {
            fqn: artifact_key(a),
            object_type: a.artifact_type.to_string(),
            detail: Some("Contains potentially breaking DDL".to_string()),
        })
        .collect();

    DiffSummary {
        added,
        modified: vec![],
        removed: vec![],
        breaking_changes,
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn make_artifact(artifact_type: ArtifactType, path: &str, content: &str) -> ChangeSetArtifact {
        ChangeSetArtifact {
            artifact_id: Uuid::new_v4(),
            change_set_id: Uuid::new_v4(),
            artifact_type,
            ordinal: 0,
            path: Some(path.to_string()),
            content: content.to_string(),
            content_hash: format!("hash_{}", content.len()),
            metadata: None,
        }
    }

    #[test]
    fn test_diff_identical() {
        let a = make_artifact(ArtifactType::MigrationSql, "001.sql", "SELECT 1;");
        let b = make_artifact(ArtifactType::MigrationSql, "001.sql", "SELECT 1;");
        // Same hash
        let summary = diff_changesets(&[a], &[b]);
        assert!(summary.added.is_empty());
        assert!(summary.modified.is_empty());
        assert!(summary.removed.is_empty());
        assert!(summary.breaking_changes.is_empty());
    }

    #[test]
    fn test_diff_added() {
        let b = make_artifact(ArtifactType::MigrationSql, "001.sql", "SELECT 1;");
        let summary = diff_changesets(&[], &[b]);
        assert_eq!(summary.added.len(), 1);
        assert!(summary.removed.is_empty());
    }

    #[test]
    fn test_diff_removed() {
        let a = make_artifact(ArtifactType::MigrationSql, "001.sql", "SELECT 1;");
        let summary = diff_changesets(&[a], &[]);
        assert_eq!(summary.removed.len(), 1);
        assert_eq!(
            summary.breaking_changes.len(),
            1,
            "Removal is always breaking"
        );
    }

    #[test]
    fn test_diff_modified_non_breaking() {
        let a = make_artifact(ArtifactType::VerbYaml, "verb.yaml", "desc: old");
        let mut b = make_artifact(ArtifactType::VerbYaml, "verb.yaml", "desc: new");
        b.content_hash = "different_hash".to_string();
        let summary = diff_changesets(&[a], &[b]);
        assert_eq!(summary.modified.len(), 1);
        assert!(summary.breaking_changes.is_empty());
    }

    #[test]
    fn test_diff_breaking_ddl() {
        let a = make_artifact(
            ArtifactType::MigrationSql,
            "001.sql",
            "CREATE TABLE t (id INT);",
        );
        let mut b = make_artifact(ArtifactType::MigrationSql, "001.sql", "DROP TABLE t;");
        b.content_hash = "changed".to_string();
        let summary = diff_changesets(&[a], &[b]);
        assert_eq!(summary.breaking_changes.len(), 1);
    }

    #[test]
    fn test_summarize_changeset() {
        let artifacts = vec![
            make_artifact(
                ArtifactType::MigrationSql,
                "001.sql",
                "CREATE TABLE t (id INT);",
            ),
            make_artifact(ArtifactType::VerbYaml, "verb.yaml", "fqn: test.create"),
            make_artifact(ArtifactType::MigrationSql, "002.sql", "DROP TABLE old;"),
        ];
        let summary = summarize_changeset(&artifacts);
        assert_eq!(summary.added.len(), 3);
        assert_eq!(summary.breaking_changes.len(), 1, "DROP TABLE is breaking");
    }

    #[test]
    fn test_attribute_type_change_is_breaking() {
        let a = make_artifact(
            ArtifactType::AttributeJson,
            "attr.json",
            r#"{"fqn":"test.attr","data_type":"text"}"#,
        );
        let mut b = make_artifact(
            ArtifactType::AttributeJson,
            "attr.json",
            r#"{"fqn":"test.attr","data_type":"integer"}"#,
        );
        b.content_hash = "changed".to_string();
        let summary = diff_changesets(&[a], &[b]);
        assert_eq!(summary.breaking_changes.len(), 1, "Type change is breaking");
    }
}
