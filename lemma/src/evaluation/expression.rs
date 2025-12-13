//! Iterative expression evaluation
//!
//! Evaluates expressions without recursion using a stack-based approach.
//! All runtime errors (division by zero, etc.) result in Veto instead of errors.

use super::operations::{
    arithmetic_operation, comparison_operation, ComputationKind, OperationKind, OperationResult,
};
use super::proof::{ProofNode, ValueOrigin};
use crate::planning::ExecutableRule;
use crate::{BooleanValue, Expression, ExpressionKind, LiteralValue, MathematicalComputation};
use std::collections::HashMap;

/// Get a proof node that must exist (returns LemmaError if missing - indicates engine bug)
fn get_proof_node_required(
    context: &crate::evaluation::EvaluationContext,
    expr: &Expression,
    operand_name: &str,
) -> crate::LemmaResult<ProofNode> {
    context.get_proof_node(expr).cloned().ok_or_else(|| {
        crate::LemmaError::Engine(format!(
            "bug: {} was evaluated but has no proof node",
            operand_name
        ))
    })
}

/// Get an operand result from the results map (returns LemmaError if missing - indicates engine bug)
///
/// This error occurs when the dependency resolution logic incorrectly reports that a dependency
/// is ready (results.contains_key() returned true), but the dependency's result is not actually
/// in the results map when we try to use it. This indicates a bug in the dependency resolution
/// loop in evaluate_expression() where dependencies are checked but not properly tracked.
fn get_operand_result(
    results: &HashMap<Expression, OperationResult>,
    expr: &Expression,
    operand_name: &str,
) -> crate::LemmaResult<OperationResult> {
    results
        .get(expr)
        .cloned()
        .ok_or_else(|| {
            crate::LemmaError::Engine(format!(
                "Dependency resolution failed - {} operand was marked as ready but result is missing. \
                 The dependency check reported the operand was evaluated, but it's not in the results map. \
                 This indicates a bug in evaluate_expression's dependency resolution loop. \
                 Missing operand: {:?}",
                operand_name, expr.kind
            ))
        })
}

/// Get source text for an expression
fn get_source_text(
    context: &crate::evaluation::EvaluationContext,
    expr: &Expression,
) -> crate::LemmaResult<String> {
    match &expr.source {
        Some(source) => source.get_text(&context.source_text),
        None => Ok("<no source>".to_string()),
    }
}

/// Propagate a veto result by copying the proof node from the vetoed operand
fn propagate_veto_proof(
    context: &mut crate::evaluation::EvaluationContext,
    current: &Expression,
    vetoed_operand: &Expression,
    veto_result: OperationResult,
    operand_name: &str,
) -> crate::LemmaResult<OperationResult> {
    let proof = get_proof_node_required(context, vetoed_operand, operand_name)?;
    context.set_proof_node(current, proof);
    Ok(veto_result)
}

