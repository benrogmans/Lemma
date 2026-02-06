//! Type-aware arithmetic operations

use crate::evaluation::OperationResult;
use crate::planning::semantics::{
    primitive_number, ArithmeticComputation, LiteralValue, ValueKind,
};
use rust_decimal::Decimal;

/// Perform type-aware arithmetic operation, returning OperationResult (Veto for runtime errors)
pub fn arithmetic_operation(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (&left.value, &right.value) {
        (ValueKind::Number(l), ValueKind::Number(r)) => match number_arithmetic(*l, op, *r) {
            Ok(result) => OperationResult::Value(Box::new(LiteralValue::number_with_type(
                result,
                left.lemma_type.clone(),
            ))),
            Err(msg) => OperationResult::Veto(Some(msg)),
        },

        (ValueKind::Date(_), _) | (_, ValueKind::Date(_)) => {
            super::datetime::datetime_arithmetic(left, op, right)
        }

        (ValueKind::Time(_), _) | (_, ValueKind::Time(_)) => {
            super::datetime::time_arithmetic(left, op, right)
        }

        // Duration arithmetic
        (ValueKind::Duration(l, lu), ValueKind::Duration(r, ru)) => {
            let left_seconds = super::units::duration_to_seconds(*l, lu);
            let right_seconds = super::units::duration_to_seconds(*r, ru);
            match op {
                ArithmeticComputation::Add => {
                    let result_seconds = left_seconds + right_seconds;
                    let result_value = super::units::seconds_to_duration(result_seconds, lu);
                    OperationResult::Value(Box::new(LiteralValue::duration_with_type(
                        result_value,
                        lu.clone(),
                        left.lemma_type.clone(),
                    )))
                }
                ArithmeticComputation::Subtract => {
                    let result_seconds = left_seconds - right_seconds;
                    let result_value = super::units::seconds_to_duration(result_seconds, lu);
                    OperationResult::Value(Box::new(LiteralValue::duration_with_type(
                        result_value,
                        lu.clone(),
                        left.lemma_type.clone(),
                    )))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for durations",
                    op
                ))),
            }
        }

        // Duration with number
        (ValueKind::Duration(value, unit), ValueKind::Number(n)) => match op {
            ArithmeticComputation::Multiply => OperationResult::Value(Box::new(
                LiteralValue::duration_with_type(value * n, unit.clone(), left.lemma_type.clone()),
            )),
            ArithmeticComputation::Divide => {
                if *n == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
                }
                OperationResult::Value(Box::new(LiteralValue::duration_with_type(
                    value / n,
                    unit.clone(),
                    left.lemma_type.clone(),
                )))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for duration and number",
                op
            ))),
        },

        (ValueKind::Number(n), ValueKind::Duration(value, unit)) => match op {
            ArithmeticComputation::Multiply => OperationResult::Value(Box::new(
                LiteralValue::duration_with_type(n * value, unit.clone(), left.lemma_type.clone()),
            )),
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for number and duration",
                op
            ))),
        },

        // Ratio operations
        // Ratio op Number → Number (ratio semantics: ratio + number = number * (1 + ratio))
        (ValueKind::Ratio(r, _), ValueKind::Number(n)) if right.get_type().is_number() => {
            match op {
                ArithmeticComputation::Add => {
                    // ratio + number = number * (1 + ratio)
                    let result = *n * (Decimal::ONE + *r);
                    OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        result,
                        primitive_number().clone(),
                    )))
                }
                ArithmeticComputation::Subtract => {
                    // ratio - number = number * (1 - ratio)
                    let result = *n * (Decimal::ONE - *r);
                    OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        result,
                        primitive_number().clone(),
                    )))
                }
                ArithmeticComputation::Multiply => match number_arithmetic(*r, op, *n) {
                    Ok(result) => OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        result,
                        primitive_number().clone(),
                    ))),
                    Err(msg) => OperationResult::Veto(Some(msg)),
                },
                ArithmeticComputation::Divide => {
                    if *n == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    match number_arithmetic(*r, op, *n) {
                        Ok(result) => OperationResult::Value(Box::new(
                            LiteralValue::number_with_type(result, primitive_number().clone()),
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
        (ValueKind::Number(n), ValueKind::Ratio(r, _)) if left.get_type().is_number() => {
            match op {
                ArithmeticComputation::Add => {
                    // number + ratio = number * (1 + ratio)
                    let result = *n * (Decimal::ONE + *r);
                    OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        result,
                        primitive_number().clone(),
                    )))
                }
                ArithmeticComputation::Subtract => {
                    // number - ratio = number * (1 - ratio)
                    let result = *n * (Decimal::ONE - *r);
                    OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        result,
                        primitive_number().clone(),
                    )))
                }
                ArithmeticComputation::Multiply => match number_arithmetic(*n, op, *r) {
                    Ok(result) => OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        result,
                        primitive_number().clone(),
                    ))),
                    Err(msg) => OperationResult::Veto(Some(msg)),
                },
                ArithmeticComputation::Divide => {
                    if *r == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    match number_arithmetic(*n, op, *r) {
                        Ok(result) => OperationResult::Value(Box::new(
                            LiteralValue::number_with_type(result, primitive_number().clone()),
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
        (ValueKind::Ratio(l, lu), ValueKind::Ratio(r, _ru)) => {
            // Preserve unit from left operand
            match number_arithmetic(*l, op, *r) {
                Ok(result) => OperationResult::Value(Box::new(LiteralValue::ratio_with_type(
                    result,
                    lu.clone(),
                    left.lemma_type.clone(),
                ))),
                Err(msg) => OperationResult::Veto(Some(msg)),
            }
        }
        // Scale operations with Scale
        (ValueKind::Scale(l_val, l_unit), ValueKind::Scale(r_val, r_unit)) => {
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
                Ok(result) => OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                    result,
                    preserved_unit,
                    left.lemma_type.clone(),
                ))),
                Err(msg) => OperationResult::Veto(Some(msg)),
            }
        }
        // Ratio op Scale → Scale (inherits Scale type and unit)
        (ValueKind::Ratio(ratio_val, _), ValueKind::Scale(scale_val, scale_unit)) => {
            match op {
                ArithmeticComputation::Multiply => {
                    match number_arithmetic(*ratio_val, op, *scale_val) {
                        Ok(result) => {
                            OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                                result,
                                scale_unit.clone(),
                                right.lemma_type.clone(),
                            )))
                        }
                        Err(msg) => OperationResult::Veto(Some(msg)),
                    }
                }
                ArithmeticComputation::Divide => {
                    if *scale_val == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    match number_arithmetic(*ratio_val, op, *scale_val) {
                        Ok(result) => {
                            OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                                result,
                                scale_unit.clone(),
                                right.lemma_type.clone(),
                            )))
                        }
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
                    OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                        result,
                        scale_unit.clone(), // Preserve Scale unit
                        right.lemma_type.clone(),
                    )))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for ratio and scale",
                    op
                ))),
            }
        }
        // Scale op Ratio → Scale (inherits Scale type and unit)
        (ValueKind::Scale(scale_val, scale_unit), ValueKind::Ratio(ratio_val, _)) => {
            match op {
                ArithmeticComputation::Multiply => {
                    match number_arithmetic(*scale_val, op, *ratio_val) {
                        Ok(result) => {
                            OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                                result,
                                scale_unit.clone(),
                                left.lemma_type.clone(),
                            )))
                        }
                        Err(msg) => OperationResult::Veto(Some(msg)),
                    }
                }
                ArithmeticComputation::Divide => {
                    if *ratio_val == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    match number_arithmetic(*scale_val, op, *ratio_val) {
                        Ok(result) => {
                            OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                                result,
                                scale_unit.clone(),
                                left.lemma_type.clone(), // Inherit Scale type
                            )))
                        }
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
                    OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                        result,
                        scale_unit.clone(), // Preserve Scale unit
                        left.lemma_type.clone(),
                    )))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for scale and ratio",
                    op
                ))),
            }
        }

        // Scale op Number → Scale (preserves unit)
        (ValueKind::Scale(scale_val, scale_unit), ValueKind::Number(n)) => {
            match number_arithmetic(*scale_val, op, *n) {
                Ok(result) => OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                    result,
                    scale_unit.clone(),
                    left.lemma_type.clone(),
                ))),
                Err(msg) => OperationResult::Veto(Some(msg)),
            }
        }
        // Number op Scale → Scale (preserves unit)
        (ValueKind::Number(n), ValueKind::Scale(scale_val, scale_unit)) => {
            match number_arithmetic(*n, op, *scale_val) {
                Ok(result) => OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                    result,
                    scale_unit.clone(),
                    right.lemma_type.clone(),
                ))),
                Err(msg) => OperationResult::Veto(Some(msg)),
            }
        }
        // Scale op Duration - not supported
        (ValueKind::Scale(_scale_val, _scale_unit), ValueKind::Duration(_d_val, _d_unit)) => {
            match op {
                ArithmeticComputation::Multiply => {
                    OperationResult::Veto(Some("Cannot multiply scale and duration".to_string()))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for scale and duration",
                    op
                ))),
            }
        }
        // Duration op Scale - not supported
        (ValueKind::Duration(_d_val, _d_unit), ValueKind::Scale(_scale_val, _scale_unit)) => {
            match op {
                ArithmeticComputation::Multiply => {
                    OperationResult::Veto(Some("Cannot multiply duration and scale".to_string()))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for duration and scale",
                    op
                ))),
            }
        }
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
