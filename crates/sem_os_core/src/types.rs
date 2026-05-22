//! sem_os_core::types compat shim.
//!
//! All real definitions live in `sem_os_types`. This shim preserves
//! `sem_os_core::types::*` paths during the sem_os_core-split v1
//! migration (Phase 2.5 + Phase 9, 2026-05-14). Both `ChangesetStatus`
//! and `ChangeSetStatus` canonical homes are sem_os_types, so the
//! single wildcard re-export is sufficient.

pub use sem_os_types::*;
