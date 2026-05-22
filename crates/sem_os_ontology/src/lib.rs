//! sem_os_ontology — the SemOS `*_def` vocabulary.
//!
//! ## Capability claim
//!
//! Definition bodies for everything SemOS catalogues: attributes, entity
//! types, document types, relationship types, state graphs, taxonomies,
//! verbs, views, requirement profiles, evidence strategies, observations,
//! universe / constellation maps, derivations, evidence DTOs, and the
//! snapshot-body wrapper that registry storage round-trips through.
//!
//! Pure data-with-validation. No ABAC, no projection, no enforcement,
//! no registry mutation.
//!
//! ## Anti-charter
//!
//! - Does NOT decide what may happen — that's `sem_os_policy::{abac,
//!   enforce, gates, ...}`.
//! - Does NOT project or observe — that's `sem_os_policy::{acp_projection,
//!   affinity, observatory, diagram, ...}`.
//! - Does NOT mutate the registry — that's the SemOS authoring / stewardship
//!   pipeline in `sem_os_policy::{authoring, stewardship}`.
//! - Does NOT depend on `sem_os_policy`. Ever.
//!
//! ## Dependency discipline
//!
//! May depend on `sem_os_core` (the engine: `types`, `ids`, `error`,
//! `principal`, `ports`, `proto`, `seeds`, `service`, `execution`).
//! MUST NOT depend on `sem_os_policy`.
//!
//! ## Migration status (2026-05-14)
//!
//! Phase 2 of `docs/todo/sem-os-core-split-v1.md`. Modules land in
//! Phases 2–4 via `git mv` from `sem_os_core/src/`.
//!
//! - Phase 2: 9 pure-type leaves (policy_rule, observation_def,
//!   verb_contract, view_def, taxonomy_def, universe_def,
//!   requirement_profile_def, relationship_type_def, state_machine_def).
//! - Phase 3 (current): 13 body-with-types modules. attribute_def
//!   and derivation_spec import from `sem_os_types` (which moved to
//!   the new bottom tier in Phase 2.5). `derivation` deferred to a
//!   later phase — it reaches `security::compute_inherited_label` in
//!   the policy plane. The 3 previously-`pub(crate)` modules
//!   (constellation_family_def, constellation_map_def, macro_def) are
//!   promoted to `pub` by this move.
//!
//! Compat re-exported from `sem_os_core` until Phase 12 cleanup.

pub mod attribute_def;
pub mod constellation_family_def;
pub mod constellation_map_def;
pub mod derivation_spec;
pub mod document_type_def;
pub mod entity_type_def;
pub mod evidence;
pub mod evidence_strategy_def;
pub mod macro_def;
pub mod membership;
pub mod observation_def;
pub mod policy_rule;
pub mod proof_obligation_def;
pub mod relationship_type_def;
pub mod requirement_profile_def;
pub mod service_resource_def;
pub mod state_graph_def;
pub mod state_machine_def;
pub mod taxonomy_def;
pub mod universe_def;
pub mod verb_contract;
pub mod view_def;
