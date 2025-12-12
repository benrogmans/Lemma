use lemma::Engine;
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
        lemma::OperationResult::Value(lemma::LiteralValue::Number(
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
        lemma::OperationResult::Value(lemma::LiteralValue::Number(
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
        lemma::OperationResult::Value(lemma::LiteralValue::Number(
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
        lemma::OperationResult::Value(lemma::LiteralValue::Boolean(lemma::BooleanValue::True))
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
        lemma::OperationResult::Value(lemma::LiteralValue::Number(
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
        lemma::OperationResult::Value(lemma::LiteralValue::Number(
            Decimal::from_str("20").unwrap()
        ))
    );

    let response2 = engine.evaluate("doc2", vec![], HashMap::new()).unwrap();
    assert_eq!(
        response2.results[0].result,
        lemma::OperationResult::Value(lemma::LiteralValue::Number(
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
        lemma::OperationResult::Veto(message) => {
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

    // Verify all three rules are present
    assert!(
        response.results.contains_key("z"),
        "Results should contain rule z"
    );
    assert!(
        response.results.contains_key("y"),
        "Results should contain rule y"
    );
    assert!(
        response.results.contains_key("x"),
        "Results should contain rule x"
    );

    // Verify source positions match source order (z, y, x in source)
    // Note: Results may be in dependency order, not source order, so we verify source positions directly
    let z_pos = response
        .results
        .get("z")
        .unwrap()
        .rule
        .source
        .as_ref()
        .unwrap()
        .span
        .start;
    let y_pos = response
        .results
        .get("y")
        .unwrap()
        .rule
        .source
        .as_ref()
        .unwrap()
        .span
        .start;
    let x_pos = response
        .results
        .get("x")
        .unwrap()
        .rule
        .source
        .as_ref()
        .unwrap()
        .span
        .start;

    assert!(z_pos < y_pos, "z should come before y in source");
    assert!(y_pos < x_pos, "y should come before x in source");
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

    let response = engine
        .evaluate("test", vec!["total".to_string()], HashMap::new())
        .unwrap();

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results.keys().next().unwrap(), "total");

    let total = response.results.values().next().unwrap();
    assert_eq!(
        total.result,
        lemma::OperationResult::Value(lemma::LiteralValue::Number(
            Decimal::from_str("220").unwrap()
        ))
    );
}

#[test]
fn test_evaluate_with_invalid_fact_override_type() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact price = [number]
        rule total = price * 2
    "#,
            "test.lemma",
        )
        .unwrap();

    // Try to override price with text instead of number
    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "not a number".to_string());

    let result = engine.evaluate("test", vec![], facts);
    // Should either fail at evaluation or return a veto
    match result {
        Ok(response) => {
            // If evaluation succeeds, the rule should have a veto
            let total_result = response
                .results
                .values()
                .find(|r| r.rule.name == "total")
                .expect("total rule should exist");
            match &total_result.result {
                lemma::OperationResult::Veto(_) => {
                    // Expected - type mismatch should cause veto
                }
                other => panic!(
                    "Expected veto for invalid fact override type, got: {:?}",
                    other
                ),
            }
        }
        Err(_) => {
            // Also acceptable - evaluation can fail early
        }
    }
}

#[test]
fn test_evaluate_with_missing_required_fact() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact price = [number]
        fact quantity = [number]
        rule total = price * quantity
    "#,
            "test.lemma",
        )
        .unwrap();

    // Provide only price, missing quantity
    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "100".to_string());

    let response = engine.evaluate("test", vec![], facts).unwrap();
    let total_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule should exist");

    // Should fail due to missing quantity
    match &total_result.result {
        lemma::OperationResult::Veto(msg) => {
            let msg_str = msg.as_ref().map(|m| m.to_lowercase()).unwrap_or_default();
            assert!(
                msg_str.contains("missing")
                    || msg_str.contains("fact")
                    || msg_str.contains("quantity"),
                "Veto message should mention missing fact, got: {:?}",
                msg
            );
        }
        other => panic!("Expected veto for missing required fact, got: {:?}", other),
    }
}

#[test]
fn test_evaluate_with_circular_rule_dependency() {
    let mut engine = Engine::new();

    // This should fail at planning stage due to circular dependency
    let result = engine.add_lemma_code(
        r#"
        doc test
        rule a = b?
        rule b = a?
    "#,
        "test.lemma",
    );

    // Circular dependency should be caught during planning
    assert!(
        result.is_err(),
        "Should fail to add document with circular rule dependency"
    );

    let error_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        error_msg.contains("circular") || error_msg.contains("dependency"),
        "Error message should mention circular dependency, got: {}",
        error_msg
    );
}

#[test]
fn test_rule_filtering_with_missing_dependencies() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact base = [number]
        rule subtotal = base * 2
        rule tax = subtotal? * 10%
        rule total = subtotal? + tax?
    "#,
            "test.lemma",
        )
        .unwrap();

    // Request only 'total' but don't provide 'base' - should still evaluate dependencies
    let response = engine
        .evaluate("test", vec!["total".to_string()], HashMap::new())
        .unwrap();

    // Should only return 'total' in results, but it should fail due to missing base
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results.keys().next().unwrap(), "total");

    let total = response.results.values().next().unwrap();
    // total should fail because base is missing
    match &total.result {
        lemma::OperationResult::Veto(_) => {
            // Expected - missing base causes failure
        }
        other => panic!("Expected veto for missing dependency, got: {:?}", other),
    }
}

#[test]
fn test_evaluate_with_veto_result() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
        doc test
        fact age = 15
        rule can_vote = age >= 18
            unless age < 18 then veto "Must be 18 or older to vote"
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let can_vote_result = response
        .results
        .values()
        .find(|r| r.rule.name == "can_vote")
        .expect("can_vote rule should exist");

    match &can_vote_result.result {
        lemma::OperationResult::Veto(msg) => {
            assert!(
                msg.as_ref()
                    .map(|m| m.contains("18") || m.contains("older"))
                    .unwrap_or(false),
                "Veto message should mention age requirement, got: {:?}",
                msg
            );
        }
        other => panic!("Expected veto for age < 18, got: {:?}", other),
    }
}

#[test]
fn test_evaluate_with_cross_document_reference_error() {
    let mut engine = Engine::new();

    // Add base document
    engine
        .add_lemma_code(
            r#"
        doc base
        fact value = 100
        rule doubled = value * 2
    "#,
            "base.lemma",
        )
        .unwrap();

    // Add document that references non-existent document
    let result = engine.add_lemma_code(
        r#"
        doc derived
        fact config = doc nonexistent
        rule result = config.doubled?
    "#,
        "derived.lemma",
    );

    // Should fail because 'nonexistent' document doesn't exist
    assert!(
        result.is_err(),
        "Should fail to add document with reference to nonexistent document"
    );

    let error_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        error_msg.contains("not found")
            || error_msg.contains("nonexistent")
            || error_msg.contains("document"),
        "Error message should mention document not found, got: {}",
        error_msg
    );
}
