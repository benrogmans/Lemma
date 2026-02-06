//! Type-aware comparison operations

use crate::evaluation::OperationResult;
use crate::planning::semantics::{
    primitive_boolean, ComparisonComputation, LiteralValue, SemanticConversionTarget, ValueKind,
};
use rust_decimal::Decimal;

/// Perform type-aware comparison, returning OperationResult (Veto on error)
pub fn comparison_operation(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (&left.value, &right.value) {
        (ValueKind::Number(l), ValueKind::Number(r)) => {
            OperationResult::Value(Box::new(LiteralValue::from_bool(compare_decimals(*l, op, r))))
        }

        (ValueKind::Boolean(l), ValueKind::Boolean(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => {
                OperationResult::Value(Box::new(LiteralValue::from_bool(l == r)))
            }
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => {
                OperationResult::Value(Box::new(LiteralValue {
                    value: ValueKind::Boolean(l != r),
                    lemma_type: primitive_boolean().clone(),
                }))
            }
            _ => unreachable!(
                "BUG: invalid boolean comparison operator {}; this should be rejected during planning",
                op
            ),
        },

        (ValueKind::Text(l), ValueKind::Text(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => {
                OperationResult::Value(Box::new(LiteralValue::from_bool(l == r)))
            }
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => {
                OperationResult::Value(Box::new(LiteralValue {
                    value: ValueKind::Boolean(l != r),
                    lemma_type: primitive_boolean().clone(),
                }))
            }
            _ => unreachable!(
                "BUG: invalid text comparison operator {}; this should be rejected during planning",
                op
            ),
        },

        (ValueKind::Ratio(l, _), ValueKind::Ratio(r, _)) => {
            OperationResult::Value(Box::new(LiteralValue::from_bool(compare_decimals(*l, op, r))))
        }
        (ValueKind::Scale(l, lu), ValueKind::Scale(r, ru)) => {
            if !left.lemma_type.same_scale_family(&right.lemma_type) {
                unreachable!(
                    "BUG: compared different scale families ({} vs {}); this should be rejected during planning",
                    left.lemma_type.name(),
                    right.lemma_type.name()
                );
            }

            if lu.eq_ignore_ascii_case(ru) {
                return OperationResult::Value(Box::new(LiteralValue::from_bool(
                    compare_decimals(*l, op, r),
                )));
            }

            // Convert right to left's unit for comparison
            let target = SemanticConversionTarget::ScaleUnit(lu.clone());
            match super::units::convert_unit(right, &target) {
                OperationResult::Value(converted) => match converted.as_ref().value {
                    ValueKind::Scale(converted_value, _) => OperationResult::Value(Box::new(
                        LiteralValue::from_bool(compare_decimals(*l, op, &converted_value)),
                    )),
                    _ => unreachable!("BUG: scale unit conversion returned non-scale value"),
                },
                OperationResult::Veto(msg) => {
                    unreachable!("BUG: scale unit conversion vetoed unexpectedly: {:?}", msg)
                }
            }
        }

        (ValueKind::Date(_), ValueKind::Date(_)) => super::datetime::datetime_comparison(left, op, right),
        (ValueKind::Time(_), ValueKind::Time(_)) => super::datetime::time_comparison(left, op, right),

        // Duration comparison
        (ValueKind::Duration(l, lu), ValueKind::Duration(r, ru)) => {
            let left_seconds = super::units::duration_to_seconds(*l, lu);
            let right_seconds = super::units::duration_to_seconds(*r, ru);
            OperationResult::Value(Box::new(LiteralValue::from_bool(
                compare_decimals(left_seconds, op, &right_seconds),
            )))
        }

        // Duration with number
        (ValueKind::Duration(value, _), ValueKind::Number(n)) => OperationResult::Value(Box::new(
            LiteralValue::from_bool(compare_decimals(*value, op, n)),
        )),
        (ValueKind::Number(n), ValueKind::Duration(value, _)) => OperationResult::Value(Box::new(
            LiteralValue::from_bool(compare_decimals(*n, op, value)),
        )),

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
