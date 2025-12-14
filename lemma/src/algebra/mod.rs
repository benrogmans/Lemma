//! Mathematical reasoning tools for the Lemma engine
//!
//! Provides algebraic operations for planning, evaluation, and inversion.
//! NOT to be confused with computation/ which contains Lemma's runtime operations.

pub mod constraints;
pub mod expansion;
pub mod isolation;
pub mod math_properties;
pub mod simplification;

// Re-export commonly used items
pub use constraints::{
    Bound, ConstraintSet, DomainRestriction, FactBounds, FactConstraint, UnsatReason,
};
pub use expansion::{expand, reverse_comparison};
pub use simplification::reduce;
