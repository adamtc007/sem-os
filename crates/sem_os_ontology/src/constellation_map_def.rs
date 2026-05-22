//! Constellation map definition body types.
//!
//! **Canonical source**: `dsl_types::constellation_map_def`.
//! This module is a compat re-export so existing `sem_os_ontology::constellation_map_def::*`
//! import paths keep resolving during the Phase 1 → Phase 3 migration window.
//!
//! After Phase 3 (dsl-core split), callers that live in the sem-os layer will
//! import from `dsl_types::constellation_map_def` directly. This shim is
//! removed at Phase 12 cleanup.

pub use dsl_types::constellation_map_def::*;
