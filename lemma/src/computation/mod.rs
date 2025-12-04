//! Pure computation operations for Lemma
//!
//! This module contains stateless, pure functions for type-aware arithmetic
//! and comparison operations. It has no dependencies on evaluation state.
//!
//! Both the evaluation engine and inversion system use these operations.

pub mod arithmetic;
pub mod comparison;
pub mod datetime;
pub mod result;
pub mod units;

pub use arithmetic::arithmetic_operation;
pub use comparison::comparison_operation;
pub use result::OperationResult;
pub use units::{convert_unit, to_base_unit_value};

