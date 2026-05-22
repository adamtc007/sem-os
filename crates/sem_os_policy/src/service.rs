//! CoreService — the central domain service for Semantic OS.
//!
//! Refactored from `rust/src/sem_reg/registry.rs`. Takes port traits via `Arc<dyn PortTrait>`
//! so that the same logic works against Postgres (InProcessClient) or test doubles.
//!
//! `InProcessClient` wraps `Arc<dyn CoreService>` and delegates all calls here.
//! `HttpClient` calls `sem_os_server` over HTTP, which itself holds a CoreService.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    affinity::AffinityGraph,
    authoring::{
        bundle::BundleContents,
        governance_verbs::GovernanceVerbService,
        ports::{AuthoringStore, ScratchSchemaRunner},
        types::{
            ChangeSetFull, DiffSummary, DryRunReport, PublishBatch, PublishPlan, ValidationReport,
        },
    },
    context_resolution::{
        compute_composite_access, compute_confidence, evaluate_policies,
        evaluate_verb_preconditions, filter_and_rank_attributes, filter_and_rank_verbs,
        generate_disambiguation, generate_governance_signals, rank_views_by_overlap,
        ContextResolutionRequest, ContextResolutionResponse, DiscoverySurface, DslCandidate,
        EvidenceSummary, GovernanceSignal, GroundedActionSurface, GroundedTraversalEdge,
        GroundingReadiness, RankedConstellation, RankedConstellationFamily, RankedUniverse,
        RankedUniverseDomain, RankedView, ResolutionStage, ResolvedSubject, SubjectMemberships,
        SubjectRelationships,
    },
    grounding::{compute_slot_action_surface, ConstellationModel},
};
use sem_os_core::{
    error::SemOsError,
    ports::{
        AuditStore, BootstrapAuditStore, ChangesetStore, EvidenceInstanceStore, ObjectStore,
        OutboxStore, ProjectionWriter, SnapshotStore,
    },
    principal::Principal,
    proto::*,
    seeds::SeedBundle,
};
use sem_os_ontology::{
    constellation_family_def::{ConstellationFamilyDefBody, ConstellationRef},
    constellation_map_def::ConstellationMapDefBody,
    state_machine_def::StateMachineDefBody,
    universe_def::{GroundingInput, UniverseDefBody, UniverseDomain},
    view_def::ViewDefBody,
};
use sem_os_types::{ChangeSetStatus, *};

pub type Result<T> = std::result::Result<T, SemOsError>;

// ── CoreService trait ─────────────────────────────────────────

/// The single service interface that `InProcessClient` delegates to.
///
/// All methods take `&Principal` explicitly — no implicit identity, no thread-local context.
/// Implementations are expected to enforce ABAC and publish gates internally.
#[async_trait]
pub trait CoreService: Send + Sync {
    /// Run the 12-step context resolution pipeline.
    async fn resolve_context(
        &self,
        principal: &Principal,
        req: ContextResolutionRequest,
    ) -> Result<ContextResolutionResponse>;

    /// Get the manifest (list of snapshots) for a snapshot set.
    async fn get_manifest(&self, snapshot_set_id: &str) -> Result<GetManifestResponse>;

    /// Export all snapshots in a snapshot set.
    async fn export_snapshot_set(&self, snapshot_set_id: &str)
        -> Result<ExportSnapshotSetResponse>;

    /// Bootstrap seed data (idempotent — skips already-seeded objects).
    async fn bootstrap_seed_bundle(
        &self,
        principal: &Principal,
        bundle: SeedBundle,
    ) -> Result<BootstrapSeedBundleResponse>;

    /// Dispatch a named MCP tool with JSON arguments.
    async fn dispatch_tool(
        &self,
        principal: &Principal,
        req: sem_os_core::proto::ToolCallRequest,
    ) -> Result<sem_os_core::proto::ToolCallResponse>;

    /// List all available tool specifications.
    async fn list_tool_specs(&self) -> Result<sem_os_core::proto::ListToolSpecsResponse>;

    /// Promote an approved changeset — publish all entries as new snapshots.
    /// Returns the number of snapshots created.
    async fn promote_changeset(&self, principal: &Principal, changeset_id: &str) -> Result<u32>;

    /// List changesets with optional filters.
    async fn list_changesets(
        &self,
        query: sem_os_core::proto::ListChangesetsQuery,
    ) -> Result<sem_os_core::proto::ListChangesetsResponse>;

    /// Diff a changeset's entries against current active snapshots.
    async fn changeset_diff(
        &self,
        changeset_id: &str,
    ) -> Result<sem_os_core::proto::ChangesetDiffResponse>;

    /// Impact analysis — find downstream dependents for each modified FQN.
    async fn changeset_impact(
        &self,
        changeset_id: &str,
    ) -> Result<sem_os_core::proto::ChangesetImpactResponse>;

    /// Gate preview — run publish gates against draft entries without persisting.
    async fn changeset_gate_preview(
        &self,
        changeset_id: &str,
    ) -> Result<sem_os_core::proto::GatePreviewResponse>;

    // ── Authoring pipeline (governance verbs) ──────────────────

    /// Propose a new ChangeSet from a parsed bundle.
    /// Content-addressed idempotent: if an active ChangeSet with the same hash
    /// exists, returns the existing one.
    async fn authoring_propose(
        &self,
        principal: &Principal,
        bundle: &BundleContents,
    ) -> Result<ChangeSetFull>;

    /// Run Stage 1 (pure) validation. Transitions Draft → Validated or Rejected.
    async fn authoring_validate(&self, change_set_id: Uuid) -> Result<ValidationReport>;

    /// Run Stage 2 (DB-backed) dry-run. Transitions Validated → DryRunPassed or DryRunFailed.
    async fn authoring_dry_run(&self, change_set_id: Uuid) -> Result<DryRunReport>;

    /// Generate a publish plan with blast-radius analysis. Read-only.
    async fn authoring_plan_publish(&self, change_set_id: Uuid) -> Result<PublishPlan>;

    /// Publish a ChangeSet. Transitions DryRunPassed → Published.
    async fn authoring_publish(&self, change_set_id: Uuid, publisher: &str)
        -> Result<PublishBatch>;

    /// Publish multiple ChangeSets atomically in topological order.
    async fn authoring_publish_batch(
        &self,
        change_set_ids: &[Uuid],
        publisher: &str,
    ) -> Result<PublishBatch>;

    /// Compute structural diff between two ChangeSets.
    async fn authoring_diff(&self, base_id: Uuid, target_id: Uuid) -> Result<DiffSummary>;

    /// List ChangeSets with optional status filter.
    async fn authoring_list(
        &self,
        status: Option<ChangeSetStatus>,
        limit: i64,
    ) -> Result<Vec<ChangeSetFull>>;

    /// Get a single ChangeSet by ID.
    async fn authoring_get(&self, change_set_id: Uuid) -> Result<ChangeSetFull>;

    // ── Authoring health ──────────────────────────────────────────

    /// Health check: pending ChangeSets grouped by status.
    async fn authoring_health_pending(
        &self,
    ) -> Result<crate::authoring::types::PendingChangeSetsHealth>;

    /// Health check: ChangeSets with stale dry-run evaluations.
    async fn authoring_health_stale_dryruns(
        &self,
    ) -> Result<crate::authoring::types::StaleDryRunsHealth>;

    /// Run the cleanup process to archive old terminal/orphan ChangeSets.
    async fn authoring_run_cleanup(
        &self,
        policy: &crate::authoring::cleanup::CleanupPolicy,
    ) -> Result<crate::authoring::cleanup::CleanupReport>;

    // ── Bootstrap audit (idempotent seed bundle tracking) ──────

    /// Check if a bootstrap audit record exists for the given bundle hash.
    async fn bootstrap_check(
        &self,
        bundle_hash: &str,
    ) -> Result<Option<(String, Option<uuid::Uuid>)>>;

    /// Insert or update a bootstrap audit record to 'in_progress'.
    async fn bootstrap_start(
        &self,
        bundle_hash: &str,
        actor_id: &str,
        bundle_counts: serde_json::Value,
    ) -> Result<()>;

    /// Mark a bootstrap audit record as 'published'.
    async fn bootstrap_mark_published(&self, bundle_hash: &str) -> Result<()>;

    /// Mark a bootstrap audit record as 'failed' with an error message.
    async fn bootstrap_mark_failed(&self, bundle_hash: &str, error: &str) -> Result<()>;

    /// Get the pre-computed AffinityGraph (bidirectional verb↔data index).
    ///
    /// Built lazily on first call from active snapshots. Cached until the next publish
    /// (bootstrap or changeset promotion) which sets the cache to `None`.
    /// Returns an empty AffinityGraph if no active snapshots exist yet.
    async fn get_affinity_graph(&self) -> Result<Arc<AffinityGraph>>;

    /// Test-only: synchronously drain and process all pending outbox events.
    async fn drain_outbox_for_test(&self) -> Result<()>;
}

// ── CoreServiceImpl ───────────────────────────────────────────

