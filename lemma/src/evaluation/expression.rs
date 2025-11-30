//! Iterative expression evaluation
//!
//! Evaluates expressions without recursion using a stack-based approach.
//! All runtime errors (division by zero, etc.) result in Veto instead of errors.

use super::operations::{
    arithmetic_operation, comparison_operation, ComputationKind, OperationKind, OperationResult,
};
use super::proof::{ProofNode, ValueSource};
use crate::planning::ExecutableRule;
use crate::{
    BooleanValue, Expression, ExpressionId, ExpressionKind, LiteralValue, MathematicalComputation,
};
use std::collections::HashMap;

/// Evaluate a rule to produce its final result and proof
pub fn evaluate_rule(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext,
) -> (OperationResult, crate::evaluation::proof::Proof) {
    use crate::evaluation::proof::{Branch, NonMatchedBranch};

    // If rule has no unless clauses, just evaluate the default expression
    if exec_rule.branches.len() == 1 {
        return evaluate_rule_without_unless(exec_rule, context);
    }

    // Rule has unless clauses - collect all branch evaluations for Branches proof node
    let mut non_matched_branches: Vec<NonMatchedBranch> = Vec::new();

    // Evaluate branches in reverse order (last matching wins)
    for branch_index in (1..exec_rule.branches.len()).rev() {
        let branch = &exec_rule.branches[branch_index];
        if let Some(ref condition) = branch.condition {
            let condition_expr = condition.get_source_text(&context.sources);
            let result_expr = branch.result.get_source_text(&context.sources);

            let condition_result = evaluate_expression(condition, context);
            let condition_proof = context
                .get_proof_node(&condition.id)
                .cloned()
                .expect("bug: condition was evaluated but has no proof node");

            let matched = match condition_result {
                OperationResult::Veto(ref msg) => {
                    // Condition vetoed - this becomes the result
                    let unless_clause_index = branch_index - 1;
                    context.push_operation(
                        OperationKind::RuleBranchEvaluated {
                            index: Some(unless_clause_index),
                            matched: true,
                            condition_expr,
                            result_expr,
                            result_value: Some(OperationResult::Veto(msg.clone())),
                        },
                        condition.id,
                    );

                    // Build Branches node with this as the matched branch
                    let matched_branch = Branch {
                        condition: Some(Box::new(condition_proof)),
                        result: Box::new(ProofNode::Veto {
                            message: msg.clone(),
                            source_location: branch.result.source_location.clone(),
                        }),
                        clause_index: Some(unless_clause_index),
                        source_location: branch.source.clone(),
                    };

                    let branches_node = ProofNode::Branches {
                        matched: Box::new(matched_branch),
                        non_matched: non_matched_branches,
                        source_location: exec_rule.source.clone(),
                    };

                    let proof = crate::evaluation::proof::Proof {
                        rule_path: exec_rule.path.clone(),
                        source: exec_rule.source.clone(),
                        result: OperationResult::Veto(msg.clone()),
                        tree: branches_node,
                    };
                    return (OperationResult::Veto(msg.clone()), proof);
                }
                OperationResult::Value(LiteralValue::Boolean(b)) => b,
                _ => {
                    let veto = OperationResult::Veto(Some(
                        "Unless condition must evaluate to boolean".to_string(),
                    ));
                    let proof = crate::evaluation::proof::Proof {
                        rule_path: exec_rule.path.clone(),
                        source: exec_rule.source.clone(),
                        result: veto.clone(),
                        tree: ProofNode::Veto {
                            message: Some("Unless condition must evaluate to boolean".to_string()),
                            source_location: exec_rule.source.clone(),
                        },
                    };
                    return (veto, proof);
                }
            };

            let unless_clause_index = branch_index - 1;

            if bool::from(matched) {
                // This unless clause matched - evaluate its result
                let result = evaluate_expression(&branch.result, context);

                context.push_operation(
                    OperationKind::RuleBranchEvaluated {
                        index: Some(unless_clause_index),
                        matched: true,
                        condition_expr,
                        result_expr,
                        result_value: Some(result.clone()),
                    },
                    branch.result.id,
                );

                let result_proof = context
                    .get_proof_node(&branch.result.id)
                    .cloned()
                    .expect("bug: result expression was evaluated but has no proof node");

                // Build Branches node with this as the matched branch
                let matched_branch = Branch {
                    condition: Some(Box::new(condition_proof)),
                    result: Box::new(result_proof),
                    clause_index: Some(unless_clause_index),
                    source_location: branch.source.clone(),
                };

                let branches_node = ProofNode::Branches {
                    matched: Box::new(matched_branch),
                    non_matched: non_matched_branches,
                    source_location: exec_rule.source.clone(),
                };

                let proof = crate::evaluation::proof::Proof {
                    rule_path: exec_rule.path.clone(),
                    source: exec_rule.source.clone(),
                    result: result.clone(),
                    tree: branches_node,
                };
                return (result, proof);
            } else {
                // Branch didn't match - record it as non-matched
                context.push_operation(
                    OperationKind::RuleBranchEvaluated {
                        index: Some(unless_clause_index),
                        matched: false,
                        condition_expr,
                        result_expr,
                        result_value: None,
                    },
                    condition.id,
                );

                non_matched_branches.push(NonMatchedBranch {
                    condition: Box::new(condition_proof),
                    result: None,
                    clause_index: Some(unless_clause_index),
                    source_location: branch.source.clone(),
                });
            }
        }
    }

    // No unless clause matched - evaluate default expression (first branch)
    let default_branch = &exec_rule.branches[0];
    let default_expr = default_branch.result.get_source_text(&context.sources);
    let default_result = evaluate_expression(&default_branch.result, context);

    context.push_operation(
        OperationKind::RuleBranchEvaluated {
            index: None,
            matched: true,
            condition_expr: None,
            result_expr: default_expr,
            result_value: Some(default_result.clone()),
        },
        default_branch.result.id,
    );

    let default_result_proof = context
        .get_proof_node(&default_branch.result.id)
        .cloned()
        .expect("bug: default result was evaluated but has no proof node");

    // Default branch has no condition
    let matched_branch = Branch {
        condition: None,
        result: Box::new(default_result_proof),
        clause_index: None,
        source_location: default_branch.source.clone(),
    };

    let branches_node = ProofNode::Branches {
        matched: Box::new(matched_branch),
        non_matched: non_matched_branches,
        source_location: exec_rule.source.clone(),
    };

    let proof = crate::evaluation::proof::Proof {
        rule_path: exec_rule.path.clone(),
        source: exec_rule.source.clone(),
        result: default_result.clone(),
        tree: branches_node,
    };

    (default_result, proof)
}

