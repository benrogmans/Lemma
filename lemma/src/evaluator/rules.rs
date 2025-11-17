//! Rule evaluation
//!
//! Handles evaluation of rules including default expressions and unless clauses.

use super::context::EvaluationContext;
use super::expression::evaluate_expression;
use crate::{LemmaError, LemmaRule, OperationResult};

/// Evaluate a rule to produce its final result
///
/// Unless clauses are evaluated in reverse order (last matching wins).
/// If no unless clause matches, evaluate the default expression.
///
/// When evaluating a rule from a document referenced by a fact, pass the fact path
/// via `fact_prefix` to qualify fact lookups. For local rules, pass an empty slice.
pub fn evaluate_rule(
    rule: &LemmaRule,
    rule_doc: &crate::LemmaDoc,
    context: &mut EvaluationContext,
    fact_prefix: &[String],
) -> Result<OperationResult, LemmaError> {
    // Evaluate unless clauses in reverse order (last matching wins)
    for (index, unless_clause) in rule.unless_clauses.iter().enumerate().rev() {
        // Extract expression text from source
        let condition_expr = context.extract_expr_text(&unless_clause.condition, rule_doc);
        let result_expr = context.extract_expr_text(&unless_clause.result, rule_doc);

        let condition_result =
            evaluate_expression(&unless_clause.condition, rule_doc, context, fact_prefix)?;

        // If condition is vetoed, the veto applies to this rule
        if let OperationResult::Veto(msg) = condition_result {
            return Ok(OperationResult::Veto(msg));
        }

        let condition_value = condition_result.value().unwrap();
        let matched = match condition_value {
            crate::LiteralValue::Boolean(b) => *b,
            _ => {
                return Err(LemmaError::Engine(
                    "Unless condition must evaluate to boolean".to_string(),
                ));
            }
        };

        if matched {
            let result =
                evaluate_expression(&unless_clause.result, rule_doc, context, fact_prefix)?;

            // If result is vetoed, the veto applies to this rule
            if let OperationResult::Veto(msg) = result {
                return Ok(OperationResult::Veto(msg));
            }

            let result_value = result.value().unwrap().clone();
            context.push_operation(
                crate::OperationKind::RuleBranchEvaluated {
                    index: Some(index),
                    matched: true,
                    condition_expr,
                    result_expr,
                    result_value: Some(result_value.clone()),
                },
                unless_clause.result.id,
            );
            return Ok(OperationResult::Value(result_value));
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
    let default_expr = context.extract_expr_text(&rule.expression, rule_doc);
    let default_result = evaluate_expression(&rule.expression, rule_doc, context, fact_prefix)?;

    // If default is vetoed, the veto applies to this rule
    if let OperationResult::Veto(msg) = default_result {
        return Ok(OperationResult::Veto(msg));
    }

    let default_value = default_result.value().unwrap().clone();
    context.push_operation(
        crate::OperationKind::RuleBranchEvaluated {
            index: None,
            matched: true,
            condition_expr: None,
            result_expr: default_expr,
            result_value: Some(default_value.clone()),
        },
        rule.expression.id,
    );
    Ok(OperationResult::Value(default_value))
}
