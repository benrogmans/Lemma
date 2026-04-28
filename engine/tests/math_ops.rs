use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use rust_decimal::Decimal;
use std::{collections::HashMap, str::FromStr};

fn run(code: &str, rule: &str) -> Result<String, lemma::Errors> {
    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Labeled("test.lemma"))?;
    let now = DateTimeValue::now();
    let mut resp = engine
        .run("test", Some(&now), HashMap::new(), false)
        .map_err(|e| lemma::Errors {
            errors: vec![e],
            sources: engine.sources().clone(),
        })?;
    resp.filter_rules(&[rule.to_string()]);
    let v = resp
        .results
        .values()
        .find(|r| r.rule.name == rule)
        .and_then(|r| r.result.value().cloned())
        .expect("rule value");
    Ok(v.to_string())
}

fn run_num(code: &str, rule: &str) -> Result<Decimal, lemma::Errors> {
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
fn test_exp_and_power() -> Result<(), lemma::Errors> {
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
fn test_sqrt_and_log_basic() -> Result<(), lemma::Errors> {
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
fn test_trig_at_zero() -> Result<(), lemma::Errors> {
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
fn test_nested_math_ops() -> Result<(), lemma::Errors> {
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
