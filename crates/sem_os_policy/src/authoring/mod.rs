//! Research → Governed Change Boundary authoring pipeline.
//!
//! This module implements the authoring pipeline defined in
//! `docs/semantic_os_research_governed_boundary_v0.4.md`:
//!
//! - **types**: ChangeSet status, artifacts, reports, governance audit
//! - **errors**: Structured error codes (V:*, D:*, PUBLISH:*)
//! - **canonical_hash**: Content-addressed hashing for idempotent propose
//! - **ports**: AuthoringStore + ScratchSchemaRunner traits
//! - **validate_stage1**: Pure validation (hash, parse, reference, semantic)
//! - **validate_stage2**: DB-backed validation (scratch schema, compatibility)
//! - **diff**: Structural diff between artifact sets
//! - **governance_verbs**: 7 governance verb orchestration
//! - **bundle**: Bundle ingestion (changeset.yaml manifest + directory layout)

// agent_mode relocated to sem_os_types in Phase 9 — needed by sem_os_core::
// principal.rs, which can't reach up into the policy plane. Back-compat
// re-export so `sem_os_policy::authoring::agent_mode::AgentMode` still
// resolves.
pub use sem_os_types::agent_mode;
#[allow(dead_code)]
pub(crate) mod canonical_hash; // content-addressed hashing — internal pipeline detail
#[allow(dead_code)]
pub(crate) mod diff;           // structural diff — internal pipeline detail
#[allow(dead_code)]
pub(crate) mod errors;         // structured error codes — internal, exposed via SemOsError at boundary
pub mod ports;
pub mod types;
#[allow(dead_code)]
pub(crate) mod validate_stage1; // pure validation stage — internal pipeline detail
#[allow(dead_code)]
pub(crate) mod validate_stage2; // DB-backed validation stage — internal pipeline detail

pub mod bundle;
pub mod cleanup;
pub mod governance_verbs;
#[allow(dead_code)]
pub(crate) mod metrics;         // internal instrumentation
