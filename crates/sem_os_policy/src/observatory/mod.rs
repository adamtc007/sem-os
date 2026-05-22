//! Observatory — visual projection layer for SemOS.
//!
//! The Observatory renders the same structures the Sage/REPL agent pipeline
//! traverses. It adds two things:
//! 1. A render projection layer (OrientationContract, GraphSceneModel)
//! 2. An observation frame (client-owned camera state)
//!
//! It does NOT add new semantic pipelines — only projections from existing types.

pub mod orientation;
pub mod projection;
