//! Mirrors fuzz target API usage so that compile-breaking changes
//! to the public API are caught by `cargo nextest run` before the
//! nightly-only fuzz job ever runs.

use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, SourceType};
use std::collections::HashMap;

fn engine_with_files(files: HashMap<String, String>) -> Engine {
    let mut engine = Engine::new();
    for (attr, code) in files {
        let src = if attr.trim().is_empty() {
            SourceType::Inline
        } else {
            SourceType::Labeled(attr.as_str())
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
        .load(code, SourceType::Labeled("fuzz_binding"))
        .unwrap();
    let mut data = HashMap::new();
    data.insert("x".to_string(), "40000000000000000460903669760".to_string());
    let now = DateTimeValue::now();
    let result = engine.run("fuzz_test", Some(&now), data, false);
    assert!(
        result.is_err(),
        "expected validation error for 29-digit number, got {:?}",
        result
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("too many digits") || err.to_string().contains("Invalid number"),
        "expected 'too many digits' or parse error, got: {}",
        err
    );
}
