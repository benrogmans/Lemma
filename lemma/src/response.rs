use crate::{ArithmeticComputation, ComparisonComputation, LiteralValue, MathematicalComputation};
use serde::Serialize;

/// Unique identifier for an operation record
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct OperationId(pub usize);

/// A fact with its name and optional value
#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub name: String,
    pub value: Option<LiteralValue>,
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
    Mathematical(MathematicalComputation),
}

/// A record of a single operation during evaluation
///
/// Represents one operation performed during rule evaluation,
/// capturing the actual values and decisions made during execution.
#[derive(Debug, Clone, Serialize)]
pub struct OperationRecord {
    pub id: OperationId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<OperationId>,
    pub depth: usize,
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
        value: LiteralValue,
    },
    Computation {
        kind: ComputationKind,
        inputs: Vec<LiteralValue>,
        result: LiteralValue,
        /// The original expression as written in source
        #[serde(skip_serializing_if = "Option::is_none", default)]
        expr: Option<String>,
    },
    UnlessClauseEvaluated {
        index: usize,
        matched: bool,
        result_if_matched: Option<LiteralValue>,
        /// The condition expression as written in source
        #[serde(skip_serializing_if = "Option::is_none", default)]
        condition_expr: Option<String>,
        /// The result expression as written in source
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_expr: Option<String>,
    },
    DefaultValue {
        value: LiteralValue,
        /// The default expression as written in source
        #[serde(skip_serializing_if = "Option::is_none", default)]
        expr: Option<String>,
    },
}

/// Result of evaluating a single rule
///
/// Represents the outcome of evaluating one rule, including
/// whether it matched and what value it produced.
#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    pub rule: crate::LemmaRule,
    pub result: Option<LiteralValue>,
    pub facts: Vec<Fact>,
    pub veto_message: Option<String>,
    pub operations: Vec<OperationRecord>,
}

impl OperationRecord {
    /// Create a copy of this operation with a new parent
    pub fn with_parent(&self, new_parent: OperationId) -> Self {
        OperationRecord {
            id: self.id,
            parent_id: Some(new_parent),
            depth: self.depth + 1,
            kind: self.kind.clone(),
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
