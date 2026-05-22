//! Stewardship Agent — Type Definitions
//!
//! All types match the stewardship-agent-architecture spec §8, §9.1–9.7, §9.14.
//! Phase 0 (Changeset Layer) + Phase 1 (Show Loop) types in one file.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════
//  Phase 0: Changeset Layer Types
// ═══════════════════════════════════════════════════════════════════

// ─── Changeset Action (§9.1) ───────────────────────────────────

/// Action type for changeset entries — spec §9.1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangesetAction {
    Add,
    Modify,
    Promote,
    Deprecate,
    Alias,
}

impl ChangesetAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Modify => "modify",
            Self::Promote => "promote",
            Self::Deprecate => "deprecate",
            Self::Alias => "alias",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "add" => Some(Self::Add),
            "modify" => Some(Self::Modify),
            "promote" => Some(Self::Promote),
            "deprecate" => Some(Self::Deprecate),
            "alias" => Some(Self::Alias),
            _ => None,
        }
    }
}

// ─── Changeset Status (§9.1) ──────────────────────────────────

// Re-export the canonical 9-state ChangeSetStatus from authoring as ChangesetStatus.
// The authoring pipeline's enum is the superset covering all lifecycle states.
pub use crate::authoring::types::ChangeSetStatus as ChangesetStatus;

// ─── Changeset Row (from DB) ──────────────────────────────────

/// A changeset row as returned from `sem_reg.changesets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetRow {
    pub changeset_id: Uuid,
    pub status: ChangesetStatus,
    pub owner_actor_id: String,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A changeset entry row from `sem_reg.changeset_entries` (with stewardship columns).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetEntryRow {
    pub entry_id: Uuid,
    pub changeset_id: Uuid,
    pub object_fqn: String,
    pub object_type: String,
    pub change_kind: String,
    pub draft_payload: serde_json::Value,
    pub base_snapshot_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    // Stewardship columns (migration 097)
    pub action: ChangesetAction,
    pub predecessor_id: Option<Uuid>,
    pub revision: i32,
    pub reasoning: Option<String>,
    pub guardrail_log: serde_json::Value,
}

// ─── Basis (§9.3) ─────────────────────────────────────────────

/// Kind of basis record — spec §9.3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasisKind {
    RegulatoryFact,
    MarketPractice,
    PlatformConvention,
    ClientRequirement,
    Precedent,
}

impl BasisKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RegulatoryFact => "regulatory_fact",
            Self::MarketPractice => "market_practice",
            Self::PlatformConvention => "platform_convention",
            Self::ClientRequirement => "client_requirement",
            Self::Precedent => "precedent",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "regulatory_fact" => Some(Self::RegulatoryFact),
            "market_practice" => Some(Self::MarketPractice),
            "platform_convention" => Some(Self::PlatformConvention),
            "client_requirement" => Some(Self::ClientRequirement),
            "precedent" => Some(Self::Precedent),
            _ => None,
        }
    }
}

/// A basis record linking evidence/rationale to a changeset — spec §9.3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasisRecord {
    pub basis_id: Uuid,
    pub changeset_id: Uuid,
    pub entry_id: Option<Uuid>,
    pub kind: BasisKind,
    pub title: String,
    pub narrative: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

/// A claim within a basis record — spec §9.3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasisClaim {
    pub claim_id: Uuid,
    pub basis_id: Uuid,
    pub claim_text: String,
    pub reference_uri: Option<String>,
    pub excerpt: Option<String>,
    pub confidence: Option<f64>,
    pub flagged_as_open_question: bool,
}

// ─── Conflict Model (§9.6) ────────────────────────────────────

/// Conflict resolution strategy — spec §9.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    Merge,
    Rebase,
    Supersede,
}

impl ConflictStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::Supersede => "supersede",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "merge" => Some(Self::Merge),
            "rebase" => Some(Self::Rebase),
            "supersede" => Some(Self::Supersede),
            _ => None,
        }
    }
}

