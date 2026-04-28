use rust_decimal::Decimal;
use std::{collections::HashMap, str::FromStr};

use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, ValueKind};

/// Get the result of a rule evaluation.
/// Panics if the rule is not found (test failure).
/// Returns the OperationResult which must be checked explicitly.
fn get_rule_result(
    engine: &mut Engine,
    spec_name: &str,
    rule_name: &str,
) -> lemma::OperationResult {
    let now = DateTimeValue::now();
    let response = engine
        .run(spec_name, Some(&now), HashMap::new(), false)
        .unwrap();
    response
        .results
        .values()
        .find(|r| r.rule.name == rule_name)
        .map(|r| r.result.clone())
        .unwrap_or_else(|| panic!("Rule '{}' not found in spec '{}'", rule_name, spec_name))
}

#[test]
fn test_duration_conversion_properties() {
    let mut engine = Engine::new();
    let code = r#"
spec test
data duration: 60 minutes
rule to_hours: duration in hours
"#;
    engine
        .load(code, lemma::SourceType::Labeled("test"))
        .unwrap();

    let result = get_rule_result(&mut engine, "test", "to_hours");

    let val = result
        .value()
        .expect("Expected value result, got veto")
        .clone();

    if let ValueKind::Duration(value, _) = &val.value {
        // 60 minutes = 1 hour (the conversion returns 1 hour, not 3600 seconds)
        assert_eq!(
            *value,
            Decimal::from_str("1").unwrap(),
            "minutes to hours conversion failed: got {}",
            value
        );
    } else {
        panic!(
            "to_hours should be a Duration after conversion, got {:?}",
            val
        );
    }
}
