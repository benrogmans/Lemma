//! Type-aware arithmetic operations

use crate::computation::rational::{
    checked_add, checked_div, checked_mul, checked_sub, rational_operation_with_fallback,
    NumericFailure, NumericOperation, RationalInteger,
};
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::planning::semantics::{
    primitive_number, ArithmeticComputation, BaseQuantityVector, LemmaType, LiteralValue,
    SemanticCalendarUnit, ValueKind, CALENDAR_DIMENSION, DURATION_DIMENSION,
};

fn number_op_on_stored_rationals(
    left: RationalInteger,
    operator: &ArithmeticComputation,
    right: RationalInteger,
    lemma_type: LemmaType,
) -> OperationResult {
    match number_arithmetic(left, operator, right) {
        Ok(rational) => OperationResult::Value(Box::new(LiteralValue::number_with_type(
            rational, lemma_type,
        ))),
        Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
    }
}

fn number_op_on_stored_rationals_primitive(
    left: RationalInteger,
    operator: &ArithmeticComputation,
    right: RationalInteger,
) -> OperationResult {
    number_op_on_stored_rationals(left, operator, right, primitive_number().clone())
}

fn number_ratio_arithmetic(
    n: RationalInteger,
    op: &ArithmeticComputation,
    r: RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    let one = RationalInteger::new(1, 1);
    match op {
        ArithmeticComputation::Add => {
            let factor = checked_add(&one, &r).map_err(map_numeric_failure)?;
            checked_mul(&n, &factor).map_err(map_numeric_failure)
        }
        ArithmeticComputation::Subtract => {
            let factor = checked_sub(&one, &r).map_err(map_numeric_failure)?;
            checked_mul(&n, &factor).map_err(map_numeric_failure)
        }
        ArithmeticComputation::Multiply => checked_mul(&n, &r).map_err(map_numeric_failure),
        ArithmeticComputation::Divide => checked_div(&n, &r).map_err(map_numeric_failure),
        _ => unreachable!(
            "BUG: number/ratio with {:?}; planning should have rejected this",
            op
        ),
    }
}

fn calendar_from_months_arithmetic(
    left: RationalInteger,
    left_unit: &SemanticCalendarUnit,
    operator: &ArithmeticComputation,
    right: RationalInteger,
    right_unit: &SemanticCalendarUnit,
    result_lemma_type: LemmaType,
) -> OperationResult {
    let left_months =
        super::units::convert_calendar_magnitude(left, left_unit, &SemanticCalendarUnit::Month);
    let right_months =
        super::units::convert_calendar_magnitude(right, right_unit, &SemanticCalendarUnit::Month);

    let result_months = match operator {
        ArithmeticComputation::Add => match checked_add(&left_months, &right_months) {
            Ok(r) => r,
            Err(failure) => {
                return OperationResult::Veto(VetoType::computation(
                    map_numeric_failure(failure).message(),
                ))
            }
        },
        ArithmeticComputation::Subtract => match checked_sub(&left_months, &right_months) {
            Ok(r) => r,
            Err(failure) => {
                return OperationResult::Veto(VetoType::computation(
                    map_numeric_failure(failure).message(),
                ))
            }
        },
        _ => unreachable!(
            "BUG: calendar_from_months_arithmetic called with {:?}; planning rejects this",
            operator
        ),
    };

    let result_unit = left_unit.clone();
    let result_value = super::units::convert_calendar_magnitude(
        result_months,
        &SemanticCalendarUnit::Month,
        &result_unit,
    );
    OperationResult::Value(Box::new(LiteralValue::calendar_with_type(
        result_value,
        result_unit,
        result_lemma_type,
    )))
}

