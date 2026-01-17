use lemma::{parse, Engine, ResourceLimits};
use std::collections::HashMap;

#[test]
fn test_type_definition_parsing() {
    let code = r#"doc test
type dice = number -> minimum 0 -> maximum 6"#;

    let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
    assert_eq!(docs.len(), 1);

    let doc = &docs[0];
    assert_eq!(doc.name, "test");
    assert_eq!(doc.types.len(), 1);

    let type_def = &doc.types[0];
    match type_def {
        lemma::TypeDef::Regular {
            name,
            parent,
            overrides,
        } => {
            assert_eq!(name, "dice");
            assert_eq!(parent, "number");
            assert!(overrides.is_some());

            let overrides = overrides.as_ref().unwrap();
            assert_eq!(overrides.len(), 2);
            assert_eq!(overrides[0].0, "minimum");
            assert_eq!(overrides[0].1, vec!["0"]);
            assert_eq!(overrides[1].0, "maximum");
            assert_eq!(overrides[1].1, vec!["6"]);
        }
        _ => panic!("Expected Regular type definition"),
    }
}

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

    engine.add_lemma_code(age_doc, "age.lemma").unwrap();
    engine
        .add_lemma_code(test_types_doc, "test_types.lemma")
        .unwrap();

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
