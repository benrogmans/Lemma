use lemma::Engine;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("test.lemma")))
}

fn load_ok(code: impl AsRef<str>) -> Engine {
    let code = code.as_ref();
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    engine
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

fn eval_rule_quantity_unit(
    code: impl AsRef<str>,
    spec_name: &str,
    rule_name: &str,
    unit: &str,
) -> String {
    let code = code.as_ref();
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let now = lemma::DateTimeValue::now();
    let response = engine
        .run(None, spec_name, Some(&now), HashMap::new(), true)
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

fn eval_rule(code: impl AsRef<str>, spec_name: &str, rule_name: &str) -> String {
    let code = code.as_ref();
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let now = lemma::DateTimeValue::now();
    let response = engine
        .run(
            None,
            spec_name,
            Some(&now),
            std::collections::HashMap::new(),
            false,
        )
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .display
        .clone()
        .expect("display")
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
fn planning_local_duration_typedef_accepts_singular_and_plural_literals() {
    let code = r#"spec test
uses lemma units
rule value: (2 hours + 30 minute) as minutes"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["150", "minute"]);
}

#[test]
fn planning_local_duration_typedef_accepts_plural_to_singular_conversion() {
    let code = r#"spec test
uses lemma units
rule value: 1 hours as hour"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["1"]);
}

#[test]
fn planning_imported_duration_typedef_exposes_units() {
    let code = r#"spec base_types
uses lemma units
data duration: units.duration

spec test
uses base_types
uses lemma units
data duration: base_types.duration
rule value: 90 minutes as hours as number"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["1.5"]);
}

#[test]
fn planning_duration_name_is_ordinary_user_type_name_after_keyword_removal() {
    let code = r#"spec test
uses lemma units
data duration: units.duration
data elapsed: duration -> default 2 hours
rule value: elapsed as minutes"#;
    let _engine = load_ok(code);
    assert_eq!(
        eval_rule_quantity_unit(code, "test", "value", "minutes"),
        "120"
    );
}

#[test]
fn planning_duration_trait_allows_extra_custom_units() {
    let code = r#"spec test
uses lemma units
data duration: units.duration
data travel_duration: duration
  -> unit fortnight 1209600
data trip: 1 fortnight
rule value: trip as days as number"#;
    let value = eval_rule(code, "test", "value");
    assert_contains_all(&value, &["14"]);
}

#[test]
fn planning_bare_duration_literal_without_visible_typedef_rejected() {
    let code = r#"spec test
rule value: 2 hours"#;
    expect_plan_error(code, "hours");
}

#[test]
fn planning_duration_parent_type_without_visible_typedef_rejected() {
    let code = r#"spec test
data elapsed: duration"#;
    expect_plan_error(code, "duration");
}

#[test]
fn planning_trait_duration_requires_second_factor_one() {
    let code = r#"spec test
data duration: quantity
  -> unit second 2
  -> unit hour 3600
  -> trait duration"#;
    expect_plan_error(code, "second 1");
}

#[test]
fn planning_trait_duration_requires_second_unit() {
    let code = r#"spec test
data duration: quantity
  -> unit hour 3600
  -> trait duration"#;
    expect_plan_error(code, "second");
}

#[test]
fn planning_duplicate_trait_duration_rejected() {
    let code = r#"spec test
data duration: quantity
  -> unit second 1
  -> trait duration
  -> trait duration"#;
    expect_plan_error(code, "duplicate");
}

#[test]
fn planning_unknown_trait_rejected() {
    let code = r#"spec test
data duration: quantity
  -> unit second 1
  -> trait temporal"#;
    expect_plan_error(code, "trait");
}

#[test]
fn planning_trait_duration_on_non_quantity_rejected() {
    let code = r#"spec test
data x: number
  -> trait duration"#;
    expect_plan_error(code, "quantity");
}
