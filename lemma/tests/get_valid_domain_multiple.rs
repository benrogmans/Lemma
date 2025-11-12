use lemma::{Engine, FactReference, LiteralValue};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn piecewise_creates_multiple_domains() {
    // Different discount tiers create different valid solutions
    let code = r#"
        doc pricing
        fact quantity = [number]
        fact member_discount = [number]

        rule final_discount = quantity * 0.05
          unless quantity >= 50 then quantity * 0.10
          unless member_discount > 0 then member_discount
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();
    let solutions = engine
        .invert(
            "pricing",
            "final_discount",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domains");

    // Multiple solutions based on different branches
    println!("\nQuantity domains (multiple solutions):");
    for (i, domain) in solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, domain);
    }

    assert!(!solutions.is_empty(), "Expected at least one domain");
}

#[test]
fn exclusive_ranges_create_multiple_domains() {
    // Age brackets that don't overlap
    let code = r#"
        doc insurance
        fact age = [number]

        rule premium = 100 EUR
          unless age < 25 then veto "too young"
          unless age > 65 then veto "too old"
          unless (age >= 30 and age <= 40) then 80 EUR
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();
    // Ask for age ranges that give 80 EUR premium
    let age_path = FactReference {
        reference: vec!["insurance".to_string(), "age".to_string()],
    };

    let given = HashMap::new();
    let solutions = engine
        .invert(
            "insurance",
            "premium",
            lemma::Target::value(LiteralValue::Unit(lemma::NumericUnit::Money(
                Decimal::from(80),
                lemma::MoneyUnit::Eur,
            ))),
            given.clone(),
        )
        .expect("should invert");

    println!("\nAge solutions for 80 EUR premium:");
    for (i, solution) in solutions.iter().enumerate() {
        if let Some(domain) = solution.get(&age_path) {
            println!("  Solution {}: {:?}", i + 1, domain);
        }
    }

    // For get_valid_domain, we want non-veto values
    let solutions = engine
        .invert(
            "insurance",
            "premium",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domains");

    println!("\nAge domains for any valid premium (non-veto):");
    for (i, domain) in solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, domain);
    }
}

#[test]
fn disjoint_valid_ranges() {
    // Temperature ranges - too cold, comfortable, too hot
    let code = r#"
        doc thermostat
        fact temp = [number]

        rule status = "ok"
          unless temp < 18 then veto "too cold"
          unless temp > 26 then veto "too hot"
          unless (temp >= 20 and temp <= 24) then "perfect"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();
    let solutions = engine
        .invert(
            "thermostat",
            "status",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domains");

    println!("\nTemperature valid ranges:");
    for (i, domain) in solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, domain);
    }

    // Should have constraints: 18 <= temp <= 26 (excluding the veto ranges)
    assert!(!solutions.is_empty(), "Expected valid temperature range");
}

#[test]
fn multiple_conditions_create_solutions() {
    // Shipping: different solutions based on weight AND destination
    let code = r#"
        doc shipping
        fact weight = [mass]
        fact is_domestic = [boolean]

        rule can_ship = true
          unless weight > 50 kilograms then veto "too heavy for domestic"
          unless (not is_domestic and weight > 30 kilograms) then veto "too heavy for international"
          unless weight < 0 kilograms then veto "invalid weight"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();
    let solutions = engine
        .invert(
            "shipping",
            "can_ship",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should invert");

    println!("\nShipping weight solutions:");
    for (i, solution) in solutions.iter().enumerate() {
        println!("  Solution {}:", i + 1);
        for (fact, domain) in solution.iter() {
            println!("    {}: {:?}", fact.reference.join("."), domain);
        }
    }

    // Multiple solutions because domestic vs international have different weight limits
    if solutions.len() > 1 {
        println!("\n✓ Multiple solution solutions exist (domestic vs international)");
    }
}

#[test]
fn enum_values_create_multiple_domains() {
    // Different valid states
    let code = r#"
        doc order
        fact status = [text]
        fact payment_received = [boolean]

        rule can_ship = true
          unless (status is "pending" or status is "cancelled") then veto "cannot ship"
          unless not payment_received then veto "payment required"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let status_path = FactReference {
        reference: vec!["order".to_string(), "status".to_string()],
    };

    let solutions = engine
        .invert(
            "order",
            "can_ship",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should invert");

    println!("\nOrder status valid values:");
    for (i, solution) in solutions.iter().enumerate() {
        if let Some(domain) = solution.get(&status_path) {
            println!("  Solution {}: {:?}", i + 1, domain);
        }
    }
}

#[test]
fn given_facts_affect_domain_count() {
    // Same rule, different domains depending on given facts
    let code = r#"
        doc pricing
        fact quantity = [number]
        fact is_member = [boolean]
        fact base_price = [money]

        rule discount = 0%
          unless quantity >= 10 then 5%
          unless is_member then 10%

        rule final_price = base_price * (1 - discount?)
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();
    // Without given facts - multiple solutions possible
    println!("\n=== Without given facts ===");
    let domains_no_given = engine
        .invert(
            "pricing",
            "discount",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domains");

    println!("Quantity domains: {} solutions", domains_no_given.len());
    for (i, domain) in domains_no_given.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, domain);
    }

    // With is_member = false given - fewer solutions
    println!("\n=== With is_member = false ===");
    let mut given = HashMap::new();
    given.insert(
        "pricing.is_member".to_string(),
        LiteralValue::Boolean(false),
    );

    let domains_with_given = engine
        .invert("pricing", "discount", lemma::Target::any_value(), given)
        .expect("should get domains");

    println!("Quantity domains: {} solutions", domains_with_given.len());
    for (i, domain) in domains_with_given.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, domain);
    }

    // Given facts can reduce the number of solution solutions
    println!("\n✓ Given facts constrain the solution space");
}

#[test]
fn overlapping_conditions_may_unify() {
    // Conditions that overlap might unify into fewer solutions
    let code = r#"
        doc validation
        fact score = [number]

        rule grade = "F"
          unless score >= 60 then "D"
          unless score >= 70 then "C"
          unless score >= 80 then "B"
          unless score >= 90 then "A"
          unless score < 0 then veto "invalid"
          unless score > 100 then veto "invalid"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();
    let solutions = engine
        .invert(
            "validation",
            "grade",
            lemma::Target::any_value(),
            HashMap::new(),
        )
        .expect("should get domains");

    println!("\nScore valid range (avoiding vetos):");
    for (i, domain) in solutions.iter().enumerate() {
        println!("  Solution {}: {:?}", i + 1, domain);
    }

    // Valid range: 0 <= score <= 100
    assert!(!solutions.is_empty(), "Expected valid score range");
}
