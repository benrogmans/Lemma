//! Type-aware arithmetic operations
//!
//! Handles arithmetic on different types: Number, Percentage, Unit, Date, Time.
//! Returns OperationResult with Veto for runtime errors instead of Result.

use super::result::OperationResult;
use crate::{ArithmeticComputation, LiteralValue};
use rust_decimal::Decimal;

const PERCENT_DENOMINATOR: i32 = 100;

/// Check for division by zero, returning Veto if divisor is zero
fn check_division_by_zero(divisor: &Decimal) -> Option<OperationResult> {
    if *divisor == Decimal::ZERO {
        Some(OperationResult::Veto(Some("Division by zero".to_string())))
    } else {
        None
    }
}

/// Perform type-aware arithmetic operation, returning OperationResult (Veto on error)
pub fn arithmetic_operation(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (left, right) {
        (LiteralValue::Number(l), LiteralValue::Number(r)) => match number_arithmetic(*l, op, *r) {
            Ok(result) => OperationResult::Value(LiteralValue::Number(result)),
            Err(msg) => OperationResult::Veto(Some(msg)),
        },

        (LiteralValue::Percentage(l), LiteralValue::Number(r)) => match op {
            ArithmeticComputation::Multiply => OperationResult::Value(LiteralValue::Number(
                l * r / Decimal::from(PERCENT_DENOMINATOR),
            )),
            ArithmeticComputation::Divide => {
                if let Some(veto) = check_division_by_zero(r) {
                    return veto;
                }
                OperationResult::Value(LiteralValue::Percentage(l / r))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for percentage and number",
                op
            ))),
        },

        (LiteralValue::Number(n), LiteralValue::Percentage(p)) => match op {
            ArithmeticComputation::Multiply => OperationResult::Value(LiteralValue::Number(
                n * p / Decimal::from(PERCENT_DENOMINATOR),
            )),
            ArithmeticComputation::Add => OperationResult::Value(LiteralValue::Number(
                n + (n * p / Decimal::from(PERCENT_DENOMINATOR)),
            )),
            ArithmeticComputation::Subtract => OperationResult::Value(LiteralValue::Number(
                n - (n * p / Decimal::from(PERCENT_DENOMINATOR)),
            )),
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for number and percentage",
                op
            ))),
        },

        (LiteralValue::Percentage(l), LiteralValue::Percentage(r)) => match op {
            ArithmeticComputation::Add => OperationResult::Value(LiteralValue::Percentage(l + r)),
            ArithmeticComputation::Subtract => {
                OperationResult::Value(LiteralValue::Percentage(l - r))
            }
            ArithmeticComputation::Multiply => OperationResult::Value(LiteralValue::Percentage(
                l * r / Decimal::from(PERCENT_DENOMINATOR),
            )),
            ArithmeticComputation::Divide => {
                if let Some(veto) = check_division_by_zero(r) {
                    return veto;
                }
                OperationResult::Value(LiteralValue::Number(l / r))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for percentage and percentage",
                op
            ))),
        },

        (LiteralValue::Date(_), _) | (_, LiteralValue::Date(_)) => {
            super::datetime::datetime_arithmetic(left, op, right)
        }

        (LiteralValue::Time(_), _) | (_, LiteralValue::Time(_)) => {
            super::datetime::time_arithmetic(left, op, right)
        }

        // Same category unit operations (e.g., Length + Length)
        // Convert to base units for correct arithmetic, then back to left unit type
        (LiteralValue::Unit(l), LiteralValue::Unit(r)) if l.same_category(r) => {
            let left_base = super::units::to_base_unit_value(l);
            let right_base = super::units::to_base_unit_value(r);

            match op {
                ArithmeticComputation::Add => {
                    // Add in base units, then convert back to left's unit
                    let result_base = left_base + right_base;
                    let left_value = l.value();
                    let left_base_value = super::units::to_base_unit_value(l);
                    // Conversion factor: left_value / left_base_value
                    // result_in_left_unit = result_base * (left_value / left_base_value)
                    let result_value = if left_base_value == Decimal::ZERO {
                        result_base
                    } else {
                        result_base * left_value / left_base_value
                    };
                    OperationResult::Value(LiteralValue::Unit(l.with_value(result_value)))
                }
                ArithmeticComputation::Subtract => {
                    let result_base = left_base - right_base;
                    let left_value = l.value();
                    let left_base_value = super::units::to_base_unit_value(l);
                    let result_value = if left_base_value == Decimal::ZERO {
                        result_base
                    } else {
                        result_base * left_value / left_base_value
                    };
                    OperationResult::Value(LiteralValue::Unit(l.with_value(result_value)))
                }
                ArithmeticComputation::Multiply => {
                    OperationResult::Value(LiteralValue::Number(left_base * right_base))
                }
                ArithmeticComputation::Divide => {
                    if let Some(veto) = check_division_by_zero(&right_base) {
                        return veto;
                    }
                    OperationResult::Value(LiteralValue::Number(left_base / right_base))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for same-category units",
                    op
                ))),
            }
        }

        // Different category unit operations produce dimensionless numbers
        (LiteralValue::Unit(l), LiteralValue::Unit(r)) => match op {
            ArithmeticComputation::Multiply => {
                OperationResult::Value(LiteralValue::Number(l.value() * r.value()))
            }
            ArithmeticComputation::Divide => {
                if let Some(veto) = check_division_by_zero(&r.value()) {
                    return veto;
                }
                OperationResult::Value(LiteralValue::Number(l.value() / r.value()))
            }
            _ => OperationResult::Veto(Some(format!(
                "Cannot add/subtract different unit categories: {:?} and {:?}",
                type_name(left),
                type_name(right)
            ))),
        },

        // Number and Unit operations
        (LiteralValue::Number(n), LiteralValue::Unit(u)) => match op {
            ArithmeticComputation::Multiply => {
                OperationResult::Value(LiteralValue::Unit(u.with_value(*n * u.value())))
            }
            ArithmeticComputation::Divide => {
                if let Some(veto) = check_division_by_zero(&u.value()) {
                    return veto;
                }
                OperationResult::Value(LiteralValue::Number(*n / u.value()))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for number and unit",
                op
            ))),
        },

        (LiteralValue::Unit(u), LiteralValue::Number(n)) => match op {
            ArithmeticComputation::Multiply => {
                OperationResult::Value(LiteralValue::Unit(u.with_value(u.value() * *n)))
            }
            ArithmeticComputation::Divide => {
                if let Some(veto) = check_division_by_zero(n) {
                    return veto;
                }
                OperationResult::Value(LiteralValue::Unit(u.with_value(u.value() / *n)))
            }
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => OperationResult::Veto(
                Some("Cannot add/subtract number and unit directly".to_string()),
            ),
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for unit and number",
                op
            ))),
        },

        _ => OperationResult::Veto(Some(format!(
            "Arithmetic operation {:?} not supported for types {:?} and {:?}",
            op,
            type_name(left),
            type_name(right)
        ))),
    }
}

