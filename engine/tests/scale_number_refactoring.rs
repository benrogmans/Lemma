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
data money: scale
  -> unit eur 1.00
  -> unit usd 1.19

data price1: money
data price2: money

rule total: price1 + price2
rule difference: price1 - price2
rule product: price1 * price2
rule quotient: price1 / price2"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("price1".to_string(), "10 eur".to_string());
    data.insert("price2".to_string(), "5 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
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
fn test_scale_op_number_allowed() {
    // Test that Scale op Number is allowed
    let code = r#"spec test
data money: scale
  -> unit eur 1.00

data price: money
data multiplier: number

rule scaled: price * multiplier
rule divided: price / multiplier"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("price".to_string(), "10 eur".to_string());
    data.insert("multiplier".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
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
data money: scale
  -> unit eur 1.00

data multiplier: number
data price: money

rule scaled: multiplier * price
rule divided: multiplier / price"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("multiplier".to_string(), "2".to_string());
    data.insert("price".to_string(), "10 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
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
data ratio_value: ratio
data multiplier: number

rule result: ratio_value * multiplier"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("ratio_value".to_string(), "0.5".to_string());
    data.insert("multiplier".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
        .expect("Should evaluate");

    let s = rule_value_str(&response, "result");
    assert!(s.contains('1'), "0.5 * 2 => 1, got {s}");
}

#[test]
fn test_ratio_op_scale_allowed() {
    // Test that Ratio op Scale is allowed (result is Scale)
    let code = r#"spec test
data money: scale
  -> unit eur 1.00

data ratio_value: ratio
data price: money

rule result: ratio_value * price"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("ratio_value".to_string(), "0.5".to_string());
    data.insert("price".to_string(), "10 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
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
data money: scale
  -> unit eur 1.00

data price: money
data ratio_value: ratio

rule result: price * ratio_value"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("price".to_string(), "10 eur".to_string());
    data.insert("ratio_value".to_string(), "0.5".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
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
data money: scale
  -> unit eur 1.00

data price1: money
data price2: money

rule is_greater: price1 > price2
rule is_equal: price1 is price2"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("price1".to_string(), "10 eur".to_string());
    data.insert("price2".to_string(), "5 eur".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
        .expect("Should evaluate");

    assert_eq!(rule_value_str(&response, "is_greater"), "true");
    assert_eq!(rule_value_str(&response, "is_equal"), "false");
}

#[test]
fn test_all_arithmetic_operators_scale_same_type() {
    // Test all arithmetic operators with same Scale type
    // Note: Modulo requires Number divisor, so we test it separately
    let code = r#"spec test
data money: scale
  -> unit eur 1.00

data a: money
data b: money
data divisor: number
data exponent: number

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

    let mut data = HashMap::new();
    data.insert("a".to_string(), "10 eur".to_string());
    data.insert("b".to_string(), "3 eur".to_string());
    data.insert("divisor".to_string(), "3".to_string());
    data.insert("exponent".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
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
fn test_number_operations_all_operators() {
    // Test all arithmetic operators with Number types
    let code = r#"spec test
data a: number
data b: number

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

    let mut data = HashMap::new();
    data.insert("a".to_string(), "10".to_string());
    data.insert("b".to_string(), "3".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
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
fn test_complex_mixed_operations() {
    // Test complex expressions with mixed types
    let code = r#"spec test
data money: scale
  -> unit eur 1.00

data base_price: money
data discount_ratio: ratio
data tax_multiplier: number
data quantity: number

rule discounted: base_price * discount_ratio
rule with_tax: discounted * tax_multiplier
rule total: with_tax * quantity"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("base_price".to_string(), "100 eur".to_string());
    data.insert("discount_ratio".to_string(), "0.9".to_string());
    data.insert("tax_multiplier".to_string(), "1.2".to_string());
    data.insert("quantity".to_string(), "5".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
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
    // This test uses a proper scale type (money) and unitful data value.
    let code = r#"spec test
data money: scale
  -> unit eur 1.00
  -> minimum 0 eur

data scale_value: money
data number_value: number

rule result: scale_value * number_value"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .expect("Should parse");

    let mut data = HashMap::new();
    data.insert("scale_value".to_string(), "10 eur".to_string());
    data.insert("number_value".to_string(), "2".to_string());

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), data, false)
        .expect("Should evaluate");

    let s = rule_value_str(&response, "result");
    assert!(
        s.contains("20") && s.to_lowercase().contains("eur"),
        "10 eur * 2 => ~20 eur: {s}"
    );
}
