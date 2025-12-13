//! Pure computation operations for Lemma
//!
//! This module contains stateless, pure functions for type-aware arithmetic
//! and comparison operations. It has no dependencies on evaluation state.
//!
//! Both the evaluation engine, planning, and inversion systems use these operations.

pub mod arithmetic;
pub mod expansion;
pub mod comparison;
pub mod constraints;
pub mod datetime;
pub mod math_properties;
pub mod result;
pub mod units;
pub mod simplification;

pub use units::{convert_unit, to_base_unit_value};
pub use arithmetic::arithmetic_operation;
pub use expansion::{expand, reverse_comparison};
pub use comparison::comparison_operation;
pub use constraints::{
    Bound, ConstraintSet, DomainRestriction, FactBounds, FactConstraint, UnsatReason,
};
pub use math_properties::{check_function_range_violation, collect_domain_restrictions};
pub use result::OperationResult;
pub use simplification::reduce;
