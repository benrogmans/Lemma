//! Iterative expression evaluation
//!
//! Evaluates expressions without recursion using a stack-based approach.
//! All runtime errors (division by zero, etc.) result in Veto instead of errors.

use super::conversion_trace::build_conversion_steps;
use super::evaluation_trace::{trace_expression, TraceNode, TraceValueSource};
use super::operations::{ComputationKind, OperationResult, VetoType};
use crate::computation::{
    arithmetic_operation, comparison_operation, convert_unit, UnitResolutionContext,
};
use crate::planning::semantics::{
    negated_comparison, DataPath, Expression, ExpressionKind, LiteralValue, LogicalComputation,
    MathematicalComputation, SemanticConversionTarget, Source, ValueKind,
};
use crate::planning::ExecutableRule;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type ExpressionEval = (OperationResult, TraceNode);

fn expr_ptr(expr: &Expression) -> usize {
    expr as *const Expression as usize
}

/// Convert a value to `target`, record operations, and build a computation explanation node.
pub(crate) fn finish_unit_conversion<'plan>(
    value: &LiteralValue,
    target: &SemanticConversionTarget,
    operand_explanation: TraceNode,
    data_ref: Option<&DataPath>,
    source_location: Option<Source>,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
) -> ExpressionEval {
    let conversion_result = convert_unit(
        value,
        target,
        UnitResolutionContext::WithIndex(context.plan.expression_unit_index()),
    );

    match &conversion_result {
        OperationResult::Value(converted) => {
            let converted = converted.as_ref();
            let kind = ComputationKind::UnitConversion {
                target: target.clone(),
            };
            let conversion_steps = build_conversion_steps(
                value,
                target,
                converted,
                data_ref,
                UnitResolutionContext::WithIndex(context.plan.expression_unit_index()),
            );
            assert!(
                !conversion_steps.is_empty(),
                "BUG: unit conversion succeeded but explanation steps are empty"
            );
            let explanation_node = TraceNode::Computation {
                kind,
                conversion_steps,
                expression: String::new(),
                result: converted.clone(),
                source_location: source_location.clone(),
                operands: vec![operand_explanation],
            };
            (conversion_result, explanation_node)
        }
        OperationResult::Veto(_) => (conversion_result, operand_explanation),
    }
}

fn data_ref_for_conversion_operand<'a>(
    value_expr: &'a Expression,
    operand_explanation: &'a TraceNode,
) -> Option<&'a DataPath> {
    if let ExpressionKind::DataPath(data_path) = &value_expr.kind {
        return Some(data_path);
    }
    match operand_explanation {
        TraceNode::Value {
            source: TraceValueSource::Data { data_ref },
            ..
        } => Some(data_ref),
        _ => None,
    }
}

fn get_operand(
    results: &HashMap<usize, ExpressionEval>,
    expr: &Expression,
    operand_name: &str,
) -> ExpressionEval {
    let loc = expr
        .source_location
        .as_ref()
        .expect("BUG: expression missing source in evaluation");
    results.get(&expr_ptr(expr)).cloned().unwrap_or_else(|| {
        unreachable!(
            "BUG: {} operand was marked ready but result is missing ({}:{}:{})",
            operand_name, loc.source_type, loc.span.line, loc.span.col
        )
    })
}

fn unwrap_value_after_veto_check<'a>(
    result: &'a OperationResult,
    operand_name: &str,
    source_location: &Option<crate::planning::semantics::Source>,
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

fn operation_results_agree(left: &OperationResult, right: &OperationResult) -> bool {
    match (left, right) {
        (OperationResult::Value(left_value), OperationResult::Value(right_value)) => {
            left_value.value == right_value.value
        }
        (OperationResult::Veto(left_veto), OperationResult::Veto(right_veto)) => {
            left_veto == right_veto
        }
        _ => false,
    }
}

fn evaluate_normalized_branches<'plan>(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
) -> (OperationResult, TraceNode) {
    for branch in &exec_rule.normalized_branches {
        let (condition_result, condition_trace) = evaluate_expression(&branch.condition, context);

        match condition_result {
            OperationResult::Veto(reason) => {
                return (OperationResult::Veto(reason), condition_trace);
            }
            OperationResult::Value(literal) => match &literal.value {
                ValueKind::Boolean(true) => {
                    return evaluate_expression(&branch.result, context);
                }
                ValueKind::Boolean(false) => {}
                _ => {
                    unreachable!("BUG: decision table condition non-boolean after type validation")
                }
            },
        }
    }
    unreachable!("BUG: decision table conditions must be exhaustive");
}

