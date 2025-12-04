use lemma::*;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_end_to_end_simple_rule() {
    let code = r#"
doc test

fact quantity = 25

rule discount = 0
  unless quantity >= 10 then 10
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount")
        .unwrap();

    // Since quantity=25 is >= 10, we should get 10
    match &discount_result.result {
        lemma::OperationResult::Value(lemma::LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("10").unwrap())
        }
        _ => panic!("Expected number result"),
    }

    // Verify proof structure exists
    assert!(
        discount_result.proof.is_some(),
        "Proof should be generated for discount rule"
    );
}

#[test]
fn test_end_to_end_boolean_rule() {
    let code = r#"
doc test

fact age = 25
fact has_license = true

rule can_drive = age >= 18 and has_license
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "can_drive")
        .unwrap();

    match &result.result {
        lemma::OperationResult::Value(lemma::LiteralValue::Boolean(b)) => assert!(bool::from(b)),
        _ => panic!("Expected boolean result, got {:?}", result.result),
    }

    // Verify proof structure exists
    assert!(
        result.proof.is_some(),
        "Proof should be generated for can_drive rule"
    );
}

#[test]
fn test_end_to_end_arithmetic() {
    let code = r#"
doc test

fact base = 100
fact multiplier = 2

rule result = base * multiplier
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .unwrap();

    match &result.result {
        lemma::OperationResult::Value(lemma::LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("200").unwrap())
        }
        _ => panic!("Expected number result, got {:?}", result.result),
    }

    // Verify proof structure exists
    assert!(
        result.proof.is_some(),
        "Proof should be generated for result rule"
    );
}

#[test]
fn test_end_to_end_rule_reference() {
    let code = r#"
doc test

fact quantity = 25

rule discount = 0
  unless quantity >= 10 then 10

rule final_price = 100 - discount?
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "final_price")
        .unwrap();

    match &result.result {
        lemma::OperationResult::Value(lemma::LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("90").unwrap())
        }
        _ => panic!("Expected number result, got {:?}", result.result),
    }

    // Verify proof structure exists and contains rule reference
    assert!(
        result.proof.is_some(),
        "Proof should be generated for final_price rule"
    );
}

#[test]
fn test_end_to_end_quantity_less_than_10() {
    let code = r#"
doc test

fact quantity = 5

rule discount = 0
  unless quantity >= 10 then 10
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount")
        .unwrap();

    // Since quantity=5 is < 10, we should get 0 (default value)
    match &discount_result.result {
        lemma::OperationResult::Value(lemma::LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("0").unwrap())
        }
        _ => panic!("Expected number result"),
    }
}

#[test]
fn test_end_to_end_quantity_boundary_case() {
    let code = r#"
doc test

fact quantity = 10

rule discount = 0
  unless quantity >= 10 then 10
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount")
        .unwrap();

    // Since quantity=10 is >= 10, we should get 10
    match &discount_result.result {
        lemma::OperationResult::Value(lemma::LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("10").unwrap())
        }
        _ => panic!("Expected number result"),
    }
}

#[test]
fn test_end_to_end_boolean_rule_false() {
    let code = r#"
doc test

fact age = 15
fact has_license = true

rule can_drive = age >= 18 and has_license
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "can_drive")
        .unwrap();

    // Since age=15 is < 18, we should get false
    match &result.result {
        lemma::OperationResult::Value(lemma::LiteralValue::Boolean(b)) => {
            assert!(!bool::from(b), "can_drive should be false when age < 18")
        }
        _ => panic!("Expected boolean result, got {:?}", result.result),
    }
}

#[test]
fn test_end_to_end_rule_reference_missing_dependency() {
    let code = r#"
doc test

fact quantity = [number]

rule discount = 0
  unless quantity >= 10 then 10

rule final_price = 100 - discount?
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // Evaluate without providing quantity - discount should fail, causing final_price to fail
    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    
    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount")
        .unwrap();

    // discount should fail due to missing quantity
    match &discount_result.result {
        lemma::OperationResult::Veto(_) => {
            // Expected - missing fact causes veto
        }
        other => panic!("Expected veto for discount due to missing quantity, got {:?}", other),
    }

    let final_price_result = response
        .results
        .values()
        .find(|r| r.rule.name == "final_price")
        .unwrap();

    // final_price should also fail due to missing discount dependency
    match &final_price_result.result {
        lemma::OperationResult::Veto(_) => {
            // Expected - rule reference to failed rule causes veto
        }
        other => panic!("Expected veto for final_price due to missing discount dependency, got {:?}", other),
    }
}

#[test]
fn test_end_to_end_arithmetic_negative_numbers() {
    let code = r#"
doc test

fact base = -50
fact multiplier = 3

rule result = base * multiplier
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .unwrap();

    match &result.result {
        lemma::OperationResult::Value(lemma::LiteralValue::Number(n)) => {
            assert_eq!(*n, Decimal::from_str("-150").unwrap())
        }
        _ => panic!("Expected number result, got {:?}", result.result),
    }
}

#[test]
fn test_end_to_end_division_by_zero() {
    let code = r#"
doc test

fact numerator = 100
fact denominator = 0

rule result = numerator / denominator
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .unwrap();

    // Division by zero should result in veto
    match &result.result {
        lemma::OperationResult::Veto(msg) => {
            assert!(
                msg.as_ref()
                    .map(|m| m.to_lowercase().contains("division") || m.to_lowercase().contains("zero"))
                    .unwrap_or(false),
                "Veto message should mention division by zero, got: {:?}",
                msg
            );
        }
        other => panic!("Expected veto for division by zero, got {:?}", other),
    }
}

#[test]
fn test_end_to_end_invalid_document_name() {
    let engine = Engine::new();
    let result = engine.evaluate("nonexistent_document", vec![], HashMap::new());

    assert!(result.is_err(), "Should fail for nonexistent document");
    let error_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        error_msg.contains("not found") || error_msg.contains("nonexistent"),
        "Error message should mention document not found, got: {}",
        error_msg
    );
}

#[test]
fn test_end_to_end_missing_required_fact() {
    let code = r#"
doc test

fact price = [number]
fact quantity = [number]

rule total = price * quantity
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // Evaluate without providing required facts
    let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
    let total_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .unwrap();

    // Should fail due to missing facts
    match &total_result.result {
        lemma::OperationResult::Veto(msg) => {
            let msg_str = msg.as_ref().map(|m| m.to_lowercase()).unwrap_or_default();
            assert!(
                msg_str.contains("missing") || msg_str.contains("fact"),
                "Veto message should mention missing fact, got: {:?}",
                msg
            );
        }
        other => panic!("Expected veto for missing required facts, got {:?}", other),
    }
}