/// A conflict record between competing changesets — spec §9.6.
/// Uses `competing_changeset_id` (not left/right snapshot IDs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub conflict_id: Uuid,
    pub changeset_id: Uuid,
    pub competing_changeset_id: Uuid,
    pub fqn: String,
    pub detected_at: DateTime<Utc>,
    pub resolution_strategy: Option<ConflictStrategy>,
    pub resolution_rationale: Option<String>,
    pub resolved_by: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
}

// ─── Stewardship Events (§9.4) ────────────────────────────────

/// Event types in the stewardship audit chain — spec §9.4.
/// Tagged enum with inner data for rich audit records.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StewardshipEventType {
    ChangesetCreated,
    ItemAdded,
    ItemRemoved,
    ItemRefined,
    BasisAttached,
    GuardrailFired {
        guardrail_id: GuardrailId,
        severity: GuardrailSeverity,
        resolution: String,
    },
    GatePrechecked {
        result: serde_json::Value,
    },
    SubmittedForReview,
    ReviewNoteAdded,
    ReviewDecisionRecorded {
        disposition: ReviewDisposition,
    },
    FocusChanged {
        from: serde_json::Value,
        to: serde_json::Value,
        source: FocusUpdateSource,
    },
    Published,
    Rejected,
}

impl StewardshipEventType {
    /// DB event_type string for the stewardship.events CHECK constraint.
    pub fn db_event_type(&self) -> &'static str {
        match self {
            Self::ChangesetCreated => "changeset_created",
            Self::ItemAdded => "item_added",
            Self::ItemRemoved => "item_removed",
            Self::ItemRefined => "item_refined",
            Self::BasisAttached => "basis_attached",
            Self::GuardrailFired { .. } => "guardrail_fired",
            Self::GatePrechecked { .. } => "gate_prechecked",
            Self::SubmittedForReview => "submitted_for_review",
            Self::ReviewNoteAdded => "review_note_added",
            Self::ReviewDecisionRecorded { .. } => "review_decision_recorded",
            Self::FocusChanged { .. } => "focus_changed",
            Self::Published => "published",
            Self::Rejected => "rejected",
        }
    }
}

/// A stewardship event record (append-only audit chain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StewardshipRecord {
    pub event_id: Uuid,
    pub changeset_id: Uuid,
    pub event_type: StewardshipEventType,
    pub actor_id: String,
    pub payload: serde_json::Value,
    pub viewport_manifest_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Review disposition for review decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDisposition {
    Approve,
    RequestChange,
    Reject,
}

/// Source of a focus state update (agent-driven vs user navigation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusUpdateSource {
    Agent,
    UserNavigation,
}

impl FocusUpdateSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::UserNavigation => "user_navigation",
        }
    }
}

// ─── Template (§9.5) ──────────────────────────────────────────

/// A stewardship template — versioned, domain-scoped — spec §9.5.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StewardshipTemplate {
    pub template_id: Uuid,
    pub fqn: String,
    pub display_name: String,
    pub version: SemanticVersion,
    pub domain: String,
    pub scope: Vec<String>,
    pub items: Vec<TemplateItem>,
    pub steward: String,
    pub basis_ref: Option<Uuid>,
    pub status: TemplateStatus,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

/// Semantic version (major.minor.patch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl std::fmt::Display for SemanticVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// An item within a template — pre-populates changeset entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateItem {
    pub object_type: String,
    pub fqn_pattern: String,
    pub action: ChangesetAction,
    pub default_payload: Option<serde_json::Value>,
}

/// Template lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateStatus {
    Draft,
    Active,
    Deprecated,
}

impl TemplateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Deprecated => "deprecated",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "active" => Some(Self::Active),
            "deprecated" => Some(Self::Deprecated),
            _ => None,
        }
    }
}

// ─── VerbImplementationBinding (§9.7) ─────────────────────────

/// Binding kind for verb implementation — spec §9.7.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    RustHandler,
    BpmnProcess,
    RemoteHttp,
    MacroExpansion,
}

impl BindingKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RustHandler => "rust_handler",
            Self::BpmnProcess => "bpmn_process",
            Self::RemoteHttp => "remote_http",
            Self::MacroExpansion => "macro_expansion",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rust_handler" => Some(Self::RustHandler),
            "bpmn_process" => Some(Self::BpmnProcess),
            "remote_http" => Some(Self::RemoteHttp),
            "macro_expansion" => Some(Self::MacroExpansion),
            _ => None,
        }
    }
}