/// Evaluate a rule that has no unless clauses (simple case)
fn evaluate_rule_without_unless(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext,
) -> (OperationResult, crate::evaluation::proof::Proof) {
    let default_branch = &exec_rule.branches[0];
    let default_expr = default_branch.result.get_source_text(&context.sources);
    let default_result = evaluate_expression(&default_branch.result, context);

    context.push_operation(
        OperationKind::RuleBranchEvaluated {
            index: None,
            matched: true,
            condition_expr: None,
            result_expr: default_expr,
            result_value: Some(default_result.clone()),
        },
        default_branch.result.id,
    );

    let root_proof_node = context
        .get_proof_node(&default_branch.result.id)
        .cloned()
        .expect("bug: default branch result was evaluated but has no proof node");

    let proof = crate::evaluation::proof::Proof {
        rule_path: exec_rule.path.clone(),
        source: exec_rule.source.clone(),
        result: default_result.clone(),
        tree: root_proof_node,
    };

    (default_result, proof)
}

/// Evaluate an expression iteratively without recursion
/// Uses a work list approach: collect all expressions first, then evaluate in dependency order
fn evaluate_expression(
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext,
) -> OperationResult {
    // First, collect all expressions in the tree
    let mut all_exprs: HashMap<ExpressionId, &Expression> = HashMap::new();
    let mut work_list: Vec<&Expression> = vec![expr];

    while let Some(e) = work_list.pop() {
        if all_exprs.contains_key(&e.id) {
            continue;
        }
        all_exprs.insert(e.id, e);

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
    let mut results: HashMap<ExpressionId, OperationResult> = HashMap::new();
    let mut remaining: Vec<ExpressionId> = all_exprs.keys().cloned().collect();

    while !remaining.is_empty() {
        let mut progress = false;
        let mut to_remove = Vec::new();

        for &expr_id in &remaining {
            let current = match all_exprs.get(&expr_id) {
                Some(c) => c,
                None => {
                    // This shouldn't happen, but handle gracefully
                    continue;
                }
            };

            // Check if all dependencies are ready
            let deps_ready = match &current.kind {
                ExpressionKind::Arithmetic(left, _, right)
                | ExpressionKind::Comparison(left, _, right)
                | ExpressionKind::LogicalAnd(left, right)
                | ExpressionKind::LogicalOr(left, right) => {
                    results.contains_key(&left.id) && results.contains_key(&right.id)
                }
                ExpressionKind::LogicalNegation(operand, _)
                | ExpressionKind::UnitConversion(operand, _)
                | ExpressionKind::MathematicalComputation(_, operand) => {
                    results.contains_key(&operand.id)
                }
                _ => true,
            };

            if deps_ready {
                to_remove.push(expr_id);
                progress = true;
            }
        }

        if !progress {
            // Circular dependency or missing dependency - evaluate what we can
            for &expr_id in &remaining {
                to_remove.push(expr_id);
            }
        }

        // Evaluate expressions that are ready
        for expr_id in &to_remove {
            let current = match all_exprs.get(expr_id) {
                Some(c) => c,
                None => {
                    // This shouldn't happen, but handle gracefully
                    results.insert(
                        *expr_id,
                        OperationResult::Veto(Some(
                            "Expression not found in evaluation tree".to_string(),
                        )),
                    );
                    continue;
                }
            };

            // Evaluate the expression
            let result = evaluate_single_expression(current, &results, context);
            results.insert(*expr_id, result);
        }

        remaining.retain(|id| !to_remove.contains(id));
    }

    results
        .get(&expr.id)
        .cloned()
        .expect("bug: expression was processed but has no result")
}

/// Evaluate a single expression given its dependencies are already evaluated
fn evaluate_single_expression(
    current: &Expression,
    results: &HashMap<ExpressionId, OperationResult>,
    context: &mut crate::evaluation::EvaluationContext,
) -> OperationResult {
    let result = match &current.kind {
        ExpressionKind::Literal(lit) => {
            let proof_node = ProofNode::Value {
                value: lit.clone(),
                source: ValueSource::Literal,
                source_location: current.source_location.clone(),
            };
            context.set_proof_node(current.id, proof_node);
            OperationResult::Value(lit.clone())
        }

        ExpressionKind::FactPath(fact_path) => {
            let fact_path_clone = fact_path.clone();
            let value = context.get_fact(fact_path).cloned();
            match value {
                Some(v) => {
                    context.push_operation(
                        OperationKind::FactUsed {
                            fact_ref: fact_path_clone.clone(),
                            value: v.clone(),
                        },
                        current.id,
                    );
                    let proof_node = ProofNode::Value {
                        value: v.clone(),
                        source: ValueSource::Fact {
                            fact_ref: fact_path_clone,
                        },
                        source_location: current.source_location.clone(),
                    };
                    context.set_proof_node(current.id, proof_node);
                    OperationResult::Value(v)
                }
                None => {
                    let proof_node = ProofNode::Veto {
                        message: Some(format!("Missing fact: {}", fact_path)),
                        source_location: current.source_location.clone(),
                    };
                    context.set_proof_node(current.id, proof_node);
                    OperationResult::Veto(Some(format!("Missing fact: {}", fact_path)))
                }
            }
        }

        ExpressionKind::RulePath(rule_path) => {
            let rule_path_clone = rule_path.clone();
            let result = context.rule_results.get(rule_path).cloned();
            match result {
                Some(r) => {
                    context.push_operation(
                        OperationKind::RuleUsed {
                            rule_path: rule_path_clone.clone(),
                            result: r.clone(),
                        },
                        current.id,
                    );

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
                                source: ValueSource::Computed,
                                source_location: current.source_location.clone(),
                            }
                        }
                    };

                    let proof_node = ProofNode::RuleReference {
                        rule_path: rule_path_clone,
                        result: r.clone(),
                        source_location: current.source_location.clone(),
                        expansion: Box::new(expansion),
                    };
                    context.set_proof_node(current.id, proof_node);
                    r
                }
                None => {
                    let proof_node = ProofNode::Veto {
                        message: Some(format!(
                            "Rule {} not found or not yet computed",
                            rule_path.rule
                        )),
                        source_location: current.source_location.clone(),
                    };
                    context.set_proof_node(current.id, proof_node);
                    OperationResult::Veto(Some(format!(
                        "Rule {} not found or not yet computed",
                        rule_path.rule
                    )))
                }
            }
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_result = match results.get(&left.id) {
                Some(r) => r.clone(),
                None => {
                    return OperationResult::Veto(Some("Missing left operand result".to_string()))
                }
            };
            let right_result = match results.get(&right.id) {
                Some(r) => r.clone(),
                None => {
                    return OperationResult::Veto(Some("Missing right operand result".to_string()))
                }
            };

            if let OperationResult::Veto(_) = left_result {
                let proof = context
                    .get_proof_node(&left.id)
                    .cloned()
                    .expect("bug: left operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                left_result
            } else if let OperationResult::Veto(_) = right_result {
                let proof = context
                    .get_proof_node(&right.id)
                    .cloned()
                    .expect("bug: right operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                right_result
            } else {
                let left_val = match left_result.value() {
                    Some(v) => v,
                    None => {
                        return OperationResult::Veto(Some("Left operand is vetoed".to_string()))
                    }
                };
                let right_val = match right_result.value() {
                    Some(v) => v,
                    None => {
                        return OperationResult::Veto(Some("Right operand is vetoed".to_string()))
                    }
                };
                let result = arithmetic_operation(left_val, op, right_val);

                let left_proof = context
                    .get_proof_node(&left.id)
                    .cloned()
                    .expect("bug: left operand was evaluated but has no proof node");
                let right_proof = context
                    .get_proof_node(&right.id)
                    .cloned()
                    .expect("bug: right operand was evaluated but has no proof node");

                if let OperationResult::Value(ref val) = result {
                    let expr_text = current.get_source_text(&context.sources);
                    let original_expr = expr_text.clone().unwrap_or_default();
                    let substituted_expr = format!("{} {} {}", left_val, op.symbol(), right_val);
                    context.push_operation(
                        OperationKind::Computation {
                            kind: ComputationKind::Arithmetic(op.clone()),
                            inputs: vec![left_val.clone(), right_val.clone()],
                            result: val.clone(),
                            expr: expr_text,
                        },
                        current.id,
                    );
                    let proof_node = ProofNode::Computation {
                        kind: ComputationKind::Arithmetic(op.clone()),
                        original_expression: original_expr,
                        expression: substituted_expr,
                        result: val.clone(),
                        source_location: current.source_location.clone(),
                        operands: vec![left_proof, right_proof],
                    };
                    context.set_proof_node(current.id, proof_node);
                } else if let OperationResult::Veto(_) = result {
                    let proof_node = left_proof;
                    context.set_proof_node(current.id, proof_node);
                }
                result
            }
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_result = match results.get(&left.id) {
                Some(r) => r.clone(),
                None => {
                    return OperationResult::Veto(Some("Missing left operand result".to_string()))
                }
            };
            let right_result = match results.get(&right.id) {
                Some(r) => r.clone(),
                None => {
                    return OperationResult::Veto(Some("Missing right operand result".to_string()))
                }
            };

            if let OperationResult::Veto(_) = left_result {
                let proof = context
                    .get_proof_node(&left.id)
                    .cloned()
                    .expect("bug: left operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                left_result
            } else if let OperationResult::Veto(_) = right_result {
                let proof = context
                    .get_proof_node(&right.id)
                    .cloned()
                    .expect("bug: right operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                right_result
            } else {
                let left_val = match left_result.value() {
                    Some(v) => v,
                    None => {
                        return OperationResult::Veto(Some("Left operand is vetoed".to_string()))
                    }
                };
                let right_val = match right_result.value() {
                    Some(v) => v,
                    None => {
                        return OperationResult::Veto(Some("Right operand is vetoed".to_string()))
                    }
                };
                let result = comparison_operation(left_val, op, right_val);

                let left_proof = context
                    .get_proof_node(&left.id)
                    .cloned()
                    .expect("bug: left operand was evaluated but has no proof node");
                let right_proof = context
                    .get_proof_node(&right.id)
                    .cloned()
                    .expect("bug: right operand was evaluated but has no proof node");

                if let OperationResult::Value(ref val) = result {
                    let expr_text = current.get_source_text(&context.sources);
                    let original_expr = expr_text.clone().unwrap_or_default();
                    let substituted_expr = format!("{} {} {}", left_val, op.symbol(), right_val);
                    context.push_operation(
                        OperationKind::Computation {
                            kind: ComputationKind::Comparison(op.clone()),
                            inputs: vec![left_val.clone(), right_val.clone()],
                            result: val.clone(),
                            expr: expr_text,
                        },
                        current.id,
                    );
                    let proof_node = ProofNode::Computation {
                        kind: ComputationKind::Comparison(op.clone()),
                        original_expression: original_expr,
                        expression: substituted_expr,
                        result: val.clone(),
                        source_location: current.source_location.clone(),
                        operands: vec![left_proof, right_proof],
                    };
                    context.set_proof_node(current.id, proof_node);
                } else if let OperationResult::Veto(_) = result {
                    let proof_node = left_proof;
                    context.set_proof_node(current.id, proof_node);
                }
                result
            }
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_result = match results.get(&left.id) {
                Some(r) => r.clone(),
                None => {
                    return OperationResult::Veto(Some("Missing left operand result".to_string()))
                }
            };
            if let OperationResult::Veto(_) = left_result {
                let proof = context
                    .get_proof_node(&left.id)
                    .cloned()
                    .expect("bug: left operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                left_result
            } else {
                let left_bool = match left_result.value() {
                    Some(LiteralValue::Boolean(b)) => b,
                    Some(_) => {
                        return OperationResult::Veto(Some(
                            "Logical AND requires boolean operands".to_string(),
                        ));
                    }
                    None => {
                        return OperationResult::Veto(Some("Left operand is vetoed".to_string()))
                    }
                };

                if !bool::from(left_bool) {
                    let left_proof = context
                        .get_proof_node(&left.id)
                        .cloned()
                        .expect("bug: left operand was evaluated but has no proof node");
                    context.set_proof_node(current.id, left_proof);
                    OperationResult::Value(LiteralValue::Boolean(BooleanValue::False))
                } else {
                    let right_result = match results.get(&right.id) {
                        Some(r) => r.clone(),
                        None => {
                            return OperationResult::Veto(Some(
                                "Missing right operand result".to_string(),
                            ))
                        }
                    };
                    let right_proof = context
                        .get_proof_node(&right.id)
                        .cloned()
                        .expect("bug: right operand was evaluated but has no proof node");
                    context.set_proof_node(current.id, right_proof);
                    right_result
                }
            }
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_result = match results.get(&left.id) {
                Some(r) => r.clone(),
                None => {
                    return OperationResult::Veto(Some("Missing left operand result".to_string()))
                }
            };
            if let OperationResult::Veto(_) = left_result {
                let proof = context
                    .get_proof_node(&left.id)
                    .cloned()
                    .expect("bug: left operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                left_result
            } else {
                let left_bool = match left_result.value() {
                    Some(LiteralValue::Boolean(b)) => b,
                    Some(_) => {
                        return OperationResult::Veto(Some(
                            "Logical OR requires boolean operands".to_string(),
                        ));
                    }
                    None => {
                        return OperationResult::Veto(Some("Left operand is vetoed".to_string()))
                    }
                };

                if bool::from(left_bool) {
                    let left_proof = context
                        .get_proof_node(&left.id)
                        .cloned()
                        .expect("bug: left operand was evaluated but has no proof node");
                    context.set_proof_node(current.id, left_proof);
                    OperationResult::Value(LiteralValue::Boolean(BooleanValue::True))
                } else {
                    let right_result = match results.get(&right.id) {
                        Some(r) => r.clone(),
                        None => {
                            return OperationResult::Veto(Some(
                                "Missing right operand result".to_string(),
                            ))
                        }
                    };
                    let right_proof = context
                        .get_proof_node(&right.id)
                        .cloned()
                        .expect("bug: right operand was evaluated but has no proof node");
                    context.set_proof_node(current.id, right_proof);
                    right_result
                }
            }
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let result = match results.get(&operand.id) {
                Some(r) => r.clone(),
                None => return OperationResult::Veto(Some("Missing operand result".to_string())),
            };
            if let OperationResult::Veto(_) = result {
                let proof = context
                    .get_proof_node(&operand.id)
                    .cloned()
                    .expect("bug: operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                result
            } else {
                let value = match result.value() {
                    Some(v) => v,
                    None => return OperationResult::Veto(Some("Operand is vetoed".to_string())),
                };
                let operand_proof = context
                    .get_proof_node(&operand.id)
                    .cloned()
                    .expect("bug: operand was evaluated but has no proof node");
                match value {
                    LiteralValue::Boolean(b) => {
                        let result_bool = !bool::from(b);
                        context.set_proof_node(current.id, operand_proof);
                        OperationResult::Value(LiteralValue::Boolean(if result_bool {
                            BooleanValue::True
                        } else {
                            BooleanValue::False
                        }))
                    }
                    _ => OperationResult::Veto(Some(
                        "Logical NOT requires boolean operand".to_string(),
                    )),
                }
            }
        }

        ExpressionKind::UnitConversion(value_expr, target) => {
            let result = match results.get(&value_expr.id) {
                Some(r) => r.clone(),
                None => return OperationResult::Veto(Some("Missing operand result".to_string())),
            };
            if let OperationResult::Veto(_) = result {
                let proof = context
                    .get_proof_node(&value_expr.id)
                    .cloned()
                    .expect("bug: operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                result
            } else {
                let value = match result.value() {
                    Some(v) => v,
                    None => return OperationResult::Veto(Some("Operand is vetoed".to_string())),
                };
                let operand_proof = context
                    .get_proof_node(&value_expr.id)
                    .cloned()
                    .expect("bug: operand was evaluated but has no proof node");
                let conversion_result = super::units::convert_unit(value, target);
                context.set_proof_node(current.id, operand_proof);
                conversion_result
            }
        }

        ExpressionKind::MathematicalComputation(op, operand) => {
            let result = match results.get(&operand.id) {
                Some(r) => r.clone(),
                None => return OperationResult::Veto(Some("Missing operand result".to_string())),
            };
            if let OperationResult::Veto(_) = result {
                let proof = context
                    .get_proof_node(&operand.id)
                    .cloned()
                    .expect("bug: operand was evaluated but has no proof node");
                context.set_proof_node(current.id, proof);
                result
            } else {
                let value = match result.value() {
                    Some(v) => v,
                    None => return OperationResult::Veto(Some("Operand is vetoed".to_string())),
                };
                let operand_proof = context
                    .get_proof_node(&operand.id)
                    .cloned()
                    .expect("bug: operand was evaluated but has no proof node");
                let math_result = evaluate_mathematical_operator(op, value, current.id, context);
                context.set_proof_node(current.id, operand_proof);
                math_result
            }
        }

        ExpressionKind::Veto(veto_expr) => {
            let proof_node = ProofNode::Veto {
                message: veto_expr.message.clone(),
                source_location: current.source_location.clone(),
            };
            context.set_proof_node(current.id, proof_node);
            OperationResult::Veto(veto_expr.message.clone())
        }

        ExpressionKind::FactReference(_) | ExpressionKind::RuleReference(_) => {
            let proof_node = ProofNode::Veto {
                    message: Some("FactReference and RuleReference should be resolved to FactPath/RulePath during planning".to_string()),
                    source_location: current.source_location.clone(),
                };
            context.set_proof_node(current.id, proof_node);
            OperationResult::Veto(Some(
                    "FactReference and RuleReference should be resolved to FactPath/RulePath during planning".to_string(),
                ))
        }
    };
    result
}

fn evaluate_mathematical_operator(
    op: &MathematicalComputation,
    value: &LiteralValue,
    expression_id: ExpressionId,
    context: &mut crate::evaluation::EvaluationContext,
) -> OperationResult {
    match value {
        LiteralValue::Number(n) => {
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
                    return OperationResult::Value(LiteralValue::Number(n.abs()));
                }
                MathematicalComputation::Floor => {
                    return OperationResult::Value(LiteralValue::Number(n.floor()));
                }
                MathematicalComputation::Ceil => {
                    return OperationResult::Value(LiteralValue::Number(n.ceil()));
                }
                MathematicalComputation::Round => {
                    return OperationResult::Value(LiteralValue::Number(n.round()));
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

            let result_value = LiteralValue::Number(decimal_result);
            context.push_operation(
                OperationKind::Computation {
                    kind: ComputationKind::Mathematical(op.clone()),
                    inputs: vec![value.clone()],
                    result: result_value.clone(),
                    expr: None,
                },
                expression_id,
            );
            OperationResult::Value(result_value)
        }
        _ => OperationResult::Veto(Some(
            "Mathematical operators require number operands".to_string(),
        )),
    }
}
