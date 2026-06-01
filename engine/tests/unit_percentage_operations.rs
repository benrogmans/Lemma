use lemma::DateTimeValue;
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn decimal_lit(d: &str) -> Decimal {
    Decimal::from_str(d).unwrap()
}

#[test]
fn test_unit_subtract_percentage() -> Result<(), lemma::Errors> {
    let mut engine = Engine::new();

    // This is shown in the README as a feature - it must work
    engine.load(
        r#"
        spec pricing

        data quantity: 10
        data is_vip: false

        rule discount: 0%
            unless quantity >= 10 then 10%
            unless quantity >= 50 then 20%
            unless is_vip then 25%

        rule price: 200 - discount
        "#,
        lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
            "pricing.lemma",
        ))),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "pricing", Some(&now), HashMap::new(), true)
        .expect("run should succeed after load");

    // Check discount rule result
    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount")
        .expect("discount rule not found");

    let lit = discount_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        assert_eq!(
            lit.value,
            lemma::LiteralValue::ratio_from_decimal(
                decimal_lit("0.1"),
                Some("percent".to_string())
            )
            .value
        );
    }

    // Check price rule result
    let price_result = response
        .results
        .values()
        .find(|r| r.rule.name == "price")
        .expect("price rule not found");

    let lit = price_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Number(n) = &lit.value {
            assert_eq!(
                lemma::ValueKind::Number(*n).as_decimal_magnitude().unwrap(),
                decimal_lit("180")
            );
        } else {
            panic!("Expected number for price, got {:?}", price_result.display);
        }
    }

    Ok(())
}

#[test]
fn test_unit_add_percentage() -> Result<(), lemma::Errors> {
    let mut engine = Engine::new();

    engine.load(
        r#"
        spec tax_calculation

        data base_price: 100
        data tax_rate: 8.5%

        rule price_with_tax: base_price + tax_rate
        "#,
        lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("tax.lemma"))),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "tax_calculation", Some(&now), HashMap::new(), true)
        .expect("run should succeed after load");

    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "price_with_tax")
        .expect("price_with_tax rule not found");

    let lit = result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Number(_n) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::number_from_decimal(decimal_lit("108.5")).value
            );
        } else {
            panic!(
                "Expected number for price_with_tax, got {:?}",
                result.display
            );
        }
    }

    Ok(())
}

#[test]
fn test_various_unit_percentage_operations() -> Result<(), lemma::Errors> {
    let mut engine = Engine::new();

    engine.load(
        r#"
        spec unit_percentage_ops

        data price: 50
        data increase: 20%
        data decrease: 15%

        rule increased: price + increase
        rule decreased: price - decrease
        rule scaled: price * increase
        "#,
        lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("ops.lemma"))),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "unit_percentage_ops",
            Some(&now),
            HashMap::new(),
            true,
        )
        .expect("run should succeed after load");

    // Check increased (50 + 20% = 60)
    let increased_result = response
        .results
        .values()
        .find(|r| r.rule.name == "increased")
        .expect("increased rule not found");

    let lit = increased_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Number(_n) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::number_from_decimal(decimal_lit("60")).value
            );
        } else {
            panic!("Expected number for increased");
        }
    }

    // Check decreased (50 - 15% = 42.50)
    let decreased_result = response
        .results
        .values()
        .find(|r| r.rule.name == "decreased")
        .expect("decreased rule not found");

    let lit = decreased_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Number(_n) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::number_from_decimal(decimal_lit("42.5")).value
            );
        } else {
            panic!(
                "Expected number for decreased, got {:?}",
                decreased_result.display
            );
        }
    }

    // Check scaled (50 * 20% = 10)
    let scaled_result = response
        .results
        .values()
        .find(|r| r.rule.name == "scaled")
        .expect("scaled rule not found");

    let lit = scaled_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Number(_n) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::number_from_decimal(decimal_lit("10")).value
            );
        } else {
            panic!(
                "Expected number for scaled, got {:?}",
                scaled_result.display
            );
        }
    }

    Ok(())
}

