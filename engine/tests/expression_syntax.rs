use lemma::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

#[test]
fn parentheses_syntax_evaluates_correctly() {
    // Integration test: parentheses syntax is accepted by parser and behaves correctly in evaluation.
    let code = r#"
spec test
data x: true
data y: false
data num: 16
rule not_x: not(x)
rule sqrt_num: sqrt(num)
rule sin_zero: sin(0)
rule log_ten: log(10)
rule combined: not(x) and sqrt(16) is 4
rule with_spaces: not  (  x  )
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test", Some(&now), HashMap::new(), false, None)
        .unwrap();

    let not_x_rule = response.results.get("not_x").unwrap();
    assert_eq!(not_x_rule.boolean, Some(false));

    let sqrt_rule = response.results.get("sqrt_num").unwrap();
    assert_eq!(sqrt_rule.display.as_deref(), Some("4"));

    let sin_rule = response.results.get("sin_zero").unwrap();
    assert_eq!(sin_rule.display.as_deref(), Some("0"));

    let combined_rule = response.results.get("combined").unwrap();
    assert_eq!(combined_rule.boolean, Some(false));
}
