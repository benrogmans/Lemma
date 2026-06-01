use lemma::DateTimeValue;
use lemma::{Engine, Error};
use std::collections::HashMap;

#[test]
fn test_inline_data_import() -> Result<(), Error> {
    let mut engine = Engine::new();

    // Define a type in one spec
    let age_spec = r#"
spec age
data age: number -> minimum 0 -> maximum 150
"#;

    // Use that type inline in another spec (without commands)
    let test_spec = r#"
spec test
uses age
data user_age: age.age
rule is_adult: user_age >= 18
"#;

    engine
        .load(
            age_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("age.lemma"))),
        )
        .expect("add age spec");
    engine
        .load(test_spec, lemma::SourceType::Volatile)
        .expect("add test spec");
    let now = DateTimeValue::now();

    let mut data = HashMap::new();
    data.insert("user_age".to_string(), "25".to_string());

    let response = engine.run(None, "test", Some(&now), data, false)?;

    // The data should be evaluated correctly with the imported type

    // Check the rule result
    let is_adult_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_adult")
        .expect("is_adult rule not found");

    assert_eq!(
        is_adult_result.boolean,
        Some(true),
        "25 >= 18 should be true"
    );

    Ok(())
}

#[test]
fn test_inline_data_import_with_constraints() -> Result<(), Error> {
    let mut engine = Engine::new();

    // Define a type in one spec
    let age_spec = r#"
spec age
data age: number -> minimum 0 -> maximum 150
"#;

    // Declare slot with tighter constraint, then with from imported type
    let test_spec = r#"
spec test
uses age
data user_age: age.age -> maximum 120
rule is_senior: user_age >= 65
"#;

    engine
        .load(
            age_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("age.lemma"))),
        )
        .expect("add age spec");
    engine
        .load(test_spec, lemma::SourceType::Volatile)
        .expect("add test spec");
    let now = DateTimeValue::now();

    let mut data = HashMap::new();
    data.insert("user_age".to_string(), "70".to_string());

    let response = engine.run(None, "test", Some(&now), data, false)?;

    // Check the rule result
    let is_senior_result = response
        .results
        .values()
        .find(|r| r.rule.name == "is_senior")
        .expect("is_senior rule not found");

    assert_eq!(
        is_senior_result.boolean,
        Some(true),
        "70 >= 65 should be true"
    );

    Ok(())
}
