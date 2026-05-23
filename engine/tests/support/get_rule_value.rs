use lemma::parsing::ast::DateTimeValue;
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
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run");
    response
        .get(rule_name)
        .unwrap_or_else(|_| panic!("rule {rule_name}"))
        .result
        .value()
        .expect("value")
        .clone()
}
