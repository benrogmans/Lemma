//! Strict runtime-input grammar for Ratio-typed data overrides.
//!
//! Covers `engine.run(spec, ..., data: HashMap<String, String>, ...)` where each
//! string flows through `parse_value_from_string` → `parse_number_unit::Ratio` →
//! `RatioLiteral::parse`. Pins exact `ValueKind::Ratio(decimal, optional_unit)`,
//! not substrings of `Display`, so a 100x off value cannot pass.
//!
//! Also includes a Scale-side regression: `"5%"` against a Scale type must error
//! with the friendly Scale message (no leftover `%`/`%%` handling in the Scale
//! literal parser).

use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use lemma::ValueKind;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn load(engine: &mut Engine, code: &str) {
    engine
        .load(code, lemma::SourceType::Labeled("ratio_in.lemma"))
        .unwrap_or_else(|errs| {
            let joined = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            panic!("expected load to succeed, got: {joined}");
        });
}

fn run_ratio(engine: &Engine, spec: &str, raw: &str) -> (Decimal, Option<String>) {
    let mut data = HashMap::new();
    data.insert("r".to_string(), raw.to_string());
    let now = DateTimeValue::now();
    let resp = engine
        .run(spec, Some(&now), data, false)
        .unwrap_or_else(|e| panic!("run failed for input '{raw}': {e}"));
    let rr = resp
        .results
        .get("out")
        .unwrap_or_else(|| panic!("rule 'out' not found"));
    let lit = match &rr.result {
        OperationResult::Value(v) => v.as_ref(),
        OperationResult::Veto(v) => panic!("input '{raw}' produced veto: {v}"),
    };
    match &lit.value {
        ValueKind::Ratio(n, u) => (*n, u.clone()),
        other => panic!("input '{raw}' produced non-Ratio: {:?}", other),
    }
}

fn run_err(engine: &Engine, spec: &str, raw: &str) -> String {
    let mut data = HashMap::new();
    data.insert("r".to_string(), raw.to_string());
    let now = DateTimeValue::now();
    engine
        .run(spec, Some(&now), data, false)
        .err()
        .unwrap_or_else(|| panic!("expected '{raw}' to be rejected, but run succeeded"))
        .to_string()
}

fn percent_spec() -> &'static str {
    r#"
spec s
data r: percent
rule out: r
"#
}

// ─── Accepted: bare number, no unit ───────────────────────────────────

#[test]
fn accepts_bare_zero() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "0");
    assert_eq!(n, Decimal::from(0));
    assert_eq!(u, None);
}

#[test]
fn accepts_bare_decimal() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "0.5");
    assert_eq!(n, Decimal::from_str("0.5").unwrap());
    assert_eq!(u, None);
}

#[test]
fn accepts_bare_negative() {
    // Round-trip: the evaluator can produce negative ratios from valid inputs;
    // the parser must accept anything the evaluator can emit.
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "-0.25");
    assert_eq!(n, Decimal::from_str("-0.25").unwrap());
    assert_eq!(u, None);
}

// ─── Accepted: percent sigil ──────────────────────────────────────────

#[test]
fn accepts_percent_sigil_integer() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "50%");
    assert_eq!(n, Decimal::from_str("0.50").unwrap());
    assert_eq!(u.as_deref(), Some("percent"));
}

#[test]
fn accepts_percent_sigil_decimal() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "50.5%");
    assert_eq!(n, Decimal::from_str("0.505").unwrap());
    assert_eq!(u.as_deref(), Some("percent"));
}

#[test]
fn accepts_percent_sigil_negative() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "-50%");
    assert_eq!(n, Decimal::from_str("-0.5").unwrap());
    assert_eq!(u.as_deref(), Some("percent"));
}

#[test]
fn accepts_percent_sigil_with_thousands_separator() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "5,000%");
    assert_eq!(n, Decimal::from_str("50").unwrap());
    assert_eq!(u.as_deref(), Some("percent"));
}

// ─── Accepted: permille sigil ─────────────────────────────────────────

#[test]
fn accepts_permille_sigil() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "25%%");
    assert_eq!(n, Decimal::from_str("0.025").unwrap());
    assert_eq!(u.as_deref(), Some("permille"));
}

#[test]
fn accepts_permille_sigil_negative() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "-25%%");
    assert_eq!(n, Decimal::from_str("-0.025").unwrap());
    assert_eq!(u.as_deref(), Some("permille"));
}

// ─── Accepted: keyword forms (single and multi-space) ─────────────────

#[test]
fn accepts_percent_keyword_single_space() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "50 percent");
    assert_eq!(n, Decimal::from_str("0.50").unwrap());
    assert_eq!(u.as_deref(), Some("percent"));
}

#[test]
fn accepts_percent_keyword_multi_space() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "50    percent");
    assert_eq!(n, Decimal::from_str("0.50").unwrap());
    assert_eq!(u.as_deref(), Some("percent"));
}

#[test]
fn accepts_percent_keyword_tab() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "50\tpercent");
    assert_eq!(n, Decimal::from_str("0.50").unwrap());
    assert_eq!(u.as_deref(), Some("percent"));
}

#[test]
fn accepts_permille_keyword() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let (n, u) = run_ratio(&engine, "s", "25 permille");
    assert_eq!(n, Decimal::from_str("0.025").unwrap());
    assert_eq!(u.as_deref(), Some("permille"));
}

