//! Context resolution — pure scoring/ranking/filtering logic.
//!
//! This module contains all the **pure** types and functions from the 12-step
//! context resolution pipeline. The async DB-loading portion (resolve_subject,
//! load_subject_memberships, load_view_defs, load_typed_snapshots, etc.) lives
//! in `sem_os_postgres`.
//!
//! ## 12-Step Resolution Pipeline (pure steps marked ✓)
//!
//! 1. Determine snapshot epoch (trivial)
//! 2. Resolve subject → entity type + jurisdiction + state (DB)
//!    2b. Load taxonomy memberships (DB) + evaluate conditional memberships ✓
//!    2c. Load subject relationships (DB)
//! 3. Select applicable ViewDefs by taxonomy overlap ✓
//! 4. Extract verb_surface + attribute_prominence from top view ✓
//! 5. Filter verbs by taxonomy + ABAC + tier ✓
//! 6. Filter attributes similarly ✓
//! 7. Rank by ViewDef prominence weights ✓
//! 8. Evaluate preconditions ✓
//! 9. Evaluate policies ✓
//! 10. Compute composite access decision ✓
//! 11. Generate governance signals ✓
//! 12. Compute confidence score ✓

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::abac::{evaluate_abac, AccessDecision, AccessPurpose, ActorContext};
use sem_os_ontology::constellation_family_def::GroundingThreshold;
#[cfg(test)]
use sem_os_ontology::membership::MembershipKind;
use sem_os_ontology::membership::{MembershipCondition, MembershipRuleBody};
use sem_os_ontology::policy_rule::PolicyRuleBody;
use sem_os_ontology::relationship_type_def::RelationshipTypeDefBody;
use sem_os_ontology::universe_def::{EntryQuestion, GroundingInput};
use sem_os_ontology::view_def::ViewDefBody;
use sem_os_types::{GovernanceTier, SnapshotRow, TrustClass};

// ── Evidence Mode ─────────────────────────────────────────────

/// Controls how trust-class and governance-tier filtering is applied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceMode {
    /// Only governed + Proof/DecisionSupport. Operational excluded unless view allows.
    Strict,
    /// Governed primary, operational allowed if view includes it. Tagged `usable_for_proof = false`.
    #[default]
    Normal,
    /// All tiers and trust classes, annotated with tier/trust metadata.
    Exploratory,
    /// Coverage metrics focus — stewardship gaps, classification gaps, stale evidence.
    Governance,
}

// ── Subject Reference ─────────────────────────────────────────

/// What the resolution is about — the subject being analysed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "id")]
#[serde(rename_all = "snake_case")]
pub enum SubjectRef {
    CaseId(Uuid),
    EntityId(Uuid),
    DocumentId(Uuid),
    TaskId(Uuid),
    ViewId(Uuid),
}

impl SubjectRef {
    pub fn id(&self) -> Uuid {
        match self {
            Self::CaseId(id)
            | Self::EntityId(id)
            | Self::DocumentId(id)
            | Self::TaskId(id)
            | Self::ViewId(id) => *id,
        }
    }
}

// ── Resolution Constraints ────────────────────────────────────

/// Optional constraints that narrow the resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolutionConstraints {
    /// Jurisdiction filter (e.g. "LU", "DE").
    #[serde(default)]
    pub jurisdiction: Option<String>,
    /// Risk posture filter (e.g. "high", "standard").
    #[serde(default)]
    pub risk_posture: Option<String>,
    /// Arbitrary key-value thresholds for custom filtering.
    #[serde(default)]
    pub thresholds: std::collections::HashMap<String, serde_json::Value>,
}

// ── Request ───────────────────────────────────────────────────

/// The single input to `resolve_context()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextResolutionRequest {
    /// What we are resolving context for.
    pub subject: SubjectRef,
    /// Optional Sage-produced intent summary.
    #[serde(default)]
    pub intent_summary: Option<String>,
    /// Raw user utterance for non-lossy Sem OS interpretation.
    #[serde(default)]
    pub raw_utterance: Option<String>,
    /// Who is asking — drives ABAC evaluation.
    pub actor: ActorContext,
    /// What the actor is trying to achieve.
    #[serde(default)]
    pub goals: Vec<String>,
    /// Optional narrowing constraints.
    #[serde(default)]
    pub constraints: ResolutionConstraints,
    /// Trust-aware filtering mode.
    #[serde(default)]
    pub evidence_mode: EvidenceMode,
    /// Point-in-time for historical resolution (None = now).
    #[serde(default)]
    pub point_in_time: Option<DateTime<Utc>>,
    /// Entity kind of the dominant entity (for subject_kinds filtering).
    /// When set, verbs with non-empty subject_kinds that don't include this
    /// kind are filtered out.
    #[serde(default)]
    pub entity_kind: Option<String>,
    /// Confidence in the dominant entity-kind signal.
    /// Low-confidence signals widen the candidate verb set instead of filtering.
    #[serde(default)]
    pub entity_confidence: Option<f64>,
    /// Explicit discovery navigation hints captured during Sage bootstrap.
    #[serde(default)]
    pub discovery: DiscoveryContext,
}

/// Explicit discovery-stage hints threaded into Sem OS resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryContext {
    /// Selected work-area/domain identifier.
    #[serde(default)]
    pub selected_domain_id: Option<String>,
    /// Selected constellation family identifier.
    #[serde(default)]
    pub selected_family_id: Option<String>,
    /// Selected concrete constellation identifier.
    #[serde(default)]
    pub selected_constellation_id: Option<String>,
    /// Structured answers collected during bootstrap.
    #[serde(default)]
    pub known_inputs: std::collections::HashMap<String, String>,
}

// ── Response ──────────────────────────────────────────────────

/// The full output of context resolution — everything a consumer needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextResolutionResponse {
    /// The point-in-time that was resolved (either requested or now).
    pub as_of_time: DateTime<Utc>,
    /// When this resolution was computed.
    pub resolved_at: DateTime<Utc>,
    /// Applicable views, ranked by taxonomy overlap with subject.
    pub applicable_views: Vec<RankedView>,
    /// Candidate verbs, ranked and filtered by ABAC + tier + preconditions.
    pub candidate_verbs: Vec<VerbCandidate>,
    /// Candidate attributes, ranked and filtered similarly.
    pub candidate_attributes: Vec<AttributeCandidate>,
    /// Precondition status for top verb candidates.
    pub required_preconditions: Vec<PreconditionStatus>,
    /// Questions to ask if context is ambiguous.
    pub disambiguation_questions: Vec<DisambiguationPrompt>,
    /// Evidence summary (positive and negative).
    pub evidence: EvidenceSummary,
    /// Policy verdicts with snapshot refs.
    pub policy_verdicts: Vec<PolicyVerdict>,
    /// Composite access decision for the overall request.
    pub security_handling: AccessDecision,
    /// Governance signals (gaps, staleness, unowned objects).
    pub governance_signals: Vec<GovernanceSignal>,
    /// Verbs pruned specifically because the requested entity kind did not match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_kind_pruned_verbs: Vec<EntityKindPrunedVerb>,
    /// Overall confidence in the resolution (0.0–1.0).
    pub confidence: f64,
    /// Grounded Sem OS action surface for the resolved subject/slot/node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grounded_action_surface: Option<GroundedActionSurface>,
    /// Whether Sem OS is still navigating discovery or is fully grounded.
    #[serde(default)]
    pub resolution_stage: ResolutionStage,
    /// Discovery-stage navigation surface for empty or under-grounded sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_surface: Option<DiscoverySurface>,
}

/// Top-level stage of the single Sem OS resolution pipeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStage {
    Discovery,
    #[default]
    Grounded,
}

/// Discovery-stage navigation surface returned before full grounding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverySurface {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_universes: Vec<RankedUniverse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_domains: Vec<RankedUniverseDomain>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_families: Vec<RankedConstellationFamily>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_constellations: Vec<RankedConstellation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_inputs: Vec<GroundingInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_questions: Vec<EntryQuestion>,
    pub grounding_readiness: GroundingReadiness,
}

/// Ranked discovery universe match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedUniverse {
    pub fqn: String,
    pub universe_id: String,
    pub name: String,
    pub score: f64,
}

/// Ranked domain match inside a discovery universe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedUniverseDomain {
    pub universe_fqn: String,
    pub domain_id: String,
    pub label: String,
    pub score: f64,
}

/// Ranked constellation family match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedConstellationFamily {
    pub fqn: String,
    pub family_id: String,
    pub label: String,
    pub domain_id: String,
    pub score: f64,
    pub grounding_threshold: GroundingThreshold,
}

