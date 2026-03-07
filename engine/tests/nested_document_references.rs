mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

/// Rule references work through one level of document reference.
#[test]
fn test_single_level_doc_ref_with_rule_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc pricing
fact base_price: 100
fact tax_rate: 21%
rule final_price: base_price * (1 + tax_rate)
"#;

    let line_item_doc = r#"
doc line_item
fact pricing: doc pricing
fact quantity: 10
rule line_total: pricing.final_price * quantity
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "pricing.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, line_item_doc, "line_item.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("line_item", None, &now, vec![], HashMap::new())
        .unwrap();
    let line_total = response
        .results
        .values()
        .find(|r| r.rule.name == "line_total")
        .unwrap();

    // Should be: (100 * 1.21) * 10 = 1210
    match &line_total.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("1210").unwrap()),
            other => panic!("Expected Number for line_total, got {:?}", other),
        },
        other => panic!("Expected Value for line_total, got {:?}", other),
    }
}

/// Multi-level document rule references should work correctly.
/// When document A references document B which references document C,
/// rule references through the chain should resolve properly.
#[test]
fn test_multi_level_document_rule_reference() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact value: 100
rule doubled: value * 2
"#;

    let middle_doc = r#"
doc middle
fact base_ref: doc base
rule middle_calc: base_ref.doubled + 50
"#;

    let top_doc = r#"
doc top
fact middle_ref: doc middle
rule top_calc: middle_ref.middle_calc
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, middle_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, top_doc, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("top", None, &now, vec![], HashMap::new())
        .unwrap();

    let top_calc = response
        .results
        .values()
        .find(|r| r.rule.name == "top_calc")
        .expect("top_calc rule not found in results");

    match &top_calc.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("250").unwrap()),
            other => panic!("Expected Number for top_calc, got {:?}", other),
        },
        other => panic!("Expected Value for top_calc, got {:?}", other),
    }
}

/// Overriding nested document references should propagate through rule evaluations.
/// When we bind a nested document reference and reference rules through that chain,
/// the overridden document should be used in the evaluation.
#[test]
fn test_nested_document_binding_with_rule_reference() {
    let mut engine = Engine::new();

    let pricing_doc = r#"
doc pricing
fact base_price: 100
rule final_price: base_price * 1.1
"#;

    let wholesale_doc = r#"
doc wholesale_pricing
fact base_price: 75
rule final_price: base_price * 1.1
"#;

    let line_item_doc = r#"
doc line_item
fact pricing: doc pricing
fact quantity: 10
rule line_total: pricing.final_price * quantity
"#;

    let order_doc = r#"
doc order
fact line: doc line_item
fact line.pricing: doc wholesale_pricing
fact line.quantity: 100
rule order_total: line.line_total
"#;

    add_lemma_code_blocking(&mut engine, pricing_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, wholesale_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, line_item_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, order_doc, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("order", None, &now, vec![], HashMap::new())
        .unwrap();

    let order_total = response
        .results
        .values()
        .find(|r| r.rule.name == "order_total")
        .expect("order_total rule not found in results");

    match &order_total.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("8250").unwrap()),
            other => panic!("Expected Number for order_total, got {:?}", other),
        },
        other => panic!("Expected Value for order_total, got {:?}", other),
    }
}

/// Accessing facts through multi-level document references with nested bindings works correctly.
#[test]
fn test_multi_level_fact_access_through_doc_refs() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact value: 50
"#;

    let middle_doc = r#"
doc middle
fact config: doc base
fact config.value: 100
"#;

    let top_doc = r#"
doc top
fact settings: doc middle
rule final_value: settings.config.value * 2
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, middle_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, top_doc, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("top", None, &now, vec![], HashMap::new())
        .unwrap();
    let final_value = response
        .results
        .values()
        .find(|r| r.rule.name == "final_value")
        .unwrap();

    // Should be: 100 * 2 = 200 (using the overridden value from middle)
    match &final_value.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("200").unwrap()),
            other => panic!("Expected Number for final_value, got {:?}", other),
        },
        other => panic!("Expected Value for final_value, got {:?}", other),
    }
}

