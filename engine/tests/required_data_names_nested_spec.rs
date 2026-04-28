use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;

#[test]
fn necessary_data_include_nested_spec_data_for_local_rule_deps() {
    let code = r#"
spec money
data money: scale
  -> unit eur 1
  -> unit usd 1.19

spec pricing
data money from money
data quantity: 10
data is_member: false
data price: money
rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_member then 15%
rule total: price - discount
  unless price < 50 eur then price

spec cashier
with calc: pricing
rule total: calc.total
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("cashier", Some(&now)).unwrap();

    // Schema for all rules: cashier.total depends on pricing.total (via calc.total),
    // so cashier's schema must include nested data like calc.price.
    let schema_all = plan.schema();
    assert!(
        schema_all.data.contains_key("calc.price"),
        "Expected schema data to include calc.price, got: {:?}",
        schema_all.data.keys().collect::<Vec<_>>()
    );
    let price_type = &schema_all.data.get("calc.price").unwrap().lemma_type;
    assert_eq!(
        price_type.name(),
        "money",
        "Expected calc.price to have type 'money'"
    );

    // Schema for specific rule: same result for cashier.total
    let schema_total = plan.schema_for_rules(&["total".to_string()]).unwrap();
    let scoped_price_type = &schema_total
        .data
        .get("calc.price")
        .expect("schema_for_rules must include calc.price with same typing as full schema")
        .lemma_type;
    assert_eq!(
        scoped_price_type.name(),
        "money",
        "scoped schema must preserve nested data type"
    );
    assert!(
        scoped_price_type.is_scale(),
        "calc.price must remain scale money in scoped schema"
    );
}

#[test]
fn schema_errors_on_unknown_rule() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec test\ndata x: 1\nrule y: x",
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_plan("test", Some(&now)).unwrap();
    let result = plan.schema_for_rules(&["nonexistent".to_string()]);
    assert!(result.is_err(), "Expected error for unknown rule");
    assert!(
        result.unwrap_err().to_string().contains("not found"),
        "Error should mention rule not found"
    );
}
