//! Response types for inversion queries
//!
//! Contains the solution and response structures returned by inversion.

use crate::computation::{FactConstraint, OperationResult};
use crate::semantic::FactPath;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::collections::HashMap;

/// A single solution to an inversion query
///
/// Each solution represents one valid combination of fact values
/// that produces the target outcome.
#[derive(Debug, Clone)]
pub struct Solution {
    /// The outcome this solution produces
    pub outcome: OperationResult,

    /// Constraints on fact values for this solution
    pub fact_constraints: HashMap<FactPath, FactConstraint>,
}

impl Solution {
    /// Create a new solution with the given outcome and fact constraints
    pub fn new(
        outcome: OperationResult,
        fact_constraints: HashMap<FactPath, FactConstraint>,
    ) -> Self {
        Self {
            outcome,
            fact_constraints,
        }
    }
}

impl Serialize for Solution {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Solution", 2)?;

        state.serialize_field("outcome", &self.outcome)?;

        let fact_constraints_serializable: HashMap<String, &FactConstraint> = self
            .fact_constraints
            .iter()
            .map(|(path, constraint)| (path.to_string(), constraint))
            .collect();
        state.serialize_field("fact_constraints", &fact_constraints_serializable)?;

        state.end()
    }
}

/// Response from an inversion query
///
/// Contains all valid solutions that produce the target outcome.
#[derive(Debug, Clone)]
pub struct InversionResponse {
    /// All valid solutions
    pub solutions: Vec<Solution>,
}

impl InversionResponse {
    /// Create a new inversion response with the given solutions
    pub fn new(solutions: Vec<Solution>) -> Self {
        Self { solutions }
    }

    /// Create an empty response (no solutions)
    pub fn empty() -> Self {
        Self {
            solutions: Vec::new(),
        }
    }

    /// Check if the response has any solutions
    pub fn has_solutions(&self) -> bool {
        !self.solutions.is_empty()
    }

    /// Get the number of solutions
    pub fn solutions_count(&self) -> usize {
        self.solutions.len()
    }
}

impl Serialize for InversionResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("InversionResponse", 1)?;
        state.serialize_field("solutions", &self.solutions)?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LiteralValue;
    use rust_decimal::Decimal;

    #[test]
    fn test_empty_response() {
        let response = InversionResponse::empty();
        assert!(!response.has_solutions());
        assert_eq!(response.solutions_count(), 0);
    }

    #[test]
    fn test_response_with_solutions() {
        let solutions = vec![Solution::new(
            OperationResult::Value(LiteralValue::Number(Decimal::from(42))),
            HashMap::new(),
        )];
        let response = InversionResponse::new(solutions);
        assert!(response.has_solutions());
        assert_eq!(response.solutions_count(), 1);
    }
}
