use lemma::parsing::ast::DateTimeValue;
use lemma::{Bound, DataPath, Domain, Engine, LiteralValue, Target, ValueKind};
use rust_decimal::Decimal;

fn ratio_percent_value(v: &LiteralValue) -> Option<Decimal> {
    match &v.value {
        ValueKind::Ratio(n, u) if u.as_deref() == Some("percent") => {
            Some(lemma::commit_rational_to_decimal(n).unwrap())
        }
        _ => None,
    }
}

#[test]
fn veto_query_with_value_branches_filters_correctly() {
    let code = r#"
        spec pricing
        data discount: ratio

        rule final_price: 100
            unless discount >= 10%  then 90
            unless discount >= 25%  then 75
            unless discount >= 50%  then veto "discount too high"
            unless discount < 0%    then veto "invalid discount"
    "#;

    let mut engine = Engine::new();
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();

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

    let discount_path = DataPath::local("discount".to_string());
    assert_eq!(response.len(), 2, "expected only veto solutions");

    let fifty = Decimal::new(5, 1);
    let zero = Decimal::ZERO;

    let mut found_high_discount = false;
    let mut found_negative_discount = false;

    for domains in &response.domains {
        let discount_domain = domains
            .get(&discount_path)
            .expect("solution should contain discount domain");

        match discount_domain {
            Domain::Range { min, max } => {
                if matches!(min, Bound::Inclusive(v) if ratio_percent_value(v.as_ref()) == Some(fifty))
                    && matches!(max, Bound::Unbounded)
                {
                    found_high_discount = true;
                } else if matches!(min, Bound::Unbounded)
                    && matches!(max, Bound::Exclusive(v) if ratio_percent_value(v.as_ref()) == Some(zero))
                {
                    found_negative_discount = true;
                }
            }
            Domain::Complement(inner) => {
                if let Domain::Range { min, max } = inner.as_ref() {
                    if matches!(min, Bound::Inclusive(v) if ratio_percent_value(v.as_ref()) == Some(zero))
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
