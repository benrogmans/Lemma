use lemma::{Engine, LemmaResult, LiteralValue};

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

        rule price = 200 eur - discount?
        "#,
        "pricing.lemma",
    )?;

    let response = engine.evaluate("pricing", None, None)?;

    // Check discount rule result
    let discount_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "discount")
        .expect("discount rule not found");

    match &discount_result.result {
        Some(LiteralValue::Percentage(p)) => {
            assert_eq!(p.to_string(), "10", "discount should be 10%");
        }
        _ => panic!("Expected percentage for discount"),
    }

    // Check price rule result
    let price_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "price")
        .expect("price rule not found");

    match &price_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(
                unit.to_string(),
                "180 EUR",
                "price should be 180 eur (200 - 10%)"
            );
        }
        _ => panic!("Expected unit for price, got {:?}", price_result.result),
    }

    Ok(())
}

#[test]
fn test_unit_add_percentage() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc tax_calculation

        fact base_price = 100 usd
        fact tax_rate = 8.5%

        rule price_with_tax = base_price + tax_rate
        "#,
        "tax.lemma",
    )?;

    let response = engine.evaluate("tax_calculation", None, None)?;

    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "price_with_tax")
        .expect("price_with_tax rule not found");

    match &result.result {
        Some(LiteralValue::Unit(unit)) => {
            // 100 usd + 8.5% = 108.50 usd
            assert_eq!(
                unit.to_string(),
                "108.5 USD",
                "price_with_tax should be 108.5 USD"
            );
        }
        _ => panic!("Expected unit for price_with_tax, got {:?}", result.result),
    }

    Ok(())
}

#[test]
fn test_various_unit_percentage_operations() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc unit_percentage_ops

        fact price = 50 gbp
        fact increase = 20%
        fact decrease = 15%

        rule increased = price + increase
        rule decreased = price - decrease
        rule scaled = price * increase
        "#,
        "ops.lemma",
    )?;

    let response = engine.evaluate("unit_percentage_ops", None, None)?;

    // Check increased (50 gbp + 20% = 60 gbp)
    let increased_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "increased")
        .expect("increased rule not found");

    match &increased_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(unit.to_string(), "60 GBP", "50 gbp + 20% should be 60 gbp");
        }
        _ => panic!(
            "Expected unit for increased, got {:?}",
            increased_result.result
        ),
    }

    // Check decreased (50 gbp - 15% = 42.50 gbp)
    let decreased_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "decreased")
        .expect("decreased rule not found");

    match &decreased_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(
                unit.to_string(),
                "42.50 GBP",
                "50 gbp - 15% should be 42.50 gbp"
            );
        }
        _ => panic!(
            "Expected unit for decreased, got {:?}",
            decreased_result.result
        ),
    }

    // Check scaled (50 gbp * 20% = 10 gbp)
    let scaled_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "scaled")
        .expect("scaled rule not found");

    match &scaled_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(unit.to_string(), "10 GBP", "50 gbp * 20% should be 10 gbp");
        }
        _ => panic!("Expected unit for scaled, got {:?}", scaled_result.result),
    }

    Ok(())
}

#[test]
fn test_complex_discount_scenario() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc complex_pricing

        fact base_price = 1000 eur
        fact bulk_discount = 15%
        fact loyalty_discount = 5%

        rule after_bulk = base_price - bulk_discount
        rule final_price = after_bulk? - loyalty_discount
        "#,
        "complex.lemma",
    )?;

    let response = engine.evaluate("complex_pricing", None, None)?;

    // Check after_bulk (1000 eur - 15% = 850 eur)
    let after_bulk_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "after_bulk")
        .expect("after_bulk rule not found");

    match &after_bulk_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(
                unit.to_string(),
                "850 EUR",
                "1000 eur - 15% should be 850 eur"
            );
        }
        _ => panic!(
            "Expected unit for after_bulk, got {:?}",
            after_bulk_result.result
        ),
    }

    // Check final_price (850 eur - 5% = 807.50 eur)
    let final_price_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "final_price")
        .expect("final_price rule not found");

    match &final_price_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(
                unit.to_string(),
                "807.50 EUR",
                "850 eur - 5% should be 807.50 eur"
            );
        }
        _ => panic!(
            "Expected unit for final_price, got {:?}",
            final_price_result.result
        ),
    }

    Ok(())
}

#[test]
fn test_unit_percentage_with_different_currencies() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc multi_currency

        fact usd_price = 100 usd
        fact eur_price = 85 eur
        fact jpy_price = 10000 jpy
        fact discount = 12%

        rule usd_discounted = usd_price - discount
        rule eur_discounted = eur_price - discount
        rule jpy_discounted = jpy_price - discount
        "#,
        "multi.lemma",
    )?;

    let response = engine.evaluate("multi_currency", None, None)?;

    // Check USD (100 usd - 12% = 88 usd)
    let usd_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "usd_discounted")
        .expect("usd_discounted rule not found");

    match &usd_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(unit.to_string(), "88 USD", "100 usd - 12% should be 88 usd");
        }
        _ => panic!("Expected unit for usd_discounted"),
    }

    // Check EUR (85 eur - 12% = 74.8 eur)
    let eur_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "eur_discounted")
        .expect("eur_discounted rule not found");

    match &eur_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(
                unit.to_string(),
                "74.80 EUR",
                "85 eur - 12% should be 74.80 eur"
            );
        }
        _ => panic!("Expected unit for eur_discounted"),
    }

    // Check JPY (10000 jpy - 12% = 8800 jpy)
    let jpy_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "jpy_discounted")
        .expect("jpy_discounted rule not found");

    match &jpy_result.result {
        Some(LiteralValue::Unit(unit)) => {
            assert_eq!(
                unit.to_string(),
                "8800 JPY",
                "10000 jpy - 12% should be 8800 jpy"
            );
        }
        _ => panic!("Expected unit for jpy_discounted"),
    }

    Ok(())
}