/// Ranked concrete constellation suggestion from a family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedConstellation {
    pub family_fqn: String,
    pub constellation_id: String,
    pub label: String,
    pub score: f64,
}

/// Readiness for switching from discovery to grounded resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroundingReadiness {
    #[default]
    NotReady,
    FamilyReady,
    ConstellationReady,
    Grounded,
}

// ── Supporting Types ──────────────────────────────────────────

/// A view definition ranked by taxonomy overlap with the subject.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedView {
    /// Snapshot ID of the ViewDef (pinned).
    pub view_snapshot_id: Uuid,
    /// Object ID of the ViewDef.
    pub view_id: Uuid,
    /// Fully qualified name.
    pub fqn: String,
    /// Human-readable name.
    pub name: String,
    /// Overlap score with subject's taxonomy memberships (0.0–1.0).
    pub overlap_score: f64,
    /// The parsed ViewDef body.
    pub body: ViewDefBody,
}

/// A verb candidate with ranking, precondition, and tier metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbCandidate {
    /// Snapshot ID of the VerbContract (pinned).
    pub verb_snapshot_id: Uuid,
    /// Object ID of the VerbContract.
    pub verb_id: Uuid,
    /// Fully qualified name (e.g. "kyc-case.create").
    pub fqn: String,
    /// Human-readable description.
    pub description: String,
    /// Governance tier of this verb contract.
    pub governance_tier: GovernanceTier,
    /// Trust class.
    pub trust_class: TrustClass,
    /// Ranking score from view prominence (0.0–1.0).
    pub rank_score: f64,
    /// Whether preconditions are currently satisfiable.
    pub preconditions_met: bool,
    /// ABAC access decision for this verb.
    pub access_decision: AccessDecision,
    /// Whether this verb's output can be used as proof evidence.
    pub usable_for_proof: bool,
}

/// The grounded slot/node/action result Sem OS should eventually make canonical.
///
/// This structure is intentionally additive to the existing context-resolution
/// response so callers can migrate to the single pipeline incrementally while
/// Sem OS absorbs the legacy reducer/constellation runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundedActionSurface {
    /// The subject Sem OS resolved this action surface for.
    pub resolved_subject: SubjectRef,
    /// Constellation map selected for this context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_constellation: Option<String>,
    /// Slot path selected inside the bound constellation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_slot_path: Option<String>,
    /// Node identifier inside the hydrated instance graph, if one was bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_node_id: Option<String>,
    /// State machine governing the resolved slot/node, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_state_machine: Option<String>,
    /// Current effective state of the resolved slot/node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_state: Option<String>,
    /// Traversed DAG-like edges Sem OS followed to ground this slot/node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traversed_edges: Vec<GroundedTraversalEdge>,
    /// Structured constraint signals that shaped legality for this slot/node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraint_signals: Vec<GroundedConstraintSignal>,
    /// Valid primitive or macro actions for the resolved slot/node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_actions: Vec<GroundedActionOption>,
    /// Blocked primitive or macro actions with concrete reasons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_actions: Vec<BlockedActionOption>,
    /// DSL candidates Sem OS can render deterministically for execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dsl_candidates: Vec<DslCandidate>,
}

/// A traversed edge in Sem OS grounding provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundedTraversalEdge {
    /// Source node/type in the grounding walk.
    pub from_type: String,
    /// Target node/type in the grounding walk.
    pub to_type: String,
    /// Relationship or derivation name.
    pub relationship: String,
    /// Direction of the traversal.
    pub direction: String,
    /// Source instance identifier when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_instance: Option<String>,
    /// Target instance identifier when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_instance: Option<String>,
}

/// A structured Sem OS legality constraint observed during grounding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundedConstraintSignal {
    /// Signal classification, e.g. `dependency_block` or `state_gate`.
    pub kind: String,
    /// Slot/node the signal applies to.
    pub slot_path: String,
    /// The related slot or dependency, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_slot: Option<String>,
    /// Required state or threshold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_state: Option<String>,
    /// Actual observed state, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_state: Option<String>,
    /// Human-readable explanation.
    pub message: String,
}

/// A grounded action Sem OS considers valid from the current slot/node state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundedActionOption {
    /// Canonical action identifier — primitive verb or macro verb.
    pub action_id: String,
    /// Action kind, e.g. `primitive`, `macro`, or `diagnostic`.
    pub action_kind: String,
    /// Human-readable description for UI and agent ranking.
    pub description: String,
    /// Whether executing this action requires a subject binding.
    #[serde(default)]
    pub requires_subject: bool,
    /// Optional rationale for why this action is currently valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

/// A grounded action Sem OS considers blocked from the current slot/node state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedActionOption {
    /// Canonical action identifier — primitive verb or macro verb.
    pub action_id: String,
    /// Action kind, e.g. `primitive`, `macro`, or `diagnostic`.
    pub action_kind: String,
    /// Human-readable description for UI and agent ranking.
    pub description: String,
    /// Concrete reasons this action is currently blocked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

/// A deterministic DSL candidate emitted from a grounded Sem OS action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DslCandidate {
    /// The canonical action from which this DSL rendering was produced.
    pub action_id: String,
    /// DSL statement or macro invocation text.
    pub dsl: String,
    /// Whether this DSL candidate is directly executable without clarification.
    #[serde(default)]
    pub executable: bool,
}

/// An attribute candidate with ranking and tier metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeCandidate {
    /// Snapshot ID of the AttributeDef (pinned).
    pub attribute_snapshot_id: Uuid,
    /// Object ID of the AttributeDef.
    pub attribute_id: Uuid,
    /// Fully qualified name.
    pub fqn: String,
    /// Human-readable name.
    pub name: String,
    /// Governance tier.
    pub governance_tier: GovernanceTier,
    /// Trust class.
    pub trust_class: TrustClass,
    /// Ranking score from view prominence (0.0–1.0).
    pub rank_score: f64,
    /// ABAC access decision.
    pub access_decision: AccessDecision,
    /// Whether this attribute is required (by policy or evidence).
    pub required: bool,
    /// Whether this attribute is currently populated for the subject.
    pub present: bool,
}

/// Precondition evaluability status for a verb.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreconditionStatus {
    /// Verb FQN.
    pub verb_fqn: String,
    /// Verb snapshot ID.
    pub verb_snapshot_id: Uuid,
    /// Individual precondition checks.
    pub checks: Vec<PreconditionCheck>,
    /// Whether all preconditions are satisfied.
    pub all_satisfied: bool,
    /// Remediation hint if not satisfied.
    #[serde(default)]
    pub remediation_hint: Option<String>,
}

/// A single precondition check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreconditionCheck {
    /// Precondition description.
    pub description: String,
    /// Whether this check passed.
    pub satisfied: bool,
    /// Reason for failure (if any).
    #[serde(default)]
    pub reason: Option<String>,
}

/// A disambiguation prompt when context is ambiguous.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisambiguationPrompt {
    /// Unique ID for this prompt.
    pub prompt_id: Uuid,
    /// The question to ask.
    pub question: String,
    /// Available options.
    pub options: Vec<DisambiguationOption>,
    /// Whether answering is required to proceed.
    pub required_to_proceed: bool,
    /// Rationale for why disambiguation is needed.
    #[serde(default)]
    pub rationale: Option<String>,
}

/// An option in a disambiguation prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisambiguationOption {
    /// Option identifier.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// How this option narrows the context.
    #[serde(default)]
    pub narrows_to: Option<serde_json::Value>,
}

/// Summary of evidence for and against the resolved context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceSummary {
    /// Positive evidence supporting the context.
    pub positive: Vec<EvidenceRef>,
    /// Negative evidence or missing items.
    pub negative: Vec<EvidenceRef>,
}

/// A reference to a piece of evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    /// What kind of evidence (observation, document, attribute value).
    pub kind: String,
    /// FQN or description.
    pub reference: String,
    /// Snapshot ID if pinned.
    #[serde(default)]
    pub snapshot_id: Option<Uuid>,
    /// Freshness — when was this evidence last updated.
    #[serde(default)]
    pub last_updated: Option<DateTime<Utc>>,
    /// Confidence in this evidence (0.0–1.0).
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// A policy verdict with snapshot provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyVerdict {
    /// Snapshot ID of the PolicyRule that produced this verdict.
    pub policy_snapshot_id: Uuid,
    /// Policy FQN.
    pub policy_fqn: String,
    /// Policy name.
    pub policy_name: String,
    /// Whether the policy allows the action.
    pub allowed: bool,
    /// Reason for the verdict.
    pub reason: String,
    /// Actions required by the policy (if any).
    #[serde(default)]
    pub required_actions: Vec<String>,
    /// Regulatory reference (if any).
    #[serde(default)]
    pub regulatory_reference: Option<String>,
}

