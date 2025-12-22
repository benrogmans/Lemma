//! Pure computation operations for Lemma
//!
//! Stateless, pure functions for type-aware arithmetic, comparison, and unit operations.
//! No dependencies on evaluation state - used by both evaluation and inversion systems.

pub mod arithmetic;
pub mod comparison;
pub mod datetime;
pub mod units;

pub use arithmetic::arithmetic_operation;
pub use comparison::comparison_operation;
pub use units::{convert_unit, to_base_unit_value};
