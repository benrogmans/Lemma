//! Type-aware arithmetic operations

use crate::evaluation::OperationResult;
use crate::semantic::standard_number;
use crate::{ArithmeticComputation, LiteralValue, Value};
use rust_decimal::Decimal;

/// Perform type-aware arithmetic operation, returning OperationResult (Veto for runtime errors)
pub fn arithmetic_operation(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (&left.value, &right.value) {
        (Value::Number(l), Value::Number(r)) => match number_arithmetic(*l, op, *r) {
            Ok(result) => OperationResult::Value(LiteralValue::number_with_type(
                result,
                left.lemma_type.clone(),
            )),
            Err(msg) => OperationResult::Veto(Some(msg)),
        },

        (Value::Date(_), _) | (_, Value::Date(_)) => {
            super::datetime::datetime_arithmetic(left, op, right)
        }

        (Value::Time(_), _) | (_, Value::Time(_)) => {
            super::datetime::time_arithmetic(left, op, right)
        }

        // Duration arithmetic
        (Value::Duration(l, lu), Value::Duration(r, ru)) => {
            let left_seconds = super::units::duration_to_seconds(*l, lu);
            let right_seconds = super::units::duration_to_seconds(*r, ru);
            match op {
                ArithmeticComputation::Add => {
                    let result_seconds = left_seconds + right_seconds;
                    let result_value = super::units::seconds_to_duration(result_seconds, lu);
                    OperationResult::Value(LiteralValue::duration_with_type(
                        result_value,
                        lu.clone(),
                        left.lemma_type.clone(),
                    ))
                }
                ArithmeticComputation::Subtract => {
                    let result_seconds = left_seconds - right_seconds;
                    let result_value = super::units::seconds_to_duration(result_seconds, lu);
                    OperationResult::Value(LiteralValue::duration_with_type(
                        result_value,
                        lu.clone(),
                        left.lemma_type.clone(),
                    ))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for durations",
                    op
                ))),
            }
        }

        // Duration with number
        (Value::Duration(value, unit), Value::Number(n)) => match op {
            ArithmeticComputation::Multiply => OperationResult::Value(
                LiteralValue::duration_with_type(value * n, unit.clone(), left.lemma_type.clone()),
            ),
            ArithmeticComputation::Divide => {
                if *n == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
                }
                OperationResult::Value(LiteralValue::duration_with_type(
                    value / n,
                    unit.clone(),
                    left.lemma_type.clone(),
                ))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for duration and number",
                op
            ))),
        },

        (Value::Number(n), Value::Duration(value, unit)) => match op {
            ArithmeticComputation::Multiply => OperationResult::Value(
                LiteralValue::duration_with_type(n * value, unit.clone(), left.lemma_type.clone()),
            ),
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for number and duration",
                op
            ))),
        },

        // Ratio operations
        // Ratio op Number → Number (ratio semantics: ratio + number = number * (1 + ratio))
        (Value::Ratio(r, _), Value::Number(n)) if right.get_type().is_number() => {
            match op {
                ArithmeticComputation::Add => {
                    // ratio + number = number * (1 + ratio)
                    let result = *n * (Decimal::ONE + *r);
                    OperationResult::Value(LiteralValue::number_with_type(
                        result,
                        standard_number().clone(),
                    ))
                }
                ArithmeticComputation::Subtract => {
                    // ratio - number = number * (1 - ratio)
                    let result = *n * (Decimal::ONE - *r);
                    OperationResult::Value(LiteralValue::number_with_type(
                        result,
                        standard_number().clone(),
                    ))
                }
                ArithmeticComputation::Multiply => match number_arithmetic(*r, op, *n) {
                    Ok(result) => OperationResult::Value(LiteralValue::number_with_type(
                        result,
                        standard_number().clone(),
                    )),
                    Err(msg) => OperationResult::Veto(Some(msg)),
                },
                ArithmeticComputation::Divide => {
                    if *n == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    match number_arithmetic(*r, op, *n) {
                        Ok(result) => OperationResult::Value(LiteralValue::number_with_type(
                            result,
                            standard_number().clone(),
                        )),
                        Err(msg) => OperationResult::Veto(Some(msg)),
                    }
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for ratio and number",
                    op
                ))),
            }
        }
        // Number op Ratio → Number (ratio semantics: number + ratio = number * (1 + ratio))
        (Value::Number(n), Value::Ratio(r, _)) if left.get_type().is_number() => {
            match op {
                ArithmeticComputation::Add => {
                    // number + ratio = number * (1 + ratio)
                    let result = *n * (Decimal::ONE + *r);
                    OperationResult::Value(LiteralValue::number_with_type(
                        result,
                        standard_number().clone(),
                    ))
                }
                ArithmeticComputation::Subtract => {
                    // number - ratio = number * (1 - ratio)
                    let result = *n * (Decimal::ONE - *r);
                    OperationResult::Value(LiteralValue::number_with_type(
                        result,
                        standard_number().clone(),
                    ))
                }
                ArithmeticComputation::Multiply => match number_arithmetic(*n, op, *r) {
                    Ok(result) => OperationResult::Value(LiteralValue::number_with_type(
                        result,
                        standard_number().clone(),
                    )),
                    Err(msg) => OperationResult::Veto(Some(msg)),
                },
                ArithmeticComputation::Divide => {
                    if *r == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    match number_arithmetic(*n, op, *r) {
                        Ok(result) => OperationResult::Value(LiteralValue::number_with_type(
                            result,
                            standard_number().clone(),
                        )),
                        Err(msg) => OperationResult::Veto(Some(msg)),
                    }
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for number and ratio",
                    op
                ))),
            }
        }
        // Ratio op Ratio → Ratio
        (Value::Ratio(l, lu), Value::Ratio(r, ru)) => {
            // Preserve unit from left operand, or right if left is None
            let preserved_unit = lu.clone().or_else(|| ru.clone());
            match number_arithmetic(*l, op, *r) {
                Ok(result) => OperationResult::Value(LiteralValue::ratio_with_type(
                    result,
                    preserved_unit,
                    left.lemma_type.clone(),
                )),
                Err(msg) => OperationResult::Veto(Some(msg)),
            }
        }
        // Scale operations with Scale
        (Value::Scale(l_val, l_unit), Value::Scale(r_val, r_unit)) => {
            // Units must match for addition/subtraction
            if l_unit != r_unit
                && (matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ))
            {
                return OperationResult::Veto(Some(format!(
                    "Cannot apply '{}' to values with different units: {:?} and {:?}",
                    op, l_unit, r_unit
                )));
            }
            // Preserve unit from left
            let preserved_unit = l_unit.clone();
            match number_arithmetic(*l_val, op, *r_val) {
                Ok(result) => OperationResult::Value(LiteralValue::scale_with_type(
                    result,
                    preserved_unit,
                    left.lemma_type.clone(),
                )),
                Err(msg) => OperationResult::Veto(Some(msg)),
            }
        }
        // Ratio op Scale → Scale (inherits Scale type and unit)
        (Value::Ratio(ratio_val, _), Value::Scale(scale_val, scale_unit)) => {
            match op {
                ArithmeticComputation::Multiply => {
                    match number_arithmetic(*ratio_val, op, *scale_val) {
                        Ok(result) => OperationResult::Value(LiteralValue::scale_with_type(
                            result,
                            scale_unit.clone(),
                            right.lemma_type.clone(),
                        )),
                        Err(msg) => OperationResult::Veto(Some(msg)),
                    }
                }
                ArithmeticComputation::Divide => {
                    if *scale_val == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    match number_arithmetic(*ratio_val, op, *scale_val) {
                        Ok(result) => OperationResult::Value(LiteralValue::scale_with_type(
                            result,
                            scale_unit.clone(),
                            right.lemma_type.clone(),
                        )),
                        Err(msg) => OperationResult::Veto(Some(msg)),
                    }
                }
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    // Scale +/- Ratio applies ratio semantics: scale +/- (scale * ratio) = scale * (1 +/- ratio)
                    let ratio_amount = *scale_val * *ratio_val;
                    let result = match op {
                        ArithmeticComputation::Add => *scale_val + ratio_amount,
                        ArithmeticComputation::Subtract => *scale_val - ratio_amount,
                        _ => {
                            return OperationResult::Veto(Some(format!(
                                "Operation '{}' not supported for ratio and scale",
                                op
                            )))
                        }
                    };
                    OperationResult::Value(LiteralValue::scale_with_type(
                        result,
                        scale_unit.clone(), // Preserve Scale unit
                        right.lemma_type.clone(),
                    ))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for ratio and scale",
                    op
                ))),
            }
        }
        // Scale op Ratio → Scale (inherits Scale type and unit)
        (Value::Scale(scale_val, scale_unit), Value::Ratio(ratio_val, _)) => {
            match op {
                ArithmeticComputation::Multiply => {
                    match number_arithmetic(*scale_val, op, *ratio_val) {
                        Ok(result) => OperationResult::Value(LiteralValue::scale_with_type(
                            result,
                            scale_unit.clone(),
                            left.lemma_type.clone(),
                        )),
                        Err(msg) => OperationResult::Veto(Some(msg)),
                    }
                }
                ArithmeticComputation::Divide => {
                    if *ratio_val == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    match number_arithmetic(*scale_val, op, *ratio_val) {
                        Ok(result) => OperationResult::Value(LiteralValue::scale_with_type(
                            result,
                            scale_unit.clone(),
                            left.lemma_type.clone(), // Inherit Scale type
                        )),
                        Err(msg) => OperationResult::Veto(Some(msg)),
                    }
                }
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    // Scale +/- Ratio applies ratio semantics: scale +/- (scale * ratio) = scale * (1 +/- ratio)
                    let ratio_amount = *scale_val * *ratio_val;
                    let result = match op {
                        ArithmeticComputation::Add => *scale_val + ratio_amount,
                        ArithmeticComputation::Subtract => *scale_val - ratio_amount,
                        _ => {
                            return OperationResult::Veto(Some(format!(
                                "Operation '{}' not supported for scale and ratio",
                                op
                            )))
                        }
                    };
                    OperationResult::Value(LiteralValue::scale_with_type(
                        result,
                        scale_unit.clone(), // Preserve Scale unit
                        left.lemma_type.clone(),
                    ))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for scale and ratio",
                    op
                ))),
            }
        }

        // Scale op Number → Scale (preserves unit)
        (Value::Scale(scale_val, scale_unit), Value::Number(n)) => {
            match number_arithmetic(*scale_val, op, *n) {
                Ok(result) => OperationResult::Value(LiteralValue::scale_with_type(
                    result,
                    scale_unit.clone(),
                    left.lemma_type.clone(),
                )),
                Err(msg) => OperationResult::Veto(Some(msg)),
            }
        }
        // Number op Scale → Scale (preserves unit)
        (Value::Number(n), Value::Scale(scale_val, scale_unit)) => {
            match number_arithmetic(*n, op, *scale_val) {
                Ok(result) => OperationResult::Value(LiteralValue::scale_with_type(
                    result,
                    scale_unit.clone(),
                    right.lemma_type.clone(),
                )),
                Err(msg) => OperationResult::Veto(Some(msg)),
            }
        }
        // Scale op Duration - not supported
        (Value::Scale(_scale_val, _scale_unit), Value::Duration(_d_val, _d_unit)) => match op {
            ArithmeticComputation::Multiply => {
                OperationResult::Veto(Some("Cannot multiply scale and duration".to_string()))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for scale and duration",
                op
            ))),
        },
        // Duration op Scale - not supported
        (Value::Duration(_d_val, _d_unit), Value::Scale(_scale_val, _scale_unit)) => match op {
            ArithmeticComputation::Multiply => {
                OperationResult::Veto(Some("Cannot multiply duration and scale".to_string()))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for duration and scale",
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
    value.get_type().name().to_string()
}