/// Perform type-aware arithmetic operation, returning OperationResult (Veto for runtime errors)
pub fn arithmetic_operation(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (&left.value, &right.value) {
        (ValueKind::Range(range_left, range_right), ValueKind::Calendar(value, unit))
            if left.lemma_type.is_calendar_range()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            shift_calendar_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                *value,
                unit,
                matches!(op, ArithmeticComputation::Add),
            )
        }

        (ValueKind::Calendar(value, unit), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_calendar_range()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            shift_calendar_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                *value,
                unit,
                matches!(op, ArithmeticComputation::Add),
            )
        }

        (ValueKind::Range(range_left, range_right), ValueKind::Calendar(value, unit))
            if left.lemma_type.is_date_range()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            shift_date_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                *value,
                unit,
                matches!(op, ArithmeticComputation::Add),
            )
        }

        (ValueKind::Calendar(value, unit), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_date_range()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            shift_date_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                *value,
                unit,
                matches!(op, ArithmeticComputation::Add),
            )
        }

        (
            ValueKind::Range(left_range_left, left_range_right),
            ValueKind::Range(right_range_left, right_range_right),
        ) if matches!(
            op,
            ArithmeticComputation::Add | ArithmeticComputation::Subtract
        ) =>
        {
            let left_measure = super::range::compute_quantity(
                left_range_left.as_ref(),
                left_range_right.as_ref(),
                None,
            );
            let right_measure = super::range::compute_quantity(
                right_range_left.as_ref(),
                right_range_right.as_ref(),
                None,
            );
            operate_on_operation_results(left_measure, op, right_measure)
        }

        (ValueKind::Range(range_left, range_right), _)
            if matches!(
                op,
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
            ) =>
        {
            let measure = super::range::compute_quantity(
                range_left.as_ref(),
                range_right.as_ref(),
                Some(right),
            );
            operate_with_left_result(measure, op, right)
        }

        (_, ValueKind::Range(range_left, range_right))
            if matches!(
                op,
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
            ) =>
        {
            let measure = super::range::compute_quantity(
                range_left.as_ref(),
                range_right.as_ref(),
                Some(left),
            );
            operate_with_right_result(left, op, measure)
        }

        (ValueKind::Range(range_left, range_right), ValueKind::Number(_))
            if left.lemma_type.is_date_range() && matches!(op, ArithmeticComputation::Multiply) =>
        {
            let span = super::range::compute_span(range_left.as_ref(), range_right.as_ref());
            operate_with_left_result(span, op, right)
        }

        (ValueKind::Number(_), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_date_range()
                && matches!(op, ArithmeticComputation::Multiply) =>
        {
            let span = super::range::compute_span(range_left.as_ref(), range_right.as_ref());
            operate_with_right_result(left, op, span)
        }

        (ValueKind::Quantity(_, _, _), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_date_range()
                && matches!(op, ArithmeticComputation::Multiply) =>
        {
            let projected = project_date_range_through_quantity(
                left,
                range_left.as_ref(),
                range_right.as_ref(),
            );
            operate_with_right_result(left, op, projected)
        }

        (ValueKind::Range(range_left, range_right), ValueKind::Quantity(_, _, _))
            if left.lemma_type.is_date_range() && matches!(op, ArithmeticComputation::Multiply) =>
        {
            let projected = project_date_range_through_quantity(
                right,
                range_left.as_ref(),
                range_right.as_ref(),
            );
            operate_with_right_result(right, op, projected)
        }

        (ValueKind::Number(l), ValueKind::Number(r)) => {
            number_op_on_stored_rationals(*l, op, *r, left.lemma_type.clone())
        }

        (ValueKind::Date(_), _) | (_, ValueKind::Date(_)) => {
            super::datetime::datetime_arithmetic(left, op, right)
        }

        (ValueKind::Time(_), _) | (_, ValueKind::Time(_)) => {
            super::datetime::time_arithmetic(left, op, right)
        }

        // Calendar arithmetic
        (ValueKind::Calendar(l, lu), ValueKind::Calendar(r, ru)) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                calendar_from_months_arithmetic(*l, lu, op, *r, ru, left.lemma_type.clone())
            }
            ArithmeticComputation::Divide => {
                let left_months =
                    super::units::convert_calendar_magnitude(*l, lu, &SemanticCalendarUnit::Month);
                let right_months =
                    super::units::convert_calendar_magnitude(*r, ru, &SemanticCalendarUnit::Month);
                number_op_on_stored_rationals_primitive(
                    left_months,
                    &ArithmeticComputation::Divide,
                    right_months,
                )
            }
            _ => unreachable!(
                "BUG: calendar * calendar with op {:?}; planning should have rejected this",
                op
            ),
        },

        // Calendar op Number → Calendar (scaling, division, modulo all preserve the unit)
        (ValueKind::Calendar(value, unit), ValueKind::Number(n)) => {
            match number_arithmetic(*value, op, *n) {
                Ok(rational) => OperationResult::Value(Box::new(LiteralValue::calendar_with_type(
                    rational,
                    unit.clone(),
                    left.lemma_type.clone(),
                ))),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }

        // Number with Calendar → Calendar for multiply; Number otherwise
        (ValueKind::Number(n), ValueKind::Calendar(value, unit)) => {
            match number_arithmetic(*n, op, *value) {
                Ok(rational) => match op {
                    ArithmeticComputation::Multiply => {
                        OperationResult::Value(Box::new(LiteralValue::calendar_with_type(
                            rational,
                            unit.clone(),
                            right.lemma_type.clone(),
                        )))
                    }
                    _ => OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        rational,
                        primitive_number().clone(),
                    ))),
                },
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }

        // Ratio with Number → Number (ratio semantics: add/subtract apply as multiplier)
        // r + n means n * (1 + r), r - n means n * (1 - r)
        (ValueKind::Ratio(_, _), ValueKind::Number(_))
        | (ValueKind::Number(_), ValueKind::Ratio(_, _)) => {
            let (n, r) = match (&left.value, &right.value) {
                (ValueKind::Number(n), ValueKind::Ratio(r, _)) => (*n, *r),
                (ValueKind::Ratio(r, _), ValueKind::Number(n)) => (*n, *r),
                _ => unreachable!("BUG: matched number/ratio arm with other value kinds"),
            };
            match number_ratio_arithmetic(n, op, r) {
                Ok(rational) => OperationResult::Value(Box::new(LiteralValue::number_with_type(
                    rational,
                    primitive_number().clone(),
                ))),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }

        // Ratio with Calendar → Calendar (same ratio semantics)
        (ValueKind::Calendar(_, _), ValueKind::Ratio(_, _))
        | (ValueKind::Ratio(_, _), ValueKind::Calendar(_, _)) => {
            let (cal_val, cal_unit, cal_type, ratio_val) = match (
                &left.value,
                &right.value,
                &left.lemma_type,
                &right.lemma_type,
            ) {
                (ValueKind::Calendar(c, u), ValueKind::Ratio(r, _), ct, _) => (*c, u, ct, *r),
                (ValueKind::Ratio(r, _), ValueKind::Calendar(c, u), _, ct) => (*c, u, ct, *r),
                _ => unreachable!("BUG: matched calendar/ratio arm with other value kinds"),
            };
            match number_ratio_arithmetic(cal_val, op, ratio_val) {
                Ok(rational) => OperationResult::Value(Box::new(LiteralValue::calendar_with_type(
                    rational,
                    cal_unit.clone(),
                    cal_type.clone(),
                ))),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }
        // Ratio op Ratio → Ratio
        (ValueKind::Ratio(l, lu), ValueKind::Ratio(r, _ru)) => {
            match number_arithmetic(*l, op, *r) {
                Ok(rational) => OperationResult::Value(Box::new(LiteralValue::ratio_with_type(
                    rational,
                    lu.clone(),
                    left.lemma_type.clone(),
                ))),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }
        // Quantity operations with Quantity
        (
            ValueKind::Quantity(l_val, l_unit, _l_decomp),
            ValueKind::Quantity(r_val, r_unit, _r_decomp),
        ) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                let same_family = left.lemma_type.same_quantity_family(&right.lemma_type);
                let anonymous_compatible = left
                    .lemma_type
                    .compatible_with_anonymous_quantity(&right.lemma_type);
                if !same_family && !anonymous_compatible {
                    unreachable!(
                        "BUG: quantity add/subtract with incompatible types ({} vs {}); should be rejected during planning",
                        left.lemma_type.name(),
                        right.lemma_type.name()
                    );
                }
                match quantity_add_subtract(
                    *l_val,
                    l_unit,
                    &left.lemma_type,
                    op,
                    *r_val,
                    r_unit,
                    &right.lemma_type,
                ) {
                    Ok(rational) => {
                        OperationResult::Value(Box::new(LiteralValue::quantity_with_type(
                            rational,
                            l_unit.clone(),
                            left.lemma_type.clone(),
                        )))
                    }
                    Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
                }
            }
            ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {
                let l_canonical =
                    quantity_canonical_magnitude_rational(*l_val, l_unit, &left.lemma_type);
                let r_canonical =
                    quantity_canonical_magnitude_rational(*r_val, r_unit, &right.lemma_type);
                match (l_canonical, r_canonical) {
                    (Ok(lc), Ok(rc)) => {
                        if matches!(op, ArithmeticComputation::Divide)
                            && crate::computation::rational::rational_is_zero(&rc)
                        {
                            return OperationResult::Veto(VetoType::computation(
                                "Division by zero",
                            ));
                        }
                        let numeric_op = match op {
                            ArithmeticComputation::Multiply => NumericOperation::Multiply,
                            ArithmeticComputation::Divide => NumericOperation::Divide,
                            _ => unreachable!("BUG: matched multiply/divide arm with other op"),
                        };
                        let result = match rational_operation_with_fallback(&lc, numeric_op, &rc) {
                            Ok(p) => p,
                            Err(failure) => {
                                return OperationResult::Veto(VetoType::computation(
                                    map_numeric_failure(failure).message(),
                                ))
                            }
                        };
                        let l_decomp = left.lemma_type.quantity_type_decomposition();
                        let r_decomp = right.lemma_type.quantity_type_decomposition();
                        let combined = crate::planning::semantics::combine_decompositions(
                            l_decomp,
                            r_decomp,
                            matches!(op, ArithmeticComputation::Multiply),
                        );
                        if combined.is_empty() {
                            OperationResult::Value(Box::new(LiteralValue::number_with_type(
                                result,
                                primitive_number().clone(),
                            )))
                        } else {
                            OperationResult::Value(Box::new(LiteralValue::quantity_anonymous(
                                result, combined,
                            )))
                        }
                    }
                    (Err(failure), _) | (_, Err(failure)) => {
                        OperationResult::Veto(VetoType::computation(failure.message()))
                    }
                }
            }
            _ => unreachable!(
                "BUG: quantity {:?} quantity is rejected during planning",
                op
            ),
        },
        (ValueKind::Quantity(_q_val, _q_unit, _), ValueKind::Ratio(_r, _))
        | (ValueKind::Ratio(_r, _), ValueKind::Quantity(_q_val, _q_unit, _)) => {
            let (quantity_val, quantity_unit, quantity_type, ratio_val) = match (
                &left.value,
                &right.value,
                &left.lemma_type,
                &right.lemma_type,
            ) {
                (ValueKind::Quantity(q_val, q_unit, _), ValueKind::Ratio(r, _), qt, _) => {
                    (*q_val, q_unit, qt, *r)
                }
                (ValueKind::Ratio(r, _), ValueKind::Quantity(q_val, q_unit, _), _, qt) => {
                    (*q_val, q_unit, qt, *r)
                }
                _ => unreachable!("BUG: matched quantity/ratio arm with other value kinds"),
            };
            match quantity_ratio_arithmetic(
                quantity_val,
                quantity_unit,
                quantity_type,
                op,
                ratio_val,
            ) {
                Ok(rational) => OperationResult::Value(Box::new(LiteralValue::quantity_with_type(
                    rational,
                    quantity_unit.clone(),
                    quantity_type.clone(),
                ))),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }

        // Quantity op Number → Quantity (preserves unit)
        (
            ValueKind::Quantity(quantity_val, quantity_unit, _quantity_decomp),
            ValueKind::Number(n),
        ) => match number_arithmetic(*quantity_val, op, *n) {
            Ok(rational) => OperationResult::Value(Box::new(LiteralValue::quantity_with_type(
                rational,
                quantity_unit.clone(),
                left.lemma_type.clone(),
            ))),
            Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
        },
        // Number op Quantity → Quantity for multiply; for divide, negate decomp if anonymous Quantity.
        (
            ValueKind::Number(n),
            ValueKind::Quantity(quantity_val, quantity_unit, quantity_decomp),
        ) => match op {
            ArithmeticComputation::Multiply => match number_arithmetic(*n, op, *quantity_val) {
                Ok(rational) => OperationResult::Value(Box::new(LiteralValue::quantity_with_type(
                    rational,
                    quantity_unit.clone(),
                    right.lemma_type.clone(),
                ))),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            },
            ArithmeticComputation::Divide => {
                if quantity_decomp.is_empty() {
                    match number_arithmetic(*n, op, *quantity_val) {
                        Ok(rational) => OperationResult::Value(Box::new(
                            LiteralValue::number_with_type(rational, primitive_number().clone()),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    }
                } else {
                    let negated: BaseQuantityVector = quantity_decomp
                        .iter()
                        .map(|(k, &e)| (k.clone(), -e))
                        .collect();
                    match number_arithmetic(*n, op, *quantity_val) {
                        Ok(rational) => OperationResult::Value(Box::new(
                            LiteralValue::quantity_anonymous(rational, negated),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    }
                }
            }
            _ => number_op_on_stored_rationals_primitive(*n, op, *quantity_val),
        },
        (ValueKind::Quantity(q_val, q_unit, _), ValueKind::Calendar(c_val, c_unit))
        | (ValueKind::Calendar(c_val, c_unit), ValueKind::Quantity(q_val, q_unit, _)) => {
            let (quantity_type, is_quantity_left) =
                if matches!(&left.value, ValueKind::Quantity(..)) {
                    (&left.lemma_type, true)
                } else {
                    (&right.lemma_type, false)
                };
            let q_canonical =
                match quantity_canonical_magnitude_rational(*q_val, q_unit, quantity_type) {
                    Ok(c) => c,
                    Err(failure) => {
                        return OperationResult::Veto(VetoType::computation(failure.message()))
                    }
                };
            let c_months = super::units::convert_calendar_magnitude(
                *c_val,
                c_unit,
                &SemanticCalendarUnit::Month,
            );
            let result = match op {
                ArithmeticComputation::Multiply => rational_operation_with_fallback(
                    &q_canonical,
                    NumericOperation::Multiply,
                    &c_months,
                ),
                ArithmeticComputation::Divide => {
                    if is_quantity_left {
                        rational_operation_with_fallback(
                            &q_canonical,
                            NumericOperation::Divide,
                            &c_months,
                        )
                    } else {
                        rational_operation_with_fallback(
                            &c_months,
                            NumericOperation::Divide,
                            &q_canonical,
                        )
                    }
                }
                _ => unreachable!(
                    "BUG: quantity {:?} calendar is rejected during planning",
                    op
                ),
            };
            let result_rational = match result {
                Ok(r) => r,
                Err(failure) => {
                    return OperationResult::Veto(VetoType::computation(
                        map_numeric_failure(failure).message(),
                    ))
                }
            };
            let q_decomp = quantity_type.quantity_type_decomposition();
            let c_decomp = crate::planning::semantics::calendar_decomposition();
            let combined = crate::planning::semantics::combine_decompositions(
                q_decomp,
                &c_decomp,
                matches!(op, ArithmeticComputation::Multiply),
            );
            if combined.is_empty() {
                OperationResult::Value(Box::new(LiteralValue::number_with_type(
                    result_rational,
                    primitive_number().clone(),
                )))
            } else {
                OperationResult::Value(Box::new(LiteralValue::quantity_anonymous(
                    result_rational,
                    combined,
                )))
            }
        }
        _ => unreachable!(
            "BUG: arithmetic {:?} for {:?} and {:?}; planning should have rejected this",
            op,
            type_name(left),
            type_name(right)
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NumberArithmeticFailure {
    DivisionByZero,
    ModuloByZero,
    Computation(String),
}

impl NumberArithmeticFailure {
    pub(crate) fn message(self) -> String {
        match self {
            Self::DivisionByZero => "Division by zero".to_string(),
            Self::ModuloByZero => "Division by zero (modulo)".to_string(),
            Self::Computation(message) => message,
        }
    }
}

fn numeric_operation_from_arithmetic(operator: &ArithmeticComputation) -> NumericOperation {
    match operator {
        ArithmeticComputation::Add => NumericOperation::Add,
        ArithmeticComputation::Subtract => NumericOperation::Subtract,
        ArithmeticComputation::Multiply => NumericOperation::Multiply,
        ArithmeticComputation::Divide => NumericOperation::Divide,
        ArithmeticComputation::Modulo => NumericOperation::Modulo,
        ArithmeticComputation::Power => NumericOperation::Power,
    }
}

fn map_numeric_failure(failure: NumericFailure) -> NumberArithmeticFailure {
    match failure {
        NumericFailure::DivisionByZero => NumberArithmeticFailure::DivisionByZero,
        other => NumberArithmeticFailure::Computation(other.to_string()),
    }
}

fn map_numeric_failure_modulo(failure: NumericFailure) -> NumberArithmeticFailure {
    match failure {
        NumericFailure::DivisionByZero => NumberArithmeticFailure::ModuloByZero,
        other => map_numeric_failure(other),
    }
}

pub(crate) fn quantity_canonical_magnitude_rational(
    value: RationalInteger,
    unit: &str,
    lemma_type: &LemmaType,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    if unit.is_empty() {
        Ok(value)
    } else {
        checked_mul(&value, lemma_type.quantity_unit_factor(unit)).map_err(map_numeric_failure)
    }
}

fn quantity_scale_magnitude_by_rational(
    magnitude: RationalInteger,
    unit: &str,
    lemma_type: &LemmaType,
    factor: &RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    let canonical = quantity_canonical_magnitude_rational(magnitude, unit, lemma_type)?;
    let scaled = checked_mul(&canonical, factor).map_err(map_numeric_failure)?;
    let unit_factor = lemma_type.quantity_unit_factor(unit);
    checked_div(&scaled, unit_factor).map_err(map_numeric_failure)
}

fn quantity_ratio_arithmetic(
    quantity_value: RationalInteger,
    quantity_unit: &str,
    quantity_type: &LemmaType,
    operator: &ArithmeticComputation,
    ratio_value: RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    let one = RationalInteger::new(1, 1);
    match operator {
        ArithmeticComputation::Add => {
            let factor = checked_add(&one, &ratio_value).map_err(map_numeric_failure)?;
            quantity_scale_magnitude_by_rational(
                quantity_value,
                quantity_unit,
                quantity_type,
                &factor,
            )
        }
        ArithmeticComputation::Subtract => {
            let factor = checked_sub(&one, &ratio_value).map_err(map_numeric_failure)?;
            quantity_scale_magnitude_by_rational(
                quantity_value,
                quantity_unit,
                quantity_type,
                &factor,
            )
        }
        ArithmeticComputation::Multiply => quantity_scale_magnitude_by_rational(
            quantity_value,
            quantity_unit,
            quantity_type,
            &ratio_value,
        ),
        ArithmeticComputation::Divide => {
            let canonical = quantity_canonical_magnitude_rational(
                quantity_value,
                quantity_unit,
                quantity_type,
            )?;
            let quotient = rational_operation_with_fallback(
                &canonical,
                NumericOperation::Divide,
                &ratio_value,
            )
            .map_err(map_numeric_failure)?;
            let unit_factor = quantity_type.quantity_unit_factor(quantity_unit);
            checked_div(&quotient, unit_factor).map_err(map_numeric_failure)
        }
        _ => unreachable!(
            "BUG: quantity {:?} ratio is rejected during planning",
            operator
        ),
    }
}

fn quantity_add_subtract(
    left_value: RationalInteger,
    left_unit: &str,
    left_type: &LemmaType,
    operator: &ArithmeticComputation,
    right_value: RationalInteger,
    right_unit: &str,
    right_type: &LemmaType,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    if left_unit == right_unit {
        return number_arithmetic(left_value, operator, right_value);
    }
    let left_canonical = quantity_canonical_magnitude_rational(left_value, left_unit, left_type)?;
    let right_canonical =
        quantity_canonical_magnitude_rational(right_value, right_unit, right_type)?;
    let canonical_result = rational_operation_with_fallback(
        &left_canonical,
        numeric_operation_from_arithmetic(operator),
        &right_canonical,
    )
    .map_err(map_numeric_failure)?;
    let unit_factor = left_type.quantity_unit_factor(left_unit);
    checked_div(&canonical_result, unit_factor).map_err(map_numeric_failure)
}

pub(crate) fn number_arithmetic(
    left: RationalInteger,
    operator: &ArithmeticComputation,
    right: RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    rational_operation_with_fallback(&left, numeric_operation_from_arithmetic(operator), &right)
        .map_err(|failure| {
            if matches!(operator, ArithmeticComputation::Modulo) {
                map_numeric_failure_modulo(failure)
            } else {
                map_numeric_failure(failure)
            }
        })
}

fn operate_on_operation_results(
    left_result: OperationResult,
    op: &ArithmeticComputation,
    right_result: OperationResult,
) -> OperationResult {
    let left_value = match left_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    let right_value = match right_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    arithmetic_operation(left_value.as_ref(), op, right_value.as_ref())
}

fn operate_with_left_result(
    left_result: OperationResult,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    let left_value = match left_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    arithmetic_operation(left_value.as_ref(), op, right)
}

fn operate_with_right_result(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right_result: OperationResult,
) -> OperationResult {
    let right_value = match right_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    arithmetic_operation(left, op, right_value.as_ref())
}

fn project_date_range_through_quantity(
    quantity_value: &LiteralValue,
    range_left: &LiteralValue,
    range_right: &LiteralValue,
) -> OperationResult {
    match date_range_projection_axis(quantity_value) {
        DateRangeProjectionAxis::Duration => super::range::compute_span(range_left, range_right),
        DateRangeProjectionAxis::Calendar => {
            let (ValueKind::Date(left_date), ValueKind::Date(right_date)) =
                (&range_left.value, &range_right.value)
            else {
                unreachable!(
                    "BUG: date range projection received non-date endpoints; planning should have rejected this"
                );
            };
            super::datetime::compute_date_calendar_difference(
                left_date,
                right_date,
                &SemanticCalendarUnit::Month,
            )
        }
    }
}

fn shift_date_range_right_endpoint(
    range_left: &LiteralValue,
    range_right: &LiteralValue,
    calendar_value: RationalInteger,
    calendar_unit: &SemanticCalendarUnit,
    add: bool,
) -> OperationResult {
    let (ValueKind::Date(_), ValueKind::Date(_)) = (&range_left.value, &range_right.value) else {
        unreachable!(
            "BUG: date range calendar arithmetic received non-date endpoints; planning should have rejected this"
        );
    };

    let calendar_literal = LiteralValue::calendar(calendar_value, calendar_unit.clone());
    let shifted_right = match super::datetime::datetime_arithmetic(
        range_right,
        if add {
            &ArithmeticComputation::Add
        } else {
            &ArithmeticComputation::Subtract
        },
        &calendar_literal,
    ) {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };

    OperationResult::Value(Box::new(LiteralValue::range(
        range_left.clone(),
        shifted_right.as_ref().clone(),
    )))
}

fn shift_calendar_range_right_endpoint(
    range_left: &LiteralValue,
    range_right: &LiteralValue,
    calendar_value: RationalInteger,
    calendar_unit: &SemanticCalendarUnit,
    add: bool,
) -> OperationResult {
    let (ValueKind::Calendar(_, _), ValueKind::Calendar(_, _)) =
        (&range_left.value, &range_right.value)
    else {
        unreachable!(
            "BUG: calendar range calendar arithmetic received non-calendar endpoints; planning should have rejected this"
        );
    };

    let calendar_literal = LiteralValue::calendar(calendar_value, calendar_unit.clone());
    let shifted_right = match arithmetic_operation(
        range_right,
        if add {
            &ArithmeticComputation::Add
        } else {
            &ArithmeticComputation::Subtract
        },
        &calendar_literal,
    ) {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };

    OperationResult::Value(Box::new(LiteralValue::range(
        range_left.clone(),
        shifted_right.as_ref().clone(),
    )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateRangeProjectionAxis {
    Duration,
    Calendar,
}

fn date_range_projection_axis(quantity_value: &LiteralValue) -> DateRangeProjectionAxis {
    let quantity_decomposition = quantity_value.lemma_type.quantity_type_decomposition();
    let has_duration_axis = quantity_decomposition
        .get(DURATION_DIMENSION)
        .is_some_and(|exponent| *exponent != 0);
    let has_calendar_axis = quantity_decomposition
        .get(CALENDAR_DIMENSION)
        .is_some_and(|exponent| *exponent != 0);

    match (has_duration_axis, has_calendar_axis) {
        (true, false) => DateRangeProjectionAxis::Duration,
        (false, true) => DateRangeProjectionAxis::Calendar,
        (false, false) => unreachable!(
            "BUG: quantity without temporal axis reached date range projection; planning should have rejected this"
        ),
        (true, true) => unreachable!(
            "BUG: quantity with ambiguous temporal axes reached date range projection; planning should have rejected this"
        ),
    }
}

fn type_name(value: &LiteralValue) -> String {
    value.get_type().name().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::RationalInteger;
    use crate::evaluation::operations::{OperationResult, VetoType};
    use crate::planning::semantics::{ArithmeticComputation, LiteralValue, ValueKind};
    use rust_decimal::Decimal;

    #[test]
    fn number_arithmetic_add_on_stored_decimals() {
        let sum = number_arithmetic(
            RationalInteger::new(1, 1),
            &ArithmeticComputation::Add,
            RationalInteger::new(2, 1),
        )
        .unwrap();
        assert_eq!(sum, RationalInteger::new(3, 1));
    }

    #[test]
    fn number_arithmetic_divide_ten_by_three_returns_value_decimal() {
        let quotient = number_arithmetic(
            RationalInteger::new(10, 1),
            &ArithmeticComputation::Divide,
            RationalInteger::new(3, 1),
        )
        .unwrap();
        let decimal = crate::computation::rational::commit_rational_to_decimal(&quotient).unwrap();
        assert!(decimal > Decimal::from(3));
        assert!(decimal < Decimal::from(4));
    }

    #[test]
    fn number_arithmetic_division_by_zero_returns_failure() {
        let failure = number_arithmetic(
            RationalInteger::new(10, 1),
            &ArithmeticComputation::Divide,
            RationalInteger::new(0, 1),
        )
        .unwrap_err();
        assert_eq!(failure, NumberArithmeticFailure::DivisionByZero);
    }

    #[test]
    fn arithmetic_operation_adds_primitive_numbers() {
        use crate::computation::rational::decimal_to_rational;
        use rust_decimal::Decimal;
        let left = LiteralValue::number(decimal_to_rational(Decimal::new(11, 1)).unwrap());
        let right = LiteralValue::number(decimal_to_rational(Decimal::new(9, 1)).unwrap());
        let OperationResult::Value(lit) =
            arithmetic_operation(&left, &ArithmeticComputation::Add, &right)
        else {
            panic!("expected value");
        };
        match &lit.value {
            ValueKind::Number(n) => {
                assert_eq!(*n, decimal_to_rational(Decimal::new(2, 0)).unwrap())
            }
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn arithmetic_operation_propagates_veto_from_left() {
        let left = OperationResult::Veto(VetoType::computation("left failed"));
        let right = LiteralValue::number(RationalInteger::new(1, 1));
        let result = operate_on_operation_results(
            left,
            &ArithmeticComputation::Add,
            OperationResult::Value(Box::new(right)),
        );
        assert!(matches!(result, OperationResult::Veto(_)));
    }
}
