//! Iterative expression evaluation
//!
//! Evaluates expressions without recursion using a stack-based approach.
//! All runtime errors (division by zero, etc.) result in Veto instead of errors.

use super::operations::{ComputationKind, OperationKind, OperationResult};
use super::proof::{ProofNode, ValueSource};
use crate::computation::{arithmetic_operation, comparison_operation};
use crate::planning::ExecutableRule;
use crate::{
    BooleanValue, Expression, ExpressionKind, LemmaResult, LiteralValue, MathematicalComputation,
    Value,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Get a proof node, returning error if not found (indicates engine bug)
fn get_proof_node_required(
    context: &crate::evaluation::EvaluationContext,
    expr: &Expression,
    operand_name: &str,
) -> LemmaResult<ProofNode> {
    let loc = expr
        .source_location
        .as_ref()
        .expect("BUG: evaluated expression missing source_location");
    let proof = context.get_proof_node(expr).cloned().unwrap_or_else(|| {
        panic!(
            "BUG: {} was evaluated but has no proof node ({}:{}:{} in {})",
            operand_name, loc.attribute, loc.span.line, loc.span.col, loc.doc_name
        )
    });
    Ok(proof)
}

/// Get operand result, returning error if not found (indicates engine bug)
fn get_operand_result(
    results: &HashMap<Expression, OperationResult>,
    expr: &Expression,
    operand_name: &str,
) -> LemmaResult<OperationResult> {
    let loc = expr
        .source_location
        .as_ref()
        .expect("BUG: expression operand missing source_location");
    let result = results.get(expr).cloned().unwrap_or_else(|| {
        panic!(
            "BUG: {} operand was marked ready but result is missing ({}:{}:{} in {})",
            operand_name, loc.attribute, loc.span.line, loc.span.col, loc.doc_name
        )
    });
    Ok(result)
}

/// Propagate veto proof from operand to current expression
fn propagate_veto_proof(
    context: &mut crate::evaluation::EvaluationContext,
    current: &Expression,
    vetoed_operand: &Expression,
    veto_result: OperationResult,
    operand_name: &str,
) -> LemmaResult<OperationResult> {
    let proof = get_proof_node_required(context, vetoed_operand, operand_name)?;
    context.set_proof_node(current, proof);
    Ok(veto_result)
}

/// Evaluate a rule to produce its final result and proof
pub fn evaluate_rule(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext,
) -> LemmaResult<(OperationResult, crate::evaluation::proof::Proof)> {
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

            let condition_result = evaluate_expression(condition, context)?;
            let condition_proof = get_proof_node_required(context, condition, "condition")?;

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
                    return Ok((OperationResult::Veto(msg.clone()), proof));
                }
                OperationResult::Value(lit) => match &lit.value {
                    Value::Boolean(b) => bool::from(b),
                    _ => {
                        let veto = OperationResult::Veto(Some(
                            "Unless condition must evaluate to boolean".to_string(),
                        ));
                        let proof = crate::evaluation::proof::Proof {
                            rule_path: exec_rule.path.clone(),
                            source: exec_rule.source.clone(),
                            result: veto.clone(),
                            tree: ProofNode::Veto {
                                message: Some(
                                    "Unless condition must evaluate to boolean".to_string(),
                                ),
                                source_location: exec_rule.source.clone(),
                            },
                        };
                        return Ok((veto, proof));
                    }
                },
            };

            let unless_clause_index = branch_index - 1;

            if matched {
                // This unless clause matched - evaluate its result
                let result = evaluate_expression(&branch.result, context)?;

                context.push_operation(OperationKind::RuleBranchEvaluated {
                    index: Some(unless_clause_index),
                    matched: true,
                    condition_expr,
                    result_expr,
                    result_value: Some(result.clone()),
                });

                let result_proof = get_proof_node_required(context, &branch.result, "result")?;

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
                return Ok((result, proof));
            } else {
                // Branch didn't match - record it as non-matched.
                context.push_operation(OperationKind::RuleBranchEvaluated {
                    index: Some(unless_clause_index),
                    matched: false,
                    condition_expr,
                    result_expr,
                    result_value: None,
                });

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
    let default_result = evaluate_expression(&default_branch.result, context)?;

    context.push_operation(OperationKind::RuleBranchEvaluated {
        index: None,
        matched: true,
        condition_expr: None,
        result_expr: default_expr,
        result_value: Some(default_result.clone()),
    });

    let default_result_proof =
        get_proof_node_required(context, &default_branch.result, "default result")?;

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

    Ok((default_result, proof))
}