/// Evaluate a rule to produce its final result and proof
pub fn evaluate_rule(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext,
) -> crate::LemmaResult<(OperationResult, crate::evaluation::proof::Proof)> {
    use crate::evaluation::proof::{Branch, NonMatchedBranch};

    // All branches have explicit conditions (normalized during graph building)
    // Evaluate branches in reverse order (last matching wins)
    // For single-branch rules, the condition will be true, so it will always match
    let mut non_matched_branches: Vec<NonMatchedBranch> = Vec::new();

    // Helper: calculate clause index for proof/response structure
    // Branch 0 maps to None (first branch), branches 1+ map to their unless clause index
    let clause_index = |idx: usize| -> Option<usize> {
        if idx == 0 {
            None // First branch (condition excludes all later branches)
        } else {
            Some(idx - 1) // Later branches (unless clause index)
        }
    };

    // Evaluate branches in reverse order (last matching wins)
    // All branches have explicit conditions (normalized during graph building)
    for branch_index in (0..exec_rule.branches.len()).rev() {
        let branch = &exec_rule.branches[branch_index];
        let condition_expr = get_source_text(context, &branch.condition)?;
        let result_expr = get_source_text(context, &branch.result)?;

        let condition_result = evaluate_expression(&branch.condition, context)?;

        // Ensure proof node exists for the condition (defensive check for normalized conditions)
        let condition_proof = if let Some(proof) = context.get_proof_node(&branch.condition) {
            proof.clone()
        } else {
            // Create a proof node from the result if one wasn't created during evaluation
            // This can happen for normalized conditions that are simple literals or complex expressions
            let original_expr = get_source_text(context, &branch.condition)?;
            let result_bool = match &condition_result {
                OperationResult::Value(LiteralValue::Boolean(b)) => bool::from(b.clone()),
                _ => false,
            };
            let proof_node = ProofNode::Condition {
                original_expression: original_expr,
                expression: format!("{}", result_bool),
                result: result_bool,
                source: branch.condition.source.clone(),
                operands: vec![],
            };
            context.set_proof_node(&branch.condition, proof_node.clone());
            proof_node
        };

        let matched = match condition_result {
            OperationResult::Veto(ref msg) => {
                // Condition vetoed - this becomes the result
                let idx = clause_index(branch_index);
                context.push_operation(OperationKind::RuleBranchEvaluated {
                    index: idx,
                    matched: true,
                    condition_expression: condition_expr.clone(),
                    result_expression: result_expr.clone(),
                    result_value: Some(OperationResult::Veto(msg.clone())),
                });

                // Build Branches node with this as the matched branch
                let matched_branch = Branch {
                    condition: Box::new(condition_proof),
                    result: Box::new(ProofNode::Veto {
                        message: msg.clone(),
                        source: branch.result.source.clone(),
                    }),
                    clause_index: idx,
                    source: branch.source.clone(),
                };

                let branches_node = ProofNode::Branches {
                    matched: Box::new(matched_branch),
                    non_matched: non_matched_branches,
                    source: exec_rule.source.clone(),
                };

                let proof = crate::evaluation::proof::Proof {
                    rule_path: exec_rule.path.clone(),
                    source: exec_rule.source.clone(),
                    result: OperationResult::Veto(msg.clone()),
                    tree: branches_node,
                };
                return Ok((OperationResult::Veto(msg.clone()), proof));
            }
            OperationResult::Value(LiteralValue::Boolean(b)) => b,
            _ => {
                let veto = OperationResult::Veto(Some(
                    "Branch condition must evaluate to boolean".to_string(),
                ));
                let proof = crate::evaluation::proof::Proof {
                    rule_path: exec_rule.path.clone(),
                    source: exec_rule.source.clone(),
                    result: veto.clone(),
                    tree: ProofNode::Veto {
                        message: Some("Branch condition must evaluate to boolean".to_string()),
                        source: exec_rule.source.clone(),
                    },
                };
                return Ok((veto, proof));
            }
        };

        let idx = clause_index(branch_index);

        if bool::from(matched) {
            // This branch matched - evaluate its result
            let result = evaluate_expression(&branch.result, context)?;

            context.push_operation(OperationKind::RuleBranchEvaluated {
                index: idx,
                matched: true,
                condition_expression: condition_expr.clone(),
                result_expression: result_expr.clone(),
                result_value: Some(result.clone()),
            });

            let result_proof = context
                .get_proof_node(&branch.result)
                .cloned()
                .ok_or_else(|| {
                    crate::LemmaError::Engine(format!(
                        "bug: result expression was evaluated but has no proof node - expression: {:?}",
                        branch.result
                    ))
                })?;

            // Build Branches node with this as the matched branch
            let matched_branch = Branch {
                condition: Box::new(condition_proof),
                result: Box::new(result_proof),
                clause_index: idx,
                source: branch.source.clone(),
            };

            let branches_node = ProofNode::Branches {
                matched: Box::new(matched_branch),
                non_matched: non_matched_branches,
                source: exec_rule.source.clone(),
            };

            let proof = crate::evaluation::proof::Proof {
                rule_path: exec_rule.path.clone(),
                source: exec_rule.source.clone(),
                result: result.clone(),
                tree: branches_node,
            };
            return Ok((result, proof));
        } else {
            // Branch didn't match - record it as non-matched
            // All evaluated branches that don't match are recorded, including default
            // The default branch's normalized condition (NOT cond1 AND NOT cond2 AND ...)
            // is insightful in proofs, showing why it matched or didn't match
            context.push_operation(OperationKind::RuleBranchEvaluated {
                index: idx,
                matched: false,
                condition_expression: condition_expr.clone(),
                result_expression: result_expr.clone(),
                result_value: None,
            });

            non_matched_branches.push(NonMatchedBranch {
                condition: Box::new(condition_proof),
                result: None,
                clause_index: idx,
                source: branch.source.clone(),
            });
        }
    }

    // This should never be reached - branch 0's condition should always match
    // if no later branches matched (it's NOT(cond_1) AND NOT(cond_2) AND ...)
    Err(crate::LemmaError::Engine(
        "bug: No branch matched - branch 0 should always match when normalized".to_string(),
    ))
}

