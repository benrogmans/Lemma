use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

/// Test cross-document fact references (should work)
#[test]
fn test_cross_doc_fact_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact price: 100
fact quantity: 5
"#;

    let derived_doc = r#"
doc derived
fact base_data: doc base
rule total: base_data.price * base_data.quantity
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", vec![], HashMap::new()).unwrap();
    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .unwrap();

    assert_eq!(total.result.value().unwrap().to_string(), "500");
}

/// Test cross-document rule reference
#[test]
fn test_cross_doc_rule_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact value: 50
rule doubled: value * 2
"#;

    let derived_doc = r#"
doc derived
fact base_data: doc base
rule derived_value: base_data.doubled + 10
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", vec![], HashMap::new()).unwrap();
    let derived_value = response
        .results
        .values()
        .find(|r| r.rule.name == "derived_value")
        .unwrap();

    assert_eq!(derived_value.result.value().unwrap().to_string(), "110");
}

/// Test cross-document rule reference with dependencies
#[test]
fn test_cross_doc_rule_reference_with_dependencies() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base_employee
fact monthly_salary: 5000
rule annual_salary: monthly_salary * 12
rule with_bonus: annual_salary * 1.1
"#;

    let derived_doc = r#"
doc manager
fact employee: doc base_employee
rule manager_bonus: employee.annual_salary * 0.15
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("manager", vec![], HashMap::new()).unwrap();
    let bonus = response
        .results
        .values()
        .find(|r| r.rule.name == "manager_bonus")
        .unwrap();

    assert_eq!(bonus.result.value().unwrap().to_string(), "9000");
}

/// Test fact binding with cross-doc rule reference
#[test]
fn test_cross_doc_fact_binding_with_rule_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact price: 100
fact quantity: 5
rule total: price * quantity
"#;

    let derived_doc = r#"
doc derived
fact config: doc base
fact config.price: 200
fact config.quantity: 3
rule derived_total: config.total
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", vec![], HashMap::new()).unwrap();
    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "derived_total")
        .unwrap();

    assert_eq!(total.result.value().unwrap().to_string(), "600");
}

/// Test nested cross-document rule references
#[test]
fn test_nested_cross_doc_rule_reference() {
    let mut engine = Engine::new();

    let config_doc = r#"
doc config
fact base_days: 3
rule standard_processing_days: base_days
rule express_processing_days: 1
"#;

    let order_doc = r#"
doc order
fact is_express: false
rule processing_days: 5
"#;

    let derived_doc = r#"
doc derived
fact settings: doc config
fact order_info: doc order
rule total_days: settings.standard_processing_days + order_info.processing_days
"#;

    add_lemma_code_blocking(&mut engine, config_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, order_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", vec![], HashMap::new()).unwrap();
    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total_days")
        .unwrap();

    assert_eq!(total.result.value().unwrap().to_string(), "8");
}

/// Test cross-document rule reference in unless clause
#[test]
fn test_cross_doc_rule_reference_in_unless_clause() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact threshold: 100
fact value: 150
rule is_valid: value >= threshold
"#;

    let derived_doc = r#"
doc derived
fact base_data: doc base
rule status: "invalid"
  unless base_data.is_valid then "valid"
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", vec![], HashMap::new()).unwrap();
    let status = response
        .results
        .values()
        .find(|r| r.rule.name == "status")
        .unwrap();

    assert_eq!(status.result.value().unwrap().to_string(), "valid");
}

/// Test that we can mix cross-doc fact and rule references
#[test]
fn test_cross_doc_mixed_fact_and_rule_references() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact input: 50
rule calculated: input * 2
"#;

    let derived_doc = r#"
doc derived
fact base_data: doc base
rule combined: base_data.input + base_data.calculated
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", vec![], HashMap::new()).unwrap();
    let combined = response
        .results
        .values()
        .find(|r| r.rule.name == "combined")
        .unwrap();

    assert_eq!(combined.result.value().unwrap().to_string(), "150");
}

/// Test cross-document fact binding with multiple levels (should work)
#[test]
fn test_multi_level_fact_binding() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact x: 10
fact y: 20
fact z: 30
"#;

    let derived_doc = r#"
doc derived
fact data: doc base
fact data.x: 100
fact data.y: 200
rule sum: data.x + data.y + data.z
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", vec![], HashMap::new()).unwrap();
    let sum = response
        .results
        .values()
        .find(|r| r.rule.name == "sum")
        .unwrap();

    // x=100 (overridden), y=200 (overridden), z=30 (original)
    // 100 + 200 + 30 = 330
    assert_eq!(sum.result.value().unwrap().to_string(), "330");
}

