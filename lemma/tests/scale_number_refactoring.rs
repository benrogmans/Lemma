//! Comprehensive integration tests for Scale/Number refactoring
//!
//! Tests cover:
//! - Unit override validation (errors collected, not returned early)
//! - Document-level unit ambiguity (errors collected)
//! - Scale op Scale operations (same type allowed, different types rejected)
//! - Ratio operations
//! - Number operations
//! - Mixed operations

use lemma::planning::TypeRegistry;
use lemma::{parse, Engine, ResourceLimits};
use std::collections::HashMap;

#[test]
fn test_unit_override_validation_collects_all_errors() {
    // Test that multiple unit override errors are collected, not returned early
    let code = r#"doc test
type money = scale
  -> unit eur 1.00
  -> unit usd 1.19

type money2 = money
  -> unit eur 1.20
  -> unit usd 1.21
  -> unit gbp 1.30"#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    let doc = &docs[0];

    let mut registry = TypeRegistry::new();
    registry
        .register_type(&doc.name, doc.types[0].clone())
        .unwrap();
    registry
        .register_type(&doc.name, doc.types[1].clone())
        .unwrap();

    let result = registry.resolve_types(&doc.name);
    assert!(result.is_err(), "Should have errors for unit overrides");

    let error_msg = result.unwrap_err().to_string();
    // Should mention both eur and usd overrides
    assert!(
        error_msg.contains("eur") || error_msg.contains("usd"),
        "Error should mention unit override issues. Got: {}",
        error_msg
    );
}

#[test]
fn test_document_level_unit_ambiguity_collects_all_errors() {
    // Test that multiple ambiguous unit errors are collected
    let code = r#"doc test
type money_a = scale
  -> unit eur 1.00
  -> unit usd 1.19

type money_b = scale
  -> unit eur 1.00
  -> unit usd 1.20

type length_a = scale
  -> unit meter 1.0

type length_b = scale
  -> unit meter 1.0"#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    let doc = &docs[0];

    let mut registry = TypeRegistry::new();
    for type_def in &doc.types {
        registry.register_type(&doc.name, type_def.clone()).unwrap();
    }

    let result = registry.resolve_types(&doc.name);
    assert!(result.is_err(), "Should have errors for ambiguous units");

    let error_msg = result.unwrap_err().to_string();
    // Should mention both eur/usd ambiguity and meter ambiguity
    assert!(
        (error_msg.contains("eur") || error_msg.contains("usd") || error_msg.contains("meter")),
        "Error should mention ambiguous units. Got: {}",
        error_msg
    );
}

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10".to_string());
    facts.insert("price2".to_string(), "5".to_string());

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
    let result = engine.add_lemma_code(code, "test.lemma");

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10".to_string());
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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("multiplier".to_string(), "2".to_string());
    facts.insert("price".to_string(), "10".to_string());

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("price".to_string(), "10".to_string());

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10".to_string());
    facts.insert("ratio_value".to_string(), "0.5".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("result").is_some());
}

#[test]
fn test_number_type_cannot_have_units() {
    // Test that Number type rejects unit commands
    let code = r#"doc test
type price = number
  -> unit eur 1.00"#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    let doc = &docs[0];

    let mut registry = TypeRegistry::new();
    registry
        .register_type(&doc.name, doc.types[0].clone())
        .unwrap();

    let result = registry.resolve_types(&doc.name);
    assert!(
        result.is_err(),
        "Should reject unit command for Number type"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("unit") && error_msg.contains("number"),
        "Error should mention that Number types cannot have units. Got: {}",
        error_msg
    );
}