/// Evaluate an expression iteratively without recursion
/// Uses a work list approach: collect all expressions first, then evaluate in dependency order
fn evaluate_expression(
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext,
) -> crate::LemmaResult<OperationResult> {
    // First, collect all expressions in the tree
    let mut all_exprs: HashMap<Expression, ()> = HashMap::new();
    let mut work_list: Vec<&Expression> = vec![expr];

    while let Some(e) = work_list.pop() {
        if all_exprs.contains_key(e) {
            continue;
        }
        all_exprs.insert(e.clone(), ());

        // Add dependencies to work list
        match &e.kind {
            ExpressionKind::Arithmetic(left, _, right)
            | ExpressionKind::Comparison(left, _, right)
            | ExpressionKind::LogicalAnd(left, right)
            | ExpressionKind::LogicalOr(left, right) => {
                work_list.push(left);
                work_list.push(right);
            }
            ExpressionKind::LogicalNegation(operand, _)
            | ExpressionKind::UnitConversion(operand, _)
            | ExpressionKind::MathematicalComputation(_, operand) => {
                work_list.push(operand);
            }
            _ => {}
        }
    }

    // Now evaluate expressions in dependency order
    let mut results: HashMap<Expression, OperationResult> = HashMap::new();
    let mut remaining: Vec<Expression> = all_exprs.keys().cloned().collect();

    while !remaining.is_empty() {
        let mut progress = false;
        let mut to_remove = Vec::new();

        for expr_key in &remaining {
            let current = expr_key;

            // Check if all dependencies are ready
            let deps_ready = match &current.kind {
                ExpressionKind::Arithmetic(left, _, right)
                | ExpressionKind::Comparison(left, _, right)
                | ExpressionKind::LogicalAnd(left, right)
                | ExpressionKind::LogicalOr(left, right) => {
                    results.contains_key(left) && results.contains_key(right)
                }
                ExpressionKind::LogicalNegation(operand, _)
                | ExpressionKind::UnitConversion(operand, _)
                | ExpressionKind::MathematicalComputation(_, operand) => {
                    results.contains_key(operand)
                }
                _ => true,
            };

            if deps_ready {
                to_remove.push(expr_key.clone());
                progress = true;
            }
        }

        if !progress {
            // This should never happen - planning should have validated all dependencies
            // If we can't make progress, it indicates a bug in normalization or evaluation
            let remaining_exprs: Vec<String> = remaining
                .iter()
                .map(|expr| format!("{:?}", expr.kind))
                .collect();
            return Err(crate::LemmaError::Engine(format!(
                "bug: cannot evaluate expression - circular dependency or missing dependencies in expression tree.\n\
                 Remaining expressions: {:?}\n\
                 This indicates a bug in planning/normalization - all dependencies should have been validated.",
                remaining_exprs
            )));
        }

        // Evaluate expressions that are ready
        for expr_key in &to_remove {
            // Evaluate the expression
            let result = evaluate_single_expression(expr_key, &results, context)?;
            results.insert(expr_key.clone(), result);
        }

        for key in &to_remove {
            remaining.retain(|k| k != key);
        }
    }

    results.get(expr).cloned().ok_or_else(|| {
        crate::LemmaError::Engine("bug: expression was processed but has no result".to_string())
    })
}

