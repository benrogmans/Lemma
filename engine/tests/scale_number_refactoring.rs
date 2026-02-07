//! Integration tests for Scale/Number arithmetic behavior.
//!
//! Unit-resolution and TypeRegistry behavior tests live in `src/planning/types.rs`.

use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

#[test]
fn test_scale_op_scale_same_type_allowed() {
    // Test that Scale op Scale with same type is allowed
    let code = r#"doc test
type money = scale
  -> unit eur 1.00
  -> unit usd 1.19

fact price1 = [money]
fact price2 = [money]

rule total = price1 + price2
rule difference = price1 - price2
rule product = price1 * price2
rule quotient = price1 / price2"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10 eur".to_string());
    facts.insert("price2".to_string(), "5 eur".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    // All operations should work
    assert!(response.results.get("total").is_some());
    assert!(response.results.get("difference").is_some());
    assert!(response.results.get("product").is_some());
    assert!(response.results.get("quotient").is_some());
}

#[test]
fn test_scale_op_scale_different_types_rejected() {
    // Test that Scale op Scale with different types is rejected
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

type length = scale
  -> unit meter 1.0

fact price = [money]
fact distance = [length]

rule invalid = price + distance"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");

    // Should fail during planning/validation
    assert!(
        result.is_err(),
        "Should reject different Scale types in arithmetic"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("different scale types") || error_msg.contains("Cannot add"),
        "Error should mention different scale types. Got: {}",
        error_msg
    );
}

#[test]
fn test_scale_op_number_allowed() {
    // Test that Scale op Number is allowed
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact price = [money]
fact multiplier = [number]

rule scaled = price * multiplier
rule divided = price / multiplier"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10 eur".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("scaled").is_some());
    assert!(response.results.get("divided").is_some());
}

#[test]
fn test_number_op_scale_allowed() {
    // Test that Number op Scale is allowed
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact multiplier = [number]
fact price = [money]

rule scaled = multiplier * price
rule divided = multiplier / price"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("multiplier".to_string(), "2".to_string());
    facts.insert("price".to_string(), "10 eur".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("scaled").is_some());
    assert!(response.results.get("divided").is_some());
}

#[test]
fn test_ratio_op_number_allowed() {
    // Test that Ratio op Number is allowed (result is Number)
    let code = r#"doc test
fact ratio_value = [ratio]
fact multiplier = [number]

rule result = ratio_value * multiplier"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("result").is_some());
}

#[test]
fn test_ratio_op_ratio_allowed() {
    // Test that Ratio op Ratio is allowed (result is Ratio)
    let code = r#"doc test
fact ratio1 = [ratio]
fact ratio2 = [ratio]

rule product = ratio1 * ratio2
rule quotient = ratio1 / ratio2"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio1".to_string(), "0.5".to_string());
    facts.insert("ratio2".to_string(), "0.25".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("product").is_some());
    assert!(response.results.get("quotient").is_some());
}

#[test]
fn test_ratio_op_scale_allowed() {
    // Test that Ratio op Scale is allowed (result is Scale)
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact ratio_value = [ratio]
fact price = [money]

rule result = ratio_value * price"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("price".to_string(), "10 eur".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("result").is_some());
}

#[test]
fn test_scale_op_ratio_allowed() {
    // Test that Scale op Ratio is allowed (result is Scale)
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact price = [money]
fact ratio_value = [ratio]

rule result = price * ratio_value"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10 eur".to_string());
    facts.insert("ratio_value".to_string(), "0.5".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("result").is_some());
}

#[test]
fn test_scale_comparison_same_type_allowed() {
    // Test that comparing same Scale types is allowed
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact price1 = [money]
fact price2 = [money]

rule is_greater = price1 > price2
rule is_equal = price1 == price2"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10 eur".to_string());
    facts.insert("price2".to_string(), "5 eur".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("is_greater").is_some());
    assert!(response.results.get("is_equal").is_some());
}

#[test]
fn test_scale_comparison_different_types_rejected() {
    // Test that comparing different Scale types is rejected
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

type length = scale
  -> unit meter 1.0

fact price = [money]
fact distance = [length]

rule invalid = price > distance"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject comparison between different Scale types"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("different scale types") || error_msg.contains("Cannot compare"),
        "Error should mention different scale types. Got: {}",
        error_msg
    );
}

#[test]
fn test_scale_comparison_with_number_rejected() {
    // Comparing Scale with Number is ambiguous (Number has no unit) and must be rejected.
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact price = [money]
fact threshold = [number]

rule is_above = price > threshold"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(result.is_err(), "Should reject scale vs number comparison");
}

#[test]
fn test_all_arithmetic_operators_scale_same_type() {
    // Test all arithmetic operators with same Scale type
    // Note: Modulo requires Number divisor, so we test it separately
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact a = [money]
fact b = [money]
fact divisor = [number]
fact exponent = [number]

rule add = a + b
rule subtract = a - b
rule multiply = a * b
rule divide = a / b
rule modulo = a % divisor
rule power = a ^ exponent"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("a".to_string(), "10 eur".to_string());
    facts.insert("b".to_string(), "3 eur".to_string());
    facts.insert("divisor".to_string(), "3".to_string());
    facts.insert("exponent".to_string(), "2".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    // All operations should work for same Scale type (modulo uses Number divisor)
    assert!(response.results.get("add").is_some());
    assert!(response.results.get("subtract").is_some());
    assert!(response.results.get("multiply").is_some());
    assert!(response.results.get("divide").is_some());
    assert!(response.results.get("modulo").is_some());
    assert!(response.results.get("power").is_some());
}

#[test]
fn test_all_arithmetic_operators_scale_different_types_rejected() {
    // Test that all arithmetic operators reject different Scale types
    let code = r#"doc test
type money = scale -> unit eur 1.00
type length = scale -> unit meter 1.0

fact price = [money]
fact distance = [length]

rule add = price + distance
rule subtract = price - distance
rule multiply = price * distance
rule divide = price / distance
rule modulo = price % distance
rule power = price ^ distance"#;

    let mut engine = Engine::new();
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject all operations between different Scale types"
    );

    let error_msg = result.unwrap_err().to_string();
    // Should mention that different scale types cannot be used
    assert!(
        error_msg.contains("different scale types") || error_msg.contains("Cannot"),
        "Error should mention different scale types. Got: {}",
        error_msg
    );
}

