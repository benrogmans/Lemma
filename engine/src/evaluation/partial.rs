//! Three-valued partial expression evaluation for branch liveness analysis.
//!
//! These functions determine whether a normalized branch condition is definitely
//! false given already-known data values. They are conservative: any unknown
//! operand or unsupported expression form returns [`None`] (indeterminate),
//! which keeps the branch alive. Only `Some(false)` means a branch is dead.
//!
//! Reuses the same computation functions as the full evaluator so precision
//! guarantees (exact rational arithmetic, unit conversion) are identical.

use crate::computation::{
    arithmetic_operation, comparison_operation, convert_unit, UnitResolutionContext,
};
use crate::evaluation::OperationResult;
use crate::planning::semantics::{DataPath, Expression, ExpressionKind, LiteralValue, ValueKind};
use crate::planning::ExecutionPlan;
use std::collections::HashMap;

/// Attempt to resolve an expression to a concrete [`LiteralValue`] given a set
/// of already-known data values.
///
/// Returns `None` when any required operand is missing from `known_values`, when
/// the expression references a rule result (unavailable at schema time), or when
/// the expression kind is not supported for partial evaluation (date-relative,
/// range containment, mathematical functions, etc.). Conservative: `None` means
/// the expression cannot be simplified, not that it is invalid.
///
/// Boolean-producing operators (`LogicalAnd`, `LogicalOr`, `LogicalNegation`)
/// are intentionally excluded here — use [`try_evaluate_condition`] for those.
pub(crate) fn try_resolve_value(
    expr: &Expression,
    known_values: &HashMap<DataPath, LiteralValue>,
    plan: &ExecutionPlan,
) -> Option<LiteralValue> {
    let unit_index = plan.expression_unit_index();

    match &expr.kind {
        ExpressionKind::Literal(literal) => Some(*literal.clone()),

        ExpressionKind::DataPath(data_path) => known_values.get(data_path).cloned(),

        ExpressionKind::Comparison(left_expr, op, right_expr) => {
            let left_value = try_resolve_value(left_expr, known_values, plan)?;
            let right_value = try_resolve_value(right_expr, known_values, plan)?;
            match comparison_operation(
                &left_value,
                op,
                &right_value,
                UnitResolutionContext::WithIndex(unit_index),
            ) {
                OperationResult::Value(result) => Some(result),
                OperationResult::Veto(_) => None,
            }
        }

        ExpressionKind::Arithmetic(left_expr, op, right_expr) => {
            let left_value = try_resolve_value(left_expr, known_values, plan)?;
            let right_value = try_resolve_value(right_expr, known_values, plan)?;
            match arithmetic_operation(
                &left_value,
                op,
                &right_value,
                unit_index,
                &plan.signature_index,
            ) {
                OperationResult::Value(result) => Some(result),
                OperationResult::Veto(_) => None,
            }
        }

        ExpressionKind::UnitConversion(inner_expr, target) => {
            let inner_value = try_resolve_value(inner_expr, known_values, plan)?;
            match convert_unit(
                &inner_value,
                target,
                UnitResolutionContext::WithIndex(unit_index),
            ) {
                OperationResult::Value(result) => Some(result),
                OperationResult::Veto(_) => None,
            }
        }

        // Rule results are unavailable at schema time.
        ExpressionKind::RulePath(_)
        // Veto expressions do not produce a value.
        | ExpressionKind::Veto(_)
        // `now` requires an evaluation-time date.
        | ExpressionKind::Now
        // Date-relative and calendar period expressions require evaluation-time date.
        | ExpressionKind::DateRelative(_, _)
        | ExpressionKind::DateCalendar(_, _, _)
        | ExpressionKind::PastFutureRange(_, _)
        // Range construction and containment require dedicated range logic.
        | ExpressionKind::RangeLiteral(_, _)
        | ExpressionKind::RangeContainment(_, _)
        // Mathematical functions (sqrt, abs, etc.) are not needed for condition pruning.
        | ExpressionKind::MathematicalComputation(_, _)
        // ResultIsVeto requires evaluating a rule result.
        | ExpressionKind::ResultIsVeto(_)
        // Boolean operators belong to try_evaluate_condition, not here.
        | ExpressionKind::LogicalAnd(_, _)
        | ExpressionKind::LogicalOr(_, _)
        | ExpressionKind::LogicalNegation(_, _)
        | ExpressionKind::Piecewise(_) => None,
    }
}

/// Attempt to evaluate a boolean condition using three-valued logic.
///
/// - `Some(false)` — condition is definitely false; branch is dead and can be pruned.
/// - `Some(true)` — condition is definitely true; branch is definitely live.
/// - `None` — indeterminate (some operand unknown or expression unsupported);
///   branch is kept alive to remain conservative.
///
/// Short-circuit rules applied before both sides are fully evaluated:
/// - `AND`: either side `Some(false)` → `Some(false)` (dominates).
/// - `OR`: either side `Some(true)` → `Some(true)` (dominates).
/// - `NOT`: negates a deterministic result; `None` propagates as `None`.
pub(crate) fn try_evaluate_condition(
    expr: &Expression,
    known_values: &HashMap<DataPath, LiteralValue>,
    plan: &ExecutionPlan,
) -> Option<bool> {
    match &expr.kind {
        ExpressionKind::LogicalAnd(left_expr, right_expr) => {
            let left_result = try_evaluate_condition(left_expr, known_values, plan);
            let right_result = try_evaluate_condition(right_expr, known_values, plan);
            match (left_result, right_result) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            }
        }

        ExpressionKind::LogicalOr(left_expr, right_expr) => {
            let left_result = try_evaluate_condition(left_expr, known_values, plan);
            let right_result = try_evaluate_condition(right_expr, known_values, plan);
            match (left_result, right_result) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            }
        }

        ExpressionKind::LogicalNegation(inner_expr, _negation_type) => {
            try_evaluate_condition(inner_expr, known_values, plan).map(|b| !b)
        }

        // For any other expression kind, delegate to value resolution and extract boolean.
        _ => {
            let value = try_resolve_value(expr, known_values, plan)?;
            match value.value {
                ValueKind::Boolean(boolean) => Some(boolean),
                _ => None,
            }
        }
    }
}