fn evaluate_original_branch_trace<'plan>(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
) -> (OperationResult, TraceNode) {
    use crate::evaluation::evaluation_trace::{TraceBranch, TraceNonMatchedBranch};

    if exec_rule.branches.len() == 1 {
        let branch = &exec_rule.branches[0];
        return evaluate_expression(&branch.result, context);
    }

    let mut non_matched_branches: Vec<TraceNonMatchedBranch> = Vec::new();

    for branch_index in (1..exec_rule.branches.len()).rev() {
        let branch = &exec_rule.branches[branch_index];
        let condition = branch
            .condition
            .as_ref()
            .expect("BUG: non-default branch missing condition");
        let (condition_result, condition_explanation) = evaluate_expression(condition, context);

        let matched = match condition_result {
            OperationResult::Veto(ref reason) => {
                let unless_clause_index = branch_index - 1;

                let matched_branch = TraceBranch {
                    condition: Some(Box::new(condition_explanation)),
                    result: Box::new(TraceNode::Veto {
                        message: Some(reason.to_string()),
                        source_location: branch.result.source_location.clone(),
                    }),
                    clause_index: Some(unless_clause_index),
                    source_location: Some(branch.source.clone()),
                };

                return (
                    OperationResult::Veto(reason.clone()),
                    TraceNode::Branches {
                        matched: Box::new(matched_branch),
                        non_matched: non_matched_branches,
                        source_location: Some(exec_rule.source.clone()),
                    },
                );
            }
            OperationResult::Value(lit) => match &lit.value {
                ValueKind::Boolean(b) => *b,
                _ => unreachable!("BUG: unless condition non-boolean after type validation"),
            },
        };

        let unless_clause_index = branch_index - 1;

        if matched {
            let (result, result_explanation) = evaluate_expression(&branch.result, context);

            let matched_branch = TraceBranch {
                condition: Some(Box::new(condition_explanation)),
                result: Box::new(result_explanation),
                clause_index: Some(unless_clause_index),
                source_location: Some(branch.source.clone()),
            };

            return (
                result,
                TraceNode::Branches {
                    matched: Box::new(matched_branch),
                    non_matched: non_matched_branches,
                    source_location: Some(exec_rule.source.clone()),
                },
            );
        }

        non_matched_branches.push(TraceNonMatchedBranch {
            condition: Box::new(condition_explanation),
            result: None,
            clause_index: Some(unless_clause_index),
            source_location: Some(branch.source.clone()),
        });
    }

    let default_branch = &exec_rule.branches[0];
    let (default_result, default_result_explanation) =
        evaluate_expression(&default_branch.result, context);

    (
        default_result,
        TraceNode::Branches {
            matched: Box::new(TraceBranch {
                condition: None,
                result: Box::new(default_result_explanation),
                clause_index: None,
                source_location: Some(default_branch.source.clone()),
            }),
            non_matched: non_matched_branches,
            source_location: Some(exec_rule.source.clone()),
        },
    )
}

fn minimal_trace_tree(result: &OperationResult, source: Option<Source>) -> TraceNode {
    match result {
        OperationResult::Value(value) => TraceNode::Value {
            value: value.as_ref().clone(),
            source: TraceValueSource::Computed,
            source_location: source,
        },
        OperationResult::Veto(veto) => TraceNode::Veto {
            message: Some(veto.to_string()),
            source_location: source,
        },
    }
}

