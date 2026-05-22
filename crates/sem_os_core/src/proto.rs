//! API request/response types for the Semantic OS service boundary.
//!
//! These are the "proto" types that both `SemOsClient` and `CoreService` use.
//! Today they are plain Rust structs; in Stage 2.x they will be generated
//! from `.proto` files via prost-build for the gRPC boundary.

use serde::{Deserialize, Serialize};

use crate::types::ObjectType;

// Note (sem_os_core-split v1 Phase 9): the historical re-exports of
// `crate::context_resolution::ContextResolution{Request,Response}`
// were removed when context_resolution moved to sem_os_policy. They
// were never referenced under their proto aliases (`ResolveContextRequest`
// / `ResolveContextResponse`) anywhere in the workspace.

// ── Manifest ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetManifestResponse {
    pub snapshot_set_id: String,
    pub published_at: String,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub snapshot_id: String,
    pub object_type: ObjectType,
    pub fqn: String,
    pub content_hash: String,
}

// ── Export ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSnapshotSetResponse {
    pub snapshots: Vec<ExportedSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedSnapshot {
    pub snapshot_id: String,
    pub fqn: String,
    pub object_type: ObjectType,
    pub payload: serde_json::Value,
}

// ── Bootstrap ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapSeedBundleResponse {
    pub created: u32,
    pub skipped: u32,
    pub bundle_hash: String,
    /// The snapshot_set_id created for this bootstrap (None if all seeds were skipped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_set_id: Option<String>,
}

// ── Tool Dispatch ─────────────────────────────────────────────

/// Request to invoke a named MCP tool with JSON arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

/// Result of an MCP tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    pub success: bool,
    pub data: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Description of a single tool parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParamSpec {
    pub name: String,
    pub param_type: String,
    pub description: String,
    pub required: bool,
}

/// Specification for a single MCP tool (name, description, parameters).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ToolParamSpec>,
}

/// Response containing the list of available tool specifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolSpecsResponse {
    pub tools: Vec<ToolSpec>,
}

// ── Changeset / Workbench ─────────────────────────────────────

/// Response for listing changesets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListChangesetsResponse {
    pub changesets: Vec<crate::types::Changeset>,
}

/// Query parameters for listing changesets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListChangesetsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// A single diff entry showing what changed vs the current active snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    pub entry_id: String,
    pub object_fqn: String,
    pub object_type: ObjectType,
    pub change_kind: String,
    /// The base snapshot this entry was drafted against (None for new additions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_snapshot_id: Option<String>,
    /// The current active snapshot for this FQN (None if not yet published).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_snapshot_id: Option<String>,
    /// True if base_snapshot_id differs from current_snapshot_id (stale draft).
    pub is_stale: bool,
    /// The draft payload that would be published.
    pub draft_payload: serde_json::Value,
    /// The current active payload for comparison (None for additions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_payload: Option<serde_json::Value>,
}

/// Response for changeset diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetDiffResponse {
    pub changeset_id: String,
    pub status: String,
    pub entries: Vec<DiffEntry>,
    pub stale_count: u32,
}

/// A downstream dependent affected by a changeset entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactEntry {
    pub source_fqn: String,
    pub source_object_type: ObjectType,
    pub dependent_fqn: String,
    pub dependent_object_type: ObjectType,
    pub relationship: String,
}

/// Response for changeset impact analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetImpactResponse {
    pub changeset_id: String,
    pub impacts: Vec<ImpactEntry>,
}

/// Response for gate preview on a changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatePreviewResponse {
    pub changeset_id: String,
    pub would_block: bool,
    pub error_count: u32,
    pub warning_count: u32,
    pub gate_results: Vec<GatePreviewEntry>,
}