// ─── Accepted: user-defined ratio unit ────────────────────────────────

#[test]
fn accepts_user_defined_ratio_unit() {
    let code = r#"
spec s
data r: ratio -> unit basis_points 10000
rule out: r
"#;
    let mut engine = Engine::new();
    load(&mut engine, code);
    let (n, u) = run_ratio(&engine, "s", "500 basis_points");
    assert_eq!(n, Decimal::from_str("0.05").unwrap());
    assert_eq!(u.as_deref(), Some("basis_points"));
}

// ─── Cross-form equivalence ───────────────────────────────────────────

#[test]
fn sigil_and_keyword_produce_same_value() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let sigil = run_ratio(&engine, "s", "50%");
    let keyword = run_ratio(&engine, "s", "50 percent");
    assert_eq!(sigil, keyword);
}

#[test]
fn permille_sigil_and_keyword_produce_same_value() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let sigil = run_ratio(&engine, "s", "25%%");
    let keyword = run_ratio(&engine, "s", "25 permille");
    assert_eq!(sigil, keyword);
}

// ─── Rejected: empty / whitespace ─────────────────────────────────────

#[test]
fn rejects_empty() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let msg = run_err(&engine, "s", "");
    assert!(
        msg.to_lowercase().contains("empty") || msg.to_lowercase().contains("ratio"),
        "expected empty/ratio message, got: {msg}"
    );
}

#[test]
fn rejects_whitespace_only() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let _msg = run_err(&engine, "s", "   ");
}

// ─── Rejected: sigil without number ───────────────────────────────────

#[test]
fn rejects_bare_percent_sigil() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let _msg = run_err(&engine, "s", "%");
}

#[test]
fn rejects_bare_permille_sigil() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let _msg = run_err(&engine, "s", "%%");
}

#[test]
fn rejects_sigil_before_number() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let _msg = run_err(&engine, "s", "%5");
    let _msg = run_err(&engine, "s", "%%5");
}

// ─── Rejected: sigil with separator (strict glue rule) ────────────────

#[test]
fn rejects_percent_sigil_with_space() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let msg = run_err(&engine, "s", "5 %");
    assert!(
        msg.contains("glued") || msg.contains("'%'"),
        "expected explicit glue-rule error, got: {msg}"
    );
}

#[test]
fn rejects_permille_sigil_with_space() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let msg = run_err(&engine, "s", "5  %%");
    assert!(
        msg.contains("glued") || msg.contains("'%%'"),
        "expected explicit glue-rule error, got: {msg}"
    );
}

// ─── Rejected: digit after sigil / mixed sigil-keyword ────────────────

#[test]
fn rejects_digit_after_percent_sigil() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let _msg = run_err(&engine, "s", "5%5");
}

#[test]
fn rejects_digit_after_permille_sigil() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let _msg = run_err(&engine, "s", "5%%5");
}

#[test]
fn rejects_sigil_glued_to_keyword() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let _msg = run_err(&engine, "s", "5%percent");
}

// ─── Rejected: trailing junk ──────────────────────────────────────────

#[test]
fn rejects_trailing_token_after_keyword() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let msg = run_err(&engine, "s", "50 percent extra");
    assert!(
        msg.to_lowercase().contains("extra") || msg.to_lowercase().contains("expected"),
        "expected extra-token error, got: {msg}"
    );
}

// ─── Rejected: unknown unit name ──────────────────────────────────────

#[test]
fn rejects_unknown_unit_name() {
    let mut engine = Engine::new();
    load(&mut engine, percent_spec());
    let msg = run_err(&engine, "s", "50 fictional");
    assert!(
        msg.contains("Unknown unit") && msg.contains("Valid units"),
        "expected `Unknown unit … Valid units …` error, got: {msg}"
    );
    assert!(
        msg.contains("fictional"),
        "error must name the offending unit, got: {msg}"
    );
}

// ─── Scale-side regression: `%` is no longer accepted by NumberWithUnit ───

#[test]
fn scale_type_rejects_percent_sigil() {
    let code = r#"
spec s
data r: scale -> unit eur 1
rule out: r
"#;
    let mut engine = Engine::new();
    load(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("r".to_string(), "5%".to_string());
    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("'5%' must not parse as a Scale value");
    let msg = err.to_string();
    assert!(
        msg.contains("Scale value")
            || msg.contains("must include a unit")
            || msg.contains("Invalid scale"),
        "expected friendly Scale-unit error, got: {msg}"
    );
    assert!(
        !msg.contains("Unknown unit 'percent'"),
        "Scale path must not leak a 'percent' unit lookup, got: {msg}"
    );
}

#[test]
fn scale_type_rejects_permille_sigil() {
    let code = r#"
spec s
data r: scale -> unit eur 1
rule out: r
"#;
    let mut engine = Engine::new();
    load(&mut engine, code);
    let mut data = HashMap::new();
    data.insert("r".to_string(), "5%%".to_string());
    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("'5%%' must not parse as a Scale value");
    let msg = err.to_string();
    assert!(
        !msg.contains("Unknown unit 'permille'"),
        "Scale path must not leak a 'permille' unit lookup, got: {msg}"
    );
}
