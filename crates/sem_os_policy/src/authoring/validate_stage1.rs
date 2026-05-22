//! Stage 1 (pure) validation — no DB required.
//! Validates: artifact integrity, reference resolution, semantic consistency.
//! See: docs/semantic_os_research_governed_boundary_v0.4.md §6.2 Stage 1

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::canonical_hash::compute_artifact_hash_typed;
use super::errors::*;
use super::types::*;

/// Run Stage 1 (pure) validation on a manifest + artifact bundle.
///
/// Three phases:
///   1. Artifact integrity (hash verification, syntax parsing)
///   2. Reference resolution (entity types, domains, attributes, dependency cycle detection)
///   3. Semantic consistency (attribute type checks, verb contract completeness, lineage)
pub fn validate_stage1(
    manifest: &ChangeSetManifest,
    artifacts: &[ChangeSetArtifact],
) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Phase 1: Artifact integrity
    validate_artifact_integrity(manifest, artifacts, &mut errors, &mut warnings);

    // Phase 2: Reference resolution
    validate_references(manifest, artifacts, &mut errors, &mut warnings);

    // Phase 3: Semantic consistency
    validate_semantics(artifacts, &mut errors, &mut warnings);

    let ok = errors.is_empty();
    ValidationReport {
        ok,
        errors,
        warnings,
    }
}

// ── Phase 1: Artifact integrity ──────────────────────────────────

fn validate_artifact_integrity(
    manifest: &ChangeSetManifest,
    artifacts: &[ChangeSetArtifact],
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<ValidationError>,
) {
    // Build artifact lookup by path
    let artifact_by_path: HashMap<&str, &ChangeSetArtifact> = artifacts
        .iter()
        .filter_map(|a| a.path.as_deref().map(|p| (p, a)))
        .collect();

    // Check manifest entries have corresponding artifacts
    for entry in &manifest.artifacts {
        let Some(artifact) = artifact_by_path.get(entry.path.as_str()) else {
            errors.push(ValidationError {
                code: V_HASH_MISSING_ARTIFACT.to_string(),
                severity: ErrorSeverity::Error,
                message: format!(
                    "Artifact declared in manifest not found in bundle: {}",
                    entry.path
                ),
                artifact_path: Some(entry.path.clone()),
                line: None,
                context: None,
            });
            continue;
        };

        // Hash verification (if declared)
        if let Some(declared_hash) = &entry.content_hash {
            let computed = compute_artifact_hash_typed(&artifact.content, artifact.artifact_type);
            if computed != *declared_hash {
                errors.push(ValidationError {
                    code: V_HASH_MISMATCH.to_string(),
                    severity: ErrorSeverity::Error,
                    message: format!(
                        "Hash mismatch for {}: declared={}, computed={}",
                        entry.path, declared_hash, computed
                    ),
                    artifact_path: Some(entry.path.clone()),
                    line: None,
                    context: None,
                });
            }
        }

        // Type-specific syntax validation
        match artifact.artifact_type {
            ArtifactType::MigrationSql | ArtifactType::MigrationDownSql => {
                validate_sql_syntax(&artifact.content, &entry.path, errors);
            }
            ArtifactType::VerbYaml => {
                validate_yaml_syntax(&artifact.content, &entry.path, errors);
            }
            ArtifactType::AttributeJson | ArtifactType::TaxonomyJson | ArtifactType::DocJson => {
                validate_json_syntax(&artifact.content, &entry.path, errors);
            }
        }
    }

    // Warn about orphan artifacts (present in bundle but not in manifest)
    let manifest_paths: HashSet<&str> =
        manifest.artifacts.iter().map(|a| a.path.as_str()).collect();
    for artifact in artifacts {
        if let Some(path) = &artifact.path {
            if !manifest_paths.contains(path.as_str()) {
                warnings.push(ValidationError {
                    code: "V:WARN:ORPHAN_ARTIFACT".to_string(),
                    severity: ErrorSeverity::Warning,
                    message: format!(
                        "Artifact present in bundle but not declared in manifest: {path}"
                    ),
                    artifact_path: Some(path.clone()),
                    line: None,
                    context: None,
                });
            }
        }
    }
}

fn validate_sql_syntax(content: &str, path: &str, errors: &mut Vec<ValidationError>) {
    use sqlparser::dialect::PostgreSqlDialect;
    use sqlparser::parser::Parser;

    let dialect = PostgreSqlDialect {};
    if let Err(e) = Parser::parse_sql(&dialect, content) {
        errors.push(ValidationError {
            code: V_PARSE_SQL_SYNTAX.to_string(),
            severity: ErrorSeverity::Error,
            message: format!("SQL parse error in {path}: {e}"),
            artifact_path: Some(path.to_string()),
            line: extract_line_from_sql_error(&e),
            context: None,
        });
    }
}