/// A single gate result in the preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatePreviewEntry {
    pub entry_id: String,
    pub object_fqn: String,
    pub gate_name: String,
    pub severity: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Response for changeset publish (promotion).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetPublishResponse {
    pub changeset_id: String,
    pub snapshots_created: u32,
    pub snapshot_set_id: String,
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_request_serde() {
        let req = ToolCallRequest {
            tool_name: "sem_reg_describe_attribute".into(),
            arguments: serde_json::json!({"fqn": "cbu.jurisdiction_code"}),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["tool_name"], "sem_reg_describe_attribute");
        let back: ToolCallRequest = serde_json::from_value(json).unwrap();
        assert_eq!(back.tool_name, req.tool_name);
    }

    #[test]
    fn test_tool_call_response_serde_success() {
        let resp = ToolCallResponse {
            success: true,
            data: serde_json::json!({"result": 42}),
            error: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["success"], true);
        // error should be skipped (skip_serializing_if = "Option::is_none")
        assert!(json.get("error").is_none());
    }

    #[test]
    fn test_tool_call_response_serde_failure() {
        let resp = ToolCallResponse {
            success: false,
            data: serde_json::json!(null),
            error: Some("not found".into()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "not found");
        let back: ToolCallResponse = serde_json::from_value(json).unwrap();
        assert!(!back.success);
        assert_eq!(back.error.unwrap(), "not found");
    }

    #[test]
    fn test_bootstrap_response_skip_none_snapshot_set() {
        let resp = BootstrapSeedBundleResponse {
            created: 5,
            skipped: 3,
            bundle_hash: "v1:abc123".into(),
            snapshot_set_id: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["created"], 5);
        assert_eq!(json["skipped"], 3);
        // snapshot_set_id should be skipped when None
        assert!(json.get("snapshot_set_id").is_none());
    }

    #[test]
    fn test_bootstrap_response_includes_snapshot_set() {
        let resp = BootstrapSeedBundleResponse {
            created: 10,
            skipped: 0,
            bundle_hash: "v1:def456".into(),
            snapshot_set_id: Some("set-001".into()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["snapshot_set_id"], "set-001");
    }

    #[test]
    fn test_list_changesets_query_defaults() {
        let query: ListChangesetsQuery = serde_json::from_str("{}").unwrap();
        assert!(query.status.is_none());
        assert!(query.owner.is_none());
        assert!(query.scope.is_none());
    }

    #[test]
    fn test_diff_entry_optional_fields() {
        let entry = DiffEntry {
            entry_id: "e1".into(),
            object_fqn: "cbu.name".into(),
            object_type: ObjectType::AttributeDef,
            change_kind: "add".into(),
            base_snapshot_id: None,
            current_snapshot_id: None,
            is_stale: false,
            draft_payload: serde_json::json!({"value": "test"}),
            current_payload: None,
        };
        let json = serde_json::to_value(&entry).unwrap();
        // Optional None fields should be skipped
        assert!(json.get("base_snapshot_id").is_none());
        assert!(json.get("current_snapshot_id").is_none());
        assert!(json.get("current_payload").is_none());
    }

    #[test]
    fn test_gate_preview_response_serde() {
        let resp = GatePreviewResponse {
            changeset_id: "cs-1".into(),
            would_block: true,
            error_count: 2,
            warning_count: 1,
            gate_results: vec![GatePreviewEntry {
                entry_id: "e1".into(),
                object_fqn: "cbu.name".into(),
                gate_name: "proof_rule".into(),
                severity: "error".into(),
                passed: false,
                reason: Some("proof chain broken".into()),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["would_block"], true);
        assert_eq!(json["gate_results"][0]["gate_name"], "proof_rule");
        let back: GatePreviewResponse = serde_json::from_value(json).unwrap();
        assert_eq!(back.gate_results.len(), 1);
        assert!(!back.gate_results[0].passed);
    }

    #[test]
    fn test_manifest_entry_serde() {
        let resp = GetManifestResponse {
            snapshot_set_id: "set-1".into(),
            published_at: "2026-01-01T00:00:00Z".into(),
            entries: vec![ManifestEntry {
                snapshot_id: "snap-1".into(),
                object_type: ObjectType::VerbContract,
                fqn: "cbu.create".into(),
                content_hash: "abc".into(),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["entries"][0]["fqn"], "cbu.create");
        let back: GetManifestResponse = serde_json::from_value(json).unwrap();
        assert_eq!(back.entries.len(), 1);
    }

    #[test]
    fn test_tool_spec_serde() {
        let spec = ToolSpec {
            name: "my_tool".into(),
            description: "Does something".into(),
            parameters: vec![ToolParamSpec {
                name: "fqn".into(),
                param_type: "string".into(),
                description: "Fully qualified name".into(),
                required: true,
            }],
        };
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["parameters"][0]["required"], true);
        let back: ToolSpec = serde_json::from_value(json).unwrap();
        assert_eq!(back.parameters.len(), 1);
    }
}
