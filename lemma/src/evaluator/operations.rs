//! Type-aware arithmetic and comparison operations
//!
//! Handles operations on different types: Number, Money, Percentage, Duration, etc.

use crate::{ArithmeticOperation, ComparisonOperator, LemmaError, LemmaResult, LiteralValue};
use rust_decimal::Decimal;

// Percentage calculations: percentages are stored as numbers (e.g., 20 for 20%)
// To apply a percentage, divide by 100 (e.g., 20% of 100 = 100 * 20 / 100 = 20)
const PERCENT_DENOMINATOR: i32 = 100;

/// Perform type-aware arithmetic operation.
///
/// Handles operations between different types with appropriate semantics:
/// - Number + Number = Number
/// - Money + Money = Money (same currency)
/// - Number * Percentage = Number (applies percentage)
/// - Date + Duration = Date
/// - Time + Duration = Time
///
/// # Examples
/// ```text
/// 100 + 20 = 120
/// $50 + $30 = $80
/// 100 * 20% = 20
/// 100 + 20% = 120
/// 2024-01-15 + 5 days = 2024-01-20
/// ```
pub fn arithmetic_operation(
    left: &LiteralValue,
    op: &ArithmeticOperation,
    right: &LiteralValue,
) -> LemmaResult<LiteralValue> {
    match (left, right) {
        // Number arithmetic
        (LiteralValue::Number(l), LiteralValue::Number(r)) => {
            Ok(LiteralValue::Number(number_arithmetic(*l, op, *r)?))
        }

        // Unit arithmetic - unified handling for all unit types
        (LiteralValue::Unit(l_unit), LiteralValue::Unit(r_unit)) => {
            // Validate currency compatibility
            l_unit.validate_same_currency(r_unit)?;

            if l_unit.same_category(r_unit) {
                // Convert right to left's unit
                let right_converted = convert_to_matching_unit(right, l_unit)?;
                let r_value = match &right_converted {
                    LiteralValue::Unit(u) => u.value(),
                    _ => unreachable!(),
                };
                let result_value = number_arithmetic(l_unit.value(), op, r_value)?;
                Ok(LiteralValue::Unit(l_unit.with_value(result_value)))
            } else {
                // Different categories: produce dimensionless number
                Ok(LiteralValue::Number(number_arithmetic(
                    l_unit.value(),
                    op,
                    r_unit.value(),
                )?))
            }
        }

        // Unit op Number: produce unit
        (LiteralValue::Unit(unit), LiteralValue::Number(n)) => {
            let result_value = number_arithmetic(unit.value(), op, *n)?;
            Ok(LiteralValue::Unit(unit.with_value(result_value)))
        }

        // Number op Unit: produce unit
        (LiteralValue::Number(n), LiteralValue::Unit(unit)) => {
            let result_value = number_arithmetic(*n, op, unit.value())?;
            Ok(LiteralValue::Unit(unit.with_value(result_value)))
        }

        // Percentage operations - percentages are operators that apply to values
        (LiteralValue::Percentage(p), LiteralValue::Number(n)) => {
            match op {
                ArithmeticOperation::Multiply => {
                    // 20% * 100 = 20 (apply percentage)
                    Ok(LiteralValue::Number(
                        p * n / Decimal::from(PERCENT_DENOMINATOR),
                    ))
                }
                _ => Err(LemmaError::Engine(format!(
                    "Operation {:?} not supported for percentage and number",
                    op
                ))),
            }
        }
        (LiteralValue::Number(n), LiteralValue::Percentage(p)) => {
            match op {
                ArithmeticOperation::Multiply => {
                    // 100 * 20% = 20 (apply percentage)
                    Ok(LiteralValue::Number(
                        n * p / Decimal::from(PERCENT_DENOMINATOR),
                    ))
                }
                ArithmeticOperation::Add => {
                    // 100 + 20% = 120 (increase by percentage)
                    Ok(LiteralValue::Number(
                        n + (n * p / Decimal::from(PERCENT_DENOMINATOR)),
                    ))
                }
                ArithmeticOperation::Subtract => {
                    // 100 - 20% = 80 (decrease by percentage)
                    Ok(LiteralValue::Number(
                        n - (n * p / Decimal::from(PERCENT_DENOMINATOR)),
                    ))
                }
                _ => Err(LemmaError::Engine(format!(
                    "Operation {:?} not supported for number and percentage",
                    op
                ))),
            }
        }

        (LiteralValue::Percentage(p), LiteralValue::Unit(unit))
        | (LiteralValue::Unit(unit), LiteralValue::Percentage(p)) => match op {
            ArithmeticOperation::Multiply => {
                // Unit * Percentage = Unit scaled by percentage (e.g., 100 eur * 20% = 20 eur)
                let result_value = unit.value() * p / Decimal::from(PERCENT_DENOMINATOR);
                Ok(LiteralValue::Unit(unit.with_value(result_value)))
            }
            ArithmeticOperation::Add => {
                // Unit + Percentage = Unit increased by percentage (e.g., 100 eur + 20% = 120 eur)
                let increase = unit.value() * p / Decimal::from(PERCENT_DENOMINATOR);
                let result_value = unit.value() + increase;
                Ok(LiteralValue::Unit(unit.with_value(result_value)))
            }
            ArithmeticOperation::Subtract => {
                // Unit - Percentage = Unit decreased by percentage (e.g., 100 eur - 20% = 80 eur)
                let decrease = unit.value() * p / Decimal::from(PERCENT_DENOMINATOR);
                let result_value = unit.value() - decrease;
                Ok(LiteralValue::Unit(unit.with_value(result_value)))
            }
            _ => Err(LemmaError::Engine(format!(
                "Operation {:?} not supported for percentage and unit",
                op
            ))),
        },

        // Date arithmetic with duration
        (LiteralValue::Date(_), _) | (_, LiteralValue::Date(_)) => {
            super::datetime::datetime_arithmetic(left, op, right)
        }

        // Time arithmetic with duration
        (LiteralValue::Time(_), _) | (_, LiteralValue::Time(_)) => {
            super::datetime::time_arithmetic(left, op, right)
        }

        _ => Err(LemmaError::Engine(format!(
            "Arithmetic operation {:?} not supported for types {:?} and {:?}",
            op,
            type_name(left),
            type_name(right)
        ))),
    }
}