/// A governance signal indicating a gap or issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceSignal {
    /// Signal kind.
    pub kind: GovernanceSignalKind,
    /// Human-readable message.
    pub message: String,
    /// Severity: info, warning, error.
    pub severity: GovernanceSignalSeverity,
    /// Related object FQN (if applicable).
    #[serde(default)]
    pub related_fqn: Option<String>,
    /// Related snapshot ID (if applicable).
    #[serde(default)]
    pub related_snapshot_id: Option<Uuid>,
}

/// A verb pruned because its `subject_kinds` did not include the requested entity kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityKindPrunedVerb {
    pub fqn: String,
    pub verb_kinds: Vec<String>,
    pub subject_kind: String,
}

/// Categories of governance signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceSignalKind {
    /// A governed object has no steward assigned.
    UnownedObject,
    /// A governed object is not classified in any taxonomy.
    UnclassifiedObject,
    /// Evidence has exceeded its freshness window.
    StaleEvidence,
    /// A retention deadline is approaching.
    RetentionApproaching,
    /// A policy rule has no matching verbs.
    OrphanPolicy,
    /// An attribute is defined but not produced by any verb.
    OrphanAttribute,
    /// Coverage gap in the registry.
    CoverageGap,
}

/// Severity levels for governance signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceSignalSeverity {
    Info,
    Warning,
    Error,
}

// ── Resolved Subject (passed from DB layer) ───────────────────

/// Internal struct for the resolved subject metadata.
/// Constructed by the DB layer (`sem_os_postgres`) and passed into pure functions.
#[derive(Debug, Clone, Default)]
pub struct ResolvedSubject {
    pub entity_type_fqn: Option<String>,
    pub jurisdiction: Option<String>,
    pub state: Option<String>,
}

// ── Taxonomy Membership (loaded from DB, used in pure scoring) ─

/// A loaded taxonomy membership record from `v_active_memberships_by_subject`.
#[derive(Debug, Clone)]
pub struct TaxonomyMembership {
    /// The taxonomy this membership belongs to (e.g., "domain.kyc-tier")
    pub taxonomy_fqn: String,
    /// The specific taxonomy node (e.g., "domain.kyc-tier.high")
    pub node_fqn: String,
    /// What type of registry object is classified
    pub target_type: String,
    /// FQN of the classified object
    pub target_fqn: String,
    /// Kind of membership (direct, inherited, conditional, excluded)
    pub membership_kind: String,
}

/// Aggregated membership context for the subject — computed once and passed through.
#[derive(Debug, Clone, Default)]
pub struct SubjectMemberships {
    /// Taxonomy FQNs the subject's entity type belongs to
    pub subject_taxonomy_fqns: HashSet<String>,
    /// All loaded memberships (for filtering verbs/attributes by taxonomy overlap)
    pub all_memberships: Vec<TaxonomyMembership>,
}

impl SubjectMemberships {
    /// Returns the set of taxonomy FQNs that a given target (verb/attribute) belongs to.
    pub fn taxonomy_fqns_for_target(&self, target_fqn: &str) -> HashSet<String> {
        self.all_memberships
            .iter()
            .filter(|m| m.target_fqn == target_fqn && m.membership_kind != "excluded")
            .map(|m| m.taxonomy_fqn.clone())
            .collect()
    }

    /// Returns true if the subject has any taxonomy memberships.
    pub fn has_memberships(&self) -> bool {
        !self.subject_taxonomy_fqns.is_empty()
    }

    /// Returns the number of overlapping taxonomy FQNs between the subject and a target.
    pub fn taxonomy_overlap_count(&self, target_fqn: &str) -> usize {
        let target_taxonomies = self.taxonomy_fqns_for_target(target_fqn);
        self.subject_taxonomy_fqns
            .intersection(&target_taxonomies)
            .count()
    }
}

/// Relationship context for the subject — loaded in Step 2c (D5).
///
/// Contains relationship type definitions where the subject's entity type
/// appears as either source or target. Exposes `edge_class` for verb filtering.
#[derive(Debug, Clone, Default)]
pub struct SubjectRelationships {
    /// Relationship types where the subject is the source entity type
    pub outgoing: Vec<RelationshipTypeDefBody>,
    /// Relationship types where the subject is the target entity type
    pub incoming: Vec<RelationshipTypeDefBody>,
}

impl SubjectRelationships {
    /// Returns the set of edge classes that the subject participates in.
    pub fn edge_classes(&self) -> HashSet<String> {
        self.outgoing
            .iter()
            .chain(self.incoming.iter())
            .filter_map(|r| r.edge_class.clone())
            .collect()
    }

    /// Returns the set of domains covered by the subject's relationships.
    pub fn relationship_domains(&self) -> HashSet<String> {
        self.outgoing
            .iter()
            .chain(self.incoming.iter())
            .map(|r| r.domain.clone())
            .collect()
    }

    /// Returns true if the subject has any relationships loaded.
    pub fn has_relationships(&self) -> bool {
        !self.outgoing.is_empty() || !self.incoming.is_empty()
    }
}

/// Inputs used to filter and rank candidate verbs during context resolution.
pub struct VerbFilterContext<'a> {
    /// Actor context used for ABAC evaluation.
    pub actor: &'a ActorContext,
    /// Evidence mode for the current resolution.
    pub mode: EvidenceMode,
    /// Highest-ranked view candidate, if available.
    pub top_view: Option<&'a ViewDefBody>,
    /// Canonical dominant entity kind inferred for the subject.
    pub entity_kind: Option<&'a str>,
    /// Confidence for the dominant entity-kind signal.
    pub entity_confidence: Option<f64>,
    /// Taxonomy memberships attached to the subject.
    pub memberships: &'a SubjectMemberships,
    /// Relationship metadata attached to the subject.
    pub relationships: &'a SubjectRelationships,
}

// ── Pure Scoring / Filtering Functions ────────────────────────

/// Step 3: Rank views by taxonomy overlap with the subject.
pub fn rank_views_by_overlap(
    views: &[(SnapshotRow, ViewDefBody)],
    subject: &ResolvedSubject,
    memberships: &SubjectMemberships,
) -> Vec<RankedView> {
    views
        .iter()
        .map(|(row, body)| {
            let overlap = compute_view_overlap(body, subject, memberships);
            RankedView {
                view_snapshot_id: row.snapshot_id,
                view_id: row.object_id,
                fqn: body.fqn.clone(),
                name: body.name.clone(),
                overlap_score: overlap,
                body: body.clone(),
            }
        })
        .filter(|rv| rv.overlap_score > 0.0)
        .collect()
}

/// Compute overlap score between a view and a resolved subject.
pub fn compute_view_overlap(
    view: &ViewDefBody,
    subject: &ResolvedSubject,
    memberships: &SubjectMemberships,
) -> f64 {
    let mut score = 0.0;

    if let Some(ref entity_type) = subject.entity_type_fqn {
        if view.base_entity_type == *entity_type {
            score += 0.8;
        } else if view.domain == entity_type.split('.').next().unwrap_or("") {
            score += 0.4;
        }
    }

    // Taxonomy membership overlap bonus
    if memberships.has_memberships() {
        let view_taxonomies = memberships.taxonomy_fqns_for_target(&view.fqn);
        let overlap_count = memberships
            .subject_taxonomy_fqns
            .intersection(&view_taxonomies)
            .count();
        if overlap_count > 0 {
            score += (overlap_count as f64 * 0.1).min(0.2);
        }
    }

    // Jurisdiction constraint bonus
    if subject.jurisdiction.is_some() {
        let has_jurisdiction_filter = view
            .filters
            .iter()
            .any(|f| f.attribute_fqn.contains("jurisdiction"));
        if has_jurisdiction_filter {
            score += 0.1;
        }
    }

    // Views with more columns are more comprehensive (small bonus)
    if !view.columns.is_empty() {
        score += 0.1_f64.min(view.columns.len() as f64 * 0.01);
    }

    score.min(1.0)
}

