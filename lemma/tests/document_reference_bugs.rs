use lemma::Engine;

/// Bug: Cross-document rule references through nested document references fail
///
/// When document A references document B, and we try to reference rules from B,
/// the system should resolve them but instead reports missing facts.
#[test]
fn test_nested_document_rule_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc pricing
fact base_price = 100
fact tax_rate = 21%
rule final_price = base_price * (1 + tax_rate)
"#;

    let line_item_doc = r#"
doc line_item
fact pricing = doc pricing
fact quantity = 10
rule line_total = pricing.final_price? * quantity
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(line_item_doc, "test.lemma").unwrap();

    let response = engine.evaluate("line_item", None, None).unwrap();
    let line_total = response
        .results
        .iter()
        .find(|r| r.rule_name == "line_total")
        .unwrap();

    // Should be: (100 * 1.21) * 10 = 1210
    assert_eq!(line_total.result.as_ref().unwrap().to_string(), "1210.00");
}

/// Bug: Multi-level document references fail
///
/// When document A references document B which references document C,
/// rule references through the chain fail.
#[test]
fn test_multi_level_document_rule_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact value = 100
rule doubled = value * 2
"#;

    let middle_doc = r#"
doc middle
fact base_ref = doc base
rule middle_calc = base_ref.doubled? + 50
"#;

    let top_doc = r#"
doc top
fact middle_ref = doc middle
rule top_calc = middle_ref.middle_calc?
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(middle_doc, "test.lemma").unwrap();
    engine.add_lemma_code(top_doc, "test.lemma").unwrap();

    let response = engine.evaluate("top", None, None).unwrap();

    println!("Available rules:");
    for result in &response.results {
        println!("  - {}: {:?}", result.rule_name, result.result);
    }

    let top_calc = response
        .results
        .iter()
        .find(|r| r.rule_name == "top_calc")
        .expect("top_calc rule not found in results");

    // Should be: ((100 * 2) + 50) = 250
    assert_eq!(top_calc.result.as_ref().unwrap().to_string(), "250");
}

/// Bug: Overriding document reference facts in nested structures fails
///
/// When we override a nested document reference (e.g., line.pricing = doc wholesale),
/// the override doesn't properly propagate through rule evaluations.
#[test]
fn test_nested_document_override_with_rule_reference() {
    let mut engine = Engine::new();

    let pricing_doc = r#"
doc pricing
fact base_price = 100
rule final_price = base_price * 1.1
"#;

    let wholesale_doc = r#"
doc wholesale_pricing
fact base_price = 75
rule final_price = base_price * 1.1
"#;

    let line_item_doc = r#"
doc line_item
fact pricing = doc pricing
fact quantity = 10
rule line_total = pricing.final_price? * quantity
"#;

    let order_doc = r#"
doc order
fact line = doc line_item
fact line.pricing = doc wholesale_pricing
fact line.quantity = 100
rule order_total = line.line_total?
"#;

    engine.add_lemma_code(pricing_doc, "test.lemma").unwrap();
    engine.add_lemma_code(wholesale_doc, "test.lemma").unwrap();
    engine.add_lemma_code(line_item_doc, "test.lemma").unwrap();
    engine.add_lemma_code(order_doc, "test.lemma").unwrap();

    let response = engine.evaluate("order", None, None).unwrap();

    println!("Available rules:");
    for result in &response.results {
        println!("  - {}: {:?}", result.rule_name, result.result);
    }

    let order_total = response
        .results
        .iter()
        .find(|r| r.rule_name == "order_total")
        .expect("order_total rule not found in results");

    // Should use wholesale pricing: (75 * 1.1) * 100 = 8250
    assert_eq!(order_total.result.as_ref().unwrap().to_string(), "8250");
}

/// Bug: Accessing facts through multi-level document references with nested overrides
///
/// When document A has nested doc refs and we try to access deeply nested facts
/// through multiple levels, the resolution fails.
#[test]
fn test_multi_level_fact_access_through_doc_refs() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact value = 50
"#;

    let middle_doc = r#"
doc middle
fact config = doc base
fact config.value = 100
"#;

    let top_doc = r#"
doc top
fact settings = doc middle
rule final_value = settings.config.value * 2
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(middle_doc, "test.lemma").unwrap();
    engine.add_lemma_code(top_doc, "test.lemma").unwrap();

    let response = engine.evaluate("top", None, None).unwrap();
    let final_value = response
        .results
        .iter()
        .find(|r| r.rule_name == "final_value")
        .unwrap();

    // Should be: 100 * 2 = 200 (using the overridden value from middle)
    assert_eq!(final_value.result.as_ref().unwrap().to_string(), "200");
}
