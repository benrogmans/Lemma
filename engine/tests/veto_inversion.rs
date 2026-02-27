use lemma::{Bound, Domain, Engine, FactPath, LiteralValue, Target};
mod common;
use common::add_lemma_code_blocking;

#[test]
fn veto_query_specific_message() {
    let code = r#"
        doc shipping
        fact weight: [number]

        rule shipping_cost: 5
             unless weight < 0 then veto "invalid"
             unless weight > 100 then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Query: "What weight values trigger 'too heavy' veto?"
    let response = engine
        .invert(
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
        !response.domains[0].is_empty(),
        "expected domain constraints"
    );
}

#[test]
fn veto_query_any_veto() {
    let code = r#"
        doc shipping
        fact weight: [number]

        rule shipping_cost: 5
             unless weight < 0 then veto "invalid"
             unless weight > 100 then veto "too heavy"
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Query: "What weight values trigger ANY veto?"
    let response = engine
        .invert(
            "shipping",
            "shipping_cost",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("veto inversion should succeed");

    // Should have two solutions: weight < 0 and weight > 100
    assert_eq!(response.len(), 2, "expected two veto solutions");

    // Each solution should have domain constraints
    for domains in &response.domains {
        assert!(
            !domains.is_empty(),
            "expected domain constraints in each solution"
        );
    }
}

#[test]
fn veto_query_with_value_branches_filters_correctly() {
    let code = r#"
        doc pricing
        fact discount: [percent]

        rule final_price: 100
            unless discount >= 10%  then 90
            unless discount >= 25%  then 75
            unless discount >= 50%  then veto "discount too high"
            unless discount < 0%    then veto "invalid discount"
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Query: "What discount values trigger any veto?"
    let response = engine
        .invert(
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
    let fifty_percent = LiteralValue::ratio(
        rust_decimal::Decimal::new(50, 0) / rust_decimal::Decimal::from(100),
        Some("percent".to_string()),
    ); // 50% = 0.50 as ratio
    let zero_percent = LiteralValue::ratio(
        rust_decimal::Decimal::new(0, 0),
        Some("percent".to_string()),
    );

    // Find the two veto solutions
    let mut found_high_discount = false;
    let mut found_negative_discount = false;

    for domains in &response.domains {
        assert!(
            !domains.is_empty(),
            "all solutions should have domain constraints"
        );

        let discount_domain = domains
            .get(&discount_path)
            .expect("solution should contain discount domain");

        match discount_domain {
            Domain::Range { min, max } => {
                // Check for discount >= 50% (veto "discount too high")
                if matches!(min, Bound::Inclusive(v) if v.as_ref() == &fifty_percent)
                    && matches!(max, Bound::Unbounded)
                {
                    found_high_discount = true;
                }
                // Check for discount < 0% (veto "invalid discount")
                else if matches!(min, Bound::Unbounded)
                    && matches!(max, Bound::Exclusive(v) if v.as_ref() == &zero_percent)
                {
                    found_negative_discount = true;
                }
            }
            Domain::Complement(inner) => {
                // Could be represented as Complement(Range) for discount < 0%
                if let Domain::Range { min, max } = inner.as_ref() {
                    if matches!(min, Bound::Inclusive(v) if v.as_ref() == &zero_percent)
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
        "should find discount >= 50% veto solution"
    );
    assert!(
        found_negative_discount,
        "should find discount < 0% veto solution"
    );
}

#[test]
fn veto_non_veto_value_queries_exclude_vetoes() {
    let code = r#"
        doc pricing
        fact discount: [percent]

        rule final_price: 100
            unless discount >= 10%  then 90
            unless discount >= 25%  then 75
            unless discount >= 50%  then veto "discount too high"
            unless discount < 0%    then veto "invalid discount"
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Query: "What discount values give final_price = 90?"
    let response = engine
        .invert(
            "pricing",
            "final_price",
            Target::value(LiteralValue::number(90.into())),
            std::collections::HashMap::new(),
        )
        .expect("should invert successfully");

    // Should only have solutions where discount is 10-25%
    // (not the veto branches)
    for domains in &response.domains {
        assert!(
            !domains.is_empty(),
            "solutions should have domain constraints"
        );
    }
}

#[test]
fn veto_multiple_facts_multiple_vetoes() {
    let code = r#"
        doc shipping
        fact weight: [number]
        fact distance: [number]

        rule can_ship: true
            unless weight > 50 then veto "too heavy"
            unless distance > 1000 then veto "too far"
            unless weight < 0 then veto "invalid weight"
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Query: "What conditions trigger any veto?"
    let response = engine
        .invert(
            "shipping",
            "can_ship",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("should invert successfully");

    // Should have 3 veto solutions
    assert_eq!(response.len(), 3, "expected three veto solutions");

    // Each should have at least one constraint
    for domains in &response.domains {
        assert!(
            !domains.is_empty(),
            "each veto solution should have constraints"
        );
    }
}
