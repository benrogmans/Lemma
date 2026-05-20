use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;

#[test]
fn necessary_data_include_nested_spec_data_for_local_rule_deps() {
    let code = r#"
spec money
data money: quantity
  -> unit eur 1
  -> unit usd 0.84

spec pricing
uses money
data quantity: 10
data is_member: false
data price: money.money
rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_member then 15%
rule total: price - discount
  unless price < 50 eur then price

spec cashier
uses calc: pricing
rule total: calc.total
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan(None, "cashier", Some(&now)).unwrap();

    // Schema for all rules: cashier.total depends on pricing.total (via calc.total),
    // so cashier's schema must include nested data like calc.price.
    let schema_all = plan.schema();
    assert!(
        schema_all.data.contains_key("calc.price"),
        "Expected schema data to include calc.price, got: {:?}",
        schema_all.data.keys().collect::<Vec<_>>()
    );
    let price_type = &schema_all.data.get("calc.price").unwrap().lemma_type;
    assert!(
        price_type.is_quantity(),
        "Expected calc.price to be a quantity type, got {:?}",
        price_type.name()
    );

    // Schema for specific rule: same result for cashier.total
    let schema_total = plan.schema_for_rules(&["total".to_string()]).unwrap();
    let scoped_price_type = &schema_total
        .data
        .get("calc.price")
        .expect("schema_for_rules must include calc.price with same typing as full schema")
        .lemma_type;
    assert!(
        scoped_price_type.is_quantity(),
        "scoped schema must preserve nested quantity type for calc.price"
    );
    assert_eq!(
        scoped_price_type.name(),
        price_type.name(),
        "scoped schema must match full schema type for calc.price"
    );
}

#[test]
fn schema_errors_on_unknown_rule() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec test\ndata x: 1\nrule y: x",
            lemma::SourceType::Volatile,
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan(None, "test", Some(&now)).unwrap();
    let result = plan.schema_for_rules(&["nonexistent".to_string()]);
    assert!(result.is_err(), "Expected error for unknown rule");
    assert!(
        result.unwrap_err().to_string().contains("not found"),
        "Error should mention rule not found"
    );
}
