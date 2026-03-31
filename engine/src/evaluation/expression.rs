//! Iterative expression evaluation
//!
//! Evaluates expressions without recursion using a stack-based approach.
//! All runtime errors (division by zero, etc.) result in Veto instead of errors.

use super::explanation::{ExplanationNode, ValueSource};
use super::operations::{ComputationKind, OperationKind, OperationResult};
use crate::computation::{arithmetic_operation, comparison_operation};
use crate::planning::semantics::{
    negated_comparison, Expression, ExpressionKind, LiteralValue, MathematicalComputation,
    ValueKind,
};
use crate::planning::ExecutableRule;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Get an explanation node for an expression that was already evaluated.
/// Panics if the explanation node is missing — this indicates a bug in the evaluator,
/// since we always set an explanation node immediately after evaluating an expression.
fn get_explanation_node_required(
    context: &crate::evaluation::EvaluationContext,
    expr: &Expression,
    operand_name: &str,
) -> ExplanationNode {
    let loc = expr
        .source_location
        .as_ref()
        .expect("BUG: expression missing source in evaluation");
    context
        .get_explanation_node(expr)
        .cloned()
        .unwrap_or_else(|| {
            unreachable!(
                "BUG: {} was evaluated but has no explanation node ({}:{}:{})",
                operand_name, loc.attribute, loc.span.line, loc.span.col
            )
        })
}

fn expr_ptr(expr: &Expression) -> usize {
    expr as *const Expression as usize
}

/// Get the result of an operand expression that was already evaluated.
fn get_operand_result(
    results: &HashMap<usize, OperationResult>,
    expr: &Expression,
    operand_name: &str,
) -> OperationResult {
    let loc = expr
        .source_location
        .as_ref()
        .expect("BUG: expression missing source in evaluation");
    results.get(&expr_ptr(expr)).cloned().unwrap_or_else(|| {
        unreachable!(
            "BUG: {} operand was marked ready but result is missing ({}:{}:{})",
            operand_name, loc.attribute, loc.span.line, loc.span.col
        )
    })
}

/// Extract the value from an OperationResult that is known to not be Veto.
/// Called only after an explicit Veto check has already returned early.
/// Panics if the result has no value — this is unreachable after the Veto guard.
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
            operand_name, loc.attribute, loc.span.line, loc.span.col
        )
    })
}

/// Propagate veto explanation from operand to current expression
fn propagate_veto_explanation(
    context: &mut crate::evaluation::EvaluationContext,
    current: &Expression,
    vetoed_operand: &Expression,
    veto_result: OperationResult,
    operand_name: &str,
) -> OperationResult {
    let node = get_explanation_node_required(context, vetoed_operand, operand_name);
    context.set_explanation_node(current, node);
    veto_result
}