fn number_arithmetic(
    left: Decimal,
    op: &ArithmeticComputation,
    right: Decimal,
) -> Result<Decimal, String> {
    use rust_decimal::prelude::ToPrimitive;

    match op {
        ArithmeticComputation::Add => Ok(left + right),
        ArithmeticComputation::Subtract => Ok(left - right),
        ArithmeticComputation::Multiply => Ok(left * right),
        ArithmeticComputation::Divide => {
            if right == Decimal::ZERO {
                return Err("Division by zero".to_string());
            }
            Ok(left / right)
        }
        ArithmeticComputation::Modulo => {
            if right == Decimal::ZERO {
                return Err("Division by zero (modulo)".to_string());
            }
            Ok(left % right)
        }
        ArithmeticComputation::Power => {
            let base = left
                .to_f64()
                .ok_or_else(|| "Cannot convert base to float".to_string())?;
            let exp = right
                .to_f64()
                .ok_or_else(|| "Cannot convert exponent to float".to_string())?;
            let result = base.powf(exp);
            Decimal::from_f64_retain(result)
                .ok_or_else(|| "Power result cannot be represented".to_string())
        }
    }
}

fn type_name(value: &LiteralValue) -> String {
    value.to_type().to_string()
}

#[cfg(test)]
mod tests {
    use crate::{Engine, LemmaError, LemmaResult};
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
        s.parse::<Decimal>()
            .map_err(|e| LemmaError::Engine(format!("Failed to parse '{s}' as Decimal: {e}")))
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
        Decimal::new(1, scale)
    }

    #[test]
    fn test_modulo_simple() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test