/// Concrete implementation holding port trait references.
///
/// Constructed at startup in `ob-poc-web/src/main.rs` (in-process mode) or
/// in `sem_os_server/src/main.rs` (standalone server mode).
pub struct CoreServiceImpl {
    pub snapshots: Arc<dyn SnapshotStore>,
    pub objects: Arc<dyn ObjectStore>,
    pub changesets: Arc<dyn ChangesetStore>,
    pub audit: Arc<dyn AuditStore>,
    pub outbox: Arc<dyn OutboxStore>,
    pub evidence: Arc<dyn EvidenceInstanceStore>,
    pub projections: Arc<dyn ProjectionWriter>,
    pub authoring: Option<Arc<dyn AuthoringStore>>,
    pub scratch_runner: Option<Arc<dyn ScratchSchemaRunner>>,
    pub cleanup: Option<Arc<dyn crate::authoring::cleanup::CleanupStore>>,
    pub bootstrap_audit: Option<Arc<dyn BootstrapAuditStore>>,
    /// Cached AffinityGraph. `None` means cache is cold (build on next access).
    pub affinity_graph: Arc<RwLock<Option<AffinityGraph>>>,
}

impl CoreServiceImpl {
    pub fn new(
        snapshots: Arc<dyn SnapshotStore>,
        objects: Arc<dyn ObjectStore>,
        changesets: Arc<dyn ChangesetStore>,
        audit: Arc<dyn AuditStore>,
        outbox: Arc<dyn OutboxStore>,
        evidence: Arc<dyn EvidenceInstanceStore>,
        projections: Arc<dyn ProjectionWriter>,
    ) -> Self {
        Self {
            snapshots,
            objects,
            changesets,
            audit,
            outbox,
            evidence,
            projections,
            authoring: None,
            scratch_runner: None,
            cleanup: None,
            bootstrap_audit: None,
            affinity_graph: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the authoring store (builder pattern).
    pub fn with_authoring(mut self, authoring: Arc<dyn AuthoringStore>) -> Self {
        self.authoring = Some(authoring);
        self
    }

    /// Set the scratch schema runner (builder pattern).
    pub fn with_scratch_runner(mut self, runner: Arc<dyn ScratchSchemaRunner>) -> Self {
        self.scratch_runner = Some(runner);
        self
    }

    /// Set the cleanup store (builder pattern).
    pub fn with_cleanup(
        mut self,
        cleanup: Arc<dyn crate::authoring::cleanup::CleanupStore>,
    ) -> Self {
        self.cleanup = Some(cleanup);
        self
    }

    /// Set the bootstrap audit store (builder pattern).
    pub fn with_bootstrap_audit(mut self, store: Arc<dyn BootstrapAuditStore>) -> Self {
        self.bootstrap_audit = Some(store);
        self
    }

    /// Build a `GovernanceVerbService` from the authoring + scratch_runner stores.
    /// Returns an error if either is not configured.
    fn governance_verb_service(&self) -> Result<(&dyn AuthoringStore, &dyn ScratchSchemaRunner)> {
        let authoring = self
            .authoring
            .as_deref()
            .ok_or_else(|| SemOsError::MigrationPending("authoring store not configured".into()))?;
        let scratch = self
            .scratch_runner
            .as_deref()
            .ok_or_else(|| SemOsError::MigrationPending("scratch runner not configured".into()))?;
        Ok((authoring, scratch))
    }
}

fn render_grounded_dsl(action_id: &str, subject: &crate::context_resolution::SubjectRef) -> String {
    format!("{action_id} :subject {}", subject.id())
}

fn score_slot_against_request(
    slot_name: &str,
    verbs: &[String],
    intent: Option<&str>,
    goals: &[String],
) -> i32 {
    let mut score = 0;
    let mut haystacks = Vec::new();
    if let Some(intent) = intent {
        haystacks.push(intent.to_ascii_lowercase());
    }
    haystacks.extend(goals.iter().map(|goal| goal.to_ascii_lowercase()));
    if haystacks.is_empty() {
        return 0;
    }

    let slot_terms = slot_name
        .split('.')
        .map(|term| term.replace('_', " "))
        .collect::<Vec<_>>();
    for haystack in haystacks {
        for term in &slot_terms {
            if haystack.contains(term) {
                score += 2;
            }
        }
        for verb in verbs {
            let normalized = verb.replace(['.', '-'], " ");
            if haystack.contains(&normalized) {
                score += 4;
            } else {
                for term in normalized.split_whitespace() {
                    if haystack.contains(term) {
                        score += 1;
                    }
                }
            }
        }
    }
    score
}

fn build_grounded_action_surface(
    subject: &crate::context_resolution::SubjectRef,
    req: &ContextResolutionRequest,
    constellation_rows: &[SnapshotRow],
    state_machine_rows: &[SnapshotRow],
) -> Option<GroundedActionSurface> {
    let state_machines = state_machine_rows
        .iter()
        .filter_map(|row| {
            row.parse_definition::<StateMachineDefBody>()
                .ok()
                .map(|body| (body.state_machine.clone(), body))
        })
        .collect::<HashMap<_, _>>();

    let mut best: Option<(i32, GroundedActionSurface)> = None;
    for row in constellation_rows {
        let Ok(map) = row.parse_definition::<ConstellationMapDefBody>() else {
            continue;
        };
        let model = ConstellationModel::from_parts(map.clone(), state_machines.clone());
        for (slot_name, slot) in &model.slots {
            if slot.def.verbs.is_empty() {
                continue;
            }
            let verbs = slot
                .def
                .verbs
                .values()
                .map(|entry| entry.verb_fqn().to_string())
                .collect::<Vec<_>>();
            let score =
                score_slot_against_request(slot_name, &verbs, request_summary(req), &req.goals);
            if score <= 0
                && (request_summary(req).is_some() || request_raw_utterance(req).is_some())
            {
                continue;
            }

            let mut slot_states = HashMap::new();
            if let Some(state_machine) = slot
                .def
                .state_machine
                .as_ref()
                .and_then(|name| model.state_machines.get(name))
            {
                slot_states.insert(slot_name.clone(), state_machine.initial.clone());
            }

            let Ok(surface) = compute_slot_action_surface(&model, &slot_states, slot_name) else {
                continue;
            };
            let dsl_candidates = surface
                .valid_actions
                .iter()
                .map(|action| DslCandidate {
                    action_id: action.action_id.clone(),
                    dsl: render_grounded_dsl(&action.action_id, subject),
                    executable: action.requires_subject,
                })
                .collect::<Vec<_>>();

            let grounded = GroundedActionSurface {
                resolved_subject: subject.clone(),
                resolved_constellation: Some(model.constellation.clone()),
                resolved_slot_path: Some(slot_name.clone()),
                resolved_node_id: Some(format!("{}:{}", model.constellation, slot_name)),
                resolved_state_machine: slot.def.state_machine.clone(),
                current_state: slot_states.get(slot_name).cloned(),
                traversed_edges: build_grounded_traversal_edges(&model, slot_name, subject),
                constraint_signals: surface.constraint_signals,
                valid_actions: surface.valid_actions,
                blocked_actions: surface.blocked_actions,
                dsl_candidates,
            };

            match &best {
                Some((best_score, _)) if score <= *best_score => {}
                _ => best = Some((score, grounded)),
            }
        }
    }

    best.map(|(_, grounded)| grounded)
}

fn build_grounded_traversal_edges(
    model: &ConstellationModel,
    slot_name: &str,
    subject: &crate::context_resolution::SubjectRef,
) -> Vec<GroundedTraversalEdge> {
    let mut edges = Vec::new();
    let node_id = format!("{}:{}", model.constellation, slot_name);

    edges.push(GroundedTraversalEdge {
        from_type: model.constellation.clone(),
        to_type: slot_name.to_string(),
        relationship: "resolved_slot_path".to_string(),
        direction: "parent_to_child".to_string(),
        from_instance: Some(subject.id().to_string()),
        to_instance: Some(node_id.clone()),
    });

    let slot_segments = slot_name.split('.').collect::<Vec<_>>();
    if slot_segments.len() > 1 {
        let mut parent = slot_segments[0].to_string();
        for segment in slot_segments.iter().skip(1) {
            let child = format!("{parent}.{segment}");
            edges.push(GroundedTraversalEdge {
                from_type: parent.clone(),
                to_type: child.clone(),
                relationship: "contains_slot".to_string(),
                direction: "parent_to_child".to_string(),
                from_instance: Some(format!("{}:{}", model.constellation, parent)),
                to_instance: Some(format!("{}:{}", model.constellation, child)),
            });
            parent = child;
        }
    }

    if let Some(slot) = model.slots.get(slot_name) {
        for dependency in &slot.def.depends_on {
            let dependency_slot = dependency.slot_name().to_string();
            edges.push(GroundedTraversalEdge {
                from_type: dependency_slot.clone(),
                to_type: slot_name.to_string(),
                relationship: format!("depends_on[min_state={}]", dependency.min_state()),
                direction: "prerequisite_to_dependent".to_string(),
                from_instance: Some(format!("{}:{}", model.constellation, dependency_slot)),
                to_instance: Some(node_id.clone()),
            });
        }
    }

    edges
}

fn is_discovery_stage(req: &ContextResolutionRequest) -> bool {
    matches!(
        req.subject,
        crate::context_resolution::SubjectRef::TaskId(_)
    ) && req.entity_kind.is_none()
}

fn request_entity_kind(req: &ContextResolutionRequest) -> Option<&str> {
    req.entity_kind.as_deref().or_else(|| {
        req.discovery
            .known_inputs
            .get("entity_kind")
            .map(String::as_str)
    })
}

fn request_summary(req: &ContextResolutionRequest) -> Option<&str> {
    req.intent_summary.as_deref()
}

fn request_raw_utterance(req: &ContextResolutionRequest) -> Option<&str> {
    req.raw_utterance.as_deref()
}

fn combined_request_text(req: &ContextResolutionRequest) -> String {
    let mut parts = Vec::new();

    if let Some(raw) = request_raw_utterance(req)
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        parts.push(raw.to_string());
    }
    if let Some(summary) = request_summary(req)
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        if !parts.iter().any(|part| part == summary) {
            parts.push(summary.to_string());
        }
    }
    if !req.goals.is_empty() {
        let goals = req.goals.join(" ");
        if !goals.trim().is_empty() {
            parts.push(goals);
        }
    }
    for value in req.discovery.known_inputs.values() {
        let value = value.trim();
        if !value.is_empty() {
            parts.push(value.to_string());
        }
    }

