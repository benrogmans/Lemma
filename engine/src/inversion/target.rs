//! Target specification for inversion queries

use crate::evaluation::operations::VetoType;
use crate::planning::semantics::LiteralValue;
use crate::OperationResult;
use serde::Serialize;

/// Desired outcome for an inversion query
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Target {
    /// The comparison operator
    pub op: TargetOp,

    /// The desired outcome (value or veto)
    /// None means "any value" (wildcard for non-veto results)
    pub outcome: Option<OperationResult>,
}

/// Comparison operators for targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum TargetOp {
    /// Equal to (`is`)
    Eq,
    /// Not equal to (`is not`)
    Neq,
    /// Less than (<)
    Lt,
    /// Less than or equal to (<=)
    Lte,
    /// Greater than (>)
    Gt,
    /// Greater than or equal to (>=)
    Gte,
}

impl Target {
    /// Create a target for a specific value with equality operator
    pub fn value(value: LiteralValue) -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Value(Box::new(value))),
        }
    }

    /// Create a target for a specific veto message
    pub fn veto(message: Option<String>) -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Veto(VetoType::UserDefined { message })),
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

    /// Format target for display
    pub fn format(&self) -> String {
        let op_str = match self.op {
            TargetOp::Eq => "=",
            TargetOp::Neq => "is not",
            TargetOp::Lt => "<",
            TargetOp::Lte => "<=",
            TargetOp::Gt => ">",
            TargetOp::Gte => ">=",
        };

        let value_str = match &self.outcome {
            None => "any".to_string(),
            Some(OperationResult::Value(v)) => v.to_string(),
            Some(OperationResult::Veto(reason)) => format!("veto({reason})"),
        };

        format!("{} {}", op_str, value_str)
    }
}
