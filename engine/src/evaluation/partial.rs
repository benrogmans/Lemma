//! Partial expression evaluation for schema branch skip.
//!
//! [`DataPath`](crate::planning::semantics::DataPath) operands use
//! [`DataOverlay::supplied_value`](crate::planning::execution_plan::DataOverlay) only;
//! spec prefilled values are ignored. Used by
//! [`ExecutionPlan::collect_needed_data_paths`](crate::planning::execution_plan::ExecutionPlan)
//! to skip branch arms whose unless conditions are already decided — not by the VM.
//!
//! # Three-valued results
//!
//! - `Some(false)` — unless arm cannot apply; skip branch.
//! - `Some(true)` — unless arm definitely applies (default arm skipped when any unless is true).
//! - `None` — indeterminate; keep branch (conservative).
//!
//! # Boolean short-circuit
//!
//! Symmetric AND/OR short-circuit is intentional for unless-arm analysis
//! (e.g. `flag and (1 > 2)` is `Some(false)` even when `flag` is unbound).

use crate::computation::{
    arithmetic_operation, comparison_operation, convert_unit, UnitResolutionContext,
};
use crate::evaluation::OperationResult;
use crate::planning::execution_plan::{DataOverlay, ExecutionPlan};
use crate::planning::semantics::{Expression, ExpressionKind, LiteralValue, ValueKind};

