use lemma::DateTimeValue;
use lemma::{Engine, LiteralValue};
use std::collections::HashMap;

pub fn get_rule_value(engine: &Engine, spec_name: &str, rule_name: &str) -> LiteralValue {
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            spec_name,
            Some(&now),
            HashMap::new(),
            Some(&[rule_name.to_string()]),
            false,
        )
        .expect("run");
    let rule_result = response
        .get(rule_name)
        .unwrap_or_else(|_| panic!("rule {rule_name}"));
    if rule_result.vetoed {
        panic!(
            "rule {rule_name} vetoed: {}",
            rule_result.veto_reason.as_deref().unwrap_or("Vetoed")
        );
    }
    rule_result.to_literal()
}
