//! Bundle ingestion — parse `changeset.yaml` manifest + artifact directory layout.
//! See: docs/semantic_os_research_governed_boundary_v0.4.md §6.1
//!
//! A bundle is a directory or inline envelope containing:
//!   - `changeset.yaml` — the manifest (title, rationale, depends_on, supersedes, artifacts)
//!   - Artifact files referenced by the manifest (SQL, YAML, JSON)

use uuid::Uuid;

use sem_os_core::error::SemOsError;

use super::ports::Result;

use super::canonical_hash::compute_artifact_hash_typed;
use super::types::*;

/// Parsed bundle ready for `propose_change_set`.
#[derive(Debug, Clone)]
pub struct BundleContents {
    /// The parsed manifest from `changeset.yaml`.
    pub manifest: ChangeSetManifest,
    /// Resolved artifacts with content loaded.
    pub artifacts: Vec<ChangeSetArtifact>,
}

/// Raw manifest as it appears in `changeset.yaml` (serde-parseable).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawManifest {
    pub title: String,
    pub rationale: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<Uuid>,
    pub supersedes: Option<Uuid>,
    pub artifacts: Vec<RawArtifactEntry>,
}

/// Raw artifact entry in the manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawArtifactEntry {
    /// Artifact type discriminator.
    #[serde(rename = "type")]
    pub artifact_type: String,
    /// File path relative to bundle root.
    pub path: String,
    /// Optional declared content hash for integrity verification.
    pub content_hash: Option<String>,
    /// Optional ordinal for ordering (defaults to position in list).
    pub ordinal: Option<i32>,
    /// Optional metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Parse a `changeset.yaml` manifest string.
pub fn parse_manifest(yaml: &str) -> Result<RawManifest> {
    serde_yaml::from_str(yaml)
        .map_err(|e| SemOsError::InvalidInput(format!("Failed to parse changeset.yaml: {e}")))
}

/// Build a `BundleContents` from a raw manifest and a content resolver.
///
/// `resolve_content` maps `(artifact_type, path)` → file content.
/// This abstraction allows both filesystem and inline/base64 bundles.
pub fn build_bundle<F>(raw: &RawManifest, resolve_content: F) -> Result<BundleContents>
where
    F: Fn(&str, &str) -> std::result::Result<String, String>,
{
    let dummy_cs_id = Uuid::nil();
    let mut artifacts = Vec::with_capacity(raw.artifacts.len());
    let mut manifest_entries = Vec::with_capacity(raw.artifacts.len());

    for (idx, entry) in raw.artifacts.iter().enumerate() {
        let artifact_type = entry.artifact_type.parse::<ArtifactType>().map_err(|_| {
            SemOsError::InvalidInput(format!(
                "Unknown artifact type '{}' for path '{}'",
                entry.artifact_type, entry.path
            ))
        })?;

        let content = resolve_content(&entry.artifact_type, &entry.path).map_err(|e| {
            SemOsError::InvalidInput(format!("Failed to resolve artifact '{}': {e}", entry.path))
        })?;

        let content_hash = compute_artifact_hash_typed(&content, artifact_type);

        // Verify declared hash if present
        if let Some(ref declared) = entry.content_hash {
            if *declared != content_hash {
                return Err(SemOsError::InvalidInput(format!(
                    "Content hash mismatch for '{}': declared={}, computed={}",
                    entry.path, declared, content_hash
                )));
            }
        }

        let ordinal = entry.ordinal.unwrap_or(idx as i32);

        artifacts.push(ChangeSetArtifact {
            artifact_id: Uuid::new_v4(),
            change_set_id: dummy_cs_id,
            artifact_type,
            ordinal,
            path: Some(entry.path.clone()),
            content,
            content_hash: content_hash.clone(),
            metadata: entry.metadata.clone(),
        });

        manifest_entries.push(ArtifactManifestEntry {
            artifact_type,
            path: entry.path.clone(),
            content_hash: Some(content_hash),
        });
    }

    let manifest = ChangeSetManifest {
        title: raw.title.clone(),
        rationale: raw.rationale.clone(),
        depends_on: raw.depends_on.clone(),
        supersedes: raw.supersedes,
        artifacts: manifest_entries,
    };

    Ok(BundleContents {
        manifest,
        artifacts,
    })
}

