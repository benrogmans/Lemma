use crate::evaluation::response::{EvaluatedRule, Response, RuleResult};
use crate::planning::semantics::{LiteralValue, RulePath, Source, Span};
use crate::OperationResult;
use indexmap::IndexMap;
use rust_decimal::Decimal;

fn dummy_source() -> Source {
    Source::new(
        crate::parsing::source::SourceType::Volatile,
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
        source_location: dummy_source(),
        rule_type: crate::planning::semantics::primitive_boolean().clone(),
    }
}

fn number_rule_result(name: &str, value: Decimal) -> RuleResult {
    let expression_units = std::collections::HashMap::new();
    RuleResult::from_operation_result(
        dummy_rule(name),
        OperationResult::Value(Box::new(LiteralValue::number_from_decimal(value))),
        crate::planning::semantics::primitive_number(),
        &expression_units,
        None,
    )
}

#[test]
fn test_response_serialization() {
    let mut results = IndexMap::new();
    results.insert(
        "test_rule".to_string(),
        number_rule_result("test_rule", 42.into()),
    );
    let response = Response {
        spec_name: "test_spec".to_string(),
        effective: "2026-01-01".to_string(),
        spec_hash: None,
        spec_effective_from: None,
        spec_effective_to: None,
        data: vec![],
        results,
    };

    let json = serde_json::to_string(&response).unwrap();
    let deserialized: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized["spec"], "test_spec");
    assert!(deserialized["results"]
        .as_object()
        .unwrap()
        .contains_key("test_rule"));
    assert_eq!(deserialized["results"]["test_rule"]["display"], "42");
    assert_eq!(deserialized["results"]["test_rule"]["number"], "42");
    assert_eq!(deserialized["results"]["test_rule"]["vetoed"], false);
}

#[test]
fn response_number_json_is_scalar() {
    let mut results = IndexMap::new();
    results.insert(
        "double".to_string(),
        number_rule_result("double", 20.into()),
    );
    let response = Response {
        spec_name: "test_spec".to_string(),
        effective: "2026-01-01".to_string(),
        spec_hash: None,
        spec_effective_from: None,
        spec_effective_to: None,
        data: vec![],
        results,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    let number = &json["results"]["double"]["number"];
    assert!(!number.is_array());
    assert_eq!(number.as_str(), Some("20"));
}

#[test]
fn test_response_filter_rules() {
    let expression_units = std::collections::HashMap::new();
    let mut results = IndexMap::new();
    results.insert(
        "rule1".to_string(),
        RuleResult::from_operation_result(
            dummy_rule("rule1"),
            OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
            crate::planning::semantics::primitive_boolean(),
            &expression_units,
            None,
        ),
    );
    results.insert(
        "rule2".to_string(),
        RuleResult::from_operation_result(
            dummy_rule("rule2"),
            OperationResult::Value(Box::new(LiteralValue::from_bool(false))),
            crate::planning::semantics::primitive_boolean(),
            &expression_units,
            None,
        ),
    );
    let mut response = Response {
        spec_name: "test_spec".to_string(),
        effective: "2026-01-01".to_string(),
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
