//! Tests for all documentation example files
//!
//! Ensures all example files in documentation/examples/ are valid and can be evaluated

use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn get_rule_value(
    engine: &Engine,
    doc_name: &str,
    rule_name: &str,
    facts: HashMap<String, String>,
) -> lemma::LiteralValue {
    let response = engine.evaluate(doc_name, vec![], facts).unwrap();
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("rule '{}' not found in {}", rule_name, doc_name))
        .result
        .value()
        .unwrap_or_else(|| panic!("rule '{}' had no value", rule_name))
        .clone()
}

fn load_documentation_examples() -> Engine {
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
            .add_lemma_code(&content, path)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path, e));
    }

    engine
}

#[test]
fn test_01_coffee_order() {
    let engine = load_documentation_examples();

    let mut facts = HashMap::new();
    facts.insert("product".to_string(), "latte".to_string());
    facts.insert("size".to_string(), "large".to_string());
    facts.insert("number_of_cups".to_string(), "2".to_string());
    facts.insert("has_loyalty_card".to_string(), "true".to_string());
    facts.insert("age".to_string(), "70".to_string());

    let total = get_rule_value(&engine, "coffee_order", "total", facts);

    // latte base_price (3.50 eur) * large size_multiplier (120%) = 4.20 eur per cup
    // 4.20 eur * 2 cups = 8.40 eur subtotal
    // loyalty card discount 10% = 0.84 eur (age >= 65, so age_discount = 10%)
    // discount_amount = 0.84 eur - 10% = 0.84 - 0.084 = 0.756 eur
    // total = 8.40 - 0.756 = 7.644 eur
    assert_eq!(
        total.value,
        lemma::Value::Scale(Decimal::from_str("7.644").unwrap(), Some("eur".to_string()))
    );
}

#[test]
fn test_02_library_fees() {
    let engine = load_documentation_examples();

    let mut facts = HashMap::new();
    facts.insert("days_overdue".to_string(), "5".to_string());
    facts.insert("book_type".to_string(), "regular".to_string());
    facts.insert("is_first_offense".to_string(), "false".to_string());

    let final_fee = get_rule_value(&engine, "library_fees", "final_fee", facts.clone());
    assert_eq!(
        final_fee.value,
        lemma::Value::Scale(Decimal::from_str("1.25").unwrap(), Some("eur".to_string()))
    );

    let can_checkout = get_rule_value(&engine, "library_fees", "can_checkout", facts);
    assert_eq!(
        can_checkout.value,
        lemma::Value::Boolean(lemma::BooleanValue::True)
    );
}

#[test]
fn test_03_recipe_scaling() {
    let engine = load_documentation_examples();

    let mut facts = HashMap::new();
    facts.insert("original_servings".to_string(), "4".to_string());
    facts.insert("desired_servings".to_string(), "8".to_string());
    facts.insert("recipe_name".to_string(), "chocolate_cake".to_string());

    let scaling_factor = get_rule_value(&engine, "recipe_scaling", "scaling_factor", facts.clone());
    assert_eq!(
        scaling_factor.value,
        lemma::Value::Number(Decimal::from_str("2").unwrap())
    );

    let baking_time = get_rule_value(
        &engine,
        "recipe_scaling",
        "baking_time_minutes",
        facts.clone(),
    );
    assert_eq!(
        baking_time.value,
        lemma::Value::Number(Decimal::from_str("40").unwrap())
    );

    let oven_temp = get_rule_value(&engine, "recipe_scaling", "oven_temperature", facts);
    assert_eq!(
        oven_temp.value,
        lemma::Value::Scale(
            Decimal::from_str("175").unwrap(),
            Some("celsius".to_string())
        )
    );
}

#[test]
fn test_04_membership_benefits() {
    let engine = load_documentation_examples();

    // Test premium_membership document (has rules, no facts needed)
    let discount_rate = get_rule_value(
        &engine,
        "premium_membership",
        "discount_rate",
        HashMap::new(),
    );
    assert_eq!(
        discount_rate.value,
        lemma::Value::Ratio(
            Decimal::from_str("0.10").unwrap(),
            Some("percent".to_string())
        )
    );

    // Test membership_benefits document (references premium_membership)
    let discount = get_rule_value(&engine, "membership_benefits", "discount", HashMap::new());
    assert_eq!(
        discount.value,
        lemma::Value::Number(Decimal::from_str("15").unwrap())
    );

    let shipping_cost = get_rule_value(
        &engine,
        "membership_benefits",
        "shipping_cost",
        HashMap::new(),
    );
    assert_eq!(
        shipping_cost.value,
        lemma::Value::Number(Decimal::from_str("0").unwrap())
    );

    let total_points = get_rule_value(
        &engine,
        "membership_benefits",
        "total_points",
        HashMap::new(),
    );
    assert_eq!(
        total_points.value,
        lemma::Value::Number(Decimal::from_str("325").unwrap())
    );
}

#[test]
fn test_05_weather_clothing() {
    let engine = load_documentation_examples();

    let mut facts = HashMap::new();
    facts.insert("temperature".to_string(), "15 celsius".to_string());
    facts.insert("is_raining".to_string(), "false".to_string());
    facts.insert("wind_speed".to_string(), "10".to_string());

    let clothing_layer =
        get_rule_value(&engine, "weather_clothing", "clothing_layer", facts.clone());
    assert_eq!(
        clothing_layer.value,
        lemma::Value::Text("light".to_string())
    );

    let needs_jacket = get_rule_value(&engine, "weather_clothing", "needs_jacket", facts);
    assert_eq!(
        needs_jacket.value,
        lemma::Value::Boolean(lemma::BooleanValue::False)
    );
}

#[test]
fn test_all_documentation_examples_parse() {
    // This test just ensures all examples can be loaded without errors
    let engine = load_documentation_examples();

    // Verify all documents are loaded
    let docs = engine.list_documents();

    // Verify we have at least the expected documents loaded
    assert!(
        docs.len() >= 6,
        "Expected at least 6 documents (examples + coffee_order), found {}. Available: {:?}",
        docs.len(),
        docs
    );

    // Verify key documents exist
    let key_docs = vec![
        "coffee_order",        // from 01_coffee_order.lemma
        "library_fees",        // from 02_library_fees.lemma
        "recipe_scaling",      // from 03_recipe_scaling.lemma
        "premium_membership",  // from 04_membership_benefits.lemma
        "membership_benefits", // from 04_membership_benefits.lemma
        "weather_clothing",    // from 05_weather_clothing.lemma
    ];

    for expected in key_docs {
        assert!(
            docs.contains(&expected.to_string()),
            "Expected document '{}' not found. Available: {:?}",
            expected,
            docs
        );
    }
}
