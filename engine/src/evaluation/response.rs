use crate::evaluation::operations::{OperationRecord, OperationResult};
use crate::planning::semantics::{Expression, Fact, LemmaType, RulePath, Source};
use indexmap::IndexMap;
use serde::Serialize;

/// Rule info with resolved expressions for use in evaluation response.
/// Evaluation uses only semantics types; no parsing types.
#[derive(Debug, Clone, Serialize)]
pub struct EvaluatedRule {
    pub name: String,
    pub path: RulePath,
    pub default_expression: Expression,
    pub unless_branches: Vec<(Option<Expression>, Expression)>,
    pub source_location: Source,
    pub rule_type: LemmaType,
}

/// Facts from a specific document (semantics types only).
#[derive(Debug, Clone, Serialize)]
pub struct Facts {
    pub fact_path: String,
    pub referencing_fact_name: String,
    pub facts: Vec<Fact>,
}

/// Response from evaluating a Lemma document
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub doc_name: String,
    pub facts: Vec<Facts>,
    pub results: IndexMap<String, RuleResult>,
}

/// Result of evaluating a single rule (semantics types only).
#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    #[serde(skip_serializing)]
    pub rule: EvaluatedRule,
    pub result: OperationResult,
    pub facts: Vec<Fact>,
    #[serde(skip_serializing)]
    pub operations: Vec<OperationRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<crate::evaluation::proof::Proof>,
    /// Computed type of this rule's result (semantics).
    pub rule_type: LemmaType,
}

impl Response {
    pub fn add_result(&mut self, result: RuleResult) {
        self.results.insert(result.rule.name.clone(), result);
    }

    pub fn filter_rules(&mut self, rule_names: &[String]) {
        self.results.retain(|name, _| rule_names.contains(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::semantics::{
        primitive_boolean, primitive_number, Expression, ExpressionKind, LemmaType, LiteralValue,
        RulePath, Span,
    };
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dummy_source() -> Source {
        Source::new(
            "test",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 1,
            },
            "test_doc",
            std::sync::Arc::from("doc test_doc\nfact x = 1\nrule result = x"),
        )
    }

    fn dummy_evaluated_rule(name: &str) -> EvaluatedRule {
        EvaluatedRule {
            name: name.to_string(),
            path: RulePath::new(vec![], name.to_string()),
            default_expression: Expression::new(
                ExpressionKind::Literal(Box::new(LiteralValue::from_bool(true))),
                dummy_source(),
            ),
            unless_branches: vec![],
            source_location: dummy_source(),
            rule_type: primitive_number().clone(),
        }
    }

    #[test]
    fn test_response_serialization() {
        let mut results = IndexMap::new();
        results.insert(
            "test_rule".to_string(),
            RuleResult {
                rule: dummy_evaluated_rule("test_rule"),
                result: OperationResult::Value(Box::new(LiteralValue::number(
                    Decimal::from_str("42").unwrap(),
                ))),
                facts: vec![],
                operations: vec![],
                proof: None,
                rule_type: primitive_number().clone(),
            },
        );
        let response = Response {
            doc_name: "test_doc".to_string(),
            facts: vec![],
            results,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test_doc"));
        assert!(json.contains("test_rule"));
        assert!(json.contains("results"));
    }

    #[test]
    fn test_response_filter_rules() {
        let mut results = IndexMap::new();
        results.insert(
            "rule1".to_string(),
            RuleResult {
                rule: dummy_evaluated_rule("rule1"),
                result: OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
                facts: vec![],
                operations: vec![],
                proof: None,
                rule_type: primitive_boolean().clone(),
            },
        );
        results.insert(
            "rule2".to_string(),
            RuleResult {
                rule: dummy_evaluated_rule("rule2"),
                result: OperationResult::Value(Box::new(LiteralValue::from_bool(false))),
                facts: vec![],
                operations: vec![],
                proof: None,
                rule_type: primitive_boolean().clone(),
            },
        );
        let mut response = Response {
            doc_name: "test_doc".to_string(),
            facts: vec![],
            results,
        };

        response.filter_rules(&["rule1".to_string()]);

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results.values().next().unwrap().rule.name, "rule1");
    }

    #[test]
    fn test_rule_result_types() {
        let success = RuleResult {
            rule: dummy_evaluated_rule("rule1"),
            result: OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
            facts: vec![],
            operations: vec![],
            proof: None,
            rule_type: primitive_boolean().clone(),
        };
        assert!(matches!(success.result, OperationResult::Value(_)));

        let missing = RuleResult {
            rule: dummy_evaluated_rule("rule3"),
            result: OperationResult::Veto(Some("Missing fact: fact1".to_string())),
            facts: vec![crate::planning::semantics::Fact {
                path: crate::planning::semantics::FactPath::new(vec![], "fact1".to_string()),
                value: crate::planning::semantics::FactValue::Literal(
                    crate::planning::semantics::LiteralValue::from_bool(false),
                ),
                source: None,
            }],
            operations: vec![],
            proof: None,
            rule_type: LemmaType::veto_type(),
        };
        assert_eq!(missing.facts.len(), 1);
        assert_eq!(missing.facts[0].path.fact, "fact1");
        assert!(matches!(missing.result, OperationResult::Veto(_)));

        let veto = RuleResult {
            rule: dummy_evaluated_rule("rule4"),
            result: OperationResult::Veto(Some("Vetoed".to_string())),
            facts: vec![],
            operations: vec![],
            proof: None,
            rule_type: LemmaType::veto_type(),
        };
        assert_eq!(
            veto.result,
            OperationResult::Veto(Some("Vetoed".to_string()))
        );
    }
}
