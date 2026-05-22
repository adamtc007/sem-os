//! Structured error code constants for the authoring pipeline.
//! See: docs/semantic_os_research_governed_boundary_v0.4.md §6.2
//!
//! Format: `{STAGE}:{CATEGORY}:{CODE}`
//!   V:*       — Stage 1 (validate_change_set) — pure, no DB
//!   D:*       — Stage 2 (dry_run_change_set) — needs DB
//!   PUBLISH:* — Publish-time errors

// ── Stage 1: Validation (V:*) ──────────────────────────────────

/// Artifact content hash does not match declared hash.
pub const V_HASH_MISMATCH: &str = "V:HASH:MISMATCH";
/// An artifact declared in the manifest is missing from the bundle.
pub const V_HASH_MISSING_ARTIFACT: &str = "V:HASH:MISSING_ARTIFACT";

/// SQL migration cannot be parsed.
pub const V_PARSE_SQL_SYNTAX: &str = "V:PARSE:SQL_SYNTAX";
/// YAML artifact has invalid syntax.
pub const V_PARSE_YAML_SYNTAX: &str = "V:PARSE:YAML_SYNTAX";
/// YAML artifact does not conform to expected schema.
pub const V_PARSE_YAML_SCHEMA: &str = "V:PARSE:YAML_SCHEMA";
/// JSON artifact has invalid syntax.
pub const V_PARSE_JSON_SYNTAX: &str = "V:PARSE:JSON_SYNTAX";
/// JSON artifact does not conform to expected schema.
pub const V_PARSE_JSON_SCHEMA: &str = "V:PARSE:JSON_SCHEMA";

/// Referenced entity type not found in registry or bundle.
pub const V_REF_MISSING_ENTITY: &str = "V:REF:MISSING_ENTITY";
/// Referenced domain not found.
pub const V_REF_MISSING_DOMAIN: &str = "V:REF:MISSING_DOMAIN";
/// Referenced attribute not found.
pub const V_REF_MISSING_ATTRIBUTE: &str = "V:REF:MISSING_ATTRIBUTE";
/// Declared dependency ChangeSet not found.
pub const V_REF_MISSING_DEPENDENCY: &str = "V:REF:MISSING_DEPENDENCY";
/// Circular dependency detected among ChangeSet dependencies.
pub const V_REF_CIRCULAR_DEPENDENCY: &str = "V:REF:CIRCULAR_DEPENDENCY";

/// Attribute data type mismatch between contract and definition.
pub const V_TYPE_ATTRIBUTE_MISMATCH: &str = "V:TYPE:ATTRIBUTE_MISMATCH";
/// Verb contract is missing required fields.
pub const V_TYPE_CONTRACT_INCOMPLETE: &str = "V:TYPE:CONTRACT_INCOMPLETE";
/// Derivation lineage chain is broken.
pub const V_TYPE_LINEAGE_BROKEN: &str = "V:TYPE:LINEAGE_BROKEN";

// ── Stage 2: Dry-Run (D:*) ────────────────────────────────────

/// Migration SQL failed to apply in scratch schema.
pub const D_SCHEMA_APPLY_FAILED: &str = "D:SCHEMA:APPLY_FAILED";
/// Migration contains non-transactional DDL (e.g., CREATE INDEX CONCURRENTLY).
pub const D_SCHEMA_NON_TRANSACTIONAL_DDL: &str = "D:SCHEMA:NON_TRANSACTIONAL_DDL";
/// Migration contains forbidden DDL (e.g., DROP TABLE without breaking_change=true).
pub const D_SCHEMA_FORBIDDEN_DDL: &str = "D:SCHEMA:FORBIDDEN_DDL";
/// No corresponding down migration for a forward migration.
pub const D_SCHEMA_DOWN_MISSING: &str = "D:SCHEMA:DOWN_MISSING";
/// Down migration failed during scratch cleanup.
pub const D_SCHEMA_DOWN_FAILED: &str = "D:SCHEMA:DOWN_FAILED";

