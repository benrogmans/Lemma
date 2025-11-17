use crate::{
    ArithmeticComputation, ComparisonComputation, ExpressionId, LiteralValue, LogicalComputation,
    MathematicalComputation, OperationResult,
};
use serde::Serialize;

/// A fact with its name and optional value
#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub name: String,
    pub value: Option<LiteralValue>,
    /// If this fact is a document reference, contains the referenced document name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_reference: Option<String>,
}

/// Response from evaluating a Lemma document
///
/// Contains the results of evaluating all rules in a document,
/// including their computed values.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub doc_name: String,
    pub facts: Vec<Fact>,
    pub results: Vec<RuleResult>,
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
///
/// Represents one operation performed during rule evaluation,
/// capturing the actual values and decisions made during execution.
///
/// Operations are stored as a flat chronological trace in execution order.
/// Tree structure for proofs is derived from the Expression AST, not from these records.
#[derive(Debug, Clone, Serialize)]
pub struct OperationRecord {
    /// Expression ID for direct lookup of the Expression AST node
    pub expression_id: ExpressionId,
    #[serde(flatten)]
    pub kind: OperationKind,
}

/// The kind of operation performed
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationKind {
    FactUsed {
        fact_ref: crate::FactReference,
        value: LiteralValue,
    },
    RuleUsed {
        rule_ref: crate::RuleReference,
        rule_path: crate::RulePath,
        result: OperationResult,
    },
    Computation {
        kind: ComputationKind,
        inputs: Vec<LiteralValue>,
        result: LiteralValue,
        /// The original expression as written in source
        #[serde(skip_serializing_if = "Option::is_none", default)]
        expr: Option<String>,
    },
    RuleBranchEvaluated {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        matched: bool,
        /// The condition expression as written in source
        #[serde(skip_serializing_if = "Option::is_none", default)]
        condition_expr: Option<String>,
        /// The result expression as written in source
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_expr: Option<String>,
        /// The result value - only present for matched branches (None for non-matched branches)
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_value: Option<OperationResult>,
    },
}

/// Result of evaluating a single rule
///
/// Represents the outcome of evaluating one rule, including
/// whether it matched and what value it produced.
#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    pub rule: crate::LemmaRule,
    pub result: OperationResult,
    pub facts: Vec<Fact>,
    pub operations: Vec<OperationRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<crate::proof::Proof>,
}

impl OperationRecord {
    /// Create a new operation record
    pub fn new(expression_id: ExpressionId, kind: OperationKind) -> Self {
        OperationRecord {
            expression_id,
            kind,
        }
    }
}

impl Response {
    pub fn add_result(&mut self, result: RuleResult) {
        self.results.push(result);
    }

    pub fn filter_rules(&mut self, rule_names: &[String]) {
        self.results.retain(|r| rule_names.contains(&r.rule.name));
    }
}
