use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, Error};
use std::collections::HashMap;

#[test]
fn test_inline_type_import() -> Result<(), Error> {
    let mut engine = Engine::new();

    // Define a type in one spec
    let age_spec = r#"
spec age
type age: number -> minimum 0 -> maximum 150
"#;

    // Use that type inline in another spec (without commands)
    let test_spec = r#"
spec test
fact user_age: [age from age]
rule is_adult: user_age >= 18
"#;

    engine
        .load(age_spec, lemma::SourceType::Labeled("age.lemma"))
        .expect("add age spec");
    engine
        .load(test_spec, lemma::SourceType::Labeled("test.lemma"))
        .expect("add test spec");
    let now = DateTimeValue::now();

    let mut facts = HashMap::new();
    facts.insert("user_age".to_string(), "25".to_string());

    let response = engine.run("test", Some(&now), facts, false)?;

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
fn test_inline_type_import_with_constraints() -> Result<(), Error> {
    let mut engine = Engine::new();

    // Define a type in one spec
    let age_spec = r#"
spec age
type age: number -> minimum 0 -> maximum 150
"#;

    // Use that type inline with additional constraints
    let test_spec = r#"
spec test
fact user_age: [age from age -> maximum 120]
rule is_senior: user_age >= 65
"#;

    engine
        .load(age_spec, lemma::SourceType::Labeled("age.lemma"))
        .expect("add age spec");
    engine
        .load(test_spec, lemma::SourceType::Labeled("test.lemma"))
        .expect("add test spec");
    let now = DateTimeValue::now();

    let mut facts = HashMap::new();
    facts.insert("user_age".to_string(), "70".to_string());

    let response = engine.run("test", Some(&now), facts, false)?;

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
