use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

#[test]
fn test_type_system_with_imports_and_extensions() {
    let mut engine = Engine::new();

    let age_doc = r#"
doc age
type age = number
  -> minimum 0
  -> maximum 150
"#;

    let test_types_doc = r#"
doc test_types

type age from age

type adult_age = age
  -> minimum 21

fact age = [age]
fact adult_age = [adult_age]
fact twenties = [adult_age -> maximum 30]

rule total = age + adult_age + twenties
"#;

    add_lemma_code_blocking(&mut engine, age_doc, "age.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, test_types_doc, "test_types.lemma").unwrap();

    let mut facts = HashMap::new();
    facts.insert("age".to_string(), "25".to_string());
    facts.insert("adult_age".to_string(), "30".to_string());
    facts.insert("twenties".to_string(), "25".to_string());

    let response = engine
        .evaluate("test_types", vec![], facts)
        .expect("Evaluation failed");

    assert_eq!(response.doc_name, "test_types");

    let total_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule not found");

    // 25 + 30 + 25 = 80
    assert_eq!(total_rule.result.value().unwrap().to_string(), "80");
}
