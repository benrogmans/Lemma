use lemma::{DateTimeValue, TimezoneValue};
use lemma::{Engine, ValueKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("test.lemma")))
}

const MONEY_TYPEDEF: &str = r#"
data money: quantity
  -> unit eur 1
"#;

fn effective(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTimeValue {
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

fn eval_literal(
    code: impl AsRef<str>,
    spec_name: &str,
    rule_name: &str,
    effective: &DateTimeValue,
) -> lemma::LiteralValue {
    let code = code.as_ref();
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let response = engine
        .run(None, spec_name, Some(effective), HashMap::new(), true)
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .unwrap_or_else(|| panic!("Rule '{}' returned non-value", rule_name))
        .clone()
}

fn eval_rule(
    code: impl AsRef<str>,
    spec_name: &str,
    rule_name: &str,
    effective: &DateTimeValue,
) -> String {
    eval_literal(code, spec_name, rule_name, effective).to_string()
}

fn eval_rule_quantity_unit(
    code: impl AsRef<str>,
    spec_name: &str,
    rule_name: &str,
    unit: &str,
    effective: &DateTimeValue,
) -> String {
    let code = code.as_ref();
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let response = engine
        .run(None, spec_name, Some(effective), HashMap::new(), true)
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .quantity
        .as_ref()
        .and_then(|m| m.get(unit))
        .cloned()
        .unwrap_or_else(|| panic!("quantity map missing unit '{unit}'"))
}

fn eval_bool(
    code: impl AsRef<str>,
    spec_name: &str,
    rule_name: &str,
    effective: &DateTimeValue,
) -> bool {
    match eval_literal(code, spec_name, rule_name, effective).value {
        ValueKind::Boolean(value) => value,
        other => panic!("Expected Boolean, got {:?}", other),
    }
}

fn expect_plan_error(code: impl AsRef<str>, expected_fragment: &str) {
    let code = code.as_ref();
    let mut engine = Engine::new();
    let result = engine.load(code, source());
    assert!(result.is_err(), "Expected planning error");
    let combined = result
        .unwrap_err()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        combined
            .to_lowercase()
            .contains(&expected_fragment.to_lowercase()),
        "Expected error containing '{}', got: {}",
        expected_fragment,
        combined
    );
}

fn assert_contains_all(actual: &str, expected_parts: &[&str]) {
    assert!(
        !actual.contains("..."),
        "Expected scalar/date output, got range-like output '{}'",
        actual
    );
    let lower = actual.to_lowercase();
    for part in expected_parts {
        assert!(
            contains_expected_fragment(&lower, &part.to_lowercase()),
            "Expected '{}' to contain '{}'",
            actual,
            part
        );
    }
}

fn contains_expected_fragment(haystack: &str, needle: &str) -> bool {
    if is_numeric_fragment(needle) {
        contains_numeric_fragment(haystack, needle)
    } else {
        haystack.contains(needle)
    }
}

fn contains_numeric_fragment(haystack: &str, needle: &str) -> bool {
    let mut search_from = 0;
    while let Some(relative_index) = haystack[search_from..].find(needle) {
        let index = search_from + relative_index;
        let mut start = index;
        while start > 0 {
            let previous = haystack[..start].chars().next_back().unwrap();
            if !is_numeric_context_character(previous) {
                break;
            }
            start -= previous.len_utf8();
        }

        let mut end = index + needle.len();
        while end < haystack.len() {
            let next = haystack[end..].chars().next().unwrap();
            if !is_numeric_context_character(next) {
                break;
            }
            end += next.len_utf8();
        }

        let candidate = &haystack[start..end];
        if candidate == needle {
            return true;
        }
        if let (Ok(candidate_decimal), Ok(needle_decimal)) = (
            candidate.parse::<rust_decimal::Decimal>(),
            needle.parse::<rust_decimal::Decimal>(),
        ) {
            if candidate_decimal == needle_decimal {
                return true;
            }
        }
        if start == index && end == index + needle.len() {
            return true;
        }
        search_from = index + needle.len();
    }
    false
}

fn is_numeric_fragment(fragment: &str) -> bool {
    let mut has_digit = false;
    for character in fragment.chars() {
        if character.is_ascii_digit() {
            has_digit = true;
            continue;
        }
        if character == '-' || character == '.' {
            continue;
        }
        return false;
    }
    has_digit
}

fn is_numeric_context_character(character: char) -> bool {
    character.is_ascii_digit() || character == '.' || character == '-'
}

#[test]
fn anon_date_range_to_hours() {
    let code = r#"spec test
uses lemma units
rule value: (2024-01-01...2024-01-02) as hours as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["24"]);
}

