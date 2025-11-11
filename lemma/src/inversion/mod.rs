//! Inverse reasoning for Lemma rules
//!
//! Determines what inputs produce desired outputs through symbolic manipulation.

pub mod algebra;
pub mod boolean;
pub mod domain_extraction;
pub mod domain_ops;
pub mod hydration;
pub mod inverter;
pub mod shape;
pub mod target;

pub use shape::{Bound, BranchOutcome, Domain, Shape, ShapeBranch};
pub use target::{Target, TargetOp};
