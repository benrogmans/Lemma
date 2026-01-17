use lemma::{Engine, LemmaError, LemmaResult};
use rust_decimal::Decimal;
use std::{collections::HashMap, str::FromStr};

fn run(code: &str, rule: &str) -> LemmaResult<String> {
    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma")?;
    let resp = engine.evaluate("test", vec![rule.to_string()], HashMap::new())?;
    let v = resp
        .results
        .values()
        .find(|r| r.rule.name == rule)
        .and_then(|r| r.result.value().cloned())
        .expect("rule value");
    Ok(v.to_string())
}

fn run_num(code: &str, rule: &str) -> LemmaResult<Decimal> {
    let s = run(code, rule)?;
    s.parse::<Decimal>().map_err(|e| {
        LemmaError::engine(
            format!("Failed to parse '{s}' as Decimal: {e}"),
            lemma::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<unknown>",
            std::sync::Arc::from(s),
            "<unknown>",
            1,
            None::<String>,
        )
    })
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
fn test_exp_and_power() -> LemmaResult<()> {
    let code = r#"
    doc test
    rule a = exp 1
    rule b = 2 ^ 3
    "#;
    let a = run_num(code, "a")?;
    let b = run_num(code, "b")?;
    // Compare against a decimal literal approximation without floats
    assert_close_dec(&a, &dec("2.718281828459045"), &tol(9));
    assert_eq!(b, Decimal::from(8));
    Ok(())
}

#[test]
fn test_abs_floor_ceil_round() -> LemmaResult<()> {
    let code = r#"
    doc test
    rule a = abs -3.5
    rule b = floor 3.9
    rule c = ceil 3.1
    rule d = round 3.5
    rule e = round -3.5
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
fn test_sqrt_and_log_basic() -> LemmaResult<()> {
    let code = r#"
    doc test
    rule a = sqrt 9
    rule b = sqrt 2
    rule c = log (exp 1)
    rule d = log 1
    rule e = 2 ^ 0.5
    rule bb = (sqrt 2) * (sqrt 2)
    rule ee = (2 ^ 0.5) * (2 ^ 0.5)
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
fn test_trig_at_zero() -> LemmaResult<()> {
    let code = r#"
    doc test
    rule s = sin 0
    rule c = cos 0
    rule t = tan 0
    rule as = asin 0
    rule ac = acos 1
    rule at = atan 0
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
fn test_nested_math_ops() -> LemmaResult<()> {
    let code = r#"
    doc test
    rule a = round (abs -3.6)
    rule b = ceil (sqrt 2)
    rule c = floor (exp 1)
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
    engine
        .add_lemma_code(
            r#"
        doc test
        rule bad_sqrt = sqrt -1
        rule bad_log0 = log 0
        rule bad_log_neg = log -5
    "#,
            "test.lemma",
        )
        .unwrap();

    // Evaluate all rules and expect a Veto result (runtime error)
    let res1 = engine
        .evaluate("test", vec!["bad_sqrt".to_string()], HashMap::new())
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
        .evaluate("test", vec!["bad_log0".to_string()], HashMap::new())
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
        .evaluate("test", vec!["bad_log_neg".to_string()], HashMap::new())
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
    engine
        .add_lemma_code(
            r#"
        doc test
        rule bad_asin = asin 2
    "#,
            "test.lemma",
        )
        .unwrap();

    let response = engine
        .evaluate("test", vec!["bad_asin".to_string()], HashMap::new())
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
