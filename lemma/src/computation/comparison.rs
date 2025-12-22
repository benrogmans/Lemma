//! Type-aware comparison operations
//!
//! Pure functions for comparisons on different types: Number, Text, Boolean, Unit, etc.

use crate::evaluation::OperationResult;
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

        (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => {
                OperationResult::Value(LiteralValue::Boolean((l == r).into()))
            }
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => {
                OperationResult::Value(LiteralValue::Boolean((l != r).into()))
            }
            _ => OperationResult::Veto(Some("Can only use == and != with booleans".to_string())),
        },

        (LiteralValue::Text(l), LiteralValue::Text(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => {
                OperationResult::Value(LiteralValue::Boolean((l == r).into()))
            }
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => {
                OperationResult::Value(LiteralValue::Boolean((l != r).into()))
            }
            _ => OperationResult::Veto(Some("Can only use == and != with text".to_string())),
        },

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
        ComparisonComputation::Equal | ComparisonComputation::Is => left == *right,
        ComparisonComputation::NotEqual | ComparisonComputation::IsNot => left != *right,
    }
}

fn type_name(value: &LiteralValue) -> String {
    value.to_type().to_string()
}