fact a = 10
fact b = 3
rule remainder = a % b
"#,
                "test",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        let result = response.results.get("remainder").unwrap();

        match &result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(n, &Decimal::from_str("1").unwrap())
            }
            _ => panic!("Expected number, got {:?}", result.result),
        }
    }

    #[test]
    fn test_power_simple() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test
fact base = 2
fact exponent = 3
rule result = base ^ exponent
"#,
                "test",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        let result = response.results.get("result").unwrap();

        match &result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(n, &Decimal::from_str("8").unwrap())
            }
            _ => panic!("Expected number, got {:?}", result.result),
        }
    }

    #[test]
    fn test_modulo_in_expression() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test
fact value = 17
rule is_even = (value % 2) == 0
rule is_odd = (value % 2) == 1
"#,
                "test",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

        let is_even = response.results.get("is_even").unwrap();
        assert_eq!(
            is_even.result,
            crate::OperationResult::Value(crate::LiteralValue::Boolean(crate::BooleanValue::False))
        );

        let is_odd = response.results.get("is_odd").unwrap();
        assert_eq!(
            is_odd.result,
            crate::OperationResult::Value(crate::LiteralValue::Boolean(crate::BooleanValue::True))
        );
    }

    #[test]
    fn test_power_with_fractions() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test
fact base = 4
rule square_root = base ^ 0.5
"#,
                "test",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        let result = response.results.get("square_root").unwrap();

        match &result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(n, &Decimal::from_str("2").unwrap())
            }
            _ => panic!("Expected number, got {:?}", result.result),
        }
    }

    #[test]
    fn test_combined_operations() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test