/// Evaluate a single expression given its dependencies are already evaluated
fn evaluate_single_expression(
    current: &Expression,
    results: &HashMap<Expression, OperationResult>,
    context: &mut crate::evaluation::EvaluationContext,
) -> crate::LemmaResult<OperationResult> {
    let result = match &current.kind {
        ExpressionKind::Literal(lit) => {
            let proof_node = ProofNode::Value {
                value: lit.clone(),
                origin: ValueOrigin::Literal,
                source: current.source.clone(),
            };
            context.set_proof_node(current, proof_node);
            return Ok(OperationResult::Value(lit.clone()));
        }

        ExpressionKind::FactPath(fact_path) => {
            let fact_path_clone = fact_path.clone();
            let value = context.get_fact(fact_path).cloned();
            match value {
                Some(v) => {
                    context.push_operation(OperationKind::FactUsed {
                        fact_ref: fact_path_clone.clone(),
                        value: v.clone(),
                        expression: get_source_text(context, current)?,
                    });
                    let proof_node = ProofNode::Value {
                        value: v.clone(),
                        origin: ValueOrigin::Fact {
                            fact_ref: fact_path_clone,
                        },
                        source: current.source.clone(),
                    };
                    context.set_proof_node(current, proof_node);
                    return Ok(OperationResult::Value(v));
                }
                None => {
                    let proof_node = ProofNode::Veto {
                        message: Some(format!("Missing fact: {}", fact_path)),
                        source: current.source.clone(),
                    };
                    context.set_proof_node(current, proof_node);
                    return Ok(OperationResult::Veto(Some(format!(
                        "Missing fact: {}",
                        fact_path
                    ))));
                }
            }
        }

        ExpressionKind::RulePath(rule_path) => {
            let rule_path_clone = rule_path.clone();
            let result = context.rule_results.get(rule_path).cloned();
            match result {
                Some(r) => {
                    context.push_operation(OperationKind::RuleUsed {
                        rule_path: rule_path_clone.clone(),
                        result: r.clone(),
                        expression: get_source_text(context, current)?,
                    });

                    // Get the full proof tree from the referenced rule (evaluated earlier due to topological order)
                    let expansion = match context.get_rule_proof(rule_path) {
                        Some(existing_proof) => existing_proof.tree.clone(),
                        None => {
                            // Fallback to a simple value node if proof not found
                            ProofNode::Value {
                                value: match &r {
                                    OperationResult::Value(v) => v.clone(),
                                    OperationResult::Veto(_) => {
                                        LiteralValue::Boolean(BooleanValue::False)
                                    }
                                },
                                origin: ValueOrigin::Computed,
                                source: current.source.clone(),
                            }
                        }
                    };

                    let proof_node = ProofNode::RuleReference {
                        rule_path: rule_path_clone,
                        result: r.clone(),
                        source: current.source.clone(),
                        expansion: Box::new(expansion),
                    };
                    context.set_proof_node(current, proof_node);
                    return Ok(r);
                }
                None => {
                    let proof_node = ProofNode::Veto {
                        message: Some(format!(
                            "Rule {} not found or not yet computed",
                            rule_path.rule
                        )),
                        source: current.source.clone(),
                    };
                    context.set_proof_node(current, proof_node);
                    return Ok(OperationResult::Veto(Some(format!(
                        "Rule {} not found or not yet computed",
                        rule_path.rule
                    ))));
                }
            }
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_result = get_operand_result(results, left, "left")?;
            let right_result = get_operand_result(results, right, "right")?;

            if let OperationResult::Veto(_) = left_result {
                return propagate_veto_proof(context, current, left, left_result, "left operand");
            }
            if let OperationResult::Veto(_) = right_result {
                return propagate_veto_proof(
                    context,
                    current,
                    right,
                    right_result,
                    "right operand",
                );
            }

            let left_val = left_result.value().ok_or_else(|| {
                crate::LemmaError::Engine("Left operand result has no value".to_string())
            })?;
            let right_val = right_result.value().ok_or_else(|| {
                crate::LemmaError::Engine("Right operand result has no value".to_string())
            })?;
            let result = arithmetic_operation(left_val, op, right_val);

            let left_proof = get_proof_node_required(context, left, "left operand")?;
            let right_proof = get_proof_node_required(context, right, "right operand")?;

            if let OperationResult::Value(ref val) = result {
                let original_expr = get_source_text(context, current)?;
                let substituted_expr = format!("{} {} {}", left_val, op.symbol(), right_val);
                context.push_operation(OperationKind::Computation {
                    kind: ComputationKind::Arithmetic(op.clone()),
                    inputs: vec![left_val.clone(), right_val.clone()],
                    result: val.clone(),
                    expression: get_source_text(context, current)?,
                });
                let proof_node = ProofNode::Computation {
                    kind: ComputationKind::Arithmetic(op.clone()),
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: val.clone(),
                    source: current.source.clone(),
                    operands: vec![left_proof, right_proof],
                };
                context.set_proof_node(current, proof_node);
            } else if let OperationResult::Veto(_) = result {
                context.set_proof_node(current, left_proof);
            }
            Ok(result)
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_result = get_operand_result(results, left, "left")?;
            let right_result = get_operand_result(results, right, "right")?;

            if let OperationResult::Veto(_) = left_result {
                return propagate_veto_proof(context, current, left, left_result, "left operand");
            }
            if let OperationResult::Veto(_) = right_result {
                return propagate_veto_proof(
                    context,
                    current,
                    right,
                    right_result,
                    "right operand",
                );
            }

            let left_val = left_result.value().ok_or_else(|| {
                crate::LemmaError::Engine("Left operand result has no value".to_string())
            })?;
            let right_val = right_result.value().ok_or_else(|| {
                crate::LemmaError::Engine("Right operand result has no value".to_string())
            })?;
            let result = comparison_operation(left_val, op, right_val);

            let left_proof = get_proof_node_required(context, left, "left operand")?;
            let right_proof = get_proof_node_required(context, right, "right operand")?;

            if let OperationResult::Value(ref val) = result {
                let original_expr = get_source_text(context, current)?;
                let substituted_expr = format!("{} {} {}", left_val, op.symbol(), right_val);
                context.push_operation(OperationKind::Computation {
                    kind: ComputationKind::Comparison(op.clone()),
                    inputs: vec![left_val.clone(), right_val.clone()],
                    result: val.clone(),
                    expression: get_source_text(context, current)?,
                });
                let proof_node = ProofNode::Computation {
                    kind: ComputationKind::Comparison(op.clone()),
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: val.clone(),
                    source: current.source.clone(),
                    operands: vec![left_proof, right_proof],
                };
                context.set_proof_node(current, proof_node);
            } else if let OperationResult::Veto(_) = result {
                context.set_proof_node(current, left_proof);
            }
            Ok(result)
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_result = get_operand_result(results, left, "left")?;
            if let OperationResult::Veto(_) = left_result {
                return propagate_veto_proof(context, current, left, left_result, "left operand");
            }

            let left_bool = match left_result.value() {
                Some(LiteralValue::Boolean(b)) => b,
                Some(_) => {
                    return Ok(OperationResult::Veto(Some(
                        "Logical AND requires boolean operands".to_string(),
                    )));
                }
                None => {
                    return Ok(OperationResult::Veto(Some(
                        "Left operand is vetoed".to_string(),
                    )))
                }
            };

            if !bool::from(left_bool) {
                let left_proof = get_proof_node_required(context, left, "left operand")?;
                let original_expr = get_source_text(context, current)?;
                let substituted_expr = format!("{} and ...", left_bool);
                let proof_node = ProofNode::Condition {
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: false,
                    source: current.source.clone(),
                    operands: vec![left_proof],
                };
                context.set_proof_node(current, proof_node);
                Ok(OperationResult::Value(LiteralValue::Boolean(
                    BooleanValue::False,
                )))
            } else {
                let right_result = get_operand_result(results, right, "right")?;
                let left_proof = get_proof_node_required(context, left, "left operand")?;
                let right_proof = get_proof_node_required(context, right, "right operand")?;
                let original_expr = get_source_text(context, current)?;
                let right_bool = match right_result.value() {
                    Some(LiteralValue::Boolean(b)) => bool::from(b),
                    _ => false,
                };
                let substituted_expr = format!("{} and {}", bool::from(left_bool), right_bool);
                let result_bool = bool::from(left_bool) && right_bool;
                let proof_node = ProofNode::Condition {
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: result_bool,
                    source: current.source.clone(),
                    operands: vec![left_proof, right_proof],
                };
                context.set_proof_node(current, proof_node);
                Ok(right_result)
            }
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_result = get_operand_result(results, left, "left")?;
            if let OperationResult::Veto(_) = left_result {
                return propagate_veto_proof(context, current, left, left_result, "left operand");
            }

            let left_bool = match left_result.value() {
                Some(LiteralValue::Boolean(b)) => b,
                Some(_) => {
                    return Ok(OperationResult::Veto(Some(
                        "Logical OR requires boolean operands".to_string(),
                    )));
                }
                None => {
                    return Ok(OperationResult::Veto(Some(
                        "Left operand is vetoed".to_string(),
                    )))
                }
            };

            if bool::from(left_bool) {
                let left_proof = get_proof_node_required(context, left, "left operand")?;
                let original_expr = get_source_text(context, current)?;
                let substituted_expr = format!("{} or ...", bool::from(left_bool));
                let proof_node = ProofNode::Condition {
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: true,
                    source: current.source.clone(),
                    operands: vec![left_proof],
                };
                context.set_proof_node(current, proof_node);
                Ok(OperationResult::Value(LiteralValue::Boolean(
                    BooleanValue::True,
                )))
            } else {
                let right_result = get_operand_result(results, right, "right")?;
                let left_proof = get_proof_node_required(context, left, "left operand")?;
                let right_proof = get_proof_node_required(context, right, "right operand")?;
                let original_expr = get_source_text(context, current)?;
                let right_bool = match right_result.value() {
                    Some(LiteralValue::Boolean(b)) => bool::from(b),
                    _ => false,
                };
                let substituted_expr = format!("{} or {}", bool::from(left_bool), right_bool);
                let result_bool = bool::from(left_bool) || right_bool;
                let proof_node = ProofNode::Condition {
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: result_bool,
                    source: current.source.clone(),
                    operands: vec![left_proof, right_proof],
                };
                context.set_proof_node(current, proof_node);
                Ok(right_result)
            }
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let result = get_operand_result(results, operand, "operand")?;
            if let OperationResult::Veto(_) = result {
                return propagate_veto_proof(context, current, operand, result, "operand");
            }

            let value = result.value().ok_or_else(|| {
                crate::LemmaError::Engine("Operand result has no value".to_string())
            })?;
            let operand_proof = get_proof_node_required(context, operand, "operand")?;
            match value {
                LiteralValue::Boolean(b) => {
                    let result_bool = !bool::from(b);
                    let original_expr = get_source_text(context, current)?;
                    let substituted_expr = format!("not {}", bool::from(b));
                    let proof_node = ProofNode::Condition {
                        original_expression: original_expr,
                        expression: substituted_expr,
                        result: result_bool,
                        source: current.source.clone(),
                        operands: vec![operand_proof],
                    };
                    context.set_proof_node(current, proof_node);
                    Ok(OperationResult::Value(LiteralValue::Boolean(
                        if result_bool {
                            BooleanValue::True
                        } else {
                            BooleanValue::False
                        },
                    )))
                }
                _ => Ok(OperationResult::Veto(Some(
                    "Logical NOT requires boolean operand".to_string(),
                ))),
            }
        }

        ExpressionKind::UnitConversion(value_expr, target) => {
            let result = get_operand_result(results, value_expr, "operand")?;
            if let OperationResult::Veto(_) = result {
                return propagate_veto_proof(context, current, value_expr, result, "operand");
            }

            let value = result.value().ok_or_else(|| {
                crate::LemmaError::Engine("Operand result has no value".to_string())
            })?;
            let operand_proof = get_proof_node_required(context, value_expr, "operand")?;
            let conversion_result = super::operations::convert_unit(value, target);
            context.set_proof_node(current, operand_proof);
            Ok(conversion_result)
        }

        ExpressionKind::MathematicalComputation(op, operand) => {
            let result = get_operand_result(results, operand, "operand")?;
            if let OperationResult::Veto(_) = result {
                return propagate_veto_proof(context, current, operand, result, "operand");
            }

            let value = result.value().ok_or_else(|| {
                crate::LemmaError::Engine("Operand result has no value".to_string())
            })?;
            let operand_proof = get_proof_node_required(context, operand, "operand")?;
            let math_result = evaluate_mathematical_operator(op, value, current, context)?;
            context.set_proof_node(current, operand_proof);
            Ok(math_result)
        }

        ExpressionKind::Veto(veto_expr) => {
            let proof_node = ProofNode::Veto {
                message: veto_expr.message.clone(),
                source: current.source.clone(),
            };
            context.set_proof_node(current, proof_node);
            Ok(OperationResult::Veto(veto_expr.message.clone()))
        }

        ExpressionKind::FactReference(_) | ExpressionKind::RuleReference(_) => {
            unreachable!(
                "bug: FactReference/RuleReference in evaluation - \
                 should have been converted to FactPath/RulePath during graph building"
            )
        }
    };
    result
}

