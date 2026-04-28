use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

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
        lemma::SourceType::Labeled("pricing.lemma"),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run("pricing", Some(&now), HashMap::new(), false)
        .map_err(|e| lemma::Errors {
            errors: vec![e],
            sources: engine.sources().clone(),
        })?;

    // Check discount rule result
    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount")
        .expect("discount rule not found");

    match &discount_result.result {
        lemma::OperationResult::Value(lit) => {
            assert_eq!(
                lit.value,
                lemma::ValueKind::Ratio(
                    Decimal::from_str("0.1").unwrap(),
                    Some("percent".to_string())
                )
            );
        }
        _ => panic!("Expected percentage for discount"),
    }

    // Check price rule result
    let price_result = response
        .results
        .values()
        .find(|r| r.rule.name == "price")
        .expect("price rule not found");

    match &price_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(n) = &lit.value {
                assert_eq!(*n, Decimal::from_str("180").unwrap());
            } else {
                panic!("Expected number for price, got {:?}", price_result.result);
            }
        }
        _ => panic!("Expected number for price, got {:?}", price_result.result),
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
        lemma::SourceType::Labeled("tax.lemma"),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run("tax_calculation", Some(&now), HashMap::new(), false)
        .map_err(|e| lemma::Errors {
            errors: vec![e],
            sources: engine.sources().clone(),
        })?;

    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "price_with_tax")
        .expect("price_with_tax rule not found");

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(_n) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Number(Decimal::from_str("108.5").unwrap())
                );
            } else {
                panic!(
                    "Expected number for price_with_tax, got {:?}",
                    result.result
                );
            }
        }
        _ => panic!(
            "Expected number for price_with_tax, got {:?}",
            result.result
        ),
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
        lemma::SourceType::Labeled("ops.lemma"),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run("unit_percentage_ops", Some(&now), HashMap::new(), false)
        .map_err(|e| lemma::Errors {
            errors: vec![e],
            sources: engine.sources().clone(),
        })?;

    // Check increased (50 + 20% = 60)
    let increased_result = response
        .results
        .values()
        .find(|r| r.rule.name == "increased")
        .expect("increased rule not found");

    match &increased_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(_n) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Number(Decimal::from_str("60").unwrap())
                );
            } else {
                panic!("Expected number for increased");
            }
        }
        _ => panic!(
            "Expected number for increased, got {:?}",
            increased_result.result
        ),
    }

    // Check decreased (50 - 15% = 42.50)
    let decreased_result = response
        .results
        .values()
        .find(|r| r.rule.name == "decreased")
        .expect("decreased rule not found");

    match &decreased_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(_n) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Number(Decimal::from_str("42.5").unwrap())
                );
            } else {
                panic!(
                    "Expected number for decreased, got {:?}",
                    decreased_result.result
                );
            }
        }
        _ => panic!(
            "Expected number for decreased, got {:?}",
            decreased_result.result
        ),
    }

    // Check scaled (50 * 20% = 10)
    let scaled_result = response
        .results
        .values()
        .find(|r| r.rule.name == "scaled")
        .expect("scaled rule not found");

    match &scaled_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(_n) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Number(Decimal::from_str("10").unwrap())
                );
            } else {
                panic!("Expected number for scaled, got {:?}", scaled_result.result);
            }
        }
        _ => panic!("Expected number for scaled, got {:?}", scaled_result.result),
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
        lemma::SourceType::Labeled("complex.lemma"),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run("complex_pricing", Some(&now), HashMap::new(), false)
        .map_err(|e| lemma::Errors {
            errors: vec![e],
            sources: engine.sources().clone(),
        })?;

    // Check after_bulk (1000 - 15% = 850)
    let after_bulk_result = response
        .results
        .values()
        .find(|r| r.rule.name == "after_bulk")
        .expect("after_bulk rule not found");

    match &after_bulk_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(_n) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Number(Decimal::from_str("850").unwrap())
                );
            } else {
                panic!(
                    "Expected number for after_bulk, got {:?}",
                    after_bulk_result.result
                );
            }
        }
        _ => panic!(
            "Expected number for after_bulk, got {:?}",
            after_bulk_result.result
        ),
    }

    // Check final_price (850 - 5% = 807.50)
    let final_price_result = response
        .results
        .values()
        .find(|r| r.rule.name == "final_price")
        .expect("final_price rule not found");

    match &final_price_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Number(_n) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Number(Decimal::from_str("807.5").unwrap())
                );
            } else {
                panic!(
                    "Expected number for final_price, got {:?}",
                    final_price_result.result
                );
            }
        }
        _ => panic!(
            "Expected number for final_price, got {:?}",
            final_price_result.result
        ),
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
        lemma::SourceType::Labeled("percentage.lemma"),
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .run("percentage_ops", Some(&now), HashMap::new(), false)
        .map_err(|e| lemma::Errors {
            errors: vec![e],
            sources: engine.sources().clone(),
        })?;

    // Check combined_discount (5% + 10% = 15%)
    let combined_result = response
        .results
        .values()
        .find(|r| r.rule.name == "combined_discount")
        .expect("combined_discount rule not found");

    match &combined_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Ratio(_r, _) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Ratio(
                        Decimal::from_str("0.15").unwrap(),
                        Some("percent".to_string())
                    )
                );
            } else {
                panic!(
                    "Expected percentage for combined_discount, got {:?}",
                    combined_result.result
                );
            }
        }
        _ => panic!(
            "Expected percentage for combined_discount, got {:?}",
            combined_result.result
        ),
    }

    // Check net_rate (15% - 5% = 10%)
    let net_rate_result = response
        .results
        .values()
        .find(|r| r.rule.name == "net_rate")
        .expect("net_rate rule not found");

    match &net_rate_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Ratio(_r, _) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Ratio(
                        Decimal::from_str("0.10").unwrap(),
                        Some("percent".to_string())
                    )
                );
            } else {
                panic!(
                    "Expected percentage for net_rate, got {:?}",
                    net_rate_result.result
                );
            }
        }
        _ => panic!(
            "Expected percentage for net_rate, got {:?}",
            net_rate_result.result
        ),
    }

    // Check compound (20% * 20% = 4%)
    let compound_result = response
        .results
        .values()
        .find(|r| r.rule.name == "compound")
        .expect("compound rule not found");

    match &compound_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Ratio(_r, _) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Ratio(
                        Decimal::from_str("0.04").unwrap(),
                        Some("percent".to_string())
                    )
                );
            } else {
                panic!(
                    "Expected percentage for compound, got {:?}",
                    compound_result.result
                );
            }
        }
        _ => panic!(
            "Expected percentage for compound, got {:?}",
            compound_result.result
        ),
    }

    // Check ratio (20% / 5% = 4)
    let ratio_result = response
        .results
        .values()
        .find(|r| r.rule.name == "ratio")
        .expect("ratio rule not found");

    match &ratio_result.result {
        lemma::OperationResult::Value(lit) => {
            // 20% / 5% = 4 (ratio / ratio = ratio)
            match &lit.value {
                lemma::ValueKind::Ratio(r, unit) => {
                    assert_eq!(*r, Decimal::from_str("4").unwrap());
                    assert_eq!(unit.as_deref(), Some("percent"));
                }
                _ => panic!(
                    "Expected ratio for 20% / 5% (ratio / ratio = ratio), got {:?}",
                    lit.value
                ),
            }
        }
        _ => panic!("Expected number for ratio, got {:?}", ratio_result.result),
    }

    Ok(())
}
