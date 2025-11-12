use lemma::{Engine, LiteralValue, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn demonstrate_rule_reference_with_vetos() {
    let code = r#"
        doc example
        fact x = [number]
        rule base = x
          unless x > 3 then veto "too much"
          unless x < 0 then veto "too little"

        rule another = base?
          unless x > 5 then veto "way too much"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: another == 3
    let solutions = engine
        .invert(
            "example",
            "another",
            Target::value(LiteralValue::Number(Decimal::from(3))),
            HashMap::new(),
        )
        .expect("inversion should succeed");

    println!("\n=== Inversion Result ===");
    println!("Query: another == 3");
    // Shape is now solutions;
    println!(
        "Free variables: {:?}",
        solutions.iter().flat_map(|r| r.keys())
    );
    println!("========================\n");

    // Expect a solution solution
    assert!(!solutions.is_empty(), "expected at least one solution");

    // Test validates that rule references in expressions are expanded during inversion
}

#[test]
fn demonstrate_no_solution_for_value_7() {
    let code = r#"
        doc example
        fact x = [number]
        rule base = x
          unless x > 3 then veto "too much"
          unless x < 0 then veto "too little"

        rule another = base?
          unless x > 5 then veto "way too much"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: another == 7 should have no satisfying inputs because base vetoes for x > 3
    let result = engine.invert(
        "example",
        "another",
        Target::value(LiteralValue::Number(Decimal::from(7))),
        HashMap::new(),
    );

    println!("\n=== Inversion: another == 7 ===");
    match result {
        Ok(_) => {
            panic!("expected inversion to fail for another == 7");
        }
        Err(err) => {
            println!("Expected failure: {}", err);
        }
    }
    println!("===============================\n");
}

#[test]
fn demonstrate_inversion_with_given_x() {
    let code = r#"
        doc example
        fact x = [number]
        rule base = x
          unless x > 3 then veto "too much"
          unless x < 0 then veto "too little"

        rule another = base?
          unless x > 5 then veto "way too much"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: another == 3, given x = 3
    let mut given = HashMap::new();
    given.insert("x".to_string(), LiteralValue::Number(Decimal::from(3)));

    let solutions = engine
        .invert(
            "example",
            "another",
            Target::value(LiteralValue::Number(Decimal::from(3))),
            given,
        )
        .expect("inversion should succeed");

    println!("\n=== Inversion with x=3 ===");
    println!("Query: another == 3, given x = 3");
    // Shape is now solutions;
    println!(
        "Free variables: {:?}",
        solutions.iter().flat_map(|r| r.keys())
    );
    println!("==========================\n");

    // With x given, there should be no free variables
    assert_eq!(solutions.iter().flat_map(|r| r.keys()).count(), 0);
}

#[test]
fn demonstrate_veto_query() {
    let code = r#"
        doc example
        fact x = [number]
        rule base = x
          unless x > 3 then veto "too much"
          unless x < 0 then veto "too little"

        rule another = base?
          unless x > 5 then veto "way too much"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: when does "another" produce "way too much" veto?
    let solutions = engine
        .invert(
            "example",
            "another",
            Target::veto(Some("way too much".to_string())),
            HashMap::new(),
        )
        .expect("inversion should succeed");

    println!("\n=== Veto Query ===");
    println!("Query: when does another veto with 'way too much'?");
    // Shape is now solutions;
    println!(
        "Free variables: {:?}",
        solutions.iter().flat_map(|r| r.keys())
    );
    println!("==================\n");

    // Should show that x > 5 triggers this veto
}

#[test]
fn demonstrate_any_veto_query() {
    let code = r#"
        doc example
        fact x = [number]
        rule base = x
          unless x > 3 then veto "too much"
          unless x < 0 then veto "too little"

        rule another = base?
          unless x > 5 then veto "way too much"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: when does "another" produce ANY veto?
    let solutions = engine
        .invert(
            "example",
            "another",
            Target::any_veto(), // None = any veto
            HashMap::new(),
        )
        .expect("inversion should succeed");

    println!("\n=== Any Veto Query ===");
    println!("Query: when does another produce any veto?");
    // Shape is now solutions;
    println!(
        "Free variables: {:?}",
        solutions.iter().flat_map(|r| r.keys())
    );
    println!("======================\n");

    // Should show all veto conditions: x < 0, x > 3, x > 5
}
