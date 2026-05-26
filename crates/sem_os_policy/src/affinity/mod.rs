//! AffinityGraph — bidirectional verb↔data index built from registry snapshots.
//!
//! The AffinityGraph is a pre-computed in-memory index that makes implicit
//! verb↔data relationships queryable. It is built from active snapshots via
//! a 5-pass builder and supports 10 query methods for navigation and governance.

pub(crate) mod builder;   // 5-pass construction algorithm — internal, not part of public contract
pub mod discovery;         // sources of re-exported DiscoveryResponse etc. — must stay pub
#[allow(dead_code)]
pub(crate) mod query;     // internal query helpers
pub mod types;             // types accessed by ob-poc at affinity::types::{DataRef, TableRef}

pub use discovery::{
    discover_dsl, discovery_edges, generate_disambiguation, match_intent, synthesize_chain,
    DisambiguationPrompt, DiscoveryResponse, GovernanceContext, IntentMatch, VerbChainSuggestion,
};
pub use types::{
    AffinityEdge, AffinityGraph, AffinityKind, AffinityProvenance, ColumnRef, DataAffinity,
    DataFootprint, DataRef, DerivationEdge, EntityRelationship, TableRef, VerbAffinity,
};
