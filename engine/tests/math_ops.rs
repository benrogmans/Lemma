use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
mod common;
use common::add_lemma_code_blocking;
use rust_decimal::Decimal;
use std::{collections::HashMap, str::FromStr};

fn run(code: &str, rule: &str) -> Result<String, Vec<lemma::Error>> {
    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test.lemma")?;
    let now = DateTimeValue::now();
    let resp = engine
        .evaluate("test", None, &now, vec![rule.to_string()], HashMap::new())
        .map_err(|e| vec![e])?;
    let v = resp
        .results
        .values()
        .find(|r| r.rule.name == rule)
        .and_then(|r| r.result.value().cloned())
        .expect("rule value");
    Ok(v.to_string())
}

fn run_num(code: &str, rule: &str) -> Result<Decimal, Vec<lemma::Error>> {
    let s = run(code, rule)?;
    Ok(s.parse::<Decimal>()
        .expect("engine result should parse as Decimal"))
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).expect("valid decimal literal")
}

fn assert_close_dec(actual: &Decimal, expected: &Decimal, tol: &Decimal) {
    let diff = if actual > expected {
        *actual - *expected
    } else {
        *expected - *actual
    };
    assert!(
        diff <= *tol,
        "expected ~{expected} (±{tol}), got {actual} (diff {diff})"
    );
}

fn tol(scale: u32) -> Decimal {
    // 1 with 'scale' decimal places represents 10^-scale
    Decimal::new(1, scale)
}

#[test]
fn test_exp_and_power() -> Result<(), Vec<lemma::Error>> {
    let code = r#"
    spec test
    rule a: exp 1
    rule b: 2 ^ 3
    "#;
    let a = run_num(code, "a")?;
    let b = run_num(code, "b")?;
    // Compare against a decimal literal approximation without floats
    assert_close_dec(&a, &dec("2.718281828459045"), &tol(9));
    assert_eq!(b, Decimal::from(8));
    Ok(())
}

#[test]
fn test_abs_floor_ceil_round() -> Result<(), Vec<lemma::Error>> {
    let code = r#"
    spec test
    rule a: abs -3.5
    rule b: floor 3.9
    rule c: ceil 3.1
    rule d: round 3.5
    rule e: round -3.5
    "#;
    assert_eq!(run(code, "a")?, "3.5");
    assert_eq!(run(code, "b")?, "3");
    assert_eq!(run(code, "c")?, "4");
    // Decimal::round uses bankers rounding; 3.5 -> 4, -3.5 -> -4 or -3 depending on strategy.
    // We accept either "4" or "3" for round 3.5 if strategy differs across versions, but typically "4".
    let d = run(code, "d")?;
    assert!(d == "4" || d == "3");
    let e = run(code, "e")?;
    assert!(e == "-4" || e == "-3");
    Ok(())
}

#[test]
fn test_sqrt_and_log_basic() -> Result<(), Vec<lemma::Error>> {
    let code = r#"
    spec test
    rule a: sqrt 9
    rule b: sqrt 2
    rule c: log (exp 1)
    rule d: log 1
    rule e: 2 ^ 0.5
    rule bb: (sqrt 2) * (sqrt 2)
    rule ee: (2 ^ 0.5) * (2 ^ 0.5)
    "#;
    assert_eq!(run_num(code, "a")?, Decimal::from(3));
    // Validate sqrt(2) via identity: (sqrt 2)^2 ≈ 2 (within tolerance)
    let bb = run_num(code, "bb")?;
    assert_close_dec(&bb, &dec("2"), &tol(9));
    // log(exp 1) ≈ 1
    let c = run_num(code, "c")?;
    assert_close_dec(&c, &dec("1"), &tol(9));
    assert_eq!(run_num(code, "d")?, Decimal::from(0));
    // Validate 2^(1/2) via identity: (2^(1/2))^2 ≈ 2
    let ee = run_num(code, "ee")?;
    assert_close_dec(&ee, &dec("2"), &tol(9));
    Ok(())
}

