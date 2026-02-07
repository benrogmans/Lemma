//! Integration test for coffee_order example
//!
//! Tests type imports, inline type declarations with constraints, and complex rule chains

mod common;
use common::add_lemma_code_blocking;
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn load_coffee_order() -> Engine {
    let mut engine = Engine::new();

    // Load the examples document first (contains money and priority types)
    let examples = r#"
doc examples

type money = scale
  -> decimals 2
  -> unit eur 1.00
  -> unit gbp 1.17
  -> minimum 0 eur

type priority = text
  -> option "low"
  -> option "medium"
  -> option "high"
"#;

    let coffee_order = r#"
doc coffee_order

type coffee = text
  -> option "espresso"
  -> option "latte"
  -> option "cappuccino"
  -> option "mocha"

type size = text
  -> option "small"
  -> option "medium"
  -> option "large"

fact price            = [money from examples]
fact priority         = [priority from examples]
fact product          = [coffee]
fact size             = [size -> option "extra large"]
fact number_of_cups   = [number -> maximum 10]
fact has_loyalty_card = [boolean]

rule ordered_priority = veto "Unknown priority"
  unless priority is "low"    then 1
  unless priority is "medium" then 2
  unless priority is "high"   then 3

rule base_price = veto "Unknown type of coffee"
  unless product is "espresso"   then 2.50 eur
  unless product is "latte"      then 3.50 eur
  unless product is "cappuccino" then 3.50 eur
  unless product is "mocha"      then 4.00 eur

rule size_multiplier = veto "Unknown size of coffee"
  unless size is "small"  then 0.80
  unless size is "medium" then 1.00
  unless size is "large"  then 1.20

rule price_per_cup = base_price? * size_multiplier?

rule subtotal = price_per_cup? * number_of_cups

rule loyalty_discount = 0.0
  unless has_loyalty_card then 0.10

rule discount_amount = subtotal? * loyalty_discount?

rule total = subtotal? - discount_amount?
"#;

    add_lemma_code_blocking(&mut engine, examples, "examples.lemma")
        .expect("Failed to parse examples");
    add_lemma_code_blocking(&mut engine, coffee_order, "coffee_order.lemma")
        .expect("Failed to parse coffee_order");

    engine
}

#[test]
fn test_coffee_order_parses() {
    let engine = load_coffee_order();

    // Verify documents are loaded
    let docs = engine.list_documents();
    assert!(docs.contains(&"examples".to_string()));
    assert!(docs.contains(&"coffee_order".to_string()));
}

