use crate::evaluation::response::{EvaluatedRule, Response, RuleResult};
use crate::planning::semantics::{
    Expression, ExpressionKind, LiteralValue, RulePath, Source, Span,
};
use crate::OperationResult;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;

fn dummy_source() -> Source {
    Source::new(
        "test.lemma",
        Span {
            start: 0,
            end: 0,
            line: 1,
            col: 1,
        },
        "test_doc",
        Arc::from("doc test_doc\nrule dummy: true"),
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
            facts: vec![],
            operations: vec![],
            proof: None,
            rule_type: crate::planning::semantics::primitive_number().clone(),
        },
    );
    let response = Response {
        doc_name: "test_doc".to_string(),
        facts: vec![],
        results,
    };

    let json = serde_json::to_string(&response).unwrap();
    let deserialized: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized["doc_name"], "test_doc");
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
            facts: vec![],
            operations: vec![],
            proof: None,
            rule_type: crate::planning::semantics::primitive_boolean().clone(),
        },
    );
    results.insert(
        "rule2".to_string(),
        RuleResult {
            rule: dummy_rule("rule2"),
            result: OperationResult::Value(Box::new(LiteralValue::from_bool(false))),
            facts: vec![],
            operations: vec![],
            proof: None,
            rule_type: crate::planning::semantics::primitive_boolean().clone(),
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
        rule: dummy_rule("rule1"),
        result: OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
        facts: vec![],
        operations: vec![],
        proof: None,
        rule_type: crate::planning::semantics::primitive_boolean().clone(),
    };
    assert!(matches!(success.result, OperationResult::Value(_)));

    let missing = RuleResult {
        rule: dummy_rule("rule3"),
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
        rule_type: crate::planning::LemmaType::veto_type(),
    };
    assert_eq!(missing.facts.len(), 1);
    assert_eq!(missing.facts[0].path.fact, "fact1");
    assert!(matches!(missing.result, OperationResult::Veto(_)));

    let veto = RuleResult {
        rule: dummy_rule("rule4"),
        result: OperationResult::Veto(Some("Vetoed".to_string())),
        facts: vec![],
        operations: vec![],
        proof: None,
        rule_type: crate::planning::LemmaType::veto_type(),
    };
    assert_eq!(
        veto.result,
        OperationResult::Veto(Some("Vetoed".to_string()))
    );
}
