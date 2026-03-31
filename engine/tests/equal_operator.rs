use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

#[test]
fn test_equal_operator_numbers() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test_equal_numbers

fact a: 42
fact b: 42
fact c: 100

rule equal_true: a is b
rule equal_false: a is c
"#,
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("test_equal_numbers", Some(&now), HashMap::new(), false)
        .unwrap();

    let equal_true = response.results.get("equal_true").unwrap();
    assert_eq!(equal_true.result.value().unwrap().to_string(), "true");

    let equal_false = response.results.get("equal_false").unwrap();
    assert_eq!(equal_false.result.value().unwrap().to_string(), "false");
}

#[test]
fn test_equal_operator_text() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test_equal_text

fact greeting: "hello"
fact other: "world"

rule same_greeting: greeting is "hello"
rule different_greeting: greeting is other
"#,
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("test_equal_text", Some(&now), HashMap::new(), false)
        .unwrap();

    let same = response.results.get("same_greeting").unwrap();
    assert_eq!(same.result.value().unwrap().to_string(), "true");

    let different = response.results.get("different_greeting").unwrap();
    assert_eq!(different.result.value().unwrap().to_string(), "false");
}

#[test]
fn test_equal_operator_money() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test_equal_money

fact price_a: 100
fact price_b: 100
fact price_c: 50

rule same_price: price_a is price_b
rule different_price: price_a is price_c
"#,
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("test_equal_money", Some(&now), HashMap::new(), false)
        .unwrap();

    let same = response.results.get("same_price").unwrap();
    assert_eq!(same.result.value().unwrap().to_string(), "true");

    let different = response.results.get("different_price").unwrap();
    assert_eq!(different.result.value().unwrap().to_string(), "false");
}

#[test]
fn test_equal_operator_booleans() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test_equal_booleans

fact flag_a: true
fact flag_b: true
fact flag_c: false

rule both_true: flag_a is flag_b
rule mixed: flag_a is flag_c
"#,
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("test_equal_booleans", Some(&now), HashMap::new(), false)
        .unwrap();

    let both_true = response.results.get("both_true").unwrap();
    assert_eq!(both_true.result.value().unwrap().to_string(), "true");

    let mixed = response.results.get("mixed").unwrap();
    assert_eq!(mixed.result.value().unwrap().to_string(), "false");
}

#[test]
fn test_equal_operator_in_conditions() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec test_equal_conditions

fact status: "active"
fact count: 10

rule message: "inactive"
  unless status is "active" then "active"
  unless count is 10 then "count is 10"
"#,
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("test_equal_conditions", Some(&now), HashMap::new(), false)
        .unwrap();

    let message = response.results.get("message").unwrap();
    assert_eq!(message.result.value().unwrap().to_string(), "count is 10");
}