#[test]
fn test_coffee_order_espresso_small_no_loyalty() {
    let engine = load_coffee_order();

    let fact_values = HashMap::from([
        ("product".to_string(), "espresso".to_string()),
        ("size".to_string(), "small".to_string()),
        ("number_of_cups".to_string(), "2".to_string()),
        ("has_loyalty_card".to_string(), "false".to_string()),
    ]);

    let response = engine
        .evaluate("coffee_order", vec![], fact_values)
        .expect("Evaluation failed");

    // Check base_price: espresso = 2.50 usd
    let base_price = response
        .results
        .values()
        .find(|r| r.rule.name == "base_price")
        .expect("base_price rule not found");

    let base_price_value = base_price
        .result
        .value()
        .expect("base_price should have value");
    // base_price should be Scale with unit "eur"
    match &base_price_value.value {
        lemma::ValueKind::Scale(n, unit) => {
            assert_eq!(
                unit.as_str(),
                "eur",
                "base_price should have unit 'eur', got: {:?}",
                unit
            );
            // base_price preserves the numeric value as written for the unit.
            assert_eq!(
                *n,
                Decimal::from_str("2.50").unwrap(),
                "base_price should be exactly 2.50 (2.50 eur), got: {}",
                n
            );
        }
        _ => panic!(
            "base_price should be Scale type, got: {:?}",
            base_price_value.value
        ),
    }

    // Check size_multiplier: small = 0.80
    let size_multiplier = response
        .results
        .values()
        .find(|r| r.rule.name == "size_multiplier")
        .expect("size_multiplier rule not found");

    let multiplier_value = size_multiplier
        .result
        .value()
        .expect("size_multiplier should have value");
    // size_multiplier should be Number (no unit)
    match &multiplier_value.value {
        lemma::ValueKind::Number(n) => {
            assert_eq!(
                *n,
                Decimal::from_str("0.80").unwrap(),
                "size_multiplier should be 0.80, got: {}",
                n
            );
        }
        _ => panic!(
            "size_multiplier should be Number type, got: {:?}",
            multiplier_value.value
        ),
    }

    // Check price_per_cup = base_price * size_multiplier
    let price_per_cup = response
        .results
        .values()
        .find(|r| r.rule.name == "price_per_cup")
        .expect("price_per_cup rule not found");

    let cup_price = price_per_cup
        .result
        .value()
        .expect("price_per_cup should have value");
    // price_per_cup should be Scale with unit "eur" (inherited from base_price)
    match &cup_price.value {
        lemma::ValueKind::Scale(n, unit) => {
            assert_eq!(
                unit.as_str(),
                "eur",
                "price_per_cup should have unit 'eur', got: {:?}",
                unit
            );
            // base_price = 2.50, size_multiplier = 0.80
            // price_per_cup = 2.50 * 0.80 = 2.00
            assert_eq!(
                *n,
                Decimal::from_str("2.00").unwrap(),
                "price_per_cup should be exactly 2.00 (2.50 * 0.80), got: {}",
                n
            );
        }
        _ => panic!(
            "price_per_cup should be Scale type, got: {:?}",
            cup_price.value
        ),
    }

    // Check subtotal = price_per_cup * 2 cups
    let subtotal = response
        .results
        .values()
        .find(|r| r.rule.name == "subtotal")
        .expect("subtotal rule not found");

    let subtotal_value = subtotal.result.value().expect("subtotal should have value");
    // subtotal should be Scale with unit "eur" (inherited from price_per_cup)
    let subtotal_num = match &subtotal_value.value {
        lemma::ValueKind::Scale(n, unit) => {
            assert_eq!(
                unit.as_str(),
                "eur",
                "subtotal should have unit 'eur', got: {:?}",
                unit
            );
            *n
        }
        _ => panic!(
            "subtotal should be Scale type, got: {:?}",
            subtotal_value.value
        ),
    };
    // price_per_cup = 2.00, number_of_cups = 2
    // subtotal = 2.00 * 2 = 4.00
    assert_eq!(
        subtotal_num,
        Decimal::from_str("4.00").unwrap(),
        "subtotal should be exactly 4.00 (2.00 * 2), got: {}",
        subtotal_num
    );

    // Check loyalty_discount: false = 0.0
    let loyalty_discount = response
        .results
        .values()
        .find(|r| r.rule.name == "loyalty_discount")
        .expect("loyalty_discount rule not found");

    let discount = loyalty_discount
        .result
        .value()
        .expect("loyalty_discount should have value");
    // loyalty_discount: false = 0.0 (should be Number, not Ratio when 0.0)
    match &discount.value {
        lemma::ValueKind::Number(n) => {
            assert_eq!(
                *n,
                Decimal::from_str("0.00").unwrap(),
                "loyalty_discount should be 0.00, got: {}",
                n
            );
        }
        _ => panic!(
            "loyalty_discount should be Number type when 0.0, got: {:?}",
            discount.value
        ),
    }

    // Check total = subtotal - discount_amount (should equal subtotal when no discount)
    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule not found");

    let total_value = total.result.value().expect("total should have value");
    // total should be Scale with unit "eur" (inherited from subtotal)
    let total_num = match &total_value.value {
        lemma::ValueKind::Scale(n, unit) => {
            assert_eq!(
                unit.as_str(),
                "eur",
                "total should have unit 'eur', got: {:?}",
                unit
            );
            *n
        }
        _ => panic!("total should be Scale type, got: {:?}", total_value.value),
    };
    // Total should equal subtotal when discount is 0
    assert!(
        (total_num - subtotal_num).abs() < Decimal::from_str("0.01").unwrap(),
        "total should equal subtotal when discount is 0, got total: {}, subtotal: {}",
        total_num,
        subtotal_num
    );
}

