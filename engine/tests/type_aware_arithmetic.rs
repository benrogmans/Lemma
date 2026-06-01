use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

#[test]
fn test_money_minus_percentage() {
    let mut engine = Engine::new();

    let code = r#"
spec test_money_minus_percentage

data base_price: 200
data discount_rate: 25%

rule price_after_discount: base_price - discount_rate
rule expected: 150

rule test_passes: price_after_discount is expected
"#;

    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_money_minus_percentage",
            Some(&now),
            HashMap::new(),
            false,
        )
        .unwrap();

    let price_after_discount = response.results.get("price_after_discount").unwrap();
    assert_eq!(
        price_after_discount.display.clone().expect("display"),
        "150"
    );

    let test_passes = response.results.get("test_passes").unwrap();
    assert_eq!(test_passes.display.clone().expect("display"), "true");
}

#[test]
fn test_money_plus_percentage() {
    let mut engine = Engine::new();

    let code = r#"
spec test_money_plus_percentage

data base: 100
data markup: 10%

rule price_with_markup: base + markup
rule expected: 110

rule test_passes: price_with_markup is expected
"#;

    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_money_plus_percentage",
            Some(&now),
            HashMap::new(),
            false,
        )
        .unwrap();

    let price_with_markup = response.results.get("price_with_markup").unwrap();
    assert_eq!(price_with_markup.display.clone().expect("display"), "110");

    let test_passes = response.results.get("test_passes").unwrap();
    assert_eq!(test_passes.display.clone().expect("display"), "true");
}

#[test]
fn test_number_times_percentage() {
    let mut engine = Engine::new();

    let code = r#"
spec test_number_times_percentage

data amount: 1000
data rate: 15%

rule result: amount * rate
rule expected: 150

rule test_passes: result is expected
"#;

    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_number_times_percentage",
            Some(&now),
            HashMap::new(),
            false,
        )
        .unwrap();

    let result = response.results.get("result").unwrap();
    assert_eq!(result.display.clone().expect("display"), "150");

    let test_passes = response.results.get("test_passes").unwrap();
    assert_eq!(test_passes.display.clone().expect("display"), "true");
}

#[test]
fn test_money_minus_percentage_with_rule_reference() {
    let mut engine = Engine::new();

    let code = r#"
spec test_with_rule_reference

data base_price: 200
data discount_rate: 25%

rule discount_amount: base_price * discount_rate
rule final_price: base_price - discount_amount
rule expected: 150

rule test_passes: final_price is expected
"#;

    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_with_rule_reference",
            Some(&now),
            HashMap::new(),
            false,
        )
        .unwrap();

    let discount_amount = response.results.get("discount_amount").unwrap();
    assert_eq!(discount_amount.display.clone().expect("display"), "50");

    let final_price = response.results.get("final_price").unwrap();
    assert_eq!(final_price.display.clone().expect("display"), "150");
}

#[test]
fn test_chained_percentage_operations() {
    let mut engine = Engine::new();

    let code = r#"
spec test_chained_percentages

data original_price: 100
data first_discount: 20%
data second_discount: 10%

rule after_first: original_price - first_discount
rule after_second: after_first - second_discount

rule expected: 72

rule test_passes: after_second is expected
"#;

    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_chained_percentages",
            Some(&now),
            HashMap::new(),
            false,
        )
        .unwrap();

    let after_first = response.results.get("after_first").unwrap();
    assert_eq!(after_first.display.clone().expect("display"), "80");

    let after_second = response.results.get("after_second").unwrap();
    assert_eq!(after_second.display.clone().expect("display"), "72");
}
