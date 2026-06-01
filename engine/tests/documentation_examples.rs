//! Tests for example files under documentation/examples/
//!
//! Ensures all example files in documentation/examples/ are valid and can be evaluated

use lemma::{DateTimeValue, Engine, LiteralValue, ValueKind};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn decimal_lit(s: &str) -> Decimal {
    Decimal::from_str(s).expect("BUG: test decimal literal must parse")
}

fn get_rule_value(
    engine: &Engine,
    spec_name: &str,
    rule_name: &str,
    data: HashMap<String, String>,
) -> lemma::LiteralValue {
    let now = DateTimeValue::now();
    let response = engine.run(None, spec_name, Some(&now), data, true).unwrap();
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("rule '{}' not found in {}", rule_name, spec_name))
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value")
        .clone()
}

fn load_specs_folder_examples() -> Engine {
    let mut engine = Engine::new();

    // Load all example files - paths relative to lemma/ crate root (same pattern as integration_examples.rs)
    let examples = [
        "../documentation/examples/01_coffee_order.lemma",
        "../documentation/examples/02_library_fees.lemma",
        "../documentation/examples/03_recipe_scaling.lemma",
        "../documentation/examples/04_membership_benefits.lemma",
        "../documentation/examples/05_weather_clothing.lemma",
        "../documentation/examples/06_dutch_net_salary.lemma",
    ];

    for path in examples {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
        engine
            .load(
                &content,
                lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(path))),
            )
            .unwrap_or_else(|errs| {
                panic!(
                    "Failed to parse {}: {}",
                    path,
                    errs.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            });
    }

    engine
}

#[test]
fn test_01_coffee_order() {
    let engine = load_specs_folder_examples();

    let mut data = HashMap::new();
    data.insert("product".to_string(), "latte".to_string());
    data.insert("size".to_string(), "large".to_string());
    data.insert("number_of_cups".to_string(), "2".to_string());
    data.insert("has_loyalty_card".to_string(), "true".to_string());
    data.insert("age".to_string(), "70".to_string());

    let total = get_rule_value(&engine, "coffee_order", "total", data);

    // latte base_price (3.50 eur) * large size_multiplier (120%) = 4.20 eur per cup
    // 4.20 eur * 2 cups = 8.40 eur subtotal
    // loyalty_discount 10% + age_discount 10% = combined_discount 20%
    // discount_amount = 8.40 * 20% = 1.68 eur
    // total = 8.40 - 1.68 = 6.72 eur
    match &total.value {
        ValueKind::Quantity(n, signature) => {
            assert_eq!(
                ValueKind::Number(*n).as_decimal_magnitude().unwrap(),
                decimal_lit("6.72")
            );
            assert_eq!(
                signature.first().map(|(name, _)| name.as_str()),
                Some("eur")
            );
        }
        other => panic!("expected Quantity total, got {other:?}"),
    }
}

#[test]
fn test_02_library_fees() {
    let engine = load_specs_folder_examples();

    let mut data = HashMap::new();
    data.insert("days_overdue".to_string(), "5".to_string());
    data.insert("book_type".to_string(), "regular".to_string());
    data.insert("is_first_offense".to_string(), "false".to_string());

    let final_fee = get_rule_value(&engine, "library_fees", "final_fee", data.clone());
    match &final_fee.value {
        ValueKind::Quantity(n, signature) => {
            assert_eq!(
                ValueKind::Number(*n).as_decimal_magnitude().unwrap(),
                decimal_lit("1.25")
            );
            assert_eq!(
                signature.first().map(|(name, _)| name.as_str()),
                Some("eur")
            );
        }
        other => panic!("expected Quantity final_fee, got {other:?}"),
    }

    let can_checkout = get_rule_value(&engine, "library_fees", "can_checkout", data);
    assert_eq!(can_checkout.value, lemma::ValueKind::Boolean(true));
}

