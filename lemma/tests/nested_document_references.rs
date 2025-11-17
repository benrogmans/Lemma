use lemma::Engine;

/// Rule references work through one level of document reference.
#[test]
fn test_single_level_doc_ref_with_rule_reference() {
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
    assert_eq!(line_total.result.as_ref().unwrap().to_string(), "1210");
}

/// Multi-level document rule references should work correctly.
/// When document A references document B which references document C,
/// rule references through the chain should resolve properly.
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

    println!("Response results:");
    for r in &response.results {
        println!("  Rule: {} = {:?}", r.rule_name, r.result);
        println!("       missing: {:?}", r.missing_facts);
        println!("       veto: {:?}", r.veto_message);
    }

    let top_calc = response
        .results
        .iter()
        .find(|r| r.rule_name == "top_calc")
        .expect("top_calc rule not found in results");

    assert_eq!(top_calc.result.as_ref().unwrap().to_string(), "250");
}

/// Overriding nested document references should propagate through rule evaluations.
/// When we override a nested document reference and reference rules through that chain,
/// the overridden document should be used in the evaluation.
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

    let order_total = response
        .results
        .iter()
        .find(|r| r.rule_name == "order_total")
        .expect("order_total rule not found in results");

    assert_eq!(order_total.result.as_ref().unwrap().to_string(), "8250");
}

/// Accessing facts through multi-level document references with nested overrides works correctly.
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

/// Deep nested fact overrides through multiple document layers should work.
/// Overriding facts like order.line.pricing.tax_rate through multiple levels.
#[test]
fn test_deep_nested_fact_override() {
    let mut engine = Engine::new();

    let pricing_doc = r#"
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

    let order_doc = r#"
doc order
fact line = doc line_item
fact line.pricing.tax_rate = 10%
fact line.quantity = 5
rule order_total = line.line_total?
"#;

    engine.add_lemma_code(pricing_doc, "test.lemma").unwrap();
    engine.add_lemma_code(line_item_doc, "test.lemma").unwrap();
    engine.add_lemma_code(order_doc, "test.lemma").unwrap();

    let response = engine.evaluate("order", None, None).unwrap();

    let order_total = response
        .results
        .iter()
        .find(|r| r.rule_name == "order_total")
        .expect("order_total rule not found");

    // base_price=100, tax_rate=10% (overridden), quantity=5
    // (100 * 1.10) * 5 = 550
    assert_eq!(order_total.result.as_ref().unwrap().to_string(), "550");
}

/// Different fact paths to the same base document should produce different results
/// when overrides are applied. This tests that rule evaluation respects the specific
/// path through document references.
#[test]
fn test_different_paths_different_results() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact price = 100
rule total = price * 1.21
"#;

    let wrapper_doc = r#"
doc wrapper
fact base = doc base
"#;

    let comparison_doc = r#"
doc comparison
fact path1 = doc wrapper
fact path2 = doc wrapper
fact path2.base.price = 75
rule total1 = path1.base.total?
rule total2 = path2.base.total?
rule difference = total2? - total1?
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(wrapper_doc, "test.lemma").unwrap();
    engine.add_lemma_code(comparison_doc, "test.lemma").unwrap();

    let response = engine.evaluate("comparison", None, None).unwrap();

    let total1 = response
        .results
        .iter()
        .find(|r| r.rule_name == "total1")
        .unwrap();
    let total2 = response
        .results
        .iter()
        .find(|r| r.rule_name == "total2")
        .unwrap();
    let difference = response
        .results
        .iter()
        .find(|r| r.rule_name == "difference")
        .unwrap();

    // path1: 100 * 1.21 = 121
    assert_eq!(total1.result.as_ref().unwrap().to_string(), "121");
    // path2: 75 * 1.21 = 90.75
    assert_eq!(total2.result.as_ref().unwrap().to_string(), "90.75");
    // difference: 90.75 - 121 = -30.25
    assert_eq!(difference.result.as_ref().unwrap().to_string(), "-30.25");
}

