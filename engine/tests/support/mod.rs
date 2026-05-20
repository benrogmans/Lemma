//! Shared helpers for engine integration tests (import with `mod support;`).

#![allow(dead_code)]

use lemma::parsing::ast::{DateTimeValue, TimezoneValue};
use lemma::{Engine, LiteralValue, OperationResult};
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

pub fn make_effective(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTimeValue {
    DateTimeValue {
        year: y,
        month: m,
        day: d,
        hour: h,
        minute: min,
        second: s,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: 0,
            offset_minutes: 0,
        }),
    }
}

pub fn make_effective_tz(
    (y, m, d, h, min, s): (i32, u32, u32, u32, u32, u32),
    (tz_h, tz_m): (i8, u8),
) -> DateTimeValue {
    DateTimeValue {
        year: y,
        month: m,
        day: d,
        hour: h,
        minute: min,
        second: s,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: tz_h,
            offset_minutes: tz_m,
        }),
    }
}