    parts.join(" ")
}

fn request_jurisdiction(req: &ContextResolutionRequest) -> Option<&str> {
    req.constraints.jurisdiction.as_deref().or_else(|| {
        req.discovery
            .known_inputs
            .get("jurisdiction")
            .map(String::as_str)
    })
}

fn has_objective(req: &ContextResolutionRequest) -> bool {
    request_summary(req).is_some_and(|value| !value.trim().is_empty())
        || request_raw_utterance(req).is_some_and(|value| !value.trim().is_empty())
        || req
            .discovery
            .known_inputs
            .get("objective")
            .is_some_and(|value| !value.trim().is_empty())
}

fn score_domain(
    domain: &UniverseDomain,
    request_text: &str,
    req: &ContextResolutionRequest,
) -> f64 {
    if let Some(selected_domain_id) = req.discovery.selected_domain_id.as_deref() {
        return if selected_domain_id == domain.domain_id {
            10.0
        } else {
            0.0
        };
    }

    let lowered = request_text.to_lowercase();
    let mut score = 0.0;

    for tag in &domain.objective_tags {
        if lowered.contains(&tag.to_lowercase()) {
            score += 0.45;
        }
    }

    for signal in &domain.utterance_signals {
        if lowered.contains(&signal.pattern.to_lowercase()) {
            score += signal.weight;
        }
    }

    if let Some(entity_kind) = request_entity_kind(req) {
        if domain
            .candidate_entity_kinds
            .iter()
            .any(|kind| kind == entity_kind)
        {
            score += 0.35;
        }
    }

    score
}

fn score_constellation_ref(
    constellation: &ConstellationRef,
    family: &ConstellationFamilyDefBody,
    request_text: &str,
    req: &ContextResolutionRequest,
) -> f64 {
    if let Some(selected_constellation_id) = req.discovery.selected_constellation_id.as_deref() {
        return if selected_constellation_id == constellation.constellation_id {
            10.0
        } else {
            0.0
        };
    }

    let lowered = request_text.to_lowercase();
    let mut score = 0.0;

    if let Some(entity_kind) = request_entity_kind(req) {
        if constellation.entity_kind.as_deref() == Some(entity_kind) {
            score += 0.4;
        }
    }

    if let Some(jurisdiction) = request_jurisdiction(req) {
        if constellation.jurisdiction.as_deref() == Some(jurisdiction) {
            score += 0.4;
        }
    }

    if family
        .candidate_entity_kinds
        .iter()
        .any(|kind| lowered.contains(&kind.to_lowercase()))
    {
        score += 0.1;
    }

    for trigger in &constellation.triggers {
        if lowered.contains(&trigger.to_lowercase()) {
            score += 0.2;
        }
    }

    score
}

fn compute_grounding_readiness(
    family: Option<&ConstellationFamilyDefBody>,
    req: &ContextResolutionRequest,
    matched_constellations: &[RankedConstellation],
) -> GroundingReadiness {
    let Some(family) = family else {
        return GroundingReadiness::NotReady;
    };

    let has_all_required_inputs =
        family
            .grounding_threshold
            .required_input_keys
            .iter()
            .all(|key| match key.as_str() {
                "objective" => has_objective(req),
                "jurisdiction" => request_jurisdiction(req).is_some(),
                "entity_kind" => request_entity_kind(req).is_some(),
                _ => false,
            });

    if matched_constellations.is_empty() {
        GroundingReadiness::FamilyReady
    } else if has_all_required_inputs {
        GroundingReadiness::ConstellationReady
    } else {
        GroundingReadiness::FamilyReady
    }
}

