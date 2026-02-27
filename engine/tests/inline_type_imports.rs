mod common;
use common::add_lemma_code_blocking;
use lemma::{Engine, LemmaResult};
use std::collections::HashMap;

#[test]
fn test_inline_type_import() -> LemmaResult<()> {
    let mut engine = Engine::new();

    // Define a type in one document
    let age_doc = r#"
doc age
type age: number -> minimum 0 -> maximum 150
"#;

    // Use that type inline in another document (without commands)
    let test_doc = r#"
doc test
fact user_age: [age from age]
rule is_adult: user_age >= 18
"#;

    add_lemma_code_blocking(&mut engine, age_doc, "age.lemma")?;
    add_lemma_code_blocking(&mut engine, test_doc, "test.lemma")?;

    let mut facts = HashMap::new();
    facts.insert("user_age".to_string(), "25".to_string());

    let response = engine.evaluate("test", vec![], facts)?;

    // The fact should be evaluated correctly with the imported type

    // Check the rule result
    let is_adult_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_adult")
        .expect("is_adult rule not found");

    match &is_adult_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Boolean(b) = &lit.value {
                assert!(*b, "25 >= 18 should be true");
            } else {
                panic!("Expected boolean result");
            }
        }
        _ => panic!("Expected boolean result"),
    }

    Ok(())
}

#[test]
fn test_inline_type_import_with_constraints() -> LemmaResult<()> {
    let mut engine = Engine::new();

    // Define a type in one document
    let age_doc = r#"
doc age
type age: number -> minimum 0 -> maximum 150
"#;

    // Use that type inline with additional constraints
    let test_doc = r#"
doc test
fact user_age: [age from age -> maximum 120]
rule is_senior: user_age >= 65
"#;

    add_lemma_code_blocking(&mut engine, age_doc, "age.lemma")?;
    add_lemma_code_blocking(&mut engine, test_doc, "test.lemma")?;

    let mut facts = HashMap::new();
    facts.insert("user_age".to_string(), "70".to_string());

    let response = engine.evaluate("test", vec![], facts)?;

    // Check the rule result
    let is_senior_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_senior")
        .expect("is_senior rule not found");

    match &is_senior_result.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Boolean(b) = &lit.value {
                assert!(*b, "70 >= 65 should be true");
            } else {
                panic!("Expected boolean result");
            }
        }
        _ => panic!("Expected boolean result"),
    }

    Ok(())
}
