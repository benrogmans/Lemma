use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, OperationResult};
use std::collections::HashMap;

pub fn eval_rule_bool(
    engine: &Engine,
    spec_name: &str,
    rule: &str,
    effective: &DateTimeValue,
    data: HashMap<String, String>,
) -> bool {
    let response = engine
        .run(
            None,
            spec_name,
            Some(effective),
            data,
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run");
    let rule_result = response.get(rule).unwrap_or_else(|_| panic!("rule {rule}"));
    match &rule_result.result {
        OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Boolean(b) => *b,
            other => panic!("expected boolean, got {other:?}"),
        },
        OperationResult::Veto(v) => panic!("rule {rule} vetoed: {v:?}"),
    }
}
