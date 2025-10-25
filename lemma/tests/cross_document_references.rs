use lemma::Engine;

/// Test cross-document fact references (should work)
#[test]
fn test_cross_doc_fact_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact price = 100
fact quantity = 5
"#;

    let derived_doc = r#"
doc derived
fact base_data = doc base
rule total = base_data.price * base_data.quantity
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", None, None).unwrap();
    let total = response
        .results
        .iter()
        .find(|r| r.rule_name == "total")
        .unwrap();

    assert_eq!(total.result.as_ref().unwrap().to_string(), "500");
}

/// Test cross-document rule reference
#[test]
fn test_cross_doc_rule_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact value = 50
rule doubled = value * 2
"#;

    let derived_doc = r#"
doc derived
fact base_data = doc base
rule derived_value = base_data.doubled? + 10
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", None, None).unwrap();
    let derived_value = response
        .results
        .iter()
        .find(|r| r.rule_name == "derived_value")
        .unwrap();

    assert_eq!(derived_value.result.as_ref().unwrap().to_string(), "110");
}

/// Test cross-document rule reference with dependencies
#[test]
fn test_cross_doc_rule_reference_with_dependencies() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base_employee
fact monthly_salary = 5000
rule annual_salary = monthly_salary * 12
rule with_bonus = annual_salary? * 1.1
"#;

    let derived_doc = r#"
doc manager
fact employee = doc base_employee
rule manager_bonus = employee.annual_salary? * 0.15
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("manager", None, None).unwrap();
    let bonus = response
        .results
        .iter()
        .find(|r| r.rule_name == "manager_bonus")
        .unwrap();

    assert_eq!(bonus.result.as_ref().unwrap().to_string(), "9000.00");
}

/// Test fact override with cross-doc rule reference
#[test]
fn test_cross_doc_fact_override_with_rule_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact price = 100
fact quantity = 5
rule total = price * quantity
"#;

    let derived_doc = r#"
doc derived
fact config = doc base
fact config.price = 200
fact config.quantity = 3
rule derived_total = config.total?
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", None, None).unwrap();
    let total = response
        .results
        .iter()
        .find(|r| r.rule_name == "derived_total")
        .unwrap();

    assert_eq!(total.result.as_ref().unwrap().to_string(), "600");
}

/// Test nested cross-document rule references
#[test]
fn test_nested_cross_doc_rule_reference() {
    let mut engine = Engine::new();

    let config_doc = r#"
doc config
fact base_days = 3
rule standard_processing_days = base_days
rule express_processing_days = 1
"#;

    let order_doc = r#"
doc order
fact is_express = false
rule processing_days = 5
"#;

    let derived_doc = r#"
doc derived
fact settings = doc config
fact order_info = doc order
rule total_days = settings.standard_processing_days? + order_info.processing_days?
"#;

    engine.add_lemma_code(config_doc, "test.lemma").unwrap();
    engine.add_lemma_code(order_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", None, None).unwrap();
    let total = response
        .results
        .iter()
        .find(|r| r.rule_name == "total_days")
        .unwrap();

    assert_eq!(total.result.as_ref().unwrap().to_string(), "8");
}

/// Test cross-document rule reference in unless clause
#[test]
fn test_cross_doc_rule_reference_in_unless_clause() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact threshold = 100
fact value = 150
rule is_valid = value >= threshold
"#;

    let derived_doc = r#"
doc derived
fact base_data = doc base
rule status = "invalid"
  unless base_data.is_valid? then "valid"
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", None, None).unwrap();
    let status = response
        .results
        .iter()
        .find(|r| r.rule_name == "status")
        .unwrap();

    assert_eq!(status.result.as_ref().unwrap().to_string(), "\"valid\"");
}

/// Test that we can mix cross-doc fact and rule references
#[test]
fn test_cross_doc_mixed_fact_and_rule_references() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact input = 50
rule calculated = input * 2
"#;

    let derived_doc = r#"
doc derived
fact base_data = doc base
rule combined = base_data.input + base_data.calculated?
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", None, None).unwrap();
    let combined = response
        .results
        .iter()
        .find(|r| r.rule_name == "combined")
        .unwrap();

    assert_eq!(combined.result.as_ref().unwrap().to_string(), "150");
}

/// Test cross-document fact override with multiple levels (should work)
#[test]
fn test_multi_level_fact_override() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact x = 10
fact y = 20
fact z = 30
"#;

    let derived_doc = r#"
doc derived
fact data = doc base
fact data.x = 100
fact data.y = 200
rule sum = data.x + data.y + data.z
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", None, None).unwrap();
    let sum = response
        .results
        .iter()
        .find(|r| r.rule_name == "sum")
        .unwrap();

    // x=100 (overridden), y=200 (overridden), z=30 (original)
    // 100 + 200 + 30 = 330
    assert_eq!(sum.result.as_ref().unwrap().to_string(), "330");
}

/// Test simple fact override without rule references (should work)
#[test]
fn test_simple_fact_override() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact price = 100
fact quantity = 5
"#;

    let derived_doc = r#"
doc derived
fact config = doc base
fact config.price = 200
fact config.quantity = 3
rule total = config.price * config.quantity
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", None, None).unwrap();
    let total = response
        .results
        .iter()
        .find(|r| r.rule_name == "total")
        .unwrap();

    // Should be 200 * 3 = 600 (using overridden fact values)
    assert_eq!(total.result.as_ref().unwrap().to_string(), "600");
}

/// Test that different fact paths to the same rule produce different results
/// This is the critical test for the RulePath implementation!
#[test]
fn test_different_fact_paths_produce_different_results() {
    let mut engine = Engine::new();

    let example1_doc = r#"
doc example1
fact price = 99
rule total = price * 1.21
"#;

    let example2_doc = r#"
doc example2
fact base = doc example1
"#;

    let example3_doc = r#"
doc example3
fact base = doc example2
rule total1 = base.base.total?

fact base2 = doc example2
fact base2.base.price = 79
rule total2 = base2.base.total?
"#;

    engine.add_lemma_code(example1_doc, "test.lemma").unwrap();
    engine.add_lemma_code(example2_doc, "test.lemma").unwrap();
    engine.add_lemma_code(example3_doc, "test.lemma").unwrap();

    let response = engine.evaluate("example3", None, None).unwrap();

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

    // total1 uses original price: 99 * 1.21 = 119.79
    assert_eq!(total1.result.as_ref().unwrap().to_string(), "119.79");

    // total2 uses overridden price: 79 * 1.21 = 95.59
    assert_eq!(total2.result.as_ref().unwrap().to_string(), "95.59");
}
