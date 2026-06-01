//! Type-aware comparison operations

use crate::computation::rational::RationalInteger;
use crate::computation::UnitResolutionContext;
use crate::evaluation::OperationResult;
use crate::planning::semantics::{
    primitive_boolean_arc, ComparisonComputation, LiteralValue, ValueKind,
};

/// Perform type-aware comparison, returning OperationResult (Veto on error)
pub fn comparison_operation(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
    unit_context: UnitResolutionContext<'_>,
) -> OperationResult {
    let _ = unit_context;
    match (&left.value, &right.value) {
        (ValueKind::Range(range_left, range_right), ValueKind::Quantity(_, sig))
            if left.lemma_type.is_date_range() && right.lemma_type.is_calendar_like() =>
        {
            let (ValueKind::Date(left_date), ValueKind::Date(right_date)) =
                (&range_left.value, &range_right.value)
            else {
                unreachable!(
                    "BUG: date range calendar comparison received non-date endpoints; planning should have rejected this"
                );
            };
            let calendar_unit =
                crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(sig);
            let measure = super::datetime::compute_date_calendar_difference(
                left_date,
                right_date,
                &calendar_unit,
                right.lemma_type.clone(),
            );
            compare_with_operation_result(measure, op, right)
        }

        (ValueKind::Quantity(_, sig), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_date_range() && left.lemma_type.is_calendar_like() =>
        {
            let (ValueKind::Date(left_date), ValueKind::Date(right_date)) =
                (&range_left.value, &range_right.value)
            else {
                unreachable!(
                    "BUG: date range calendar comparison received non-date endpoints; planning should have rejected this"
                );
            };
            let calendar_unit =
                crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(sig);
            let measure = super::datetime::compute_date_calendar_difference(
                left_date,
                right_date,
                &calendar_unit,
                left.lemma_type.clone(),
            );
            compare_with_right_result(left, op, measure)
        }

        (ValueKind::Range(range_left, range_right), _) => {
            let measure =
                super::range::compute_quantity(range_left.as_ref(), range_right.as_ref(), Some(right));
            compare_with_operation_result(measure, op, right)
        }

        (ValueKind::Number(l), ValueKind::Number(r)) => OperationResult::Value(Box::new(
            LiteralValue::from_bool(compare_stored_rationals(l, op, r)),
        )),

        (ValueKind::Boolean(l), ValueKind::Boolean(r)) => match op {
            ComparisonComputation::Is => {
                OperationResult::Value(Box::new(LiteralValue::from_bool(l == r)))
            }
            ComparisonComputation::IsNot => {
                OperationResult::Value(Box::new(LiteralValue {
                    value: ValueKind::Boolean(l != r),
                    lemma_type: primitive_boolean_arc().clone(),
                }))
            }
            _ => unreachable!(
                "BUG: invalid boolean comparison operator {}; this should be rejected during planning",
                op
            ),
        },

        (ValueKind::Text(l), ValueKind::Text(r)) => match op {
            ComparisonComputation::Is => {
                OperationResult::Value(Box::new(LiteralValue::from_bool(l == r)))
            }
            ComparisonComputation::IsNot => {
                OperationResult::Value(Box::new(LiteralValue {
                    value: ValueKind::Boolean(l != r),
                    lemma_type: primitive_boolean_arc().clone(),
                }))
            }
            _ => unreachable!(
                "BUG: invalid text comparison operator {}; this should be rejected during planning",
                op
            ),
        },

        (ValueKind::Ratio(l, _), ValueKind::Ratio(r, _)) => OperationResult::Value(Box::new(
            LiteralValue::from_bool(compare_stored_rationals(l, op, r)),
        )),
        (ValueKind::Quantity(left_value, _), ValueKind::Quantity(right_value, _))
            if left.lemma_type.is_calendar_like() && right.lemma_type.is_calendar_like() =>
        {
            OperationResult::Value(Box::new(LiteralValue::from_bool(
                compare_stored_rationals(left_value, op, right_value),
            )))
        }

        (ValueKind::Quantity(l, _), ValueKind::Quantity(r, _)) => {
            let l_decomp = left
                .lemma_type
                .quantity_type_decomposition()
                .expect("BUG: decomposition must be resolved after planning");
            let r_decomp = right
                .lemma_type
                .quantity_type_decomposition()
                .expect("BUG: decomposition must be resolved after planning");
            let same_decomp = l_decomp == r_decomp;
            if !left.lemma_type.same_quantity_family(&right.lemma_type)
                && !left.lemma_type.compatible_with_anonymous_quantity(&right.lemma_type)
                && !same_decomp
            {
                unreachable!(
                    "BUG: compared incompatible quantity types ({} vs {}); planning must reject this",
                    left.lemma_type.name(),
                    right.lemma_type.name()
                );
            }
            OperationResult::Value(Box::new(LiteralValue::from_bool(
                compare_stored_rationals(l, op, r),
            )))
        }

        (ValueKind::Date(_), ValueKind::Date(_)) => super::datetime::datetime_comparison(left, op, right),
        (ValueKind::Time(_), ValueKind::Time(_)) => super::datetime::time_comparison(left, op, right),

        (ValueKind::Quantity(value, _), ValueKind::Number(n))
            if left.lemma_type.is_duration_like_quantity() =>
        {
            OperationResult::Value(Box::new(LiteralValue::from_bool(compare_stored_rationals(
                value, op, n,
            ))))
        }
        (ValueKind::Number(n), ValueKind::Quantity(value, _))
            if right.lemma_type.is_duration_like_quantity() =>
        {
            OperationResult::Value(Box::new(LiteralValue::from_bool(compare_stored_rationals(
                n, op, value,
            ))))
        }
        (ValueKind::Quantity(value, _), ValueKind::Number(n))
            if left.lemma_type.is_calendar_like() =>
        {
            OperationResult::Value(Box::new(LiteralValue::from_bool(compare_stored_rationals(
                value, op, n,
            ))))
        }
        (ValueKind::Number(n), ValueKind::Quantity(value, _))
            if right.lemma_type.is_calendar_like() =>
        {
            OperationResult::Value(Box::new(LiteralValue::from_bool(compare_stored_rationals(
                n, op, value,
            ))))
        }

        _ => unreachable!(
            "BUG: unsupported comparison during evaluation: {} {} {}",
            type_name(left),
            op,
            type_name(right)
        ),
    }
}

fn compare_stored_rationals(
    left: &RationalInteger,
    op: &ComparisonComputation,
    right: &RationalInteger,
) -> bool {
    let ordering = left.cmp(right);
    match op {
        ComparisonComputation::GreaterThan => ordering == std::cmp::Ordering::Greater,
        ComparisonComputation::LessThan => ordering == std::cmp::Ordering::Less,
        ComparisonComputation::GreaterThanOrEqual => ordering != std::cmp::Ordering::Less,
        ComparisonComputation::LessThanOrEqual => ordering != std::cmp::Ordering::Greater,
        ComparisonComputation::Is => ordering == std::cmp::Ordering::Equal,
        ComparisonComputation::IsNot => ordering != std::cmp::Ordering::Equal,
    }
}

fn compare_with_operation_result(
    left_result: OperationResult,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> OperationResult {
    let left_value = match left_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    comparison_operation(
        left_value.as_ref(),
        op,
        right,
        UnitResolutionContext::NamedQuantityOnly,
    )
}

fn compare_with_right_result(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right_result: OperationResult,
) -> OperationResult {
    let right_value = match right_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    comparison_operation(
        left,
        op,
        right_value.as_ref(),
        UnitResolutionContext::NamedQuantityOnly,
    )
}

fn type_name(value: &LiteralValue) -> String {
    value.get_type().name().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::RationalInteger;
    use crate::evaluation::operations::OperationResult;
    use crate::planning::semantics::{ComparisonComputation, LiteralValue, ValueKind};

    fn eval_bool(left: &LiteralValue, op: &ComparisonComputation, right: &LiteralValue) -> bool {
        let OperationResult::Value(lit) =
            comparison_operation(left, op, right, UnitResolutionContext::NamedQuantityOnly)
        else {
            panic!("expected boolean value");
        };
        match &lit.value {
            ValueKind::Boolean(b) => *b,
            other => panic!("expected boolean, got {other:?}"),
        }
    }

    #[test]
    fn number_less_than_uses_exact_rational_ordering() {
        let small = LiteralValue::number(RationalInteger::new(1, 10));
        let large = LiteralValue::number(RationalInteger::new(2, 1));
        assert!(eval_bool(&small, &ComparisonComputation::LessThan, &large));
        assert!(!eval_bool(&large, &ComparisonComputation::LessThan, &small));
    }

    #[test]
    fn text_is_and_is_not() {
        let a = LiteralValue::text("alpha".to_string());
        let b = LiteralValue::text("beta".to_string());
        assert!(eval_bool(&a, &ComparisonComputation::Is, &a));
        assert!(!eval_bool(&a, &ComparisonComputation::Is, &b));
        assert!(eval_bool(&a, &ComparisonComputation::IsNot, &b));
    }

    #[test]
    fn boolean_is_only_accepts_is_operators() {
        let t = LiteralValue::from_bool(true);
        let f = LiteralValue::from_bool(false);
        assert!(eval_bool(&t, &ComparisonComputation::Is, &t));
        assert!(eval_bool(&t, &ComparisonComputation::IsNot, &f));
    }
}
