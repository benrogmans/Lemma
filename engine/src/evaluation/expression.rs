//! Iterative expression evaluation
//!
//! Evaluates normalized expressions without recursion using a stack-based approach.
//! All runtime errors (division by zero, etc.) result in Veto instead of errors.

use super::operations::{OperationResult, VetoType};
use crate::computation::{
    arithmetic_operation, comparison_operation, convert_unit, UnitResolutionContext,
};
use crate::planning::semantics::{
    Expression, ExpressionKind, LiteralValue, MathematicalComputation, Source, ValueKind,
};
use std::collections::{HashMap, HashSet};

fn evaluate_postorder_expression<'plan>(
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
) -> OperationResult {
    let nodes = collect_postorder(expr);
    let mut values: HashMap<usize, OperationResult> = HashMap::with_capacity(nodes.len());

    for node in &nodes {
        let value = evaluate_single_value(node, &values, context);
        values.insert(expr_ptr(node), value);
    }

    values.remove(&expr_ptr(expr)).unwrap_or_else(|| {
        let loc = expr
            .source_location
            .as_ref()
            .expect("BUG: expression missing source in evaluation");
        unreachable!(
            "BUG: expression was processed but has no value ({}:{}:{})",
            loc.source_type, loc.span.start, loc.span.end
        )
    })
}

fn expr_ptr(expr: &Expression) -> usize {
    expr as *const Expression as usize
}

fn get_value_operand(
    values: &HashMap<usize, OperationResult>,
    expr: &Expression,
    operand_name: &str,
) -> OperationResult {
    let loc = expr
        .source_location
        .as_ref()
        .expect("BUG: expression missing source in evaluation");
    values.get(&expr_ptr(expr)).cloned().unwrap_or_else(|| {
        unreachable!(
            "BUG: {} operand was marked ready but value is missing ({}:{}:{})",
            operand_name, loc.source_type, loc.span.line, loc.span.col
        )
    })
}

fn unwrap_value_after_veto_check<'a>(
    result: &'a OperationResult,
    operand_name: &str,
    source_location: &Option<Source>,
) -> &'a LiteralValue {
    result.value().unwrap_or_else(|| {
        let loc = source_location
            .as_ref()
            .expect("BUG: expression missing source in evaluation");
        unreachable!(
            "BUG: {} passed Veto check but has no value ({}:{}:{})",
            operand_name, loc.source_type, loc.span.line, loc.span.col
        )
    })
}