#[test]
fn anon_date_range_to_minutes() {
    let code = r#"spec test
uses lemma units
rule value: (2024-01-01...2024-01-02) as minutes"#;
    assert_eq!(
        eval_rule_quantity_unit(
            code,
            "test",
            "value",
            "minutes",
            &effective(2026, 3, 8, 12, 0, 0)
        ),
        "1440"
    );
}

#[test]
fn anon_date_range_compare_named_duration() {
    let code = r#"spec test
uses lemma units
rule ok: (2024-01-01...2024-01-02) >= 12 hours"#;
    assert!(eval_bool(
        code,
        "test",
        "ok",
        &effective(2026, 3, 8, 12, 0, 0)
    ));
}

#[test]
fn anon_date_range_compare_named_duration_equal() {
    let code = r#"spec test
uses lemma units
rule ok: (2024-01-01...2024-01-02) >= 1 day"#;
    assert!(eval_bool(
        code,
        "test",
        "ok",
        &effective(2026, 3, 8, 12, 0, 0)
    ));
}

#[test]
fn anon_sum_of_two_date_ranges_to_named_duration() {
    let code = r#"spec test
uses lemma units
rule value: ((2024-01-01...2024-01-02) + (2024-01-02...2024-01-03)) as hours as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["48"]);
}

#[test]
fn anon_date_range_plus_named_duration() {
    let code = r#"spec test
uses lemma units
rule value: ((2024-01-01...2024-01-02) + 2 hours) as hours as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["26"]);
}

#[test]
fn anon_named_duration_plus_date_range() {
    let code = r#"spec test
uses lemma units
rule value: (2 hours + (2024-01-01...2024-01-02)) as hours as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["26"]);
}

#[test]
fn anon_date_range_times_number() {
    let code = r#"spec test
uses lemma units
rule value: (2 * (2024-01-01...2024-01-02 as hours)) as hours as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["48"]);
}

#[test]
fn anon_number_times_date_range() {
    let code = r#"spec test
uses lemma units
rule value: (2 * (2024-01-01...2024-01-02 as hours)) as hours as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["48"]);
}

#[test]
fn anon_datetime_minus_time_to_named_duration() {
    let code = r#"spec test
uses lemma units
rule value: (2024-01-01T03:30:00Z - 01:00:00) as minutes"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["150", "minute"]);
}

#[test]
fn anon_time_minus_time_rejected_with_datetime_range_suggestion() {
    let code = r#"spec test
uses lemma units
rule value: (14:30:00 - 13:00:00) as minutes"#;
    expect_plan_error(code, "datetime range");
}

#[test]
fn anon_reversed_date_range_span_is_absolute_hours() {
    let code = r#"spec test
uses lemma units
rule value: (2024-01-02...2024-01-01) as hours as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["24"]);
}

#[test]
fn anon_explicit_temporal_window_plus_named_duration() {
    let code = r#"spec test
uses lemma units
rule value: ((now - 7 days...now) + 2 hours) as hours as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert_contains_all(&value, &["170"]);
}

#[test]
fn anon_date_range_to_unrelated_unit_rejected() {
    let code = format!(
        r#"spec test
uses lemma units
{money}
rule value: (2024-01-01...2024-01-02) as eur"#,
        money = MONEY_TYPEDEF
    );
    expect_plan_error(code, "convert");
}

#[test]
fn anon_date_range_compare_to_bare_number_rejected() {
    let code = r#"spec test
uses lemma units
rule ok: (2024-01-01...2024-01-02) > 5"#;
    expect_plan_error(code, "number");
}

#[test]
fn date_range_sum_promotes_to_duration_and_strips_to_canonical_seconds() {
    // `(range + range)` produces a duration-decomp quantity. Chained `as seconds as number`
    // yields canonical seconds (2 days = 172800 seconds).
    let code = r#"spec test
uses lemma units
rule value: ((2024-01-01...2024-01-02) + (2024-01-02...2024-01-03)) as seconds as number"#;
    let value = eval_rule(code, "test", "value", &effective(2026, 3, 8, 12, 0, 0));
    assert!(
        value.contains("172800"),
        "expected canonical seconds (172800), got: {}",
        value
    );
}

#[test]
fn anon_result_compare_to_unrelated_quantity_rejected() {
    let code = format!(
        r#"spec test
uses lemma units
{money}
rule ok: ((2024-01-01...2024-01-02) + (2024-01-02...2024-01-03)) > 5 eur"#,
        money = MONEY_TYPEDEF
    );
    expect_plan_error(code, "unrelated");
}

#[test]
fn anon_result_plus_unrelated_quantity_rejected() {
    let code = format!(
        r#"spec test
uses lemma units
{money}
rule value: (2024-01-01...2024-01-02) + 5 eur"#,
        money = MONEY_TYPEDEF
    );
    expect_plan_error(code, "date range");
}
