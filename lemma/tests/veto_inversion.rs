use lemma::{Engine, Target};

#[test]
fn veto_query_specific_message() {
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule shipping_cost = 5 EUR
             unless weight < 0 kilograms then veto "invalid"
             unless weight > 100 kilograms then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: "What weight values trigger 'too heavy' veto?"
    let solutions = engine
        .invert(
            "shipping",
            "shipping_cost",
            Target::veto(Some("too heavy".to_string())),
            std::collections::HashMap::new(),
        )
        .expect("veto inversion should succeed");

    // Should have exactly one solution solution: weight > 100
    assert_eq!(solutions.len(), 1, "expected one veto solution");

    // Should have domain constraint for weight
    assert!(!solutions[0].is_empty(), "expected domain constraints");
}

#[test]
fn veto_query_any_veto() {
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule shipping_cost = 5 EUR
             unless weight < 0 kilograms then veto "invalid"
             unless weight > 100 kilograms then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: "What weight values trigger ANY veto?"
    let solutions = engine
        .invert(
            "shipping",
            "shipping_cost",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("veto inversion should succeed");

    // Should have two solution solutions: weight < 0 and weight > 100
    assert_eq!(solutions.len(), 2, "expected two veto solutions");

    // Each solution should have domain constraints
    for solution in &solutions {
        assert!(
            !solution.is_empty(),
            "expected domain constraints in each solution"
        );
    }
}

#[test]
fn veto_query_with_value_branches_filters_correctly() {
    let code = r#"
        doc pricing
        fact discount = [percentage]

        rule final_price = 100 EUR
             unless discount >= 50% then veto "discount too high"
             unless discount < 0% then veto "invalid discount"
             unless discount >= 10% then 90 EUR
             unless discount >= 25% then 75 EUR
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: "What discount values trigger any veto?"
    let solutions = engine
        .invert(
            "pricing",
            "final_price",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("should invert successfully");

    // Should have the matching veto solutions
    // Should only have the two veto solutions, not the value solutions
    assert_eq!(solutions.len(), 2, "expected only veto solutions");

    // Each solution should have domain constraints
    for solution in &solutions {
        assert!(
            !solution.is_empty(),
            "all solutions should have domain constraints"
        );
    }
}

#[test]
fn veto_query_no_veto_clauses_should_error() {
    let code = r#"
        doc simple
        fact x = [number]
        rule y = x + 1
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: "What x values trigger a veto?"
    let result = engine.invert(
        "simple",
        "y",
        Target::any_veto(),
        std::collections::HashMap::new(),
    );

    assert!(
        result.is_err(),
        "should fail when querying veto on rule with no veto clauses"
    );
}

#[test]
fn veto_query_last_wins_semantics() {
    let code = r#"
        doc test
        fact x = [number]

        rule result = 0
             unless x < 0 then veto "negative"
             unless x < 10 then 1
             unless x < 5 then veto "overridden"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: "What x values trigger any veto?"
    let solutions = engine
        .invert(
            "test",
            "result",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("veto inversion should succeed");

    // Last-wins semantics generates effective conditions that may be contradictory
    // E.g., (x < 0) AND NOT(x < 10) is always false but we don't prune it (optimization not needed)
    // Veto solutions should be present in the result
    assert!(!solutions.is_empty(), "expected at least one veto solution");

    // Each solution should have domain constraints
    for solution in &solutions {
        assert!(!solution.is_empty(), "expected domain constraints");
    }
}
