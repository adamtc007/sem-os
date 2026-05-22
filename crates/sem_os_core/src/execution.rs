//! Empty — execution types moved to `dsl_runtime` in Phase 2.
//!
//! Per `docs/backlog/three-plane-architecture-implementation-plan-v0.1.md`
//! §3 Phase 2, every type that was formerly here now lives in
//! `dsl_runtime`:
//!
//! - `VerbExecutionPort`, `CrudExecutionPort` → `dsl_runtime::{VerbExecutionPort, CrudExecutionPort}`.
//! - `VerbExecutionContext`, `VerbExecutionOutcome`, `VerbSideEffects`,
//!   `VerbExecutionResult` → `dsl_runtime::execution::*`.
//! - `Result<T>` alias → `dsl_runtime::Result<T>` (same `SemOsError` error type).
//!
//! This module is kept as an empty file with a stable path so upstream
//! import-path migration can land in a single PR per crate without
//! breaking the crate graph mid-bisect. A follow-up slice removes the
//! `pub mod execution;` declaration entirely.