#[test]
fn test_03_recipe_scaling() {
    let engine = load_specs_folder_examples();

    let mut data = HashMap::new();
    data.insert("original_servings".to_string(), "4".to_string());
    data.insert("desired_servings".to_string(), "8".to_string());
    data.insert("recipe_name".to_string(), "chocolate_cake".to_string());

    let scaling_factor = get_rule_value(&engine, "recipe_scaling", "scaling_factor", data.clone());
    assert_eq!(
        scaling_factor.value,
        LiteralValue::number_from_decimal(decimal_lit("2")).value
    );

    let baking_time = get_rule_value(&engine, "recipe_scaling", "baking_time", data.clone());
    match &baking_time.value {
        ValueKind::Quantity(n, signature) => {
            assert_eq!(
                ValueKind::Number(*n).as_decimal_magnitude().unwrap(),
                decimal_lit("2400")
            );
            assert_eq!(
                signature.first().map(|(name, _)| name.as_str()),
                Some("minutes")
            );
        }
        other => panic!("expected Quantity baking_time, got {other:?}"),
    }

    let oven_temp = get_rule_value(&engine, "recipe_scaling", "oven_temperature", data);
    match &oven_temp.value {
        ValueKind::Quantity(n, signature) => {
            assert_eq!(
                ValueKind::Number(*n).as_decimal_magnitude().unwrap(),
                decimal_lit("175")
            );
            assert_eq!(
                signature.first().map(|(name, _)| name.as_str()),
                Some("celsius")
            );
        }
        other => panic!("expected Quantity oven_temperature, got {other:?}"),
    }
}

#[test]
fn test_04_membership_benefits() {
    let engine = load_specs_folder_examples();

    // Test premium_membership spec (has rules, no data needed)
    let discount_rate = get_rule_value(
        &engine,
        "premium_membership",
        "discount_rate",
        HashMap::new(),
    );
    assert_eq!(
        discount_rate.value,
        LiteralValue::ratio_from_decimal(decimal_lit("0.10"), Some("percent".to_string())).value
    );

    // Test membership_benefits spec (references premium_membership)
    let discount = get_rule_value(&engine, "membership_benefits", "discount", HashMap::new());
    assert_eq!(
        discount.value,
        LiteralValue::number_from_decimal(decimal_lit("15")).value
    );

    let shipping_cost = get_rule_value(
        &engine,
        "membership_benefits",
        "shipping_cost",
        HashMap::new(),
    );
    assert_eq!(
        shipping_cost.value,
        LiteralValue::number_from_decimal(decimal_lit("0")).value
    );

    let total_points = get_rule_value(
        &engine,
        "membership_benefits",
        "total_points",
        HashMap::new(),
    );
    assert_eq!(
        total_points.value,
        LiteralValue::number_from_decimal(decimal_lit("325")).value
    );
}

#[test]
fn test_05_weather_clothing() {
    let engine = load_specs_folder_examples();

    let mut data = HashMap::new();
    data.insert("temperature".to_string(), "15 celsius".to_string());
    data.insert("is_raining".to_string(), "false".to_string());
    data.insert("wind_speed".to_string(), "10".to_string());

    let clothing_layer =
        get_rule_value(&engine, "weather_clothing", "clothing_layer", data.clone());
    assert_eq!(
        clothing_layer.value,
        lemma::ValueKind::Text("light".to_string())
    );

    let needs_jacket = get_rule_value(&engine, "weather_clothing", "needs_jacket", data);
    assert_eq!(needs_jacket.value, lemma::ValueKind::Boolean(false));
}

#[test]
fn test_06_dutch_net_salary() {
    let engine = load_specs_folder_examples();

    let effective = DateTimeValue {
        year: 2026,
        month: 6,
        day: 1,
        hour: 12,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
    };

    let mut data = HashMap::new();
    data.insert("gross_salary".to_string(), "5000 eur".to_string());
    data.insert("pay_period".to_string(), "month".to_string());

    let response = engine
        .run(None, "net_salary", Some(&effective), data, false)
        .expect("net_salary run");

    let net = response
        .results
        .get("net_salary")
        .expect("net_salary in results");
    assert!(
        !net.vetoed,
        "net_salary must not veto with all required data provided"
    );

    let gross_annual = response
        .results
        .get("gross_annual_salary")
        .expect("gross_annual_salary in results");
    assert!(!gross_annual.vetoed, "gross_annual_salary must not veto");
}