/// Evaluate a rule to produce its final result and explanation.
/// After planning, evaluation is guaranteed to complete — this function never returns
/// a Error. It produces an OperationResult (Value or Veto) and an Explanation tree.
pub(crate) fn evaluate_rule(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext,
) -> (OperationResult, crate::evaluation::explanation::Explanation) {
    use crate::evaluation::explanation::{Branch, NonMatchedBranch};

    // If rule has no unless clauses, just evaluate the default expression
    if exec_rule.branches.len() == 1 {
        return evaluate_rule_without_unless(exec_rule, context);
    }

    // Rule has unless clauses - collect all branch evaluations for Branches explanation node
    let mut non_matched_branches: Vec<NonMatchedBranch> = Vec::new();

    // Evaluate branches in reverse order (last matching wins)
    for branch_index in (1..exec_rule.branches.len()).rev() {
        let branch = &exec_rule.branches[branch_index];
        if let Some(ref condition) = branch.condition {
            let condition_expr = condition.get_source_text(&context.sources);
            let result_expr = branch.result.get_source_text(&context.sources);

            let condition_result = evaluate_expression(condition, context);
            let condition_explanation =
                get_explanation_node_required(context, condition, "condition");

            let matched = match condition_result {
                OperationResult::Veto(ref msg) => {
                    // Condition vetoed - this becomes the result
                    let unless_clause_index = branch_index - 1;
                    context.push_operation(OperationKind::RuleBranchEvaluated {
                        index: Some(unless_clause_index),
                        matched: true,
                        condition_expr,
                        result_expr,
                        result_value: Some(OperationResult::Veto(msg.clone())),
                    });

                    // Build Branches node with this as the matched branch
                    let matched_branch = Branch {
                        condition: Some(Box::new(condition_explanation)),
                        result: Box::new(ExplanationNode::Veto {
                            message: msg.clone(),
                            source_location: branch.result.source_location.clone(),
                        }),
                        clause_index: Some(unless_clause_index),
                        source_location: Some(branch.source.clone()),
                    };

                    let branches_node = ExplanationNode::Branches {
                        matched: Box::new(matched_branch),
                        non_matched: non_matched_branches,
                        source_location: Some(exec_rule.source.clone()),
                    };

                    let explanation = crate::evaluation::explanation::Explanation {
                        rule_path: exec_rule.path.clone(),
                        source: Some(exec_rule.source.clone()),
                        result: OperationResult::Veto(msg.clone()),
                        tree: Arc::new(branches_node),
                    };
                    return (OperationResult::Veto(msg.clone()), explanation);
                }
                OperationResult::Value(lit) => match &lit.value {
                    ValueKind::Boolean(b) => *b,
                    _ => {
                        let veto = OperationResult::Veto(Some(
                            "Unless condition must evaluate to boolean".to_string(),
                        ));
                        let explanation = crate::evaluation::explanation::Explanation {
                            rule_path: exec_rule.path.clone(),
                            source: Some(exec_rule.source.clone()),
                            result: veto.clone(),
                            tree: Arc::new(ExplanationNode::Veto {
                                message: Some(
                                    "Unless condition must evaluate to boolean".to_string(),
                                ),
                                source_location: Some(exec_rule.source.clone()),
                            }),
                        };
                        return (veto, explanation);
                    }
                },
            };

            let unless_clause_index = branch_index - 1;

            if matched {
                // This unless clause matched - evaluate its result
                let result = evaluate_expression(&branch.result, context);

                context.push_operation(OperationKind::RuleBranchEvaluated {
                    index: Some(unless_clause_index),
                    matched: true,
                    condition_expr,
                    result_expr,
                    result_value: Some(result.clone()),
                });

                let result_explanation =
                    get_explanation_node_required(context, &branch.result, "result");

                // Build Branches node with this as the matched branch
                let matched_branch = Branch {
                    condition: Some(Box::new(condition_explanation)),
                    result: Box::new(result_explanation),
                    clause_index: Some(unless_clause_index),
                    source_location: Some(branch.source.clone()),
                };

                let branches_node = ExplanationNode::Branches {
                    matched: Box::new(matched_branch),
                    non_matched: non_matched_branches,
                    source_location: Some(exec_rule.source.clone()),
                };

                let explanation = crate::evaluation::explanation::Explanation {
                    rule_path: exec_rule.path.clone(),
                    source: Some(exec_rule.source.clone()),
                    result: result.clone(),
                    tree: Arc::new(branches_node),
                };
                return (result, explanation);
            }
            // Branch didn't match - record it as non-matched.
            context.push_operation(OperationKind::RuleBranchEvaluated {
                index: Some(unless_clause_index),
                matched: false,
                condition_expr,
                result_expr,
                result_value: None,
            });

            non_matched_branches.push(NonMatchedBranch {
                condition: Box::new(condition_explanation),
                result: None,
                clause_index: Some(unless_clause_index),
                source_location: Some(branch.source.clone()),
            });
        }
    }

    // No unless clause matched - evaluate default expression (first branch)
    let default_branch = &exec_rule.branches[0];
    let default_expr = default_branch.result.get_source_text(&context.sources);
    let default_result = evaluate_expression(&default_branch.result, context);

    context.push_operation(OperationKind::RuleBranchEvaluated {
        index: None,
        matched: true,
        condition_expr: None,
        result_expr: default_expr,
        result_value: Some(default_result.clone()),
    });

    let default_result_explanation =
        get_explanation_node_required(context, &default_branch.result, "default result");

    // Default branch has no condition
    let matched_branch = Branch {
        condition: None,
        result: Box::new(default_result_explanation),
        clause_index: None,
        source_location: Some(default_branch.source.clone()),
    };

    let branches_node = ExplanationNode::Branches {
        matched: Box::new(matched_branch),
        non_matched: non_matched_branches,
        source_location: Some(exec_rule.source.clone()),
    };

    let explanation = crate::evaluation::explanation::Explanation {
        rule_path: exec_rule.path.clone(),
        source: Some(exec_rule.source.clone()),
        result: default_result.clone(),
        tree: Arc::new(branches_node),
    };

    (default_result, explanation)
}

