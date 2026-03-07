use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

#[test]
fn test_money_minus_percentage() {
    let mut engine = Engine::new();

    let code = r#"
doc test_money_minus_percentage

fact base_price: 200
fact discount_rate: 25%

rule price_after_discount: base_price - discount_rate
rule expected: 150

rule test_passes: price_after_discount == expected
"#;

    add_lemma_code_blocking(&mut engine, code, "test").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate(
            "test_money_minus_percentage",
            None,
            &now,
            vec![],
            HashMap::new(),
        )
        .unwrap();

    let price_after_discount = response.results.get("price_after_discount").unwrap();
    assert_eq!(
        price_after_discount.result.value().unwrap().to_string(),
        "150"
    );

    let test_passes = response.results.get("test_passes").unwrap();
    assert_eq!(test_passes.result.value().unwrap().to_string(), "true");
}

#[test]
fn test_money_plus_percentage() {
    let mut engine = Engine::new();

    let code = r#"
doc test_money_plus_percentage

fact base: 100
fact markup: 10%

rule price_with_markup: base + markup
rule expected: 110

rule test_passes: price_with_markup == expected
"#;

    add_lemma_code_blocking(&mut engine, code, "test").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate(
            "test_money_plus_percentage",
            None,
            &now,
            vec![],
            HashMap::new(),
        )
        .unwrap();

    let price_with_markup = response.results.get("price_with_markup").unwrap();
    assert_eq!(price_with_markup.result.value().unwrap().to_string(), "110");

    let test_passes = response.results.get("test_passes").unwrap();
    assert_eq!(test_passes.result.value().unwrap().to_string(), "true");
}

#[test]
fn test_number_times_percentage() {
    let mut engine = Engine::new();

    let code = r#"
doc test_number_times_percentage

fact amount: 1000
fact rate: 15%

rule result: amount * rate
rule expected: 150

rule test_passes: result == expected
"#;

    add_lemma_code_blocking(&mut engine, code, "test").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate(
            "test_number_times_percentage",
            None,
            &now,
            vec![],
            HashMap::new(),
        )
        .unwrap();

    let result = response.results.get("result").unwrap();
    assert_eq!(result.result.value().unwrap().to_string(), "150");

    let test_passes = response.results.get("test_passes").unwrap();
    assert_eq!(test_passes.result.value().unwrap().to_string(), "true");
}

#[test]
fn test_money_minus_percentage_with_rule_reference() {
    let mut engine = Engine::new();

    let code = r#"
doc test_with_rule_reference

fact base_price: 200
fact discount_rate: 25%

rule discount_amount: base_price * discount_rate
rule final_price: base_price - discount_amount
rule expected: 150

rule test_passes: final_price == expected
"#;

    add_lemma_code_blocking(&mut engine, code, "test").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate(
            "test_with_rule_reference",
            None,
            &now,
            vec![],
            HashMap::new(),
        )
        .unwrap();

    let discount_amount = response.results.get("discount_amount").unwrap();
    assert_eq!(discount_amount.result.value().unwrap().to_string(), "50");

    let final_price = response.results.get("final_price").unwrap();
    assert_eq!(final_price.result.value().unwrap().to_string(), "150");
}

#[test]
fn test_chained_percentage_operations() {
    let mut engine = Engine::new();

    let code = r#"
doc test_chained_percentages

fact original_price: 100
fact first_discount: 20%
fact second_discount: 10%

rule after_first: original_price - first_discount
rule after_second: after_first - second_discount

rule expected: 72

rule test_passes: after_second == expected
"#;

    add_lemma_code_blocking(&mut engine, code, "test").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate(
            "test_chained_percentages",
            None,
            &now,
            vec![],
            HashMap::new(),
        )
        .unwrap();

    let after_first = response.results.get("after_first").unwrap();
    assert_eq!(after_first.result.value().unwrap().to_string(), "80");

    let after_second = response.results.get("after_second").unwrap();
    assert_eq!(after_second.result.value().unwrap().to_string(), "72");
}
