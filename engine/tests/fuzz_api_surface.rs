//! Mirrors fuzz target API usage so that compile-breaking changes
//! to the public API are caught by `cargo nextest run` before the
//! nightly-only fuzz job ever runs.

use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

fn engine_with_files(files: HashMap<String, String>) -> Engine {
    let mut engine = Engine::new();
    let _ = engine.add_lemma_files(files);
    engine
}

fn single_file(name: &str, code: &str) -> HashMap<String, String> {
    std::iter::once((name.to_string(), code.to_string())).collect()
}

// --- mirrors fuzz_parser ---

#[test]
fn fuzz_parser_api_valid_spec() {
    let code = "spec hello\nfact x: 42\n";
    engine_with_files(single_file("input", code));
}

#[test]
fn fuzz_parser_api_garbage() {
    engine_with_files(single_file("input", "not a valid spec!!!"));
}

#[test]
fn fuzz_parser_api_empty() {
    engine_with_files(single_file("input", ""));
}

// --- mirrors fuzz_expressions ---

#[test]
fn fuzz_expressions_api_valid() {
    let code = "spec fuzz_test\nfact x: 100\nfact y: 50\nrule test_expr: x + y\n";
    engine_with_files(single_file("fuzz_expr", code));
}

#[test]
fn fuzz_expressions_api_garbage_expr() {
    let code = "spec fuzz_test\nfact x: 100\nfact y: 50\nrule test_expr: @#$%^&\n";
    engine_with_files(single_file("fuzz_expr", code));
}

// --- mirrors fuzz_literals ---

#[test]
fn fuzz_literals_api_number() {
    let code = "spec fuzz_test\nfact test_value: 42\n";
    engine_with_files(single_file("fuzz_literal", code));
}

#[test]
fn fuzz_literals_api_garbage() {
    let code = "spec fuzz_test\nfact test_value: !!!garbage\n";
    engine_with_files(single_file("fuzz_literal", code));
}

// --- mirrors fuzz_deeply_nested ---

#[test]
fn fuzz_deeply_nested_api() {
    for depth in 1..=5 {
        let mut expr = String::from("1");
        for _ in 0..depth {
            expr = format!("({} + 1)", expr);
        }
        let code = format!(
            "spec fuzz_nested\nfact x: 1\nrule deeply_nested: {}\n",
            expr
        );
        engine_with_files(single_file("fuzz_nested", &code));
    }
}

// --- mirrors fuzz_fact_bindings ---

#[test]
fn fuzz_fact_bindings_api_valid_number() {
    let code = "spec fuzz_test\nfact x: [number]\nrule doubled: x * 2\n";
    let mut engine = Engine::new();
    let files = single_file("fuzz_binding", code);
    if engine.add_lemma_files(files).is_ok() {
        let mut facts = HashMap::new();
        facts.insert("x".to_string(), "42".to_string());
        let now = DateTimeValue::now();
        let _ = engine.evaluate("fuzz_test", None, &now, vec![], facts);
    }
}

#[test]
fn fuzz_fact_bindings_api_garbage_value() {
    let code = "spec fuzz_test\nfact x: [number]\nrule doubled: x * 2\n";
    let mut engine = Engine::new();
    let files = single_file("fuzz_binding", code);
    if engine.add_lemma_files(files).is_ok() {
        let mut facts = HashMap::new();
        facts.insert("x".to_string(), "not_a_number".to_string());
        let now = DateTimeValue::now();
        let _ = engine.evaluate("fuzz_test", None, &now, vec![], facts);
    }
}

#[test]
fn fuzz_fact_bindings_api_empty_value() {
    let code = "spec fuzz_test\nfact x: [number]\nrule doubled: x * 2\n";
    let mut engine = Engine::new();
    let files = single_file("fuzz_binding", code);
    if engine.add_lemma_files(files).is_ok() {
        let mut facts = HashMap::new();
        facts.insert("x".to_string(), String::new());
        let now = DateTimeValue::now();
        let _ = engine.evaluate("fuzz_test", None, &now, vec![], facts);
    }
}
