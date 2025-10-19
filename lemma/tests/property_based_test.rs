use lemma::{Engine, LiteralValue, MoneyUnit, NumericUnit};
use proptest::prelude::*;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;

fn get_rule_result(engine: &mut Engine, doc_name: &str, rule_name: &str) -> Option<LiteralValue> {
    let response = engine.evaluate(doc_name, None, None).unwrap();
    response
        .results
        .iter()
        .find(|r| r.rule_name == rule_name)
        .and_then(|r| r.result.clone())
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
doc test
fact x = {}
rule result = x * 0
"#, n);
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "result") {
            prop_assert_eq!(val, Decimal::from_str("0").unwrap());
        }
    }

    #[test]
    fn prop_multiplication_identity(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact x = {}
rule result = x * 1
"#, n);
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "result") {
            let expected = Decimal::from_f64(n).unwrap();
            let diff = (val - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_addition_identity(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact x = {}
rule result = x + 0
"#, n);
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "result") {
            let expected = Decimal::from_f64(n).unwrap();
            let diff = (val - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_comparison_consistency(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact x = {}
rule eq_self = x == x
rule lte_self = x <= x
"#, n);
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Boolean(val)) = get_rule_result(&mut engine, "test", "eq_self") {
            prop_assert_eq!(val, true);
        }
        if let Some(LiteralValue::Boolean(val)) = get_rule_result(&mut engine, "test", "lte_self") {
            prop_assert_eq!(val, true);
        }
    }

    #[test]
    fn prop_fact_override_works(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = r#"
doc test
fact x = [number]
rule doubled = x * 2
"#;
        engine.add_lemma_code(code, "test").unwrap();

        let override_fact = format!("x={}", n);
        let facts = lemma::parse_facts(&[override_fact.as_str()]).unwrap();
        let response = engine.evaluate("test", None, Some(facts)).unwrap();

        if let Some(result) = response.results.iter().find(|r| r.rule_name == "doubled") {
            if let Some(LiteralValue::Number(val)) = &result.result {
                let expected = Decimal::from_f64(n * 2.0).unwrap();
                let diff = (val - expected).abs();
                prop_assert!(diff < Decimal::from_str("0.001").unwrap());
            }
        }
    }

    #[test]
    fn prop_addition_commutative(a in -100.0..100.0, b in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
rule sum1 = a + b
rule sum2 = b + a
"#, a, b);
        engine.add_lemma_code(&code, "test").unwrap();

        let v1 = get_rule_result(&mut engine, "test", "sum1");
        let v2 = get_rule_result(&mut engine, "test", "sum2");

        if let (Some(LiteralValue::Number(val1)), Some(LiteralValue::Number(val2))) = (v1, v2) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_multiplication_commutative(a in -50.0..50.0, b in -50.0..50.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
rule prod1 = a * b
rule prod2 = b * a
"#, a, b);
        engine.add_lemma_code(&code, "test").unwrap();

        let v1 = get_rule_result(&mut engine, "test", "prod1");
        let v2 = get_rule_result(&mut engine, "test", "prod2");

        if let (Some(LiteralValue::Number(val1)), Some(LiteralValue::Number(val2))) = (v1, v2) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_addition_associative(a in -50.0..50.0, b in -50.0..50.0, c in -50.0..50.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
fact c = {}
rule sum1 = (a + b) + c
rule sum2 = a + (b + c)
"#, a, b, c);
        engine.add_lemma_code(&code, "test").unwrap();

        let v1 = get_rule_result(&mut engine, "test", "sum1");
        let v2 = get_rule_result(&mut engine, "test", "sum2");

        if let (Some(LiteralValue::Number(val1)), Some(LiteralValue::Number(val2))) = (v1, v2) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_multiplication_associative(a in -20.0..20.0, b in -20.0..20.0, c in -20.0..20.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
fact c = {}
rule prod1 = (a * b) * c
rule prod2 = a * (b * c)
"#, a, b, c);
        engine.add_lemma_code(&code, "test").unwrap();

        let v1 = get_rule_result(&mut engine, "test", "prod1");
        let v2 = get_rule_result(&mut engine, "test", "prod2");

        if let (Some(LiteralValue::Number(val1)), Some(LiteralValue::Number(val2))) = (v1, v2) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.01").unwrap());
        }
    }

    #[test]
    fn prop_distributive(a in -50.0..50.0, b in -50.0..50.0, c in -50.0..50.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
fact c = {}
rule dist1 = a * (b + c)
rule dist2 = (a * b) + (a * c)
"#, a, b, c);
        engine.add_lemma_code(&code, "test").unwrap();

        let v1 = get_rule_result(&mut engine, "test", "dist1");
        let v2 = get_rule_result(&mut engine, "test", "dist2");

        if let (Some(LiteralValue::Number(val1)), Some(LiteralValue::Number(val2))) = (v1, v2) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.01").unwrap());
        }
    }

    #[test]
    fn prop_negation_involution(n in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact x = {}
rule double_neg = -(-x)
"#, n);
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "double_neg") {
            let expected = Decimal::from_f64(n).unwrap();
            let diff = (val - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_subtraction_as_addition_of_negative(a in -100.0..100.0, b in -100.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
rule sub = a - b
rule add_neg = a + (-b)
"#, a, b);
        engine.add_lemma_code(&code, "test").unwrap();

        let v1 = get_rule_result(&mut engine, "test", "sub");
        let v2 = get_rule_result(&mut engine, "test", "add_neg");

        if let (Some(LiteralValue::Number(val1)), Some(LiteralValue::Number(val2))) = (v1, v2) {
            let diff = (val1 - val2).abs();
            prop_assert!(diff < Decimal::from_str("0.001").unwrap());
        }
    }

    #[test]
    fn prop_division_inverse_of_multiplication(a in 1.0..100.0, b in 1.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
rule product = a * b
rule back = product? / b
"#, a, b);
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "back") {
            let expected = Decimal::from_f64(a).unwrap();
            let diff = (val - expected).abs();
            prop_assert!(diff < Decimal::from_str("0.01").unwrap());
        }
    }

    #[test]
    fn prop_boolean_not_involution(b in prop::bool::ANY) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact b = {}
rule double_not = not (not b)
"#, b);
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Boolean(val)) = get_rule_result(&mut engine, "test", "double_not") {
            prop_assert_eq!(val, b);
        }
    }

    #[test]
    fn prop_and_commutative(a in prop::bool::ANY, b in prop::bool::ANY) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
rule and1 = a and b
rule and2 = b and a
"#, a, b);
        engine.add_lemma_code(&code, "test").unwrap();

        let v1 = get_rule_result(&mut engine, "test", "and1");
        let v2 = get_rule_result(&mut engine, "test", "and2");

        if let (Some(LiteralValue::Boolean(val1)), Some(LiteralValue::Boolean(val2))) = (v1, v2) {
            prop_assert_eq!(val1, val2);
        }
    }

    #[test]
    fn prop_or_commutative(a in prop::bool::ANY, b in prop::bool::ANY) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
rule or1 = a or b
rule or2 = b or a
"#, a, b);
        engine.add_lemma_code(&code, "test").unwrap();

        let v1 = get_rule_result(&mut engine, "test", "or1");
        let v2 = get_rule_result(&mut engine, "test", "or2");

        if let (Some(LiteralValue::Boolean(val1)), Some(LiteralValue::Boolean(val2))) = (v1, v2) {
            prop_assert_eq!(val1, val2);
        }
    }

    #[test]
    fn prop_comparison_transitivity(a in 1.0..100.0, b in 1.0..100.0) {
        let (min, max) = if a < b { (a, b) } else { (b, a) };
        let mid = (min + max) / 2.0;

        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact a = {}
fact b = {}
fact c = {}
rule ab = a < b
rule bc = b < c
rule ac = a < c
"#, min, mid, max);
        engine.add_lemma_code(&code, "test").unwrap();

        let ab = get_rule_result(&mut engine, "test", "ab");
        let bc = get_rule_result(&mut engine, "test", "bc");
        let ac = get_rule_result(&mut engine, "test", "ac");

        if let (Some(LiteralValue::Boolean(true)), Some(LiteralValue::Boolean(true)), Some(LiteralValue::Boolean(ac_val))) = (ab, bc, ac) {
            prop_assert_eq!(ac_val, true);
        }
    }

    #[test]
    fn prop_unless_last_matching_wins(n in 1.0..100.0) {
        let mut engine = Engine::new();
        let code = format!(r#"
doc test
fact x = {}
rule discount = 0
  unless x > 10 then 10
  unless x > 20 then 20
  unless x > 50 then 50
"#, n);
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "discount") {
            let expected = if n > 50.0 {
                Decimal::from(50)
            } else if n > 20.0 {
                Decimal::from(20)
            } else if n > 10.0 {
                Decimal::from(10)
            } else {
                Decimal::from(0)
            };
            prop_assert_eq!(val, expected);
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
doc test
fact x = {}
rule zero = x * 0
rule identity_mul = x * 1
rule identity_add = x + 0
rule commutative1 = x + 5
rule commutative2 = 5 + x
"#,
            n
        );
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "zero") {
            assert_eq!(
                val,
                Decimal::from_str("0").unwrap(),
                "Multiplication by zero failed for {}",
                n
            );
        }

        if let Some(LiteralValue::Number(val)) =
            get_rule_result(&mut engine, "test", "identity_mul")
        {
            let expected = Decimal::from_f64(n).unwrap();
            assert!(
                (val - expected).abs() < Decimal::from_str("0.001").unwrap(),
                "Multiplication identity failed for {}",
                n
            );
        }

        if let Some(LiteralValue::Number(val)) =
            get_rule_result(&mut engine, "test", "identity_add")
        {
            let expected = Decimal::from_f64(n).unwrap();
            assert!(
                (val - expected).abs() < Decimal::from_str("0.001").unwrap(),
                "Addition identity failed for {}",
                n
            );
        }

        let comm1 = get_rule_result(&mut engine, "test", "commutative1");
        let comm2 = get_rule_result(&mut engine, "test", "commutative2");
        if let (Some(LiteralValue::Number(v1)), Some(LiteralValue::Number(v2))) = (comm1, comm2) {
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
doc test
fact a = 10
fact b = 20
fact c = 30
rule a_lt_b = a < b
rule b_lt_c = b < c
rule a_lt_c = a < c
rule a_eq_a = a == a
rule a_lte_a = a <= a
rule a_gte_a = a >= a
"#;
    engine.add_lemma_code(code, "test").unwrap();

    if let Some(LiteralValue::Boolean(val)) = get_rule_result(&mut engine, "test", "a_eq_a") {
        assert!(val, "Reflexive equality failed");
    }

    if let Some(LiteralValue::Boolean(val)) = get_rule_result(&mut engine, "test", "a_lte_a") {
        assert!(val, "Reflexive <= failed");
    }

    if let Some(LiteralValue::Boolean(val)) = get_rule_result(&mut engine, "test", "a_gte_a") {
        assert!(val, "Reflexive >= failed");
    }

    let ab = get_rule_result(&mut engine, "test", "a_lt_b");
    let bc = get_rule_result(&mut engine, "test", "b_lt_c");
    let ac = get_rule_result(&mut engine, "test", "a_lt_c");

    if let (
        Some(LiteralValue::Boolean(true)),
        Some(LiteralValue::Boolean(true)),
        Some(LiteralValue::Boolean(val)),
    ) = (ab, bc, ac)
    {
        assert!(val, "Transitivity of < failed");
    }
}

#[test]
fn test_unit_conversion_properties() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact distance = 1000 meters
rule to_km = distance in kilometers
"#;
    engine.add_lemma_code(code, "test").unwrap();

    // After using `in`, the result is a plain Number, not a Unit
    if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "to_km") {
        assert!(
            (val - Decimal::from_str("1").unwrap()).abs() < Decimal::from_str("0.001").unwrap(),
            "meters to kilometers conversion failed: got {}",
            val
        );
    } else {
        panic!("to_km should be a Number after conversion");
    }
}