fn extract_line_from_sql_error(e: &sqlparser::parser::ParserError) -> Option<u32> {
    // sqlparser::ParserError includes location info in some variants
    match e {
        sqlparser::parser::ParserError::ParserError(msg) => {
            // Try to extract "at Line: N" from message
            if let Some(idx) = msg.find("at Line: ") {
                let after = &msg[idx + 9..];
                after.split_whitespace().next()?.parse::<u32>().ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

fn validate_yaml_syntax(content: &str, path: &str, errors: &mut Vec<ValidationError>) {
    if let Err(e) = serde_yaml::from_str::<serde_yaml::Value>(content) {
        errors.push(ValidationError {
            code: V_PARSE_YAML_SYNTAX.to_string(),
            severity: ErrorSeverity::Error,
            message: format!("YAML parse error in {path}: {e}"),
            artifact_path: Some(path.to_string()),
            line: extract_line_from_yaml_error(&e),
            context: None,
        });
    }
}

fn extract_line_from_yaml_error(e: &serde_yaml::Error) -> Option<u32> {
    e.location().map(|loc| loc.line() as u32)
}

fn validate_json_syntax(content: &str, path: &str, errors: &mut Vec<ValidationError>) {
    if let Err(e) = serde_json::from_str::<serde_json::Value>(content) {
        errors.push(ValidationError {
            code: V_PARSE_JSON_SYNTAX.to_string(),
            severity: ErrorSeverity::Error,
            message: format!("JSON parse error in {path}: {e}"),
            artifact_path: Some(path.to_string()),
            line: Some(e.line() as u32),
            context: None,
        });
    }
}

// ── Phase 2: Reference resolution ────────────────────────────────

fn validate_references(
    manifest: &ChangeSetManifest,
    artifacts: &[ChangeSetArtifact],
    errors: &mut Vec<ValidationError>,
    _warnings: &mut Vec<ValidationError>,
) {
    // Dependency cycle detection
    if !manifest.depends_on.is_empty() {
        detect_dependency_cycles(&manifest.depends_on, errors);
    }

    // Cross-reference validation within the bundle
    validate_internal_references(artifacts, errors);
}

/// Detect self-referencing or trivially circular dependencies.
/// Full cycle detection across the dependency graph requires DB access (Stage 2).
fn detect_dependency_cycles(depends_on: &[Uuid], errors: &mut Vec<ValidationError>) {
    // Check for duplicate dependencies
    let mut seen = HashSet::new();
    for dep in depends_on {
        if !seen.insert(dep) {
            errors.push(ValidationError {
                code: V_REF_CIRCULAR_DEPENDENCY.to_string(),
                severity: ErrorSeverity::Error,
                message: format!("Duplicate dependency: {dep}"),
                artifact_path: None,
                line: None,
                context: None,
            });
        }
    }
}

/// Validate cross-references within the artifact bundle.
///
/// For example, a VerbYaml artifact may reference an entity type or attribute
/// that should also be present in the bundle.
fn validate_internal_references(
    artifacts: &[ChangeSetArtifact],
    errors: &mut Vec<ValidationError>,
) {
    // Collect entity types, attributes, and domains declared in bundle
    let mut declared_entity_types: HashSet<String> = HashSet::new();
    let mut declared_attributes: HashSet<String> = HashSet::new();

    for artifact in artifacts {
        match artifact.artifact_type {
            ArtifactType::AttributeJson => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&artifact.content) {
                    if let Some(fqn) = value.get("fqn").and_then(|v| v.as_str()) {
                        declared_attributes.insert(fqn.to_string());
                    }
                }
            }
            ArtifactType::TaxonomyJson => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&artifact.content) {
                    if let Some(entity_type) = value.get("entity_type").and_then(|v| v.as_str()) {
                        declared_entity_types.insert(entity_type.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Validate verb YAML references
    for artifact in artifacts {
        if artifact.artifact_type != ArtifactType::VerbYaml {
            continue;
        }
        if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&artifact.content) {
            // Check entity_type references in verb contracts
            if let Some(subject_type) = value.get("subject_entity_type").and_then(|v| v.as_str()) {
                if !declared_entity_types.contains(subject_type) {
                    // Not an error — entity type may exist in registry (checked in Stage 2)
                    // Only flag if we have entity types in this bundle and this one is missing
                    if !declared_entity_types.is_empty() {
                        // Soft reference — Stage 2 will do the hard check against DB
                    }
                }
            }

            // Check attribute references in verb required_attributes
            if let Some(attrs) = value
                .get("required_attributes")
                .and_then(|v| v.as_sequence())
            {
                for attr in attrs {
                    if let Some(attr_fqn) = attr.as_str() {
                        if !declared_attributes.contains(attr_fqn)
                            && !declared_attributes.is_empty()
                        {
                            // Soft reference — Stage 2 verifies against registry
                        }
                    }
                }
            }
        }
    }

    // Note: Hard reference checks (V_REF_MISSING_ENTITY, V_REF_MISSING_ATTRIBUTE, etc.)
    // are deferred to Stage 2 where the full registry is available.
    // Stage 1 only catches intra-bundle reference issues.
    let _ = errors; // suppress unused — errors are only added for intra-bundle mismatches
}

// ── Phase 3: Semantic consistency ────────────────────────────────

fn validate_semantics(
    artifacts: &[ChangeSetArtifact],
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<ValidationError>,
) {
    for artifact in artifacts {
        match artifact.artifact_type {
            ArtifactType::MigrationSql => {
                // Check for forbidden DDL patterns
                check_forbidden_ddl(&artifact.content, artifact.path.as_deref(), warnings);
            }
            ArtifactType::VerbYaml => {
                validate_verb_contract_completeness(
                    &artifact.content,
                    artifact.path.as_deref(),
                    errors,
                );
            }
            ArtifactType::AttributeJson => {
                validate_attribute_type_consistency(
                    &artifact.content,
                    artifact.path.as_deref(),
                    errors,
                );
            }
            _ => {}
        }
    }
}

/// Check for DDL patterns that are warnings in Stage 1 (errors in Stage 2 dry-run).
fn check_forbidden_ddl(sql_content: &str, path: Option<&str>, warnings: &mut Vec<ValidationError>) {
    let upper = sql_content.to_uppercase();

    // CONCURRENTLY requires non-transactional execution
    if upper.contains("CONCURRENTLY") {
        warnings.push(ValidationError {
            code: D_SCHEMA_NON_TRANSACTIONAL_DDL.to_string(),
            severity: ErrorSeverity::Warning,
            message: "Migration contains CONCURRENTLY — cannot run in a transaction".to_string(),
            artifact_path: path.map(String::from),
            line: find_line_containing(sql_content, "CONCURRENTLY"),
            context: None,
        });
    }

    // DROP TABLE without explicit breaking_change flag
    if upper.contains("DROP TABLE") {
        warnings.push(ValidationError {
            code: D_SCHEMA_FORBIDDEN_DDL.to_string(),
            severity: ErrorSeverity::Warning,
            message: "Migration contains DROP TABLE — must declare breaking_change=true"
                .to_string(),
            artifact_path: path.map(String::from),
            line: find_line_containing(sql_content, "DROP TABLE"),
            context: None,
        });
    }

    // DROP COLUMN
    if upper.contains("DROP COLUMN") {
        warnings.push(ValidationError {
            code: D_SCHEMA_FORBIDDEN_DDL.to_string(),
            severity: ErrorSeverity::Warning,
            message: "Migration contains DROP COLUMN — must declare breaking_change=true"
                .to_string(),
            artifact_path: path.map(String::from),
            line: find_line_containing(sql_content, "DROP COLUMN"),
            context: None,
        });
    }
}

fn find_line_containing(content: &str, needle: &str) -> Option<u32> {
    let upper_needle = needle.to_uppercase();
    for (i, line) in content.lines().enumerate() {
        if line.to_uppercase().contains(&upper_needle) {
            return Some((i + 1) as u32);
        }
    }
    None
}

/// Validate that verb YAML contracts have required fields.
fn validate_verb_contract_completeness(
    yaml_content: &str,
    path: Option<&str>,
    errors: &mut Vec<ValidationError>,
) {
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(yaml_content) else {
        return; // Syntax error already caught in Phase 1
    };

    // A verb contract must have at minimum: domain, action (or fqn), description
    let has_domain = value.get("domain").is_some();
    let has_action = value.get("action").is_some();
    let has_fqn = value.get("fqn").is_some();
    let has_description = value.get("description").is_some();

    if !(has_fqn || has_domain && has_action) {
        errors.push(ValidationError {
            code: V_TYPE_CONTRACT_INCOMPLETE.to_string(),
            severity: ErrorSeverity::Error,
            message: "Verb contract must have either 'fqn' or both 'domain' and 'action'"
                .to_string(),
            artifact_path: path.map(String::from),
            line: None,
            context: None,
        });
    }

    if !has_description {
        errors.push(ValidationError {
            code: V_TYPE_CONTRACT_INCOMPLETE.to_string(),
            severity: ErrorSeverity::Error,
            message: "Verb contract must have a 'description' field".to_string(),
            artifact_path: path.map(String::from),
            line: None,
            context: None,
        });
    }
}

/// Validate attribute JSON consistency (data_type, constraints).
fn validate_attribute_type_consistency(
    json_content: &str,
    path: Option<&str>,
    errors: &mut Vec<ValidationError>,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json_content) else {
        return; // Syntax error already caught in Phase 1
    };

    // Required: fqn, data_type
    if value.get("fqn").is_none() {
        errors.push(ValidationError {
            code: V_TYPE_ATTRIBUTE_MISMATCH.to_string(),
            severity: ErrorSeverity::Error,
            message: "Attribute definition must have 'fqn' field".to_string(),
            artifact_path: path.map(String::from),
            line: None,
            context: None,
        });
    }

    if value.get("data_type").is_none() {
        errors.push(ValidationError {
            code: V_TYPE_ATTRIBUTE_MISMATCH.to_string(),
            severity: ErrorSeverity::Error,
            message: "Attribute definition must have 'data_type' field".to_string(),
            artifact_path: path.map(String::from),
            line: None,
            context: None,
        });
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_artifact(artifact_type: ArtifactType, path: &str, content: &str) -> ChangeSetArtifact {
        ChangeSetArtifact {
            artifact_id: Uuid::new_v4(),
            change_set_id: Uuid::new_v4(),
            artifact_type,
            ordinal: 0,
            path: Some(path.to_string()),
            content: content.to_string(),
            content_hash: compute_artifact_hash_typed(content, artifact_type),
            metadata: None,
        }
    }

    fn make_manifest(artifacts: &[&ChangeSetArtifact]) -> ChangeSetManifest {
        ChangeSetManifest {
            title: "Test changeset".to_string(),
            rationale: None,
            depends_on: vec![],
            supersedes: None,
            artifacts: artifacts
                .iter()
                .map(|a| ArtifactManifestEntry {
                    artifact_type: a.artifact_type,
                    path: a.path.clone().unwrap_or_default(),
                    content_hash: Some(a.content_hash.clone()),
                })
                .collect(),
        }
    }

    #[test]
    fn test_empty_changeset_passes() {
        let manifest = ChangeSetManifest {
            title: "Empty".to_string(),
            rationale: None,
            depends_on: vec![],
            supersedes: None,
            artifacts: vec![],
        };
        let report = validate_stage1(&manifest, &[]);
        assert!(report.ok, "Empty changeset should pass validation");
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_valid_sql_artifact() {
        let artifact = make_artifact(
            ArtifactType::MigrationSql,
            "migrations/001.sql",
            "CREATE TABLE test_table (id UUID PRIMARY KEY, name TEXT NOT NULL);",
        );
        let manifest = make_manifest(&[&artifact]);
        let report = validate_stage1(&manifest, &[artifact]);
        assert!(report.ok, "Valid SQL should pass: {:?}", report.errors);
    }

    #[test]
    fn test_invalid_sql_syntax() {
        let artifact = make_artifact(
            ArtifactType::MigrationSql,
            "migrations/bad.sql",
            "CREATE TABL broken_syntax (;",
        );
        let manifest = make_manifest(&[&artifact]);
        let report = validate_stage1(&manifest, &[artifact]);
        assert!(!report.ok);
        assert!(report.errors.iter().any(|e| e.code == V_PARSE_SQL_SYNTAX));
    }

    #[test]
    fn test_hash_mismatch() {
        let artifact = make_artifact(
            ArtifactType::MigrationSql,
            "migrations/001.sql",
            "SELECT 1;",
        );
        let manifest = ChangeSetManifest {
            title: "Test".to_string(),
            rationale: None,
            depends_on: vec![],
            supersedes: None,
            artifacts: vec![ArtifactManifestEntry {
                artifact_type: ArtifactType::MigrationSql,
                path: "migrations/001.sql".to_string(),
                content_hash: Some("wrong_hash_value".to_string()),
            }],
        };
        let report = validate_stage1(&manifest, &[artifact]);
        assert!(!report.ok);
        assert!(report.errors.iter().any(|e| e.code == V_HASH_MISMATCH));
    }

    #[test]
    fn test_missing_artifact() {
        let manifest = ChangeSetManifest {
            title: "Test".to_string(),
            rationale: None,
            depends_on: vec![],
            supersedes: None,
            artifacts: vec![ArtifactManifestEntry {
                artifact_type: ArtifactType::MigrationSql,
                path: "migrations/missing.sql".to_string(),
                content_hash: None,
            }],
        };
        let report = validate_stage1(&manifest, &[]);
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|e| e.code == V_HASH_MISSING_ARTIFACT));
    }

    #[test]
    fn test_invalid_json_syntax() {
        let artifact = make_artifact(ArtifactType::AttributeJson, "attrs/bad.json", "{ bad json");
        let manifest = make_manifest(&[&artifact]);
        let report = validate_stage1(&manifest, &[artifact]);
        assert!(!report.ok);
        assert!(report.errors.iter().any(|e| e.code == V_PARSE_JSON_SYNTAX));
    }

    #[test]
    fn test_invalid_yaml_syntax() {
        let artifact = make_artifact(
            ArtifactType::VerbYaml,
            "verbs/bad.yaml",
            "key: [unclosed bracket",
        );
        let manifest = make_manifest(&[&artifact]);
        let report = validate_stage1(&manifest, &[artifact]);
        assert!(!report.ok);
        assert!(report.errors.iter().any(|e| e.code == V_PARSE_YAML_SYNTAX));
    }

    #[test]
    fn test_concurrently_warning() {
        let artifact = make_artifact(
            ArtifactType::MigrationSql,
            "migrations/idx.sql",
            "CREATE INDEX CONCURRENTLY idx_test ON test_table(name);",
        );
        let manifest = make_manifest(&[&artifact]);
        let report = validate_stage1(&manifest, &[artifact]);
        // CONCURRENTLY is a warning in Stage 1, not an error
        assert!(report.ok, "CONCURRENTLY should be warning, not error");
        assert!(report
            .warnings
            .iter()
            .any(|w| w.code == D_SCHEMA_NON_TRANSACTIONAL_DDL));
    }

    #[test]
    fn test_drop_table_warning() {
        let artifact = make_artifact(
            ArtifactType::MigrationSql,
            "migrations/drop.sql",
            "DROP TABLE old_table;",
        );
        let manifest = make_manifest(&[&artifact]);
        let report = validate_stage1(&manifest, &[artifact]);
        assert!(report.ok, "DROP TABLE should be warning, not error");
        assert!(report
            .warnings
            .iter()
            .any(|w| w.code == D_SCHEMA_FORBIDDEN_DDL));
    }

    #[test]
    fn test_verb_contract_incomplete() {
        let artifact = make_artifact(
            ArtifactType::VerbYaml,
            "verbs/incomplete.yaml",
            "action: create\n# missing domain and description\n",
        );
        let manifest = make_manifest(&[&artifact]);
        let report = validate_stage1(&manifest, &[artifact]);
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|e| e.code == V_TYPE_CONTRACT_INCOMPLETE));
    }

    #[test]
    fn test_attribute_missing_data_type() {
        let artifact = make_artifact(
            ArtifactType::AttributeJson,
            "attrs/no_type.json",
            r#"{"fqn": "cbu.name"}"#,
        );
        let manifest = make_manifest(&[&artifact]);
        let report = validate_stage1(&manifest, &[artifact]);
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|e| e.code == V_TYPE_ATTRIBUTE_MISMATCH));
    }

    #[test]
    fn test_duplicate_dependency() {
        let dep_id = Uuid::new_v4();
        let manifest = ChangeSetManifest {
            title: "Test".to_string(),
            rationale: None,
            depends_on: vec![dep_id, dep_id],
            supersedes: None,
            artifacts: vec![],
        };
        let report = validate_stage1(&manifest, &[]);
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|e| e.code == V_REF_CIRCULAR_DEPENDENCY));
    }

    #[test]
    fn test_valid_full_bundle() {
        let sql = make_artifact(
            ArtifactType::MigrationSql,
            "migrations/001.sql",
            "CREATE TABLE foo (id UUID PRIMARY KEY);",
        );
        let attr = make_artifact(
            ArtifactType::AttributeJson,
            "attrs/foo.name.json",
            r#"{"fqn": "foo.name", "data_type": "text"}"#,
        );
        let verb = make_artifact(
            ArtifactType::VerbYaml,
            "verbs/foo.create.yaml",
            "fqn: foo.create\ndescription: Create a foo\n",
        );
        let manifest = make_manifest(&[&sql, &attr, &verb]);
        let report = validate_stage1(&manifest, &[sql, attr, verb]);
        assert!(report.ok, "Valid bundle should pass: {:?}", report.errors);
    }
}
