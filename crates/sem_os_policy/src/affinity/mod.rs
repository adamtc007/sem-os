//! AffinityGraph — bidirectional verb↔data index built from registry snapshots.
//!
//! The AffinityGraph is a pre-computed in-memory index that makes implicit
//! verb↔data relationships queryable. It is built from active snapshots via
//! a 5-pass builder and supports 10 query methods for navigation and governance.

pub mod builder;
pub mod discovery;
pub mod query;
pub mod types;

pub use discovery::{
    discover_dsl, discovery_edges, generate_disambiguation, match_intent, synthesize_chain,
    DisambiguationPrompt, DiscoveryResponse, GovernanceContext, IntentMatch, VerbChainSuggestion,
};
pub use types::{
    AffinityEdge, AffinityGraph, AffinityKind, AffinityProvenance, ColumnRef, DataAffinity,
    DataFootprint, DataRef, DerivationEdge, EntityRelationship, TableRef, VerbAffinity,
};
