use lemma::DateTimeValue;
use lemma::{Engine, LiteralValue, OperationResult};
use std::collections::HashMap;

pub fn get_rule_value(engine: &Engine, spec_name: &str, rule_name: &str) -> LiteralValue {
    let now = DateTimeValue::now();
    let response = engine
        .run(None, spec_name, Some(&now), HashMap::new(), true)
        .expect("run");
    let rule_result = response
        .get(rule_name)
        .unwrap_or_else(|_| panic!("rule {rule_name}"));
    let trace = rule_result
        .trace
        .as_ref()
        .expect("BUG: get_rule_value requires evaluation_trace on rule result");
    match &trace.result {
        OperationResult::Value(literal) => literal.as_ref().clone(),
        OperationResult::Veto(veto) => panic!("rule {rule_name} vetoed: {veto}"),
    }
}