pub(crate) fn evaluate_rule<'plan>(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
    explain: bool,
) -> (
    OperationResult,
    crate::evaluation::evaluation_trace::EvaluationTrace,
) {
    let (authoritative_result, _) = evaluate_normalized_branches(exec_rule, context);

    let trace_tree = if explain {
        let (trace_result, trace_tree) = evaluate_original_branch_trace(exec_rule, context);
        debug_assert!(
            operation_results_agree(&authoritative_result, &trace_result),
            "BUG: normalized and original expression results diverged"
        );
        trace_tree
    } else {
        minimal_trace_tree(&authoritative_result, Some(exec_rule.source.clone()))
    };

    let explanation = crate::evaluation::evaluation_trace::EvaluationTrace {
        rule_path: exec_rule.path.clone(),
        source: Some(exec_rule.source.clone()),
        result: authoritative_result.clone(),
        tree: Arc::new(trace_tree),
    };

    (authoritative_result, explanation)
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
                    _ => {}
                }
            }
            Visit::Exit(e) => {
                nodes.push(e);
            }
        }
    }

    nodes
}

fn evaluate_expression<'plan>(
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
) -> ExpressionEval {
    let nodes = collect_postorder(expr);
    let mut results: HashMap<usize, ExpressionEval> = HashMap::with_capacity(nodes.len());

    for node in &nodes {
        let eval = evaluate_single_expression(node, &results, context);
        results.insert(expr_ptr(node), eval);
    }

    results.remove(&expr_ptr(expr)).unwrap_or_else(|| {
        let loc = expr
            .source_location
            .as_ref()
            .expect("BUG: expression missing source in evaluation");
        unreachable!(
            "BUG: expression was processed but has no result ({}:{}:{})",
            loc.source_type, loc.span.start, loc.span.end
        )
    })
}

