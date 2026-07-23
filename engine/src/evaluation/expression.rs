//! Shared evaluation helpers used by the tree evaluator.
//!
//! Domain failures (division by zero, etc.) surface as Veto, not Error.

use super::operations::{OperationResult, VetoType};
use crate::computation::measure_math::{
    mathematical_computation_preserves_measure_magnitude, measure_magnitude_math,
};
use crate::planning::semantics::{LiteralValue, MathematicalComputation, ValueKind};
use std::sync::Arc;

pub(crate) fn evaluate_mathematical_operator(
    op: &MathematicalComputation,
    value: &LiteralValue,
) -> OperationResult {
    use crate::computation::decimal_math::{decimal_acos, decimal_asin, decimal_atan};
    use rust_decimal::MathematicalOps;

    if matches!(&value.value, ValueKind::Measure(_, _))
        && mathematical_computation_preserves_measure_magnitude(op)
    {
        return measure_magnitude_math(op, value);
    }

    match &value.value {
        ValueKind::Number(stored_rational) => {
            use crate::computation::rational::decimal_to_rational;
            let stored_decimal = match stored_rational.try_to_decimal() {
                Ok(decimal) => decimal,
                Err(crate::computation::rational::NumericFailure::Overflow) => {
                    return OperationResult::Veto(VetoType::computation(
                        "Calculated result exceeds decimal value limit",
                    ));
                }
                Err(failure) => {
                    return OperationResult::Veto(VetoType::computation(failure.to_string()));
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

            let rounded_decimal = match decimal_result {
                Some(rounded) => rounded,
                None => {
                    return OperationResult::Veto(VetoType::computation(
                        "Mathematical operation result is undefined for this input",
                    ));
                }
            };

            let result_rational = decimal_to_rational(rounded_decimal)
                .expect("BUG: transcendental result must lift back to stored rational");
            let result_value =
                LiteralValue::number_with_type(result_rational, value.lemma_type.clone());
            OperationResult::from_literal(result_value)
        }
        _ => unreachable!(
            "BUG: mathematical operator with non-number operand; planning should have rejected this"
        ),
    }
}

pub(crate) fn resolve_data_path_value(
    data_path: &crate::planning::semantics::DataPath,
    plan: &crate::planning::execution_plan::ExecutionPlan,
    context: &crate::evaluation::EvaluationContext,
) -> OperationResult {
    if let Some(veto) = context.get_veto(data_path) {
        return OperationResult::Veto(veto.clone());
    }
    if let Some(value) = context.get_data_value(data_path) {
        return OperationResult::from_literal_arc(Arc::clone(value));
    }
    if let Some(rule_path) =
        crate::planning::normalize::follow_data_reference_to_rule_target(&plan.data, data_path)
    {
        panic!(
            "BUG: rule-target data path '{}' (→ rule '{}') reached evaluation; planning must inline these",
            data_path, rule_path.rule
        );
    }
    OperationResult::Veto(VetoType::missing_data(
        data_path.clone(),
        context.missing_data_suggestion(data_path),
    ))
}