/// Perform basic number arithmetic, returning the numeric result
fn number_arithmetic(
    left: Decimal,
    op: &ArithmeticOperation,
    right: Decimal,
) -> LemmaResult<Decimal> {
    use rust_decimal::prelude::ToPrimitive;

    let result = match op {
        ArithmeticOperation::Add => left + right,
        ArithmeticOperation::Subtract => left - right,
        ArithmeticOperation::Multiply => left * right,
        ArithmeticOperation::Divide => {
            if right == Decimal::ZERO {
                return Err(LemmaError::Engine("Division by zero".to_string()));
            }
            left / right
        }
        ArithmeticOperation::Modulo => left % right,
        ArithmeticOperation::Power => {
            let base = left
                .to_f64()
                .ok_or_else(|| LemmaError::Engine("Cannot convert base to float".to_string()))?;
            let exp = right.to_f64().ok_or_else(|| {
                LemmaError::Engine("Cannot convert exponent to float".to_string())
            })?;
            let result = base.powf(exp);
            Decimal::from_f64_retain(result).ok_or_else(|| {
                LemmaError::Engine("Power result cannot be represented".to_string())
            })?
        }
    };

    Ok(result)
}

/// Perform type-aware comparison.
///
/// Handles comparisons between compatible types:
/// - Numbers can be compared with numbers
/// - Strings can be compared with strings
/// - Dates can be compared with dates (timezone-aware)
/// - Booleans can be compared with booleans
/// - Money can only be compared within the same currency
///
/// # Examples
/// ```text
/// 100 > 50 = true
/// "apple" < "banana" = true
/// 2024-01-15 > 2024-01-10 = true
/// $100 > $50 = true (same currency)
/// ```
pub fn comparison_operation(
    left: &LiteralValue,
    op: &ComparisonOperator,
    right: &LiteralValue,
) -> LemmaResult<bool> {
    match (left, right) {
        // Number comparisons
        (LiteralValue::Number(l), LiteralValue::Number(r)) => Ok(compare_decimals(*l, op, r)),

        // Unit > Unit
        (LiteralValue::Unit(l_unit), LiteralValue::Unit(r_unit)) => {
            // Validate currency compatibility
            l_unit.validate_same_currency(r_unit)?;

            if l_unit.same_category(r_unit) {
                // Convert right to left's unit, then compare
                let right_converted = convert_to_matching_unit(right, l_unit)?;
                let r_value = match &right_converted {
                    LiteralValue::Unit(u) => u.value(),
                    LiteralValue::Number(n) => *n, // Duration conversion returns Number
                    _ => return Err(LemmaError::Engine("Invalid unit conversion".to_string())),
                };
                Ok(compare_decimals(l_unit.value(), op, &r_value))
            } else {
                // Different categories: compare numeric values directly
                Ok(compare_decimals(l_unit.value(), op, &r_unit.value()))
            }
        }

        // Unit > Number: extract value and compare
        (LiteralValue::Unit(unit), LiteralValue::Number(n))
        | (LiteralValue::Number(n), LiteralValue::Unit(unit)) => {
            Ok(compare_decimals(unit.value(), op, n))
        }

        // Percentage comparisons
        (LiteralValue::Percentage(l), LiteralValue::Percentage(r)) => {
            Ok(compare_decimals(*l, op, r))
        }

        // Boolean comparisons
        (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => match op {
            ComparisonOperator::Equal | ComparisonOperator::Is => Ok(l == r),
            ComparisonOperator::NotEqual | ComparisonOperator::IsNot => Ok(l != r),
            _ => Err(LemmaError::Engine(
                "Can only use == and != with booleans".to_string(),
            )),
        },

        // Text comparisons
        (LiteralValue::Text(l), LiteralValue::Text(r)) => match op {
            ComparisonOperator::Equal | ComparisonOperator::Is => Ok(l == r),
            ComparisonOperator::NotEqual | ComparisonOperator::IsNot => Ok(l != r),
            _ => Err(LemmaError::Engine(
                "Can only use == and != with text".to_string(),
            )),
        },

        // Date comparisons
        (LiteralValue::Date(_), LiteralValue::Date(_)) => {
            super::datetime::datetime_comparison(left, op, right)
        }

        _ => Err(LemmaError::Engine(format!(
            "Comparison {:?} not supported for types {:?} and {:?}",
            op,
            type_name(left),
            type_name(right)
        ))),
    }
}

/// Convert a Unit value to match the target Unit's type
fn convert_to_matching_unit(
    value: &LiteralValue,
    target: &crate::NumericUnit,
) -> LemmaResult<LiteralValue> {
    let conversion_target = match target {
        crate::NumericUnit::Mass(_, u) => crate::ConversionTarget::Mass(u.clone()),
        crate::NumericUnit::Length(_, u) => crate::ConversionTarget::Length(u.clone()),
        crate::NumericUnit::Volume(_, u) => crate::ConversionTarget::Volume(u.clone()),
        crate::NumericUnit::Duration(_, unit) => crate::ConversionTarget::Duration(unit.clone()),
        crate::NumericUnit::Temperature(_, u) => crate::ConversionTarget::Temperature(u.clone()),
        crate::NumericUnit::Power(_, u) => crate::ConversionTarget::Power(u.clone()),
        crate::NumericUnit::Force(_, u) => crate::ConversionTarget::Force(u.clone()),
        crate::NumericUnit::Pressure(_, u) => crate::ConversionTarget::Pressure(u.clone()),
        crate::NumericUnit::Energy(_, u) => crate::ConversionTarget::Energy(u.clone()),
        crate::NumericUnit::Frequency(_, u) => crate::ConversionTarget::Frequency(u.clone()),
        crate::NumericUnit::DataSize(_, u) => crate::ConversionTarget::DataSize(u.clone()),
        crate::NumericUnit::Money(_, u) => crate::ConversionTarget::Money(u.clone()),
    };
    super::units::convert_unit_for_arithmetic(value, &conversion_target)
}

/// Helper to compare two decimal values
fn compare_decimals(left: Decimal, op: &ComparisonOperator, right: &Decimal) -> bool {
    match op {
        ComparisonOperator::GreaterThan => left > *right,
        ComparisonOperator::LessThan => left < *right,
        ComparisonOperator::GreaterThanOrEqual => left >= *right,
        ComparisonOperator::LessThanOrEqual => left <= *right,
        ComparisonOperator::Equal | ComparisonOperator::Is => left == *right,
        ComparisonOperator::NotEqual | ComparisonOperator::IsNot => left != *right,
    }
}

/// Helper to get a human-readable type name
fn type_name(value: &LiteralValue) -> &'static str {
    match value {
        LiteralValue::Number(_) => "Number",
        LiteralValue::Percentage(_) => "Percentage",
        LiteralValue::Boolean(_) => "Boolean",
        LiteralValue::Text(_) => "Text",
        LiteralValue::Date(_) => "Date",
        LiteralValue::Time(_) => "Time",
        LiteralValue::Unit(unit) => unit.category(),
        LiteralValue::Regex(_) => "Regex",
    }
}