fn build_discovery_surface(
    req: &ContextResolutionRequest,
    universe_rows: &[SnapshotRow],
    family_rows: &[SnapshotRow],
) -> Option<DiscoverySurface> {
    let request_text = combined_request_text(req);

    let universes: Vec<UniverseDefBody> = universe_rows
        .iter()
        .filter_map(|row| row.parse_definition::<UniverseDefBody>().ok())
        .collect();
    let families: Vec<ConstellationFamilyDefBody> = family_rows
        .iter()
        .filter_map(|row| row.parse_definition::<ConstellationFamilyDefBody>().ok())
        .collect();

    if universes.is_empty() && families.is_empty() {
        return None;
    }

    let mut matched_universes = Vec::new();
    let mut matched_domains = Vec::new();
    for universe in &universes {
        let mut best_domain_score: f64 = 0.0;
        for domain in &universe.domains {
            let score = score_domain(domain, &request_text, req);
            best_domain_score = best_domain_score.max(score);
            matched_domains.push(RankedUniverseDomain {
                universe_fqn: universe.fqn.clone(),
                domain_id: domain.domain_id.clone(),
                label: domain.label.clone(),
                score,
            });
        }

        matched_universes.push(RankedUniverse {
            fqn: universe.fqn.clone(),
            universe_id: universe.universe_id.clone(),
            name: universe.name.clone(),
            score: best_domain_score.max(0.1),
        });
    }

    matched_universes.sort_by(|a, b| b.score.total_cmp(&a.score));
    matched_domains.sort_by(|a, b| b.score.total_cmp(&a.score));

    let top_domain = matched_domains
        .first()
        .map(|domain| domain.domain_id.as_str());
    let preferred_family_ids: std::collections::HashSet<String> = matched_domains
        .first()
        .and_then(|top_domain| {
            universes
                .iter()
                .flat_map(|universe| universe.domains.iter())
                .find(|domain| domain.domain_id == top_domain.domain_id)
        })
        .map(|domain| domain.candidate_family_ids.iter().cloned().collect())
        .unwrap_or_default();
    let lowered_request = request_text.to_lowercase();
    let mut matched_families = Vec::new();
    let mut matched_constellations = Vec::new();

    for family in &families {
        let mut score = 0.0;
        if top_domain == Some(family.domain_id.as_str()) {
            score += 0.5;
            if preferred_family_ids.contains(&family.family_id) {
                score += 0.6;
            } else if !preferred_family_ids.is_empty() {
                score -= 0.15;
            }
        }
        if let Some(selected_family_id) = req.discovery.selected_family_id.as_deref() {
            if selected_family_id == family.family_id {
                score += 10.0;
            } else {
                score = 0.0;
            }
        }
        if let Some(entity_kind) = request_entity_kind(req) {
            if family
                .candidate_entity_kinds
                .iter()
                .any(|kind| kind == entity_kind)
            {
                score += 0.35;
            }
        }
        if let Some(jurisdiction) = request_jurisdiction(req) {
            if family
                .candidate_jurisdictions
                .iter()
                .any(|value| value == jurisdiction)
            {
                score += 0.35;
            }
        }
        if lowered_request.contains(&family.label.to_lowercase()) {
            score += 0.25;
        }

        matched_families.push(RankedConstellationFamily {
            fqn: family.fqn.clone(),
            family_id: family.family_id.clone(),
            label: family.label.clone(),
            domain_id: family.domain_id.clone(),
            score,
            grounding_threshold: family.grounding_threshold.clone(),
        });

        for constellation in &family.constellation_refs {
            matched_constellations.push(RankedConstellation {
                family_fqn: family.fqn.clone(),
                constellation_id: constellation.constellation_id.clone(),
                label: constellation.label.clone(),
                score: score + score_constellation_ref(constellation, family, &request_text, req),
            });
        }
    }

    matched_families.sort_by(|a, b| b.score.total_cmp(&a.score));
    matched_constellations.sort_by(|a, b| b.score.total_cmp(&a.score));
    matched_constellations.truncate(8);

    let top_family = matched_families
        .first()
        .and_then(|family_rank| families.iter().find(|family| family.fqn == family_rank.fqn));

    let missing_inputs = top_family
        .map(|family| {
            family
                .grounding_threshold
                .required_input_keys
                .iter()
                .filter_map(|key| match key.as_str() {
                    "objective" if !has_objective(req) => Some(GroundingInput {
                        key: "objective".to_string(),
                        label: "Objective".to_string(),
                        required: true,
                        input_type: "string".to_string(),
                    }),
                    "jurisdiction" if request_jurisdiction(req).is_none() => Some(GroundingInput {
                        key: "jurisdiction".to_string(),
                        label: "Jurisdiction".to_string(),
                        required: true,
                        input_type: "string".to_string(),
                    }),
                    "entity_kind" if request_entity_kind(req).is_none() => Some(GroundingInput {
                        key: "entity_kind".to_string(),
                        label: "Entity kind".to_string(),
                        required: true,
                        input_type: "string".to_string(),
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let entry_questions = matched_domains
        .first()
        .and_then(|top_domain| {
            universes
                .iter()
                .flat_map(|universe| universe.domains.iter())
                .find(|domain| domain.domain_id == top_domain.domain_id)
                .map(|domain| domain.entry_questions.clone())
        })
        .unwrap_or_default();

    let readiness = compute_grounding_readiness(top_family, req, &matched_constellations);

    Some(DiscoverySurface {
        matched_universes,
        matched_domains,
        matched_families,
        matched_constellations,
        missing_inputs,
        entry_questions,
        grounding_readiness: readiness,
    })
}

#[derive(Debug)]
enum SeedBootstrapPlan {
    Skip,
    Publish(Box<SnapshotMeta>, serde_json::Value),
}

fn plan_seed_bootstrap_item(
    principal: &Principal,
    bundle_hash: &str,
    object_type: ObjectType,
    fqn: &str,
    payload: &serde_json::Value,
    current: Option<&SnapshotRow>,
) -> Result<SeedBootstrapPlan> {
    match current {
        Some(current) => {
            if current.object_type != object_type {
                return Err(SemOsError::Conflict(format!(
                    "FQN {} is already active as {:?}, cannot bootstrap as {:?}",
                    fqn, current.object_type, object_type
                )));
            }

            if current.definition == *payload {
                return Ok(SeedBootstrapPlan::Skip);
            }

            Ok(SeedBootstrapPlan::Publish(
                Box::new(SnapshotMeta {
                    object_type,
                    object_id: current.object_id,
                    version_major: current.version_major,
                    version_minor: current.version_minor + 1,
                    status: SnapshotStatus::Active,
                    governance_tier: GovernanceTier::Operational,
                    trust_class: TrustClass::Convenience,
                    security_label: SecurityLabel::default(),
                    change_type: ChangeType::NonBreaking,
                    change_rationale: Some(format!("Refreshed from seed bundle {}", bundle_hash)),
                    created_by: principal.actor_id.clone(),
                    approved_by: Some(principal.actor_id.clone()),
                    predecessor_id: Some(current.snapshot_id),
                }),
                payload.clone(),
            ))
        }
        None => {
            let object_id = sem_os_core::ids::object_id_for(object_type, fqn);
            Ok(SeedBootstrapPlan::Publish(
                Box::new(SnapshotMeta::new_operational(
                    object_type,
                    object_id,
                    &principal.actor_id,
                )),
                payload.clone(),
            ))
        }
    }
}

#[async_trait]
impl CoreService for CoreServiceImpl {
    async fn resolve_context(
        &self,
        _principal: &Principal,
        req: ContextResolutionRequest,
    ) -> Result<ContextResolutionResponse> {
        let active = self.snapshots.load_active_snapshots().await?;
        let mut view_rows = Vec::new();
        let mut verb_rows = Vec::new();
        let mut attr_rows = Vec::new();
        let mut policy_rows = Vec::new();
        let mut universe_rows = Vec::new();
        let mut family_rows = Vec::new();
        let mut constellation_rows = Vec::new();
        let mut state_machine_rows = Vec::new();

        for row in active {
            match row.object_type {
                ObjectType::ViewDef => view_rows.push(row),
                ObjectType::VerbContract => verb_rows.push(row),
                ObjectType::AttributeDef => attr_rows.push(row),
                ObjectType::PolicyRule => policy_rows.push(row),
                ObjectType::UniverseDef => universe_rows.push(row),
                ObjectType::ConstellationFamilyDef => family_rows.push(row),
                ObjectType::ConstellationMap => constellation_rows.push(row),
                ObjectType::StateMachine => state_machine_rows.push(row),
                _ => {}
            }
        }

        let views = view_rows
            .iter()
            .filter_map(|row| {
                row.parse_definition::<ViewDefBody>()
                    .ok()
                    .map(|body| (row.clone(), body))
            })
            .collect::<Vec<_>>();

        let memberships = SubjectMemberships::default();
        let relationships = SubjectRelationships::default();
        let resolved_subject = ResolvedSubject {
            entity_type_fqn: req.entity_kind.clone(),
            jurisdiction: req
                .constraints
                .jurisdiction
                .clone()
                .or_else(|| req.actor.jurisdictions.first().cloned()),
            state: None,
        };

        let mut applicable_views = rank_views_by_overlap(&views, &resolved_subject, &memberships);
        applicable_views.sort_by(|a, b| {
            b.overlap_score
                .partial_cmp(&a.overlap_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let top_view = applicable_views.first().map(|view| &view.body);
        let (mut candidate_verbs, entity_kind_pruned_verbs) = filter_and_rank_verbs(
            &verb_rows,
            crate::context_resolution::VerbFilterContext {
                actor: &req.actor,
                mode: req.evidence_mode,
                top_view,
                entity_kind: req.entity_kind.as_deref(),
                entity_confidence: req.entity_confidence,
                memberships: &memberships,
                relationships: &relationships,
            },
        );
        candidate_verbs.sort_by(|a, b| {
            b.rank_score
                .partial_cmp(&a.rank_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidate_verbs.truncate(2048);

        let mut candidate_attributes = filter_and_rank_attributes(
            &attr_rows,
            &req.actor,
            req.evidence_mode,
            top_view,
            &memberships,
        );
        candidate_attributes.sort_by(|a, b| {
            b.rank_score
                .partial_cmp(&a.rank_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidate_attributes.truncate(2048);

        let required_preconditions = evaluate_verb_preconditions(&candidate_verbs);
        let policy_verdicts = evaluate_policies(&policy_rows, &candidate_verbs, &req.actor);
        let security_handling = compute_composite_access(&candidate_verbs, &policy_verdicts);
        let governance_signals: Vec<GovernanceSignal> =
            generate_governance_signals(&candidate_verbs, &candidate_attributes, req.evidence_mode);
        let confidence = compute_confidence(
            &applicable_views,
            &candidate_verbs,
            &required_preconditions,
            &candidate_attributes,
        );
        let grounded_action_surface = build_grounded_action_surface(
            &req.subject,
            &req,
            &constellation_rows,
            &state_machine_rows,
        );
        let discovery_surface = if is_discovery_stage(&req) {
            build_discovery_surface(&req, &universe_rows, &family_rows)
        } else {
            None
        };
        let resolution_stage = if discovery_surface.is_some() {
            ResolutionStage::Discovery
        } else {
            ResolutionStage::Grounded
        };
        let candidate_verbs = if discovery_surface.is_some() {
            Vec::new()
        } else {
            candidate_verbs
        };
        let candidate_attributes = if discovery_surface.is_some() {
            Vec::new()
        } else {
            candidate_attributes
        };
        let required_preconditions = if discovery_surface.is_some() {
            Vec::new()
        } else {
            required_preconditions
        };
        let policy_verdicts = if discovery_surface.is_some() {
            Vec::new()
        } else {
            policy_verdicts
        };
        let grounded_action_surface = if discovery_surface.is_some() {
            None
        } else {
            grounded_action_surface
        };

        Ok(ContextResolutionResponse {
            as_of_time: req.point_in_time.unwrap_or_else(Utc::now),
            resolved_at: Utc::now(),
            applicable_views,
            candidate_verbs,
            candidate_attributes,
            required_preconditions,
            disambiguation_questions: generate_disambiguation(
                &views
                    .iter()
                    .map(|(row, body)| RankedView {
                        view_snapshot_id: row.snapshot_id,
                        view_id: row.object_id,
                        fqn: body.fqn.clone(),
                        name: body.name.clone(),
                        overlap_score: 0.0,
                        body: body.clone(),
                    })
                    .collect::<Vec<_>>(),
                &[],
            ),
            evidence: EvidenceSummary::default(),
            policy_verdicts,
            security_handling,
            governance_signals,
            entity_kind_pruned_verbs,
            confidence,
            grounded_action_surface,
            resolution_stage,
            discovery_surface,
        })
    }

    async fn get_manifest(&self, snapshot_set_id: &str) -> Result<GetManifestResponse> {
        let set_id = parse_uuid(snapshot_set_id, "snapshot_set_id")?;
        let manifest = self.snapshots.get_manifest(&SnapshotSetId(set_id)).await?;
        Ok(GetManifestResponse {
            snapshot_set_id: manifest.snapshot_set_id.0.to_string(),
            published_at: manifest.published_at.to_rfc3339(),
            entries: manifest
                .entries
                .into_iter()
                .map(|e| ManifestEntry {
                    snapshot_id: e.snapshot_id.0.to_string(),
                    object_type: e.object_type,
                    fqn: e.fqn.0,
                    content_hash: e.content_hash,
                })
                .collect(),
        })
    }

    async fn export_snapshot_set(
        &self,
        snapshot_set_id: &str,
    ) -> Result<ExportSnapshotSetResponse> {
        let set_id = parse_uuid(snapshot_set_id, "snapshot_set_id")?;
        let exports = self.snapshots.export(&SnapshotSetId(set_id)).await?;
        Ok(ExportSnapshotSetResponse {
            snapshots: exports
                .into_iter()
                .map(|e| ExportedSnapshot {
                    snapshot_id: e.snapshot_id.0.to_string(),
                    fqn: e.fqn.0,
                    object_type: e.object_type,
                    payload: e.payload,
                })
                .collect(),
        })
    }

    async fn bootstrap_seed_bundle(
        &self,
        principal: &Principal,
        bundle: SeedBundle,
    ) -> Result<BootstrapSeedBundleResponse> {
        // v3.3 compliant bootstrap:
        // 1. Collect all missing seeds into a Vec<(SnapshotMeta, Value)>.
        // 2. Publish the entire batch as one snapshot set with one outbox event.
        let mut to_publish: Vec<(SnapshotMeta, serde_json::Value)> = Vec::new();
        let mut skipped = 0u32;

        // Helper: check if FQN already exists, if not queue for batch publish.
        let all_seeds: Vec<(ObjectType, &str, &serde_json::Value)> = bundle
            .verb_contracts
            .iter()
            .map(|s| (ObjectType::VerbContract, s.fqn.as_str(), &s.payload))
            .chain(
                bundle
                    .macro_defs
                    .iter()
                    .map(|s| (ObjectType::MacroDef, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .universes
                    .iter()
                    .map(|s| (ObjectType::UniverseDef, s.fqn.as_str(), &s.payload)),
            )
            .chain(bundle.constellation_families.iter().map(|s| {
                (
                    ObjectType::ConstellationFamilyDef,
                    s.fqn.as_str(),
                    &s.payload,
                )
            }))
            .chain(
                bundle
                    .constellation_maps
                    .iter()
                    .map(|s| (ObjectType::ConstellationMap, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .state_machines
                    .iter()
                    .map(|s| (ObjectType::StateMachine, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .state_graphs
                    .iter()
                    .map(|s| (ObjectType::StateGraph, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .dag_taxonomies
                    .iter()
                    .map(|s| (ObjectType::DagTaxonomy, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .domain_packs
                    .iter()
                    .map(|s| (ObjectType::DomainPack, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .attributes
                    .iter()
                    .map(|s| (ObjectType::AttributeDef, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .entity_types
                    .iter()
                    .map(|s| (ObjectType::EntityTypeDef, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .taxonomies
                    .iter()
                    .map(|s| (ObjectType::TaxonomyDef, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .policies
                    .iter()
                    .map(|s| (ObjectType::PolicyRule, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .views
                    .iter()
                    .map(|s| (ObjectType::ViewDef, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .derivation_specs
                    .iter()
                    .map(|s| (ObjectType::DerivationSpec, s.fqn.as_str(), &s.payload)),
            )
            .chain(bundle.requirement_profiles.iter().map(|s| {
                (
                    ObjectType::RequirementProfileDef,
                    s.fqn.as_str(),
                    &s.payload,
                )
            }))
            .chain(
                bundle
                    .proof_obligations
                    .iter()
                    .map(|s| (ObjectType::ProofObligationDef, s.fqn.as_str(), &s.payload)),
            )
            .chain(
                bundle
                    .evidence_strategies
                    .iter()
                    .map(|s| (ObjectType::EvidenceStrategyDef, s.fqn.as_str(), &s.payload)),
            )
            .collect();

        for (object_type, fqn, payload) in &all_seeds {
            let fqn_obj = Fqn::new(*fqn);
            let current = match self.snapshots.resolve(&fqn_obj, None).await {
                Ok(current) => Some(current),
                Err(SemOsError::NotFound(_)) => None,
                Err(e) => return Err(e),
            };

            match plan_seed_bootstrap_item(
                principal,
                &bundle.bundle_hash,
                *object_type,
                fqn,
                payload,
                current.as_ref(),
            )? {
                SeedBootstrapPlan::Skip => skipped += 1,
                SeedBootstrapPlan::Publish(meta, payload) => to_publish.push((*meta, payload)),
            }
        }

        let created = to_publish.len() as u32;
        let mut snapshot_set_id_str: Option<String> = None;

        if !to_publish.is_empty() {
            // Create a single snapshot set for the entire bootstrap batch.
            let set_id = self
                .snapshots
                .publish(
                    principal,
                    PublishInput {
                        payload: serde_json::json!({
                            "source": "bootstrap_seed_bundle",
                            "bundle_hash": &bundle.bundle_hash,
                        }),
                    },
                )
                .await?;
            let correlation_id = Uuid::new_v4();

            // Atomic batch publish: all snapshots + exactly one outbox event.
            self.snapshots
                .publish_batch_into_set(to_publish, set_id.0, correlation_id)
                .await?;

            // Invalidate AffinityGraph cache — new snapshots change the verb↔data index.
            {
                let mut guard = self.affinity_graph.write().await;
                *guard = None;
            }

            snapshot_set_id_str = Some(set_id.0.to_string());
        }

        // Audit the bootstrap
        self.audit
            .append(
                principal,
                AuditEntry {
                    action: "bootstrap_seed_bundle".into(),
                    details: serde_json::json!({
                        "bundle_hash": bundle.bundle_hash,
                        "created": created,
                        "skipped": skipped,
                        "snapshot_set_id": snapshot_set_id_str,
                    }),
                },
            )
            .await?;

        Ok(BootstrapSeedBundleResponse {
            created,
            skipped,
            bundle_hash: bundle.bundle_hash,
            snapshot_set_id: snapshot_set_id_str,
        })
    }

    async fn dispatch_tool(
        &self,
        _principal: &Principal,
        _req: sem_os_core::proto::ToolCallRequest,
    ) -> Result<sem_os_core::proto::ToolCallResponse> {
        // Tool dispatch requires the ob-poc host's sem_reg::agent::mcp_tools.
        // InProcessClient overrides this; HttpClient calls the server endpoint.
        Err(SemOsError::MigrationPending(
            "tool dispatch requires ob-poc host (InProcessClient overrides this method)".into(),
        ))
    }

    async fn list_tool_specs(&self) -> Result<sem_os_core::proto::ListToolSpecsResponse> {
        // Tool specs are provided by ob-poc's all_tool_specs().
        // InProcessClient overrides this; HttpClient calls the server endpoint.
        Err(SemOsError::MigrationPending(
            "tool specs require ob-poc host (InProcessClient overrides this method)".into(),
        ))
    }

    async fn promote_changeset(&self, principal: &Principal, changeset_id: &str) -> Result<u32> {
        let cs_id = parse_uuid(changeset_id, "changeset_id")?;

        // 1. Load changeset and verify status is 'approved'.
        let changeset = self.changesets.get_changeset(cs_id).await?;
        if changeset.status != ChangesetStatus::Approved {
            return Err(SemOsError::Conflict(format!(
                "changeset {} is in '{}' status — only 'approved' changesets can be promoted",
                cs_id, changeset.status
            )));
        }

        // 2. Load all entries.
        let entries = self.changesets.list_entries(cs_id).await?;
        if entries.is_empty() {
            return Err(SemOsError::InvalidInput(format!(
                "changeset {} has no entries to promote",
                cs_id
            )));
        }

        // 2b. Stewardship guardrails: role constraints + proof chain.
        crate::stewardship::validate_role_constraints(principal, &entries)?;
        crate::stewardship::check_proof_chain_compatibility(&entries, self.snapshots.as_ref())
            .await?;

        // 3. Stale detection + predecessor resolution: for each entry with a
        //    base_snapshot_id, verify it still matches the current active snapshot.
        //    Collect resolved predecessors for version derivation in step 5.
        let mut predecessors: std::collections::HashMap<Uuid, SnapshotRow> =
            std::collections::HashMap::new();
        for entry in &entries {
            if let Some(base_id) = entry.base_snapshot_id {
                let fqn = Fqn::new(&entry.object_fqn);
                match self.snapshots.resolve(&fqn, None).await {
                    Ok(current) => {
                        if current.snapshot_id != base_id {
                            return Err(SemOsError::Conflict(format!(
                                "stale draft: FQN {} — base_snapshot_id {} does not match \
                                 current active snapshot {}",
                                entry.object_fqn, base_id, current.snapshot_id
                            )));
                        }
                        predecessors.insert(entry.entry_id, current);
                    }
                    Err(SemOsError::NotFound(_)) => {
                        return Err(SemOsError::Conflict(format!(
                            "stale draft: FQN {} — base_snapshot_id {} no longer has an \
                             active snapshot",
                            entry.object_fqn, base_id
                        )));
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        // 4. Create a snapshot set for this promotion.
        let set_id = self
            .snapshots
            .publish(
                principal,
                PublishInput {
                    payload: serde_json::json!({
                        "promotion_source": "changeset",
                        "changeset_id": cs_id,
                    }),
                },
            )
            .await?;
        let correlation_id = Uuid::new_v4();

        // 5. Build batch of snapshots for atomic publish (v3.3: one outbox event).
        let mut items: Vec<(SnapshotMeta, serde_json::Value)> = Vec::with_capacity(entries.len());
        for entry in &entries {
            let object_type = entry.object_type;

            let object_id = sem_os_core::ids::object_id_for(object_type, &entry.object_fqn);

            let change_type = match entry.change_kind {
                ChangeKind::Add => ChangeType::Created,
                ChangeKind::Modify => ChangeType::NonBreaking,
                ChangeKind::Remove => ChangeType::Retirement,
            };

            let status = match entry.change_kind {
                ChangeKind::Remove => SnapshotStatus::Retired,
                _ => SnapshotStatus::Active,
            };

            // Derive version from predecessor (resolved in step 3).
            let (version_major, version_minor) =
                if let Some(pred) = predecessors.get(&entry.entry_id) {
                    match entry.change_kind {
                        ChangeKind::Add => (1, 0),
                        ChangeKind::Modify => (pred.version_major, pred.version_minor + 1),
                        ChangeKind::Remove => (pred.version_major, pred.version_minor),
                    }
                } else {
                    (1, 0)
                };

            let meta = SnapshotMeta {
                object_type,
                object_id,
                version_major,
                version_minor,
                status,
                governance_tier: GovernanceTier::Operational,
                trust_class: TrustClass::Convenience,
                security_label: SecurityLabel::default(),
                change_type,
                change_rationale: Some(format!("Promoted from changeset {}", cs_id)),
                created_by: principal.actor_id.clone(),
                approved_by: Some(principal.actor_id.clone()),
                predecessor_id: entry.base_snapshot_id,
            };

            items.push((meta, entry.draft_payload.clone()));
        }

        let created = items.len() as u32;
        self.snapshots
            .publish_batch_into_set(items, set_id.0, correlation_id)
            .await?;

        // Invalidate AffinityGraph cache — new snapshots change the verb↔data index.
        {
            let mut guard = self.affinity_graph.write().await;
            *guard = None;
        }

        // 6. Update changeset status to 'published'.
        self.changesets
            .update_status(cs_id, ChangesetStatus::Published)
            .await?;

        // 7. Audit the promotion.
        self.audit
            .append(
                principal,
                AuditEntry {
                    action: "promote_changeset".into(),
                    details: serde_json::json!({
                        "changeset_id": cs_id,
                        "snapshot_set_id": set_id.0,
                        "snapshots_created": created,
                    }),
                },
            )
            .await?;

        Ok(created)
    }

    async fn list_changesets(
        &self,
        query: sem_os_core::proto::ListChangesetsQuery,
    ) -> Result<sem_os_core::proto::ListChangesetsResponse> {
        let changesets = self
            .changesets
            .list_changesets(
                query.status.as_deref(),
                query.owner.as_deref(),
                query.scope.as_deref(),
            )
            .await?;
        Ok(sem_os_core::proto::ListChangesetsResponse { changesets })
    }

    async fn changeset_diff(
        &self,
        changeset_id: &str,
    ) -> Result<sem_os_core::proto::ChangesetDiffResponse> {
        let cs_id = parse_uuid(changeset_id, "changeset_id")?;
        let changeset = self.changesets.get_changeset(cs_id).await?;
        let entries = self.changesets.list_entries(cs_id).await?;

        let mut diff_entries = Vec::with_capacity(entries.len());
        let mut stale_count = 0u32;

        for entry in &entries {
            let fqn = Fqn::new(&entry.object_fqn);
            let current = self.snapshots.resolve(&fqn, None).await.ok();

            let current_snapshot_id = current.as_ref().map(|r| r.snapshot_id.to_string());
            let current_payload = current.as_ref().map(|r| r.definition.clone());

            let is_stale = match (entry.base_snapshot_id, current.as_ref()) {
                (Some(base_id), Some(curr)) => base_id != curr.snapshot_id,
                (Some(_), None) => true, // base existed but object was removed
                (None, _) => false,      // new addition — never stale
            };

            if is_stale {
                stale_count += 1;
            }

            diff_entries.push(sem_os_core::proto::DiffEntry {
                entry_id: entry.entry_id.to_string(),
                object_fqn: entry.object_fqn.clone(),
                object_type: entry.object_type,
                change_kind: entry.change_kind.to_string(),
                base_snapshot_id: entry.base_snapshot_id.map(|id| id.to_string()),
                current_snapshot_id,
                is_stale,
                draft_payload: entry.draft_payload.clone(),
                current_payload,
            });
        }

        Ok(sem_os_core::proto::ChangesetDiffResponse {
            changeset_id: cs_id.to_string(),
            status: changeset.status.to_string(),
            entries: diff_entries,
            stale_count,
        })
    }

    async fn changeset_impact(
        &self,
        changeset_id: &str,
    ) -> Result<sem_os_core::proto::ChangesetImpactResponse> {
        let cs_id = parse_uuid(changeset_id, "changeset_id")?;
        let _changeset = self.changesets.get_changeset(cs_id).await?;
        let entries = self.changesets.list_entries(cs_id).await?;

        // Real JSONB dependency traversal: for each modified FQN, search active
        // snapshots whose definition JSON references that FQN.
        let mut impacts = Vec::new();

        for entry in &entries {
            let source_fqn = &entry.object_fqn;
            let source_type = &entry.object_type;

            let dependents = self
                .snapshots
                .find_dependents(source_fqn, 100)
                .await
                .unwrap_or_default();

            for dep in dependents {
                let relationship = match dep.object_type {
                    ObjectType::ViewDef => "surfaces",
                    ObjectType::PolicyRule => "governed_by",
                    ObjectType::VerbContract => "references",
                    ObjectType::EntityTypeDef => "uses_attribute",
                    ObjectType::TaxonomyNode => "member_of",
                    _ => "depends_on",
                };

                impacts.push(sem_os_core::proto::ImpactEntry {
                    source_fqn: source_fqn.clone(),
                    source_object_type: *source_type,
                    dependent_fqn: dep.fqn,
                    dependent_object_type: dep.object_type,
                    relationship: relationship.into(),
                });
            }
        }

        Ok(sem_os_core::proto::ChangesetImpactResponse {
            changeset_id: cs_id.to_string(),
            impacts,
        })
    }

    async fn changeset_gate_preview(
        &self,
        changeset_id: &str,
    ) -> Result<sem_os_core::proto::GatePreviewResponse> {
        let cs_id = parse_uuid(changeset_id, "changeset_id")?;
        let _changeset = self.changesets.get_changeset(cs_id).await?;
        let entries = self.changesets.list_entries(cs_id).await?;

        let mut gate_results = Vec::new();
        let mut total_errors = 0u32;
        let mut total_warnings = 0u32;

        // Stewardship: proof chain compatibility check
        if let Err(e) =
            crate::stewardship::check_proof_chain_compatibility(&entries, self.snapshots.as_ref())
                .await
        {
            gate_results.push(sem_os_core::proto::GatePreviewEntry {
                entry_id: String::new(),
                object_fqn: String::new(),
                gate_name: "proof_chain_compatibility".into(),
                severity: "error".into(),
                passed: false,
                reason: Some(e.to_string()),
            });
            total_errors += 1;
        }

        // Stewardship: stale draft detection
        if let Ok(stale) =
            crate::stewardship::detect_stale_drafts(&entries, self.snapshots.as_ref()).await
        {
            for conflict in &stale {
                gate_results.push(sem_os_core::proto::GatePreviewEntry {
                    entry_id: conflict.entry_id.to_string(),
                    object_fqn: conflict.object_fqn.clone(),
                    gate_name: "stale_draft_detection".into(),
                    severity: "warning".into(),
                    passed: false,
                    reason: Some(format!(
                        "base_snapshot_id {} does not match current active {}",
                        conflict.base_snapshot_id, conflict.current_snapshot_id,
                    )),
                });
                total_warnings += 1;
            }
        }

        for entry in &entries {
            let object_type = entry.object_type;

            let object_id = sem_os_core::ids::object_id_for(object_type, &entry.object_fqn);

            // Build SnapshotMeta from the draft entry
            let meta = SnapshotMeta {
                object_type,
                object_id,
                version_major: 1,
                version_minor: 0,
                status: match entry.change_kind {
                    ChangeKind::Remove => SnapshotStatus::Retired,
                    _ => SnapshotStatus::Active,
                },
                governance_tier: GovernanceTier::Operational,
                trust_class: TrustClass::Convenience,
                security_label: SecurityLabel::default(),
                change_type: match entry.change_kind {
                    ChangeKind::Add => ChangeType::Created,
                    ChangeKind::Modify => ChangeType::NonBreaking,
                    ChangeKind::Remove => ChangeType::Retirement,
                },
                change_rationale: Some(format!("Gate preview for changeset {}", cs_id)),
                created_by: "gate_preview".into(),
                approved_by: Some("gate_preview".into()),
                predecessor_id: entry.base_snapshot_id,
            };

            // Load predecessor if it exists
            let predecessor = if entry.base_snapshot_id.is_some() {
                let fqn = Fqn::new(&entry.object_fqn);
                self.snapshots.resolve(&fqn, None).await.ok()
            } else {
                None
            };

            // Run the simple 4-gate pipeline
            let gate_result = crate::gates::evaluate_publish_gates(&meta, predecessor.as_ref());

            for gr in &gate_result.results {
                if !gr.passed {
                    total_errors += 1;
                }
                gate_results.push(sem_os_core::proto::GatePreviewEntry {
                    entry_id: entry.entry_id.to_string(),
                    object_fqn: entry.object_fqn.clone(),
                    gate_name: gr.gate_name.to_string(),
                    severity: if gr.passed {
                        "info".into()
                    } else {
                        "error".into()
                    },
                    passed: gr.passed,
                    reason: gr.reason.clone(),
                });
            }
        }

        Ok(sem_os_core::proto::GatePreviewResponse {
            changeset_id: cs_id.to_string(),
            would_block: total_errors > 0,
            error_count: total_errors,
            warning_count: total_warnings,
            gate_results,
        })
    }

    // ── Authoring pipeline implementations ─────────────────────

    async fn authoring_propose(
        &self,
        principal: &Principal,
        bundle: &BundleContents,
    ) -> Result<ChangeSetFull> {
        let (authoring, scratch) = self.governance_verb_service()?;
        let svc = GovernanceVerbService::new(authoring, scratch);
        svc.propose(bundle, principal).await
    }

    async fn authoring_validate(&self, change_set_id: Uuid) -> Result<ValidationReport> {
        let (authoring, scratch) = self.governance_verb_service()?;
        let svc = GovernanceVerbService::new(authoring, scratch);
        svc.validate(change_set_id).await
    }

    async fn authoring_dry_run(&self, change_set_id: Uuid) -> Result<DryRunReport> {
        let (authoring, scratch) = self.governance_verb_service()?;
        let svc = GovernanceVerbService::new(authoring, scratch);
        svc.dry_run(change_set_id).await
    }

    async fn authoring_plan_publish(&self, change_set_id: Uuid) -> Result<PublishPlan> {
        let (authoring, scratch) = self.governance_verb_service()?;
        let svc = GovernanceVerbService::new(authoring, scratch);
        svc.plan_publish(change_set_id).await
    }

    async fn authoring_publish(
        &self,
        change_set_id: Uuid,
        publisher: &str,
    ) -> Result<PublishBatch> {
        let (authoring, scratch) = self.governance_verb_service()?;
        let svc = GovernanceVerbService::new(authoring, scratch);
        svc.publish(change_set_id, publisher).await
    }

    async fn authoring_publish_batch(
        &self,
        change_set_ids: &[Uuid],
        publisher: &str,
    ) -> Result<PublishBatch> {
        let (authoring, scratch) = self.governance_verb_service()?;
        let svc = GovernanceVerbService::new(authoring, scratch);
        svc.publish_batch(change_set_ids, publisher).await
    }

    async fn authoring_diff(&self, base_id: Uuid, target_id: Uuid) -> Result<DiffSummary> {
        let (authoring, scratch) = self.governance_verb_service()?;
        let svc = GovernanceVerbService::new(authoring, scratch);
        svc.diff(base_id, target_id).await
    }

    async fn authoring_list(
        &self,
        status: Option<ChangeSetStatus>,
        limit: i64,
    ) -> Result<Vec<ChangeSetFull>> {
        let authoring = self
            .authoring
            .as_deref()
            .ok_or_else(|| SemOsError::MigrationPending("authoring store not configured".into()))?;
        authoring.list_change_sets(status, limit).await
    }

    async fn authoring_get(&self, change_set_id: Uuid) -> Result<ChangeSetFull> {
        let authoring = self
            .authoring
            .as_deref()
            .ok_or_else(|| SemOsError::MigrationPending("authoring store not configured".into()))?;
        authoring.get_change_set(change_set_id).await
    }

    async fn authoring_health_pending(
        &self,
    ) -> Result<crate::authoring::types::PendingChangeSetsHealth> {
        let authoring = self
            .authoring
            .as_deref()
            .ok_or_else(|| SemOsError::MigrationPending("authoring store not configured".into()))?;
        let status_counts = authoring.count_by_status().await?;
        let mut counts = Vec::new();
        let mut total_pending: i64 = 0;
        for (status, count) in &status_counts {
            counts.push(crate::authoring::types::StatusCount {
                status: status.to_string(),
                count: *count,
            });
            if !status.is_terminal() {
                total_pending += count;
            }
        }
        Ok(crate::authoring::types::PendingChangeSetsHealth {
            counts,
            total_pending,
        })
    }

    async fn authoring_health_stale_dryruns(
        &self,
    ) -> Result<crate::authoring::types::StaleDryRunsHealth> {
        let authoring = self
            .authoring
            .as_deref()
            .ok_or_else(|| SemOsError::MigrationPending("authoring store not configured".into()))?;
        let stale = authoring.find_stale_dry_runs().await?;
        let stale_change_set_ids: Vec<Uuid> = stale.iter().map(|cs| cs.change_set_id).collect();
        Ok(crate::authoring::types::StaleDryRunsHealth {
            stale_count: stale_change_set_ids.len() as i64,
            stale_change_set_ids,
        })
    }

    async fn authoring_run_cleanup(
        &self,
        policy: &crate::authoring::cleanup::CleanupPolicy,
    ) -> Result<crate::authoring::cleanup::CleanupReport> {
        let cleanup = self
            .cleanup
            .as_ref()
            .ok_or_else(|| SemOsError::MigrationPending("cleanup store not configured".into()))?;
        crate::authoring::cleanup::run_cleanup(cleanup.as_ref(), policy)
            .await
            .map_err(|e| SemOsError::Internal(anyhow::anyhow!("{e}")))
    }

    // ── Bootstrap audit implementations ────────────────────────

    async fn bootstrap_check(
        &self,
        bundle_hash: &str,
    ) -> Result<Option<(String, Option<uuid::Uuid>)>> {
        let store = self.bootstrap_audit.as_deref().ok_or_else(|| {
            SemOsError::MigrationPending("bootstrap audit store not configured".into())
        })?;
        store.check_bootstrap(bundle_hash).await
    }

    async fn bootstrap_start(
        &self,
        bundle_hash: &str,
        actor_id: &str,
        bundle_counts: serde_json::Value,
    ) -> Result<()> {
        let store = self.bootstrap_audit.as_deref().ok_or_else(|| {
            SemOsError::MigrationPending("bootstrap audit store not configured".into())
        })?;
        store
            .start_bootstrap(bundle_hash, actor_id, bundle_counts)
            .await
    }

    async fn bootstrap_mark_published(&self, bundle_hash: &str) -> Result<()> {
        let store = self.bootstrap_audit.as_deref().ok_or_else(|| {
            SemOsError::MigrationPending("bootstrap audit store not configured".into())
        })?;
        store.mark_published(bundle_hash).await
    }

    async fn bootstrap_mark_failed(&self, bundle_hash: &str, error: &str) -> Result<()> {
        let store = self.bootstrap_audit.as_deref().ok_or_else(|| {
            SemOsError::MigrationPending("bootstrap audit store not configured".into())
        })?;
        store.mark_failed(bundle_hash, error).await
    }

    async fn get_affinity_graph(&self) -> Result<Arc<AffinityGraph>> {
        // Fast path: cache hit.
        {
            let guard = self.affinity_graph.read().await;
            if let Some(graph) = &*guard {
                return Ok(Arc::new(graph.clone()));
            }
        }
        // Slow path: build from active snapshots.
        let snapshots = self.snapshots.load_active_snapshots().await?;
        let graph = AffinityGraph::build(&snapshots);
        let arc_graph = Arc::new(graph.clone());
        {
            let mut guard = self.affinity_graph.write().await;
            *guard = Some(graph);
        }
        Ok(arc_graph)
    }

    async fn drain_outbox_for_test(&self) -> Result<()> {
        // Claim and process all pending outbox events.
        // Each event triggers the ProjectionWriter to update the active snapshot set.
        let claimer_id = "drain_outbox_for_test";
        while let Some(event) = self.outbox.claim_next(claimer_id).await? {
            match self.projections.write_active_snapshot_set(&event).await {
                Ok(()) => {
                    self.outbox.mark_processed(&event.event_id).await?;
                }
                Err(e) => {
                    let error_msg = format!("{e}");
                    // In test mode, dead-letter immediately and propagate the error.
                    self.outbox
                        .mark_dead_letter(&event.event_id, &error_msg)
                        .await?;
                    return Err(e);
                }
            }
        }
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────

fn parse_uuid(s: &str, field_name: &str) -> Result<Uuid> {
    Uuid::parse_str(s)
        .map_err(|_| SemOsError::InvalidInput(format!("invalid UUID for {field_name}: {s}")))
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abac::ActorContext;
    use crate::context_resolution::{
        DiscoveryContext, EvidenceMode, ResolutionConstraints, SubjectRef,
    };
    use crate::grounding::ConstellationModel;
    use chrono::Utc;
    use sem_os_ontology::constellation_map_def::ConstellationMapDefBody;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_parse_uuid_valid() {
        let id = Uuid::new_v4();
        let result = parse_uuid(&id.to_string(), "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), id);
    }

    #[test]
    fn test_parse_uuid_invalid() {
        let result = parse_uuid("not-a-uuid", "test");
        assert!(result.is_err());
        match result.unwrap_err() {
            SemOsError::InvalidInput(msg) => {
                assert!(msg.contains("test"));
                assert!(msg.contains("not-a-uuid"));
            }
            other => panic!("Expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn grounded_traversal_edges_include_parent_and_dependency_edges() {
        let map: ConstellationMapDefBody = serde_yaml::from_str(
            r#"
fqn: demo
constellation: demo
jurisdiction: LU
slots:
  cbu:
    type: cbu
    table: cbus
    pk: cbu_id
    cardinality: root
  cbu.case:
    type: case
    table: cases
    pk: case_id
    cardinality: optional
    depends_on:
      - slot: cbu
        min_state: filled
    verbs:
      open: case.open
"#,
        )
        .unwrap();
        let model = ConstellationModel::from_parts(map, HashMap::new());
        let edges =
            build_grounded_traversal_edges(&model, "cbu.case", &SubjectRef::TaskId(Uuid::nil()));

        assert!(edges
            .iter()
            .any(|edge| edge.relationship == "contains_slot"));
        assert!(edges
            .iter()
            .any(|edge| edge.relationship == "depends_on[min_state=filled]"));
    }

    fn snapshot_row(
        object_type: ObjectType,
        definition: serde_json::Value,
        fqn: &str,
    ) -> SnapshotRow {
        SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type,
            object_id: sem_os_core::ids::object_id_for(object_type, fqn),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Convenience,
            security_label: json!({}),
            effective_from: Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: Some("auto".into()),
            definition,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn seed_bootstrap_plan_skips_identical_payload() {
        let principal = Principal::system();
        let payload = json!({"pack_id": "ob-poc.cbu", "version": "1"});
        let current = snapshot_row(ObjectType::DomainPack, payload.clone(), "ob-poc.cbu");

        let plan = plan_seed_bootstrap_item(
            &principal,
            "v1:test",
            ObjectType::DomainPack,
            "ob-poc.cbu",
            &payload,
            Some(&current),
        )
        .expect("plan");

        assert!(matches!(plan, SeedBootstrapPlan::Skip));
    }

    #[test]
    fn seed_bootstrap_plan_versions_changed_payload() {
        let principal = Principal::explicit("seed-runner", vec!["admin".into()]);
        let current = snapshot_row(
            ObjectType::DomainPack,
            json!({"pack_id": "ob-poc.cbu", "version": "1"}),
            "ob-poc.cbu",
        );
        let payload = json!({"pack_id": "ob-poc.cbu", "version": "2"});

        let plan = plan_seed_bootstrap_item(
            &principal,
            "v1:test",
            ObjectType::DomainPack,
            "ob-poc.cbu",
            &payload,
            Some(&current),
        )
        .expect("plan");

        let SeedBootstrapPlan::Publish(meta, planned_payload) = plan else {
            panic!("expected publish plan");
        };
        assert_eq!(meta.object_type, ObjectType::DomainPack);
        assert_eq!(meta.object_id, current.object_id);
        assert_eq!(meta.version_major, current.version_major);
        assert_eq!(meta.version_minor, current.version_minor + 1);
        assert_eq!(meta.change_type, ChangeType::NonBreaking);
        assert_eq!(meta.predecessor_id, Some(current.snapshot_id));
        assert_eq!(meta.created_by, "seed-runner");
        assert_eq!(planned_payload, payload);
    }

    #[test]
    fn seed_bootstrap_plan_rejects_type_conflict() {
        let principal = Principal::system();
        let current = snapshot_row(
            ObjectType::DagTaxonomy,
            json!({"fqn": "ob-poc.cbu"}),
            "ob-poc.cbu",
        );

        let err = plan_seed_bootstrap_item(
            &principal,
            "v1:test",
            ObjectType::DomainPack,
            "ob-poc.cbu",
            &json!({"pack_id": "ob-poc.cbu"}),
            Some(&current),
        )
        .expect_err("type conflict");

        assert!(matches!(err, SemOsError::Conflict(_)));
    }

    #[test]
    fn discovery_surface_ranks_matching_domain_and_family() {
        let req = ContextResolutionRequest {
            subject: SubjectRef::TaskId(Uuid::new_v4()),
            intent_summary: Some("onboard a new Irish fund".into()),
            raw_utterance: Some("Please help me onboard a new Irish fund".into()),
            actor: ActorContext {
                actor_id: "test-user".into(),
                roles: vec!["analyst".into()],
                department: None,
                clearance: Some(Classification::Internal),
                jurisdictions: vec!["IE".into()],
            },
            goals: vec![],
            constraints: ResolutionConstraints {
                jurisdiction: Some("IE".into()),
                risk_posture: None,
                thresholds: HashMap::new(),
            },
            evidence_mode: EvidenceMode::default(),
            point_in_time: None,
            entity_kind: Some("fund".into()),
            entity_confidence: Some(0.9),
            discovery: DiscoveryContext::default(),
        };

        let universe = snapshot_row(
            ObjectType::UniverseDef,
            json!({
                "fqn": "universe.client_lifecycle",
                "universe_id": "client_lifecycle",
                "name": "Client Lifecycle",
                "description": "Discovery universe",
                "version": "1.0",
                "domains": [{
                    "domain_id": "onboarding",
                    "label": "Onboarding",
                    "description": "New entity work",
                    "objective_tags": ["onboarding"],
                    "utterance_signals": [{"signal_type": "keyword", "pattern": "onboard", "weight": 1.0}],
                    "candidate_entity_kinds": ["fund"],
                    "candidate_family_ids": ["fund_onboarding"],
                    "required_grounding_inputs": [],
                    "entry_questions": [{
                        "question_id": "q1",
                        "prompt": "What are you onboarding?",
                        "maps_to": "objective",
                        "priority": 1
                    }],
                    "allowed_discovery_actions": ["research.search"]
                }]
            }),
            "universe.client_lifecycle",
        );
        let family = snapshot_row(
            ObjectType::ConstellationFamilyDef,
            json!({
                "fqn": "family.fund_onboarding",
                "family_id": "fund_onboarding",
                "label": "Fund Onboarding",
                "description": "Fund onboarding family",
                "domain_id": "onboarding",
                "selection_rules": [],
                "constellation_refs": [{
                    "constellation_id": "struct.ie.ucits.icav",
                    "label": "Irish UCITS ICAV",
                    "jurisdiction": "IE",
                    "entity_kind": "fund",
                    "triggers": ["irish", "fund"]
                }],
                "candidate_jurisdictions": ["IE"],
                "candidate_entity_kinds": ["fund"],
                "grounding_threshold": {
                    "required_input_keys": ["objective", "jurisdiction", "entity_kind"],
                    "requires_entity_instance": false,
                    "allows_draft_instance": true
                }
            }),
            "family.fund_onboarding",
        );

        let surface = build_discovery_surface(&req, &[universe], &[family]).expect("surface");
        assert_eq!(
            surface
                .matched_domains
                .first()
                .map(|d| d.domain_id.as_str()),
            Some("onboarding")
        );
        assert_eq!(
            surface
                .matched_families
                .first()
                .map(|family| family.family_id.as_str()),
            Some("fund_onboarding")
        );
        assert_eq!(
            surface.grounding_readiness,
            GroundingReadiness::ConstellationReady
        );
    }

    #[test]
    fn discovery_surface_prefers_domain_candidate_family_ids() {
        let req = ContextResolutionRequest {
            subject: SubjectRef::TaskId(Uuid::new_v4()),
            intent_summary: Some("set up billing for a US client".into()),
            raw_utterance: Some("Please set up billing for a US client".into()),
            actor: ActorContext {
                actor_id: "test-user".into(),
                roles: vec!["analyst".into()],
                department: None,
                clearance: Some(Classification::Internal),
                jurisdictions: vec!["US".into()],
            },
            goals: vec![],
            constraints: ResolutionConstraints {
                jurisdiction: Some("US".into()),
                risk_posture: None,
                thresholds: HashMap::new(),
            },
            evidence_mode: EvidenceMode::default(),
            point_in_time: None,
            entity_kind: Some("contract".into()),
            entity_confidence: Some(0.9),
            discovery: DiscoveryContext::default(),
        };

        let universe = snapshot_row(
            ObjectType::UniverseDef,
            json!({
                "fqn": "universe.deal_execution",
                "universe_id": "deal_execution",
                "name": "Deal Execution",
                "description": "Commercial discovery universe",
                "version": "1.0",
                "domains": [{
                    "domain_id": "billing",
                    "label": "Billing",
                    "description": "Billing setup",
                    "objective_tags": ["billing"],
                    "utterance_signals": [{"signal_type": "keyword", "pattern": "billing", "weight": 1.0}],
                    "candidate_entity_kinds": ["contract"],
                    "candidate_family_ids": ["billing_operations"],
                    "required_grounding_inputs": [],
                    "entry_questions": [],
                    "allowed_discovery_actions": ["research.search"]
                }]
            }),
            "universe.deal_execution",
        );
        let billing_family = snapshot_row(
            ObjectType::ConstellationFamilyDef,
            json!({
                "fqn": "family.billing_operations",
                "family_id": "billing_operations",
                "label": "Billing Operations",
                "description": "Billing family",
                "domain_id": "billing",
                "selection_rules": [],
                "constellation_refs": [{
                    "constellation_id": "struct.us.40act.open-end",
                    "label": "US Billing Anchor",
                    "jurisdiction": "US",
                    "entity_kind": "fund",
                    "triggers": ["billing", "invoice"]
                }],
                "candidate_jurisdictions": ["US"],
                "candidate_entity_kinds": ["contract"],
                "grounding_threshold": {
                    "required_input_keys": ["objective", "jurisdiction"],
                    "requires_entity_instance": false,
                    "allows_draft_instance": true
                }
            }),
            "family.billing_operations",
        );
        let distractor_family = snapshot_row(
            ObjectType::ConstellationFamilyDef,
            json!({
                "fqn": "family.generic_billing",
                "family_id": "generic_billing",
                "label": "Generic Billing",
                "description": "Generic billing family",
                "domain_id": "billing",
                "selection_rules": [],
                "constellation_refs": [{
                    "constellation_id": "struct.us.40act.closed-end",
                    "label": "US Generic Billing Anchor",
                    "jurisdiction": "US",
                    "entity_kind": "fund",
                    "triggers": ["billing"]
                }],
                "candidate_jurisdictions": ["US"],
                "candidate_entity_kinds": ["contract"],
                "grounding_threshold": {
                    "required_input_keys": ["objective", "jurisdiction"],
                    "requires_entity_instance": false,
                    "allows_draft_instance": true
                }
            }),
            "family.generic_billing",
        );

        let surface =
            build_discovery_surface(&req, &[universe], &[billing_family, distractor_family])
                .expect("surface");
        assert_eq!(
            surface
                .matched_families
                .first()
                .map(|family| family.family_id.as_str()),
            Some("billing_operations")
        );
    }
}
