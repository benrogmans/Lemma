use lemma::*;

#[test]
fn test_mixed_currency_comparison_rejected() {
    let code = r#"
doc pricing
fact price_usd = 100 USD
fact price_eur = 80 EUR

rule is_more = price_usd > price_eur
"#;

    let mut engine = Engine::new();
    // Currency mismatch is now caught at compile-time by the validator
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("Cannot compare different currencies"),
        "Error: {}",
        err_msg
    );
    // Currency units are displayed with Debug format (e.g., "Usd" not "USD")
    assert!(
        err_msg.contains("Usd") || err_msg.contains("USD"),
        "Error: {}",
        err_msg
    );
    assert!(
        err_msg.contains("Eur") || err_msg.contains("EUR"),
        "Error: {}",
        err_msg
    );
}

#[test]
fn test_mixed_currency_arithmetic_rejected() {
    let code = r#"
doc pricing
fact price_usd = 100 USD
fact price_eur = 80 EUR

rule total = price_usd + price_eur
"#;

    let mut engine = Engine::new();
    // Currency mismatch is now caught at compile-time by the validator
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("Cannot perform arithmetic with different currencies"),
        "Error: {}",
        err_msg
    );
    // Currency units are displayed with Debug format (e.g., "Usd" not "USD")
    assert!(
        err_msg.contains("Usd") || err_msg.contains("USD"),
        "Error: {}",
        err_msg
    );
    assert!(
        err_msg.contains("Eur") || err_msg.contains("EUR"),
        "Error: {}",
        err_msg
    );
}

#[test]
fn test_same_currency_comparison_allowed() {
    let code = r#"
doc pricing
fact price1 = 100 USD
fact price2 = 80 USD

rule is_more = price1 > price2
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_ok());
}

#[test]
fn test_same_currency_arithmetic_allowed() {
    let code = r#"
doc pricing
fact price1 = 100 EUR
fact price2 = 20 EUR

rule total = price1 + price2
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_ok());
}
