use proptest::prelude::*;
use rust_decimal::{prelude::FromPrimitive, Decimal};
use std::{collections::HashMap, str::FromStr};

use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, ValueKind};

/// Get the result of a rule evaluation.
/// Panics if the rule is not found (test failure).
/// Returns the OperationResult which must be checked explicitly.
fn get_rule_result(
    engine: &mut Engine,
    spec_name: &str,
    rule_name: &str,
) -> lemma::OperationResult {
    let now = DateTimeValue::now();
    let response = engine
        .run(spec_name, Some(&now), HashMap::new(), false)
        .unwrap();
    response
        .results
        .values()
        .find(|r| r.rule.name == rule_name)
        .map(|r| r.result.clone())
        .unwrap_or_else(|| panic!("Rule '{}' not found in spec '{}'", rule_name, spec_name))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_multiplication_by_zero(n in -1000.0..1000.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact x: {}
rule result: x * 0
"#, n);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let result = get_rule_result(&mut engine, "test", "result");
        let val = result
            .value()
            .expect("Expected value result, got veto")
            .clone();
        if let ValueKind::Number(num) = &val.value {
            prop_assert_eq!(*num, Decimal::from_str("0").unwrap());
        }
    }

    #[test]
    fn prop_multiplication_identity(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact x: {}
rule result: x * 1
"#, n);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let result = get_rule_result(&mut engine, "test", "result");
        let val = result
            .value()
            .expect("Expected value result, got veto")
            .clone();
        if let ValueKind::Number(num) = &val.value {
            let expected = Decimal::from_f64(n).unwrap();
            let diff = (num - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_addition_identity(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact x: {}
rule result: x + 0
"#, n);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let result = get_rule_result(&mut engine, "test", "result");
        let val = result
            .value()
            .expect("Expected value result, got veto")
            .clone();
        if let ValueKind::Number(num) = &val.value {
            let expected = Decimal::from_f64(n).unwrap();
            let diff = (num - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_comparison_consistency(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact x: {}
rule eq_self: x == x
rule lte_self: x <= x
"#, n);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let result = get_rule_result(&mut engine, "test", "eq_self");
        let val = result.value().expect("Expected value result, got veto").clone();
        if let ValueKind::Boolean(b) = &val.value {
            prop_assert!(*b);
        }

        let result = get_rule_result(&mut engine, "test", "lte_self");
        let val = result.value().expect("Expected value result, got veto").clone();
        if let ValueKind::Boolean(b) = &val.value {
            prop_assert!(*b);
        }
    }

    #[test]
    fn prop_fact_binding_works(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = r#"
spec test
fact x: [number]
rule doubled: x * 2
"#;
        engine.load(code, lemma::SourceType::Labeled("test")).unwrap();

        let mut facts: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        facts.insert("x".to_string(), format!("{}", n));
        let now = DateTimeValue::now();
        let response = engine.run("test", Some(&now), facts, false).unwrap();

        let result = response
            .results.values()
            .find(|r| r.rule.name == "doubled")
            .expect("Rule 'doubled' not found");
        let val = result
            .result
            .value()
            .expect("Expected value result, got veto")
            .clone();
        if let ValueKind::Number(num) = &val.value {
            let expected = Decimal::from_f64(n * 2.0).unwrap();
            let diff = (num - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_addition_commutative(a in -100.0..100.0, b in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
rule sum1: a + b
rule sum2: b + a
"#, a, b);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let v1_result = get_rule_result(&mut engine, "test", "sum1");
        let v1 = v1_result.value().expect("Expected value, got veto").clone();
        let v2_result = get_rule_result(&mut engine, "test", "sum2");
        let v2 = v2_result.value().expect("Expected value, got veto").clone();

        if let (ValueKind::Number(val1), ValueKind::Number(val2)) = (&v1.value, &v2.value) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_multiplication_commutative(a in -50.0..50.0, b in -50.0..50.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
rule prod1: a * b
rule prod2: b * a
"#, a, b);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let v1_result = get_rule_result(&mut engine, "test", "prod1");
        let v1 = v1_result.value().expect("Expected value, got veto").clone();
        let v2_result = get_rule_result(&mut engine, "test", "prod2");
        let v2 = v2_result.value().expect("Expected value, got veto").clone();

        if let (ValueKind::Number(val1), ValueKind::Number(val2)) = (&v1.value, &v2.value) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_addition_associative(a in -50.0..50.0, b in -50.0..50.0, c in -50.0..50.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
fact c: {}
rule sum1: (a + b) + c
rule sum2: a + (b + c)
"#, a, b, c);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let v1_result = get_rule_result(&mut engine, "test", "sum1");
        let v1 = v1_result.value().expect("Expected value, got veto").clone();
        let v2_result = get_rule_result(&mut engine, "test", "sum2");
        let v2 = v2_result.value().expect("Expected value, got veto").clone();

        if let (ValueKind::Number(val1), ValueKind::Number(val2)) = (&v1.value, &v2.value) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_multiplication_associative(a in -20.0..20.0, b in -20.0..20.0, c in -20.0..20.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
fact c: {}
rule prod1: (a * b) * c
rule prod2: a * (b * c)
"#, a, b, c);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let v1_result = get_rule_result(&mut engine, "test", "prod1");
        let v1 = v1_result.value().expect("Expected value, got veto").clone();
        let v2_result = get_rule_result(&mut engine, "test", "prod2");
        let v2 = v2_result.value().expect("Expected value, got veto").clone();

        if let (ValueKind::Number(val1), ValueKind::Number(val2)) = (&v1.value, &v2.value) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.01").unwrap());
        }
    }

    #[test]
    fn prop_distributive(a in -50.0..50.0, b in -50.0..50.0, c in -50.0..50.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
fact c: {}
rule dist1: a * (b + c)
rule dist2: (a * b) + (a * c)
"#, a, b, c);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let v1_result = get_rule_result(&mut engine, "test", "dist1");
        let v1 = v1_result.value().expect("Expected value, got veto").clone();
        let v2_result = get_rule_result(&mut engine, "test", "dist2");
        let v2 = v2_result.value().expect("Expected value, got veto").clone();

        if let (ValueKind::Number(val1), ValueKind::Number(val2)) = (&v1.value, &v2.value) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.01").unwrap());
        }
    }

    #[test]
    fn prop_negation_involution(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact x: {}
rule double_neg: -(-x)
"#, n);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let result = get_rule_result(&mut engine, "test", "double_neg");


        let val = result.value().expect("Expected value result, got veto").clone();


        if let ValueKind::Number(val) = &val.value {
            let expected = Decimal::from_f64(n).unwrap();
            let diff = (val - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_subtraction_as_addition_of_negative(a in -100.0..100.0, b in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
rule sub: a - b
rule add_neg: a + (-b)
"#, a, b);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let v1_result = get_rule_result(&mut engine, "test", "sub");
        let v1 = v1_result.value().expect("Expected value, got veto").clone();
        let v2_result = get_rule_result(&mut engine, "test", "add_neg");
        let v2 = v2_result.value().expect("Expected value, got veto").clone();

        if let (ValueKind::Number(val1), ValueKind::Number(val2)) = (&v1.value, &v2.value) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_division_inverse_of_multiplication(a in 1.0..100.0, b in 1.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
rule product: a * b
rule back: product / b
"#, a, b);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let result = get_rule_result(&mut engine, "test", "back");


        let val = result.value().expect("Expected value result, got veto").clone();


        if let ValueKind::Number(val) = &val.value {
            let expected = Decimal::from_f64(a).unwrap();
            let diff = (val - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.01").unwrap());
        }
    }

    #[test]
    fn prop_boolean_not_involution(b in prop::bool::ANY) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact b: {}
rule double_not: not (not b)
"#, b);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let result = get_rule_result(&mut engine, "test", "double_not");


        let val = result.value().expect("Expected value result, got veto").clone();


        if let ValueKind::Boolean(val) = &val.value {
            prop_assert_eq!(*val, b);
        }
    }

    #[test]
    fn prop_and_commutative(a in prop::bool::ANY, b in prop::bool::ANY) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
rule and1: a and b
rule and2: b and a
"#, a, b);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let v1_result = get_rule_result(&mut engine, "test", "and1");
        let v1 = v1_result.value().expect("Expected value, got veto").clone();
        let v2_result = get_rule_result(&mut engine, "test", "and2");
        let v2 = v2_result.value().expect("Expected value, got veto").clone();

        if let (ValueKind::Boolean(val1), ValueKind::Boolean(val2)) = (&v1.value, &v2.value) {
            prop_assert_eq!(val1, val2);
        }
    }

    #[test]
    fn prop_comparison_transitivity(a in 1.0..100.0, b in 1.0..100.0) {
        let (min, max) = if a < b { (a, b) } else { (b, a) };
        let mid = (min + max) / 2.0;

        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact a: {}
fact b: {}
fact c: {}
rule ab: a < b
rule bc: b < c
rule ac: a < c
"#, min, mid, max);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let ab_result = get_rule_result(&mut engine, "test", "ab");
        let ab = ab_result.value().expect("Expected value, got veto").clone();
        let bc_result = get_rule_result(&mut engine, "test", "bc");
        let bc = bc_result.value().expect("Expected value, got veto").clone();
        let ac_result = get_rule_result(&mut engine, "test", "ac");
        let ac = ac_result.value().expect("Expected value, got veto").clone();

        if let (ValueKind::Boolean(ab_val), ValueKind::Boolean(bc_val), ValueKind::Boolean(ac_val)) = (&ab.value, &bc.value, &ac.value) {
            if *ab_val && *bc_val {
                prop_assert!(*ac_val);
            }
        }
    }

    #[test]
    fn prop_unless_last_matching_wins(n in 1.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec test
fact x: {}
rule discount: 0
  unless x > 10 then 10
  unless x > 20 then 20
  unless x > 50 then 50
"#, n);
        engine.load(&code, lemma::SourceType::Labeled("test")).unwrap();

        let result = get_rule_result(&mut engine, "test", "discount");


        let val = result.value().expect("Expected value result, got veto").clone();


        if let ValueKind::Number(val) = &val.value {
            let expected = if n > 50.0 {
                Decimal::from(50)
            } else if n > 20.0 {
                Decimal::from(20)
            } else if n > 10.0 {
                Decimal::from(10)
            } else {
                Decimal::from(0)
            };
            prop_assert_eq!(*val, expected);
        }
    }
}

#[test]
fn test_arithmetic_properties() {
    let test_values = vec![-100.0, -1.0, 0.0, 1.0, 42.0, 100.0];

    for n in test_values {
        let mut engine = Engine::new();
        let code = format!(
            r#"
spec test
fact x: {}
rule zero: x * 0
rule identity_mul: x * 1
rule identity_add: x + 0
rule commutative1: x + 5
rule commutative2: 5 + x
"#,
            n
        );
        engine
            .load(&code, lemma::SourceType::Labeled("test"))
            .unwrap();

        let result = get_rule_result(&mut engine, "test", "zero");

        let val = result
            .value()
            .expect("Expected value result, got veto")
            .clone();

        if let ValueKind::Number(val) = &val.value {
            assert_eq!(
                *val,
                Decimal::from_str("0").unwrap(),
                "Multiplication by zero failed for {}",
                n
            );
        }

        let result = get_rule_result(&mut engine, "test", "identity_mul");

        let val = result
            .value()
            .expect("Expected value result, got veto")
            .clone();

        if let ValueKind::Number(val) = &val.value {
            let expected = Decimal::from_f64(n).unwrap();
            assert!(
                (val - expected).abs() < Decimal::from_str("0.001").unwrap(),
                "Multiplication identity failed for {}",
                n
            );
        }

        let result = get_rule_result(&mut engine, "test", "identity_add");

        let val = result
            .value()
            .expect("Expected value result, got veto")
            .clone();

        if let ValueKind::Number(val) = &val.value {
            let expected = Decimal::from_f64(n).unwrap();
            assert!(
                (val - expected).abs() < Decimal::from_str("0.001").unwrap(),
                "Addition identity failed for {}",
                n
            );
        }

        let comm1_result = get_rule_result(&mut engine, "test", "commutative1");
        let comm1 = comm1_result
            .value()
            .expect("Expected value, got veto")
            .clone();
        let comm2_result = get_rule_result(&mut engine, "test", "commutative2");
        let comm2 = comm2_result
            .value()
            .expect("Expected value, got veto")
            .clone();
        if let (ValueKind::Number(v1), ValueKind::Number(v2)) = (&comm1.value, &comm2.value) {
            assert!(
                (v1 - v2).abs() < Decimal::from_str("0.001").unwrap(),
                "Commutativity failed for {}",
                n
            );
        }
    }
}

#[test]
fn test_comparison_properties() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact a: 10
fact b: 20
fact c: 30
rule a_lt_b: a < b
rule b_lt_c: b < c
rule a_lt_c: a < c
rule a_eq_a: a == a
rule a_lte_a: a <= a
rule a_gte_a: a >= a
"#;
    engine
        .load(code, lemma::SourceType::Labeled("test"))
        .unwrap();

    let result = get_rule_result(&mut engine, "test", "a_eq_a");

    let val = result
        .value()
        .expect("Expected value result, got veto")
        .clone();

    if let ValueKind::Boolean(val) = &val.value {
        assert!(*val, "Reflexive equality failed");
    }

    let result = get_rule_result(&mut engine, "test", "a_lte_a");

    let val = result
        .value()
        .expect("Expected value result, got veto")
        .clone();

    if let ValueKind::Boolean(val) = &val.value {
        assert!(*val, "Reflexive <= failed");
    }

    let result = get_rule_result(&mut engine, "test", "a_gte_a");

    let val = result
        .value()
        .expect("Expected value result, got veto")
        .clone();

    if let ValueKind::Boolean(val) = &val.value {
        assert!(*val, "Reflexive >= failed");
    }

    let ab_result = get_rule_result(&mut engine, "test", "a_lt_b");
    let ab = ab_result.value().expect("Expected value, got veto").clone();
    let bc_result = get_rule_result(&mut engine, "test", "b_lt_c");
    let bc = bc_result.value().expect("Expected value, got veto").clone();
    let ac_result = get_rule_result(&mut engine, "test", "a_lt_c");
    let ac = ac_result.value().expect("Expected value, got veto").clone();

    if let (ValueKind::Boolean(true), ValueKind::Boolean(true), ValueKind::Boolean(val)) =
        (&ab.value, &bc.value, &ac.value)
    {
        assert!(*val, "Transitivity of < failed");
    }
}

#[test]
fn test_duration_conversion_properties() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact duration: 60 minutes
rule to_hours: duration in hours
"#;
    engine
        .load(code, lemma::SourceType::Labeled("test"))
        .unwrap();

    let result = get_rule_result(&mut engine, "test", "to_hours");

    let val = result
        .value()
        .expect("Expected value result, got veto")
        .clone();

    if let ValueKind::Duration(value, _) = &val.value {
        // 60 minutes = 1 hour (the conversion returns 1 hour, not 3600 seconds)
        assert_eq!(
            *value,
            Decimal::from_str("1").unwrap(),
            "minutes to hours conversion failed: got {}",
            value
        );
    } else {
        panic!(
            "to_hours should be a Duration after conversion, got {:?}",
            val
        );
    }
}

#[test]
fn test_percentage_properties() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact base: 200
fact rate: 10%
rule result: base * rate
"#;
    engine
        .load(code, lemma::SourceType::Labeled("test"))
        .unwrap();

    let result = get_rule_result(&mut engine, "test", "result");

    let val = result
        .value()
        .expect("Expected value result, got veto")
        .clone();

    if let ValueKind::Number(val) = &val.value {
        assert!(
            (val - Decimal::from_str("20").unwrap()).abs() < Decimal::from_str("0.01").unwrap(),
            "Percentage calculation failed"
        );
    }
}

#[test]
fn test_inverse_operations() {
    let test_values = vec![(10.0, 5.0), (100.0, 25.0), (7.5, 2.5)];

    for (a, b) in test_values {
        let mut engine = Engine::new();
        let code = format!(
            r#"
spec test
fact a: {}
fact b: {}
rule sum: a + b
rule back_sub: sum - b
rule product: a * b
rule back_div: product / b
"#,
            a, b
        );
        engine
            .load(&code, lemma::SourceType::Labeled("test"))
            .unwrap();

        let result = get_rule_result(&mut engine, "test", "back_sub");

        let val = result
            .value()
            .expect("Expected value result, got veto")
            .clone();

        if let ValueKind::Number(val) = &val.value {
            let expected = Decimal::from_f64(a).unwrap();
            assert!(
                (val - expected).abs() < Decimal::from_str("0.001").unwrap(),
                "Subtraction inverse failed for ({}, {})",
                a,
                b
            );
        }

        let result = get_rule_result(&mut engine, "test", "back_div");

        let val = result
            .value()
            .expect("Expected value result, got veto")
            .clone();

        if let ValueKind::Number(val) = &val.value {
            let expected = Decimal::from_f64(a).unwrap();
            assert!(
                (val - expected).abs() < Decimal::from_str("0.01").unwrap(),
                "Division inverse failed for ({}, {})",
                a,
                b
            );
        }
    }
}
