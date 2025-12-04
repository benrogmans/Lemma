//! Type-aware comparison operations
//!
//! Handles comparisons on different types: Number, Text, Boolean, Percentage, Unit.
//! Returns OperationResult with Veto for runtime errors instead of Result.

use super::result::OperationResult;
use crate::{ComparisonComputation, LiteralValue};
use rust_decimal::Decimal;

/// Perform type-aware comparison, returning OperationResult (Veto on error)
pub fn comparison_operation(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (left, right) {
        (LiteralValue::Number(l), LiteralValue::Number(r)) => {
            OperationResult::Value(LiteralValue::Boolean(compare_decimals(*l, op, r).into()))
        }

        (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => {
            if op.is_equal() {
                OperationResult::Value(LiteralValue::Boolean((l == r).into()))
            } else if op.is_not_equal() {
                OperationResult::Value(LiteralValue::Boolean((l != r).into()))
            } else {
                OperationResult::Veto(Some("Can only use == and != with booleans".to_string()))
            }
        }

        (LiteralValue::Text(l), LiteralValue::Text(r)) => {
            if op.is_equal() {
                OperationResult::Value(LiteralValue::Boolean((l == r).into()))
            } else if op.is_not_equal() {
                OperationResult::Value(LiteralValue::Boolean((l != r).into()))
            } else {
                OperationResult::Veto(Some("Can only use == and != with text".to_string()))
            }
        }

        (LiteralValue::Percentage(l), LiteralValue::Percentage(r)) => {
            OperationResult::Value(LiteralValue::Boolean(compare_decimals(*l, op, r).into()))
        }

        (LiteralValue::Date(_), LiteralValue::Date(_)) => {
            super::datetime::datetime_comparison(left, op, right)
        }

        // Unit types with the same category can be compared
        // Convert both to base units first to ensure correct comparison
        (LiteralValue::Unit(l), LiteralValue::Unit(r)) if l.same_category(r) => {
            let left_base = super::units::to_base_unit_value(l);
            let right_base = super::units::to_base_unit_value(r);
            OperationResult::Value(LiteralValue::Boolean(
                compare_decimals(left_base, op, &right_base).into(),
            ))
        }

        // Comparing unit to number extracts the unit's value for comparison
        (LiteralValue::Unit(u), LiteralValue::Number(n)) => OperationResult::Value(
            LiteralValue::Boolean(compare_decimals(u.value(), op, n).into()),
        ),
        (LiteralValue::Number(n), LiteralValue::Unit(u)) => OperationResult::Value(
            LiteralValue::Boolean(compare_decimals(*n, op, &u.value()).into()),
        ),

        // Different category units: compare numeric values
        (LiteralValue::Unit(l), LiteralValue::Unit(r)) => OperationResult::Value(
            LiteralValue::Boolean(compare_decimals(l.value(), op, &r.value()).into()),
        ),

        _ => OperationResult::Veto(Some(format!(
            "Comparison {:?} not supported for types {:?} and {:?}",
            op,
            type_name(left),
            type_name(right)
        ))),
    }
}

fn compare_decimals(left: Decimal, op: &ComparisonComputation, right: &Decimal) -> bool {
    match op {
        ComparisonComputation::GreaterThan => left > *right,
        ComparisonComputation::LessThan => left < *right,
        ComparisonComputation::GreaterThanOrEqual => left >= *right,
        ComparisonComputation::LessThanOrEqual => left <= *right,
        ComparisonComputation::Equal(_) => left == *right,
        ComparisonComputation::NotEqual(_) => left != *right,
    }
}

fn type_name(value: &LiteralValue) -> String {
    value.to_type().to_string()
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use std::collections::HashMap;

    #[test]
    fn test_equal_operator_numbers() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test_equal_numbers

fact a = 42
fact b = 42
fact c = 100

rule equal_true = a == b
rule equal_false = a == c
"#,
                "test.lemma",
            )
            .unwrap();

        let response = engine
            .evaluate("test_equal_numbers", vec![], HashMap::new())
            .unwrap();

        let equal_true = response.results.get("equal_true").unwrap();
        assert_eq!(equal_true.result.value().unwrap().to_string(), "true");

        let equal_false = response.results.get("equal_false").unwrap();
        assert_eq!(equal_false.result.value().unwrap().to_string(), "false");
    }

    #[test]
    fn test_equal_operator_text() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test_equal_text

fact greeting = "hello"
fact other = "world"

