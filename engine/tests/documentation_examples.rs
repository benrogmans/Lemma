//! Tests for example files under documentation/examples/
//!
//! Ensures all example files in documentation/examples/ are valid and can be evaluated

use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, SemanticDurationUnit};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn get_rule_value(
    engine: &Engine,
    spec_name: &str,
    rule_name: &str,
    data: HashMap<String, String>,
) -> lemma::LiteralValue {
    let now = DateTimeValue::now();
    let response = engine.run(spec_name, Some(&now), data, false).unwrap();
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("rule '{}' not found in {}", rule_name, spec_name))
        .result
        .value()
        .unwrap_or_else(|| panic!("rule '{}' had no value", rule_name))
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
    ];

    for path in examples {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
        engine
            .load(&content, lemma::SourceType::Labeled(path))
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
    // loyalty card discount 10% = 0.84 eur (age >= 65, so age_discount = 10%)
    // discount_amount = 0.84 eur - 10% = 0.84 - 0.084 = 0.756 eur
    // total = 8.40 - 0.756 = 7.644 eur
    assert_eq!(
        total.value,
        lemma::ValueKind::Scale(Decimal::from_str("7.644").unwrap(), "eur".to_string())
    );
}

#[test]
fn test_02_library_fees() {
    let engine = load_specs_folder_examples();

    let mut data = HashMap::new();
    data.insert("days_overdue".to_string(), "5".to_string());
    data.insert("book_type".to_string(), "regular".to_string());
    data.insert("is_first_offense".to_string(), "false".to_string());

    let final_fee = get_rule_value(&engine, "library_fees", "final_fee", data.clone());
    assert_eq!(
        final_fee.value,
        lemma::ValueKind::Scale(Decimal::from_str("1.25").unwrap(), "eur".to_string())
    );

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
        lemma::ValueKind::Number(Decimal::from_str("2").unwrap())
    );

    let baking_time = get_rule_value(&engine, "recipe_scaling", "baking_time", data.clone());
    assert_eq!(
        baking_time.value,
        lemma::ValueKind::Duration(
            Decimal::from_str("40").unwrap(),
            SemanticDurationUnit::Minute.clone()
        )
    );

    let oven_temp = get_rule_value(&engine, "recipe_scaling", "oven_temperature", data);
    assert_eq!(
        oven_temp.value,
        lemma::ValueKind::Scale(Decimal::from_str("175").unwrap(), "celsius".to_string())
    );
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
        lemma::ValueKind::Ratio(
            Decimal::from_str("0.10").unwrap(),
            Some("percent".to_string())
        )
    );

    // Test membership_benefits spec (references premium_membership)
    let discount = get_rule_value(&engine, "membership_benefits", "discount", HashMap::new());
    assert_eq!(
        discount.value,
        lemma::ValueKind::Number(Decimal::from_str("15").unwrap())
    );

    let shipping_cost = get_rule_value(
        &engine,
        "membership_benefits",
        "shipping_cost",
        HashMap::new(),
    );
    assert_eq!(
        shipping_cost.value,
        lemma::ValueKind::Number(Decimal::from_str("0").unwrap())
    );

    let total_points = get_rule_value(
        &engine,
        "membership_benefits",
        "total_points",
        HashMap::new(),
    );
    assert_eq!(
        total_points.value,
        lemma::ValueKind::Number(Decimal::from_str("325").unwrap())
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
