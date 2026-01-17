use lemma::{Engine, LemmaResult};
use std::collections::HashMap;

#[test]
fn test_unit_subtract_percentage() -> LemmaResult<()> {
    let mut engine = Engine::new();

    // This is shown in the README as a feature - it must work
    engine.add_lemma_code(
        r#"
        doc pricing

        fact quantity = 10
        fact is_vip = false

        rule discount = 0%
            unless quantity >= 10 then 10%
            unless quantity >= 50 then 20%
            unless is_vip then 25%

        rule price = 200 - discount?
        "#,
        "pricing.lemma",
    )?;

    let response = engine.evaluate("pricing", vec![], HashMap::new())?;

    // Check discount rule result
    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount")
        .expect("discount rule not found");

    match &discount_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::Value::Ratio(_r, _) = &lit.value {
                // 10% is stored as 0.10 (base ratio) with "percent" unit, displays as "10%"
                assert_eq!(lit.to_string(), "10%", "discount should be 10%");
            } else {
                panic!("Expected ratio for discount");
            }
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
            if let lemma::Value::Number(n) = &lit.value {
                assert_eq!(
                    n.normalize().to_string(),
                    "180",
                    "price should be 180 (200 - 10%)"
                );
            } else {
                panic!("Expected number for price, got {:?}", price_result.result);
            }
        }
        _ => panic!("Expected number for price, got {:?}", price_result.result),
    }

    Ok(())
}

#[test]
fn test_unit_add_percentage() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc tax_calculation

        fact base_price = 100
        fact tax_rate = 8.5%

        rule price_with_tax = base_price + tax_rate
        "#,
        "tax.lemma",
    )?;

    let response = engine.evaluate("tax_calculation", vec![], HashMap::new())?;

    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "price_with_tax")
        .expect("price_with_tax rule not found");

    match &result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::Value::Number(_n) = &lit.value {
                // 100 + 8.5% = 108.5 - use Display implementation which normalizes
                assert_eq!(lit.to_string(), "108.5", "price_with_tax should be 108.5");
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
fn test_various_unit_percentage_operations() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc unit_percentage_ops

        fact price = 50
        fact increase = 20%
        fact decrease = 15%

        rule increased = price + increase
        rule decreased = price - decrease
        rule scaled = price * increase
        "#,
        "ops.lemma",
    )?;

    let response = engine.evaluate("unit_percentage_ops", vec![], HashMap::new())?;

    // Check increased (50 + 20% = 60)
    let increased_result = response
        .results
        .values()
        .find(|r| r.rule.name == "increased")
        .expect("increased rule not found");

    match &increased_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::Value::Number(_n) = &lit.value {
                assert_eq!(lit.to_string(), "60", "50 + 20% should be 60");
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
            if let lemma::Value::Number(_n) = &lit.value {
                // When decimals is None, Display normalizes, so 42.50 becomes 42.5
                assert_eq!(
                    lit.to_string(),
                    "42.5",
                    "50 - 15% should be 42.5 (normalized)"
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
            if let lemma::Value::Number(_n) = &lit.value {
                assert_eq!(lit.to_string(), "10", "50 * 20% should be 10");
            } else {
                panic!("Expected number for scaled, got {:?}", scaled_result.result);
            }
        }
        _ => panic!("Expected number for scaled, got {:?}", scaled_result.result),
    }

    Ok(())
}

#[test]
fn test_complex_discount_scenario() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc complex_pricing

        fact base_price = 1000
        fact bulk_discount = 15%
        fact loyalty_discount = 5%

        rule after_bulk = base_price - bulk_discount
        rule final_price = after_bulk? - loyalty_discount
        "#,
        "complex.lemma",
    )?;

    let response = engine.evaluate("complex_pricing", vec![], HashMap::new())?;

    // Check after_bulk (1000 - 15% = 850)
    let after_bulk_result = response
        .results
        .values()
        .find(|r| r.rule.name == "after_bulk")
        .expect("after_bulk rule not found");

    match &after_bulk_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::Value::Number(_n) = &lit.value {
                assert_eq!(lit.to_string(), "850", "1000 - 15% should be 850");
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
            if let lemma::Value::Number(_n) = &lit.value {
                // When decimals is None, Display normalizes, so 807.50 becomes 807.5
                assert_eq!(
                    lit.to_string(),
                    "807.5",
                    "850 - 5% should be 807.5 (normalized)"
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
fn test_percentage_arithmetic() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc percentage_ops

        fact discount_a = 5%
        fact discount_b = 10%
        fact tax_rate = 15%
        fact compound_rate = 20%

        rule combined_discount = discount_a + discount_b
        rule net_rate = tax_rate - discount_a
        rule compound = compound_rate * compound_rate
        rule ratio = compound_rate / discount_a
        "#,
        "percentage.lemma",
    )?;

    let response = engine.evaluate("percentage_ops", vec![], HashMap::new())?;

    // Check combined_discount (5% + 10% = 15%)
    let combined_result = response
        .results
        .values()
        .find(|r| r.rule.name == "combined_discount")
        .expect("combined_discount rule not found");

    match &combined_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::Value::Ratio(_r, _) = &lit.value {
                // 5% (0.05) + 10% (0.10) = 15% (0.15) as ratio with "percent" unit, displays as "15%"
                assert_eq!(lit.to_string(), "15%", "5% + 10% should be 15%");
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
            if let lemma::Value::Ratio(_r, _) = &lit.value {
                // 15% (0.15) - 5% (0.05) = 10% (0.10) as ratio with "percent" unit, displays as "10%"
                assert_eq!(lit.to_string(), "10%", "15% - 5% should be 10%");
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
            if let lemma::Value::Ratio(_r, _) = &lit.value {
                // 20% (0.20) * 20% (0.20) = 4% (0.04) as ratio with "percent" unit, displays as "4%"
                assert_eq!(lit.to_string(), "4%", "20% * 20% should be 4%");
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
                lemma::Value::Ratio(_r, _) => {
                    // Ratio / ratio = ratio. Ratio(4, Some("percent")) displays as "400%" (4 * 100)
                    assert_eq!(
                        lit.to_string(),
                        "400%",
                        "20% / 5% should be 400% (ratio with percent unit: 4 * 100)"
                    );
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
fn test_averaging_percentages() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc avg_percentages

        fact rate_a = 10%
        fact rate_b = 20%
        fact rate_c = 15%

        rule sum = rate_a + rate_b + rate_c
        rule average = sum? / 3
        "#,
        "avg.lemma",
    )?;

    let response = engine.evaluate("avg_percentages", vec![], HashMap::new())?;

    // Check sum (10% + 20% + 15% = 45%)
    let sum_result = response
        .results
        .values()
        .find(|r| r.rule.name == "sum")
        .expect("sum rule not found");

    match &sum_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::Value::Ratio(_r, _) = &lit.value {
                // 10% (0.10) + 20% (0.20) + 15% (0.15) = 45% (0.45) as ratio with "percent" unit, displays as "45%"
                assert_eq!(lit.to_string(), "45%", "10% + 20% + 15% should be 45%");
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
                lemma::Value::Ratio(_r, _) => {
                    // 45% / 3 = 15% as ratio with "percent" unit (if preserved), displays as "15%"
                    assert_eq!(
                        lit.to_string(),
                        "15%",
                        "45% / 3 should be 15% (ratio with percent unit)"
                    );
                }
                lemma::Value::Number(_n) => {
                    // If arithmetic produced a number (ratio / number might become number)
                    assert_eq!(lit.to_string(), "0.15", "45% / 3 should be 0.15 (number)");
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
