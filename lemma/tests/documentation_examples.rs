//! Tests for all documentation example files
//!
//! Ensures all example files in documentation/examples/ are valid and can be evaluated

use lemma::Engine;
use std::collections::HashMap;

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
    facts.insert("price".to_string(), "5.00 usd".to_string());
    facts.insert("priority".to_string(), "medium".to_string());
    facts.insert("product".to_string(), "latte".to_string());
    facts.insert("size".to_string(), "large".to_string());
    facts.insert("number_of_cups".to_string(), "2".to_string());
    facts.insert("has_loyalty_card".to_string(), "true".to_string());

    let response = engine
        .evaluate("coffee_order", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "coffee_order");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "base_price"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "size_multiplier"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "price_per_cup"));
    assert!(response.results.values().any(|r| r.rule.name == "subtotal"));
    assert!(response.results.values().any(|r| r.rule.name == "total"));
}

#[test]
fn test_02_library_fees() {
    let engine = load_documentation_examples();

    let mut facts = HashMap::new();
    facts.insert("days_overdue".to_string(), "5".to_string());
    facts.insert("book_type".to_string(), "regular".to_string());
    facts.insert("is_first_offense".to_string(), "false".to_string());

    let response = engine
        .evaluate("library_fees", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "library_fees");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "daily_fee"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "is_in_grace_period"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_fee"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "final_fee"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "can_checkout"));
}

#[test]
fn test_03_recipe_scaling() {
    let engine = load_documentation_examples();

    let mut facts = HashMap::new();
    facts.insert("original_servings".to_string(), "4".to_string());
    facts.insert("desired_servings".to_string(), "8".to_string());
    facts.insert("recipe_name".to_string(), "chocolate_cake".to_string());

    let response = engine
        .evaluate("recipe_scaling", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "recipe_scaling");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "scaling_factor"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "scaled_flour"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "scaled_sugar"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "scaled_butter"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "scaled_eggs"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_dry_ingredients"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "baking_time_minutes"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "oven_temperature"));
}

#[test]
fn test_04_membership_benefits() {
    let engine = load_documentation_examples();

    // Test premium_membership document (has rules, no facts needed)
    let response = engine
        .evaluate("premium_membership", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "premium_membership");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "discount_rate"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "free_shipping_threshold"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "points_multiplier"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "monthly_bonus_points"));

    // Test membership_benefits document (references premium_membership)
    let response = engine
        .evaluate("membership_benefits", vec![], HashMap::new())
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "membership_benefits");
    assert!(response.results.values().any(|r| r.rule.name == "discount"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "shipping_cost"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "base_points"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "bonus_points"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_points"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "total_savings"));
}

#[test]
fn test_05_weather_clothing() {
    let engine = load_documentation_examples();

    let mut facts = HashMap::new();
    facts.insert("temperature".to_string(), "15 celsius".to_string());
    facts.insert("is_raining".to_string(), "false".to_string());
    facts.insert("wind_speed".to_string(), "10".to_string());

    let response = engine
        .evaluate("weather_clothing", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "weather_clothing");
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "clothing_layer"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "needs_jacket"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "needs_umbrella"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "needs_hat"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "comfort_level"));
    assert!(response
        .results
        .values()
        .any(|r| r.rule.name == "recommendation"));
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
        "examples",            // from 01_coffee_order.lemma
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