pub(crate) fn resolve_expression_value(
    expression: &Expression,
    plan: &ExecutionPlan,
    overlay: &DataOverlay,
) -> Option<LiteralValue> {
    let unit_index = plan.expression_unit_index();

    match &expression.kind {
        ExpressionKind::Literal(literal) => Some(*literal.clone()),

        ExpressionKind::DataPath(data_path) => overlay.supplied_value(data_path).cloned(),

        ExpressionKind::Comparison(left_expression, operator, right_expression) => {
            let left_value = resolve_expression_value(left_expression, plan, overlay)?;
            let right_value = resolve_expression_value(right_expression, plan, overlay)?;
            match comparison_operation(
                &left_value,
                operator,
                &right_value,
                UnitResolutionContext::WithIndex(unit_index),
            ) {
                OperationResult::Value(result) => Some(result),
                OperationResult::Veto(_) => None,
            }
        }

        ExpressionKind::Arithmetic(left_expression, operator, right_expression) => {
            let left_value = resolve_expression_value(left_expression, plan, overlay)?;
            let right_value = resolve_expression_value(right_expression, plan, overlay)?;
            match arithmetic_operation(
                &left_value,
                operator,
                &right_value,
                unit_index,
                &plan.signature_index,
            ) {
                OperationResult::Value(result) => Some(result),
                OperationResult::Veto(_) => None,
            }
        }

        ExpressionKind::UnitConversion(inner_expression, target) => {
            let inner_value = resolve_expression_value(inner_expression, plan, overlay)?;
            match convert_unit(&inner_value, target) {
                OperationResult::Value(result) => Some(result),
                OperationResult::Veto(_) => None,
            }
        }

        ExpressionKind::RulePath(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Now
        | ExpressionKind::DateRelative(_, _)
        | ExpressionKind::DateCalendar(_, _, _)
        | ExpressionKind::PastFutureRange(_, _)
        | ExpressionKind::RangeLiteral(_, _)
        | ExpressionKind::RangeContainment(_, _)
        | ExpressionKind::MathematicalComputation(_, _)
        | ExpressionKind::ResultIsVeto(_)
        | ExpressionKind::LogicalAnd(_, _)
        | ExpressionKind::LogicalOr(_, _)
        | ExpressionKind::LogicalNegation(_, _)
        | ExpressionKind::Piecewise(_) => None,
    }
}

/// Three-valued truth of a unless condition using supplied overlay values only.
///
/// `Some(true)` = arm applies, `Some(false)` = arm cannot apply, `None` = still possible.
pub(crate) fn unless_condition_truth(
    expression: &Expression,
    plan: &ExecutionPlan,
    overlay: &DataOverlay,
) -> Option<bool> {
    match &expression.kind {
        ExpressionKind::LogicalAnd(left_expression, right_expression) => {
            let left_result = unless_condition_truth(left_expression, plan, overlay);
            let right_result = unless_condition_truth(right_expression, plan, overlay);
            match (left_result, right_result) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            }
        }

        ExpressionKind::LogicalOr(left_expression, right_expression) => {
            let left_result = unless_condition_truth(left_expression, plan, overlay);
            let right_result = unless_condition_truth(right_expression, plan, overlay);
            match (left_result, right_result) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            }
        }

        ExpressionKind::LogicalNegation(inner_expression, _negation_type) => {
            unless_condition_truth(inner_expression, plan, overlay).map(|boolean| !boolean)
        }

        _ => {
            let value = resolve_expression_value(expression, plan, overlay)?;
            match value.value {
                ValueKind::Boolean(boolean) => Some(boolean),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::DateTimeValue;
    use crate::planning::data_input::DataValueInput;
    use crate::Engine;
    use crate::SourceType;

    fn unless_condition_from_plan(
        plan: &ExecutionPlan,
        rule_name: &str,
        unless_index: usize,
    ) -> Expression {
        let rule = plan.get_rule(rule_name).expect("rule must exist in plan");
        let branch_index = unless_index + 1;
        rule.branches[branch_index]
            .condition
            .clone()
            .expect("unless branch must have condition")
    }

    fn load_spec(source: &str) -> (Engine, ExecutionPlan) {
        let mut engine = Engine::new();
        engine
            .load(source, SourceType::Volatile)
            .expect("spec must load");
        let now = DateTimeValue::now();
        let plan = engine
            .get_plan(None, "t", Some(&now))
            .expect("plan must build")
            .clone();
        (engine, plan)
    }

    #[test]
    fn unless_truth_none_without_supplied_value() {
        let source = r#"
spec t
data is_member: boolean
rule discount: 0%
  unless is_member then 20%
"#;
        let (_engine, plan) = load_spec(source);
        let condition = unless_condition_from_plan(&plan, "discount", 0);
        let overlay = DataOverlay::default();
        assert_eq!(
            unless_condition_truth(&condition, &plan, &overlay),
            None,
            "spec prefilled value must not make unless is_member definitely false"
        );
    }

    #[test]
    fn unless_truth_false_when_conjunct_constant_false() {
        let source = r#"
spec t
data flag: boolean
rule discount: 0%
  unless flag and (1 > 2) then 20%
"#;
        let (_engine, plan) = load_spec(source);
        let condition = unless_condition_from_plan(&plan, "discount", 0);
        let overlay = DataOverlay::default();
        assert_eq!(
            unless_condition_truth(&condition, &plan, &overlay),
            Some(false),
            "unless arm cannot apply when (1 > 2) is false regardless of flag"
        );
    }

    #[test]
    fn unless_truth_true_when_supplied_text_matches() {
        let source = r#"
spec t
data mode: text -> options "simple" "complex"
rule result: 0
  unless mode is "simple" then 1
"#;
        let (engine, plan) = load_spec(source);
        let condition = unless_condition_from_plan(&plan, "result", 0);
        let overlay = DataOverlay::resolve(
            &plan,
            [(
                "mode".to_string(),
                DataValueInput::convenience("simple".to_string()),
            )]
            .into(),
            engine.limits(),
        )
        .expect("overlay must resolve");
        assert_eq!(
            unless_condition_truth(&condition, &plan, &overlay),
            Some(true),
            "unless condition truth must follow caller-supplied mode"
        );
    }

    #[test]
    fn unless_truth_true_when_supplied_overrides_prefilled_false() {
        let source = r#"
spec t
data is_member: false
rule discount: 0%
  unless is_member then 20%
"#;
        let (engine, plan) = load_spec(source);
        let condition = unless_condition_from_plan(&plan, "discount", 0);
        let overlay = DataOverlay::resolve(
            &plan,
            [("is_member".to_string(), DataValueInput::Boolean(true))].into(),
            engine.limits(),
        )
        .expect("overlay must resolve");
        assert_eq!(
            unless_condition_truth(&condition, &plan, &overlay),
            Some(true),
            "caller-supplied true must decide unless arm despite spec prefilled false"
        );
    }

    #[test]
    fn unless_truth_false_when_supplied_matches_prefilled_false() {
        let source = r#"
spec t
data is_member: false
rule discount: 0%
  unless is_member then 20%
"#;
        let (engine, plan) = load_spec(source);
        let condition = unless_condition_from_plan(&plan, "discount", 0);
        let overlay = DataOverlay::resolve(
            &plan,
            [("is_member".to_string(), DataValueInput::Boolean(false))].into(),
            engine.limits(),
        )
        .expect("overlay must resolve");
        assert_eq!(
            unless_condition_truth(&condition, &plan, &overlay),
            Some(false),
            "caller-supplied false must prune unless arm when overlay commits the value"
        );
    }
}