#[test]
fn test_number_operations_all_operators() {
    // Test all arithmetic operators with Number types
    let code = r#"doc test
fact a = [number]
fact b = [number]

rule add = a + b
rule subtract = a - b
rule multiply = a * b
rule divide = a / b
rule modulo = a % b
rule power = a ^ b"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("a".to_string(), "10".to_string());
    facts.insert("b".to_string(), "3".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    // All operations should work for Number types
    assert!(response.results.get("add").is_some());
    assert!(response.results.get("subtract").is_some());
    assert!(response.results.get("multiply").is_some());
    assert!(response.results.get("divide").is_some());
    assert!(response.results.get("modulo").is_some());
    assert!(response.results.get("power").is_some());
}

#[test]
fn test_scale_result_type_preservation() {
    // Test that Scale op Scale (same type) preserves the Scale type
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact price1 = [money]
fact price2 = [money]

rule total = price1 + price2"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10 eur".to_string());
    facts.insert("price2".to_string(), "5 eur".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    let total_result = response
        .results
        .get("total")
        .expect("Total rule should exist");
    let total_value = match total_result.result.value() {
        Some(v) => v,
        None => {
            panic!(
                "Total should have a value, but got: {:?}",
                total_result.result
            );
        }
    };

    // Result should still be a Scale type (money)
    assert!(total_value.get_type().is_scale());
    assert_eq!(total_value.get_type().name(), "money");
}

#[test]
fn test_scale_number_result_inherits_scale() {
    // Test that Scale op Number results in Scale type
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact price = [money]
fact multiplier = [number]

rule result = price * multiplier"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10 eur".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    let result = response.results.get("result").unwrap();
    let result_value = result.result.value().unwrap();

    // Result should be Scale type (inherits from Scale operand)
    assert!(result_value.get_type().is_scale());
    assert_eq!(result_value.get_type().name(), "money");
}

#[test]
fn test_ratio_number_result_is_number() {
    // Test that Ratio op Number results in Number type
    let code = r#"doc test
fact ratio_value = [ratio]
fact multiplier = [number]

rule result = ratio_value * multiplier"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    let result = response
        .results
        .get("result")
        .expect("Result rule should exist");

    let result_value = match result.result.value() {
        Some(v) => v,
        None => {
            panic!("Result should have a value, but got: {:?}", result.result);
        }
    };

    // Result should be Number type
    assert!(result_value.get_type().is_number());
}

#[test]
fn test_ratio_ratio_result_is_ratio() {
    // Test that Ratio op Ratio results in Ratio type
    let code = r#"doc test
fact ratio1 = [ratio]
fact ratio2 = [ratio]

rule result = ratio1 * ratio2"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio1".to_string(), "0.5".to_string());
    facts.insert("ratio2".to_string(), "0.25".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    let result = response
        .results
        .get("result")
        .expect("Result rule should exist");

    let result_value = match result.result.value() {
        Some(v) => v,
        None => {
            panic!("Result should have a value, but got: {:?}", result.result);
        }
    };

    // Result should be Ratio type
    assert!(result_value.get_type().is_ratio());
}

#[test]
fn test_ratio_scale_result_is_scale() {
    // Test that Ratio op Scale results in Scale type
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact ratio_value = [ratio]
fact price = [money]

rule result = ratio_value * price"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("price".to_string(), "10 eur".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    let result = response.results.get("result").unwrap();
    let result_value = result.result.value().unwrap();

    // Result should be Scale type (inherits from Scale operand)
    assert!(result_value.get_type().is_scale());
    assert_eq!(result_value.get_type().name(), "money");
}

#[test]
fn test_complex_mixed_operations() {
    // Test complex expressions with mixed types
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact base_price = [money]
fact discount_ratio = [ratio]
fact tax_multiplier = [number]
fact quantity = [number]

rule discounted = base_price * discount_ratio
rule with_tax = discounted? * tax_multiplier
rule total = with_tax? * quantity"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("base_price".to_string(), "100 eur".to_string());
    facts.insert("discount_ratio".to_string(), "0.9".to_string());
    facts.insert("tax_multiplier".to_string(), "1.2".to_string());
    facts.insert("quantity".to_string(), "5".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("discounted").is_some());
    assert!(response.results.get("with_tax").is_some());
    assert!(response.results.get("total").is_some());
}

#[test]
fn test_primitive_scale_and_number_types() {
    // Scale types must declare at least one unit; scale values are unitful.
    // This test uses a proper scale type (money) and unitful fact value.
    let code = r#"doc test
type money = scale
  -> unit eur 1.00
  -> minimum 0 eur

fact scale_value = [money]
fact number_value = [number]

rule result = scale_value * number_value"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("scale_value".to_string(), "10 eur".to_string());
    facts.insert("number_value".to_string(), "2".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("result").is_some());
}