/// Evaluate a rule that has no unless clauses (simple case)
fn evaluate_rule_without_unless(
    exec_rule: &ExecutableRule,
    context: &mut crate::evaluation::EvaluationContext,
) -> LemmaResult<(OperationResult, crate::evaluation::proof::Proof)> {
    let default_branch = &exec_rule.branches[0];
    let default_expr = default_branch.result.get_source_text(&context.sources);
    let default_result = evaluate_expression(&default_branch.result, context)?;

    context.push_operation(OperationKind::RuleBranchEvaluated {
        index: None,
        matched: true,
        condition_expr: None,
        result_expr: default_expr,
        result_value: Some(default_result.clone()),
    });

    let root_proof_node =
        get_proof_node_required(context, &default_branch.result, "default result")?;

    let proof = crate::evaluation::proof::Proof {
        rule_path: exec_rule.path.clone(),
        source: exec_rule.source.clone(),
        result: default_result.clone(),
        tree: root_proof_node,
    };

    Ok((default_result, proof))
}

/// Evaluate an expression iteratively without recursion
/// Uses a work list approach: collect all expressions first, then evaluate in dependency order
fn evaluate_expression(
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext,
) -> LemmaResult<OperationResult> {
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

        for current in &remaining {
            // Check if all dependencies are ready
            let deps_ready = match &current.kind {
                ExpressionKind::Arithmetic(left, _, right)
                | ExpressionKind::Comparison(left, _, right)
                | ExpressionKind::LogicalAnd(left, right)
                | ExpressionKind::LogicalOr(left, right) => {
                    results.contains_key(left.as_ref()) && results.contains_key(right.as_ref())
                }
                ExpressionKind::LogicalNegation(operand, _)
                | ExpressionKind::UnitConversion(operand, _)
                | ExpressionKind::MathematicalComputation(_, operand) => {
                    results.contains_key(operand.as_ref())
                }
                _ => true,
            };

            if deps_ready {
                to_remove.push(current.clone());
                progress = true;
            }
        }

        if !progress {
            let loc = expr
                .source_location
                .as_ref()
                .expect("BUG: expression missing source_location");
            panic!(
                "BUG: circular dependency or missing dependency in expression tree ({}:{}:{} in {})",
                loc.attribute, loc.span.line, loc.span.col, loc.doc_name
            );
        }

        // Evaluate expressions that are ready
        for current in &to_remove {
            let result = evaluate_single_expression(current, &results, context)?;
            results.insert(current.clone(), result);
        }

        for key in &to_remove {
            remaining.retain(|k| k != key);
        }
    }

    let loc = expr
        .source_location
        .as_ref()
        .expect("BUG: expression missing source_location");
    let result = results.get(expr).cloned().unwrap_or_else(|| {
        panic!(
            "BUG: expression was processed but has no result ({}:{}:{} in {})",
            loc.attribute, loc.span.line, loc.span.col, loc.doc_name
        )
    });
    Ok(result)
}