/// Binding lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingStatus {
    Draft,
    Active,
    Deprecated,
}

impl BindingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Deprecated => "deprecated",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "active" => Some(Self::Active),
            "deprecated" => Some(Self::Deprecated),
            _ => None,
        }
    }
}

/// A verb implementation binding — spec §9.7.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbImplementationBinding {
    pub binding_id: Uuid,
    pub verb_fqn: String,
    pub binding_kind: BindingKind,
    pub binding_ref: String,
    pub exec_modes: Vec<String>,
    pub status: BindingStatus,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
}

// ─── Guardrails (§8.2) ───────────────────────────────────────

/// Guardrail identifiers — VERBATIM from spec §8.2.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GuardrailId {
    G01RolePermission,
    G02NamingConvention,
    G03TypeConstraint,
    G04ProofChainCompatibility,
    G05ClassificationRequired,
    G06SecurityLabelRequired,
    G07SilentMeaningChange,
    G08DeprecationWithoutReplacement,
    G09AIKnowledgeBoundary,
    G10ConflictDetected,
    G11StaleTemplate,
    G12ObservationImpact,
    G13ResolutionMetadataMissing,
    G14CompositionHintStale,
    G15DraftUniquenessViolation,
}

impl GuardrailId {
    /// Default severity map for guardrails — matches spec §8.2 exactly.
    pub fn default_severity(&self) -> GuardrailSeverity {
        match self {
            // Block — edit cannot be saved
            Self::G01RolePermission => GuardrailSeverity::Block,
            Self::G03TypeConstraint => GuardrailSeverity::Block,
            Self::G04ProofChainCompatibility => GuardrailSeverity::Block,
            Self::G05ClassificationRequired => GuardrailSeverity::Block,
            Self::G06SecurityLabelRequired => GuardrailSeverity::Block,
            Self::G07SilentMeaningChange => GuardrailSeverity::Block,
            Self::G08DeprecationWithoutReplacement => GuardrailSeverity::Block,
            Self::G15DraftUniquenessViolation => GuardrailSeverity::Block,
            // Warning — must be acknowledged before submit
            Self::G02NamingConvention => GuardrailSeverity::Warning,
            Self::G10ConflictDetected => GuardrailSeverity::Warning,
            Self::G11StaleTemplate => GuardrailSeverity::Warning,
            Self::G12ObservationImpact => GuardrailSeverity::Warning,
            Self::G13ResolutionMetadataMissing => GuardrailSeverity::Warning,
            // Advisory — informational only
            Self::G09AIKnowledgeBoundary => GuardrailSeverity::Advisory,
            Self::G14CompositionHintStale => GuardrailSeverity::Advisory,
        }
    }
}

/// Guardrail severity — determines enforcement behaviour.
/// Block: edit cannot be saved. Warning: must be acknowledged before submit.
/// Advisory: informational only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardrailSeverity {
    Block,
    Warning,
    Advisory,
}

/// Result of evaluating a single guardrail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailResult {
    pub guardrail_id: GuardrailId,
    pub severity: GuardrailSeverity,
    pub message: String,
    pub remediation: String,
    pub context: serde_json::Value,
}

// ═══════════════════════════════════════════════════════════════════
//  Phase 1: Show Loop Types
// ═══════════════════════════════════════════════════════════════════

// ─── FocusState (§9.14.1) ─────────────────────────────────────

/// Server-side focus state — shared truth between agent and UI.
/// Spec §9.14.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusState {
    pub session_id: Uuid,
    pub changeset_id: Option<Uuid>,
    pub overlay_mode: OverlayMode,
    pub object_refs: Vec<ObjectRef>,
    pub taxonomy_focus: Option<TaxonomyFocus>,
    pub resolution_context: Option<serde_json::Value>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: FocusUpdateSource,
}

/// Reference to a registry object within focus state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectRef {
    pub object_type: String,
    pub object_id: Uuid,
    pub fqn: String,
    pub snapshot_id: Option<Uuid>,
}

/// Optional taxonomy focus within the focus state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyFocus {
    pub taxonomy_fqn: String,
    pub node_id: Option<String>,
}

