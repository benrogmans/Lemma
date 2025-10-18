use crate::LiteralValue;
use serde::Serialize;
use std::collections::HashMap;

/// Response from evaluating a Lemma document
///
/// Contains the results of evaluating all rules in a document,
/// including their computed values and any variable bindings.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub doc_name: String,
    pub results: Vec<RuleResult>,
    pub warnings: Vec<String>,
}

/// A record of a single operation during evaluation
///
/// Represents one operation performed during rule evaluation,
/// capturing the actual values and decisions made during execution.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationRecord {
    FactUsed {
        name: String,
        value: LiteralValue,
    },
    RuleUsed {
        name: String,
        value: LiteralValue,
    },
    OperationExecuted {
        operation: String,
        inputs: Vec<LiteralValue>,
        result: LiteralValue,
        unless_clause_index: Option<usize>,
    },
    UnlessClauseEvaluated {
        index: usize,
        matched: bool,
        result_if_matched: Option<LiteralValue>,
    },
    DefaultValue {
        value: LiteralValue,
    },
    FinalResult {
        value: LiteralValue,
    },
}

/// Result of evaluating a single rule
///
/// Represents the outcome of evaluating one rule, including
/// whether it matched, what value it produced, and any variable bindings.
#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    pub rule_name: String,
    pub result: Option<LiteralValue>,
    pub bindings: HashMap<String, LiteralValue>,
    pub missing_facts: Option<Vec<String>>,
    pub veto_message: Option<String>,
    pub operations: Vec<OperationRecord>,
}

impl Response {
    pub fn new(doc_name: String) -> Self {
        Self {
            doc_name,
            results: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn add_result(&mut self, result: RuleResult) {
        self.results.push(result);
    }

    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    /// Filter results to only include specified rules
    ///
    /// Keeps only the rules whose names are in the provided list.
    /// This is used when evaluating specific rules (e.g., `doc:rule1,rule2`)
    pub fn filter_rules(&mut self, rule_names: &[String]) {
        self.results.retain(|r| rule_names.contains(&r.rule_name));
    }
}

impl RuleResult {
    pub fn success(
        rule_name: String,
        result: LiteralValue,
        bindings: HashMap<String, LiteralValue>,
    ) -> Self {
        Self {
            rule_name,
            result: Some(result),
            bindings,
            missing_facts: None,
            veto_message: None,
            operations: Vec::new(),
        }
    }

    pub fn success_with_operations(
        rule_name: String,
        result: LiteralValue,
        bindings: HashMap<String, LiteralValue>,
        operations: Vec<OperationRecord>,
    ) -> Self {
        Self {
            rule_name,
            result: Some(result),
            bindings,
            missing_facts: None,
            veto_message: None,
            operations,
        }
    }

    pub fn no_match(rule_name: String) -> Self {
        Self {
            rule_name,
            result: None,
            bindings: HashMap::new(),
            missing_facts: None,
            veto_message: None,
            operations: Vec::new(),
        }
    }

    pub fn missing_facts(rule_name: String, facts: Vec<String>) -> Self {
        Self {
            rule_name,
            result: None,
            bindings: HashMap::new(),
            missing_facts: Some(facts),
            veto_message: None,
            operations: Vec::new(),
        }
    }

    pub fn veto(rule_name: String, message: Option<String>) -> Self {
        Self {
            rule_name,
            result: None,
            bindings: HashMap::new(),
            missing_facts: None,
            veto_message: message,
            operations: Vec::new(),
        }
    }
}