fn evaluate_mathematical_operator(
    op: &MathematicalComputation,
    value: &LiteralValue,
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext,
) -> crate::LemmaResult<OperationResult> {
    match value {
        LiteralValue::Number(n) => {
            use rust_decimal::prelude::ToPrimitive;
            let float_val = match n.to_f64() {
                Some(v) => v,
                None => {
                    return Ok(OperationResult::Veto(Some(
                        "Cannot convert to float for mathematical operation".to_string(),
                    )));
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
                    return Ok(OperationResult::Value(LiteralValue::Number(n.abs())));
                }
                MathematicalComputation::Floor => {
                    return Ok(OperationResult::Value(LiteralValue::Number(n.floor())));
                }
                MathematicalComputation::Ceil => {
                    return Ok(OperationResult::Value(LiteralValue::Number(n.ceil())));
                }
                MathematicalComputation::Round => {
                    return Ok(OperationResult::Value(LiteralValue::Number(n.round())));
                }
            };

            let decimal_result = match rust_decimal::Decimal::from_f64_retain(math_result) {
                Some(d) => d,
                None => {
                    return Ok(OperationResult::Veto(Some(
                        "Mathematical operation result cannot be represented".to_string(),
                    )));
                }
            };

            let result_value = LiteralValue::Number(decimal_result);
            context.push_operation(OperationKind::Computation {
                kind: ComputationKind::Mathematical(op.clone()),
                inputs: vec![value.clone()],
                result: result_value.clone(),
                expression: get_source_text(context, expr)?,
            });
            Ok(OperationResult::Value(result_value))
        }
        _ => Ok(OperationResult::Veto(Some(
            "Mathematical operators require number operands".to_string(),
        ))),
    }
}
