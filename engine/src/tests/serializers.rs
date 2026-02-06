use crate::evaluation::response::{Response, RuleResult};
use crate::{Expression, ExpressionKind, LemmaRule, LiteralValue, OperationResult};
use indexmap::IndexMap;
use rust_decimal::Decimal;
use std::str::FromStr;

fn dummy_rule(name: &str) -> LemmaRule {
    use crate::parsing::ast::Span;
    use crate::parsing::Source;
    let src = Source::new(
        "<test>",
        Span {
            start: 0,
            end: 0,
            line: 1,
            col: 1,
        },
        "test_doc",
    );
    LemmaRule {
        name: name.to_string(),
        expression: Expression {
            kind: ExpressionKind::Literal(LiteralValue::boolean(crate::BooleanValue::True)),
            source_location: src.clone(),
        },
        unless_clauses: vec![],
        source_location: src,
    }
}

#[test]
fn test_response_serialization() {
    let mut results = IndexMap::new();
    results.insert(
        "test_rule".to_string(),
        RuleResult {
            rule: dummy_rule("test_rule"),
            result: OperationResult::Value(LiteralValue::number(Decimal::from_str("42").unwrap())),
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
            rule: dummy_rule("rule1"),
            result: OperationResult::Value(LiteralValue::boolean(crate::BooleanValue::True)),
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
            result: OperationResult::Value(LiteralValue::boolean(crate::BooleanValue::False)),
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
        result: OperationResult::Value(LiteralValue::boolean(crate::BooleanValue::True)),
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
            source: crate::Source::new(
                "",
                crate::Span {
                    start: 0,
                    end: 0,
                    line: 0,
                    col: 0,
                },
                "",
            ),
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
