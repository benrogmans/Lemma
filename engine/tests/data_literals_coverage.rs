//! QA coverage for `DataValue::Literal(Value)` — inline literal bindings.
//!
//! One test per primitive literal kind + edge cases. Assertions pin exact
//! values. Tests that encode invariants the implementation may not satisfy
//! are EXPECTED TO FAIL and must remain red. Do not weaken.

use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

fn load_ok(engine: &mut Engine, code: &str) {
    engine
        .load(code, lemma::SourceType::Labeled("literals.lemma"))
        .unwrap_or_else(|errs| {
            let joined = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            panic!("expected load to succeed, got: {joined}");
        });
}

fn load_err_joined(engine: &mut Engine, code: &str) -> String {
    let err = engine
        .load(code, lemma::SourceType::Labeled("literals.lemma"))
        .expect_err("expected load to fail");
    err.iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn rule_value(result: &lemma::evaluation::Response, rule_name: &str) -> String {
    let rr = result
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("rule '{}' not found", rule_name));
    match &rr.result {
        OperationResult::Value(v) => v.to_string(),
        OperationResult::Veto(v) => format!("VETO({})", v),
    }
}

fn run(engine: &Engine, spec: &str) -> lemma::evaluation::Response {
    let now = DateTimeValue::now();
    engine
        .run(spec, Some(&now), HashMap::new(), false)
        .expect("run")
}

// ─── Number literals ──────────────────────────────────────────────────

#[test]
fn number_literal_integer() {
    let code = r#"
spec s
data n: 42
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "42");
}

#[test]
fn number_literal_decimal() {
    let code = r#"
spec s
data n: 3.14
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "3.14");
}

#[test]
fn number_literal_zero_normalizes() {
    let code = r#"
spec s
data n: 0.0
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "0");
}

#[test]
fn number_literal_negative_via_unary_minus() {
    let code = r#"
spec s
data n: -5
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "-5");
}

#[test]
fn number_literal_explicit_positive_via_unary_plus() {
    let code = r#"
spec s
data n: +7
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "7");
}

#[test]
fn number_literal_very_long_decimal_preserves_precision() {
    let code = r#"
spec s
data n: 1.234567890123456789
rule r: n
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    // Pin the exact display. If precision is silently truncated the assertion fails.
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "1.234567890123456789");
}

// ─── Text literals ────────────────────────────────────────────────────

#[test]
fn text_literal_basic() {
    let code = r#"
spec s
data msg: "hello"
rule r: msg
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "hello");
}

#[test]
fn text_literal_empty() {
    let code = "
spec s
data msg: \"\"
rule r: msg
";
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "");
}

#[test]
fn text_literal_unicode() {
    let code = "
spec s
data msg: \"日本語 café\"
rule r: msg
";
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "日本語 café");
}

// ─── Boolean literals ─────────────────────────────────────────────────

#[test]
fn boolean_literal_true() {
    let code = r#"
spec s
data b: true
rule r: b
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "true");
}

#[test]
fn boolean_literal_false() {
    let code = r#"
spec s
data b: false
rule r: b
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    assert_eq!(rule_value(&run(&engine, "s"), "r"), "false");
}

#[test]
fn boolean_literal_yes() {
    let code = r#"
spec s
data b: yes
rule r: b
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(
        out == "true" || out == "yes",
        "boolean 'yes' must render consistently, got: {out}"
    );
}

#[test]
fn boolean_literal_no() {
    let code = r#"
spec s
data b: no
rule r: b
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(
        out == "false" || out == "no",
        "boolean 'no' must render consistently, got: {out}"
    );
}

// ─── Date / Time literals ─────────────────────────────────────────────

#[test]
fn date_literal_ymd() {
    let code = r#"
spec s
data d: 2024-01-15
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(
        out.starts_with("2024-01-15"),
        "date must round-trip starting with 2024-01-15, got: {out}"
    );
}

#[test]
fn date_literal_invalid_month_rejected() {
    // February 30 does not exist. Must be rejected at parse/plan time, NOT
    // silently clamped or rolled forward.
    let code = r#"
spec s
data d: 2024-02-30
rule r: d
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty(),
        "invalid date 2024-02-30 must produce a parse/plan error; engine silently accepted it"
    );
}

