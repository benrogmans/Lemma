//! Rule evaluation
//!
//! Handles evaluation of rules including default expressions and unless clauses.

use super::context::EvaluationContext;
use super::expression::evaluate_expression;
use crate::{LemmaError, LemmaRule, OperationResult};

/// Evaluate a rule to produce its final result and proof
///
/// Unless clauses are evaluated in reverse order (last matching wins).
/// If no unless clause matches, evaluate the default expression.
///
/// When evaluating a rule from a document referenced by a fact, pass the fact path
/// via `fact_prefix` to qualify fact lookups. For local rules, pass an empty slice.
///
/// The `rule_path` should include segments for cross-document rules.
///
/// Returns both the operation result and a proof showing how it was derived.
pub fn evaluate_rule(
    rule: &LemmaRule,
    rule_path: crate::RulePath,
    rule_doc: &crate::LemmaDoc,
    context: &mut EvaluationContext,
    fact_prefix: &[String],
) -> Result<(OperationResult, crate::proof::Proof), LemmaError> {
    // Evaluate unless clauses in reverse order (last matching wins)
    for (index, unless_clause) in rule.unless_clauses.iter().enumerate().rev() {
        // Extract expression text from source
        let condition_expr = unless_clause.condition.get_source_text(context.sources);
        let result_expr = unless_clause.result.get_source_text(context.sources);

        let condition_result =
            evaluate_expression(&unless_clause.condition, rule_doc, context, fact_prefix)?;

        let matched = match condition_result {
            OperationResult::Veto(msg) => {
                // If condition is vetoed, the veto applies to this rule
                // Record the branch operation before building proof
                context.push_operation(
                    crate::OperationKind::RuleBranchEvaluated {
                        index: Some(index),
                        matched: true,
                        condition_expr,
                        result_expr,
                        result_value: Some(OperationResult::Veto(msg.clone())),
                    },
                    unless_clause.condition.id,
                );
                let proof = build_proof_for_rule(
                    rule,
                    rule_path.clone(),
                    ProofBuildContext {
                        operations: &context.operations,
                        rule_doc,
                        all_documents: context.all_documents,
                        rule_proofs: &context.rule_proofs,
                        sources: context.sources,
                    },
                    OperationResult::Veto(msg.clone()),
                )?;
                return Ok((OperationResult::Veto(msg), proof));
            }
            OperationResult::Value(crate::LiteralValue::Boolean(b)) => b.clone(),
            OperationResult::Value(_) => {
                return Err(LemmaError::Engine(
                    "Unless condition must evaluate to boolean".to_string(),
                ));
            }
        };

        if matched.into() {
            let result =
                evaluate_expression(&unless_clause.result, rule_doc, context, fact_prefix)?;

            // If result is vetoed, record the branch and return the veto
            if let OperationResult::Veto(msg) = result {
                context.push_operation(
                    crate::OperationKind::RuleBranchEvaluated {
                        index: Some(index),
                        matched: true,
                        condition_expr,
                        result_expr,
                        result_value: Some(OperationResult::Veto(msg.clone())),
                    },
                    unless_clause.result.id,
                );

                // Build proof for veto result
                let proof = build_proof_for_rule(
                    rule,
                    rule_path.clone(),
                    ProofBuildContext {
                        operations: &context.operations,
                        rule_doc,
                        all_documents: context.all_documents,
                        rule_proofs: &context.rule_proofs,
                        sources: context.sources,
                    },
                    OperationResult::Veto(msg.clone()),
                )?;

                return Ok((OperationResult::Veto(msg), proof));
            }

            let result_value = match result {
                OperationResult::Value(v) => v,
                OperationResult::Veto(_) => {
                    unreachable!("Veto case already handled above")
                }
            };
            context.push_operation(
                crate::OperationKind::RuleBranchEvaluated {
                    index: Some(index),
                    matched: true,
                    condition_expr,
                    result_expr,
                    result_value: Some(OperationResult::Value(result_value.clone())),
                },
                unless_clause.result.id,
            );

            // Build proof using operations collected for this rule
            let proof = build_proof_for_rule(
                rule,
                rule_path.clone(),
                ProofBuildContext {
                    operations: &context.operations,
                    rule_doc,
                    all_documents: context.all_documents,
                    rule_proofs: &context.rule_proofs,
                    sources: context.sources,
                },
                OperationResult::Value(result_value.clone()),
            )?;

            return Ok((OperationResult::Value(result_value), proof));
        } else {
            context.push_operation(
                crate::OperationKind::RuleBranchEvaluated {
                    index: Some(index),
                    matched: false,
                    condition_expr,
                    result_expr,
                    result_value: None,
                },
                unless_clause.condition.id,
            );
        }
    }

    // No unless clause matched - evaluate default expression
    let default_expr = rule.expression.get_source_text(context.sources);
    let default_result = evaluate_expression(&rule.expression, rule_doc, context, fact_prefix)?;

    // If default is vetoed, record the branch and return the veto
    if let OperationResult::Veto(msg) = default_result {
        context.push_operation(
            crate::OperationKind::RuleBranchEvaluated {
                index: None,
                matched: true,
                condition_expr: None,
                result_expr: default_expr,
                result_value: Some(OperationResult::Veto(msg.clone())),
            },
            rule.expression.id,
        );

        // Build proof for veto result
        let proof = build_proof_for_rule(
            rule,
            rule_path.clone(),
            ProofBuildContext {
                operations: &context.operations,
                rule_doc,
                all_documents: context.all_documents,
                rule_proofs: &context.rule_proofs,
                sources: context.sources,
            },
            OperationResult::Veto(msg.clone()),
        )?;

        return Ok((OperationResult::Veto(msg), proof));
    }

    let default_value = match default_result {
        OperationResult::Value(v) => v,
        OperationResult::Veto(_) => {
            unreachable!("Veto case already handled above")
        }
    };
    context.push_operation(
        crate::OperationKind::RuleBranchEvaluated {
            index: None,
            matched: true,
            condition_expr: None,
            result_expr: default_expr,
            result_value: Some(OperationResult::Value(default_value.clone())),
        },
        rule.expression.id,
    );

    // Build proof for successful result
    let proof = build_proof_for_rule(
        rule,
        rule_path,
        ProofBuildContext {
            operations: &context.operations,
            rule_doc,
            all_documents: context.all_documents,
            rule_proofs: &context.rule_proofs,
            sources: context.sources,
        },
        OperationResult::Value(default_value.clone()),
    )?;

    Ok((OperationResult::Value(default_value), proof))
}

/// Context for building proofs
struct ProofBuildContext<'a> {
    operations: &'a [crate::OperationRecord],
    rule_doc: &'a crate::LemmaDoc,
    all_documents: &'a std::collections::HashMap<String, crate::LemmaDoc>,
    rule_proofs: &'a std::collections::HashMap<crate::RulePath, crate::proof::Proof>,
    sources: &'a std::collections::HashMap<String, String>,
}

/// Helper function to build proof for a rule using its operations
///
/// Note: This is a simple wrapper that constructs the Proof struct.
/// The actual rule_path (with segments for cross-document rules) is provided by the caller.
fn build_proof_for_rule(
    rule: &LemmaRule,
    rule_path: crate::RulePath,
    context: ProofBuildContext<'_>,
    result: OperationResult,
) -> Result<crate::proof::Proof, LemmaError> {
    let tree = crate::proof::build_proof_node_from_rule(
        rule,
        context.operations,
        context.rule_doc,
        context.all_documents,
        context.rule_proofs,
        context.sources,
    )?;

    Ok(crate::proof::Proof {
        rule_path,
        source: rule.source_location.clone(),
        result,
        tree,
    })
}
