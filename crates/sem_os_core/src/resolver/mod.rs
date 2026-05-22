//! Resolver computation — moved from dsl-core in Phase 3 extraction.
//!
//! Type definitions (ResolvedTemplate, ResolvedSlot, etc.) remain in
//! `dsl_core::resolver`. This module owns the computation:
//! - `resolve_template` (constellation map loading + composition)
//! - `load_shape_rules_from_dir` (shape rule loading)

mod composer;
pub mod shape_rule;

pub use composer::{
    load_constellation_maps_from_dir, resolve_template, LoadedConstellationMap, ResolveError,
    ResolverInputs,
};
pub use shape_rule::{
    load_shape_rules_from_dir, AddBranch, AddConstraint, AddTerminal, InsertBetween, RawStateEdit,
    RefineReducer, ReplaceConstraint, ShapeRule, SlotGateMetadataRefinement, StructuralFacts,
    TightenConstraint, LoadedShapeRule,
};
