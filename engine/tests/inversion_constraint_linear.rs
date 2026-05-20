use lemma::parsing::ast::DateTimeValue;
use lemma::{Bound, DataPath, Domain, Engine, Error, LiteralValue, Target, ValueKind};
use std::collections::HashMap;

#[test]
fn invert_unless_linear_addition() {
    let code = r#"
spec t
data x: number
rule r: 0
  unless x + 1 > 10 then veto "too much"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let inv = engine
        .invert(
            "t",
            Some(&now),
            "r",
            Target::veto(Some("too much".to_string())),
            HashMap::new(),
        )
        .unwrap();

    let x = DataPath::local("x".to_string());
    let nine = LiteralValue::number_from_decimal(rust_decimal::Decimal::from(9));

    assert!(!inv.is_empty(), "expected at least one inversion solution");

    let mut saw_expected = false;
    for domains in inv.domains.iter() {
        let Some(d) = domains.get(&x) else { continue };
        if let Domain::Range { min, max } = d {
            if matches!(min, Bound::Exclusive(v) if v.as_ref() == &nine)
                && matches!(max, Bound::Unbounded)
            {
                saw_expected = true;
            }
        }
    }
    assert!(saw_expected, "expected a domain equivalent to x > 9");
}

#[test]
fn invert_unless_linear_multiplication() {
    let code = r#"
spec t
data x: number
rule r: 0
  unless 2 * x <= 8 then veto "ok"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let inv = engine
        .invert(
            "t",
            Some(&now),
            "r",
            Target::veto(Some("ok".to_string())),
            HashMap::new(),
        )
        .unwrap();

    let x = DataPath::local("x".to_string());
    let four = LiteralValue::number_from_decimal(rust_decimal::Decimal::from(4));

    assert!(!inv.is_empty(), "expected at least one inversion solution");

    let mut saw_expected = false;
    for domains in inv.domains.iter() {
        let Some(d) = domains.get(&x) else { continue };
        if let Domain::Range { min, max } = d {
            if matches!(min, Bound::Unbounded)
                && matches!(max, Bound::Inclusive(v) if v.as_ref() == &four)
            {
                saw_expected = true;
            }
        }
    }
    assert!(saw_expected, "expected a domain equivalent to x <= 4");
}

#[test]
fn invert_unless_negative_coefficient_flips_inequality() {
    let code = r#"
spec t
data x: number
rule r: 0
  unless -2 * x > 4 then veto "neg"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let inv = engine
        .invert(
            "t",
            Some(&now),
            "r",
            Target::veto(Some("neg".to_string())),
            HashMap::new(),
        )
        .unwrap();

    let x = DataPath::local("x".to_string());
    let minus_two = LiteralValue::number_from_decimal(rust_decimal::Decimal::from(-2));

    assert!(!inv.is_empty(), "expected at least one inversion solution");

    let mut saw_expected = false;
    for domains in inv.domains.iter() {
        let Some(d) = domains.get(&x) else { continue };
        if let Domain::Range { min, max } = d {
            if matches!(min, Bound::Unbounded)
                && matches!(max, Bound::Exclusive(v) if v.as_ref() == &minus_two)
            {
                saw_expected = true;
            }
        }
    }
    assert!(saw_expected, "expected a domain equivalent to x < -2");
}

#[test]
fn invert_unless_quantity_unit_conversion_wrapper() {
    let code = r#"
spec t
data money: quantity -> unit eur 1.0 -> unit usd 1.18
data price: money
rule r: 0
  unless (price as eur) > 100 eur then veto "too expensive"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let inv = engine
        .invert(
            "t",
            Some(&now),
            "r",
            Target::veto(Some("too expensive".to_string())),
            HashMap::new(),
        )
        .unwrap();

    let price = DataPath::local("price".to_string());

    assert!(!inv.is_empty(), "expected at least one inversion solution");

    // We don't assert exact quantity type identity here; just that the derived lower bound is 100 eur.
    let mut saw_expected = false;
    for domains in inv.domains.iter() {
        let Some(d) = domains.get(&price) else {
            continue;
        };
        if let Domain::Range {
            min: Bound::Exclusive(v),
            max: Bound::Unbounded,
        } = d
        {
            if let ValueKind::Quantity(n, unit, _decomp) = &v.value {
                if lemma::commit_rational_to_decimal(n).unwrap() == rust_decimal::Decimal::from(100)
                    && unit == "eur"
                {
                    saw_expected = true;
                }
            }
        }
    }
    assert!(
        saw_expected,
        "expected a domain equivalent to price > 100 eur"
    );
}

#[test]
fn invert_unless_duration_unit_conversion_wrapper() {
    let code = r#"
spec t
uses lemma si
data duration: si.duration
data d: duration
rule r: 0
  unless (d as hours) >= 2 hours then veto "long"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let inv = engine
        .invert(
            "t",
            Some(&now),
            "r",
            Target::veto(Some("long".to_string())),
            HashMap::new(),
        )
        .unwrap();

    let d = DataPath::local("d".to_string());

    assert!(!inv.is_empty(), "expected at least one inversion solution");

    let mut saw_expected = false;
    for domains in inv.domains.iter() {
        let Some(dom) = domains.get(&d) else { continue };
        if let Domain::Range {
            min: Bound::Inclusive(v),
            max: Bound::Unbounded,
        } = dom
        {
            if let ValueKind::Quantity(n, unit, _) = &v.value {
                if lemma::commit_rational_to_decimal(n).unwrap() == rust_decimal::Decimal::from(2)
                    && unit.eq_ignore_ascii_case("hours")
                {
                    saw_expected = true;
                }
            }
        }
    }
    assert!(saw_expected, "expected a domain equivalent to d >= 2 hours");
}

#[test]
fn unsupported_comparison_shapes_return_inversion_error() {
    let code = r#"
spec t
data x: number
data y: number
rule r: 0
  unless x > y then veto "relational"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let err = engine
        .invert(
            "t",
            Some(&now),
            "r",
            Target::veto(Some("relational".to_string())),
            HashMap::new(),
        )
        .unwrap_err();

    assert!(matches!(err, Error::Inversion(_)));
}

#[test]
fn non_linear_comparison_returns_inversion_error() {
    let code = r#"
spec t
data x: number
rule r: 0
  unless x * x > 4 then veto "nonlinear"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let err = engine
        .invert(
            "t",
            Some(&now),
            "r",
            Target::veto(Some("nonlinear".to_string())),
            HashMap::new(),
        )
        .unwrap_err();

    assert!(matches!(err, Error::Inversion(_)));
}
