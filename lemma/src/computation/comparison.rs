//! Type-aware comparison operations

use crate::evaluation::OperationResult;
use crate::semantic::standard_boolean;
use crate::{ComparisonComputation, LiteralValue, Value};
use rust_decimal::Decimal;

/// Perform type-aware comparison, returning OperationResult (Veto on error)
pub fn comparison_operation(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (&left.value, &right.value) {
        (Value::Number(l), Value::Number(r)) => {
            OperationResult::Value(LiteralValue::boolean(compare_decimals(*l, op, r).into()))
        }

        (Value::Boolean(l), Value::Boolean(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => {
                OperationResult::Value(LiteralValue::boolean((l == r).into()))
            }
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => {
                OperationResult::Value(LiteralValue {
                    value: Value::Boolean((l != r).into()),
                    lemma_type: standard_boolean().clone(),
                })
            }
            _ => unreachable!(
                "BUG: invalid boolean comparison operator {}; this should be rejected during planning",
                op
            ),
        },

        (Value::Text(l), Value::Text(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => {
                OperationResult::Value(LiteralValue::boolean((l == r).into()))
            }
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => {
                OperationResult::Value(LiteralValue {
                    value: Value::Boolean((l != r).into()),
                    lemma_type: standard_boolean().clone(),
                })
            }
            _ => unreachable!(
                "BUG: invalid text comparison operator {}; this should be rejected during planning",
                op
            ),
        },

        (Value::Ratio(l, _), Value::Ratio(r, _)) => {
            OperationResult::Value(LiteralValue::boolean(compare_decimals(*l, op, r).into()))
        }
        (Value::Scale(l, lu_opt), Value::Scale(r, ru_opt)) => {
            if left.lemma_type != right.lemma_type {
                unreachable!(
                    "BUG: compared different scale types ({} vs {}); this should be rejected during planning",
                    left.lemma_type.name(),
                    right.lemma_type.name()
                );
            }

            match (lu_opt, ru_opt) {
                (Some(lu), Some(ru)) => {
                    if lu.eq_ignore_ascii_case(ru) {
                        return OperationResult::Value(LiteralValue::boolean(
                            compare_decimals(*l, op, r).into(),
                        ));
                    }

                    let target = crate::semantic::ConversionTarget::ScaleUnit(lu.clone());
                    match super::units::convert_unit(right, &target) {
                        OperationResult::Value(converted) => match converted.value {
                            Value::Scale(converted_value, _) => OperationResult::Value(
                                LiteralValue::boolean(compare_decimals(*l, op, &converted_value).into()),
                            ),
                            _ => unreachable!(
                                "BUG: scale unit conversion returned non-scale value"
                            ),
                        },
                        OperationResult::Veto(msg) => unreachable!(
                            "BUG: scale unit conversion vetoed unexpectedly: {:?}",
                            msg
                        ),
                    }
                }
                (None, None) => OperationResult::Value(LiteralValue::boolean(
                    compare_decimals(*l, op, r).into(),
                )),
                (Some(_), None) | (None, Some(_)) => unreachable!(
                    "BUG: scale value missing unit (left={:?}, right={:?}); this should be rejected during input validation/planning",
                    lu_opt,
                    ru_opt
                ),
            }
        }

        (Value::Date(_), Value::Date(_)) => super::datetime::datetime_comparison(left, op, right),
        (Value::Time(_), Value::Time(_)) => super::datetime::time_comparison(left, op, right),

        // Duration comparison
        (Value::Duration(l, lu), Value::Duration(r, ru)) => {
            let left_seconds = super::units::duration_to_seconds(*l, lu);
            let right_seconds = super::units::duration_to_seconds(*r, ru);
            OperationResult::Value(LiteralValue::boolean(
                compare_decimals(left_seconds, op, &right_seconds).into(),
            ))
        }

        // Duration with number
        (Value::Duration(value, _), Value::Number(n)) => OperationResult::Value(
            LiteralValue::boolean(compare_decimals(*value, op, n).into()),
        ),
        (Value::Number(n), Value::Duration(value, _)) => OperationResult::Value(
            LiteralValue::boolean(compare_decimals(*n, op, value).into()),
        ),

        _ => unreachable!(
            "BUG: unsupported comparison during evaluation: {} {} {}",
            type_name(left),
            op,
            type_name(right)
        ),
    }
}

fn compare_decimals(left: Decimal, op: &ComparisonComputation, right: &Decimal) -> bool {
    match op {
        ComparisonComputation::GreaterThan => left > *right,
        ComparisonComputation::LessThan => left < *right,
        ComparisonComputation::GreaterThanOrEqual => left >= *right,
        ComparisonComputation::LessThanOrEqual => left <= *right,
        ComparisonComputation::Equal | ComparisonComputation::Is => left == *right,
        ComparisonComputation::NotEqual | ComparisonComputation::IsNot => left != *right,
    }
}

fn type_name(value: &LiteralValue) -> String {
    value.get_type().name().to_string()
}
