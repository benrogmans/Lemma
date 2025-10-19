use lemma::{Engine, LemmaResult};

#[test]
fn test_json_to_lemma_syntax_basic_types() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc test
        fact name = "Alice"
        fact age = 30
        fact rate = 15%
        fact active = true
        "#,
        "test.lemma",
    )?;

    let doc = engine.get_document("test").unwrap();
    let all_docs = engine.get_all_documents();

    let json = r#"{
        "name": "Bob",
        "age": 25,
        "rate": 0.21,
        "active": false
    }"#;

    let lemma_strings = lemma::serializers::from_json(json.as_bytes(), doc, all_docs)?;

    assert_eq!(lemma_strings.len(), 4);
    assert!(lemma_strings.contains(&"name=\"Bob\"".to_string()));
    assert!(lemma_strings.contains(&"age=25".to_string()));
    assert!(lemma_strings.contains(&"rate=21%".to_string()));
    assert!(lemma_strings.contains(&"active=false".to_string()));

    Ok(())
}

#[test]
fn test_json_to_lemma_syntax_units() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc test
        fact weight = 50 kilogram
        fact price = 100 USD
        fact distance = 10 kilometer
        "#,
        "test.lemma",
    )?;

    let doc = engine.get_document("test").unwrap();
    let all_docs = engine.get_all_documents();

    let json = r#"{
        "weight": "75 kilogram",
        "price": "200 USD",
        "distance": "5 kilometer"
    }"#;

    let lemma_strings = lemma::serializers::from_json(json.as_bytes(), doc, all_docs)?;

    assert_eq!(lemma_strings.len(), 3);
    assert!(lemma_strings.contains(&"weight=75 kilogram".to_string()));
    assert!(lemma_strings.contains(&"price=200 USD".to_string()));
    assert!(lemma_strings.contains(&"distance=5 kilometer".to_string()));

    Ok(())
}

#[test]
fn test_json_to_lemma_syntax_type_mismatch() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
        doc test
        fact age = 30
        "#,
            "test.lemma",
        )
        .unwrap();

    let doc = engine.get_document("test").unwrap();
    let all_docs = engine.get_all_documents();

    let json = r#"{"age": "not a number"}"#;

    let result = lemma::serializers::from_json(json.as_bytes(), doc, all_docs);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid number string"));
}

#[test]
fn test_json_to_lemma_syntax_end_to_end() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(
        r#"
        doc pricing
        fact base_price = 100 USD
        fact quantity = 1
        fact tax_rate = 10%
        rule subtotal = base_price * quantity
        rule tax = subtotal? * tax_rate
        rule total = subtotal? + tax?
        "#,
        "pricing.lemma",
    )?;

    let doc = engine.get_document("pricing").unwrap();
    let all_docs = engine.get_all_documents();

    let json = r#"{
        "base_price": "50 USD",
        "quantity": 5,
        "tax_rate": 0.21
    }"#;

    let lemma_strings = lemma::serializers::from_json(json.as_bytes(), doc, all_docs)?;
    let facts = lemma::parse_facts(&lemma_strings.iter().map(|s| s.as_str()).collect::<Vec<_>>())?;

    let response = engine.evaluate("pricing", None, Some(facts))?;

    let total_rule = response
        .results
        .iter()
        .find(|r| r.rule_name == "total")
        .unwrap();

    assert_eq!(
        total_rule.result.as_ref().unwrap().to_string(),
        "302.50 USD"
    );

    Ok(())
}
