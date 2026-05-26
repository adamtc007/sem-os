//! sem_os_taxonomy — taxonomy projection for the Semantic OS registry.
//!
//! Builds a [`TaxonomyTree`] from a flat list of active [`SnapshotRow`]s and
//! provides YAML/JSON emit so a UI (or any consumer) can render the full
//! classified shape of a semantic domain without touching the database.
//!
//! ## Usage
//!
//! ```rust
//! use sem_os_taxonomy::{build_all_taxonomy_trees, TaxonomyTree};
//! use sem_os_types::SnapshotRow;
//!
//! let snapshots: Vec<SnapshotRow> = vec![]; // from registry store
//! let trees: Vec<TaxonomyTree> = build_all_taxonomy_trees(&snapshots);
//!
//! // YAML emit — ready for a UI
//! for tree in &trees {
//!     let yaml = tree.to_yaml().expect("serialization never fails on valid data");
//!     println!("{yaml}");
//! }
//! ```

mod builder;
mod types;

pub use builder::{build_all_taxonomy_trees, build_taxonomy_tree};
pub use sem_os_ontology::membership::MembershipKind;
pub use types::{MemberSummary, TaxonomyMeta, TaxonomyTree, TaxonomyTreeNode};
