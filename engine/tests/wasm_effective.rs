//! WASM `run` / `show` must reject invalid `effective` strings (same contract as HTTP/MCP).

use lemma::{DateTimeValue, Error};

#[test]
fn invalid_effective_string_must_error_not_default_to_now() {
    assert!(
        lemma::resolve_effective(Some("not-a-datetime")).is_err(),
        "invalid effective must return error before planning, not fall back to now"
    );
}

#[test]
fn absent_effective_resolves_to_now() {
    let before = DateTimeValue::now();
    let resolved = lemma::resolve_effective(None).expect("absent effective must succeed");
    let after = DateTimeValue::now();
    assert!(
        resolved >= before && resolved <= after,
        "absent effective must resolve to current time"
    );
}

#[test]
fn whitespace_effective_resolves_to_now() {
    let before = DateTimeValue::now();
    let resolved =
        lemma::resolve_effective(Some("   ")).expect("whitespace effective must succeed");
    let after = DateTimeValue::now();
    assert!(
        resolved >= before && resolved <= after,
        "whitespace effective must resolve to current time"
    );
}

#[test]
fn valid_effective_string_parses() {
    let parsed = lemma::resolve_effective(Some("2020-01-01")).expect("valid effective must parse");
    assert_eq!(parsed.to_string(), "2020-01-01");
}

#[test]
fn invalid_effective_is_request_error() {
    let err = lemma::resolve_effective(Some("garbage"))
        .expect_err("invalid effective must be request error");
    assert!(matches!(err, Error::Request { .. }));
}