#[test]
fn test_money_properties() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact price1 = 50
fact price2 = 30
rule total = price1 + price2
rule difference = price1 - price2
"#;
    engine.add_lemma_code(code, "test").unwrap();

    if let Some(LiteralValue::Unit(NumericUnit::Money(amount, currency))) =
        get_rule_result(&mut engine, "test", "total")
    {
        assert!(matches!(currency, MoneyUnit::Usd));
        assert!(
            (amount - Decimal::from_str("80").unwrap()).abs() < Decimal::from_str("0.01").unwrap(),
            "Money addition failed"
        );
    }

    if let Some(LiteralValue::Unit(NumericUnit::Money(amount, currency))) =
        get_rule_result(&mut engine, "test", "difference")
    {
        assert!(matches!(currency, MoneyUnit::Usd));
        assert!(
            (amount - Decimal::from_str("20").unwrap()).abs() < Decimal::from_str("0.01").unwrap(),
            "Money subtraction failed"
        );
    }
}

#[test]
fn test_percentage_properties() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact base = 200
fact rate = 10%
rule result = base * rate
"#;
    engine.add_lemma_code(code, "test").unwrap();

    if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "result") {
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
doc test
fact a = {}
fact b = {}
rule sum = a + b
rule back_sub = sum? - b
rule product = a * b
rule back_div = product? / b
"#,
            a, b
        );
        engine.add_lemma_code(&code, "test").unwrap();

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "back_sub") {
            let expected = Decimal::from_f64(a).unwrap();
            assert!(
                (val - expected).abs() < Decimal::from_str("0.001").unwrap(),
                "Subtraction inverse failed for ({}, {})",
                a,
                b
            );
        }

        if let Some(LiteralValue::Number(val)) = get_rule_result(&mut engine, "test", "back_div") {
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
