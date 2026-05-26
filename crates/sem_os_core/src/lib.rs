//! sem_os_core — engine + foundation primitives.
//!
//! After sem_os_core-split v1 (docs/todo/sem-os-core-split-v1.md), this
//! crate holds only the engine tier:
//!   error, execution, ids, ports, principal, proto, seeds, service,
//!   types (compat shim → sem_os_types).
//!
//! The `*_def` vocabulary lives in `sem_os_ontology`. Policy / projection
//! / observatory / authoring / stewardship live in `sem_os_policy`.
//! Foundational vocabulary (Classification, EvidenceGrade, SecurityLabel,
//! Changeset, ChangeSetStatus, …) lives in `sem_os_types`.

// ── Public API surface ──────────────────────────────────────────────────────
// `pub(crate)` modules are implementation detail — not accessible to consumers.
// ─────────────────────────────────────────────────────────────────────────────

pub mod error;
pub(crate) mod execution; // empty placeholder pending removal — not in external import surface
pub(crate) mod frontier;  // internal crate utility — not imported by any consumer
pub mod ids;
pub mod ports;
pub mod principal;
pub mod proto;
pub mod resolver;
pub mod seeds;
pub mod types;