#[test]
fn test_coffee_order_latte_large_with_loyalty() {
    let engine = load_coffee_order();

    let fact_values = HashMap::from([
        ("product".to_string(), "latte".to_string()),
        ("size".to_string(), "large".to_string()),
        ("number_of_cups".to_string(), "3".to_string()),
        ("has_loyalty_card".to_string(), "true".to_string()),
    ]);

    let response = engine
        .evaluate("coffee_order", vec![], fact_values)
        .expect("Evaluation failed");

    // Check base_price: latte = 3.50 usd
    let base_price = response
        .results
        .values()
        .find(|r| r.rule.name == "base_price")
        .expect("base_price rule not found");

    let base_price_value = base_price
        .result
        .value()
        .expect("base_price should have value");
    // base_price should be Scale with unit "eur"
    match &base_price_value.value {
        lemma::ValueKind::Scale(n, unit) => {
            assert_eq!(
                unit.as_str(),
                "eur",
                "base_price should have unit 'eur', got: {:?}",
                unit
            );
            // base_price preserves the numeric value as written for the unit.
            assert_eq!(
                *n,
                Decimal::from_str("3.50").unwrap(),
                "base_price should be exactly 3.50 (3.50 eur), got: {}",
                n
            );
        }
        _ => panic!(
            "base_price should be Scale type, got: {:?}",
            base_price_value.value
        ),
    }

    // Check size_multiplier: large = 1.20
    let size_multiplier = response
        .results
        .values()
        .find(|r| r.rule.name == "size_multiplier")
        .expect("size_multiplier rule not found");

    let multiplier_value = size_multiplier
        .result
        .value()
        .expect("size_multiplier should have value");
    // size_multiplier should be Number (no unit)
    match &multiplier_value.value {
        lemma::ValueKind::Number(n) => {
            assert_eq!(
                *n,
                Decimal::from_str("1.20").unwrap(),
                "size_multiplier should be 1.20, got: {}",
                n
            );
        }
        _ => panic!(
            "size_multiplier should be Number type, got: {:?}",
            multiplier_value.value
        ),
    }

    // Check loyalty_discount: true = 0.10
    // Note: 0.10 is written as a number literal, not "10%", so it's a Number, not a Ratio
    let loyalty_discount = response
        .results
        .values()
        .find(|r| r.rule.name == "loyalty_discount")
        .expect("loyalty_discount rule not found");

    let discount = loyalty_discount
        .result
        .value()
        .expect("loyalty_discount should have value");
    // loyalty_discount should be Number (since 0.10 is written as number, not percentage)
    match &discount.value {
        lemma::ValueKind::Number(n) => {
            assert_eq!(
                *n,
                Decimal::from_str("0.10").unwrap(),
                "loyalty_discount should be exactly 0.10, got: {}",
                n
            );
        }
        _ => panic!(
            "loyalty_discount should be Number type, got: {:?}",
            discount.value
        ),
    }

    // Check total should be less than subtotal (due to discount)
    let subtotal = response
        .results
        .values()
        .find(|r| r.rule.name == "subtotal")
        .expect("subtotal rule not found");

    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule not found");

    let subtotal_value = subtotal.result.value().expect("subtotal should have value");
    let total_value = total.result.value().expect("total should have value");

    // subtotal should be Scale with unit "eur" (inherited from price_per_cup)
    let subtotal_num = match &subtotal_value.value {
        lemma::ValueKind::Scale(n, unit) => {
            assert_eq!(
                unit.as_str(),
                "eur",
                "subtotal should have unit 'eur', got: {:?}",
                unit
            );
            *n
        }
        _ => panic!(
            "subtotal should be Scale type, got: {:?}",
            subtotal_value.value
        ),
    };
    // price_per_cup = 3.50 * 1.20 = 4.20, number_of_cups = 3
    // subtotal = 4.20 * 3 = 12.60
    assert_eq!(
        subtotal_num,
        Decimal::from_str("12.60").unwrap(),
        "subtotal should be exactly 12.60 (4.20 * 3), got: {}",
        subtotal_num
    );

    // total should be Scale with unit "eur" (inherited from subtotal)
    let total_num = match &total_value.value {
        lemma::ValueKind::Scale(n, unit) => {
            assert_eq!(
                unit.as_str(),
                "eur",
                "total should have unit 'eur', got: {:?}",
                unit
            );
            *n
        }
        _ => panic!("total should be Scale type, got: {:?}", total_value.value),
    };
    // discount_amount = 12.60 * 0.10 = 1.26
    // total = 12.60 - 1.26 = 11.34
    assert_eq!(
        total_num,
        Decimal::from_str("11.34").unwrap(),
        "total should be exactly 11.34 (12.60 - 1.26), got: {}",
        total_num
    );
}

#[test]
fn test_coffee_order_ordered_priority() {
    let engine = load_coffee_order();

    // Test priority mapping
    let priorities = ["low", "medium", "high"];
    let expected_values = ["1", "2", "3"];

    for (priority, expected) in priorities.iter().zip(expected_values.iter()) {
        let fact_values = HashMap::from([("priority".to_string(), priority.to_string())]);

        let response = engine
            .evaluate("coffee_order", vec![], fact_values)
            .expect("Evaluation failed");

        let ordered_priority = response
            .results
            .values()
            .find(|r| r.rule.name == "ordered_priority")
            .expect("ordered_priority rule not found");

        let priority_value = ordered_priority
            .result
            .value()
            .expect("ordered_priority should have value");
        assert_eq!(
            priority_value.to_string(),
            *expected,
            "priority '{}' should map to {}, got: {}",
            priority,
            expected,
            priority_value
        );
    }
}

#[test]
fn test_coffee_order_invalid_size_veto() {
    let engine = load_coffee_order();

    // Size "extra large" is defined in the inline type constraint, but size_multiplier
    // only handles small/medium/large, so it should veto
    let fact_values = HashMap::from([
        ("product".to_string(), "espresso".to_string()),
        ("size".to_string(), "extra large".to_string()),
        ("number_of_cups".to_string(), "1".to_string()),
    ]);

    let response = engine
        .evaluate("coffee_order", vec![], fact_values)
        .expect("Evaluation should complete (even with veto)");

    let size_multiplier = response
        .results
        .values()
        .find(|r| r.rule.name == "size_multiplier")
        .expect("size_multiplier rule not found");

    // size_multiplier should veto because "extra large" is not handled
    assert!(
        size_multiplier.result.is_veto(),
        "size_multiplier should veto for 'extra large' size"
    );

    // price_per_cup and subsequent rules should also fail due to dependency
    let price_per_cup = response
        .results
        .values()
        .find(|r| r.rule.name == "price_per_cup");

    if let Some(price_per_cup) = price_per_cup {
        assert!(
            price_per_cup.result.is_veto() || price_per_cup.result.value().is_none(),
            "price_per_cup should fail when size_multiplier vetoes"
        );
    }
}
