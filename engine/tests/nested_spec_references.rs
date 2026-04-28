use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

/// Rule references work through one level of spec reference.
#[test]
fn test_single_level_spec_ref_with_rule_reference() {
    let mut engine = Engine::new();

    let base_spec = r#"
spec pricing
data base_price: 100
data tax_rate: 21%
rule final_price: base_price * (1 + tax_rate)
"#;

    let line_item_spec = r#"
spec line_item
with pricing
data quantity: 10
rule line_total: pricing.final_price * quantity
"#;

    engine
        .load(base_spec, lemma::SourceType::Labeled("pricing.lemma"))
        .unwrap();
    engine
        .load(
            line_item_spec,
            lemma::SourceType::Labeled("line_item.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("line_item", Some(&now), HashMap::new(), false)
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

/// Multi-level spec rule references should work correctly.
/// When spec A references spec B which references spec C,
/// rule references through the chain should resolve properly.
#[test]
fn test_multi_level_spec_rule_reference() {
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
data value: 100
rule doubled: value * 2
"#;

    let middle_spec = r#"
spec middle
with base_ref: base
rule middle_calc: base_ref.doubled + 50
"#;

    let top_spec = r#"
spec top
with middle_ref: middle
rule top_calc: middle_ref.middle_calc
"#;

    engine
        .load(base_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(middle_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(top_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("top", Some(&now), HashMap::new(), false)
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

/// The old `data X: spec Y` syntax is rejected with a helpful error.
#[test]
fn test_old_data_spec_syntax_rejected() {
    let mut engine = Engine::new();

    let specs = r#"
spec a
data x: spec other
"#;

    let errs = engine
        .load(specs, lemma::SourceType::Labeled("test.lemma"))
        .unwrap_err();
    let msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        msg.contains("syntax has been removed"),
        "expected old syntax rejection, got: {msg}"
    );
}

/// Accessing data through multi-level spec references with nested bindings works correctly.
#[test]
fn test_multi_level_data_access_through_spec_refs() {
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
data value: 50
"#;

    let middle_spec = r#"
spec middle
with config: base
data config.value: 100
"#;

    let top_spec = r#"
spec top
with settings: middle
rule final_value: settings.config.value * 2
"#;

    engine
        .load(base_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(middle_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(top_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("top", Some(&now), HashMap::new(), false)
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

/// Deep nested data bindings through multiple spec layers should work.
/// Overriding data like order.line.pricing.tax_rate through multiple levels.
#[test]
fn test_deep_nested_data_binding() {
    let mut engine = Engine::new();

    let pricing_spec = r#"
spec pricing
data base_price: 100
data tax_rate: 21%
rule final_price: base_price * (1 + tax_rate)
"#;

    let line_item_spec = r#"
spec line_item
with pricing
data quantity: 10
rule line_total: pricing.final_price * quantity
"#;

    let order_spec = r#"
spec order
with line: line_item
data line.pricing.tax_rate: 10%
data line.quantity: 5
rule order_total: line.line_total
"#;

    engine
        .load(pricing_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(line_item_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(order_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("order", Some(&now), HashMap::new(), false)
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

/// Different data paths to the same base spec should produce different results
/// when bindings are applied. This tests that rule evaluation respects the specific
/// path through spec references.
#[test]
fn test_different_paths_different_results() {
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
data price: 100
rule total: price * 1.21
"#;

    let wrapper_spec = r#"
spec wrapper
with base
"#;

    let comparison_spec = r#"
spec comparison
with path1: wrapper
with path2: wrapper
data path2.base.price: 75
rule total1: path1.base.total
rule total2: path2.base.total
rule difference: total2 - total1
"#;

    engine
        .load(base_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(wrapper_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(comparison_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("comparison", Some(&now), HashMap::new(), false)
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

/// Multiple independent spec references in a single spec should all work.
/// Each reference should be independently resolvable.
#[test]
fn test_multiple_independent_spec_refs() {
    let mut engine = Engine::new();

    let config1_spec = r#"
spec config1
data value: 100
rule doubled: value * 2
"#;

    let config2_spec = r#"
spec config2
data value: 50
rule tripled: value * 3
"#;

    let combined_spec = r#"
spec combined
with c1: config1
with c2: config2
rule sum: c1.doubled + c2.tripled
rule product: c1.value * c2.value
"#;

    engine
        .load(config1_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(config2_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(combined_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("combined", Some(&now), HashMap::new(), false)
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

/// Referencing rules from a spec that itself has spec references.
/// This tests transitive rule dependencies across spec boundaries.
#[test]
fn test_transitive_rule_dependencies() {
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
data x: 10
rule x_squared: x * x
"#;

    let middle_spec = r#"
spec middle
with base_config: base
data base_config.x: 20
rule x_squared_plus_ten: base_config.x_squared + 10
"#;

    let top_spec = r#"
spec top
with middle_config: middle
rule final_result: middle_config.x_squared_plus_ten * 2
"#;

    engine
        .load(base_spec, lemma::SourceType::Labeled("base.lemma"))
        .unwrap();
    engine
        .load(middle_spec, lemma::SourceType::Labeled("middle.lemma"))
        .unwrap();
    engine
        .load(top_spec, lemma::SourceType::Labeled("top.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("top", Some(&now), HashMap::new(), false)
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

/// Overriding the same spec reference in different ways should produce
/// different results based on the specific binding path.
#[test]
fn test_same_spec_different_bindings() {
    let mut engine = Engine::new();

    let pricing_spec = r#"
spec pricing
data price: 100
data discount: 0%
rule final_price: price * (1 - discount)
"#;

    let scenario_spec = r#"
spec scenarios
with retail: pricing
data retail.discount: 5%

with wholesale: pricing
data wholesale.discount: 15%
data wholesale.price: 80

rule retail_final: retail.final_price
rule wholesale_final: wholesale.final_price
rule price_difference: retail_final - wholesale_final
"#;

    engine
        .load(pricing_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    engine
        .load(scenario_spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("scenarios", Some(&now), HashMap::new(), false)
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

/// The old `data X: spec Y` syntax is rejected even with dotted paths.
#[test]
fn test_old_data_spec_syntax_rejected_with_dotted_path() {
    let mut engine = Engine::new();

    let specs = r#"
spec a
rule x: 5

spec c
with aa: a
rule y: aa.x > 1

spec d
with cc: c
data cc.aa: spec a
rule yy: cc.y
"#;

    let errs = engine
        .load(specs, lemma::SourceType::Labeled("test.lemma"))
        .unwrap_err();
    let msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        msg.contains("syntax has been removed"),
        "expected old syntax rejection, got: {msg}"
    );
}
