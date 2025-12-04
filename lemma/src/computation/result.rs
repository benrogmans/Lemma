//! Operation result type
//!
//! Represents the outcome of a computation: either a value or a veto.

use crate::LiteralValue;
use serde::Serialize;

/// Result of an operation (evaluating a rule or expression)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum OperationResult {
    /// Operation produced a value
    Value(LiteralValue),
    /// Operation was vetoed (valid result, no value)
    Veto(Option<String>),
}

impl OperationResult {
    pub fn is_veto(&self) -> bool {
        matches!(self, OperationResult::Veto(_))
    }

    #[must_use]
    pub fn value(&self) -> Option<&LiteralValue> {
        match self {
            OperationResult::Value(v) => Some(v),
            OperationResult::Veto(_) => None,
        }
    }
}

impl std::fmt::Display for OperationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationResult::Value(v) => write!(f, "{}", v),
            OperationResult::Veto(Some(msg)) => write!(f, "veto(\"{}\")", msg),
            OperationResult::Veto(None) => write!(f, "veto"),
        }
    }
}
