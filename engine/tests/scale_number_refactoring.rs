//! Integration tests for Scale/Number arithmetic behavior.
//!
//! Unit-resolution and PerSliceTypeResolver behavior tests live in `src/planning/types.rs`.

use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, Response};
use std::collections::HashMap;

fn rule_value_str(response: &Response, name: &str) -> String {
    let r = response
        .results
        .get(name)
        .unwrap_or_else(|| panic!("rule '{name}' missing from results"));
    assert!(
        !r.result.vetoed(),
        "rule '{name}' must not veto, got {:?}",
        r.result
    );
    r.result
        .value()
        .unwrap_or_else(|| panic!("rule '{name}' must produce a value"))
        .to_string()
}

#[test]
fn test_scale_op_scale_same_type_allowed() {
    // Test that Scale op Scale with same type is allowed
    let code = r#"spec test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19

fact price1: [money]
fact price2: [money]

rule total: price1 + price2
rule difference: price1 - price2
rule product: price1 * price2
rule quotient: price1 / price2"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10 eur".to_string());
    facts.insert("price2".to_string(), "5 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    for name in ["total", "difference", "product", "quotient"] {
        let r = response.results.get(name).expect(name);
        assert!(
            !r.result.vetoed(),
            "{name} must not veto for valid scale inputs"
        );
        let v = r
            .result
            .value()
            .unwrap_or_else(|| panic!("{name} must produce a value"));
        assert!(
            v.get_type().is_scale(),
            "{name} result must stay in scale money type"
        );
    }
    let total_s = response
        .results
        .get("total")
        .unwrap()
        .result
        .value()
        .unwrap()
        .to_string();
    assert!(
        total_s.contains("15") && total_s.to_lowercase().contains("eur"),
        "10 eur + 5 eur => ~15 eur, got {total_s}"
    );
    let diff_s = response
        .results
        .get("difference")
        .unwrap()
        .result
        .value()
        .unwrap()
        .to_string();
    assert!(
        diff_s.contains("5") && diff_s.to_lowercase().contains("eur"),
        "10 eur - 5 eur => ~5 eur, got {diff_s}"
    );
    let prod_s = response
        .results
        .get("product")
        .unwrap()
        .result
        .value()
        .unwrap()
        .to_string();
    assert!(
        prod_s.contains("50"),
        "10 eur * 5 eur => numeric product 50 in display, got {prod_s}"
    );
    let quot_s = response
        .results
        .get("quotient")
        .unwrap()
        .result
        .value()
        .unwrap()
        .to_string();
    assert!(
        quot_s.contains("2"),
        "10 eur / 5 eur => ratio 2 in display, got {quot_s}"
    );
}

#[test]
fn test_scale_op_scale_different_types_rejected() {
    // Test that Scale op Scale with different types is rejected
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

type length: scale
  -> unit meter 1.0

fact price: [money]
fact distance: [length]

rule invalid: price + distance"#;

    let mut engine = Engine::new();
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));

    // Should fail during planning/validation
    assert!(
        result.is_err(),
        "Should reject different Scale types in arithmetic"
    );

    let errs = result.unwrap_err();
    let error_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        error_msg.contains("different scale types") || error_msg.contains("Cannot add"),
        "Error should mention different scale types. Got: {}",
        error_msg
    );
}

#[test]
fn test_scale_op_number_allowed() {
    // Test that Scale op Number is allowed
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact price: [money]
fact multiplier: [number]

rule scaled: price * multiplier
rule divided: price / multiplier"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10 eur".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let scaled = rule_value_str(&response, "scaled");
    assert!(
        scaled.contains("20") && scaled.to_lowercase().contains("eur"),
        "10 eur * 2 => ~20 eur, got {scaled}"
    );
    let divided = rule_value_str(&response, "divided");
    assert!(
        divided.contains("5") && divided.to_lowercase().contains("eur"),
        "10 eur / 2 => ~5 eur, got {divided}"
    );
}

