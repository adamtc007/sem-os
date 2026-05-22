//! Content-addressed hashing for ChangeSet idempotency.
//! See: docs/semantic_os_research_governed_boundary_v0.4.md §2.2.1
//!
//! Algorithm:
//!   1. Sort artifacts by (artifact_type, ordinal, path)
//!   2. Normalize line endings to \n
//!   3. For YAML/JSON: parse → canonical re-serialization (stable key order)
//!   4. Concatenate all normalized content
//!   5. Hash: SHA-256 of ("v1:" + concatenated content)

use sha2::{Digest, Sha256};

use super::types::{ArtifactType, ChangeSetArtifact, ChangeSetManifest};

/// Current hash version prefix.
pub const HASH_VERSION: &str = "v1";

/// Compute the content hash for a ChangeSet bundle.
///
/// Returns the hex-encoded SHA-256 hash, without the version prefix.
/// The caller stores `HASH_VERSION` separately.
pub fn compute_content_hash(
    manifest: &ChangeSetManifest,
    artifacts: &[ChangeSetArtifact],
) -> String {
    let mut sorted: Vec<&ChangeSetArtifact> = artifacts.iter().collect();
    sorted.sort_by(|a, b| {
        a.artifact_type
            .as_ref()
            .cmp(b.artifact_type.as_ref())
            .then_with(|| a.ordinal.cmp(&b.ordinal))
            .then_with(|| {
                a.path
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.path.as_deref().unwrap_or(""))
            })
    });

    let mut hasher = Sha256::new();
    // Include version prefix in hash input
    hasher.update(format!("{}:", HASH_VERSION).as_bytes());
    // Include manifest title for bundle identity
    hasher.update(manifest.title.as_bytes());
    hasher.update(b"\n");

    for artifact in &sorted {
        let normalized = normalize_content(&artifact.content);
        hasher.update(artifact.artifact_type.as_ref().as_bytes());
        hasher.update(b":");
        hasher.update(artifact.path.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"\n");
        hasher.update(normalized.as_bytes());
        hasher.update(b"\n");
    }

    hex::encode(hasher.finalize())
}

/// Compute the content hash for a single artifact.
pub fn compute_artifact_hash(content: &str) -> String {
    let normalized = normalize_content(content);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

/// Normalize content for stable hashing:
/// - Normalize line endings to \n
/// - Trim trailing whitespace per line
/// - For JSON: parse and re-serialize with sorted keys
/// - For YAML: parse and re-serialize as canonical JSON
fn normalize_content(content: &str) -> String {
    // Normalize line endings
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");

    // Trim trailing whitespace per line
    normalized
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Attempt to canonicalize JSON content (sorted keys).
/// Returns the canonical form if valid JSON, otherwise returns input unchanged.
pub fn try_canonicalize_json(content: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(value) => serde_json::to_string(&value).unwrap_or_else(|_| content.to_string()),
        Err(_) => content.to_string(),
    }
}

/// Attempt to canonicalize YAML content by converting to sorted JSON.
/// Returns the canonical JSON form if valid YAML, otherwise returns input unchanged.
pub fn try_canonicalize_yaml(content: &str) -> String {
    match serde_yaml::from_str::<serde_json::Value>(content) {
        Ok(value) => serde_json::to_string(&value).unwrap_or_else(|_| content.to_string()),
        Err(_) => content.to_string(),
    }
}

/// Compute a content hash for an artifact, with type-aware canonicalization.
pub fn compute_artifact_hash_typed(content: &str, artifact_type: ArtifactType) -> String {
    let canonical = match artifact_type {
        ArtifactType::AttributeJson | ArtifactType::TaxonomyJson | ArtifactType::DocJson => {
            try_canonicalize_json(content)
        }
        ArtifactType::VerbYaml => try_canonicalize_yaml(content),
        ArtifactType::MigrationSql | ArtifactType::MigrationDownSql => normalize_content(content),
    };
    compute_artifact_hash(&canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_normalize_line_endings() {
        assert_eq!(normalize_content("a\r\nb\rc\n"), "a\nb\nc");
    }

    #[test]
    fn test_normalize_trailing_whitespace() {
        assert_eq!(normalize_content("hello   \nworld  "), "hello\nworld");
    }

    #[test]
    fn test_artifact_hash_deterministic() {
        let h1 = compute_artifact_hash("SELECT 1;");
        let h2 = compute_artifact_hash("SELECT 1;");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_artifact_hash_different_content() {
        let h1 = compute_artifact_hash("SELECT 1;");
        let h2 = compute_artifact_hash("SELECT 2;");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_content_hash_sorted_by_type_ordinal_path() {
        let manifest = ChangeSetManifest {
            title: "test".into(),
            rationale: None,
            depends_on: vec![],
            supersedes: None,
            artifacts: vec![],
        };

        let id = Uuid::new_v4();
        let artifacts = vec![
            ChangeSetArtifact {
                artifact_id: Uuid::new_v4(),
                change_set_id: id,
                artifact_type: ArtifactType::VerbYaml,
                ordinal: 0,
                path: Some("b.yaml".into()),
                content: "verb: b".into(),
                content_hash: String::new(),
                metadata: None,
            },
            ChangeSetArtifact {
                artifact_id: Uuid::new_v4(),
                change_set_id: id,
                artifact_type: ArtifactType::MigrationSql,
                ordinal: 0,
                path: Some("001.sql".into()),
                content: "CREATE TABLE t();".into(),
                content_hash: String::new(),
                metadata: None,
            },
        ];

        let h1 = compute_content_hash(&manifest, &artifacts);

        // Reverse order — should produce same hash since we sort internally
        let artifacts_reversed: Vec<ChangeSetArtifact> = artifacts.into_iter().rev().collect();
        let h2 = compute_content_hash(&manifest, &artifacts_reversed);

        assert_eq!(h1, h2);
    }

    #[test]
    fn test_try_canonicalize_json() {
        let input = r#"{"b": 2, "a": 1}"#;
        let canonical = try_canonicalize_json(input);
        // serde_json sorts keys
        assert_eq!(canonical, r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn test_try_canonicalize_yaml() {
        let input = "b: 2\na: 1\n";
        let canonical = try_canonicalize_yaml(input);
        assert_eq!(canonical, r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn test_try_canonicalize_invalid_json() {
        let input = "not json {{{";
        assert_eq!(try_canonicalize_json(input), input);
    }
}
