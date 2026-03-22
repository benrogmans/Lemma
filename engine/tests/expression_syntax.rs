use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use lemma::LiteralValue;
use lemma::ValueKind;
use std::collections::HashMap;

#[test]
fn parentheses_syntax_evaluates_correctly() {
    // Integration test: parentheses syntax is accepted by parser and behaves correctly in evaluation.
    let code = r#"
spec test
fact x: true
fact y: false
fact num: 16
rule not_x: not(x)
rule sqrt_num: sqrt(num)
rule sin_zero: sin(0)
rule log_ten: log(10)
rule combined: not(x) and sqrt(16) == 4
rule with_spaces: not  (  x  )
"#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();

    // not(x) evaluates to false (since x = true)
    let not_x_rule = response.results.get("not_x").unwrap();
    match not_x_rule.result.value().unwrap() {
        LiteralValue {
            value: ValueKind::Boolean(b),
            ..
        } => assert!(!*b, "not(x) with x=true should be false"),
        v => panic!("Expected boolean false, got {:?}", v),
    }

    // sqrt(16) evaluates to 4
    let sqrt_rule = response.results.get("sqrt_num").unwrap();
    match sqrt_rule.result.value().unwrap() {
        LiteralValue {
            value: ValueKind::Number(n),
            ..
        } => assert_eq!(n.to_string(), "4"),
        v => panic!("Expected number 4, got {:?}", v),
    }

    // sin(0) evaluates to 0
    let sin_rule = response.results.get("sin_zero").unwrap();
    match sin_rule.result.value().unwrap() {
        LiteralValue {
            value: ValueKind::Number(n),
            ..
        } => assert_eq!(n.to_string(), "0"),
        v => panic!("Expected number 0, got {:?}", v),
    }

    // combined expression: not(true) and (sqrt(16) == 4) => false and true => false
    let combined_rule = response.results.get("combined").unwrap();
    match combined_rule.result.value().unwrap() {
        LiteralValue {
            value: ValueKind::Boolean(b),
            ..
        } => assert!(!*b, "not(x) and sqrt(16) == 4 with x=true should be false"),
        v => panic!("Expected boolean false, got {:?}", v),
    }
}
