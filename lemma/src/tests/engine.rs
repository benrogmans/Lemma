use crate::engine::Engine;
use rust_decimal::Decimal;
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

    let response = engine.evaluate("test", None, None).unwrap();
    assert_eq!(response.results.len(), 2);

    let sum_result = response
        .results
        .iter()
        .find(|r| r.rule.name == "sum")
        .unwrap();
    assert_eq!(
        sum_result.result,
        Some(crate::LiteralValue::Number(
            Decimal::from_str("15").unwrap()
        ))
    );

    let product_result = response
        .results
        .iter()
        .find(|r| r.rule.name == "product")
        .unwrap();
    assert_eq!(
        product_result.result,
        Some(crate::LiteralValue::Number(
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

    let response = engine.evaluate("test", None, None).unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(
        response.results[0].result,
        Some(crate::LiteralValue::Number(
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

    let response = engine.evaluate("test", None, None).unwrap();
    assert_eq!(
        response.results[0].result,
        Some(crate::LiteralValue::Boolean(true))
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

    let response = engine.evaluate("test", None, None).unwrap();
    assert_eq!(
        response.results[0].result,
        Some(crate::LiteralValue::Number(
            Decimal::from_str("10").unwrap()
        ))
    );
}

#[test]
fn test_document_not_found() {
    let engine = Engine::new();
    let result = engine.evaluate("nonexistent", None, None);
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

    let response1 = engine.evaluate("doc1", None, None).unwrap();
    assert_eq!(
        response1.results[0].result,
        Some(crate::LiteralValue::Number(
            Decimal::from_str("20").unwrap()
        ))
    );

    let response2 = engine.evaluate("doc2", None, None).unwrap();
    assert_eq!(
        response2.results[0].result,
        Some(crate::LiteralValue::Number(
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

    let result = engine.evaluate("test", None, None);
    // Division by zero returns an error from the evaluator
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Division by zero"));
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

    let response = engine.evaluate("test", None, None).unwrap();
    assert_eq!(response.results.len(), 3);

    // Check they all have span information for ordering
    for result in &response.results {
        assert!(
            result.rule.span.is_some(),
            "Rule {} missing span",
            result.rule.name
        );
    }

    // Verify source positions increase (z < y < x)
    let z_pos = response
        .results
        .iter()
        .find(|r| r.rule.name == "z")
        .unwrap()
        .rule
        .span
        .as_ref()
        .unwrap()
        .start;
    let y_pos = response
        .results
        .iter()
        .find(|r| r.rule.name == "y")
        .unwrap()
        .rule
        .span
        .as_ref()
        .unwrap()
        .start;
    let x_pos = response
        .results
        .iter()
        .find(|r| r.rule.name == "x")
        .unwrap()
        .rule
        .span
        .as_ref()
        .unwrap()
        .start;

    assert!(z_pos < y_pos);
    assert!(y_pos < x_pos);
}