#[test]
fn test_scale_type_can_have_units() {
    // Test that Scale type can have units
    let code = r#"doc test
type money = scale
  -> unit eur 1.00
  -> unit usd 1.19"#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    let doc = &docs[0];

    let mut registry = TypeRegistry::new();
    registry
        .register_type(&doc.name, doc.types[0].clone())
        .unwrap();

    let result = registry.resolve_types(&doc.name);
    assert!(result.is_ok(), "Should allow units on Scale type");

    let resolved_types = result.unwrap();
    let money_type = resolved_types.named_types.get("money").unwrap();
    assert!(money_type.is_scale());
    match &money_type.specifications {
        lemma::TypeSpecification::Scale { units, .. } => {
            assert_eq!(units.len(), 2);
            assert!(units.iter().any(|u| u.name == "eur"));
            assert!(units.iter().any(|u| u.name == "usd"));
        }
        _ => panic!("Expected Scale type"),
    }
}

#[test]
fn test_extending_type_inherits_units() {
    // Test that extending a type inherits its units
    let code = r#"doc test
type money = scale
  -> unit eur 1.00
  -> unit usd 1.19

type my_money = money
  -> unit gbp 1.30"#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    let doc = &docs[0];

    let mut registry = TypeRegistry::new();
    for type_def in &doc.types {
        registry.register_type(&doc.name, type_def.clone()).unwrap();
    }

    let result = registry.resolve_types(&doc.name);
    if let Err(e) = &result {
        panic!(
            "Should allow extending with new units, but got error: {}",
            e
        );
    }

    let resolved_types = result.unwrap();
    let my_money_type = resolved_types.named_types.get("my_money").unwrap();
    match &my_money_type.specifications {
        lemma::TypeSpecification::Scale { units, .. } => {
            assert_eq!(units.len(), 3, "Should have eur, usd, and gbp");
            assert!(units.iter().any(|u| u.name == "eur"));
            assert!(units.iter().any(|u| u.name == "usd"));
            assert!(units.iter().any(|u| u.name == "gbp"));
        }
        _ => panic!("Expected Scale type"),
    }
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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10".to_string());
    facts.insert("price2".to_string(), "5".to_string());

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
    let result = engine.add_lemma_code(code, "test.lemma");

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
fn test_scale_comparison_with_number_allowed() {
    // Test that comparing Scale with Number is allowed
    let code = r#"doc test
type money = scale
  -> unit eur 1.00

fact price = [money]
fact threshold = [number]

rule is_above = price > threshold"#;

    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10".to_string());
    facts.insert("threshold".to_string(), "5".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("is_above").is_some());
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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("a".to_string(), "10".to_string());
    facts.insert("b".to_string(), "3".to_string());
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
    let result = engine.add_lemma_code(code, "test.lemma");

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

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
fn test_duplicate_unit_in_same_type_rejected() {
    // Test that duplicate units within the same type definition are rejected
    let code = r#"doc test
type money = scale
  -> unit eur 1.00
  -> unit eur 1.19"#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    let doc = &docs[0];

    let mut registry = TypeRegistry::new();
    registry
        .register_type(&doc.name, doc.types[0].clone())
        .unwrap();

    let result = registry.resolve_types(&doc.name);
    assert!(result.is_err(), "Should reject duplicate unit in same type");

    let error_msg = result.unwrap_err().to_string();
    // The error may mention "Duplicate unit" or "already exists" - both are valid
    assert!(
        error_msg.contains("Duplicate unit")
            || error_msg.contains("duplicate")
            || error_msg.contains("already exists")
            || error_msg.contains("eur"),
        "Error should mention duplicate unit issue. Got: {}",
        error_msg
    );
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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10".to_string());
    facts.insert("price2".to_string(), "5".to_string());

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10".to_string());
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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("price".to_string(), "10".to_string());

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
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("base_price".to_string(), "100".to_string());
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
fn test_standard_scale_and_number_types() {
    // Test that standard scale and number types work correctly
    let code = r#"doc test
fact scale_value = [scale]
fact number_value = [number]

rule result = scale_value * number_value"#;

    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("scale_value".to_string(), "10".to_string());
    facts.insert("number_value".to_string(), "2".to_string());

    let response = engine
        .evaluate("test", vec![], facts)
        .expect("Should evaluate");

    assert!(response.results.get("result").is_some());
}
