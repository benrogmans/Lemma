use lemma::DateTimeValue;
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
  unless (price) > 100 eur then veto "too expensive"
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
            if let ValueKind::Quantity(n, signature) = &v.value {
                let unit_name = signature.first().map(|(n, _)| n.as_str()).unwrap_or("");
                if lemma::ValueKind::Number(*n).as_decimal_magnitude().unwrap()
                    == rust_decimal::Decimal::from(100)
                    && unit_name == "eur"
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
uses lemma units
data duration: units.duration
data d: duration
rule r: 0
  unless d >= 2 hours then veto "long"
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
            if let ValueKind::Quantity(n, signature) = &v.value {
                let unit_name = signature.first().map(|(n, _)| n.as_str()).unwrap_or("");
                if unit_name.eq_ignore_ascii_case("hours")
                    && lemma::ValueKind::Number(*n).as_decimal_magnitude().unwrap()
                        == rust_decimal::Decimal::from(7200)
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

/// Regression: inversion must not panic when constant-folding `number as <type>.<unit>` in unless.
#[test]
fn invert_unless_qualified_unit_cast_constant_fold_completes() {
    let code = r#"
spec t
uses lemma units
data duration: units.duration
data d: duration
rule r: 0
  unless d > 5 minutes then veto "too long"
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();

    let inv = engine
        .invert(
            "t",
            Some(&now),
            "r",
            Target::veto(Some("too long".to_string())),
            HashMap::new(),
        )
        .expect("invert must complete without internal panic");

    assert!(!inv.is_empty(), "expected at least one inversion solution");
}

/// Phase 0 — inversion with dimensionally-compatible but differently-typed quantity bounds.
///
/// Two `unless` clauses place bounds on the same unknown `r` using units from two
/// DIFFERENT quantity types (`rate_per_hour` and `rate_per_minute`). The inversion engine
/// must intersect those bounds, which calls `lit_cmp(rate_per_hour_bound, rate_per_minute_bound)`.
///
/// Today `lit_cmp` (domain.rs:534-539) panics with:
///   `unreachable!("BUG: lit_cmp compared different quantity types")`
/// because `a.lemma_type != b.lemma_type`.
///
/// After todo `inversion_lit_cmp_signature_path` the comparison uses `signature_factor`
/// for dimensionally-compatible conversions. `60 eur_per_hour = 1 eur_per_minute`, so the
/// bounds are equivalent and the tighter of the two is preserved correctly.
#[test]
fn inversion_with_real_signature_index_solves_cross_type() {
    // `r` is of type `rate_per_hour`. The two `unless` clauses use bounds expressed in
    // `rate_per_hour` (60) and `rate_per_minute` (2 eur_per_minute == 120 eur_per_hour)
    // respectively. The lower bound (cap1) fires before the higher one (cap2); with
    // last-wins semantics, cap1's solution domain is the bounded interval (60, 120] eur/h.
    let code = r#"spec cross_rate
uses lemma units

data money: quantity
  -> unit eur 1

data rate_per_hour: quantity
  -> unit eur_per_hour eur/hour

data rate_per_minute: quantity
  -> unit eur_per_minute eur/minute

data r: rate_per_hour

rule within_limit: yes
  unless r > 60 eur_per_hour then veto "cap1"
  unless r > 2 eur_per_minute then veto "cap2"
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("spec must load");
    let now = DateTimeValue::now();

    // Today planning rejects this with "Cannot compare unrelated quantity types".
    // After the plan changes (invariant 2 + inversion_lit_cmp_signature_path),
    // dimensionally-compatible comparisons are allowed and inversion returns a non-empty domain.
    let inv = engine
        .invert(
            "cross_rate",
            Some(&now),
            "within_limit",
            Target::veto(Some("cap1".to_string())),
            HashMap::new(),
        )
        .expect("inversion must not error on dimensionally-compatible cross-type bounds");

    assert!(!inv.is_empty(), "expected at least one inversion solution");

    // The domain for `r` must include an upper bound derived from the tighter constraint.
    let r_path = DataPath::local("r".to_string());
    let has_quantity_bound = inv.domains.iter().any(|domains| {
        domains
            .get(&r_path)
            .map(|dom| {
                matches!(
                    dom,
                    Domain::Range {
                        max: Bound::Exclusive(_),
                        ..
                    } | Domain::Range {
                        max: Bound::Inclusive(_),
                        ..
                    }
                )
            })
            .unwrap_or(false)
    });
    assert!(
        has_quantity_bound,
        "expected a bounded domain for `r`; got: {:?}",
        inv.domains
    );
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
