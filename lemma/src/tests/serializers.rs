use crate::response::{Fact, Response, RuleResult};
use crate::{Expression, ExpressionKind, LemmaRule, LiteralValue};
use rust_decimal::Decimal;
use std::str::FromStr;

fn dummy_rule(name: &str) -> LemmaRule {
    use crate::ast::ExpressionId;
    LemmaRule {
        name: name.to_string(),
        expression: Expression {
            kind: ExpressionKind::Literal(LiteralValue::Boolean(true)),
            span: None,
            id: ExpressionId::new(0),
        },
        unless_clauses: vec![],
        span: None,
    }
}

#[test]
fn test_response_serialization() {
    let response = Response {
        doc_name: "test_doc".to_string(),
        facts: vec![],
        results: vec![RuleResult {
            rule: dummy_rule("test_rule"),
            result: Some(LiteralValue::Number(Decimal::from_str("42").unwrap())),
            facts: vec![],
            veto_message: None,
            operations: vec![],
        }],
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("test_doc"));
    assert!(json.contains("test_rule"));
    assert!(json.contains("results"));
}

#[test]
fn test_response_filter_rules() {
    let mut response = Response {
        doc_name: "test_doc".to_string(),
        facts: vec![],
        results: vec![
            RuleResult {
                rule: dummy_rule("rule1"),
                result: Some(LiteralValue::Boolean(true)),
                facts: vec![],
                veto_message: None,
                operations: vec![],
            },
            RuleResult {
                rule: dummy_rule("rule2"),
                result: Some(LiteralValue::Boolean(false)),
                facts: vec![],
                veto_message: None,
                operations: vec![],
            },
        ],
    };

    response.filter_rules(&["rule1".to_string()]);

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].rule.name, "rule1");
}

#[test]
fn test_rule_result_types() {
    let success = RuleResult {
        rule: dummy_rule("rule1"),
        result: Some(LiteralValue::Boolean(true)),
        facts: vec![],
        veto_message: None,
        operations: vec![],
    };
    assert!(success.result.is_some());
    assert!(success.veto_message.is_none());

    let no_match = RuleResult {
        rule: dummy_rule("rule2"),
        result: None,
        facts: vec![],
        veto_message: None,
        operations: vec![],
    };
    assert!(no_match.result.is_none());

    let missing = RuleResult {
        rule: dummy_rule("rule3"),
        result: None,
        facts: vec![Fact {
            name: "fact1".to_string(),
            value: None,
        }],
        veto_message: None,
        operations: vec![],
    };
    assert_eq!(missing.facts.len(), 1);
    assert_eq!(missing.facts[0].name, "fact1");
    assert!(missing.facts[0].value.is_none());
    assert!(missing.veto_message.is_none());

    let veto = RuleResult {
        rule: dummy_rule("rule4"),
        result: None,
        facts: vec![],
        veto_message: Some("Vetoed".to_string()),
        operations: vec![],
    };
    assert_eq!(veto.veto_message, Some("Vetoed".to_string()));
}