fact x = 10
fact y = 3
rule calculation = (x % y) + (2 ^ 3)
"#,
                "test",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        let result = response.results.get("calculation").unwrap();

        match &result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(n, &Decimal::from_str("9").unwrap())
            }
            _ => panic!("Expected number, got {:?}", result.result),
        }
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
        assert_close_dec(&a, &dec("2.718281828459045"), &tol(9));
        assert_eq!(b, Decimal::from(8));
        Ok(())
    }

    #[test]
    fn test_abs_floor_ceil_round() -> LemmaResult<()> {
        let code = r#"
    doc test
    rule a = abs(-3.5)
    rule b = floor 3.9
    rule c = ceil 3.1
    rule d = round 3.5
    rule e = round -3.5
    "#;
        assert_eq!(run(code, "a")?, "3.5");
        assert_eq!(run(code, "b")?, "3");
        assert_eq!(run(code, "c")?, "4");
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
        let bb = run_num(code, "bb")?;
        assert_close_dec(&bb, &dec("2"), &tol(9));
        let c = run_num(code, "c")?;
        assert_close_dec(&c, &dec("1"), &tol(9));
        assert_eq!(run_num(code, "d")?, Decimal::from(0));
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
    rule a = round(abs(-3.6))
    rule b = ceil (sqrt 2)
    rule c = floor (exp 1)
    "#;
        assert_eq!(run(code, "a")?, "4");
        assert_eq!(run(code, "b")?, "2");
        assert_eq!(run(code, "c")?, "2");
        Ok(())
    }

    #[test]
    fn test_sqrt_negative_and_log_domain_errors() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc test
        rule bad_sqrt = sqrt(-1)
        rule bad_log0 = log 0
        rule bad_log_neg = log -5
    "#,
                "test.lemma",
            )
            .unwrap();

        let res1 = engine
            .evaluate("test", vec!["bad_sqrt".to_string()], HashMap::new())
            .unwrap();
        let rule = res1
            .results
            .values()
            .find(|r| r.rule.name == "bad_sqrt")
            .unwrap();
        match &rule.result {
            crate::OperationResult::Veto(_) => {
                // Expected - sqrt(-1) should return Veto
            }
            other => panic!("sqrt(-1) should return Veto, got: {:?}", other),
        }

        let res2 = engine
            .evaluate("test", vec!["bad_log0".to_string()], HashMap::new())
            .unwrap();
        let rule = res2
            .results
            .values()
            .find(|r| r.rule.name == "bad_log0")
            .unwrap();
        match &rule.result {
            crate::OperationResult::Veto(_) => {
                // Expected - log 0 should return Veto
            }
            other => panic!("log 0 should return Veto, got: {:?}", other),
        }

        let res3 = engine
            .evaluate("test", vec!["bad_log_neg".to_string()], HashMap::new())
            .unwrap();
        let rule = res3
            .results
            .values()
            .find(|r| r.rule.name == "bad_log_neg")
            .unwrap();
        match &rule.result {
            crate::OperationResult::Veto(_) => {
                // Expected - log negative should return Veto
            }
            other => panic!("log negative should return Veto, got: {:?}", other),
        }
    }

    #[test]
    fn test_inverse_trig_domain_error() {
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
        match &rule.result {
            crate::OperationResult::Veto(_) => {
                // Expected - asin 2 should return Veto (domain error)
            }
            other => panic!("asin 2 should return Veto, got: {:?}", other),
        }
    }

    #[test]
    fn test_unit_subtract_percentage() -> LemmaResult<()> {
        let mut engine = Engine::new();

        engine.add_lemma_code(
            r#"
        doc pricing

        fact quantity = 10
        fact is_vip = false

        rule discount = 0%
            unless quantity >= 10 then 10%
            unless quantity >= 50 then 20%
            unless is_vip then 25%

        rule price = 200 - discount?
        "#,
            "pricing.lemma",
        )?;

        let response = engine.evaluate("pricing", vec![], HashMap::new())?;

        let discount_result = response
            .results
            .values()
            .find(|r| r.rule.name == "discount")
            .expect("discount rule not found");

        match &discount_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Percentage(p)) => {
                assert_eq!(*p, dec("10"), "discount should be exactly 10%");
            }
            other => panic!("Expected percentage for discount, got: {:?}", other),
        }

        let price_result = response
            .results
            .values()
            .find(|r| r.rule.name == "price")
            .expect("price rule not found");

        match &price_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("180"), "price should be exactly 180 (200 - 10%)");
            }
            other => panic!("Expected number for price, got: {:?}", other),
        }

        Ok(())
    }

    #[test]
    fn test_unit_add_percentage() -> LemmaResult<()> {
        let mut engine = Engine::new();

        engine.add_lemma_code(
            r#"
        doc tax_calculation

        fact base_price = 100
        fact tax_rate = 8.5%

        rule price_with_tax = base_price + tax_rate
        "#,
            "tax.lemma",
        )?;

        let response = engine.evaluate("tax_calculation", vec![], HashMap::new())?;

        let result = response
            .results
            .values()
            .find(|r| r.rule.name == "price_with_tax")
            .expect("price_with_tax rule not found");

        match &result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("108.5"), "price_with_tax should be exactly 108.5");
            }
            other => panic!("Expected number for price_with_tax, got: {:?}", other),
        }

        Ok(())
    }

    #[test]
    fn test_various_unit_percentage_operations() -> LemmaResult<()> {
        let mut engine = Engine::new();

        engine.add_lemma_code(
            r#"
        doc unit_percentage_ops

        fact price = 50
        fact increase = 20%
        fact decrease = 15%

        rule increased = price + increase
        rule decreased = price - decrease
        rule scaled = price * increase
        "#,
            "ops.lemma",
        )?;

        let response = engine.evaluate("unit_percentage_ops", vec![], HashMap::new())?;

        let increased_result = response
            .results
            .values()
            .find(|r| r.rule.name == "increased")
            .expect("increased rule not found");

        match &increased_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("60"), "50 + 20% should be exactly 60");
            }
            other => panic!("Expected number for increased, got: {:?}", other),
        }

        let decreased_result = response
            .results
            .values()
            .find(|r| r.rule.name == "decreased")
            .expect("decreased rule not found");

        match &decreased_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("42.50"), "50 - 15% should be exactly 42.50");
            }
            other => panic!("Expected number for decreased, got: {:?}", other),
        }

        let scaled_result = response
            .results
            .values()
            .find(|r| r.rule.name == "scaled")
            .expect("scaled rule not found");

        match &scaled_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("10"), "50 * 20% should be exactly 10");
            }
            other => panic!("Expected number for scaled, got: {:?}", other),
        }

        Ok(())
    }

    #[test]
    fn test_complex_discount_scenario() -> LemmaResult<()> {
        let mut engine = Engine::new();

        engine.add_lemma_code(
            r#"
        doc complex_pricing

        fact base_price = 1000
        fact bulk_discount = 15%
        fact loyalty_discount = 5%

        rule after_bulk = base_price - bulk_discount
        rule final_price = after_bulk? - loyalty_discount
        "#,
            "complex.lemma",
        )?;

        let response = engine.evaluate("complex_pricing", vec![], HashMap::new())?;

        let after_bulk_result = response
            .results
            .values()
            .find(|r| r.rule.name == "after_bulk")
            .expect("after_bulk rule not found");

        match &after_bulk_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("850"), "1000 - 15% should be exactly 850");
            }
            other => panic!("Expected number for after_bulk, got: {:?}", other),
        }

        let final_price_result = response
            .results
            .values()
            .find(|r| r.rule.name == "final_price")
            .expect("final_price rule not found");

        match &final_price_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("807.50"), "850 - 5% should be exactly 807.50");
            }
            other => panic!("Expected number for final_price, got: {:?}", other),
        }

        Ok(())
    }

    #[test]
    fn test_percentage_arithmetic() -> LemmaResult<()> {
        let mut engine = Engine::new();

        engine.add_lemma_code(
            r#"
        doc percentage_ops

        fact discount_a = 5%
        fact discount_b = 10%
        fact tax_rate = 15%
        fact compound_rate = 20%

        rule combined_discount = discount_a + discount_b
        rule net_rate = tax_rate - discount_a
        rule compound = compound_rate * compound_rate
        rule ratio = compound_rate / discount_a
        "#,
            "percentage.lemma",
        )?;

        let response = engine.evaluate("percentage_ops", vec![], HashMap::new())?;

        let combined_result = response
            .results
            .values()
            .find(|r| r.rule.name == "combined_discount")
            .expect("combined_discount rule not found");

        match &combined_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Percentage(p)) => {
                assert_eq!(*p, dec("15"), "5% + 10% should be exactly 15%");
            }
            other => panic!(
                "Expected percentage for combined_discount, got: {:?}",
                other
            ),
        }

        let net_rate_result = response
            .results
            .values()
            .find(|r| r.rule.name == "net_rate")
            .expect("net_rate rule not found");

        match &net_rate_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Percentage(p)) => {
                assert_eq!(*p, dec("10"), "15% - 5% should be exactly 10%");
            }
            other => panic!("Expected percentage for net_rate, got: {:?}", other),
        }

        let compound_result = response
            .results
            .values()
            .find(|r| r.rule.name == "compound")
            .expect("compound rule not found");

        match &compound_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Percentage(p)) => {
                assert_eq!(*p, dec("4"), "20% * 20% should be exactly 4%");
            }
            other => panic!("Expected percentage for compound, got: {:?}", other),
        }

        let ratio_result = response
            .results
            .values()
            .find(|r| r.rule.name == "ratio")
            .expect("ratio rule not found");

        match &ratio_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("4"), "20% / 5% should be exactly 4");
            }
            other => panic!("Expected number for ratio, got: {:?}", other),
        }

        Ok(())
    }

    #[test]
    fn test_averaging_percentages() -> LemmaResult<()> {
        let mut engine = Engine::new();

        engine.add_lemma_code(
            r#"
        doc avg_percentages

        fact rate_a = 10%
        fact rate_b = 20%
        fact rate_c = 15%

        rule sum = rate_a + rate_b + rate_c
        rule average = sum? / 3
        "#,
            "avg.lemma",
        )?;

        let response = engine.evaluate("avg_percentages", vec![], HashMap::new())?;

        let sum_result = response
            .results
            .values()
            .find(|r| r.rule.name == "sum")
            .expect("sum rule not found");

        match &sum_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Percentage(p)) => {
                assert_eq!(*p, dec("45"), "10% + 20% + 15% should be exactly 45%");
            }
            other => panic!("Expected percentage for sum, got: {:?}", other),
        }

        let avg_result = response
            .results
            .values()
            .find(|r| r.rule.name == "average")
            .expect("average rule not found");

        match &avg_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Percentage(p)) => {
                assert_eq!(*p, dec("15"), "45% / 3 should be exactly 15%");
            }
            other => panic!("Expected percentage for average, got: {:?}", other),
        }

        Ok(())
    }

    #[test]
    fn test_money_minus_percentage() {
        let mut engine = Engine::new();

        let code = r#"
doc test_money_minus_percentage

fact base_price = 200
fact discount_rate = 25%

rule price_after_discount = base_price - discount_rate
rule expected = 150

rule test_passes = price_after_discount? == expected?
"#;

        engine.add_lemma_code(code, "test").unwrap();
        let response = engine
            .evaluate("test_money_minus_percentage", vec![], HashMap::new())
            .unwrap();

        let price_after_discount = response.results.get("price_after_discount").unwrap();
        assert_eq!(
            price_after_discount.result.value().unwrap().to_string(),
            "150"
        );

        let test_passes = response.results.get("test_passes").unwrap();
        match &test_passes.result {
            crate::OperationResult::Value(crate::LiteralValue::Boolean(b)) => {
                assert!(bool::from(b), "test_passes should be true");
            }
            other => panic!("test_passes should be boolean true, got: {:?}", other),
        }
    }

    #[test]
    fn test_money_plus_percentage() {
        let mut engine = Engine::new();

        let code = r#"
doc test_money_plus_percentage

fact base = 100
fact markup = 10%

rule price_with_markup = base + markup
rule expected = 110

rule test_passes = price_with_markup? == expected?
"#;

        engine.add_lemma_code(code, "test").unwrap();
        let response = engine
            .evaluate("test_money_plus_percentage", vec![], HashMap::new())
            .unwrap();

        let price_with_markup = response.results.get("price_with_markup").unwrap();
        match &price_with_markup.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("110"), "price_with_markup should be exactly 110");
            }
            other => panic!("price_with_markup should be 110 (number), got: {:?}", other),
        }

        let test_passes = response.results.get("test_passes").unwrap();
        match &test_passes.result {
            crate::OperationResult::Value(crate::LiteralValue::Boolean(b)) => {
                assert!(bool::from(b), "test_passes should be true");
            }
            other => panic!("test_passes should be boolean true, got: {:?}", other),
        }
    }

    #[test]
    fn test_number_times_percentage() {
        let mut engine = Engine::new();

        let code = r#"
doc test_number_times_percentage

fact amount = 1000
fact rate = 15%

rule result = amount * rate
rule expected = 150

rule test_passes = result? == expected?
"#;

        engine.add_lemma_code(code, "test").unwrap();
        let response = engine
            .evaluate("test_number_times_percentage", vec![], HashMap::new())
            .unwrap();

        let result = response.results.get("result").unwrap();
        match &result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("150"), "result should be exactly 150");
            }
            other => panic!("result should be 150 (number), got: {:?}", other),
        }

        let test_passes = response.results.get("test_passes").unwrap();
        match &test_passes.result {
            crate::OperationResult::Value(crate::LiteralValue::Boolean(b)) => {
                assert!(bool::from(b), "test_passes should be true");
            }
            other => panic!("test_passes should be boolean true, got: {:?}", other),
        }
    }

    #[test]
    fn test_money_minus_percentage_with_rule_reference() {
        let mut engine = Engine::new();

        let code = r#"
doc test_with_rule_reference

fact base_price = 200
fact discount_rate = 25%

rule discount_amount = base_price * discount_rate
rule final_price = base_price - discount_amount?
rule expected = 150

rule test_passes = final_price? == expected?
"#;

        engine.add_lemma_code(code, "test").unwrap();
        let response = engine
            .evaluate("test_with_rule_reference", vec![], HashMap::new())
            .unwrap();

        let discount_amount = response.results.get("discount_amount").unwrap();
        match &discount_amount.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("50"), "discount_amount should be exactly 50");
            }
            other => panic!("discount_amount should be 50 (number), got: {:?}", other),
        }

        let final_price = response.results.get("final_price").unwrap();
        match &final_price.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, dec("150"), "final_price should be exactly 150");
            }
            other => panic!("final_price should be 150 (number), got: {:?}", other),
        }
    }

    #[test]
    fn test_chained_percentage_operations() {
        let mut engine = Engine::new();

        let code = r#"
doc test_chained_percentages

fact original_price = 100
fact first_discount = 20%
fact second_discount = 10%

rule after_first = original_price - first_discount
rule after_second = after_first? - second_discount

rule expected = 72

rule test_passes = after_second? == expected?
"#;

        engine.add_lemma_code(code, "test").unwrap();
        let response = engine
            .evaluate("test_chained_percentages", vec![], HashMap::new())
            .unwrap();

        let after_first = response.results.get("after_first").unwrap();
        match &after_first.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(
                    *n,
                    dec("80"),
                    "after_first should be exactly 80 (100 - 20%)"
                );
            }
            other => panic!("after_first should be 80 (number), got: {:?}", other),
        }

        let after_second = response.results.get("after_second").unwrap();
        match &after_second.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(
                    *n,
                    dec("72"),
                    "after_second should be exactly 72 (80 - 10%)"
                );
            }
            other => panic!("after_second should be 72 (number), got: {:?}", other),
        }
    }

    #[test]
    fn test_logical_and_requires_boolean_operands() {
        let code = r#"
doc test
rule result = 5 and true
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Should reject non-boolean in 'and'");
        assert!(result.unwrap_err().to_string().contains("boolean"));
    }

    #[test]
    fn test_logical_or_requires_boolean_operands() {
        let code = r#"
doc test
rule result = "hello" or false
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Should reject non-boolean in 'or'");
        assert!(result.unwrap_err().to_string().contains("boolean"));
    }

    #[test]
    fn test_unless_condition_must_be_boolean() {
        let code = r#"
doc test
rule result = 10
  unless 5 then 20
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Unless condition must be boolean");
    }

    #[test]
    fn test_conversion_to_valid_unit() {
        let code = r#"
doc test
fact distance = 1000
rule km = distance in kilometers
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Valid unit conversion should pass: {:?}",
            result
        );
    }

    #[test]
    fn test_percentage_literal_type() {
        let code = r#"
doc test
fact rate = 15%
rule doubled = rate
  unless rate > 10% then 20%
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Percentage types should be consistent: {:?}",
            result
        );
    }

    #[test]
    fn test_text_number_comparison_allowed() {
        let code = r#"
doc test
fact name = "Alice"
fact age = 30
rule check = name == "Bob" and age > 25
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Text and number comparisons should be allowed separately: {:?}",
            result
        );
    }

    #[test]
    fn test_date_comparison() {
        let code = r#"
doc test
fact start = 2024-01-01
fact end = 2024-12-31
rule is_valid_range = end > start
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Date comparison should be allowed: {:?}",
            result
        );
    }

    #[test]
    fn test_all_unit_types_in_conversions() {
        let test_cases = vec![
            ("(value * 100) in kilograms", "Mass"),
            ("(value * 50) in meters", "Length"),
            ("(value * 200) in liters", "Volume"),
            ("(value * 60) in seconds", "Duration"),
            ("(value * 25) in celsius", "Temperature"),
            ("(value * 1000) in watts", "Power"),
            ("(value * 100) in newtons", "Force"),
            ("(value * 101325) in pascals", "Pressure"),
            ("(value * 1000) in joules", "Energy"),
            ("(value * 60) in hertz", "Frequency"),
            ("(value * 1024) in bytes", "Data"),
        ];

        for (conversion, unit_name) in test_cases {
            let code = format!(
                r#"
doc test
fact value = 1
rule converted = {}
"#,
                conversion
            );

            let mut engine = Engine::new();
            let result = engine.add_lemma_code(&code, "test.lemma");
            assert!(
                result.is_ok(),
                "{} conversion should work: {:?}",
                unit_name,
                result
            );
        }
    }

    #[test]
    fn test_percentage_conversion_from_number() {
        let code = r#"
doc test
fact ratio = 0.25
rule as_percentage = ratio in percentage
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Number to percentage conversion should work: {:?}",
            result
        );
    }

    #[test]
    fn test_veto_type_is_compatible_with_other_types() {
        let code = r#"
doc test
fact age = 15
rule result = 100
  unless age < 18 then veto "Too young"
  unless age > 65 then 50
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Veto should not conflict with other return types: {:?}",
            result
        );
    }

    #[test]
    fn test_mixed_text_and_number_not_allowed() {
        let code = r#"
doc test
fact flag = true
rule value = "default"
  unless flag then 42
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_err(),
            "Should reject mixing text and number types"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
            "Error message should contain type mismatch info: {}",
            err_msg
        );
    }

    #[test]
    fn test_mixed_date_and_number_not_allowed() {
        let code = r#"
doc test
fact use_date = true
rule value = 2024-01-01
  unless use_date then 100
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_err(),
            "Should reject mixing date and number types"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
            "Error message should contain type mismatch info: {}",
            err_msg
        );
    }

    #[test]
    fn test_same_category_units_allowed_in_rule() {
        let code = r#"
doc test
fact weight = 1000 grams
rule adjusted = weight
  unless weight > 500 grams then 2 kilograms
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Same category units should be allowed: {:?}",
            result
        );
    }

    #[test]
    fn test_boolean_consistency() {
        let code = r#"
doc test
fact x = 5
fact y = 10
rule check = x < y
  unless x == 0 then y > 0
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Boolean results should be consistent: {:?}",
            result
        );
    }

    #[test]
    fn test_arithmetic_result_type_inference() {
        let code = r#"
doc test
fact a = 10
fact b = 20
rule sum = a + b
  unless a == 0 then 0
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Arithmetic should infer number type: {:?}",
            result
        );
    }

    #[test]
    fn test_multiple_unless_clauses_type_consistency() {
        let code = r#"
doc test
fact x = 5
rule value = 10
  unless x < 0 then 0
  unless x > 100 then 100
  unless x == 5 then 5
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "All number branches should be consistent: {:?}",
            result
        );
    }

    #[test]
    fn test_multiple_unless_clauses_type_inconsistency() {
        let code = r#"
doc test
fact x = 5
rule value = 10
  unless x < 0 then 0
  unless x > 100 then "overflow"
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Mixed number/text should be rejected");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
            "Error message should contain type mismatch info: {}",
            err_msg
        );
    }

    #[test]
    fn test_conversion_changes_type() {
        let code = r#"
doc test
fact meters = 100
rule as_km = meters in kilometers
rule back_to_number = as_km
  unless as_km > 0 kilometers then 0
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Conversion should create distinct type");
    }

    #[test]
    fn test_rule_reference_type_propagation() {
        let code = r#"
doc test
fact base = 100
rule derived = base * 2
rule another = derived?
  unless derived? > 150 then 0
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Rule reference types should propagate: {:?}",
            result
        );
    }

    #[test]
    fn test_time_type_validation() {
        let code = r#"
doc test
fact meeting_time = 14:30:00
rule is_afternoon = meeting_time > 12:00:00
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Time type should be validated correctly: {:?}",
            result
        );
    }

    #[test]
    fn test_time_cannot_use_in_logical_operators() {
        let code = r#"
doc test
fact time1 = 14:30:00
fact time2 = 15:00:00
rule result = time1 and time2
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_err(),
            "Should reject time values in logical operators"
        );
        assert!(result.unwrap_err().to_string().contains("boolean"));
    }

    #[test]
    fn test_regex_type_validation() {
        let code = r#"
doc test
fact pattern = /[a-z]+/
fact text = "hello"
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Regex type should be validated correctly: {:?}",
            result
        );
    }

    #[test]
    fn test_regex_cannot_use_in_logical_operators() {
        let code = r#"
doc test
fact pattern1 = /[a-z]+/
fact pattern2 = /[0-9]+/
rule result = pattern1 and pattern2
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_err(),
            "Should reject regex values in logical operators"
        );
        assert!(result.unwrap_err().to_string().contains("boolean"));
    }

    #[test]
    fn test_mixed_time_and_number_not_allowed() {
        let code = r#"
doc test
fact use_time = true
rule value = 14:30:00
  unless use_time then 100
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_err(),
            "Should reject mixing time and number types"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
            "Error message should contain type mismatch info: {}",
            err_msg
        );
    }

    #[test]
    fn test_mixed_regex_and_text_not_allowed() {
        let code = r#"
doc test
fact use_pattern = true
rule value = /[a-z]+/
  unless use_pattern then "plain text"
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Should reject mixing regex and text types");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("incompatible") || err_msg.contains("Type mismatch"),
            "Error message should contain type mismatch info: {}",
            err_msg
        );
    }
}