fn collect_postorder(root: &Expression) -> Vec<&Expression> {
    enum Visit<'a> {
        Enter(&'a Expression),
        Exit(&'a Expression),
    }

    let mut stack: Vec<Visit<'_>> = vec![Visit::Enter(root)];
    let mut seen: HashSet<usize> = HashSet::new();
    let mut nodes: Vec<&Expression> = Vec::new();

    while let Some(visit) = stack.pop() {
        match visit {
            Visit::Enter(e) => {
                if !seen.insert(expr_ptr(e)) {
                    continue;
                }
                stack.push(Visit::Exit(e));
                match &e.kind {
                    ExpressionKind::Arithmetic(left, _, right)
                    | ExpressionKind::Comparison(left, _, right)
                    | ExpressionKind::LogicalAnd(left, right)
                    | ExpressionKind::LogicalOr(left, right)
                    | ExpressionKind::RangeLiteral(left, right)
                    | ExpressionKind::RangeContainment(left, right) => {
                        stack.push(Visit::Enter(right));
                        stack.push(Visit::Enter(left));
                    }
                    ExpressionKind::LogicalNegation(operand, _)
                    | ExpressionKind::UnitConversion(operand, _)
                    | ExpressionKind::MathematicalComputation(_, operand)
                    | ExpressionKind::ResultIsVeto(operand)
                    | ExpressionKind::DateCalendar(_, _, operand)
                    | ExpressionKind::PastFutureRange(_, operand) => {
                        stack.push(Visit::Enter(operand));
                    }
                    ExpressionKind::DateRelative(_, date_expr) => {
                        stack.push(Visit::Enter(date_expr));
                    }
                    ExpressionKind::Literal(_)
                    | ExpressionKind::DataPath(_)
                    | ExpressionKind::RulePath(_)
                    | ExpressionKind::Veto(_)
                    | ExpressionKind::Now
                    | ExpressionKind::Piecewise(_) => {}
                }
            }
            Visit::Exit(e) => {
                nodes.push(e);
            }
        }
    }

    nodes
}

fn evaluate_single_value<'plan>(
    current: &Expression,
    values: &HashMap<usize, OperationResult>,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
) -> OperationResult {
    match &current.kind {
        ExpressionKind::Literal(lit) => OperationResult::Value(lit.as_ref().clone()),

        ExpressionKind::DataPath(data_path) => resolve_data_path_value(data_path, context),

        ExpressionKind::RulePath(rule_path) => context
            .rule_results
            .get(rule_path)
            .cloned()
            .unwrap_or_else(|| {
                unreachable!(
                    "BUG: Rule '{}' not found in results during explanation resolution",
                    rule_path.rule
                )
            }),

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_result = get_value_operand(values, left, "left operand");
            if let OperationResult::Veto(_) = left_result {
                return left_result;
            }
            let right_result = get_value_operand(values, right, "right operand");
            if let OperationResult::Veto(_) = right_result {
                return right_result;
            }

            let left_val = unwrap_value_after_veto_check(
                &left_result,
                "left operand",
                &current.source_location,
            );
            let right_val = unwrap_value_after_veto_check(
                &right_result,
                "right operand",
                &current.source_location,
            );

            arithmetic_operation(
                left_val,
                op,
                right_val,
                context.plan().expression_unit_index(),
                &context.plan().signature_index,
            )
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_result = get_value_operand(values, left, "left operand");
            if let OperationResult::Veto(_) = left_result {
                return left_result;
            }
            let right_result = get_value_operand(values, right, "right operand");
            if let OperationResult::Veto(_) = right_result {
                return right_result;
            }

            let left_val = unwrap_value_after_veto_check(
                &left_result,
                "left operand",
                &current.source_location,
            );
            let right_val = unwrap_value_after_veto_check(
                &right_result,
                "right operand",
                &current.source_location,
            );

            comparison_operation(
                left_val,
                op,
                right_val,
                UnitResolutionContext::WithIndex(context.plan().expression_unit_index()),
            )
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_result = get_value_operand(values, left, "left operand");
            match crate::evaluation::branch_semantics::and_conjunct_outcome(&left_result) {
                crate::evaluation::branch_semantics::BranchOutcome::Propagate(result) => result,
                crate::evaluation::branch_semantics::BranchOutcome::NotTaken => {
                    OperationResult::Value(LiteralValue::from_bool(false))
                }
                crate::evaluation::branch_semantics::BranchOutcome::Taken => {
                    // The last conjunct's result (value or veto) is the
                    // conjunction's result, mirroring the compiled form.
                    get_value_operand(values, right, "right operand")
                }
            }
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_result = get_value_operand(values, left, "left operand");
            // The right child of a binary disjunction is always the last
            // disjunct: the source tree is binary while the compiled form is
            // flattened n-ary, and composing with `is_last_disjunct = true`
            // for every right child reproduces the flat semantics exactly.
            match crate::evaluation::branch_semantics::or_disjunct_outcome(&left_result, false) {
                crate::evaluation::branch_semantics::BranchOutcome::Propagate(result) => result,
                crate::evaluation::branch_semantics::BranchOutcome::Taken => {
                    OperationResult::Value(LiteralValue::from_bool(true))
                }
                crate::evaluation::branch_semantics::BranchOutcome::NotTaken => {
                    let right_result = get_value_operand(values, right, "right operand");
                    match crate::evaluation::branch_semantics::or_disjunct_outcome(
                        &right_result,
                        true,
                    ) {
                        crate::evaluation::branch_semantics::BranchOutcome::Propagate(result) => {
                            result
                        }
                        crate::evaluation::branch_semantics::BranchOutcome::Taken
                        | crate::evaluation::branch_semantics::BranchOutcome::NotTaken => {
                            unreachable!(
                                "BUG: the last disjunct's result always propagates as the disjunction's value"
                            )
                        }
                    }
                }
            }
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let result = get_value_operand(values, operand, "operand");
            if let OperationResult::Veto(_) = result {
                return result;
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            match &value.value {
                ValueKind::Boolean(b) => OperationResult::Value(LiteralValue::from_bool(!*b)),
                _ => unreachable!(
                    "BUG: logical NOT with non-boolean operand; planning should have rejected this"
                ),
            }
        }

        ExpressionKind::UnitConversion(value_expr, target) => {
            let result = get_value_operand(values, value_expr, "operand");
            if let OperationResult::Veto(_) = result {
                return result;
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            convert_unit(
                value,
                target,
                UnitResolutionContext::WithIndex(context.plan().expression_unit_index()),
            )
        }

        ExpressionKind::MathematicalComputation(op, operand) => {
            let result = get_value_operand(values, operand, "operand");
            if let OperationResult::Veto(_) = result {
                return result;
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            evaluate_mathematical_operator(op, value)
        }

        ExpressionKind::ResultIsVeto(operand) => {
            let operand_result = get_value_operand(values, operand, "operand");
            OperationResult::Value(LiteralValue::from_bool(operand_result.vetoed()))
        }

        ExpressionKind::Veto(veto_expr) => OperationResult::Veto(VetoType::UserDefined {
            message: veto_expr.message.clone(),
        }),

        ExpressionKind::Now => OperationResult::Value(context.now().clone()),

        ExpressionKind::DateRelative(kind, date_expr) => {
            let date_result = get_value_operand(values, date_expr, "date operand");
            if let OperationResult::Veto(_) = date_result {
                return date_result;
            }

            let date_val = unwrap_value_after_veto_check(
                &date_result,
                "date operand",
                &current.source_location,
            );

            let date_semantic = match &date_val.value {
                ValueKind::Date(dt) => dt,
                _ => unreachable!(
                    "BUG: date sugar with non-date operand; planning should have rejected this"
                ),
            };

            let now_val = context.now();
            let now_semantic = match &now_val.value {
                ValueKind::Date(dt) => dt,
                _ => unreachable!("BUG: context.now() must be a Date value"),
            };

            crate::computation::datetime::compute_date_relative(kind, date_semantic, now_semantic)
        }

        ExpressionKind::DateCalendar(kind, unit, date_expr) => {
            let date_result = get_value_operand(values, date_expr, "date operand");
            if let OperationResult::Veto(_) = date_result {
                return date_result;
            }

            let date_val = unwrap_value_after_veto_check(
                &date_result,
                "date operand",
                &current.source_location,
            );

            let date_semantic = match &date_val.value {
                ValueKind::Date(dt) => dt,
                _ => unreachable!(
                    "BUG: calendar sugar with non-date operand; planning should have rejected this"
                ),
            };

            let now_val = context.now();
            let now_semantic = match &now_val.value {
                ValueKind::Date(dt) => dt,
                _ => unreachable!("BUG: context.now() must be a Date value"),
            };

            crate::computation::datetime::compute_date_calendar(
                kind,
                unit,
                date_semantic,
                now_semantic,
            )
        }

        ExpressionKind::RangeLiteral(left, right) => {
            let left_result = get_value_operand(values, left, "left endpoint");
            if let OperationResult::Veto(_) = left_result {
                return left_result;
            }
            let right_result = get_value_operand(values, right, "right endpoint");
            if let OperationResult::Veto(_) = right_result {
                return right_result;
            }

            let left_value = unwrap_value_after_veto_check(
                &left_result,
                "left endpoint",
                &current.source_location,
            );
            let right_value = unwrap_value_after_veto_check(
                &right_result,
                "right endpoint",
                &current.source_location,
            );

            let range_value = LiteralValue::range(left_value.clone(), right_value.clone());
            OperationResult::Value(range_value)
        }

        ExpressionKind::PastFutureRange(kind, offset_expr) => {
            let offset_result = get_value_operand(values, offset_expr, "offset operand");
            if let OperationResult::Veto(_) = offset_result {
                return offset_result;
            }

            let offset_value = unwrap_value_after_veto_check(
                &offset_result,
                "offset operand",
                &current.source_location,
            );

            let now_value = context.now();
            let now_semantic = match &now_value.value {
                ValueKind::Date(date_time) => date_time,
                _ => unreachable!("BUG: context.now() must be a Date value"),
            };

            crate::computation::datetime::evaluate_past_future_range(
                kind,
                offset_value,
                now_semantic,
            )
        }

        ExpressionKind::RangeContainment(value_expr, range_expr) => {
            let value_result = get_value_operand(values, value_expr, "value operand");
            if let OperationResult::Veto(_) = value_result {
                return value_result;
            }
            let range_result = get_value_operand(values, range_expr, "range operand");
            if let OperationResult::Veto(_) = range_result {
                return range_result;
            }

            let value_literal = unwrap_value_after_veto_check(
                &value_result,
                "value operand",
                &current.source_location,
            );
            let range_literal = unwrap_value_after_veto_check(
                &range_result,
                "range operand",
                &current.source_location,
            );

            let contained = match &range_literal.value {
                ValueKind::Range(range_left, range_right) => {
                    crate::computation::range::check_containment(
                        value_literal,
                        range_left.as_ref(),
                        range_right.as_ref(),
                    )
                }
                _ => unreachable!(
                    "BUG: range containment reached runtime with non-range operand; planning should have rejected this"
                ),
            };

            OperationResult::Value(LiteralValue::from_bool(contained))
        }

        ExpressionKind::Piecewise(_) => {
            unreachable!("BUG: Piecewise in source expression for explanation")
        }
    }
}

pub(crate) fn evaluate_mathematical_operator(
    op: &MathematicalComputation,
    value: &LiteralValue,
) -> OperationResult {
    use crate::computation::decimal_math::{decimal_acos, decimal_asin, decimal_atan};
    use rust_decimal::MathematicalOps;

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

/// Resolve a source-branch expression from finished evaluation context.
pub(crate) fn resolve_source_expression_values<'plan>(
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
) -> OperationResult {
    evaluate_postorder_expression(expr, context)
}
