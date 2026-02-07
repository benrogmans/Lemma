use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;

#[test]
fn necessary_facts_include_nested_doc_facts_for_local_rule_deps() {
    let code = r#"
doc money
type money = scale
  -> unit eur 1
  -> unit usd 1.19

doc pricing
type money from money
fact quantity = 10
fact is_member = false
fact price = [money]
rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_member then 15%
rule total = price - discount?
  unless price < 50 eur then price

doc cashier
fact calc = doc pricing
rule total = calc.total?
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    // Local-rule interface: cashier.total depends on pricing.total (via calc.total?),
    // so cashier's necessary facts must include nested facts like calc.price.
    let necessary_all = engine.get_facts("cashier", &[]).unwrap();
    let has_calc_price = necessary_all.keys().any(|k| k.to_string() == "calc.price");
    assert!(
        has_calc_price,
        "Expected necessary facts to include calc.price, got: {:?}",
        necessary_all
            .keys()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
    );
    // Verify the type is included
    let price_type = necessary_all
        .iter()
        .find(|(k, _)| k.to_string() == "calc.price")
        .map(|(_, v)| v)
        .unwrap();
    assert_eq!(
        price_type.name(),
        "money",
        "Expected calc.price to have type 'money'"
    );

    let necessary_total = engine.get_facts("cashier", &["total".to_string()]).unwrap();
    let has_calc_price = necessary_total
        .keys()
        .any(|k| k.to_string() == "calc.price");
    assert!(
        has_calc_price,
        "Expected necessary facts for cashier.total to include calc.price, got: {:?}",
        necessary_total
            .keys()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn get_facts_errors_on_unknown_document() {
    let engine = Engine::new();
    let result = engine.get_facts("nonexistent", &[]);
    assert!(result.is_err(), "Expected error for unknown document");
    assert!(
        result.unwrap_err().to_string().contains("not found"),
        "Error should mention document not found"
    );
}

#[test]
fn get_facts_errors_on_unknown_rule() {
    let mut engine = Engine::new();
    add_lemma_code_blocking(
        &mut engine,
        "doc test\nfact x = 1\nrule y = x",
        "test.lemma",
    )
    .unwrap();

    let result = engine.get_facts("test", &["nonexistent".to_string()]);
    assert!(result.is_err(), "Expected error for unknown rule");
    assert!(
        result.unwrap_err().to_string().contains("not found"),
        "Error should mention rule not found"
    );
}