#[test]
fn test_number_op_scale_allowed() {
    // Test that Number op Scale is allowed
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact multiplier: [number]
fact price: [money]

rule scaled: multiplier * price
rule divided: multiplier / price"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("multiplier".to_string(), "2".to_string());
    facts.insert("price".to_string(), "10 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let scaled = rule_value_str(&response, "scaled");
    assert!(
        scaled.contains("20") && scaled.to_lowercase().contains("eur"),
        "2 * 10 eur => ~20 eur, got {scaled}"
    );
    let divided = rule_value_str(&response, "divided");
    assert!(
        divided.contains("0.2") || divided.contains("0,2"),
        "2 / 10 eur => dimensionless ~0.2, got {divided}"
    );
}

#[test]
fn test_ratio_op_number_allowed() {
    // Test that Ratio op Number is allowed (result is Number)
    let code = r#"spec test
fact ratio_value: [ratio]
fact multiplier: [number]

rule result: ratio_value * multiplier"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let s = rule_value_str(&response, "result");
    assert!(s.contains('1'), "0.5 * 2 => 1, got {s}");
}

#[test]
fn test_ratio_op_ratio_allowed() {
    // Test that Ratio op Ratio is allowed (result is Ratio)
    let code = r#"spec test
fact ratio1: [ratio]
fact ratio2: [ratio]

rule product: ratio1 * ratio2
rule quotient: ratio1 / ratio2"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio1".to_string(), "0.5".to_string());
    facts.insert("ratio2".to_string(), "0.25".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let prod = rule_value_str(&response, "product");
    assert!(
        prod.contains("125") || prod.contains("0.125"),
        "0.5*0.25, got {prod}"
    );
    let quot = rule_value_str(&response, "quotient");
    assert!(quot.contains('2'), "0.5/0.25 => 2, got {quot}");
}

#[test]
fn test_ratio_op_scale_allowed() {
    // Test that Ratio op Scale is allowed (result is Scale)
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact ratio_value: [ratio]
fact price: [money]

rule result: ratio_value * price"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("price".to_string(), "10 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let s = rule_value_str(&response, "result");
    assert!(
        s.contains('5') && s.to_lowercase().contains("eur"),
        "0.5 * 10 eur => ~5 eur, got {s}"
    );
}

#[test]
fn test_scale_op_ratio_allowed() {
    // Test that Scale op Ratio is allowed (result is Scale)
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact price: [money]
fact ratio_value: [ratio]

rule result: price * ratio_value"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10 eur".to_string());
    facts.insert("ratio_value".to_string(), "0.5".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let s = rule_value_str(&response, "result");
    assert!(
        s.contains('5') && s.to_lowercase().contains("eur"),
        "10 eur * 0.5 => ~5 eur, got {s}"
    );
}

#[test]
fn test_scale_comparison_same_type_allowed() {
    // Test that comparing same Scale types is allowed
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact price1: [money]
fact price2: [money]

rule is_greater: price1 > price2
rule is_equal: price1 == price2"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10 eur".to_string());
    facts.insert("price2".to_string(), "5 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    assert_eq!(rule_value_str(&response, "is_greater"), "true");
    assert_eq!(rule_value_str(&response, "is_equal"), "false");
}

#[test]
fn test_scale_comparison_different_types_rejected() {
    // Test that comparing different Scale types is rejected
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

type length: scale
  -> unit meter 1.0

fact price: [money]
fact distance: [length]

rule invalid: price > distance"#;

    let mut engine = Engine::new();
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));

    assert!(
        result.is_err(),
        "Should reject comparison between different Scale types"
    );

    let errs = result.unwrap_err();
    let error_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        error_msg.contains("different scale types") || error_msg.contains("Cannot compare"),
        "Error should mention different scale types. Got: {}",
        error_msg
    );
}

#[test]
fn test_scale_comparison_with_number_rejected() {
    // Comparing Scale with Number is ambiguous (Number has no unit) and must be rejected.
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact price: [money]
fact threshold: [number]

rule is_above: price > threshold"#;

    let mut engine = Engine::new();
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));
    let errs = result.expect_err("Should reject scale vs number comparison");
    let error_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        error_msg.to_lowercase().contains("compare")
            || error_msg.contains("scale")
            || error_msg.contains("number"),
        "Error should mention comparison/scale/number. Got: {error_msg}"
    );
}

#[test]
fn test_all_arithmetic_operators_scale_same_type() {
    // Test all arithmetic operators with same Scale type
    // Note: Modulo requires Number divisor, so we test it separately
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact a: [money]
fact b: [money]
fact divisor: [number]
fact exponent: [number]

rule add: a + b
rule subtract: a - b
rule multiply: a * b
rule divide: a / b
rule modulo: a % divisor
rule power: a ^ exponent"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("a".to_string(), "10 eur".to_string());
    facts.insert("b".to_string(), "3 eur".to_string());
    facts.insert("divisor".to_string(), "3".to_string());
    facts.insert("exponent".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let add = rule_value_str(&response, "add");
    assert!(
        add.contains("13") && add.to_lowercase().contains("eur"),
        "add: {add}"
    );
    let sub = rule_value_str(&response, "subtract");
    assert!(
        sub.contains('7') && sub.to_lowercase().contains("eur"),
        "subtract: {sub}"
    );
    let mul = rule_value_str(&response, "multiply");
    assert!(mul.contains("30"), "multiply: {mul}");
    let div = rule_value_str(&response, "divide");
    assert!(
        div.contains('3') && div.to_lowercase().contains("eur"),
        "divide: {div}"
    );
    let modulo = rule_value_str(&response, "modulo");
    assert!(
        modulo.contains('1') && modulo.to_lowercase().contains("eur"),
        "modulo: {modulo}"
    );
    let pow = rule_value_str(&response, "power");
    assert!(pow.contains("100"), "power 10^2: {pow}");
}

#[test]
fn test_all_arithmetic_operators_scale_different_types_rejected() {
    // Test that all arithmetic operators reject different Scale types
    let code = r#"spec test
type money: scale -> unit eur 1.00
type length: scale -> unit meter 1.0

fact price: [money]
fact distance: [length]

rule add: price + distance
rule subtract: price - distance
rule multiply: price * distance
rule divide: price / distance
rule modulo: price % distance
rule power: price ^ distance"#;

    let mut engine = Engine::new();
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));

    assert!(
        result.is_err(),
        "Should reject all operations between different Scale types"
    );

    let errs = result.unwrap_err();
    let error_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
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
    let code = r#"spec test