#[test]
fn date_literal_zero_day_rejected() {
    let code = r#"
spec s
data d: 2024-05-00
rule r: d
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty(),
        "invalid date 2024-05-00 must produce an error"
    );
}

#[test]
fn time_literal_hh_mm() {
    let code = r#"
spec s
data t: 14:30
rule r: t
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(
        out.starts_with("14:30"),
        "time must render starting with 14:30, got: {out}"
    );
}

#[test]
fn time_literal_hh_mm_ss() {
    let code = r#"
spec s
data t: 14:30:45
rule r: t
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(
        out.starts_with("14:30:45"),
        "time must render starting with 14:30:45, got: {out}"
    );
}

#[test]
fn time_literal_invalid_hour_rejected() {
    let code = r#"
spec s
data t: 25:00
rule r: t
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        !joined.is_empty(),
        "invalid time 25:00 must produce an error; engine silently accepted it"
    );
}

// ─── Duration literals ────────────────────────────────────────────────

#[test]
fn duration_literal_years_plural() {
    let code = r#"
spec s
data d: 5 years
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("5") && out.contains("year"), "got: {out}");
}

#[test]
fn duration_literal_year_singular() {
    let code = r#"
spec s
data d: 1 year
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("1") && out.contains("year"), "got: {out}");
}

#[test]
fn duration_literal_months() {
    let code = r#"
spec s
data d: 3 months
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("3") && out.contains("month"), "got: {out}");
}

#[test]
fn duration_literal_weeks() {
    let code = r#"
spec s
data d: 2 weeks
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("2") && out.contains("week"), "got: {out}");
}

#[test]
fn duration_literal_days() {
    let code = r#"
spec s
data d: 7 days
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("7") && out.contains("day"), "got: {out}");
}

#[test]
fn duration_literal_hours() {
    let code = r#"
spec s
data d: 12 hours
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("12") && out.contains("hour"), "got: {out}");
}

#[test]
fn duration_literal_minutes() {
    let code = r#"
spec s
data d: 90 minutes
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("90") && out.contains("minute"), "got: {out}");
}

#[test]
fn duration_literal_seconds() {
    let code = r#"
spec s
data d: 45 seconds
rule r: d
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("45") && out.contains("second"), "got: {out}");
}

#[test]
fn duration_literal_negative_rejected_or_supported_consistently() {
    // Negative durations are a semantic edge. Pin: either the parser
    // accepts and stores -5, or rejects. Silent coercion to 0 or +5 is a bug.
    let code = r#"
spec s
data d: -5 days
rule r: d
"#;
    let mut engine = Engine::new();
    match engine.load(code, lemma::SourceType::Labeled("literals.lemma")) {
        Ok(()) => {
            let out = rule_value(&run(&engine, "s"), "r");
            assert!(
                out.contains("-5") && out.contains("day"),
                "if -5 days is accepted, it must preserve the sign; got: {out}"
            );
        }
        Err(errs) => {
            let joined = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                !joined.is_empty(),
                "rejection must carry a message, not empty errors"
            );
        }
    }
}

// ─── Ratio literals ───────────────────────────────────────────────────

/// Pin a rule's value as `ValueKind::Ratio(decimal, optional_unit_name)`. Panics
/// on any other shape so a regression in either the numeric magnitude or the
/// unit name fails loudly (substring matches on `Display` cannot — a 100x off
/// value renders as `"5000%"` which still contains `"50"` and `"%"`).
fn rule_ratio(
    result: &lemma::evaluation::Response,
    rule_name: &str,
) -> (rust_decimal::Decimal, Option<String>) {
    use lemma::ValueKind;
    let rr = result
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("rule '{}' not found", rule_name));
    let lit = match &rr.result {
        OperationResult::Value(v) => v.as_ref(),
        OperationResult::Veto(v) => panic!("rule '{}' produced veto: {}", rule_name, v),
    };
    match &lit.value {
        ValueKind::Ratio(n, u) => (*n, u.clone()),
        other => panic!("rule '{}' produced non-Ratio value {:?}", rule_name, other),
    }
}

