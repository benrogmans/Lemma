//! Type-aware arithmetic operations
//!
//! Pure functions for arithmetic on different types: Number, Money, Percentage, Duration, etc.

use crate::evaluation::OperationResult;
use crate::{ArithmeticComputation, LiteralValue};
use rust_decimal::Decimal;

const PERCENT_DENOMINATOR: i32 = 100;

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
                if *r == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
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
                if *r == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
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
                    if right_base == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
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
                if r.value() == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
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
                if u.value() == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
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
                if *n == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
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
