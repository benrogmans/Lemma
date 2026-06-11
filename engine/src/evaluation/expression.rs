//! Shared evaluation helpers used by the instruction VM.
//!
//! All runtime errors (division by zero, etc.) result in Veto instead of errors.

use super::operations::{OperationResult, VetoType};
use crate::computation::quantity_math::{
    mathematical_computation_preserves_quantity_magnitude, quantity_magnitude_math,
};
use crate::planning::semantics::{LiteralValue, MathematicalComputation, ValueKind};

pub(crate) fn evaluate_mathematical_operator(
    op: &MathematicalComputation,
    value: &LiteralValue,
) -> OperationResult {
    use crate::computation::decimal_math::{decimal_acos, decimal_asin, decimal_atan};
    use rust_decimal::MathematicalOps;

    if matches!(&value.value, ValueKind::Quantity(_, _))
        && mathematical_computation_preserves_quantity_magnitude(op)
    {
        return quantity_magnitude_math(op, value);
    }

    match &value.value {
        ValueKind::Number(stored_rational) => {
            use crate::computation::rational::{commit_rational_to_decimal, decimal_to_rational};
            let stored_decimal = match commit_rational_to_decimal(stored_rational) {
                Ok(decimal) => decimal,
                Err(_) => {
                    return OperationResult::Veto(VetoType::computation(
                        "Mathematical operation requires a decimal-representable input",
                    ));
                }
            };
            let decimal_result: Option<rust_decimal::Decimal> = match op {
                MathematicalComputation::Abs => Some(stored_decimal.abs()),
                MathematicalComputation::Floor => Some(stored_decimal.floor()),
                MathematicalComputation::Ceil => Some(stored_decimal.ceil()),
                MathematicalComputation::Round => Some(stored_decimal.round()),
                MathematicalComputation::Sqrt => stored_decimal.sqrt(),
                MathematicalComputation::Sin => stored_decimal.checked_sin(),
                MathematicalComputation::Cos => stored_decimal.checked_cos(),
                MathematicalComputation::Tan => stored_decimal.checked_tan(),
                MathematicalComputation::Log => stored_decimal.checked_ln(),
                MathematicalComputation::Exp => stored_decimal.checked_exp(),
                MathematicalComputation::Asin => decimal_asin(stored_decimal),
                MathematicalComputation::Acos => decimal_acos(stored_decimal),
                MathematicalComputation::Atan => decimal_atan(stored_decimal),
            };

            let committed_decimal = match decimal_result {
                Some(committed) => committed,
                None => {
                    return OperationResult::Veto(VetoType::computation(
                        "Mathematical operation result is undefined for this input",
                    ));
                }
            };

            let result_rational = decimal_to_rational(committed_decimal)
                .expect("BUG: transcendental result must lift back to stored rational");
            let result_value =
                LiteralValue::number_with_type(result_rational, value.lemma_type.clone());
            OperationResult::Value(result_value)
        }
        _ => unreachable!(
            "BUG: mathematical operator with non-number operand; planning should have rejected this"
        ),
    }
}

pub(crate) fn resolve_data_path_value<'plan>(
    data_path: &crate::planning::semantics::DataPath,
    context: &crate::evaluation::EvaluationContext<'plan>,
) -> OperationResult {
    if let Some(veto) = context.get_veto(data_path) {
        return OperationResult::Veto(veto.clone());
    }
    if let Some(value) = context.get_data_value(data_path) {
        return OperationResult::Value(value.clone());
    }
    if let Some(rule_path) = crate::planning::normalize::follow_data_reference_to_rule_target(
        &context.plan().data,
        data_path,
    ) {
        return context
            .rule_results
            .get(&rule_path)
            .cloned()
            .unwrap_or_else(|| {
                unreachable!(
                    "BUG: rule-target reference '{}' read before target rule '{}' evaluated",
                    data_path, rule_path.rule
                )
            });
    }
    OperationResult::Veto(VetoType::MissingData {
        data: data_path.clone(),
    })
}