fn evaluate_single_expression<'plan>(
    current: &Expression,
    results: &HashMap<usize, ExpressionEval>,
    context: &mut crate::evaluation::EvaluationContext<'plan>,
) -> ExpressionEval {
    match &current.kind {
        ExpressionKind::Literal(lit) => {
            let value = lit.as_ref().clone();
            let trace = TraceNode::Value {
                value: value.clone(),
                source: TraceValueSource::Literal,
                source_location: current.source_location.clone(),
            };
            (OperationResult::Value(Box::new(value)), trace)
        }

        ExpressionKind::DataPath(data_path) => {
            let data_path_clone = data_path.clone();
            let mut value = context.get_data(data_path).cloned();
            if value.is_none() {
                if let Some(resolved) = context.lazy_rule_reference_resolve(data_path) {
                    match resolved {
                        Ok(v) => value = Some(v),
                        Err(veto) => {
                            let trace = TraceNode::Veto {
                                message: Some(veto.to_string()),
                                source_location: current.source_location.clone(),
                            };
                            return (OperationResult::Veto(veto), trace);
                        }
                    }
                } else if let Some(veto) = context.get_reference_veto(data_path) {
                    let veto = veto.clone();
                    let trace = TraceNode::Veto {
                        message: Some(veto.to_string()),
                        source_location: current.source_location.clone(),
                    };
                    return (OperationResult::Veto(veto), trace);
                }
            }
            match value {
                Some(v) => {
                    context.record_data_use(data_path);
                    let trace = TraceNode::Value {
                        value: v.clone(),
                        source: TraceValueSource::Data {
                            data_ref: data_path_clone,
                        },
                        source_location: current.source_location.clone(),
                    };
                    (OperationResult::Value(Box::new(v)), trace)
                }
                None => {
                    let reason = VetoType::MissingData {
                        data: data_path_clone.clone(),
                    };
                    let trace = TraceNode::Veto {
                        message: Some(reason.to_string()),
                        source_location: current.source_location.clone(),
                    };
                    (OperationResult::Veto(reason), trace)
                }
            }
        }

        ExpressionKind::RulePath(rule_path) => {
            let rule_path_clone = rule_path.clone();
            let loc = current
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in evaluation");
            let r = context.rule_results.get(rule_path).cloned().unwrap_or_else(|| {
                unreachable!(
                    "BUG: Rule '{}' not found in results during topological-order evaluation ({}:{}:{})",
                    rule_path.rule, loc.source_type, loc.span.line, loc.span.col
                )
            });

            let expansion = match context.get_rule_explanation(rule_path) {
                Some(existing_explanation) => Arc::clone(&existing_explanation.tree),
                None => Arc::new(TraceNode::Value {
                    value: match &r {
                        OperationResult::Value(v) => v.as_ref().clone(),
                        OperationResult::Veto(_) => LiteralValue::from_bool(false),
                    },
                    source: TraceValueSource::Computed,
                    source_location: current.source_location.clone(),
                }),
            };

            let trace = TraceNode::RuleReference {
                rule_path: rule_path_clone,
                result: r.clone(),
                source_location: current.source_location.clone(),
                expansion,
            };
            (r, trace)
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let (left_result, left_trace) = get_operand(results, left, "left operand");
            let (right_result, right_trace) = get_operand(results, right, "right operand");

            if let OperationResult::Veto(_) = left_result {
                return (left_result, left_trace);
            }
            if let OperationResult::Veto(_) = right_result {
                return (right_result, right_trace);
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

            let result = arithmetic_operation(
                left_val,
                op,
                right_val,
                context.plan.expression_unit_index(),
                &context.plan.signature_index,
            );

            if let OperationResult::Value(ref val) = result {
                let expression = format!("{} {} {}", left_val, op, right_val);
                let trace = TraceNode::Computation {
                    kind: ComputationKind::Arithmetic(op.clone()),
                    conversion_steps: vec![],
                    expression,
                    result: val.as_ref().clone(),
                    source_location: current.source_location.clone(),
                    operands: vec![left_trace, right_trace],
                };
                (result, trace)
            } else {
                (result, left_trace)
            }
        }

        ExpressionKind::Comparison(left, op, right) => {
            let (left_result, left_trace) = get_operand(results, left, "left operand");
            let (right_result, right_trace) = get_operand(results, right, "right operand");

            if let OperationResult::Veto(_) = left_result {
                return (left_result, left_trace);
            }
            if let OperationResult::Veto(_) = right_result {
                return (right_result, right_trace);
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

            let result = comparison_operation(
                left_val,
                op,
                right_val,
                UnitResolutionContext::WithIndex(context.plan.expression_unit_index()),
            );

            if let OperationResult::Value(ref val) = result {
                let is_false = matches!(val.as_ref().value, ValueKind::Boolean(false));
                let (display_op, expression, display_result) = if is_false {
                    let negated_op = negated_comparison(op.clone());
                    let expression = format!("{} {} {}", left_val, negated_op, right_val);
                    (negated_op, expression, LiteralValue::from_bool(true))
                } else {
                    let expression = format!("{} {} {}", left_val, op, right_val);
                    (op.clone(), expression, val.as_ref().clone())
                };
                let trace = TraceNode::Computation {
                    kind: ComputationKind::Comparison(display_op),
                    conversion_steps: vec![],
                    expression,
                    result: display_result,
                    source_location: current.source_location.clone(),
                    operands: vec![left_trace, right_trace],
                };
                (result, trace)
            } else {
                (result, left_trace)
            }
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let (left_result, left_trace) = get_operand(results, left, "left operand");
            if let OperationResult::Veto(_) = left_result {
                return (left_result, left_trace);
            }

            let left_val = unwrap_value_after_veto_check(
                &left_result,
                "left operand",
                &current.source_location,
            );
            let left_bool = match &left_val.value {
                ValueKind::Boolean(b) => b,
                _ => unreachable!(
                    "BUG: logical AND with non-boolean operand; planning should have rejected this"
                ),
            };

            let (operation_result, operands) = if !*left_bool {
                (
                    OperationResult::Value(Box::new(LiteralValue::from_bool(false))),
                    vec![left_trace],
                )
            } else {
                let (right_result, right_trace) = get_operand(results, right, "right operand");
                if let OperationResult::Veto(_) = right_result {
                    return (right_result, right_trace);
                }
                (right_result, vec![left_trace, right_trace])
            };

            let expression = match operands.len() {
                1 => trace_expression(&operands[0]),
                2 => format!(
                    "{} and {}",
                    trace_expression(&operands[0]),
                    trace_expression(&operands[1])
                ),
                _ => unreachable!("BUG: logical AND trace must have one or two operands"),
            };
            let result_value = match &operation_result {
                OperationResult::Value(value) => value.as_ref().clone(),
                OperationResult::Veto(_) => {
                    unreachable!("BUG: logical AND trace built after veto propagation")
                }
            };
            let trace = TraceNode::Computation {
                kind: ComputationKind::Logical(LogicalComputation::And),
                conversion_steps: vec![],
                expression,
                result: result_value,
                source_location: current.source_location.clone(),
                operands,
            };
            (operation_result, trace)
        }

        ExpressionKind::LogicalOr(left, right) => {
            let (left_result, left_trace) = get_operand(results, left, "left operand");
            if matches!(&left_result, OperationResult::Veto(_)) {
                let (right_result, right_trace) = get_operand(results, right, "right operand");
                return (right_result, right_trace);
            }

            let left_val = unwrap_value_after_veto_check(
                &left_result,
                "left operand",
                &current.source_location,
            );
            let (operation_result, operands) =
                if matches!(&left_val.value, ValueKind::Boolean(false)) {
                    let (right_result, right_trace) = get_operand(results, right, "right operand");
                    (right_result, vec![left_trace, right_trace])
                } else {
                    (left_result, vec![left_trace])
                };

            let expression = match operands.len() {
                1 => trace_expression(&operands[0]),
                2 => format!(
                    "{} or {}",
                    trace_expression(&operands[0]),
                    trace_expression(&operands[1])
                ),
                _ => unreachable!("BUG: logical OR trace must have one or two operands"),
            };
            let result_value = match &operation_result {
                OperationResult::Value(value) => value.as_ref().clone(),
                OperationResult::Veto(_) => {
                    unreachable!("BUG: logical OR trace built after veto propagation")
                }
            };
            let trace = TraceNode::Computation {
                kind: ComputationKind::Logical(LogicalComputation::Or),
                conversion_steps: vec![],
                expression,
                result: result_value,
                source_location: current.source_location.clone(),
                operands,
            };
            (operation_result, trace)
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let (result, operand_trace) = get_operand(results, operand, "operand");
            if let OperationResult::Veto(_) = result {
                return (result, operand_trace);
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            let operation_result = match &value.value {
                ValueKind::Boolean(b) => {
                    OperationResult::Value(Box::new(LiteralValue::from_bool(!*b)))
                }
                _ => unreachable!(
                    "BUG: logical NOT with non-boolean operand; planning should have rejected this"
                ),
            };
            let result_value = match &operation_result {
                OperationResult::Value(value) => value.as_ref().clone(),
                OperationResult::Veto(_) => unreachable!("BUG: logical NOT result must be boolean"),
            };
            let expression = format!("not {}", trace_expression(&operand_trace));
            let trace = TraceNode::Computation {
                kind: ComputationKind::Logical(LogicalComputation::Not),
                conversion_steps: vec![],
                expression,
                result: result_value,
                source_location: current.source_location.clone(),
                operands: vec![operand_trace],
            };
            (operation_result, trace)
        }

        ExpressionKind::UnitConversion(value_expr, target) => {
            let (result, operand_trace) = get_operand(results, value_expr, "operand");
            if let OperationResult::Veto(_) = result {
                return (result, operand_trace);
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            let data_ref = data_ref_for_conversion_operand(value_expr, &operand_trace).cloned();
            finish_unit_conversion(
                value,
                target,
                operand_trace,
                data_ref.as_ref(),
                current.source_location.clone(),
                context,
            )
        }

        ExpressionKind::MathematicalComputation(op, operand) => {
            let (result, operand_trace) = get_operand(results, operand, "operand");
            if let OperationResult::Veto(_) = result {
                return (result, operand_trace);
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            let math_result = evaluate_mathematical_operator(op, value);
            match &math_result {
                OperationResult::Value(result_value) => {
                    let expression = format!("{op} {value}");
                    let trace = TraceNode::Computation {
                        kind: ComputationKind::Mathematical(op.clone()),
                        conversion_steps: vec![],
                        expression,
                        result: result_value.as_ref().clone(),
                        source_location: current.source_location.clone(),
                        operands: vec![operand_trace],
                    };
                    (math_result, trace)
                }
                OperationResult::Veto(_) => (math_result, operand_trace),
            }
        }

        ExpressionKind::ResultIsVeto(operand) => {
            let (operand_result, operand_trace) = get_operand(results, operand, "operand");
            let is_vetoed = operand_result.vetoed();
            let result = OperationResult::Value(Box::new(LiteralValue::from_bool(is_vetoed)));

            let operand_display = match &operand_result {
                OperationResult::Value(value) => value.to_string(),
                OperationResult::Veto(veto) => veto.to_string(),
            };
            let expression = if is_vetoed {
                format!("{operand_display} is veto")
            } else {
                format!("{operand_display} is not veto")
            };
            let trace = TraceNode::Computation {
                kind: ComputationKind::ResultIsVeto,
                conversion_steps: vec![],
                expression,
                result: LiteralValue::from_bool(true),
                source_location: current.source_location.clone(),
                operands: vec![operand_trace],
            };
            (result, trace)
        }

        ExpressionKind::Veto(veto_expr) => {
            let trace = TraceNode::Veto {
                message: veto_expr.message.clone(),
                source_location: current.source_location.clone(),
            };
            (
                OperationResult::Veto(VetoType::UserDefined {
                    message: veto_expr.message.clone(),
                }),
                trace,
            )
        }

        ExpressionKind::Now => {
            let value = context.now().clone();
            let trace = TraceNode::Value {
                value: value.clone(),
                source: TraceValueSource::Computed,
                source_location: current.source_location.clone(),
            };
            (OperationResult::Value(Box::new(value)), trace)
        }

        ExpressionKind::DateRelative(kind, date_expr) => {
            let (date_result, date_trace) = get_operand(results, date_expr, "date operand");
            if let OperationResult::Veto(_) = date_result {
                return (date_result, date_trace);
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

            let result = crate::computation::datetime::compute_date_relative(
                kind,
                date_semantic,
                now_semantic,
            );
            (result, date_trace)
        }

        ExpressionKind::DateCalendar(kind, unit, date_expr) => {
            let (date_result, date_trace) = get_operand(results, date_expr, "date operand");
            if let OperationResult::Veto(_) = date_result {
                return (date_result, date_trace);
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

            let result = crate::computation::datetime::compute_date_calendar(
                kind,
                unit,
                date_semantic,
                now_semantic,
            );
            (result, date_trace)
        }

        ExpressionKind::RangeLiteral(left, right) => {
            let (left_result, left_trace) = get_operand(results, left, "left endpoint");
            let (right_result, right_trace) = get_operand(results, right, "right endpoint");

            if let OperationResult::Veto(_) = left_result {
                return (left_result, left_trace);
            }
            if let OperationResult::Veto(_) = right_result {
                return (right_result, right_trace);
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
            let trace = TraceNode::Value {
                value: range_value.clone(),
                source: TraceValueSource::Computed,
                source_location: current.source_location.clone(),
            };
            (OperationResult::Value(Box::new(range_value)), trace)
        }

        ExpressionKind::PastFutureRange(kind, offset_expr) => {
            let (offset_result, offset_trace) = get_operand(results, offset_expr, "offset operand");
            if let OperationResult::Veto(_) = offset_result {
                return (offset_result, offset_trace);
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

            let result = crate::computation::datetime::evaluate_past_future_range(
                kind,
                offset_value,
                now_semantic,
            );

            match &result {
                OperationResult::Value(range_value) => {
                    let trace = TraceNode::Value {
                        value: range_value.as_ref().clone(),
                        source: TraceValueSource::Computed,
                        source_location: current.source_location.clone(),
                    };
                    (result, trace)
                }
                OperationResult::Veto(_) => (result, offset_trace),
            }
        }

        ExpressionKind::RangeContainment(value_expr, range_expr) => {
            let (value_result, value_trace) = get_operand(results, value_expr, "value operand");
            let (range_result, range_trace) = get_operand(results, range_expr, "range operand");

            if let OperationResult::Veto(_) = value_result {
                return (value_result, value_trace);
            }
            if let OperationResult::Veto(_) = range_result {
                return (range_result, range_trace);
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

            let result_value = LiteralValue::from_bool(contained);
            let trace = TraceNode::Value {
                value: result_value.clone(),
                source: TraceValueSource::Computed,
                source_location: current.source_location.clone(),
            };
            (OperationResult::Value(Box::new(result_value)), trace)
        }
    }
}

fn evaluate_mathematical_operator(
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
            OperationResult::Value(Box::new(result_value))
        }
        _ => unreachable!(
            "BUG: mathematical operator with non-number operand; planning should have rejected this"
        ),
    }
}