/// Overlay mode — determines whether drafts are visible.
/// `ActiveOnly`: standard view (only Active snapshots).
/// `DraftOverlay`: preview with Active ∪ Changeset.Drafts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum OverlayMode {
    ActiveOnly,
    DraftOverlay { changeset_id: Uuid },
}

// ─── ShowPacket (§9.14.3) ─────────────────────────────────────

/// ShowPacket — the primary output of the Show Loop engine.
/// Contains focus state, viewports, optional deltas, and next actions.
/// Spec §9.14.3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowPacket {
    pub focus: FocusState,
    pub viewports: Vec<ViewportSpec>,
    pub deltas: Option<Vec<ViewportDelta>>,
    pub narrative: Option<String>,
    pub next_actions: Vec<SuggestedAction>,
    /// Observatory orientation contract — projected from existing resolution state.
    /// Present when the Observatory UI is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<crate::observatory::orientation::OrientationContract>,
}

// ─── SuggestedAction (§9.14.4) ────────────────────────────────

/// Suggested next action — closes the Refine step of the loop.
/// Spec §9.14.4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedAction {
    pub action_type: ActionType,
    pub label: String,
    pub target: ActionTarget,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub keyboard_hint: Option<String>,
}

/// Action types for suggested actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    AcceptItem,
    EditItem,
    RunGates,
    SubmitForReview,
    RecordReview,
    Publish,
    ResolveConflict,
    AddEvidence,
    ToggleOverlay,
    NavigateToItem,
    Remediate,
}

/// Target of a suggested action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionTarget {
    pub changeset_id: Option<Uuid>,
    pub item_id: Option<Uuid>,
    pub viewport_id: Option<String>,
    pub guardrail_id: Option<String>,
}

// ─── Viewport Types (§9.14.5, §9.14.6) ───────────────────────

/// The 8 viewport kinds from spec §9.14.6.
/// Phase 1 implements: Focus, Object, Diff, Gates.
/// Phase 2 adds: Taxonomy, Impact, ActionSurface, Coverage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportKind {
    Focus,
    Taxonomy,
    Object,
    Diff,
    Impact,
    ActionSurface,
    Gates,
    Coverage,
}

/// Render hint for viewport UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderHint {
    Tree,
    Graph,
    Table,
    Diff,
    Cards,
}

/// ViewportSpec is the request: "compute this viewport".
/// Spec §9.14.6.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportSpec {
    pub id: String,
    pub kind: ViewportKind,
    pub title: String,
    pub params: serde_json::Value,
    pub render_hint: RenderHint,
}

/// ViewportStatus — lifecycle state per viewport.
/// Spec §9.14.5.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ViewportStatus {
    Ready,
    Loading { progress: Option<f32> },
    Error { message: String },
    Stale,
}

/// ViewportModel is the response: "here is the computed viewport data".
/// Spec §9.14.6.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportModel {
    pub id: String,
    pub kind: ViewportKind,
    pub status: ViewportStatus,
    pub data: serde_json::Value,
    pub meta: ViewportMeta,
}

/// Metadata for a viewport model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportMeta {
    pub updated_at: DateTime<Utc>,
    pub sources: Vec<String>,
    pub overlay_mode: OverlayMode,
}

/// ViewportDelta — incremental viewport update.
/// Spec §9.14.7.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportDelta {
    pub viewport_id: String,
    pub op: PatchOp,
    pub path: String,
    pub value: Option<serde_json::Value>,
}

/// JSON Patch operation type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchOp {
    Add,
    Remove,
    Replace,
    Move,
}

// ─── WorkbenchPacket Transport (§9.16) ────────────────────────

/// WorkbenchPacket — transport envelope for Show Loop data.
/// Spec §9.16.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchPacket {
    pub packet_id: Uuid,
    pub session_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub frame_type: String,
    pub kind: WorkbenchPacketKind,
    pub payload: WorkbenchPayload,
}

/// WorkbenchPacket kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkbenchPacketKind {
    Show,
    DeltaUpdate,
    StatusUpdate,
}