/// Evaluate a single expression given its dependencies are already evaluated
fn evaluate_single_expression(
    current: &Expression,
    results: &HashMap<Expression, OperationResult>,
    context: &mut crate::evaluation::EvaluationContext,
) -> LemmaResult<OperationResult> {
    match &current.kind {
        ExpressionKind::Literal(lit) => {
            let proof_node = ProofNode::Value {
                value: lit.clone(),
                source: ValueSource::Literal,
                source_location: current.source_location.clone(),
            };
            context.set_proof_node(current, proof_node);
            Ok(OperationResult::Value(lit.clone()))
        }

        ExpressionKind::FactPath(fact_path) => {
            let fact_path_clone = fact_path.clone();
            let value = context.get_fact(fact_path).cloned();
            match value {
                Some(v) => {
                    context.push_operation(OperationKind::FactUsed {
                        fact_ref: fact_path_clone.clone(),
                        value: v.clone(),
                    });
                    let proof_node = ProofNode::Value {
                        value: v.clone(),
                        source: ValueSource::Fact {
                            fact_ref: fact_path_clone,
                        },
                        source_location: current.source_location.clone(),
                    };
                    context.set_proof_node(current, proof_node);
                    Ok(OperationResult::Value(v))
                }
                None => {
                    let proof_node = ProofNode::Veto {
                        message: Some(format!("Missing fact: {}", fact_path)),
                        source_location: current.source_location.clone(),
                    };
                    context.set_proof_node(current, proof_node);
                    Ok(OperationResult::Veto(Some(format!(
                        "Missing fact: {}",
                        fact_path
                    ))))
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
                    });

                    // Get the full proof tree from the referenced rule
                    let expansion = match context.get_rule_proof(rule_path) {
                        Some(existing_proof) => existing_proof.tree.clone(),
                        None => ProofNode::Value {
                            value: match &r {
                                OperationResult::Value(v) => v.clone(),
                                OperationResult::Veto(_) => {
                                    LiteralValue::boolean(BooleanValue::False)
                                }
                            },
                            source: ValueSource::Computed,
                            source_location: current.source_location.clone(),
                        },
                    };

                    let proof_node = ProofNode::RuleReference {
                        rule_path: rule_path_clone,
                        result: r.clone(),
                        source_location: current.source_location.clone(),
                        expansion: Box::new(expansion),
                    };
                    context.set_proof_node(current, proof_node);
                    Ok(r)
                }
                None => {
                    let proof_node = ProofNode::Veto {
                        message: Some(format!(
                            "Rule {} not found or not yet computed",
                            rule_path.rule
                        )),
                        source_location: current.source_location.clone(),
                    };
                    context.set_proof_node(current, proof_node);
                    Ok(OperationResult::Veto(Some(format!(
                        "Rule {} not found or not yet computed",
                        rule_path.rule
                    ))))
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
                let loc = current
                    .source_location
                    .as_ref()
                    .expect("Expression must have source_location");
                crate::LemmaError::engine(
                    "Left operand result has no value",
                    loc.span.clone(),
                    loc.attribute.clone(),
                    Arc::from(""),
                    loc.doc_name.clone(),
                    1,
                    None::<String>,
                )
            })?;
            let right_val = right_result.value().ok_or_else(|| {
                let loc = current
                    .source_location
                    .as_ref()
                    .expect("Expression must have source_location");
                crate::LemmaError::engine(
                    "Right operand result has no value",
                    loc.span.clone(),
                    loc.attribute.clone(),
                    Arc::from(""),
                    loc.doc_name.clone(),
                    1,
                    None::<String>,
                )
            })?;

            let result = arithmetic_operation(left_val, op, right_val);

            let left_proof = get_proof_node_required(context, left, "left operand")?;
            let right_proof = get_proof_node_required(context, right, "right operand")?;

            if let OperationResult::Value(ref val) = result {
                let expr_text = current.get_source_text(&context.sources);
                // Use source text if available, otherwise construct from values for proof display
                let original_expr = expr_text
                    .clone()
                    .unwrap_or_else(|| format!("{} {} {}", left_val, op.symbol(), right_val));
                let substituted_expr = format!("{} {} {}", left_val, op.symbol(), right_val);
                context.push_operation(OperationKind::Computation {
                    kind: ComputationKind::Arithmetic(op.clone()),
                    inputs: vec![left_val.clone(), right_val.clone()],
                    result: val.clone(),
                    expr: expr_text,
                });
                let proof_node = ProofNode::Computation {
                    kind: ComputationKind::Arithmetic(op.clone()),
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: val.clone(),
                    source_location: current.source_location.clone(),
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
                let loc = current
                    .source_location
                    .as_ref()
                    .expect("Expression must have source_location");
                crate::LemmaError::engine(
                    "Left operand result has no value",
                    loc.span.clone(),
                    loc.attribute.clone(),
                    Arc::from(""),
                    loc.doc_name.clone(),
                    1,
                    None::<String>,
                )
            })?;
            let right_val = right_result.value().ok_or_else(|| {
                let loc = current
                    .source_location
                    .as_ref()
                    .expect("Expression must have source_location");
                crate::LemmaError::engine(
                    "Right operand result has no value",
                    loc.span.clone(),
                    loc.attribute.clone(),
                    Arc::from(""),
                    loc.doc_name.clone(),
                    1,
                    None::<String>,
                )
            })?;

            let result = comparison_operation(left_val, op, right_val);

            let left_proof = get_proof_node_required(context, left, "left operand")?;
            let right_proof = get_proof_node_required(context, right, "right operand")?;

            if let OperationResult::Value(ref val) = result {
                let expr_text = current.get_source_text(&context.sources);
                // Use source text if available, otherwise construct from values for proof display
                let original_expr = expr_text
                    .clone()
                    .unwrap_or_else(|| format!("{} {} {}", left_val, op.symbol(), right_val));
                let substituted_expr = format!("{} {} {}", left_val, op.symbol(), right_val);
                context.push_operation(OperationKind::Computation {
                    kind: ComputationKind::Comparison(op.clone()),
                    inputs: vec![left_val.clone(), right_val.clone()],
                    result: val.clone(),
                    expr: expr_text,
                });
                let proof_node = ProofNode::Computation {
                    kind: ComputationKind::Comparison(op.clone()),
                    original_expression: original_expr,
                    expression: substituted_expr,
                    result: val.clone(),
                    source_location: current.source_location.clone(),
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
                Some(lit) => match &lit.value {
                    Value::Boolean(b) => b,
                    _ => {
                        return Ok(OperationResult::Veto(Some(
                            "Logical AND requires boolean operands".to_string(),
                        )));
                    }
                },
                None => {
                    return Ok(OperationResult::Veto(Some(
                        "Left operand is vetoed".to_string(),
                    )));
                }
            };

            if !bool::from(left_bool) {
                let left_proof = get_proof_node_required(context, left, "left operand")?;
                context.set_proof_node(current, left_proof);
                Ok(OperationResult::Value(LiteralValue::boolean(
                    BooleanValue::False,
                )))
            } else {
                let right_result = get_operand_result(results, right, "right")?;
                let right_proof = get_proof_node_required(context, right, "right operand")?;
                context.set_proof_node(current, right_proof);
                Ok(right_result)
            }
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_result = get_operand_result(results, left, "left")?;
            if let OperationResult::Veto(_) = left_result {
                return propagate_veto_proof(context, current, left, left_result, "left operand");
            }

            let left_bool = match left_result.value() {
                Some(lit) => match &lit.value {
                    Value::Boolean(b) => b,
                    _ => {
                        return Ok(OperationResult::Veto(Some(
                            "Logical OR requires boolean operands".to_string(),
                        )));
                    }
                },
                None => {
                    return Ok(OperationResult::Veto(Some(
                        "Left operand is vetoed".to_string(),
                    )));
                }
            };

            if bool::from(left_bool) {
                let left_proof = get_proof_node_required(context, left, "left operand")?;
                context.set_proof_node(current, left_proof);
                Ok(OperationResult::Value(LiteralValue::boolean(
                    BooleanValue::True,
                )))
            } else {
                let right_result = get_operand_result(results, right, "right")?;
                let right_proof = get_proof_node_required(context, right, "right operand")?;
                context.set_proof_node(current, right_proof);
                Ok(right_result)
            }
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let result = get_operand_result(results, operand, "operand")?;
            if let OperationResult::Veto(_) = result {
                return propagate_veto_proof(context, current, operand, result, "operand");
            }

            let value = match result.value() {
                Some(v) => v,
                None => return Ok(OperationResult::Veto(Some("Operand is vetoed".to_string()))),
            };
            let operand_proof = get_proof_node_required(context, operand, "operand")?;
            match &value.value {
                Value::Boolean(b) => {
                    let result_bool = !bool::from(b);
                    context.set_proof_node(current, operand_proof);
                    Ok(OperationResult::Value(LiteralValue {
                        value: Value::Boolean(if result_bool {
                            BooleanValue::True
                        } else {
                            BooleanValue::False
                        }),
                        lemma_type: crate::semantic::standard_boolean().clone(),
                    }))
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

            let value = match result.value() {
                Some(v) => v,
                None => return Ok(OperationResult::Veto(Some("Operand is vetoed".to_string()))),
            };
            let operand_proof = get_proof_node_required(context, value_expr, "operand")?;

            let conversion_result = crate::computation::convert_unit(value, target);

            context.set_proof_node(current, operand_proof);
            Ok(conversion_result)
        }

        ExpressionKind::MathematicalComputation(op, operand) => {
            let result = get_operand_result(results, operand, "operand")?;
            if let OperationResult::Veto(_) = result {
                return propagate_veto_proof(context, current, operand, result, "operand");
            }

            let value = match result.value() {
                Some(v) => v,
                None => return Ok(OperationResult::Veto(Some("Operand is vetoed".to_string()))),
            };
            let operand_proof = get_proof_node_required(context, operand, "operand")?;
            let math_result = evaluate_mathematical_operator(op, value, context);
            context.set_proof_node(current, operand_proof);
            Ok(math_result)
        }

        ExpressionKind::Veto(veto_expr) => {
            let proof_node = ProofNode::Veto {
                message: veto_expr.message.clone(),
                source_location: current.source_location.clone(),
            };
            context.set_proof_node(current, proof_node);
            Ok(OperationResult::Veto(veto_expr.message.clone()))
        }

        ExpressionKind::Reference(_)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::RuleReference(_) => {
            let proof_node = ProofNode::Veto {
                message: Some(
                    "Reference/FactReference/RuleReference must be resolved during planning"
                        .to_string(),
                ),
                source_location: current.source_location.clone(),
            };
            context.set_proof_node(current, proof_node);
            Ok(OperationResult::Veto(Some(
                "Reference/FactReference/RuleReference must be resolved during planning"
                    .to_string(),
            )))
        }
        ExpressionKind::UnresolvedUnitLiteral(_, _) => {
            panic!(
                "UnresolvedUnitLiteral found during evaluation - unresolved units must be resolved during planning"
            );
        }
    }
}

fn evaluate_mathematical_operator(
    op: &MathematicalComputation,
    value: &LiteralValue,
    context: &mut crate::evaluation::EvaluationContext,
) -> OperationResult {
    match &value.value {
        Value::Number(n) => {
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
                    return OperationResult::Value(LiteralValue::number_with_type(
                        n.abs(),
                        value.lemma_type.clone(),
                    ));
                }
                MathematicalComputation::Floor => {
                    return OperationResult::Value(LiteralValue::number_with_type(
                        n.floor(),
                        value.lemma_type.clone(),
                    ));
                }
                MathematicalComputation::Ceil => {
                    return OperationResult::Value(LiteralValue::number_with_type(
                        n.ceil(),
                        value.lemma_type.clone(),
                    ));
                }
                MathematicalComputation::Round => {
                    return OperationResult::Value(LiteralValue::number_with_type(
                        n.round(),
                        value.lemma_type.clone(),
                    ));
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
            OperationResult::Value(result_value)
        }
        _ => OperationResult::Veto(Some(
            "Mathematical operators require number operands".to_string(),
        )),
    }
}
