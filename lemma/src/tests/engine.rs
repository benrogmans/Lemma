use crate::engine::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_evaluate_document_all_rules() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact x = 10
        fact y = 5
        rule sum = x + y
        rule product = x * y
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    assert_eq!(response.results.len(), 2);

    let sum_result = response
        .results
        .values()
        .find(|r| r.rule.name == "sum")
        .unwrap();
    assert_eq!(
        sum_result.result,
        crate::OperationResult::Value(crate::LiteralValue::Number(
            Decimal::from_str("15").unwrap()
        ))
    );

    let product_result = response
        .results
        .values()
        .find(|r| r.rule.name == "product")
        .unwrap();
    assert_eq!(
        product_result.result,
        crate::OperationResult::Value(crate::LiteralValue::Number(
            Decimal::from_str("50").unwrap()
        ))
    );
}

#[test]
fn test_evaluate_empty_facts() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact price = 100
        rule total = price * 2
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(
        response.results.values().next().unwrap().result,
        crate::OperationResult::Value(crate::LiteralValue::Number(
            Decimal::from_str("200").unwrap()
        ))
    );
}

#[test]
fn test_evaluate_boolean_rule() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact age = 25
        rule is_adult = age >= 18
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    assert_eq!(
        response.results.values().next().unwrap().result,
        crate::OperationResult::Value(crate::LiteralValue::Boolean(crate::BooleanValue::True))
    );
}

#[test]
fn test_evaluate_with_unless_clause() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact quantity = 15
        rule discount = 0
          unless quantity >= 10 then 10
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    assert_eq!(
        response.results.values().next().unwrap().result,
        crate::OperationResult::Value(crate::LiteralValue::Number(
            Decimal::from_str("10").unwrap()
        ))
    );
}

#[test]
fn test_document_not_found() {
    let engine = Engine::new();
    let result = engine.evaluate("nonexistent", vec![], HashMap::new());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn test_multiple_documents() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc doc1
        fact x = 10
        rule result = x * 2
    "#,
            "doc1.lemma",
        )
        .unwrap();

    engine
        .add_lemma_code(
            r#"
        doc doc2
        fact y = 5
        rule result = y * 3
    "#,
            "doc2.lemma",
        )
        .unwrap();

    let response1 = engine.evaluate("doc1", vec![], HashMap::new()).unwrap();
    assert_eq!(
        response1.results[0].result,
        crate::OperationResult::Value(crate::LiteralValue::Number(
            Decimal::from_str("20").unwrap()
        ))
    );

    let response2 = engine.evaluate("doc2", vec![], HashMap::new()).unwrap();
    assert_eq!(
        response2.results[0].result,
        crate::OperationResult::Value(crate::LiteralValue::Number(
            Decimal::from_str("15").unwrap()
        ))
    );
}

#[test]
fn test_runtime_error_mapping() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact numerator = 10
        fact denominator = 0
        rule division = numerator / denominator
    "#,
            "test.lemma",
        )
        .unwrap();

    let result = engine.evaluate("test", vec![], HashMap::new());
    // Division by zero returns a Veto (not an error) in the new evaluation design
    assert!(result.is_ok(), "Evaluation should succeed");
    let response = result.unwrap();
    let division_result = response
        .results
        .values()
        .find(|r| r.rule.name == "division");
    assert!(
        division_result.is_some(),
        "Should have division rule result"
    );
    match &division_result.unwrap().result {
        crate::OperationResult::Veto(message) => {
            assert!(
                message
                    .as_ref()
                    .map(|m| m.contains("Division by zero"))
                    .unwrap_or(false),
                "Veto message should mention division by zero: {:?}",
                message
            );
        }
        other => panic!("Expected Veto for division by zero, got {:?}", other),
    }
}

#[test]
fn test_rules_sorted_by_source_order() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact a = 1
        fact b = 2
        rule z = a + b
        rule y = a * b
        rule x = a - b
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    assert_eq!(response.results.len(), 3);

    // Check they all have span information for ordering
    for result in response.results.values() {
        assert!(
            result.rule.source_location.is_some(),
            "Rule {} missing source_location",
            result.rule.name
        );
    }

    // Verify source positions increase (z < y < x)
    let z_pos = response
        .results
        .values()
        .find(|r| r.rule.name == "z")
        .unwrap()
        .rule
        .source_location
        .as_ref()
        .unwrap()
        .span
        .start;
    let y_pos = response
        .results
        .values()
        .find(|r| r.rule.name == "y")
        .unwrap()
        .rule
        .source_location
        .as_ref()
        .unwrap()
        .span
        .start;
    let x_pos = response
        .results
        .values()
        .find(|r| r.rule.name == "x")
        .unwrap()
        .rule
        .source_location
        .as_ref()
        .unwrap()
        .span
        .start;

    assert!(z_pos < y_pos);
    assert!(y_pos < x_pos);
}

#[test]
fn test_rule_filtering_evaluates_dependencies() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact base = 100
        rule subtotal = base * 2
        rule tax = subtotal? * 10%
        rule total = subtotal? + tax?
    "#,
            "test.lemma",
        )
        .unwrap();

    // Request only 'total', but it depends on 'subtotal' and 'tax'
    let response = engine
        .evaluate("test", vec!["total".to_string()], HashMap::new())
        .unwrap();

    // Only 'total' should be in results
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results.keys().next().unwrap(), "total");

    // But the value should be correct (dependencies were computed)
    let total = response.results.values().next().unwrap();
    assert_eq!(
        total.result,
        crate::OperationResult::Value(crate::LiteralValue::Number(
            Decimal::from_str("220").unwrap()
        ))
    );
}
