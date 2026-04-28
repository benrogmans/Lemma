use crate::evaluation::response::{EvaluatedRule, Response, RuleResult};
use crate::planning::semantics::{
    Expression, ExpressionKind, LiteralValue, RulePath, Source, Span,
};
use crate::OperationResult;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use std::str::FromStr;

fn dummy_source() -> Source {
    Source::new(
        "test.lemma",
        Span {
            start: 0,
            end: 0,
            line: 1,
            col: 1,
        },
    )
}

fn dummy_rule(name: &str) -> EvaluatedRule {
    EvaluatedRule {
        name: name.to_string(),
        path: RulePath::new(vec![], name.to_string()),
        default_expression: Expression::new(
            ExpressionKind::Literal(Box::new(LiteralValue::from_bool(true))),
            dummy_source(),
        ),
        unless_branches: vec![],
        source_location: dummy_source(),
        rule_type: crate::planning::semantics::primitive_boolean().clone(),
    }
}

#[test]
fn test_response_serialization() {
    let mut results = IndexMap::new();
    results.insert(
        "test_rule".to_string(),
        RuleResult {
            rule: dummy_rule("test_rule"),
            result: OperationResult::Value(Box::new(LiteralValue::number(
                Decimal::from_str("42").unwrap(),
            ))),
            data: vec![],
            operations: vec![],
            explanation: None,
            rule_type: crate::planning::semantics::primitive_number().clone(),
        },
    );
    let response = Response {
        spec_name: "test_spec".to_string(),
        spec_hash: None,
        spec_effective_from: None,
        spec_effective_to: None,
        data: vec![],
        results,
    };

    let json = serde_json::to_string(&response).unwrap();
    let deserialized: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized["spec_name"], "test_spec");
    assert!(deserialized["results"]
        .as_object()
        .unwrap()
        .contains_key("test_rule"));
    assert_eq!(
        deserialized["results"]["test_rule"]["result"]["value"]["display_value"],
        "42"
    );
}

#[test]
fn test_response_filter_rules() {
    let mut results = IndexMap::new();
    results.insert(
        "rule1".to_string(),
        RuleResult {
            rule: dummy_rule("rule1"),
            result: OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
            data: vec![],
            operations: vec![],
            explanation: None,
            rule_type: crate::planning::semantics::primitive_boolean().clone(),
        },
    );
    results.insert(
        "rule2".to_string(),
        RuleResult {
            rule: dummy_rule("rule2"),
            result: OperationResult::Value(Box::new(LiteralValue::from_bool(false))),
            data: vec![],
            operations: vec![],
            explanation: None,
            rule_type: crate::planning::semantics::primitive_boolean().clone(),
        },
    );
    let mut response = Response {
        spec_name: "test_spec".to_string(),
        spec_hash: None,
        spec_effective_from: None,
        spec_effective_to: None,
        data: vec![],
        results,
    };

    response.filter_rules(&["rule1".to_string()]);

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results.values().next().unwrap().rule.name, "rule1");
}
