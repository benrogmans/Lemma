use crate::response::{Response, RuleResult};
use crate::LiteralValue;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_response_serialization() {
    let mut response = Response::new("test_doc".to_string());

    let literal = LiteralValue::Number(Decimal::from_str("42").unwrap());
    let result = RuleResult::success("test_rule".to_string(), literal, HashMap::new());
    response.add_result(result);

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("test_doc"));
    assert!(json.contains("test_rule"));
    assert!(json.contains("results"));
}

#[test]
fn test_response_with_warnings() {
    let mut response = Response::new("test_doc".to_string());
    response.add_warning("Test warning".to_string());

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("Test warning"));
    assert!(json.contains("warnings"));
}

#[test]
fn test_response_filter_rules() {
    let mut response = Response::new("test_doc".to_string());

    let literal1 = LiteralValue::Boolean(true);
    let literal2 = LiteralValue::Boolean(false);

    response.add_result(RuleResult::success(
        "rule1".to_string(),
        literal1,
        HashMap::new(),
    ));
    response.add_result(RuleResult::success(
        "rule2".to_string(),
        literal2,
        HashMap::new(),
    ));

    response.filter_rules(&["rule1".to_string()]);

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].rule_name, "rule1");
}

#[test]
fn test_rule_result_types() {
    let literal = LiteralValue::Boolean(true);

    let success = RuleResult::success("rule1".to_string(), literal.clone(), HashMap::new());
    assert!(success.result.is_some());
    assert!(success.veto_message.is_none());

    let no_match = RuleResult::no_match("rule2".to_string());
    assert!(no_match.result.is_none());

    let missing = RuleResult::missing_facts("rule3".to_string(), vec!["fact1".to_string()]);
    assert_eq!(missing.missing_facts, Some(vec!["fact1".to_string()]));

    let veto = RuleResult::veto("rule4".to_string(), Some("Vetoed".to_string()));
    assert_eq!(veto.veto_message, Some("Vetoed".to_string()));
}