/// Breaking change not declared as such in the manifest.
pub const D_COMPAT_BREAKING_UNDECLARED: &str = "D:COMPAT:BREAKING_UNDECLARED";
/// Attribute definition conflicts with existing active snapshot.
pub const D_COMPAT_ATTR_CONFLICT: &str = "D:COMPAT:ATTR_CONFLICT";
/// Verb contract conflicts with existing active snapshot.
pub const D_COMPAT_VERB_CONFLICT: &str = "D:COMPAT:VERB_CONFLICT";
/// A dependency ChangeSet has not been published yet.
pub const D_COMPAT_DEPENDENCY_UNPUBLISHED: &str = "D:COMPAT:DEPENDENCY_UNPUBLISHED";
/// A dependency ChangeSet is in a failed state.
pub const D_COMPAT_DEPENDENCY_FAILED: &str = "D:COMPAT:DEPENDENCY_FAILED";
/// Supersession target is already superseded or not in a terminal state.
pub const D_COMPAT_SUPERSESSION_CONFLICT: &str = "D:COMPAT:SUPERSESSION_CONFLICT";

/// Governed ChangeSet requires approval before publish.
pub const D_POLICY_APPROVAL_REQUIRED: &str = "D:POLICY:APPROVAL_REQUIRED";
/// Actor does not have sufficient role for this operation.
pub const D_POLICY_ROLE_INSUFFICIENT: &str = "D:POLICY:ROLE_INSUFFICIENT";

// ── Publish-time (PUBLISH:*) ───────────────────────────────────

/// Active snapshot set changed since dry-run was evaluated.
pub const PUBLISH_DRIFT_DETECTED: &str = "PUBLISH:DRIFT_DETECTED";
/// Could not acquire advisory lock for single-publisher gate.
pub const PUBLISH_LOCK_CONTENTION: &str = "PUBLISH:LOCK_CONTENTION";
/// ChangeSet is not in the correct status for publish.
pub const PUBLISH_STATUS_INVALID: &str = "PUBLISH:STATUS_INVALID";
/// Batch publish dependency graph contains a cycle.
pub const PUBLISH_BATCH_CYCLE_DETECTED: &str = "PUBLISH:BATCH_CYCLE_DETECTED";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_format() {
        // All codes follow the {STAGE}:{CATEGORY}:{CODE} convention
        let all_codes = [
            V_HASH_MISMATCH,
            V_HASH_MISSING_ARTIFACT,
            V_PARSE_SQL_SYNTAX,
            V_PARSE_YAML_SYNTAX,
            V_PARSE_YAML_SCHEMA,
            V_PARSE_JSON_SYNTAX,
            V_PARSE_JSON_SCHEMA,
            V_REF_MISSING_ENTITY,
            V_REF_MISSING_DOMAIN,
            V_REF_MISSING_ATTRIBUTE,
            V_REF_MISSING_DEPENDENCY,
            V_REF_CIRCULAR_DEPENDENCY,
            V_TYPE_ATTRIBUTE_MISMATCH,
            V_TYPE_CONTRACT_INCOMPLETE,
            V_TYPE_LINEAGE_BROKEN,
            D_SCHEMA_APPLY_FAILED,
            D_SCHEMA_NON_TRANSACTIONAL_DDL,
            D_SCHEMA_FORBIDDEN_DDL,
            D_SCHEMA_DOWN_MISSING,
            D_SCHEMA_DOWN_FAILED,
            D_COMPAT_BREAKING_UNDECLARED,
            D_COMPAT_ATTR_CONFLICT,
            D_COMPAT_VERB_CONFLICT,
            D_COMPAT_DEPENDENCY_UNPUBLISHED,
            D_COMPAT_DEPENDENCY_FAILED,
            D_COMPAT_SUPERSESSION_CONFLICT,
            D_POLICY_APPROVAL_REQUIRED,
            D_POLICY_ROLE_INSUFFICIENT,
            PUBLISH_DRIFT_DETECTED,
            PUBLISH_LOCK_CONTENTION,
            PUBLISH_STATUS_INVALID,
            PUBLISH_BATCH_CYCLE_DETECTED,
        ];

        assert_eq!(
            all_codes.len(),
            32,
            "authoring pipeline defines 32 error codes"
        );

        for code in &all_codes {
            let parts: Vec<&str> = code.split(':').collect();
            assert!(
                parts.len() >= 2,
                "code {code} must have at least 2 colon-separated parts"
            );
            assert!(
                parts[0] == "V" || parts[0] == "D" || parts[0] == "PUBLISH",
                "code {code} must start with V, D, or PUBLISH"
            );
        }
    }
}
