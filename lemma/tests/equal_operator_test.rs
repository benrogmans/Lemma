use lemma::Engine;

#[test]
fn test_equal_operator_numbers() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test_equal_numbers

fact a = 42
fact b = 42
fact c = 100

rule equal_true = a == b
rule equal_false = a == c
"#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test_equal_numbers", vec![]).unwrap();

    let equal_true = response
        .results
        .iter()
        .find(|r| r.rule_name == "equal_true")
        .unwrap();
    assert_eq!(equal_true.result.as_ref().unwrap().to_string(), "true");

    let equal_false = response
        .results
        .iter()
        .find(|r| r.rule_name == "equal_false")
        .unwrap();
    assert_eq!(equal_false.result.as_ref().unwrap().to_string(), "false");
}

#[test]
fn test_equal_operator_text() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test_equal_text

fact greeting = "hello"
fact other = "world"

rule same_greeting = greeting == "hello"
rule different_greeting = greeting == other
"#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test_equal_text", vec![]).unwrap();

    let same = response
        .results
        .iter()
        .find(|r| r.rule_name == "same_greeting")
        .unwrap();
    assert_eq!(same.result.as_ref().unwrap().to_string(), "true");

    let different = response
        .results
        .iter()
        .find(|r| r.rule_name == "different_greeting")
        .unwrap();
    assert_eq!(different.result.as_ref().unwrap().to_string(), "false");
}

#[test]
fn test_equal_operator_money() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test_equal_money

fact price_a = 100
fact price_b = 100
fact price_c = 50

rule same_price = price_a == price_b
rule different_price = price_a == price_c
"#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test_equal_money", vec![]).unwrap();

    let same = response
        .results
        .iter()
        .find(|r| r.rule_name == "same_price")
        .unwrap();
    assert_eq!(same.result.as_ref().unwrap().to_string(), "true");

    let different = response
        .results
        .iter()
        .find(|r| r.rule_name == "different_price")
        .unwrap();
    assert_eq!(different.result.as_ref().unwrap().to_string(), "false");
}

#[test]
fn test_equal_operator_booleans() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test_equal_booleans

fact flag_a = true
fact flag_b = true
fact flag_c = false

rule both_true = flag_a == flag_b
rule mixed = flag_a == flag_c
"#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test_equal_booleans", vec![]).unwrap();

    let both_true = response
        .results
        .iter()
        .find(|r| r.rule_name == "both_true")
        .unwrap();
    assert_eq!(both_true.result.as_ref().unwrap().to_string(), "true");

    let mixed = response
        .results
        .iter()
        .find(|r| r.rule_name == "mixed")
        .unwrap();
    assert_eq!(mixed.result.as_ref().unwrap().to_string(), "false");
}

#[test]
fn test_equal_operator_in_conditions() {
    let mut engine = Engine::new();
    engine
        .add_lemma_code(
            r#"
doc test_equal_conditions

fact status = "active"
fact count = 10

rule message = "inactive"
  unless status == "active" then "active"
  unless count == 10 then "count is 10"
"#,
            "test.lemma",
        )
        .unwrap();

    let response = engine.evaluate("test_equal_conditions", vec![]).unwrap();

    let message = response
        .results
        .iter()
        .find(|r| r.rule_name == "message")
        .unwrap();
    assert_eq!(
        message.result.as_ref().unwrap().to_string(),
        "\"count is 10\""
    );
}
