use crate::{
    ComputationKind, Expression, ExpressionId, ExpressionKind, FactReference, LemmaDoc, LemmaError,
    LemmaResult, LiteralValue, OperationKind, OperationRecord, OperationResult, RulePath,
    SourceLocation,
};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct Proof {
    pub rule_path: RulePath,
    pub source: Option<SourceLocation>,
    pub result: OperationResult,
    pub tree: ProofNode,
}

#[derive(Debug, Clone, Serialize)]
pub enum ProofNode {
    Value {
        value: LiteralValue,
        source: ValueSource,
        source_location: Option<SourceLocation>,
    },
    RuleReference {
        rule_path: RulePath,
        result: OperationResult,
        source_location: Option<SourceLocation>,
        expansion: Box<ProofNode>,
    },
    Computation {
        kind: ComputationKind,
        original_expression: String,
        expression: String,
        result: LiteralValue,
        source_location: Option<SourceLocation>,
        operands: Vec<ProofNode>,
    },
    Branches {
        matched: Box<Branch>,
        non_matched: Vec<NonMatchedBranch>,
        source_location: Option<SourceLocation>,
    },
    Condition {
        original_expression: String,
        expression: String,
        result: bool,
        source_location: Option<SourceLocation>,
        operands: Vec<ProofNode>,
    },
    Veto {
        message: Option<String>,
        source_location: Option<SourceLocation>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub enum ValueSource {
    Fact { fact_ref: FactReference },
    Literal,
    Computed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Branch {
    pub condition: Option<Box<ProofNode>>,
    pub result: Box<ProofNode>,
    pub clause_index: Option<usize>,
    pub source_location: Option<SourceLocation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NonMatchedBranch {
    pub condition: Box<ProofNode>,
    pub result: Box<ProofNode>,
    pub clause_index: Option<usize>,
    pub source_location: Option<SourceLocation>,
}

/// Find an operation by expression ID in the flat operations list
fn find_operation_for_expression(
    expr_id: ExpressionId,
    operations: &[OperationRecord],
) -> Option<&OperationRecord> {
    operations.iter().find(|op| op.expression_id == expr_id)
}

/// Build proof tree from rule by checking for unless clauses
///
/// This is the public entry point called from evaluate_rule.
/// It uses already-built proofs from rule_proofs instead of reconstructing paths.
pub fn build_proof_node_from_rule(
    rule: &crate::LemmaRule,
    operations: &[OperationRecord],
    doc: &LemmaDoc,
    all_documents: &HashMap<String, LemmaDoc>,
    rule_proofs: &HashMap<RulePath, Proof>,
    sources: &HashMap<String, String>,
) -> LemmaResult<ProofNode> {
    // If operations is empty, the rule failed before evaluation started
    // This should not happen when building a proof - proofs are only built for successfully evaluated rules
    // or rules that failed during evaluation (which would have operations)
    if operations.is_empty() {
        return Err(LemmaError::Engine(
            "Cannot build proof for rule with no operations (rule failed before evaluation started)".to_string(),
        ));
    }

    // Check if this rule has unless clauses
    // Only use branch representation if the rule actually has unless clauses
    if !rule.unless_clauses.is_empty() {
        return build_branches_node_from_ast(
            rule,
            operations,
            doc,
            all_documents,
            rule_proofs,
            sources,
        );
    }

    // No unless clauses - just evaluate the main expression directly
    build_proof_node_from_expression(
        &rule.expression,
        operations,
        doc,
        all_documents,
        rule_proofs,
        sources,
    )
}

/// Build branches node from rule AST and operations
fn build_branches_node_from_ast(
    rule: &crate::LemmaRule,
    operations: &[OperationRecord],
    doc: &LemmaDoc,
    all_documents: &HashMap<String, LemmaDoc>,
    rule_proofs: &HashMap<RulePath, Proof>,
    sources: &HashMap<String, String>,
) -> LemmaResult<ProofNode> {
    // Find which branch matched
    let matched_branch_op = operations
        .iter()
        .find(|op| {
            matches!(
                &op.kind,
                OperationKind::RuleBranchEvaluated { matched: true, .. }
            )
        })
        .ok_or_else(|| {
            LemmaError::Engine(
                "No matched branch found in RuleBranchEvaluated operations".to_string(),
            )
        })?;

    let (_matched_index, matched_branch) = if let OperationKind::RuleBranchEvaluated {
        index,
        result_value,
        ..
    } = &matched_branch_op.kind
    {
        let condition = if let Some(clause_index) = index {
            let unless_clause = rule.unless_clauses.get(*clause_index).ok_or_else(|| {
                LemmaError::Engine(format!("Unless clause at index {clause_index} not found"))
            })?;

            // Build proof for condition
            let condition_node = build_proof_node_from_expression(
                &unless_clause.condition,
                operations,
                doc,
                all_documents,
                rule_proofs,
                sources,
            )?;
            Some(Box::new(condition_node))
        } else {
            None
        };

        // Build proof for the result expression
        let result_expr = if let Some(clause_index) = index {
            &rule.unless_clauses[*clause_index].result
        } else {
            &rule.expression
        };

        // Extract the result from the operation record
        let result_node = match result_value.as_ref() {
            Some(OperationResult::Veto(msg)) => {
                // Veto result - create Veto node with message
                ProofNode::Veto {
                    message: msg.clone(),
                    source_location: result_expr.source_location.clone(),
                }
            }
            Some(OperationResult::Value(_)) => {
                // Regular value - build proof from expression
                build_proof_node_from_expression(
                    result_expr,
                    operations,
                    doc,
                    all_documents,
                    rule_proofs,
                    sources,
                )?
            }
            None => {
                return Err(LemmaError::Engine(
                    "Matched branch has no result value".to_string(),
                ));
            }
        };

        let source_location = if let Some(clause_index) = index {
            rule.unless_clauses[*clause_index].source_location.clone()
        } else {
            rule.source_location.clone()
        };

        let branch = Branch {
            condition,
            result: Box::new(result_node),
            clause_index: *index,
            source_location,
        };

        (*index, branch)
    } else {
        return Err(LemmaError::Engine(
            "Matched branch operation is not RuleBranchEvaluated".to_string(),
        ));
    };

    // Build non-matched branches (all branches that didn't win)
    // This includes: default if not matched, and all unless clauses that didn't win
    let mut non_matched: Vec<NonMatchedBranch> = operations
        .iter()
        .filter_map(|op| {
            if let OperationKind::RuleBranchEvaluated {
                index,
                matched: false,
                ..
            } = &op.kind
            {
                // Build proof for this non-matched branch
                if let Some(clause_index) = index {
                    // Non-matched unless clause
                    let unless_clause = rule.unless_clauses.get(*clause_index)?;
                    let condition_node = build_proof_node_from_expression(
                        &unless_clause.condition,
                        operations,
                        doc,
                        all_documents,
                        rule_proofs,
                        sources,
                    )
                    .ok()?;

                    // Build result node for what this branch would have returned
                    let result_node = match &unless_clause.result.kind {
                        ExpressionKind::Veto(veto_expr) => ProofNode::Veto {
                            message: veto_expr.message.clone(),
                            source_location: unless_clause.result.source_location.clone(),
                        },
                        _ => {
                            // For non-veto results, build the proof node
                            // Note: we don't evaluate it, we just show the structure
                            build_proof_node_from_expression(
                                &unless_clause.result,
                                &[], // Empty operations since this branch didn't execute
                                doc,
                                all_documents,
                                rule_proofs,
                                sources,
                            )
                            .ok()?
                        }
                    };

                    return Some(NonMatchedBranch {
                        condition: Box::new(condition_node),
                        result: Box::new(result_node),
                        clause_index: Some(*clause_index),
                        source_location: unless_clause.source_location.clone(),
                    });
                } else {
                    // Non-matched default branch
                    // Default doesn't have a condition, so we'll represent it with a placeholder
                    let result_node = build_proof_node_from_expression(
                        &rule.expression,
                        &[], // Empty operations since this branch didn't execute
                        doc,
                        all_documents,
                        rule_proofs,
                        sources,
                    )
                    .ok()?;

                    return Some(NonMatchedBranch {
                        condition: Box::new(ProofNode::Value {
                            value: LiteralValue::Boolean(crate::BooleanValue::False),
                            source: ValueSource::Computed,
                            source_location: rule.source_location.clone(),
                        }),
                        result: Box::new(result_node),
                        clause_index: None,
                        source_location: rule.source_location.clone(),
                    });
                }
            }
            None
        })
        .collect();

    // Sort non-matched branches by clause_index to maintain original order
    // None (default) should come last
    non_matched.sort_by_key(|branch| branch.clause_index.unwrap_or(usize::MAX));

    Ok(ProofNode::Branches {
        matched: Box::new(matched_branch),
        non_matched,
        source_location: rule.source_location.clone(),
    })
}

/// Build proof node by walking the Expression AST
///
/// `doc` and `all_documents` are only used in recursive calls to resolve rule references.
#[allow(clippy::only_used_in_recursion)]
fn build_proof_node_from_expression(
    expr: &Expression,
    operations: &[OperationRecord],
    doc: &LemmaDoc,
    all_documents: &HashMap<String, LemmaDoc>,
    rule_proofs: &HashMap<RulePath, Proof>,
    sources: &HashMap<String, String>,
) -> LemmaResult<ProofNode> {
    let source_location = expr.source_location.clone();

    // Find the operation for this expression
    let op = find_operation_for_expression(expr.id, operations);

    match &expr.kind {
        ExpressionKind::Literal(lit) => Ok(ProofNode::Value {
            value: lit.clone(),
            source: ValueSource::Literal,
            source_location,
        }),

        ExpressionKind::FactReference(fact_ref) => {
            // Get the actual value from the operation
            if let Some(op) = op {
                if let OperationKind::FactUsed { value, .. } = &op.kind {
                    return Ok(ProofNode::Value {
                        value: value.clone(),
                        source: ValueSource::Fact {
                            fact_ref: fact_ref.clone(),
                        },
                        source_location,
                    });
                }
            }
            Err(LemmaError::Engine(format!(
                "No FactUsed operation found for fact reference {:?}",
                fact_ref
            )))
        }

        ExpressionKind::RuleReference(rule_ref) => {
            // The RuleReference was evaluated during execution and its proof is already available
            // We just need to find it in the rule_proofs map

            // Find the RuleUsed operation which has the full path used during evaluation
            let rule_used_op = operations.iter().find(|op| {
                matches!(&op.kind, OperationKind::RuleUsed { rule_ref: ref r, .. } if r == rule_ref)
            }).ok_or_else(|| {
                LemmaError::Engine(format!(
                    "No RuleUsed operation found for rule reference {:?}",
                    rule_ref.reference
                ))
            })?;

            // Extract the path and result that were stored during evaluation
            let (rule_path, result) = if let OperationKind::RuleUsed {
                rule_path, result, ..
            } = &rule_used_op.kind
            {
                (rule_path.clone(), result.clone())
            } else {
                unreachable!()
            };

            // Get the proof that was already built during evaluation
            // If there's no proof, the rule failed before evaluation started (e.g., missing facts)
            // In that case, we cannot expand the rule reference
            let existing_proof = rule_proofs.get(&rule_path).ok_or_else(|| {
                LemmaError::Engine(format!(
                    "Proof not found for rule path: {rule_path} (rule failed before evaluation started)"
                ))
            })?;

            Ok(ProofNode::RuleReference {
                rule_path,
                result,
                source_location: existing_proof.source.clone(),
                expansion: Box::new(existing_proof.tree.clone()),
            })
        }

        ExpressionKind::Arithmetic(left, _, right) | ExpressionKind::Comparison(left, _, right) => {
            // Recursively build operand proofs
            let left_proof = build_proof_node_from_expression(
                left,
                operations,
                doc,
                all_documents,
                rule_proofs,
                sources,
            )?;
            let right_proof = build_proof_node_from_expression(
                right,
                operations,
                doc,
                all_documents,
                rule_proofs,
                sources,
            )?;

            // Check if either operand is a Veto or has a Veto result - if so, the computation was short-circuited
            if let ProofNode::Veto { .. } = left_proof {
                return Ok(left_proof);
            }
            if let ProofNode::RuleReference {
                result: OperationResult::Veto(msg),
                source_location,
                ..
            } = &left_proof
            {
                return Ok(ProofNode::Veto {
                    message: msg.clone(),
                    source_location: source_location.clone(),
                });
            }
            if let ProofNode::Veto { .. } = right_proof {
                return Ok(right_proof);
            }
            if let ProofNode::RuleReference {
                result: OperationResult::Veto(msg),
                source_location,
                ..
            } = &right_proof
            {
                return Ok(ProofNode::Veto {
                    message: msg.clone(),
                    source_location: source_location.clone(),
                });
            }

            // Get the computation result from operations
            if let Some(op) = op {
                if let OperationKind::Computation { kind, result, .. } = &op.kind {
                    let original_expression = expr.get_source_text(sources).ok_or_else(|| {
                        LemmaError::Engine(format!(
                            "Could not extract source text for expression {:?}",
                            expr.id
                        ))
                    })?;
                    let expression = build_substituted_expression_string(
                        expr,
                        &[left_proof.clone(), right_proof.clone()],
                    )?;
                    return Ok(ProofNode::Computation {
                        kind: kind.clone(),
                        original_expression,
                        expression,
                        result: result.clone(),
                        source_location,
                        operands: vec![left_proof, right_proof],
                    });
                }
            }
            Err(LemmaError::Engine(format!(
                "No Computation operation found for expression {:?}",
                expr.id
            )))
        }

        ExpressionKind::MathematicalComputation(_, operand) => {
            let operand_proof = build_proof_node_from_expression(
                operand,
                operations,
                doc,
                all_documents,
                rule_proofs,
                sources,
            )?;

            if let Some(op) = op {
                if let OperationKind::Computation { kind, result, .. } = &op.kind {
                    let original_expression = expr.get_source_text(sources).ok_or_else(|| {
                        LemmaError::Engine(format!(
                            "Could not extract source text for expression {:?}",
                            expr.id
                        ))
                    })?;
                    let expression = build_substituted_expression_string(
                        expr,
                        std::slice::from_ref(&operand_proof),
                    )?;
                    return Ok(ProofNode::Computation {
                        kind: kind.clone(),
                        original_expression,
                        expression,
                        result: result.clone(),
                        source_location,
                        operands: vec![operand_proof],
                    });
                }
            }
            Err(LemmaError::Engine(format!(
                "No Computation operation found for mathematical operation {:?}",
                expr.id
            )))
        }

        ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
            // For logical operations, we still build proof nodes for sub-expressions
            let left_proof = build_proof_node_from_expression(
                left,
                operations,
                doc,
                all_documents,
                rule_proofs,
                sources,
            )?;
            let right_proof = build_proof_node_from_expression(
                right,
                operations,
                doc,
                all_documents,
                rule_proofs,
                sources,
            )?;

            // Extract boolean values from the proof nodes
            let left_val = extract_literal_value(&left_proof)?;
            let right_val = extract_literal_value(&right_proof)?;

            let result = match (left_val, right_val) {
                (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => {
                    let bool_result = match &expr.kind {
                        ExpressionKind::LogicalAnd(_, _) => l.into() && r.into(),
                        ExpressionKind::LogicalOr(_, _) => l.into() || r.into(),
                        _ => unreachable!(),
                    };
                    LiteralValue::Boolean(bool_result.into())
                }
                _ => {
                    return Err(LemmaError::Engine(
                        "Logical operations require boolean operands".to_string(),
                    ))
                }
            };

            // Create a synthetic computation node for logical operations
            let kind = match &expr.kind {
                ExpressionKind::LogicalAnd(_, _) => {
                    ComputationKind::Logical(crate::LogicalComputation::And)
                }
                ExpressionKind::LogicalOr(_, _) => {
                    ComputationKind::Logical(crate::LogicalComputation::Or)
                }
                _ => unreachable!(),
            };

            let original_expression = expr.get_source_text(sources).ok_or_else(|| {
                LemmaError::Engine(format!(
                    "Could not extract source text for expression {:?}",
                    expr.id
                ))
            })?;
            let expression = build_substituted_expression_string(
                expr,
                &[left_proof.clone(), right_proof.clone()],
            )?;
            Ok(ProofNode::Computation {
                kind,
                original_expression,
                expression,
                result,
                source_location,
                operands: vec![left_proof, right_proof],
            })
        }

        ExpressionKind::LogicalNegation(inner, _) => {
            let inner_proof = build_proof_node_from_expression(
                inner,
                operations,
                doc,
                all_documents,
                rule_proofs,
                sources,
            )?;

            // Extract the boolean value and negate it
            let inner_val = extract_literal_value(&inner_proof)?;
            let negated_result = match inner_val {
                LiteralValue::Boolean(b) => LiteralValue::Boolean(!b),
                _ => {
                    return Err(LemmaError::Engine(
                        "Logical negation requires boolean operand".to_string(),
                    ))
                }
            };

            // Create a Computation node to show the negation operation
            let original_expression = expr.get_source_text(sources).ok_or_else(|| {
                LemmaError::Engine(format!(
                    "Could not extract source text for expression {:?}",
                    expr.id
                ))
            })?;
            let expression =
                build_substituted_expression_string(expr, std::slice::from_ref(&inner_proof))?;
            Ok(ProofNode::Computation {
                kind: ComputationKind::Logical(crate::LogicalComputation::Not),
                original_expression,
                expression,
                result: negated_result,
                source_location,
                operands: vec![inner_proof],
            })
        }

        ExpressionKind::UnitConversion(inner, _) => {
            let inner_proof = build_proof_node_from_expression(
                inner,
                operations,
                doc,
                all_documents,
                rule_proofs,
                sources,
            )?;

            // For now, just return the inner proof - these don't have separate operation records
            Ok(inner_proof)
        }

        ExpressionKind::Veto(_) | ExpressionKind::FactHasAnyValue(_) => {
            // These should be handled elsewhere
            Err(LemmaError::Engine(format!(
                "Expression kind {:?} not supported in proof building",
                expr.kind
            )))
        }
    }
}

fn build_substituted_expression_string(
    expr: &Expression,
    operands: &[ProofNode],
) -> LemmaResult<String> {
    match &expr.kind {
        ExpressionKind::Literal(lit) => Ok(lit.display_value()),

        ExpressionKind::FactReference(fact_ref) => Ok(fact_ref.to_string()),

        ExpressionKind::RuleReference(rule_ref) => Ok(rule_ref.to_string()),

        ExpressionKind::Arithmetic(left, op, right) => {
            if operands.len() >= 2 {
                let left_val = extract_expression_or_value(&operands[0])?;
                let right_val = extract_expression_or_value(&operands[1])?;
                Ok(format!("{left_val} {op} {right_val}"))
            } else {
                // Fallback to recursive build
                let left_str = build_expr_string_recursive(left)?;
                let right_str = build_expr_string_recursive(right)?;
                Ok(format!("{left_str} {op} {right_str}"))
            }
        }

        ExpressionKind::Comparison(left, op, right) => {
            if operands.len() >= 2 {
                let left_val = extract_expression_or_value(&operands[0])?;
                let right_val = extract_expression_or_value(&operands[1])?;
                Ok(format!("{left_val} {op} {right_val}"))
            } else {
                let left_str = build_expr_string_recursive(left)?;
                let right_str = build_expr_string_recursive(right)?;
                Ok(format!("{left_str} {op} {right_str}"))
            }
        }

        ExpressionKind::MathematicalComputation(op, operand) => {
            if !operands.is_empty() {
                let operand_val = extract_display_value(&operands[0])?;
                Ok(format!("{op}({operand_val})"))
            } else {
                let operand_str = build_expr_string_recursive(operand)?;
                Ok(format!("{op}({operand_str})"))
            }
        }

        ExpressionKind::LogicalAnd(left, right) => {
            if operands.len() >= 2 {
                // For nested logical ops, use the expression if it's a Computation, otherwise use the value
                let left_val = extract_expression_or_value(&operands[0])?;
                let right_val = extract_expression_or_value(&operands[1])?;
                Ok(format!("{left_val} and {right_val}"))
            } else {
                let left_str = build_expr_string_recursive(left)?;
                let right_str = build_expr_string_recursive(right)?;
                Ok(format!("{left_str} and {right_str}"))
            }
        }

        ExpressionKind::LogicalOr(left, right) => {
            if operands.len() >= 2 {
                let left_val = extract_expression_or_value(&operands[0])?;
                let right_val = extract_expression_or_value(&operands[1])?;
                Ok(format!("{left_val} or {right_val}"))
            } else {
                let left_str = build_expr_string_recursive(left)?;
                let right_str = build_expr_string_recursive(right)?;
                Ok(format!("{left_str} or {right_str}"))
            }
        }

        ExpressionKind::LogicalNegation(inner, _) => {
            if !operands.is_empty() {
                let inner_val = extract_expression_or_value(&operands[0])?;
                Ok(format!("not {inner_val}"))
            } else {
                let inner_str = build_expr_string_recursive(inner)?;
                Ok(format!("not {inner_str}"))
            }
        }

        _ => Err(LemmaError::Engine(format!(
            "Expression kind not yet supported in substitution: {:?}",
            expr.kind
        ))),
    }
}

/// Extract the display value from a ProofNode
fn extract_display_value(node: &ProofNode) -> LemmaResult<String> {
    match node {
        ProofNode::Value { value, .. } => Ok(value.display_value()),
        ProofNode::Computation { result, .. } => Ok(result.display_value()),
        ProofNode::RuleReference { result, .. } => match result {
            OperationResult::Value(v) => Ok(v.display_value()),
            OperationResult::Veto(msg) => {
                Ok(format!("veto({})", msg.as_ref().unwrap_or(&"".to_string())))
            }
        },
        _ => Err(LemmaError::Engine(format!(
            "Cannot extract display value from proof node type: {:?}",
            std::mem::discriminant(node)
        ))),
    }
}

/// Extract the expression string (for nested computations) or value (for leaves)
fn extract_expression_or_value(node: &ProofNode) -> LemmaResult<String> {
    match node {
        ProofNode::Value { value, .. } => Ok(value.display_value()),
        ProofNode::Computation { expression, .. } => Ok(expression.clone()), // Use the full expression
        ProofNode::RuleReference { result, .. } => match result {
            OperationResult::Value(v) => Ok(v.display_value()),
            OperationResult::Veto(msg) => {
                Ok(format!("veto({})", msg.as_ref().unwrap_or(&"".to_string())))
            }
        },
        _ => Ok("<value>".to_string()),
    }
}

/// Helper function to extract a literal value from a proof node
fn extract_literal_value(node: &ProofNode) -> LemmaResult<&LiteralValue> {
    match node {
        ProofNode::Value { value, .. } => Ok(value),
        ProofNode::Computation { result, .. } => Ok(result),
        ProofNode::RuleReference { result, .. } => match result {
            OperationResult::Value(v) => Ok(v),
            OperationResult::Veto(_) => Err(LemmaError::Engine(
                "Cannot extract value from veto".to_string(),
            )),
        },
        _ => Err(LemmaError::Engine(
            "Cannot extract literal value from proof node".to_string(),
        )),
    }
}

/// Build expression string recursively from AST (fallback)
fn build_expr_string_recursive(expr: &Expression) -> LemmaResult<String> {
    match &expr.kind {
        ExpressionKind::Literal(lit) => Ok(lit.display_value()),
        ExpressionKind::FactReference(fact_ref) => Ok(fact_ref.to_string()),
        ExpressionKind::RuleReference(rule_ref) => Ok(rule_ref.to_string()),
        ExpressionKind::Arithmetic(left, op, right) => {
            let left_str = build_expr_string_recursive(left)?;
            let right_str = build_expr_string_recursive(right)?;
            Ok(format!("{left_str} {op} {right_str}"))
        }
        ExpressionKind::Comparison(left, op, right) => {
            let left_str = build_expr_string_recursive(left)?;
            let right_str = build_expr_string_recursive(right)?;
            Ok(format!("{left_str} {op} {right_str}"))
        }
        ExpressionKind::LogicalAnd(left, right) => {
            let left_str = build_expr_string_recursive(left)?;
            let right_str = build_expr_string_recursive(right)?;
            Ok(format!("{left_str} and {right_str}"))
        }
        ExpressionKind::LogicalOr(left, right) => {
            let left_str = build_expr_string_recursive(left)?;
            let right_str = build_expr_string_recursive(right)?;
            Ok(format!("{left_str} or {right_str}"))
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            let inner_str = build_expr_string_recursive(inner)?;
            Ok(format!("not {inner_str}"))
        }
        _ => Err(LemmaError::Engine(format!(
            "Expression kind not supported in build_expr_string_recursive: {:?}",
            expr.kind
        ))),
    }
}
