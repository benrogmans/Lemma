use crate::{LiteralValue, OperationResult};

/// Desired outcome for an inversion query
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The comparison operator
    pub op: TargetOp,

    /// The desired outcome (value or veto)
    /// None means "any value" (wildcard for non-veto results)
    pub outcome: Option<OperationResult>,
}

/// Comparison operators for targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetOp {
    /// Equal to (=)
    Eq,
    /// Not equal to (≠)
    Neq,
    /// Less than (<)
    Lt,
    /// Less than or equal to (≤)
    Lte,
    /// Greater than (>)
    Gt,
    /// Greater than or equal to (≥)
    Gte,
}

impl Target {
    /// Create a target for a specific value with equality operator
    pub fn value(value: LiteralValue) -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Value(value)),
        }
    }

    /// Create a target for a specific veto message
    pub fn veto(message: Option<String>) -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Veto(message)),
        }
    }

    /// Create a target for any veto
    pub fn any_veto() -> Self {
        Self::veto(None)
    }

    /// Create a target for any value (non-veto)
    pub fn any_value() -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: None,
        }
    }

    /// Create a target with a custom operator
    pub fn with_op(op: TargetOp, outcome: OperationResult) -> Self {
        Self {
            op,
            outcome: Some(outcome),
        }
    }
}
