use lemma::parsing::ast::DateTimeValue;
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

fn eval_literal(code: impl AsRef<str>, spec_name: &str, rule_name: &str) -> lemma::LiteralValue {
    let code = code.as_ref();
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
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
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .result
        .value()
        .unwrap_or_else(|| panic!("Rule '{}' returned non-value", rule_name))
        .clone()
}

fn eval_rule(code: impl AsRef<str>, spec_name: &str, rule_name: &str) -> String {
    eval_literal(code, spec_name, rule_name).to_string()
}

fn eval_bool(code: impl AsRef<str>, spec_name: &str, rule_name: &str) -> bool {
    match eval_literal(code, spec_name, rule_name).value {
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
fn arith_add_mixed_aliases() {
    let code = r#"spec test
uses lemma si
rule value: (2 hours + 30 minutes) as minutes"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["150", "minute"]);
}

#[test]
fn arith_subtract_mixed_aliases() {
    let code = r#"spec test
uses lemma si
rule value: (2 hour - 30 minutes) as minute"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["90", "minute"]);
}

#[test]
fn arith_negative_duration_result() {
    let code = r#"spec test
uses lemma si
rule value: (30 minutes - 2 hours) as minutes"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["-90", "minute"]);
}

#[test]
fn arith_duration_times_number() {
    let code = r#"spec test
uses lemma si
rule value: (2 hours * 3) as hours"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["6", "hour"]);
}

#[test]
fn arith_number_times_duration() {
    let code = r#"spec test
uses lemma si
rule value: (3 * 2 hours) as hours"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["6", "hour"]);
}

#[test]
fn arith_duration_divided_by_number() {
    let code = r#"spec test
uses lemma si
rule value: (2 hours / 2) as minutes"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["60", "minute"]);
}

#[test]
fn arith_duration_divided_by_duration_yields_number() {
    let code = r#"spec test
uses lemma si
rule value: 2 hours / 30 minutes"#;
    assert_eq!(eval_rule(code, "test", "value"), "4");
}

#[test]
fn arith_named_duration_compare_same_alias_family() {
    let code = r#"spec test
uses lemma si
rule ok: 2 hours > 90 minutes"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn arith_named_duration_compare_equal_after_conversion() {
    let code = r#"spec test
uses lemma si
rule ok: 2 hours >= 120 minutes"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn arith_number_cast_to_duration_for_comparison() {
    let code = r#"spec test
uses lemma si
data threshold: 1.5
rule ok: 2 hours > threshold as hours"#;
    assert!(eval_bool(code, "test", "ok"));
}

#[test]
fn arith_named_duration_compare_to_bare_number_rejected() {
    let code = r#"spec test
uses lemma si
rule ok: 2 hours > 5"#;
    expect_plan_error(code, "number");
}

#[test]
fn arith_named_duration_plus_calendar_rejected() {
    let code = r#"spec test
uses lemma si
rule value: 2 hours + 3 months"#;
    expect_plan_error(code, "calendar");
}

#[test]
fn arith_named_duration_plus_unrelated_quantity_rejected() {
    let code = format!(
        r#"spec test
uses lemma si
{money}
rule value: 2 hours + 5 eur"#,
        money = MONEY_TYPEDEF
    );
    expect_plan_error(code, "unrelated");
}

#[test]
fn arith_named_duration_cast_to_unrelated_unit_rejected() {
    let code = format!(
        r#"spec test
uses lemma si
{money}
rule value: 2 hours as eur"#,
        money = MONEY_TYPEDEF
    );
    expect_plan_error(code, "convert");
}

#[test]
fn arith_bare_number_not_implicitly_duration_compatible() {
    let code = r#"spec test
uses lemma si
data threshold: 1.5
rule ok: 2 hours > threshold"#;
    expect_plan_error(code, "number");
}
