use lemma::parsing::ast::DateTimeValue;
use lemma::{Bound, DataPath, Domain, Engine, LiteralValue, Target};

#[test]
fn veto_query_with_value_branches_filters_correctly() {
    let code = r#"
        spec pricing
        data discount: percent

        rule final_price: 100
            unless discount >= 10%  then 90
            unless discount >= 25%  then 75
            unless discount >= 50%  then veto "discount too high"
            unless discount < 0%    then veto "invalid discount"
    "#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test"))
        .unwrap();

    // Query: "What discount values trigger any veto?"
    let now = DateTimeValue::now();
    let response = engine
        .invert(
            "pricing",
            Some(&now),
            "final_price",
            Target::any_veto(),
            std::collections::HashMap::new(),
        )
        .expect("should invert successfully");

    // Should have the matching veto solutions
    // Should only have the two veto solutions, not the value solutions
    assert_eq!(response.len(), 2, "expected only veto solutions");

    let discount_path = DataPath::local("discount".to_string());
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