/// Evaluate a rule that has no unless clauses (simple case)
fn evaluate_rule_without_unless(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext,
) -> (OperationResult, crate::evaluation::explanation::Explanation) {
    let default_branch = &exec_rule.branches[0];
    let default_expr = default_branch.result.get_source_text(&context.sources);
    let default_result = evaluate_expression(&default_branch.result, context);

    context.push_operation(OperationKind::RuleBranchEvaluated {
        index: None,
        matched: true,
        condition_expr: None,
        result_expr: default_expr,
        result_value: Some(default_result.clone()),
    });

    let root_explanation_node =
        get_explanation_node_required(context, &default_branch.result, "default result");

    let explanation = crate::evaluation::explanation::Explanation {
        rule_path: exec_rule.path.clone(),
        source: Some(exec_rule.source.clone()),
        result: default_result.clone(),
        tree: Arc::new(root_explanation_node),
    };

    (default_result, explanation)
}

/// Evaluate an expression iteratively without recursion.
/// Uses a work list approach: collect all expressions first, then evaluate in dependency order.
/// After planning, expression evaluation is guaranteed to complete — this function never
/// returns a Error. It produces an OperationResult (Value or Veto).
/// Iterative post-order traversal: collects expression nodes children-before-parents.
/// Uses pointer identity for dedup (no Hash/Eq on Expression needed).
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
                    | ExpressionKind::LogicalAnd(left, right) => {
                        stack.push(Visit::Enter(right));
                        stack.push(Visit::Enter(left));
                    }
                    ExpressionKind::LogicalNegation(operand, _)
                    | ExpressionKind::UnitConversion(operand, _)
                    | ExpressionKind::MathematicalComputation(_, operand)
                    | ExpressionKind::DateCalendar(_, _, operand) => {
                        stack.push(Visit::Enter(operand));
                    }
                    ExpressionKind::DateRelative(_, date_expr, tolerance_expr) => {
                        if let Some(tol) = tolerance_expr {
                            stack.push(Visit::Enter(tol));
                        }
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

fn evaluate_expression(
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext,
) -> OperationResult {
    let nodes = collect_postorder(expr);
    let mut results: HashMap<usize, OperationResult> = HashMap::with_capacity(nodes.len());

    for node in &nodes {
        let result = evaluate_single_expression(node, &results, context);
        results.insert(expr_ptr(node), result);
    }

    results.remove(&expr_ptr(expr)).unwrap_or_else(|| {
        let loc = expr
            .source_location
            .as_ref()
            .expect("BUG: expression missing source in evaluation");
        unreachable!(
            "BUG: expression was processed but has no result ({}:{}:{})",
            loc.attribute, loc.span.start, loc.span.end
        )
    })
}

/// Evaluate a single expression given its dependencies are already evaluated.
/// After planning, this function is guaranteed to complete — it produces an OperationResult
/// (Value or Veto) without ever returning a Error.
fn evaluate_single_expression(
    current: &Expression,
    results: &HashMap<usize, OperationResult>,
    context: &mut crate::evaluation::EvaluationContext,
) -> OperationResult {
    match &current.kind {
        ExpressionKind::Literal(lit) => {
            let value = lit.as_ref().clone();
            let explanation_node = ExplanationNode::Value {
                value: value.clone(),
                source: ValueSource::Literal,
                source_location: current.source_location.clone(),
            };
            context.set_explanation_node(current, explanation_node);
            OperationResult::Value(Box::new(value))
        }

        ExpressionKind::FactPath(fact_path) => {
            // Fact lookup: a fact can legitimately be missing (TypeDeclaration without a
            // provided value at runtime). Returning None → Veto("Missing fact: ...") is
            // correct domain behavior.
            let fact_path_clone = fact_path.clone();
            let value = context.get_fact(fact_path).cloned();
            match value {
                Some(v) => {
                    context.push_operation(OperationKind::FactUsed {
                        fact_ref: fact_path_clone.clone(),
                        value: v.clone(),
                    });
                    let explanation_node = ExplanationNode::Value {
                        value: v.clone(),
                        source: ValueSource::Fact {
                            fact_ref: fact_path_clone,
                        },
                        source_location: current.source_location.clone(),
                    };
                    context.set_explanation_node(current, explanation_node);
                    OperationResult::Value(Box::new(v))
                }
                None => {
                    let explanation_node = ExplanationNode::Veto {
                        message: Some(format!("Missing fact: {}", fact_path)),
                        source_location: current.source_location.clone(),
                    };
                    context.set_explanation_node(current, explanation_node);
                    OperationResult::Veto(Some(format!("Missing fact: {}", fact_path)))
                }
            }
        }

        ExpressionKind::RulePath(rule_path) => {
            // Rule lookup: rules are evaluated in topological order. If a referenced rule's
            // result is not in the map, planning guaranteed no cycles and topological sort
            // ensured the dependency was evaluated first. Missing result is a bug.
            let rule_path_clone = rule_path.clone();
            let loc = current
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in evaluation");
            let r = context.rule_results.get(rule_path).cloned().unwrap_or_else(|| {
                unreachable!(
                    "BUG: Rule '{}' not found in results during topological-order evaluation ({}:{}:{})",
                    rule_path.rule, loc.attribute, loc.span.line, loc.span.col
                )
            });

            context.push_operation(OperationKind::RuleUsed {
                rule_path: rule_path_clone.clone(),
                result: r.clone(),
            });

            // Share expansion via Arc instead of cloning (avoids O(n²) for deep chains)
            let expansion = match context.get_rule_explanation(rule_path) {
                Some(existing_explanation) => Arc::clone(&existing_explanation.tree),
                None => Arc::new(ExplanationNode::Value {
                    value: match &r {
                        OperationResult::Value(v) => v.as_ref().clone(),
                        OperationResult::Veto(_) => LiteralValue::from_bool(false),
                    },
                    source: ValueSource::Computed,
                    source_location: current.source_location.clone(),
                }),
            };

            let explanation_node = ExplanationNode::RuleReference {
                rule_path: rule_path_clone,
                result: r.clone(),
                source_location: current.source_location.clone(),
                expansion,
            };
            context.set_explanation_node(current, explanation_node);
            r
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_result = get_operand_result(results, left, "left");
            let right_result = get_operand_result(results, right, "right");

            if let OperationResult::Veto(_) = left_result {
                return propagate_veto_explanation(
                    context,
                    current,
                    left,
                    left_result,
                    "left operand",
                );
            }
            if let OperationResult::Veto(_) = right_result {
                return propagate_veto_explanation(
                    context,
                    current,
                    right,
                    right_result,
                    "right operand",
                );
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

            let result = arithmetic_operation(left_val, op, right_val);

            let left_explanation = get_explanation_node_required(context, left, "left operand");
            let right_explanation = get_explanation_node_required(context, right, "right operand");

            if let OperationResult::Value(ref val) = result {
                let expr_text = current.get_source_text(&context.sources);
                // Use source text if available, otherwise construct from values for explanation display
                let original_expr = expr_text
                    .clone()
                    .unwrap_or_else(|| format!("{} {} {}", left_val, op, right_val));
                let substituted_expr = format!("{} {} {}", left_val, op, right_val);
                context.push_operation(OperationKind::Computation {
                    kind: ComputationKind::Arithmetic(op.clone()),
                    inputs: vec![left_val.clone(), right_val.clone()],
                    result: val.as_ref().clone(),
                    expr: expr_text,
                });
                let explanation_node = ExplanationNode::Computation {
                    kind: ComputationKind::Arithmetic(op.clone()),
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: val.as_ref().clone(),
                    source_location: current.source_location.clone(),
                    operands: vec![left_explanation, right_explanation],
                };
                context.set_explanation_node(current, explanation_node);
            } else if let OperationResult::Veto(_) = result {
                context.set_explanation_node(current, left_explanation);
            }
            result
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_result = get_operand_result(results, left, "left");
            let right_result = get_operand_result(results, right, "right");

            if let OperationResult::Veto(_) = left_result {
                return propagate_veto_explanation(
                    context,
                    current,
                    left,
                    left_result,
                    "left operand",
                );
            }
            if let OperationResult::Veto(_) = right_result {
                return propagate_veto_explanation(
                    context,
                    current,
                    right,
                    right_result,
                    "right operand",
                );
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

            let result = comparison_operation(left_val, op, right_val);

            let left_explanation = get_explanation_node_required(context, left, "left operand");
            let right_explanation = get_explanation_node_required(context, right, "right operand");

            if let OperationResult::Value(ref val) = result {
                let is_false = matches!(val.as_ref().value, ValueKind::Boolean(false));
                let (display_op, original_expr, substituted_expr, display_result) = if is_false {
                    let negated_op = negated_comparison(op.clone());
                    let orig = match (
                        left.get_source_text(&context.sources),
                        right.get_source_text(&context.sources),
                    ) {
                        (Some(l), Some(r)) => {
                            format!("{} {} {}", l, negated_op, r)
                        }
                        _ => format!("{} {} {}", left_val, negated_op, right_val),
                    };
                    let sub = format!("{} {} {}", left_val, negated_op, right_val);
                    (negated_op, orig, sub, LiteralValue::from_bool(true))
                } else {
                    let expr_text = current.get_source_text(&context.sources);
                    let original_expr = expr_text
                        .clone()
                        .unwrap_or_else(|| format!("{} {} {}", left_val, op, right_val));
                    let substituted_expr = format!("{} {} {}", left_val, op, right_val);
                    (
                        op.clone(),
                        original_expr,
                        substituted_expr,
                        val.as_ref().clone(),
                    )
                };
                let expr_text = current.get_source_text(&context.sources);
                context.push_operation(OperationKind::Computation {
                    kind: ComputationKind::Comparison(op.clone()),
                    inputs: vec![left_val.clone(), right_val.clone()],
                    result: val.as_ref().clone(),
                    expr: expr_text,
                });
                let explanation_node = ExplanationNode::Computation {
                    kind: ComputationKind::Comparison(display_op),
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: display_result,
                    source_location: current.source_location.clone(),
                    operands: vec![left_explanation, right_explanation],
                };
                context.set_explanation_node(current, explanation_node);
            } else if let OperationResult::Veto(_) = result {
                context.set_explanation_node(current, left_explanation);
            }
            result
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_result = get_operand_result(results, left, "left");
            if let OperationResult::Veto(_) = left_result {
                return propagate_veto_explanation(
                    context,
                    current,
                    left,
                    left_result,
                    "left operand",
                );
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

            if !*left_bool {
                let left_explanation = get_explanation_node_required(context, left, "left operand");
                context.set_explanation_node(current, left_explanation);
                OperationResult::Value(Box::new(LiteralValue::from_bool(false)))
            } else {
                let right_result = get_operand_result(results, right, "right");
                let right_explanation =
                    get_explanation_node_required(context, right, "right operand");
                context.set_explanation_node(current, right_explanation);
                right_result
            }
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let result = get_operand_result(results, operand, "operand");
            if let OperationResult::Veto(_) = result {
                return propagate_veto_explanation(context, current, operand, result, "operand");
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            let operand_explanation = get_explanation_node_required(context, operand, "operand");
            match &value.value {
                ValueKind::Boolean(b) => {
                    let result_bool = !*b;
                    context.set_explanation_node(current, operand_explanation);
                    OperationResult::Value(Box::new(LiteralValue::from_bool(result_bool)))
                }
                _ => unreachable!(
                    "BUG: logical NOT with non-boolean operand; planning should have rejected this"
                ),
            }
        }

        ExpressionKind::UnitConversion(value_expr, target) => {
            let result = get_operand_result(results, value_expr, "operand");
            if let OperationResult::Veto(_) = result {
                return propagate_veto_explanation(context, current, value_expr, result, "operand");
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            let operand_explanation = get_explanation_node_required(context, value_expr, "operand");

            let conversion_result = crate::computation::convert_unit(value, target);

            context.set_explanation_node(current, operand_explanation);
            conversion_result
        }

        ExpressionKind::MathematicalComputation(op, operand) => {
            let result = get_operand_result(results, operand, "operand");
            if let OperationResult::Veto(_) = result {
                return propagate_veto_explanation(context, current, operand, result, "operand");
            }

            let value = unwrap_value_after_veto_check(&result, "operand", &current.source_location);
            let operand_explanation = get_explanation_node_required(context, operand, "operand");
            let math_result = evaluate_mathematical_operator(op, value, context);
            context.set_explanation_node(current, operand_explanation);
            math_result
        }

        ExpressionKind::Veto(veto_expr) => {
            let explanation_node = ExplanationNode::Veto {
                message: veto_expr.message.clone(),
                source_location: current.source_location.clone(),
            };
            context.set_explanation_node(current, explanation_node);
            OperationResult::Veto(veto_expr.message.clone())
        }

        ExpressionKind::Now => {
            let value = context.now().clone();
            let explanation_node = ExplanationNode::Value {
                value: value.clone(),
                source: ValueSource::Computed,
                source_location: current.source_location.clone(),
            };
            context.set_explanation_node(current, explanation_node);
            OperationResult::Value(Box::new(value))
        }

        ExpressionKind::DateRelative(kind, date_expr, tolerance_expr) => {
            let date_result = get_operand_result(results, date_expr, "date operand");
            if let OperationResult::Veto(_) = date_result {
                return propagate_veto_explanation(
                    context,
                    current,
                    date_expr,
                    date_result,
                    "date operand",
                );
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

            let tolerance = match tolerance_expr {
                Some(tol_expr) => {
                    let tol_result = get_operand_result(results, tol_expr, "tolerance operand");
                    if let OperationResult::Veto(_) = tol_result {
                        return propagate_veto_explanation(
                            context,
                            current,
                            tol_expr,
                            tol_result,
                            "tolerance operand",
                        );
                    }
                    let tol_val = unwrap_value_after_veto_check(
                        &tol_result,
                        "tolerance operand",
                        &current.source_location,
                    );
                    match &tol_val.value {
                        ValueKind::Duration(amount, unit) => Some((*amount, unit.clone())),
                        _ => unreachable!(
                            "BUG: date sugar tolerance with non-duration; planning should have rejected this"
                        ),
                    }
                }
                None => None,
            };

            let result = crate::computation::datetime::compute_date_relative(
                kind,
                date_semantic,
                tolerance.as_ref().map(|(a, u)| (a, u)),
                now_semantic,
            );

            let date_explanation =
                get_explanation_node_required(context, date_expr, "date operand");
            context.set_explanation_node(current, date_explanation);
            result
        }

        ExpressionKind::DateCalendar(kind, unit, date_expr) => {
            let date_result = get_operand_result(results, date_expr, "date operand");
            if let OperationResult::Veto(_) = date_result {
                return propagate_veto_explanation(
                    context,
                    current,
                    date_expr,
                    date_result,
                    "date operand",
                );
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

            let date_explanation =
                get_explanation_node_required(context, date_expr, "date operand");
            context.set_explanation_node(current, date_explanation);
            result
        }
    }
}

fn evaluate_mathematical_operator(
    op: &MathematicalComputation,
    value: &LiteralValue,
    context: &mut crate::evaluation::EvaluationContext,
) -> OperationResult {
    match &value.value {
        ValueKind::Number(n) => {
            use rust_decimal::prelude::ToPrimitive;
            let float_val = match n.to_f64() {
                Some(v) => v,
                None => {
                    return OperationResult::Veto(Some(
                        "Cannot convert to float for mathematical operation".to_string(),
                    ));
                }
            };

            let math_result = match op {
                MathematicalComputation::Sqrt => float_val.sqrt(),
                MathematicalComputation::Sin => float_val.sin(),
                MathematicalComputation::Cos => float_val.cos(),
                MathematicalComputation::Tan => float_val.tan(),
                MathematicalComputation::Asin => float_val.asin(),
                MathematicalComputation::Acos => float_val.acos(),
                MathematicalComputation::Atan => float_val.atan(),
                MathematicalComputation::Log => float_val.ln(),
                MathematicalComputation::Exp => float_val.exp(),
                MathematicalComputation::Abs => {
                    return OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        n.abs(),
                        value.lemma_type.clone(),
                    )));
                }
                MathematicalComputation::Floor => {
                    return OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        n.floor(),
                        value.lemma_type.clone(),
                    )));
                }
                MathematicalComputation::Ceil => {
                    return OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        n.ceil(),
                        value.lemma_type.clone(),
                    )));
                }
                MathematicalComputation::Round => {
                    return OperationResult::Value(Box::new(LiteralValue::number_with_type(
                        n.round(),
                        value.lemma_type.clone(),
                    )));
                }
            };

            let decimal_result = match rust_decimal::Decimal::from_f64_retain(math_result) {
                Some(d) => d,
                None => {
                    return OperationResult::Veto(Some(
                        "Mathematical operation result cannot be represented".to_string(),
                    ));
                }
            };

            let result_value =
                LiteralValue::number_with_type(decimal_result, value.lemma_type.clone());
            context.push_operation(OperationKind::Computation {
                kind: ComputationKind::Mathematical(op.clone()),
                inputs: vec![value.clone()],
                result: result_value.clone(),
                expr: None,
            });
            OperationResult::Value(Box::new(result_value))
        }
        _ => unreachable!(
            "BUG: mathematical operator with non-number operand; planning should have rejected this"
        ),
    }
}
