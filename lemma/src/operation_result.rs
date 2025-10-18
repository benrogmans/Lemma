use crate::semantic::LiteralValue;

/// Result of an operation (evaluating a rule or expression)
#[derive(Debug, Clone, PartialEq)]
pub enum OperationResult {
    /// Operation produced a value
    Value(LiteralValue),
    /// Operation was vetoed (valid result, no value)
    Veto(Option<String>),
}

impl OperationResult {
    /// Check if this is a vetoed result
    pub fn is_vetoed(&self) -> bool {
        matches!(self, OperationResult::Veto(_))
    }

    /// Get the value if present, None if vetoed
    pub fn value(&self) -> Option<&LiteralValue> {
        match self {
            OperationResult::Value(v) => Some(v),
            OperationResult::Veto(_) => None,
        }
    }

    /// Get the veto message if vetoed, None otherwise
    pub fn veto_message(&self) -> Option<&Option<String>> {
        match self {
            OperationResult::Veto(msg) => Some(msg),
            OperationResult::Value(_) => None,
        }
    }
}