fact a: [number]
fact b: [number]

rule add: a + b
rule subtract: a - b
rule multiply: a * b
rule divide: a / b
rule modulo: a % b
rule power: a ^ b"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("a".to_string(), "10".to_string());
    facts.insert("b".to_string(), "3".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    assert_eq!(rule_value_str(&response, "add"), "13");
    assert_eq!(rule_value_str(&response, "subtract"), "7");
    assert_eq!(rule_value_str(&response, "multiply"), "30");
    let div = rule_value_str(&response, "divide");
    assert!(
        div.starts_with("3.333") || div == "3.3333333333333333333333333333",
        "divide 10/3: {div}"
    );
    assert_eq!(rule_value_str(&response, "modulo"), "1");
    assert_eq!(rule_value_str(&response, "power"), "1000");
}

#[test]
fn test_scale_result_type_preservation() {
    // Test that Scale op Scale (same type) preserves the Scale type
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact price1: [money]
fact price2: [money]

rule total: price1 + price2"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price1".to_string(), "10 eur".to_string());
    facts.insert("price2".to_string(), "5 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
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
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact price: [money]
fact multiplier: [number]

rule result: price * multiplier"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("price".to_string(), "10 eur".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
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
    let code = r#"spec test
fact ratio_value: [ratio]
fact multiplier: [number]

rule result: ratio_value * multiplier"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("multiplier".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
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
    let code = r#"spec test
fact ratio1: [ratio]
fact ratio2: [ratio]

rule result: ratio1 * ratio2"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio1".to_string(), "0.5".to_string());
    facts.insert("ratio2".to_string(), "0.25".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
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
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact ratio_value: [ratio]
fact price: [money]

rule result: ratio_value * price"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("ratio_value".to_string(), "0.5".to_string());
    facts.insert("price".to_string(), "10 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
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
    let code = r#"spec test
type money: scale
  -> unit eur 1.00

fact base_price: [money]
fact discount_ratio: [ratio]
fact tax_multiplier: [number]
fact quantity: [number]

rule discounted: base_price * discount_ratio
rule with_tax: discounted * tax_multiplier
rule total: with_tax * quantity"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("base_price".to_string(), "100 eur".to_string());
    facts.insert("discount_ratio".to_string(), "0.9".to_string());
    facts.insert("tax_multiplier".to_string(), "1.2".to_string());
    facts.insert("quantity".to_string(), "5".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let disc = rule_value_str(&response, "discounted");
    assert!(
        disc.contains("90") && disc.to_lowercase().contains("eur"),
        "100 eur * 0.9 => ~90 eur: {disc}"
    );
    let tax = rule_value_str(&response, "with_tax");
    assert!(
        tax.contains("108") && tax.to_lowercase().contains("eur"),
        "90 eur * 1.2 => ~108 eur: {tax}"
    );
    let tot = rule_value_str(&response, "total");
    assert!(
        tot.contains("540") && tot.to_lowercase().contains("eur"),
        "108 eur * 5 => ~540 eur: {tot}"
    );
}

#[test]
fn test_primitive_scale_and_number_types() {
    // Scale types must declare at least one unit; scale values are unitful.
    // This test uses a proper scale type (money) and unitful fact value.
    let code = r#"spec test
type money: scale
  -> unit eur 1.00
  -> minimum 0 eur

fact scale_value: [money]
fact number_value: [number]

rule result: scale_value * number_value"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut facts = HashMap::new();
    facts.insert("scale_value".to_string(), "10 eur".to_string());
    facts.insert("number_value".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), facts, false)
        .expect("Should evaluate");

    let s = rule_value_str(&response, "result");
    assert!(
        s.contains("20") && s.to_lowercase().contains("eur"),
        "10 eur * 2 => ~20 eur: {s}"
    );
}
