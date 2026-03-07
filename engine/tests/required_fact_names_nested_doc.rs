use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;

#[test]
fn necessary_facts_include_nested_doc_facts_for_local_rule_deps() {
    let code = r#"
doc money
type money: scale
  -> unit eur 1
  -> unit usd 1.19

doc pricing
type money from money
fact quantity: 10
fact is_member: false
fact price: [money]
rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_member then 15%
rule total: price - discount
  unless price < 50 eur then price

doc cashier
fact calc: doc pricing
rule total: calc.total
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_execution_plan("cashier", None, &now).unwrap();

    // Schema for all rules: cashier.total depends on pricing.total (via calc.total),
    // so cashier's schema must include nested facts like calc.price.
    let schema_all = plan.schema();
    assert!(
        schema_all.facts.contains_key("calc.price"),
        "Expected schema facts to include calc.price, got: {:?}",
        schema_all.facts.keys().collect::<Vec<_>>()
    );
    let (price_type, _) = schema_all.facts.get("calc.price").unwrap();
    assert_eq!(
        price_type.name(),
        "money",
        "Expected calc.price to have type 'money'"
    );

    // Schema for specific rule: same result for cashier.total
    let schema_total = plan.schema_for_rules(&["total".to_string()]).unwrap();
    assert!(
        schema_total.facts.contains_key("calc.price"),
        "Expected schema_for_rules facts for cashier.total to include calc.price, got: {:?}",
        schema_total.facts.keys().collect::<Vec<_>>()
    );
}

#[test]
fn schema_errors_on_unknown_rule() {
    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, "doc test\nfact x: 1\nrule y: x", "test.lemma").unwrap();
    let now = DateTimeValue::now();

    let plan = engine.get_execution_plan("test", None, &now).unwrap();
    let result = plan.schema_for_rules(&["nonexistent".to_string()]);
    assert!(result.is_err(), "Expected error for unknown rule");
    assert!(
        result.unwrap_err().to_string().contains("not found"),
        "Error should mention rule not found"
    );
}
