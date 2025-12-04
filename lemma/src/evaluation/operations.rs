//! Evaluation operation tracking types
//!
//! This module contains types for recording operations during evaluation.
//! The actual arithmetic and comparison operations are in the `computation` module.

pub use crate::computation::{
    arithmetic_operation, comparison_operation, convert_unit, to_base_unit_value, OperationResult,
};
use crate::{
    ArithmeticComputation, ComparisonComputation, FactPath, LiteralValue, LogicalComputation,
    MathematicalComputation, RulePath,
};
use serde::Serialize;

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
        expression: String,
    },
    RuleUsed {
        rule_path: RulePath,
        result: OperationResult,
        expression: String,
    },
    Computation {
        kind: ComputationKind,
        inputs: Vec<LiteralValue>,
        result: LiteralValue,
        expression: String,
    },
    RuleBranchEvaluated {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        matched: bool,
        condition_expression: String,
        result_expression: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_value: Option<OperationResult>,
    },
}
