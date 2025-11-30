use lemma::{Bound, Domain, Engine, FactPath, LiteralValue, Target};

#[test]
fn veto_query_specific_message() {
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule shipping_cost = 5
             unless weight < 0 kilograms then veto "invalid"
             unless weight > 100 kilograms then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: "What weight values trigger 'too heavy' veto?"
    let response = engine
        .invert_strict(
            "shipping",
            "shipping_cost",
            Target::veto(Some("too heavy".to_string())),
            std::collections::HashMap::new(),
        )
        .expect("veto inversion should succeed");

    // Should have exactly one solution: weight > 100
    assert_eq!(response.len(), 1, "expected one veto solution");

    // Should have domain constraint for weight
    assert!(
        !response.solutions[0].is_empty(),
        "expected domain constraints"
    );
}

#[test]
fn veto_query_any_veto() {
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule shipping_cost = 5
             unless weight < 0 kilograms then veto "invalid"
             unless weight > 100 kilograms then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: "What weight values trigger ANY veto?"
    let response = engine
        .invert_strict(
            "shipping",
            "shipping_cost",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("veto inversion should succeed");

    // Should have two solutions: weight < 0 and weight > 100
    assert_eq!(response.len(), 2, "expected two veto solutions");

    // Each solution should have domain constraints
    for solution in response.iter() {
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

        rule final_price = 100
            unless discount >= 10%  then 90
            unless discount >= 25%  then 75
            unless discount >= 50%  then veto "discount too high"
            unless discount < 0%    then veto "invalid discount"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: "What discount values trigger any veto?"
    let response = engine
        .invert_strict(
            "pricing",
            "final_price",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("should invert successfully");

    // Should have the matching veto solutions
    // Should only have the two veto solutions, not the value solutions
    assert_eq!(response.len(), 2, "expected only veto solutions");

    let discount_path = FactPath::local("discount".to_string());
    let fifty_percent = LiteralValue::Percentage(rust_decimal::Decimal::new(50, 0));
    let zero_percent = LiteralValue::Percentage(rust_decimal::Decimal::new(0, 0));

    // Find the two veto solutions
    let mut found_high_discount = false;
    let mut found_negative_discount = false;

    for solution in response.iter() {
        assert!(
            !solution.is_empty(),
            "all solutions should have domain constraints"
        );

        let discount_domain = solution
            .get(&discount_path)
            .expect("solution should contain discount domain");

        match discount_domain {
            Domain::Range { min, max } => {
                // Check for discount >= 50% (veto "discount too high")
                if matches!(min, Bound::Inclusive(v) if v == &fifty_percent)
                    && matches!(max, Bound::Unbounded)
                {
                    found_high_discount = true;
                }
                // Check for discount < 0% (veto "invalid discount")
                else if matches!(min, Bound::Unbounded)
                    && matches!(max, Bound::Exclusive(v) if v == &zero_percent)
                {
                    found_negative_discount = true;
                }
            }
            Domain::Complement(inner) => {
                // Could be represented as Complement(Range) for discount < 0%
                if let Domain::Range { min, max } = inner.as_ref() {
                    if matches!(min, Bound::Inclusive(v) if v == &zero_percent)
                        && matches!(max, Bound::Unbounded)
                    {
                        found_negative_discount = true;
                    }
                }
            }
            _ => {}
        }
    }

    assert!(
        found_high_discount,
        "should have solution for discount >= 50% (veto 'discount too high')"
    );
    assert!(
        found_negative_discount,
        "should have solution for discount < 0% (veto 'invalid discount')"
    );
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
    let result = engine.invert_strict(
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
    let response = engine
        .invert_strict(
            "test",
            "result",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("veto inversion should succeed");

    // Last-wins semantics generates effective conditions that may be contradictory
    // Veto solutions should be present in the result
    assert!(!response.is_empty(), "expected at least one veto solution");

    // Each solution should have domain constraints
    for solution in response.iter() {
        assert!(!solution.is_empty(), "expected domain constraints");
    }
}
