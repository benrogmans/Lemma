//! Mirrors fuzz target API usage so that compile-breaking changes
//! to the public API are caught by `cargo nextest run` before the
//! nightly-only fuzz job ever runs.

use lemma::DateTimeValue;
use lemma::{Engine, SourceType};
use std::collections::HashMap;

fn engine_with_files(files: HashMap<String, String>) -> Engine {
    let mut engine = Engine::new();
    for (attr, code) in files {
        let src = if attr.trim().is_empty() {
            SourceType::Volatile
        } else {
            SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(attr.as_str())))
        };
        let _ = engine.load(&code, src);
    }
    engine
}

fn single_file(name: &str, code: &str) -> HashMap<String, String> {
    std::iter::once((name.to_string(), code.to_string())).collect()
}

#[test]
fn fuzz_deeply_nested_completes_fast() {
    let start = std::time::Instant::now();
    let mut expr = String::from("1");
    for _ in 0..5 {
        expr = format!("({} + 1)", expr);
    }
    let code = format!(
        "spec fuzz_nested\ndata x: 1\nrule deeply_nested: {}\n",
        expr
    );
    engine_with_files(single_file("fuzz_nested", &code));
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "5-deep nested parse took {}ms, expected <500ms (regression guard)",
        elapsed.as_millis()
    );
}

#[test]
fn fuzz_data_bindings_api_number_too_long_no_panic() {
    let code = "spec fuzz_test\ndata x: number\nrule doubled: x * 2\n";
    let mut engine = Engine::new();
    engine
        .load(
            code,
            SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "fuzz_binding",
            ))),
        )
        .unwrap();
    // 30 nines exceeds Decimal::MAX (~7.92e28): unrepresentable input must
    // veto with a parse reason, not panic.
    let mut data = HashMap::new();
    data.insert(
        "x".to_string(),
        "999999999999999999999999999999".to_string(),
    );
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "fuzz_test", Some(&now), data, false, None)
        .expect("run must complete with veto, not Error");
    let doubled = response.results.get("doubled").expect("doubled");
    assert!(
        doubled.vetoed,
        "expected veto for unrepresentable number, got {:?}",
        doubled.display
    );
    let reason = doubled.veto_reason.as_deref().expect("veto reason");
    assert!(
        reason.contains("Invalid number"),
        "expected parse-failure veto, got: {reason}"
    );
}

#[test]
fn data_binding_with_excess_fractional_digits_truncates_at_input() {
    // 40 fractional digits: input is truncated (rounded) to the 28 significant
    // digits a Decimal holds, then evaluation proceeds normally.
    let code = "spec fuzz_test\ndata x: number\nrule doubled: x * 2\n";
    let mut engine = Engine::new();
    engine
        .load(
            code,
            SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "fuzz_binding",
            ))),
        )
        .unwrap();
    let mut data = HashMap::new();
    data.insert(
        "x".to_string(),
        "0.1234567890123456789012345678901234567890".to_string(),
    );
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "fuzz_test", Some(&now), data, false, None)
        .expect("run must complete");
    let doubled = response.results.get("doubled").expect("doubled");
    assert!(
        !doubled.vetoed,
        "truncated input must evaluate, got veto: {:?}",
        doubled.veto_reason
    );
    assert_eq!(
        doubled.number.as_deref(),
        Some("0.2469135780246913578024691358"),
        "doubled truncated input"
    );
}