/// WorkbenchPacket payload — tagged union for each packet kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkbenchPayload {
    ShowPayload {
        show_packet: Box<ShowPacket>,
    },
    DeltaPayload {
        deltas: Vec<ViewportDelta>,
    },
    StatusPayload {
        viewport_id: String,
        status: ViewportStatus,
    },
}

// ─── ViewportManifest (§9.4) ──────────────────────────────────

/// ViewportManifest — immutable audit record linking viewport state to events.
/// Spec §9.4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportManifest {
    pub manifest_id: Uuid,
    pub session_id: Uuid,
    pub changeset_id: Option<Uuid>,
    pub captured_at: DateTime<Utc>,
    pub focus_state: FocusState,
    pub rendered_viewports: Vec<ViewportRef>,
    pub overlay_mode: OverlayMode,
    pub assumed_principal: Option<String>,
}

/// Reference to a rendered viewport within a manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportRef {
    pub viewport_id: String,
    pub kind: ViewportKind,
    pub data_hash: String,
    pub registry_version: Option<Uuid>,
    pub tool_call_ref: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_changeset_action_roundtrip() {
        let action = ChangesetAction::Promote;
        assert_eq!(ChangesetAction::parse(action.as_str()), Some(action));
    }

    #[test]
    fn test_changeset_status_roundtrip() {
        let status = ChangesetStatus::UnderReview;
        let parsed: ChangesetStatus = status.as_ref().parse().unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn test_guardrail_severity_block() {
        let result = GuardrailResult {
            guardrail_id: GuardrailId::G01RolePermission,
            severity: GuardrailSeverity::Block,
            message: "No permission".into(),
            remediation: "Request access".into(),
            context: serde_json::json!({}),
        };
        assert_eq!(result.severity, GuardrailSeverity::Block);
    }

    #[test]
    fn test_stewardship_event_type_db_string() {
        let evt = StewardshipEventType::GuardrailFired {
            guardrail_id: GuardrailId::G10ConflictDetected,
            severity: GuardrailSeverity::Warning,
            resolution: "acknowledged".into(),
        };
        assert_eq!(evt.db_event_type(), "guardrail_fired");
    }

    #[test]
    fn test_overlay_mode_serde() {
        let mode = OverlayMode::DraftOverlay {
            changeset_id: Uuid::nil(),
        };
        let json = serde_json::to_value(&mode).unwrap();
        assert_eq!(json["mode"], "draft_overlay");
        let back: OverlayMode = serde_json::from_value(json).unwrap();
        assert_eq!(back, mode);
    }

    #[test]
    fn test_overlay_mode_active_only() {
        let mode = OverlayMode::ActiveOnly;
        let json = serde_json::to_value(&mode).unwrap();
        assert_eq!(json["mode"], "active_only");
    }

    #[test]
    fn test_viewport_status_serde() {
        let status = ViewportStatus::Loading {
            progress: Some(0.5),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["state"], "loading");
        assert_eq!(json["progress"], 0.5);
    }

    #[test]
    fn test_workbench_payload_serde() {
        let payload = WorkbenchPayload::StatusPayload {
            viewport_id: "gates-1".into(),
            status: ViewportStatus::Ready,
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "status_payload");
    }

    #[test]
    fn test_semantic_version_display() {
        let v = SemanticVersion {
            major: 2,
            minor: 1,
            patch: 3,
        };
        assert_eq!(v.to_string(), "2.1.3");
    }

    #[test]
    fn test_basis_kind_roundtrip() {
        let kind = BasisKind::PlatformConvention;
        assert_eq!(BasisKind::parse(kind.as_str()), Some(kind));
    }

    #[test]
    fn test_conflict_strategy_roundtrip() {
        let s = ConflictStrategy::Rebase;
        assert_eq!(ConflictStrategy::parse(s.as_str()), Some(s));
    }

    #[test]
    fn test_template_status_roundtrip() {
        let s = TemplateStatus::Active;
        assert_eq!(TemplateStatus::parse(s.as_str()), Some(s));
    }

    #[test]
    fn test_binding_kind_roundtrip() {
        let k = BindingKind::BpmnProcess;
        assert_eq!(BindingKind::parse(k.as_str()), Some(k));
    }
}