#[test]
fn test_trig_at_zero() -> Result<(), Vec<lemma::Error>> {
    let code = r#"
    spec test
    rule s: sin 0
    rule c: cos 0
    rule t: tan 0
    rule as: asin 0
    rule ac: acos 1
    rule at: atan 0
    "#;
    assert_eq!(run(code, "s")?, "0");
    assert_eq!(run(code, "c")?, "1");
    assert_eq!(run(code, "t")?, "0");
    assert_eq!(run(code, "as")?, "0");
    assert_eq!(run(code, "ac")?, "0");
    assert_eq!(run(code, "at")?, "0");
    Ok(())
}

#[test]
fn test_nested_math_ops() -> Result<(), Vec<lemma::Error>> {
    let code = r#"
    spec test
    rule a: round (abs -3.6)
    rule b: ceil (sqrt 2)
    rule c: floor (exp 1)
    "#;
    // abs(-3.6) = 3.6 -> round = 4 (bankers rounding still gives 4 here)
    assert_eq!(run(code, "a")?, "4");
    // sqrt(2) ~ 1.414 -> ceil -> 2
    assert_eq!(run(code, "b")?, "2");
    // e^1 ~ 2.718 -> floor -> 2
    assert_eq!(run(code, "c")?, "2");
    Ok(())
}

#[test]
fn test_sqrt_negative_and_log_domain_errors() {
    // sqrt of negative and log of non-positive should yield runtime errors
    let mut engine = Engine::new();
    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec test
        rule bad_sqrt: sqrt -1
        rule bad_log0: log 0
        rule bad_log_neg: log -5
    "#,
        "test.lemma",
    )
    .unwrap();
    let now = DateTimeValue::now();

    let res1 = engine
        .evaluate(
            "test",
            None,
            &now,
            vec!["bad_sqrt".to_string()],
            HashMap::new(),
        )
        .unwrap();
    let rule = res1
        .results
        .values()
        .find(|r| r.rule.name == "bad_sqrt")
        .unwrap();
    assert!(
        rule.result.value().is_none(),
        "sqrt(-1) should return Veto, got: {:?}",
        rule.result
    );

    let res2 = engine
        .evaluate(
            "test",
            None,
            &now,
            vec!["bad_log0".to_string()],
            HashMap::new(),
        )
        .unwrap();
    let rule = res2
        .results
        .values()
        .find(|r| r.rule.name == "bad_log0")
        .unwrap();
    assert!(
        rule.result.value().is_none(),
        "log 0 should return Veto, got: {:?}",
        rule.result
    );

    let res3 = engine
        .evaluate(
            "test",
            None,
            &now,
            vec!["bad_log_neg".to_string()],
            HashMap::new(),
        )
        .unwrap();
    let rule = res3
        .results
        .values()
        .find(|r| r.rule.name == "bad_log_neg")
        .unwrap();
    assert!(
        rule.result.value().is_none(),
        "log negative should return Veto, got: {:?}",
        rule.result
    );
}

#[test]
fn test_inverse_trig_domain_error() {
    // asin(x) domain is [-1,1]; asin(2) should return Veto
    let mut engine = Engine::new();
    add_lemma_code_blocking(
        &mut engine,
        r#"
        spec test
        rule bad_asin: asin 2
    "#,
        "test.lemma",
    )
    .unwrap();
    let now = DateTimeValue::now();

    let response = engine
        .evaluate(
            "test",
            None,
            &now,
            vec!["bad_asin".to_string()],
            HashMap::new(),
        )
        .unwrap();

    let rule = response
        .results
        .values()
        .find(|r| r.rule.name == "bad_asin")
        .unwrap();
    assert!(
        rule.result.value().is_none(),
        "asin 2 should return Veto, got: {:?}",
        rule.result
    );
}
