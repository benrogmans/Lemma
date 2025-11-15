/// Comprehensive tests for fact override type validation
///
/// These tests ensure that the engine correctly validates that fact overrides
/// match the expected types declared in the document, preventing type confusion bugs.
use lemma::Engine;

#[test]
fn test_money_type_validation_rejects_number() {
    let code = r#"
doc test
fact price = [money]
rule total = price * 1.1
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // This should fail - money fact cannot be overridden with a number
    let facts = lemma::parse_facts(&["price=100"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));

    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(error.contains("Type mismatch for fact 'price'"));
    assert!(error.contains("expected money"));
    assert!(error.contains("got number"));
}

#[test]
fn test_money_type_validation_accepts_money() {
    let code = r#"
doc test
fact price = [money]
rule total = price * 1.1
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // This should succeed - money fact with money override
    let facts = lemma::parse_facts(&["price=100 USD"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].rule.name, "total");
}

#[test]
fn test_number_type_validation_rejects_text() {
    let code = r#"
doc test
fact age = [number]
rule doubled = age * 2
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // This should fail - number fact cannot be overridden with text
    let facts = lemma::parse_facts(&["age=\"twenty\""]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));

    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(error.contains("Type mismatch for fact 'age'"));
    assert!(error.contains("expected number"));
    assert!(error.contains("got text"));
}

#[test]
fn test_multiple_type_validations() {
    let code = r#"
doc test
fact price = [money]
fact quantity = [number]
fact active = [boolean]
rule total = price * quantity
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // Test money type mismatch
    let facts = lemma::parse_facts(&["price=100", "quantity=5", "active=true"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Type mismatch for fact 'price'"));

    // Test number type mismatch
    let facts = lemma::parse_facts(&["price=100 USD", "quantity=\"five\"", "active=true"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Type mismatch for fact 'quantity'"));

    // Test boolean type mismatch
    let facts = lemma::parse_facts(&["price=100 USD", "quantity=5", "active=\"yes\""]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Type mismatch for fact 'active'"));

    // Test all correct types
    let facts = lemma::parse_facts(&["price=100 USD", "quantity=5", "active=true"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));
    assert!(result.is_ok());
}

#[test]
fn test_literal_fact_type_validation() {
    let code = r#"
doc test
fact base_price = 50 USD
rule total = base_price * 1.2
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // Should reject number override for money literal
    let facts = lemma::parse_facts(&["base_price=60"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Type mismatch for fact 'base_price'"));

    // Should accept money override for money literal
    let facts = lemma::parse_facts(&["base_price=60 USD"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));
    assert!(result.is_ok());
}

#[test]
fn test_unknown_fact_override_allowed() {
    let code = r#"
doc test
fact price = [money]
rule total = price * 1.1
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    // Should allow override for fact not declared in document
    let facts = lemma::parse_facts(&["price=100 USD", "unknown_fact=42"]).unwrap();
    let result = engine.evaluate("test", None, Some(facts));
    assert!(result.is_ok());
}
