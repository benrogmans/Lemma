use std::collections::HashMap;

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
        .run(
            None,
            spec_name,
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
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
uses lemma si
data duration: 60 minutes
rule to_hours: duration as hours
"#;
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();

    let result = get_rule_result(&mut engine, "test", "to_hours");

    let val = result
        .value()
        .expect("Expected value result, got veto")
        .clone();

    if let ValueKind::Quantity(value, unit, _) = &val.value {
        assert!(
            unit.is_empty() || unit.to_ascii_lowercase().contains("hour"),
            "unit={unit:?}"
        );
        // 60 minutes = 1 hour (the conversion returns 1 hour, not 3600 seconds)
        assert_eq!(
            *value,
            lemma::RationalInteger::new(1, 1),
            "minutes to hours conversion failed: got {}",
            value
        );
    } else {
        panic!(
            "to_hours should be a Quantity after conversion, got {:?}",
            val
        );
    }
}