#[test]
fn test_complex_discount_scenario() -> Result<(), lemma::Errors> {
    let mut engine = Engine::new();

    engine.load(
        r#"
        spec complex_pricing

        data base_price: 1000
        data bulk_discount: 15%
        data loyalty_discount: 5%

        rule after_bulk: base_price - bulk_discount
        rule final_price: after_bulk - loyalty_discount
        "#,
        lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
            "complex.lemma",
        ))),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "complex_pricing", Some(&now), HashMap::new(), true)
        .expect("run should succeed after load");

    // Check after_bulk (1000 - 15% = 850)
    let after_bulk_result = response
        .results
        .values()
        .find(|r| r.rule.name == "after_bulk")
        .expect("after_bulk rule not found");

    let lit = after_bulk_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Number(_n) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::number_from_decimal(decimal_lit("850")).value
            );
        } else {
            panic!(
                "Expected number for after_bulk, got {:?}",
                after_bulk_result.display
            );
        }
    }

    // Check final_price (850 - 5% = 807.50)
    let final_price_result = response
        .results
        .values()
        .find(|r| r.rule.name == "final_price")
        .expect("final_price rule not found");

    let lit = final_price_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Number(_n) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::number_from_decimal(decimal_lit("807.5")).value
            );
        } else {
            panic!(
                "Expected number for final_price, got {:?}",
                final_price_result.display
            );
        }
    }

    Ok(())
}

#[test]
fn test_percentage_arithmetic() -> Result<(), lemma::Errors> {
    let mut engine = Engine::new();

    engine.load(
        r#"
        spec percentage_ops

        data discount_a: 5%
        data discount_b: 10%
        data tax_rate: 15%
        data compound_rate: 20%

        rule combined_discount: discount_a + discount_b
        rule net_rate: tax_rate - discount_a
        rule compound: compound_rate * compound_rate
        rule ratio: compound_rate / discount_a
        "#,
        lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
            "percentage.lemma",
        ))),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "percentage_ops", Some(&now), HashMap::new(), true)
        .expect("run should succeed after load");

    // Check combined_discount (5% + 10% = 15%)
    let combined_result = response
        .results
        .values()
        .find(|r| r.rule.name == "combined_discount")
        .expect("combined_discount rule not found");

    let lit = combined_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Ratio(_r, _) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::ratio_from_decimal(
                    decimal_lit("0.15"),
                    Some("percent".to_string())
                )
                .value
            );
        } else {
            panic!(
                "Expected percentage for combined_discount, got {:?}",
                combined_result.display
            );
        }
    }

    // Check net_rate (15% - 5% = 10%)
    let net_rate_result = response
        .results
        .values()
        .find(|r| r.rule.name == "net_rate")
        .expect("net_rate rule not found");

    let lit = net_rate_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Ratio(_r, _) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::ratio_from_decimal(
                    decimal_lit("0.10"),
                    Some("percent".to_string())
                )
                .value
            );
        } else {
            panic!(
                "Expected percentage for net_rate, got {:?}",
                net_rate_result.display
            );
        }
    }

    // Check compound (20% * 20% = 4%)
    let compound_result = response
        .results
        .values()
        .find(|r| r.rule.name == "compound")
        .expect("compound rule not found");

    let lit = compound_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        if let lemma::ValueKind::Ratio(_r, _) = &lit.value {
            assert_eq!(
                lit.value,
                lemma::LiteralValue::ratio_from_decimal(
                    decimal_lit("0.04"),
                    Some("percent".to_string())
                )
                .value
            );
        } else {
            panic!(
                "Expected percentage for compound, got {:?}",
                compound_result.display
            );
        }
    }

    // Check ratio (20% / 5% = 4)
    let ratio_result = response
        .results
        .values()
        .find(|r| r.rule.name == "ratio")
        .expect("ratio rule not found");

    let lit = ratio_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    {
        // 20% / 5% = 4 (ratio / ratio = ratio)
        match &lit.value {
            lemma::ValueKind::Ratio(rational_val, unit) => {
                assert_eq!(
                    lemma::ValueKind::Number(*rational_val)
                        .as_decimal_magnitude()
                        .unwrap(),
                    decimal_lit("4")
                );
                assert_eq!(unit.as_deref(), Some("percent"));
            }
            _ => panic!(
                "Expected ratio for 20% / 5% (ratio / ratio = ratio), got {:?}",
                lit.value
            ),
        }
    }

    Ok(())
}