/// Build a `BundleContents` from a manifest + inline content map.
///
/// `content_map` maps `path → content` for each artifact.
/// Useful for in-memory bundles or test fixtures.
pub fn build_bundle_from_map(
    raw: &RawManifest,
    content_map: &std::collections::HashMap<String, String>,
) -> Result<BundleContents> {
    build_bundle(raw, |_type_str, path| {
        content_map
            .get(path)
            .cloned()
            .ok_or_else(|| format!("Missing content for path '{path}'"))
    })
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_manifest_yaml() -> &'static str {
        r#"
title: "Add KYC attributes"
rationale: "New KYC entity type with required attributes"
depends_on: []
supersedes: null
artifacts:
  - type: migration_sql
    path: "001_kyc_tables.sql"
    ordinal: 0
  - type: verb_yaml
    path: "kyc.yaml"
    ordinal: 1
  - type: attribute_json
    path: "attrs.json"
    ordinal: 2
"#
    }

    #[test]
    fn test_parse_manifest() {
        let raw = parse_manifest(sample_manifest_yaml()).unwrap();
        assert_eq!(raw.title, "Add KYC attributes");
        assert_eq!(raw.artifacts.len(), 3);
        assert_eq!(raw.artifacts[0].artifact_type, "migration_sql");
        assert_eq!(raw.artifacts[0].path, "001_kyc_tables.sql");
    }

    #[test]
    fn test_parse_manifest_invalid_yaml() {
        let result = parse_manifest("{{invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_bundle_from_map() {
        let raw = parse_manifest(sample_manifest_yaml()).unwrap();
        let mut content_map = HashMap::new();
        content_map.insert("001_kyc_tables.sql".into(), "CREATE TABLE kyc();".into());
        content_map.insert("kyc.yaml".into(), "domain: kyc\n".into());
        content_map.insert("attrs.json".into(), r#"{"fqn": "kyc.name"}"#.into());

        let bundle = build_bundle_from_map(&raw, &content_map).unwrap();
        assert_eq!(bundle.manifest.title, "Add KYC attributes");
        assert_eq!(bundle.artifacts.len(), 3);
        assert_eq!(
            bundle.artifacts[0].artifact_type,
            ArtifactType::MigrationSql
        );
        assert_eq!(bundle.artifacts[0].content, "CREATE TABLE kyc();");
        assert!(!bundle.artifacts[0].content_hash.is_empty());
    }

    #[test]
    fn test_build_bundle_missing_content() {
        let raw = parse_manifest(sample_manifest_yaml()).unwrap();
        let content_map = HashMap::new(); // empty
        let result = build_bundle_from_map(&raw, &content_map);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_bundle_unknown_type() {
        let yaml = r#"
title: "Bad type"
artifacts:
  - type: unknown_type
    path: "foo.txt"
"#;
        let raw = parse_manifest(yaml).unwrap();
        let mut content_map = HashMap::new();
        content_map.insert("foo.txt".into(), "content".into());
        let result = build_bundle_from_map(&raw, &content_map);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown artifact type"));
    }

    #[test]
    fn test_build_bundle_hash_mismatch() {
        let yaml = r#"
title: "Hash check"
artifacts:
  - type: migration_sql
    path: "001.sql"
    content_hash: "wrong_hash"
"#;
        let raw = parse_manifest(yaml).unwrap();
        let mut content_map = HashMap::new();
        content_map.insert("001.sql".into(), "CREATE TABLE t();".into());
        let result = build_bundle_from_map(&raw, &content_map);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Content hash mismatch"));
    }

    #[test]
    fn test_bundle_contents_artifacts_have_content_hash() {
        let raw = parse_manifest(sample_manifest_yaml()).unwrap();
        let mut content_map = HashMap::new();
        content_map.insert("001_kyc_tables.sql".into(), "SELECT 1;".into());
        content_map.insert("kyc.yaml".into(), "verb: test\n".into());
        content_map.insert("attrs.json".into(), r#"{"a": 1}"#.into());

        let bundle = build_bundle_from_map(&raw, &content_map).unwrap();
        for artifact in &bundle.artifacts {
            assert_eq!(
                artifact.content_hash.len(),
                64,
                "SHA-256 hex should be 64 chars"
            );
        }
    }

    #[test]
    fn test_bundle_ordinals_default_to_position() {
        let yaml = r#"
title: "Ordinal test"
artifacts:
  - type: migration_sql
    path: "a.sql"
  - type: migration_sql
    path: "b.sql"
"#;
        let raw = parse_manifest(yaml).unwrap();
        let mut content_map = HashMap::new();
        content_map.insert("a.sql".into(), "SELECT 1;".into());
        content_map.insert("b.sql".into(), "SELECT 2;".into());

        let bundle = build_bundle_from_map(&raw, &content_map).unwrap();
        assert_eq!(bundle.artifacts[0].ordinal, 0);
        assert_eq!(bundle.artifacts[1].ordinal, 1);
    }
}