/// Test simple fact binding without rule references (should work)
#[test]
fn test_simple_fact_binding() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact price: 100
fact quantity: 5
"#;

    let derived_doc = r#"
doc derived
fact config: doc base
fact config.price: 200
fact config.quantity: 3
rule total: config.price * config.quantity
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine.evaluate("derived", vec![], HashMap::new()).unwrap();
    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .unwrap();

    // Should be 200 * 3 = 600 (using overridden fact values)
    assert_eq!(total.result.value().unwrap().to_string(), "600");
}

/// Test that different fact paths to the same rule produce different results
/// This is the critical test for the RulePath implementation!
#[test]
fn test_different_fact_paths_produce_different_results() {
    let mut engine = Engine::new();

    let example1_doc = r#"
doc example1
fact price: 99
rule total: price * 1.21
"#;

    let example2_doc = r#"
doc example2
fact base: doc example1
"#;

    let example3_doc = r#"
doc example3
fact base: doc example2
rule total1: base.base.total

fact base2: doc example2
fact base2.base.price: 79
rule total2: base2.base.total
"#;

    add_lemma_code_blocking(&mut engine, example1_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, example2_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, example3_doc, "test.lemma").unwrap();

    let response = engine.evaluate("example3", vec![], HashMap::new()).unwrap();

    let total1 = response
        .results
        .values()
        .find(|r| r.rule.name == "total1")
        .unwrap();

    let total2 = response
        .results
        .values()
        .find(|r| r.rule.name == "total2")
        .unwrap();

    // total1 uses original price: 99 * 1.21 = 119.79
    assert_eq!(total1.result.value().unwrap().to_string(), "119.79");

    // total2 uses overridden price: 79 * 1.21 = 95.59
    assert_eq!(total2.result.value().unwrap().to_string(), "95.59");
}

#[test]
fn unversioned_ref_evaluates_latest_version() {
    let mut engine = Engine::new();

    let code = r#"
doc pricing.v1
fact base_price: 100

doc pricing.v2
fact base_price: 150

doc pricing.v10
fact base_price: 200

doc order
fact p: doc pricing
rule total: p.base_price
"#;

    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let response = engine.evaluate("order", vec![], HashMap::new()).unwrap();
    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .unwrap();

    assert_eq!(
        total.result.value().unwrap().to_string(),
        "200",
        "Unversioned ref should evaluate against pricing.v10 (latest by natural sort)"
    );
}

#[test]
fn cross_doc_dependency_rules_excluded_from_results() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base_employee
fact monthly_salary: 5000
fact employment_duration: 3 years
rule annual_salary: monthly_salary * 12
rule is_eligible_for_bonus: false
  unless employment_duration >= 1 years then true
"#;

    let derived_doc = r#"
doc specific_employee
fact employee: doc base_employee
rule salary_with_bonus: employee.annual_salary
  unless employee.is_eligible_for_bonus then employee.annual_salary * 1.1
rule employee_summary: employee.monthly_salary
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, derived_doc, "test.lemma").unwrap();

    let response = engine
        .evaluate("specific_employee", vec![], HashMap::new())
        .unwrap();

    let mut result_names: Vec<&str> = response.results.keys().map(|k| k.as_str()).collect();
    result_names.sort();
    assert_eq!(
        result_names,
        vec!["employee_summary", "salary_with_bonus"],
        "Only local rules should appear in results; cross-doc dependencies \
         (annual_salary, is_eligible_for_bonus) must be excluded"
    );

    assert_eq!(
        response
            .results
            .get("salary_with_bonus")
            .unwrap()
            .result
            .value()
            .unwrap()
            .to_string(),
        "66000"
    );
}

#[test]
fn versioned_ref_evaluates_exact_version() {
    let mut engine = Engine::new();

    let code = r#"
doc pricing.v1
fact base_price: 100

doc pricing.v2
fact base_price: 150

doc pricing.v10
fact base_price: 200

doc order
fact p: doc pricing.v2
rule total: p.base_price
"#;

    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let response = engine.evaluate("order", vec![], HashMap::new()).unwrap();
    let total = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .unwrap();

    assert_eq!(
        total.result.value().unwrap().to_string(),
        "150",
        "Versioned ref should evaluate against pricing.v2 exactly"
    );
}