/// Multiple independent document references in a single document should all work.
/// Each reference should be independently resolvable.
#[test]
fn test_multiple_independent_doc_refs() {
    let mut engine = Engine::new();

    let config1_doc = r#"
doc config1
fact value = 100
rule doubled = value * 2
"#;

    let config2_doc = r#"
doc config2
fact value = 50
rule tripled = value * 3
"#;

    let combined_doc = r#"
doc combined
fact c1 = doc config1
fact c2 = doc config2
rule sum = c1.doubled? + c2.tripled?
rule product = c1.value * c2.value
"#;

    engine.add_lemma_code(config1_doc, "test.lemma").unwrap();
    engine.add_lemma_code(config2_doc, "test.lemma").unwrap();
    engine.add_lemma_code(combined_doc, "test.lemma").unwrap();

    let response = engine.evaluate("combined", None, None).unwrap();

    let sum = response
        .results
        .iter()
        .find(|r| r.rule_name == "sum")
        .unwrap();
    let product = response
        .results
        .iter()
        .find(|r| r.rule_name == "product")
        .unwrap();

    // sum: (100 * 2) + (50 * 3) = 200 + 150 = 350
    assert_eq!(sum.result.as_ref().unwrap().to_string(), "350");
    // product: 100 * 50 = 5000
    assert_eq!(product.result.as_ref().unwrap().to_string(), "5000");
}

/// Referencing rules from a document that itself has document references.
/// This tests transitive rule dependencies across document boundaries.
#[test]
fn test_transitive_rule_dependencies() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact x = 10
rule x_squared = x * x
"#;

    let middle_doc = r#"
doc middle
fact base_config = doc base
fact base_config.x = 20
rule x_squared_plus_ten = base_config.x_squared? + 10
"#;

    let top_doc = r#"
doc top
fact middle_config = doc middle
rule final_result = middle_config.x_squared_plus_ten? * 2
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(middle_doc, "test.lemma").unwrap();
    engine.add_lemma_code(top_doc, "test.lemma").unwrap();

    let response = engine.evaluate("top", None, None).unwrap();

    let final_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "final_result")
        .unwrap();

    // x=20 (overridden), x_squared=400, x_squared_plus_ten=410, final=820
    assert_eq!(final_result.result.as_ref().unwrap().to_string(), "820");
}

/// Overriding the same document reference in different ways should produce
/// different results based on the specific override path.
#[test]
fn test_same_doc_different_overrides() {
    let mut engine = Engine::new();

    let pricing_doc = r#"
doc pricing
fact price = 100
fact discount = 0%
rule final_price = price * (1 - discount)
"#;

    let scenario_doc = r#"
doc scenarios
fact retail = doc pricing
fact retail.discount = 5%

fact wholesale = doc pricing
fact wholesale.discount = 15%
fact wholesale.price = 80

rule retail_final = retail.final_price?
rule wholesale_final = wholesale.final_price?
rule price_difference = retail_final? - wholesale_final?
"#;

    engine.add_lemma_code(pricing_doc, "test.lemma").unwrap();
    engine.add_lemma_code(scenario_doc, "test.lemma").unwrap();

    let response = engine.evaluate("scenarios", None, None).unwrap();

    let retail_final = response
        .results
        .iter()
        .find(|r| r.rule_name == "retail_final")
        .unwrap();
    let wholesale_final = response
        .results
        .iter()
        .find(|r| r.rule_name == "wholesale_final")
        .unwrap();
    let price_difference = response
        .results
        .iter()
        .find(|r| r.rule_name == "price_difference")
        .unwrap();

    // retail: 100 * (1 - 0.05) = 95
    assert_eq!(retail_final.result.as_ref().unwrap().to_string(), "95");
    // wholesale: 80 * (1 - 0.15) = 68
    assert_eq!(wholesale_final.result.as_ref().unwrap().to_string(), "68");
    // difference: 95 - 68 = 27
    assert_eq!(price_difference.result.as_ref().unwrap().to_string(), "27");
}