/// Step 5: Filter and rank verbs by taxonomy + ABAC + tier + relationship edge_class.
pub fn filter_and_rank_verbs(
    verb_rows: &[SnapshotRow],
    ctx: VerbFilterContext<'_>,
) -> (Vec<VerbCandidate>, Vec<EntityKindPrunedVerb>) {
    let mut candidates = Vec::new();
    let mut entity_kind_pruned = Vec::new();
    let enforce_entity_kind = ctx.entity_confidence.unwrap_or(1.0) >= 0.5;

    for row in verb_rows {
        let body: serde_json::Value = row.definition.clone();
        let fqn = body
            .get("fqn")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = body
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Entity-kind applicability filter
        if enforce_entity_kind {
            if let Some(kind) = ctx.entity_kind {
                let subject_kinds: Vec<String> = body
                    .get("subject_kinds")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let canonical_kind = canonicalize_entity_kind(kind);
                let canonical_subject_kinds: Vec<String> = subject_kinds
                    .iter()
                    .map(|sk| canonicalize_entity_kind(sk))
                    .collect();
                if !canonical_subject_kinds.is_empty()
                    && !canonical_subject_kinds
                        .iter()
                        .any(|sk| sk == &canonical_kind)
                {
                    entity_kind_pruned.push(EntityKindPrunedVerb {
                        fqn,
                        verb_kinds: canonical_subject_kinds,
                        subject_kind: canonical_kind,
                    });
                    continue;
                }
            }
        }

        // Tier/trust filtering based on evidence mode
        if !tier_allowed(row.governance_tier, row.trust_class, ctx.mode, ctx.top_view) {
            continue;
        }

        // ABAC check
        let label = row.parse_security_label().unwrap_or_default();
        let access = evaluate_abac(ctx.actor, &label, AccessPurpose::Operations);
        if !access.is_allowed() && ctx.mode != EvidenceMode::Exploratory {
            continue;
        }

        // Compute rank score from view prominence
        let mut rank_score = compute_verb_prominence(&fqn, ctx.top_view);

        // Boost verbs that explicitly match the entity kind
        if enforce_entity_kind {
            if let Some(kind) = ctx.entity_kind {
                let subject_kinds: Vec<String> = body
                    .get("subject_kinds")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let canonical_kind = canonicalize_entity_kind(kind);
                if subject_kinds
                    .iter()
                    .map(|sk| canonicalize_entity_kind(sk))
                    .any(|sk| sk == canonical_kind)
                {
                    rank_score += 0.15;
                }
            }
        }

        // Taxonomy membership filtering
        if ctx.memberships.has_memberships() {
            let verb_taxonomies = ctx.memberships.taxonomy_fqns_for_target(&fqn);
            if !verb_taxonomies.is_empty() {
                let overlap = ctx
                    .memberships
                    .subject_taxonomy_fqns
                    .intersection(&verb_taxonomies)
                    .count();
                if overlap == 0 {
                    continue;
                }
                rank_score += (overlap as f64 * 0.05).min(0.1);
            }
        }

        // D5: Relationship-aware ranking
        if ctx.relationships.has_relationships() {
            let verb_domain = fqn.split('.').next().unwrap_or("");
            let rel_domains = ctx.relationships.relationship_domains();
            if rel_domains.contains(verb_domain) {
                rank_score += 0.08;
            }
            let edge_classes = ctx.relationships.edge_classes();
            if edge_classes.contains(verb_domain) {
                rank_score += 0.07;
            }
        }

        let usable_for_proof = row.governance_tier == GovernanceTier::Governed
            && matches!(
                row.trust_class,
                TrustClass::Proof | TrustClass::DecisionSupport
            );

        candidates.push(VerbCandidate {
            verb_snapshot_id: row.snapshot_id,
            verb_id: row.object_id,
            fqn,
            description,
            governance_tier: row.governance_tier,
            trust_class: row.trust_class,
            rank_score,
            preconditions_met: true, // evaluated later in Step 8
            access_decision: access,
            usable_for_proof,
        });
    }

    (candidates, entity_kind_pruned)
}

fn canonicalize_entity_kind(kind: &str) -> String {
    let normalized = kind.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "kyc_case" | "case" => "kyc-case".to_string(),
        "client_group" => "client-group".to_string(),
        "legal-entity" | "legal_entity" | "organization" | "org" => "company".to_string(),
        "individual" | "natural_person" => "person".to_string(),
        "client-book" | "client_book" => "client-group".to_string(),
        "investor-register" | "investor_register" => "investor".to_string(),
        "investment-fund" | "umbrella" | "sub-fund" | "compartment" => "fund".to_string(),
        "doc" | "evidence-document" => "document".to_string(),
        "legal-contract" | "agreement" | "msa" => "contract".to_string(),
        "mandate" | "trading-mandate" => "trading-profile".to_string(),
        "deal-record" | "sales-deal" => "deal".to_string(),
        "client-business-unit" | "structure" | "trading-unit" => "cbu".to_string(),
        other => other.to_string(),
    }
}

/// Compute verb prominence based on the top view.
pub fn compute_verb_prominence(verb_fqn: &str, top_view: Option<&ViewDefBody>) -> f64 {
    let Some(view) = top_view else {
        return 0.5;
    };

    let verb_domain = verb_fqn.split('.').next().unwrap_or("");
    if verb_domain == view.domain {
        0.8
    } else {
        0.3
    }
}

/// Step 6: Filter and rank attributes by taxonomy + ABAC + tier.
pub fn filter_and_rank_attributes(
    attr_rows: &[SnapshotRow],
    actor: &ActorContext,
    mode: EvidenceMode,
    top_view: Option<&ViewDefBody>,
    memberships: &SubjectMemberships,
) -> Vec<AttributeCandidate> {
    let mut candidates = Vec::new();

    for row in attr_rows {
        let body: serde_json::Value = row.definition.clone();
        let fqn = body
            .get("fqn")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Tier/trust filtering
        if !tier_allowed(row.governance_tier, row.trust_class, mode, top_view) {
            continue;
        }

        // ABAC check
        let label = row.parse_security_label().unwrap_or_default();
        let access = evaluate_abac(actor, &label, AccessPurpose::Operations);
        if !access.is_allowed() && mode != EvidenceMode::Exploratory {
            continue;
        }

        // Rank from view columns
        let (mut rank_score, required) = compute_attribute_prominence(&fqn, top_view);

        // Taxonomy membership filtering
        if memberships.has_memberships() {
            let attr_taxonomies = memberships.taxonomy_fqns_for_target(&fqn);
            if !attr_taxonomies.is_empty() {
                let overlap = memberships
                    .subject_taxonomy_fqns
                    .intersection(&attr_taxonomies)
                    .count();
                if overlap == 0 {
                    continue;
                }
                rank_score += (overlap as f64 * 0.05).min(0.1);
            }
        }

        candidates.push(AttributeCandidate {
            attribute_snapshot_id: row.snapshot_id,
            attribute_id: row.object_id,
            fqn,
            name,
            governance_tier: row.governance_tier,
            trust_class: row.trust_class,
            rank_score,
            access_decision: access,
            required,
            present: false,
        });
    }

    candidates
}

/// Compute attribute prominence based on the top view.
pub fn compute_attribute_prominence(attr_fqn: &str, top_view: Option<&ViewDefBody>) -> (f64, bool) {
    let Some(view) = top_view else {
        return (0.5, false);
    };

    // Check if this attribute is a column in the view
    for (i, col) in view.columns.iter().enumerate() {
        if col.attribute_fqn == attr_fqn {
            let position_score = 1.0 - (i as f64 * 0.05).min(0.5);
            return (position_score, col.visible);
        }
    }

    // Check if used in a filter
    for filter in &view.filters {
        if filter.attribute_fqn == attr_fqn {
            return (0.6, true);
        }
    }

    // Attribute domain matches view domain
    let attr_domain = attr_fqn.split('.').next().unwrap_or("");
    if attr_domain == view.domain {
        return (0.4, false);
    }

    (0.2, false)
}

// ── Tier/Trust Filtering ──────────────────────────────────────

/// Check whether an object with the given tier/trust is allowed in the current mode.
pub fn tier_allowed(
    tier: GovernanceTier,
    trust: TrustClass,
    mode: EvidenceMode,
    view: Option<&ViewDefBody>,
) -> bool {
    match mode {
        EvidenceMode::Strict => {
            tier == GovernanceTier::Governed
                && matches!(trust, TrustClass::Proof | TrustClass::DecisionSupport)
        }
        EvidenceMode::Normal => {
            if tier == GovernanceTier::Governed {
                return true;
            }
            // Operational only if the active view opts in via includes_operational
            view.is_some_and(|v| v.includes_operational)
        }
        EvidenceMode::Exploratory => true,
        EvidenceMode::Governance => true,
    }
}

