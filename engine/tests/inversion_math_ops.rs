use lemma::{Engine, LiteralValue, Target};
mod common;
use common::add_lemma_code_blocking;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn setup_engine(code: &str) -> Engine {
    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").expect("Failed to add code");
    engine
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).expect("valid decimal")
}

#[test]
fn invert_exp_simple() {
    let code = r#"
        doc math
        fact x: [number]
        rule y: exp x
    "#;
    let engine = setup_engine(code);

    // y = e^2 ≈ 7.38905609893065
    let solutions = engine
        .invert(
            "math",
            "y",
            Target::value(LiteralValue::number(dec("7.38905609893065"))),
            HashMap::new(),
        )
        .expect("invert OK");

    assert!(!solutions.is_empty(), "Expected at least one solution");
}

#[test]
fn invert_power_exponent_unknown() {
    let code = r#"
        doc math
        fact x: [number]
        rule y: 2 ^ x
    "#;
    let engine = setup_engine(code);
    let solutions = engine
        .invert(
            "math",
            "y",
            Target::value(LiteralValue::number(8.into())),
            HashMap::new(),
        )
        .expect("invert OK");

    assert!(!solutions.is_empty(), "Expected at least one solution");
}

#[test]
fn invert_power_base_unknown() {
    let code = r#"
        doc math
        fact x: [number]
        rule y: x ^ 2
    "#;
    let engine = setup_engine(code);
    let solutions = engine
        .invert(
            "math",
            "y",
            Target::value(LiteralValue::number(9.into())),
            HashMap::new(),
        )
        .expect("invert OK");

    assert!(!solutions.is_empty(), "Expected at least one solution");
}
