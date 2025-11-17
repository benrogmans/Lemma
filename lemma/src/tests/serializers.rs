use crate::response::{Fact, Response, RuleResult};
use crate::{Expression, ExpressionKind, LemmaRule, LiteralValue, OperationResult};
use rust_decimal::Decimal;
use std::str::FromStr;

fn dummy_rule(name: &str) -> LemmaRule {
    use crate::ast::ExpressionId;
    LemmaRule {
        name: name.to_string(),
        expression: Expression {
            kind: ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::True)),
            source_location: None,
            id: ExpressionId::new(0),
        },
        unless_clauses: vec![],
        source_location: None,
    }
}

#[test]
fn test_response_serialization() {
    let response = Response {
        doc_name: "test_doc".to_string(),
        facts: vec![],
        results: vec![RuleResult {
            rule: dummy_rule("test_rule"),
            result: OperationResult::Value(LiteralValue::Number(Decimal::from_str("42").unwrap())),
            facts: vec![],
            operations: vec![],
            proof: None,
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
                result: OperationResult::Value(LiteralValue::Boolean(crate::BooleanValue::True)),
                facts: vec![],
                operations: vec![],
                proof: None,
            },
            RuleResult {
                rule: dummy_rule("rule2"),
                result: OperationResult::Value(LiteralValue::Boolean(crate::BooleanValue::False)),
                facts: vec![],
                operations: vec![],
                proof: None,
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
        result: OperationResult::Value(LiteralValue::Boolean(crate::BooleanValue::True)),
        facts: vec![],
        operations: vec![],
        proof: None,
    };
    assert!(matches!(success.result, OperationResult::Value(_)));

    let missing = RuleResult {
        rule: dummy_rule("rule3"),
        result: OperationResult::Veto(Some("Missing fact: fact1".to_string())),
        facts: vec![Fact {
            name: "fact1".to_string(),
            value: None,
            document_reference: None,
        }],
        operations: vec![],
        proof: None,
    };
    assert_eq!(missing.facts.len(), 1);
    assert_eq!(missing.facts[0].name, "fact1");
    assert!(missing.facts[0].value.is_none());
    assert!(matches!(missing.result, OperationResult::Veto(_)));

    let veto = RuleResult {
        rule: dummy_rule("rule4"),
        result: OperationResult::Veto(Some("Vetoed".to_string())),
        facts: vec![],
        operations: vec![],
        proof: None,
    };
    assert_eq!(
        veto.result,
        OperationResult::Veto(Some("Vetoed".to_string()))
    );
}