/// Deep nested fact bindings through multiple document layers should work.
/// Overriding facts like order.line.pricing.tax_rate through multiple levels.
#[test]
fn test_deep_nested_fact_binding() {
    let mut engine = Engine::new();

    let pricing_doc = r#"
doc pricing
fact base_price: 100
fact tax_rate: 21%
rule final_price: base_price * (1 + tax_rate)
"#;

    let line_item_doc = r#"
doc line_item
fact pricing: doc pricing
fact quantity: 10
rule line_total: pricing.final_price * quantity
"#;

    let order_doc = r#"
doc order
fact line: doc line_item
fact line.pricing.tax_rate: 10%
fact line.quantity: 5
rule order_total: line.line_total
"#;

    add_lemma_code_blocking(&mut engine, pricing_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, line_item_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, order_doc, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("order", None, &now, vec![], HashMap::new())
        .unwrap();

    let order_total = response
        .results
        .values()
        .find(|r| r.rule.name == "order_total")
        .expect("order_total rule not found");

    // base_price=100, tax_rate=10% (overridden), quantity=5
    // (100 * 1.10) * 5 = 550
    match &order_total.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("550").unwrap()),
            other => panic!("Expected Number for order_total, got {:?}", other),
        },
        other => panic!("Expected Value for order_total, got {:?}", other),
    }
}

/// Different fact paths to the same base document should produce different results
/// when bindings are applied. This tests that rule evaluation respects the specific
/// path through document references.
#[test]
fn test_different_paths_different_results() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact price: 100
rule total: price * 1.21
"#;

    let wrapper_doc = r#"
doc wrapper
fact base: doc base
"#;

    let comparison_doc = r#"
doc comparison
fact path1: doc wrapper
fact path2: doc wrapper
fact path2.base.price: 75
rule total1: path1.base.total
rule total2: path2.base.total
rule difference: total2 - total1
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, wrapper_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, comparison_doc, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("comparison", None, &now, vec![], HashMap::new())
        .unwrap();

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
    let difference = response
        .results
        .values()
        .find(|r| r.rule.name == "difference")
        .unwrap();

    // path1: 100 * 1.21 = 121
    match &total1.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("121").unwrap()),
            other => panic!("Expected Number for total1, got {:?}", other),
        },
        other => panic!("Expected Value for total1, got {:?}", other),
    }
    // path2: 75 * 1.21 = 90.75
    match &total2.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("90.75").unwrap()),
            other => panic!("Expected Number for total2, got {:?}", other),
        },
        other => panic!("Expected Value for total2, got {:?}", other),
    }
    // difference: 90.75 - 121 = -30.25
    match &difference.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("-30.25").unwrap()),
            other => panic!("Expected Number for difference, got {:?}", other),
        },
        other => panic!("Expected Value for difference, got {:?}", other),
    }
}

/// Multiple independent document references in a single document should all work.
/// Each reference should be independently resolvable.
#[test]
fn test_multiple_independent_doc_refs() {
    let mut engine = Engine::new();

    let config1_doc = r#"
doc config1
fact value: 100
rule doubled: value * 2
"#;

    let config2_doc = r#"
doc config2
fact value: 50
rule tripled: value * 3
"#;

    let combined_doc = r#"
doc combined
fact c1: doc config1
fact c2: doc config2
rule sum: c1.doubled + c2.tripled
rule product: c1.value * c2.value
"#;

    add_lemma_code_blocking(&mut engine, config1_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, config2_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, combined_doc, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("combined", None, &now, vec![], HashMap::new())
        .unwrap();

    let sum = response
        .results
        .values()
        .find(|r| r.rule.name == "sum")
        .unwrap();
    let product = response
        .results
        .values()
        .find(|r| r.rule.name == "product")
        .unwrap();

    // sum: (100 * 2) + (50 * 3) = 200 + 150 = 350
    match &sum.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("350").unwrap()),
            other => panic!("Expected Number for sum, got {:?}", other),
        },
        other => panic!("Expected Value for sum, got {:?}", other),
    }
    // product: 100 * 50 = 5000
    match &product.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("5000").unwrap()),
            other => panic!("Expected Number for product, got {:?}", other),
        },
        other => panic!("Expected Value for product, got {:?}", other),
    }
}

/// Referencing rules from a document that itself has document references.
/// This tests transitive rule dependencies across document boundaries.
#[test]
fn test_transitive_rule_dependencies() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact x: 10
rule x_squared: x * x
"#;

    let middle_doc = r#"