#[test]
fn ratio_literal_percent_sign() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let code = r#"
spec s
data r: 50%
rule out: r
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s");
    let (value, unit) = rule_ratio(&resp, "out");
    assert_eq!(value, Decimal::from_str("0.50").unwrap());
    assert_eq!(unit.as_deref(), Some("percent"));
    assert_eq!(rule_value(&resp, "out"), "50%");
}

#[test]
fn ratio_literal_permille_sign() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let code = r#"
spec s
data r: 25%%
rule out: r
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s");
    let (value, unit) = rule_ratio(&resp, "out");
    assert_eq!(value, Decimal::from_str("0.025").unwrap());
    assert_eq!(unit.as_deref(), Some("permille"));
    assert_eq!(rule_value(&resp, "out"), "25%%");
}

#[test]
fn ratio_literal_percent_keyword_matches_sigil() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let code = r#"
spec s
data r: 50 percent
rule out: r
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s");
    let (value, unit) = rule_ratio(&resp, "out");
    assert_eq!(value, Decimal::from_str("0.50").unwrap());
    assert_eq!(unit.as_deref(), Some("percent"));
    assert_eq!(rule_value(&resp, "out"), "50%");
}

#[test]
fn ratio_literal_permille_keyword_matches_sigil() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let code = r#"
spec s
data r: 25 permille
rule out: r
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s");
    let (value, unit) = rule_ratio(&resp, "out");
    assert_eq!(value, Decimal::from_str("0.025").unwrap());
    assert_eq!(unit.as_deref(), Some("permille"));
    assert_eq!(rule_value(&resp, "out"), "25%%");
}

#[test]
fn ratio_literal_negative_percent_sign() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let code = r#"
spec s
data r: -50%
rule out: r
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s");
    let (value, unit) = rule_ratio(&resp, "out");
    assert_eq!(value, Decimal::from_str("-0.50").unwrap());
    assert_eq!(unit.as_deref(), Some("percent"));
    assert_eq!(rule_value(&resp, "out"), "-50%");
}

#[test]
fn ratio_literal_bare_number_has_no_unit() {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let code = r#"
spec s
data r: 0.25
rule out: r
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let resp = run(&engine, "s");
    let rr = resp.results.get("out").expect("rule 'out' not found");
    let lit = match &rr.result {
        OperationResult::Value(v) => v.as_ref(),
        OperationResult::Veto(v) => panic!("rule 'out' produced veto: {}", v),
    };
    use lemma::ValueKind;
    match &lit.value {
        ValueKind::Number(n) => {
            assert_eq!(*n, Decimal::from_str("0.25").unwrap());
        }
        ValueKind::Ratio(n, u) => {
            assert_eq!(*n, Decimal::from_str("0.25").unwrap());
            assert_eq!(u.as_deref(), None);
        }
        other => panic!("expected Number or Ratio, got: {:?}", other),
    }
}

// ─── Scale literals (require user-defined unit) ───────────────────────

#[test]
fn scale_literal_with_defined_unit() {
    let code = r#"
spec s
data money: scale
  -> unit eur 1
  -> unit usd 1.19
data price: 10 eur
rule r: price
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("10") && out.contains("eur"), "got: {out}");
}

#[test]
fn scale_literal_with_unknown_unit_is_rejected() {
    // No scale type defines `banana` as a unit; the literal must fail.
    let code = r#"
spec s
data money: scale -> unit eur 1
data price: 10 banana
rule r: price
"#;
    let mut engine = Engine::new();
    let joined = load_err_joined(&mut engine, code);
    assert!(
        joined.contains("unknown unit")
            || joined.contains("Unknown unit")
            || joined.contains("'banana'"),
        "expected unknown-unit error, got: {joined}"
    );
}

#[test]
fn scale_literal_conversion_to_defined_unit() {
    let code = r#"
spec s
data money: scale
  -> unit eur 1
  -> unit usd 1.19
data price: 10 usd
rule r: price
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code);
    let out = rule_value(&run(&engine, "s"), "r");
    assert!(out.contains("10") && out.contains("usd"), "got: {out}");
}
