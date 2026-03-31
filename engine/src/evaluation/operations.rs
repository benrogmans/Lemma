//! Operation types and result handling for evaluation

use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, FactPath, LiteralValue, LogicalComputation,
    MathematicalComputation, RulePath,
};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "computation", rename_all = "snake_case")]
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
    },
    RuleBranchEvaluated {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        matched: bool,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_value: Option<OperationResult>,
    },
}

#[cfg(test)]
mod computation_kind_serde_tests {
    use super::ComputationKind;
    use crate::parsing::ast::{
        ArithmeticComputation, ComparisonComputation, MathematicalComputation,
    };
    use crate::planning::semantics::LogicalComputation;

    #[test]
    fn computation_kind_arithmetic_round_trip() {
        let k = ComputationKind::Arithmetic(ArithmeticComputation::Add);
        let json = serde_json::to_string(&k).expect("serialize");
        assert!(json.contains("\"type\"") && json.contains("\"computation\""));
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }

    #[test]
    fn computation_kind_comparison_round_trip() {
        let k = ComputationKind::Comparison(ComparisonComputation::GreaterThan);
        let json = serde_json::to_string(&k).expect("serialize");
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }

    #[test]
    fn computation_kind_logical_round_trip() {
        let k = ComputationKind::Logical(LogicalComputation::And);
        let json = serde_json::to_string(&k).expect("serialize");
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }

    #[test]
    fn computation_kind_mathematical_round_trip() {
        let k = ComputationKind::Mathematical(MathematicalComputation::Sqrt);
        let json = serde_json::to_string(&k).expect("serialize");
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }
}