// ── Step 8: Evaluate Preconditions ────────────────────────────

/// Evaluate preconditions for top-N candidate verbs.
pub fn evaluate_verb_preconditions(verbs: &[VerbCandidate]) -> Vec<PreconditionStatus> {
    verbs
        .iter()
        .take(10)
        .map(|v| PreconditionStatus {
            verb_fqn: v.fqn.clone(),
            verb_snapshot_id: v.verb_snapshot_id,
            checks: vec![],
            all_satisfied: v.preconditions_met,
            remediation_hint: None,
        })
        .collect()
}

// ── Step 9: Evaluate Policies ─────────────────────────────────

/// Evaluate policy rules against verb candidates and actor context.
pub fn evaluate_policies(
    policy_rows: &[SnapshotRow],
    verbs: &[VerbCandidate],
    _actor: &ActorContext,
) -> Vec<PolicyVerdict> {
    let mut verdicts = Vec::new();

    for row in policy_rows {
        let body: PolicyRuleBody = match row.parse_definition() {
            Ok(b) => b,
            Err(_) => continue,
        };

        if !body.enabled {
            continue;
        }

        let applies = body.predicates.iter().any(|pred| match pred.kind.as_str() {
            "governance_tier" => verbs.iter().any(|v| {
                let tier_str = format!("{:?}", v.governance_tier).to_lowercase();
                match pred.operator.as_str() {
                    "eq" => pred.value.as_str() == Some(tier_str.as_str()),
                    "ne" => pred.value.as_str() != Some(tier_str.as_str()),
                    _ => false,
                }
            }),
            "trust_class" => verbs.iter().any(|v| {
                let trust_str = format!("{:?}", v.trust_class).to_lowercase();
                pred.value.as_str() == Some(trust_str.as_str())
            }),
            _ => false,
        });

        if applies || body.predicates.is_empty() {
            let allowed = !body
                .actions
                .iter()
                .any(|a| a.kind == "block_publish" || a.kind == "restrict_access");

            let required_actions: Vec<String> = body
                .actions
                .iter()
                .filter(|a| a.kind == "require_evidence" || a.kind == "require_approval")
                .map(|a| a.description.clone().unwrap_or_else(|| a.kind.clone()))
                .collect();

            verdicts.push(PolicyVerdict {
                policy_snapshot_id: row.snapshot_id,
                policy_fqn: body.fqn.clone(),
                policy_name: body.name.clone(),
                allowed,
                reason: if allowed {
                    "Policy permits action".into()
                } else {
                    "Policy restricts action".into()
                },
                required_actions,
                regulatory_reference: None,
            });
        }
    }

    verdicts
}

// ── Step 10: Composite Access Decision ────────────────────────

/// Compute composite access decision from verb decisions and policy verdicts.
pub fn compute_composite_access(
    verbs: &[VerbCandidate],
    policy_verdicts: &[PolicyVerdict],
) -> AccessDecision {
    // If any policy blocks, deny
    if policy_verdicts.iter().any(|v| !v.allowed) {
        return AccessDecision::Deny {
            reason: "One or more policies restrict access".into(),
        };
    }

    // If any verb is denied by ABAC, report masking
    let denied_verbs: Vec<_> = verbs
        .iter()
        .filter(|v| matches!(v.access_decision, AccessDecision::Deny { .. }))
        .collect();

    if !denied_verbs.is_empty() {
        return AccessDecision::AllowWithMasking {
            masked_fields: denied_verbs.iter().map(|v| v.fqn.clone()).collect(),
        };
    }

    // Check for masking requirements
    let needs_masking: Vec<_> = verbs
        .iter()
        .filter(|v| matches!(v.access_decision, AccessDecision::AllowWithMasking { .. }))
        .collect();

    if !needs_masking.is_empty() {
        let mut all_masked = Vec::new();
        for v in &needs_masking {
            if let AccessDecision::AllowWithMasking { masked_fields } = &v.access_decision {
                all_masked.extend(masked_fields.clone());
            }
        }
        return AccessDecision::AllowWithMasking {
            masked_fields: all_masked,
        };
    }

    AccessDecision::Allow
}

// ── Step 11: Governance Signals ───────────────────────────────

/// Generate governance signals based on verb/attribute state.
pub fn generate_governance_signals(
    verbs: &[VerbCandidate],
    attributes: &[AttributeCandidate],
    mode: EvidenceMode,
) -> Vec<GovernanceSignal> {
    let mut signals = Vec::new();

    if mode != EvidenceMode::Governance && mode != EvidenceMode::Exploratory {
        return signals;
    }

    // Check for attributes that are required but not present
    for attr in attributes {
        if attr.required && !attr.present {
            signals.push(GovernanceSignal {
                kind: GovernanceSignalKind::CoverageGap,
                message: format!("Required attribute '{}' is not populated", attr.fqn),
                severity: GovernanceSignalSeverity::Warning,
                related_fqn: Some(attr.fqn.clone()),
                related_snapshot_id: Some(attr.attribute_snapshot_id),
            });
        }
    }

    // Check for verbs with failed preconditions
    for verb in verbs {
        if !verb.preconditions_met {
            signals.push(GovernanceSignal {
                kind: GovernanceSignalKind::CoverageGap,
                message: format!("Verb '{}' has unsatisfied preconditions", verb.fqn),
                severity: GovernanceSignalSeverity::Info,
                related_fqn: Some(verb.fqn.clone()),
                related_snapshot_id: Some(verb.verb_snapshot_id),
            });
        }
    }

    // Check for operational verbs being used
    for verb in verbs {
        if verb.governance_tier == GovernanceTier::Operational && !verb.usable_for_proof {
            signals.push(GovernanceSignal {
                kind: GovernanceSignalKind::CoverageGap,
                message: format!(
                    "Verb '{}' is operational-tier — outputs not usable for proof",
                    verb.fqn
                ),
                severity: GovernanceSignalSeverity::Info,
                related_fqn: Some(verb.fqn.clone()),
                related_snapshot_id: Some(verb.verb_snapshot_id),
            });
        }
    }

    signals
}

// ── Step 12: Confidence Score ─────────────────────────────────

/// Compute overall confidence score for the resolution.
pub fn compute_confidence(
    views: &[RankedView],
    verbs: &[VerbCandidate],
    preconditions: &[PreconditionStatus],
    attributes: &[AttributeCandidate],
) -> f64 {
    // view_match_score × 0.30
    let view_score = views.first().map(|v| v.overlap_score).unwrap_or(0.0);

    // precondition_satisfiable_pct × 0.25
    let precondition_pct = if preconditions.is_empty() {
        1.0
    } else {
        let satisfied = preconditions.iter().filter(|p| p.all_satisfied).count();
        satisfied as f64 / preconditions.len() as f64
    };

    // required_inputs_present_pct × 0.30
    let required_attrs = attributes.iter().filter(|a| a.required).count();
    let present_required = attributes
        .iter()
        .filter(|a| a.required && a.present)
        .count();
    let inputs_pct = if required_attrs == 0 {
        1.0
    } else {
        present_required as f64 / required_attrs as f64
    };

    // abac_permit_pct × 0.15
    let abac_pct = if verbs.is_empty() {
        1.0
    } else {
        let permitted = verbs
            .iter()
            .filter(|v| v.access_decision.is_allowed())
            .count();
        permitted as f64 / verbs.len() as f64
    };

    let confidence =
        view_score * 0.30 + precondition_pct * 0.25 + inputs_pct * 0.30 + abac_pct * 0.15;

    confidence.clamp(0.0, 1.0)
}

// ── Disambiguation Generation ─────────────────────────────────

/// Generate disambiguation prompts when confidence is low.
pub fn generate_disambiguation(
    views: &[RankedView],
    _verbs: &[VerbCandidate],
) -> Vec<DisambiguationPrompt> {
    let mut prompts = Vec::new();

    if views.len() >= 2 {
        let top = views[0].overlap_score;
        let close_views: Vec<_> = views
            .iter()
            .filter(|v| (top - v.overlap_score).abs() < 0.1)
            .collect();

        if close_views.len() >= 2 {
            prompts.push(DisambiguationPrompt {
                prompt_id: Uuid::new_v4(),
                question: "Multiple views match this context. Which perspective would you like?"
                    .into(),
                options: close_views
                    .iter()
                    .map(|v| DisambiguationOption {
                        id: v.fqn.clone(),
                        label: v.name.clone(),
                        description: Some(format!(
                            "View over {} (overlap: {:.0}%)",
                            v.body.base_entity_type,
                            v.overlap_score * 100.0
                        )),
                        narrows_to: Some(serde_json::json!({
                            "view_id": v.view_id
                        })),
                    })
                    .collect(),
                required_to_proceed: false,
                rationale: Some("Views have similar taxonomy overlap scores".into()),
            });
        }
    }

    prompts
}

