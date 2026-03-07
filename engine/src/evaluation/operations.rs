//! Operation types and result handling for evaluation

use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, FactPath, LiteralValue, LogicalComputation,
    MathematicalComputation, RulePath,
};
use serde::Serialize;

/// Result of an operation (evaluating a rule or expression)
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationResult {
    /// Operation produced a value (boxed to keep enum small)
    Value(Box<LiteralValue>),
    /// Operation was vetoed (valid result, no value)
    Veto(Option<String>),
}

impl OperationResult {
    pub fn vetoed(&self) -> bool {
        matches!(self, OperationResult::Veto(_))
    }

    #[must_use]
    pub fn value(&self) -> Option<&LiteralValue> {
        match self {
            OperationResult::Value(v) => Some(v.as_ref()),
            OperationResult::Veto(_) => None,
        }
    }
}

/// The kind of computation performed
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComputationKind {
    Arithmetic(ArithmeticComputation),
    Comparison(ComparisonComputation),
    Logical(LogicalComputation),
    Mathematical(MathematicalComputation),
}

/// A record of a single operation during evaluation
#[derive(Debug, Clone, Serialize)]
pub struct OperationRecord {
    #[serde(flatten)]
    pub kind: OperationKind,
}

/// The kind of operation performed
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationKind {
    FactUsed {
        fact_ref: FactPath,
        value: LiteralValue,
    },
    RuleUsed {
        rule_path: RulePath,
        result: OperationResult,
    },
    Computation {
        kind: ComputationKind,
        inputs: Vec<LiteralValue>,
        result: LiteralValue,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        expr: Option<String>,
    },
    RuleBranchEvaluated {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        matched: bool,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        condition_expr: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_expr: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_value: Option<OperationResult>,
    },
}