rule same_greeting = greeting == "hello"
rule different_greeting = greeting == other
"#,
                "test.lemma",
            )
            .unwrap();

        let response = engine
            .evaluate("test_equal_text", vec![], HashMap::new())
            .unwrap();

        let same = response.results.get("same_greeting").unwrap();
        assert_eq!(same.result.value().unwrap().to_string(), "true");

        let different = response.results.get("different_greeting").unwrap();
        assert_eq!(different.result.value().unwrap().to_string(), "false");
    }

    #[test]
    fn test_same_unit_mass_comparison() {
        let code = r#"
doc test
fact weight1 = 3 kilograms
fact weight2 = 300 grams

rule heavier = weight1 > weight2
rule lighter = weight1 < weight2
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();
        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

        let heavier = response.results.get("heavier").unwrap();
        assert_eq!(heavier.result.value().unwrap().to_string(), "true");

        let lighter = response.results.get("lighter").unwrap();
        assert_eq!(lighter.result.value().unwrap().to_string(), "false");
    }

    #[test]
    fn test_same_unit_length_comparison() {
        let code = r#"
doc test
fact distance1 = 100 meters
fact distance2 = 1 kilometer

rule shorter = distance1 < distance2
rule equal = 1000 meters == 1 kilometer
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();
        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

        let shorter = response.results.get("shorter").unwrap();
        assert_eq!(shorter.result.value().unwrap().to_string(), "true");

        let equal = response.results.get("equal").unwrap();
        assert_eq!(equal.result.value().unwrap().to_string(), "true");
    }

    #[test]
    fn test_same_unit_duration_comparison() {
        let code = r#"
doc test
fact time1 = 90 seconds
fact time2 = 2 minutes

rule less_time = time1 < time2
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();
        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

        let less_time = response.results.get("less_time").unwrap();
        assert_eq!(less_time.result.value().unwrap().to_string(), "true");
    }

    #[test]
    fn test_cross_category_comparison() {
        let code = r#"
doc test
fact weight = 5 kilograms
fact distance = 3 meters

rule weight_greater = weight > distance
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();
        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

        let weight_greater = response.results.get("weight_greater").unwrap();
        assert_eq!(weight_greater.result.value().unwrap().to_string(), "true");
    }

    #[test]
    fn test_unit_vs_number_comparison() {
        let code = r#"
doc test
fact weight = 5 kilograms

rule greater_than_3 = weight > 3
rule less_than_10 = weight < 10
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();
        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

        let greater = response.results.get("greater_than_3").unwrap();
        assert_eq!(greater.result.value().unwrap().to_string(), "true");

        let less = response.results.get("less_than_10").unwrap();
        assert_eq!(less.result.value().unwrap().to_string(), "true");
    }

    #[test]
    fn test_temperature_comparison_with_conversion() {
        let code = r#"
doc test
fact temp1 = 0 celsius
fact temp2 = 32 fahrenheit

rule same_temp = temp1 == temp2
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();
        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

        let same_temp = response.results.get("same_temp").unwrap();
        assert_eq!(same_temp.result.value().unwrap().to_string(), "true");
    }

    #[test]
    fn test_power_comparison_with_conversion() {
        let code = r#"
doc test
fact power1 = 500 watts
fact power2 = 1 kilowatt

rule less_power = power1 < power2
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();
        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();

        let less_power = response.results.get("less_power").unwrap();
        assert_eq!(less_power.result.value().unwrap().to_string(), "true");
    }

    #[test]
    fn test_equal_operator_money() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test_equal_money

fact price_a = 100
fact price_b = 100
fact price_c = 50

rule same_price = price_a == price_b
rule different_price = price_a == price_c
"#,
                "test.lemma",
            )
            .unwrap();

        let response = engine
            .evaluate("test_equal_money", vec![], HashMap::new())
            .unwrap();

        let same = response.results.get("same_price").unwrap();
        assert_eq!(same.result.value().unwrap().to_string(), "true");

        let different = response.results.get("different_price").unwrap();
        assert_eq!(different.result.value().unwrap().to_string(), "false");
    }

    #[test]
    fn test_equal_operator_booleans() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test_equal_booleans

fact flag_a = true
fact flag_b = true
fact flag_c = false

rule both_true = flag_a == flag_b
rule mixed = flag_a == flag_c
"#,
                "test.lemma",
            )
            .unwrap();

        let response = engine
            .evaluate("test_equal_booleans", vec![], HashMap::new())
            .unwrap();

        let both_true = response.results.get("both_true").unwrap();
        assert_eq!(both_true.result.value().unwrap().to_string(), "true");

        let mixed = response.results.get("mixed").unwrap();
        assert_eq!(mixed.result.value().unwrap().to_string(), "false");
    }

    #[test]
    fn test_equal_operator_in_conditions() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
doc test_equal_conditions

fact status = "active"
fact count = 10

rule message = "inactive"
  unless status == "active" then "active"
  unless count == 10 then "count is 10"
"#,
                "test.lemma",
            )
            .unwrap();

        let response = engine
            .evaluate("test_equal_conditions", vec![], HashMap::new())
            .unwrap();

        let message = response.results.get("message").unwrap();
        assert_eq!(
            message.result.value().unwrap().to_string(),
            "\"count is 10\""
        );
    }
}