doc middle
fact base_config: doc base
fact base_config.x: 20
rule x_squared_plus_ten: base_config.x_squared + 10
"#;

    let top_doc = r#"
doc top
fact middle_config: doc middle
rule final_result: middle_config.x_squared_plus_ten * 2
"#;

    add_lemma_code_blocking(&mut engine, base_doc, "base.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, middle_doc, "middle.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, top_doc, "top.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("top", None, &now, vec![], HashMap::new())
        .unwrap();

    let final_result = response
        .results
        .values()
        .find(|r| r.rule.name == "final_result")
        .unwrap();

    // x=20 (overridden), x_squared=400, x_squared_plus_ten=410, final=820
    match &final_result.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("820").unwrap()),
            other => panic!("Expected Number for final_result, got {:?}", other),
        },
        other => panic!("Expected Value for final_result, got {:?}", other),
    }
}

/// Overriding the same document reference in different ways should produce
/// different results based on the specific binding path.
#[test]
fn test_same_doc_different_bindings() {
    let mut engine = Engine::new();

    let pricing_doc = r#"
doc pricing
fact price: 100
fact discount: 0%
rule final_price: price * (1 - discount)
"#;

    let scenario_doc = r#"
doc scenarios
fact retail: doc pricing
fact retail.discount: 5%

fact wholesale: doc pricing
fact wholesale.discount: 15%
fact wholesale.price: 80

rule retail_final: retail.final_price
rule wholesale_final: wholesale.final_price
rule price_difference: retail_final - wholesale_final
"#;

    add_lemma_code_blocking(&mut engine, pricing_doc, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, scenario_doc, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("scenarios", None, &now, vec![], HashMap::new())
        .unwrap();

    let retail_final = response
        .results
        .values()
        .find(|r| r.rule.name == "retail_final")
        .unwrap();
    let wholesale_final = response
        .results
        .values()
        .find(|r| r.rule.name == "wholesale_final")
        .unwrap();
    let price_difference = response
        .results
        .values()
        .find(|r| r.rule.name == "price_difference")
        .unwrap();

    // retail: 100 * (1 - 0.05) = 95
    match &retail_final.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("95").unwrap()),
            other => panic!("Expected Number for retail_final, got {:?}", other),
        },
        other => panic!("Expected Value for retail_final, got {:?}", other),
    }
    // wholesale: 80 * (1 - 0.15) = 68
    match &wholesale_final.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("68").unwrap()),
            other => panic!("Expected Number for wholesale_final, got {:?}", other),
        },
        other => panic!("Expected Value for wholesale_final, got {:?}", other),
    }
    // difference: 95 - 68 = 27
    match &price_difference.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => assert_eq!(*n, Decimal::from_str("27").unwrap()),
            other => panic!("Expected Number for price_difference, got {:?}", other),
        },
        other => panic!("Expected Value for price_difference, got {:?}", other),
    }
}

/// Binding interface validation: binding a doc ref to a document with the same rule name
/// but incompatible result type is rejected at the binding site.
#[test]
fn test_doc_ref_binding_interface_rule_type_rejected() {
    let mut engine = Engine::new();

    let doc_a = r#"
doc a
rule x: 5
"#;

    let doc_b = r#"
doc b
rule x: true
"#;

    let doc_c = r#"
doc c
fact aa: doc a
rule y: aa.x > 1
"#;

    let doc_d = r#"
doc d
fact cc: doc c
fact cc.aa: doc b
rule yy: cc.y
"#;

    add_lemma_code_blocking(&mut engine, doc_a, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, doc_b, "test.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, doc_c, "test.lemma").unwrap();
    let errs = add_lemma_code_blocking(&mut engine, doc_d, "test.lemma").unwrap_err();
    let err_str = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    // We must reject the bad binding. Either we report at the binding site (preferred)
    // or the expression type checker reports the comparison error.
    let binding_site_error = err_str.contains("Fact binding 'cc.aa'")
        && err_str.contains("sets document reference to 'b'");
    let comparison_error = err_str.contains("Cannot compare") && err_str.contains("Boolean");
    assert!(
        binding_site_error || comparison_error,
        "expected binding-site or comparison type error for bad doc binding, got: {}",
        err_str
    );
}