// ── S14: Conditional Membership Evaluation ────────────────────

/// Evaluate conditional membership rules (S14).
///
/// Returns a set of `"{taxonomy_fqn}::{target_fqn}"` keys for conditional
/// memberships that should be excluded because their conditions cannot be
/// verified in the current context.
///
/// Conditions that reference entity attributes (e.g. `attribute_equals`)
/// require runtime entity state that `resolve_context()` does not have.
/// These memberships are conservatively excluded from overlap scoring.
///
/// Conditions with no predicates (empty conditions vec) are treated as
/// satisfied — they represent unconditional "conditional" memberships
/// (effectively direct).
pub fn evaluate_conditional_memberships(rules: &[MembershipRuleBody]) -> HashSet<String> {
    let mut excluded = HashSet::new();

    for rule in rules {
        if rule.conditions.is_empty() {
            continue;
        }

        let all_verifiable = rule.conditions.iter().all(evaluate_condition);
        if !all_verifiable {
            let key = format!("{}::{}", rule.taxonomy_fqn, rule.target_fqn);
            excluded.insert(key);
        }
    }

    excluded
}

/// Evaluate a single membership condition.
///
/// Returns `true` if the condition is verifiable and passes in the current
/// static context. Returns `false` if the condition requires runtime entity
/// state that we don't have.
pub fn evaluate_condition(condition: &MembershipCondition) -> bool {
    match condition.kind.as_str() {
        // Conditions that require entity attribute state — not available
        "attribute_equals" | "attribute_in" | "attribute_not_in" | "attribute_gt"
        | "attribute_lt" => false,
        // Conditions based on entity role — not available in resolution context
        "entity_has_role" | "entity_in_jurisdiction" => false,
        // Static conditions that can be evaluated without runtime state
        "always_true" => true,
        "always_false" => false,
        // Unknown condition types — conservatively exclude
        _ => false,
    }
}

