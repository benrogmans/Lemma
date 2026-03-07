use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_unit_subtract_percentage() -> Result<(), Vec<lemma::Error>> {
    let mut engine = Engine::new();

    // This is shown in the README as a feature - it must work
    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec pricing

        fact quantity: 10
        fact is_vip: false

        rule discount: 0%
            unless quantity >= 10 then 10%
            unless quantity >= 50 then 20%
            unless is_vip then 25%

        rule price: 200 - discount
        "#,
        "pricing.lemma",
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("pricing", None, &now, vec![], HashMap::new())
        .map_err(|e| vec![e])?;

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
fn test_unit_add_percentage() -> Result<(), Vec<lemma::Error>> {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec tax_calculation

        fact base_price: 100
        fact tax_rate: 8.5%

        rule price_with_tax: base_price + tax_rate
        "#,
        "tax.lemma",
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("tax_calculation", None, &now, vec![], HashMap::new())
        .map_err(|e| vec![e])?;

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
fn test_various_unit_percentage_operations() -> Result<(), Vec<lemma::Error>> {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec unit_percentage_ops

        fact price: 50
        fact increase: 20%
        fact decrease: 15%

        rule increased: price + increase
        rule decreased: price - decrease
        rule scaled: price * increase
        "#,
        "ops.lemma",
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("unit_percentage_ops", None, &now, vec![], HashMap::new())
        .map_err(|e| vec![e])?;

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
fn test_complex_discount_scenario() -> Result<(), Vec<lemma::Error>> {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec complex_pricing

        fact base_price: 1000
        fact bulk_discount: 15%
        fact loyalty_discount: 5%

        rule after_bulk: base_price - bulk_discount
        rule final_price: after_bulk - loyalty_discount
        "#,
        "complex.lemma",
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("complex_pricing", None, &now, vec![], HashMap::new())
        .map_err(|e| vec![e])?;

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
fn test_percentage_arithmetic() -> Result<(), Vec<lemma::Error>> {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec percentage_ops

        fact discount_a: 5%
        fact discount_b: 10%
        fact tax_rate: 15%
        fact compound_rate: 20%

        rule combined_discount: discount_a + discount_b
        rule net_rate: tax_rate - discount_a
        rule compound: compound_rate * compound_rate
        rule ratio: compound_rate / discount_a
        "#,
        "percentage.lemma",
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("percentage_ops", None, &now, vec![], HashMap::new())
        .map_err(|e| vec![e])?;

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

#[test]
fn test_averaging_percentages() -> Result<(), Vec<lemma::Error>> {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec avg_percentages

        fact rate_a: 10%
        fact rate_b: 20%
        fact rate_c: 15%

        rule sum: rate_a + rate_b + rate_c
        rule average: sum / 3
        "#,
        "avg.lemma",
    )?;

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("avg_percentages", None, &now, vec![], HashMap::new())
        .map_err(|e| vec![e])?;

    // Check sum (10% + 20% + 15% = 45%)
    let sum_result = response
        .results
        .values()
        .find(|r| r.rule.name == "sum")
        .expect("sum rule not found");

    match &sum_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Ratio(_r, _) = &lit.value {
                assert_eq!(
                    lit.value,
                    lemma::ValueKind::Ratio(
                        Decimal::from_str("0.45").unwrap(),
                        Some("percent".to_string())
                    )
                );
            } else {
                panic!("Expected percentage for sum, got {:?}", sum_result.result);
            }
        }
        _ => panic!("Expected percentage for sum, got {:?}", sum_result.result),
    }

    // Check average (45% / 3 = 15%)
    let avg_result = response
        .results
        .values()
        .find(|r| r.rule.name == "average")
        .expect("average rule not found");

    match &avg_result.result {
        lemma::OperationResult::Value(lit) => {
            // 45% / 3 = 15% (ratio / number = ratio or number depending on implementation)
            match &lit.value {
                lemma::ValueKind::Ratio(_r, _) => {
                    assert_eq!(
                        lit.value,
                        lemma::ValueKind::Ratio(
                            Decimal::from_str("0.15").unwrap(),
                            Some("percent".to_string())
                        )
                    );
                }
                lemma::ValueKind::Number(_n) => {
                    assert_eq!(
                        lit.value,
                        lemma::ValueKind::Number(Decimal::from_str("0.15").unwrap())
                    );
                }
                _ => panic!("Expected ratio or number for average, got {:?}", lit.value),
            }
        }
        _ => panic!(
            "Expected percentage for average, got {:?}",
            avg_result.result
        ),
    }

    Ok(())
}
