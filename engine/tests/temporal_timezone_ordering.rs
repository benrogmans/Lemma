//! Adversarial: `DateTimeValue` uses derived `Ord` (lexicographic, including timezone),
//! not normalized UTC instants. These tests document resolution behavior when two
//! `effective_from` keys differ only in offset.

use lemma::parsing::ast::TimezoneValue;
use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;

fn utc_noon() -> DateTimeValue {
    DateTimeValue {
        year: 2026,
        month: 1,
        day: 1,
        hour: 12,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: 0,
            offset_minutes: 0,
        }),
    }
}

fn assert_rule_value(response: &lemma::Response, rule: &str, expected: &str) {
    let result = response.results.get(rule).expect("rule");
    let val = result.result.value().expect("value");
    assert_eq!(val.to_string(), expected);
}

/// Same civil date/time, different offsets — distinct `effective_from` keys.
/// `spec_at` picks latest key <= query instant in `EffectiveDate` order (lexicographic).
#[test]
fn two_offsets_same_civil_time_distinct_versions() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec rate 2026-01-01T12:00:00+00:00
data v: 1
rule out: v

spec rate 2026-01-01T12:00:00+05:00
data v: 2
rule out: v
"#,
            SourceType::Labeled("tz.lemma"),
        )
        .expect("parse specs with offset on effective_from");

    let r = engine
        .run("rate", Some(&utc_noon()), HashMap::new(), false)
        .expect("run");
    assert_rule_value(&r, "out", "1");
}

#[test]
fn later_lexicographic_effective_from_wins_at_shared_instant_query() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec rate 2026-01-01T12:00:00+00:00
rule out: 10

spec rate 2026-01-01T12:00:00+05:00
rule out: 20
"#,
            SourceType::Labeled("tz2.lemma"),
        )
        .unwrap();

    let r = engine
        .run("rate", Some(&utc_noon()), HashMap::new(), false)
        .unwrap();
    assert_rule_value(&r, "out", "10");
}