/// Build `SubjectMemberships` from raw data and conditional rule evaluation.
///
/// This is the pure logic portion of Step 2b — the DB layer loads raw data
/// and conditional rules, then calls this function to build the final memberships.
pub fn build_subject_memberships(
    all_memberships: Vec<TaxonomyMembership>,
    entity_type_fqn: Option<&str>,
    conditional_rules: &[MembershipRuleBody],
) -> SubjectMemberships {
    let excluded_conditionals = evaluate_conditional_memberships(conditional_rules);

    let mut subject_taxonomy_fqns = HashSet::new();

    if let Some(entity_type_fqn) = entity_type_fqn {
        for m in &all_memberships {
            if m.target_type == "entity_type_def"
                && m.target_fqn == *entity_type_fqn
                && m.membership_kind != "excluded"
            {
                // S14: If this is a conditional membership, check if it was excluded
                if m.membership_kind == "conditional" {
                    let key = format!("{}::{}", m.taxonomy_fqn, m.target_fqn);
                    if excluded_conditionals.contains(&key) {
                        continue;
                    }
                }
                subject_taxonomy_fqns.insert(m.taxonomy_fqn.clone());
            }
        }
    }

    SubjectMemberships {
        subject_taxonomy_fqns,
        all_memberships,
    }
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sem_os_types::Classification;

    fn test_actor() -> ActorContext {
        ActorContext {
            actor_id: "agent-1".into(),
            roles: vec!["analyst".into()],
            department: Some("compliance".into()),
            clearance: Some(Classification::Confidential),
            jurisdictions: vec!["LU".into(), "DE".into()],
        }
    }

    fn test_request() -> ContextResolutionRequest {
        ContextResolutionRequest {
            subject: SubjectRef::EntityId(Uuid::new_v4()),
            intent_summary: Some("discover UBO structure".into()),
            raw_utterance: Some("Help me discover the UBO structure".into()),
            actor: test_actor(),
            goals: vec!["resolve_ubo".into()],
            constraints: ResolutionConstraints::default(),
            evidence_mode: EvidenceMode::Normal,
            point_in_time: None,
            entity_kind: None,
            entity_confidence: None,
            discovery: DiscoveryContext::default(),
        }
    }

    #[test]
    fn test_evidence_mode_default() {
        assert_eq!(EvidenceMode::default(), EvidenceMode::Normal);
    }

    #[test]
    fn test_subject_ref_id() {
        let id = Uuid::new_v4();
        assert_eq!(SubjectRef::CaseId(id).id(), id);
        assert_eq!(SubjectRef::EntityId(id).id(), id);
        assert_eq!(SubjectRef::DocumentId(id).id(), id);
        assert_eq!(SubjectRef::TaskId(id).id(), id);
        assert_eq!(SubjectRef::ViewId(id).id(), id);
    }

    #[test]
    fn test_subject_ref_serde() {
        let subject = SubjectRef::CaseId(Uuid::new_v4());
        let json = serde_json::to_value(&subject).unwrap();
        assert_eq!(json["type"], "case_id");
        let round: SubjectRef = serde_json::from_value(json).unwrap();
        assert_eq!(round, subject);
    }

    #[test]
    fn test_request_serde() {
        let req = test_request();
        let json = serde_json::to_value(&req).unwrap();
        let round: ContextResolutionRequest = serde_json::from_value(json).unwrap();
        assert_eq!(round.evidence_mode, EvidenceMode::Normal);
        assert_eq!(round.goals, vec!["resolve_ubo"]);
    }

    fn view_with_operational(includes: bool) -> ViewDefBody {
        ViewDefBody {
            fqn: "test.view".into(),
            name: "Test View".into(),
            description: "Test".into(),
            domain: "test".into(),
            base_entity_type: "test".into(),
            columns: vec![],
            filters: vec![],
            sort_order: vec![],
            includes_operational: includes,
        }
    }

    #[test]
    fn test_tier_allowed_strict() {
        assert!(tier_allowed(
            GovernanceTier::Governed,
            TrustClass::Proof,
            EvidenceMode::Strict,
            None,
        ));
        assert!(tier_allowed(
            GovernanceTier::Governed,
            TrustClass::DecisionSupport,
            EvidenceMode::Strict,
            None,
        ));
        assert!(!tier_allowed(
            GovernanceTier::Governed,
            TrustClass::Convenience,
            EvidenceMode::Strict,
            None,
        ));
        assert!(!tier_allowed(
            GovernanceTier::Operational,
            TrustClass::DecisionSupport,
            EvidenceMode::Strict,
            None,
        ));
    }

    #[test]
    fn test_tier_allowed_normal_governed_always_allowed() {
        assert!(tier_allowed(
            GovernanceTier::Governed,
            TrustClass::Proof,
            EvidenceMode::Normal,
            None,
        ));
        assert!(tier_allowed(
            GovernanceTier::Governed,
            TrustClass::Convenience,
            EvidenceMode::Normal,
            None,
        ));
    }

    #[test]
    fn test_tier_allowed_normal_operational_filtered_by_view() {
        assert!(!tier_allowed(
            GovernanceTier::Operational,
            TrustClass::Convenience,
            EvidenceMode::Normal,
            None,
        ));

        let view_no_op = view_with_operational(false);
        assert!(!tier_allowed(
            GovernanceTier::Operational,
            TrustClass::Convenience,
            EvidenceMode::Normal,
            Some(&view_no_op),
        ));

        let view_with_op = view_with_operational(true);
        assert!(tier_allowed(
            GovernanceTier::Operational,
            TrustClass::Convenience,
            EvidenceMode::Normal,
            Some(&view_with_op),
        ));
    }

    #[test]
    fn test_tier_allowed_exploratory() {
        assert!(tier_allowed(
            GovernanceTier::Operational,
            TrustClass::Convenience,
            EvidenceMode::Exploratory,
            None,
        ));
    }

    #[test]
    fn test_confidence_computation() {
        let views = vec![RankedView {
            view_snapshot_id: Uuid::new_v4(),
            view_id: Uuid::new_v4(),
            fqn: "test.view".into(),
            name: "Test View".into(),
            overlap_score: 0.8,
            body: view_with_operational(false),
        }];

        let verbs = vec![VerbCandidate {
            verb_snapshot_id: Uuid::new_v4(),
            verb_id: Uuid::new_v4(),
            fqn: "test.create".into(),
            description: "Create test".into(),
            governance_tier: GovernanceTier::Governed,
            trust_class: TrustClass::Proof,
            rank_score: 0.8,
            preconditions_met: true,
            access_decision: AccessDecision::Allow,
            usable_for_proof: true,
        }];

        let preconditions = vec![PreconditionStatus {
            verb_fqn: "test.create".into(),
            verb_snapshot_id: Uuid::new_v4(),
            checks: vec![],
            all_satisfied: true,
            remediation_hint: None,
        }];

        let attrs: Vec<AttributeCandidate> = vec![];

        let confidence = compute_confidence(&views, &verbs, &preconditions, &attrs);
        // 0.8 * 0.30 + 1.0 * 0.25 + 1.0 * 0.30 + 1.0 * 0.15 = 0.94
        assert!((confidence - 0.94).abs() < 0.01);
    }

    #[test]
    fn test_confidence_low_when_no_views() {
        let confidence = compute_confidence(&[], &[], &[], &[]);
        // 0.0 * 0.30 + 1.0 * 0.25 + 1.0 * 0.30 + 1.0 * 0.15 = 0.70
        assert!((confidence - 0.70).abs() < 0.01);
    }

    #[test]
    fn test_governance_signal_kinds() {
        let signal = GovernanceSignal {
            kind: GovernanceSignalKind::StaleEvidence,
            message: "Evidence expired".into(),
            severity: GovernanceSignalSeverity::Warning,
            related_fqn: Some("obs.pep-screening".into()),
            related_snapshot_id: None,
        };
        let json = serde_json::to_value(&signal).unwrap();
        assert_eq!(json["kind"], "stale_evidence");
        assert_eq!(json["severity"], "warning");
    }

    #[test]
    fn test_composite_access_deny_on_policy_block() {
        let verbs = vec![VerbCandidate {
            verb_snapshot_id: Uuid::new_v4(),
            verb_id: Uuid::new_v4(),
            fqn: "test.create".into(),
            description: "Test".into(),
            governance_tier: GovernanceTier::Governed,
            trust_class: TrustClass::Proof,
            rank_score: 0.8,
            preconditions_met: true,
            access_decision: AccessDecision::Allow,
            usable_for_proof: true,
        }];

        let verdicts = vec![PolicyVerdict {
            policy_snapshot_id: Uuid::new_v4(),
            policy_fqn: "test.block".into(),
            policy_name: "Block".into(),
            allowed: false,
            reason: "Blocked".into(),
            required_actions: vec![],
            regulatory_reference: None,
        }];

        let access = compute_composite_access(&verbs, &verdicts);
        assert!(matches!(access, AccessDecision::Deny { .. }));
    }

    #[test]
    fn test_disambiguation_generated_for_close_views() {
        let view_body = view_with_operational(false);

        let views = vec![
            RankedView {
                view_snapshot_id: Uuid::new_v4(),
                view_id: Uuid::new_v4(),
                fqn: "test.view1".into(),
                name: "View 1".into(),
                overlap_score: 0.8,
                body: view_body.clone(),
            },
            RankedView {
                view_snapshot_id: Uuid::new_v4(),
                view_id: Uuid::new_v4(),
                fqn: "test.view2".into(),
                name: "View 2".into(),
                overlap_score: 0.75,
                body: view_body,
            },
        ];

        let prompts = generate_disambiguation(&views, &[]);
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].options.len(), 2);
    }

    #[test]
    fn test_view_overlap_exact_entity_type_match() {
        let view = ViewDefBody {
            fqn: "kyc.ubo-view".into(),
            name: "UBO View".into(),
            description: "UBO discovery view".into(),
            domain: "kyc".into(),
            base_entity_type: "entity.proper-person".into(),
            columns: vec![],
            filters: vec![],
            sort_order: vec![],
            includes_operational: false,
        };

        let subject = ResolvedSubject {
            entity_type_fqn: Some("entity.proper-person".into()),
            jurisdiction: None,
            state: None,
        };

        let no_memberships = SubjectMemberships::default();
        let score = compute_view_overlap(&view, &subject, &no_memberships);
        assert!(score >= 0.8);
    }

    #[test]
    fn test_view_overlap_domain_match() {
        let view = ViewDefBody {
            fqn: "kyc.case-view".into(),
            name: "Case View".into(),
            description: "KYC case view".into(),
            domain: "kyc".into(),
            base_entity_type: "kyc.case".into(),
            columns: vec![],
            filters: vec![],
            sort_order: vec![],
            includes_operational: false,
        };

        let subject = ResolvedSubject {
            entity_type_fqn: Some("kyc.enhanced-case".into()),
            jurisdiction: None,
            state: None,
        };

        let no_memberships = SubjectMemberships::default();
        let score = compute_view_overlap(&view, &subject, &no_memberships);
        assert!(score >= 0.4);
        assert!(score < 0.8);
    }

    #[test]
    fn test_view_overlap_with_taxonomy_memberships() {
        let view = ViewDefBody {
            fqn: "kyc.ubo-view".into(),
            name: "UBO View".into(),
            description: "UBO discovery view".into(),
            domain: "kyc".into(),
            base_entity_type: "entity.proper-person".into(),
            columns: vec![],
            filters: vec![],
            sort_order: vec![],
            includes_operational: false,
        };

        let subject = ResolvedSubject {
            entity_type_fqn: Some("entity.proper-person".into()),
            jurisdiction: None,
            state: None,
        };

        let memberships = SubjectMemberships {
            subject_taxonomy_fqns: HashSet::from(["domain.kyc".into()]),
            all_memberships: vec![TaxonomyMembership {
                taxonomy_fqn: "domain.kyc".into(),
                node_fqn: "domain.kyc.ubo".into(),
                target_type: "view_def".into(),
                target_fqn: "kyc.ubo-view".into(),
                membership_kind: "direct".into(),
            }],
        };

        let score_with = compute_view_overlap(&view, &subject, &memberships);
        let no_memberships = SubjectMemberships::default();
        let score_without = compute_view_overlap(&view, &subject, &no_memberships);

        assert!(score_with > score_without);
    }

    #[test]
    fn test_subject_memberships_overlap_count() {
        let memberships = SubjectMemberships {
            subject_taxonomy_fqns: HashSet::from(["domain.kyc".into(), "risk.high".into()]),
            all_memberships: vec![
                TaxonomyMembership {
                    taxonomy_fqn: "domain.kyc".into(),
                    node_fqn: "domain.kyc.ubo".into(),
                    target_type: "verb_contract".into(),
                    target_fqn: "kyc-case.create".into(),
                    membership_kind: "direct".into(),
                },
                TaxonomyMembership {
                    taxonomy_fqn: "risk.high".into(),
                    node_fqn: "risk.high.pep".into(),
                    target_type: "verb_contract".into(),
                    target_fqn: "kyc-case.create".into(),
                    membership_kind: "direct".into(),
                },
                TaxonomyMembership {
                    taxonomy_fqn: "domain.trading".into(),
                    node_fqn: "domain.trading.equities".into(),
                    target_type: "verb_contract".into(),
                    target_fqn: "trading.create-profile".into(),
                    membership_kind: "direct".into(),
                },
            ],
        };

        assert_eq!(memberships.taxonomy_overlap_count("kyc-case.create"), 2);
        assert_eq!(
            memberships.taxonomy_overlap_count("trading.create-profile"),
            0
        );
        assert_eq!(memberships.taxonomy_overlap_count("unknown.verb"), 0);
    }

    #[test]
    fn test_subject_memberships_excluded_filtered() {
        let memberships = SubjectMemberships {
            subject_taxonomy_fqns: HashSet::from(["domain.kyc".into()]),
            all_memberships: vec![TaxonomyMembership {
                taxonomy_fqn: "domain.kyc".into(),
                node_fqn: "domain.kyc.sanctions".into(),
                target_type: "verb_contract".into(),
                target_fqn: "kyc.screen".into(),
                membership_kind: "excluded".into(),
            }],
        };

        let target_taxonomies = memberships.taxonomy_fqns_for_target("kyc.screen");
        assert!(target_taxonomies.is_empty());
        assert_eq!(memberships.taxonomy_overlap_count("kyc.screen"), 0);
    }

    #[test]
    fn test_graceful_degradation_no_memberships() {
        let memberships = SubjectMemberships::default();
        assert!(!memberships.has_memberships());
        assert_eq!(memberships.taxonomy_overlap_count("any.verb"), 0);
    }

    // ── D5: SubjectRelationships tests ────────────────────────

    #[test]
    fn test_subject_relationships_edge_classes() {
        let rels = SubjectRelationships {
            outgoing: vec![RelationshipTypeDefBody {
                fqn: "relationship.ownership".into(),
                name: "Ownership".into(),
                description: "Ownership".into(),
                domain: "ownership".into(),
                source_entity_type_fqn: "entity.fund".into(),
                target_entity_type_fqn: "entity.legal_entity".into(),
                cardinality:
                    sem_os_ontology::relationship_type_def::RelationshipCardinality::OneToMany,
                edge_class: Some("ownership".into()),
                directionality: Some(
                    sem_os_ontology::relationship_type_def::Directionality::Forward,
                ),
                inverse_fqn: None,
                constraints: vec![],
            }],
            incoming: vec![RelationshipTypeDefBody {
                fqn: "relationship.custody_of".into(),
                name: "Custody Of".into(),
                description: "Custody".into(),
                domain: "custody".into(),
                source_entity_type_fqn: "entity.custodian".into(),
                target_entity_type_fqn: "entity.fund".into(),
                cardinality:
                    sem_os_ontology::relationship_type_def::RelationshipCardinality::OneToMany,
                edge_class: Some("service".into()),
                directionality: None,
                inverse_fqn: None,
                constraints: vec![],
            }],
        };

        let classes = rels.edge_classes();
        assert!(classes.contains("ownership"));
        assert!(classes.contains("service"));
        assert_eq!(classes.len(), 2);
    }

    #[test]
    fn test_subject_relationships_domains() {
        let rels = SubjectRelationships {
            outgoing: vec![RelationshipTypeDefBody {
                fqn: "relationship.ownership".into(),
                name: "Ownership".into(),
                description: "Ownership".into(),
                domain: "ownership".into(),
                source_entity_type_fqn: "entity.fund".into(),
                target_entity_type_fqn: "entity.legal_entity".into(),
                cardinality:
                    sem_os_ontology::relationship_type_def::RelationshipCardinality::OneToMany,
                edge_class: None,
                directionality: None,
                inverse_fqn: None,
                constraints: vec![],
            }],
            incoming: vec![],
        };

        let domains = rels.relationship_domains();
        assert!(domains.contains("ownership"));
        assert!(rels.has_relationships());
    }

    #[test]
    fn test_subject_relationships_empty() {
        let rels = SubjectRelationships::default();
        assert!(!rels.has_relationships());
        assert!(rels.edge_classes().is_empty());
        assert!(rels.relationship_domains().is_empty());
    }

    // ── S14: Conditional membership evaluation tests ──────────

    #[test]
    fn test_evaluate_condition_attribute_equals_excluded() {
        let cond = MembershipCondition {
            kind: "attribute_equals".into(),
            field: "pep_status".into(),
            operator: "eq".into(),
            value: serde_json::json!("active"),
        };
        assert!(!evaluate_condition(&cond));
    }

    #[test]
    fn test_evaluate_condition_always_true() {
        let cond = MembershipCondition {
            kind: "always_true".into(),
            field: "".into(),
            operator: "eq".into(),
            value: serde_json::json!(true),
        };
        assert!(evaluate_condition(&cond));
    }

    #[test]
    fn test_evaluate_condition_always_false() {
        let cond = MembershipCondition {
            kind: "always_false".into(),
            field: "".into(),
            operator: "eq".into(),
            value: serde_json::json!(false),
        };
        assert!(!evaluate_condition(&cond));
    }

    #[test]
    fn test_evaluate_condition_unknown_kind_excluded() {
        let cond = MembershipCondition {
            kind: "custom_check".into(),
            field: "something".into(),
            operator: "eq".into(),
            value: serde_json::json!("value"),
        };
        assert!(!evaluate_condition(&cond));
    }

    #[test]
    fn test_evaluate_conditional_memberships_empty_conditions_pass() {
        let rules = vec![MembershipRuleBody {
            fqn: "rule.test".into(),
            name: "Test".into(),
            description: None,
            taxonomy_fqn: "taxonomy.risk-tier".into(),
            node_fqn: "taxonomy.risk-tier.high".into(),
            membership_kind: MembershipKind::Conditional,
            target_type: "entity_type_def".into(),
            target_fqn: "entity.person".into(),
            conditions: vec![],
        }];
        let excluded = evaluate_conditional_memberships(&rules);
        assert!(excluded.is_empty());
    }

    #[test]
    fn test_evaluate_conditional_memberships_with_attribute_condition_excluded() {
        let rules = vec![MembershipRuleBody {
            fqn: "rule.pep-check".into(),
            name: "PEP Check".into(),
            description: None,
            taxonomy_fqn: "taxonomy.risk-tier".into(),
            node_fqn: "taxonomy.risk-tier.high".into(),
            membership_kind: MembershipKind::Conditional,
            target_type: "entity_type_def".into(),
            target_fqn: "entity.person".into(),
            conditions: vec![MembershipCondition {
                kind: "attribute_equals".into(),
                field: "pep_status".into(),
                operator: "eq".into(),
                value: serde_json::json!("active"),
            }],
        }];
        let excluded = evaluate_conditional_memberships(&rules);
        assert_eq!(excluded.len(), 1);
        assert!(excluded.contains("taxonomy.risk-tier::entity.person"));
    }

    #[test]
    fn test_evaluate_conditional_memberships_mixed_rules() {
        let rules = vec![
            MembershipRuleBody {
                fqn: "rule.always".into(),
                name: "Always".into(),
                description: None,
                taxonomy_fqn: "taxonomy.subject-category".into(),
                node_fqn: "taxonomy.subject-category.fund".into(),
                membership_kind: MembershipKind::Conditional,
                target_type: "entity_type_def".into(),
                target_fqn: "entity.fund".into(),
                conditions: vec![MembershipCondition {
                    kind: "always_true".into(),
                    field: "".into(),
                    operator: "eq".into(),
                    value: serde_json::json!(true),
                }],
            },
            MembershipRuleBody {
                fqn: "rule.pep".into(),
                name: "PEP".into(),
                description: None,
                taxonomy_fqn: "taxonomy.risk-tier".into(),
                node_fqn: "taxonomy.risk-tier.high".into(),
                membership_kind: MembershipKind::Conditional,
                target_type: "entity_type_def".into(),
                target_fqn: "entity.person".into(),
                conditions: vec![MembershipCondition {
                    kind: "attribute_equals".into(),
                    field: "pep_status".into(),
                    operator: "eq".into(),
                    value: serde_json::json!("active"),
                }],
            },
        ];
        let excluded = evaluate_conditional_memberships(&rules);
        assert_eq!(excluded.len(), 1);
        assert!(excluded.contains("taxonomy.risk-tier::entity.person"));
        assert!(!excluded.contains("taxonomy.subject-category::entity.fund"));
    }

    #[test]
    fn test_build_subject_memberships_filters_conditionals() {
        let memberships = vec![
            TaxonomyMembership {
                taxonomy_fqn: "domain.kyc".into(),
                node_fqn: "domain.kyc.ubo".into(),
                target_type: "entity_type_def".into(),
                target_fqn: "entity.person".into(),
                membership_kind: "direct".into(),
            },
            TaxonomyMembership {
                taxonomy_fqn: "risk.high".into(),
                node_fqn: "risk.high.pep".into(),
                target_type: "entity_type_def".into(),
                target_fqn: "entity.person".into(),
                membership_kind: "conditional".into(),
            },
        ];

        let conditional_rules = vec![MembershipRuleBody {
            fqn: "rule.pep".into(),
            name: "PEP".into(),
            description: None,
            taxonomy_fqn: "risk.high".into(),
            node_fqn: "risk.high.pep".into(),
            membership_kind: MembershipKind::Conditional,
            target_type: "entity_type_def".into(),
            target_fqn: "entity.person".into(),
            conditions: vec![MembershipCondition {
                kind: "attribute_equals".into(),
                field: "pep_status".into(),
                operator: "eq".into(),
                value: serde_json::json!("active"),
            }],
        }];

        let result =
            build_subject_memberships(memberships, Some("entity.person"), &conditional_rules);

        // "domain.kyc" is direct → included
        assert!(result.subject_taxonomy_fqns.contains("domain.kyc"));
        // "risk.high" is conditional with attribute_equals → excluded
        assert!(!result.subject_taxonomy_fqns.contains("risk.high"));
    }
}
