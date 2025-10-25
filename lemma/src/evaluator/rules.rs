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
/// For cross-document rules, pass the path prefix via `fact_prefix` to qualify
/// fact lookups. For local rules, pass an empty slice.
pub fn evaluate_rule(
    rule: &LemmaRule,
    context: &mut EvaluationContext,
    fact_prefix: &[String],
) -> Result<OperationResult, LemmaError> {
    use crate::OperationRecord;

    // Evaluate unless clauses in reverse order (last matching wins)
    for (index, unless_clause) in rule.unless_clauses.iter().enumerate().rev() {
        let condition_result = evaluate_expression(&unless_clause.condition, context, fact_prefix)?;

        // If condition is vetoed, the veto applies to this rule
        if let OperationResult::Veto(msg) = condition_result {
            return Ok(OperationResult::Veto(msg));
        }

        let condition_value = condition_result.value().unwrap();
        let matched = match condition_value {
            crate::LiteralValue::Boolean(b) => b,
            _ => {
                return Err(LemmaError::Engine(
                    "Unless condition must evaluate to boolean".to_string(),
                ));
            }
        };

        if *matched {
            let result = evaluate_expression(&unless_clause.result, context, fact_prefix)?;

            // If result is vetoed, the veto applies to this rule
            if let OperationResult::Veto(msg) = result {
                return Ok(OperationResult::Veto(msg));
            }

            let result_value = result.value().unwrap().clone();
            context
                .operations
                .push(OperationRecord::UnlessClauseEvaluated {
                    index,
                    matched: true,
                    result_if_matched: Some(result_value.clone()),
                });
            context.operations.push(OperationRecord::FinalResult {
                value: result_value.clone(),
            });
            return Ok(OperationResult::Value(result_value));
        } else {
            context
                .operations
                .push(OperationRecord::UnlessClauseEvaluated {
                    index,
                    matched: false,
                    result_if_matched: None,
                });
        }
    }

    // No unless clause matched - evaluate default expression
    let default_result = evaluate_expression(&rule.expression, context, fact_prefix)?;

    // If default is vetoed, the veto applies to this rule
    if let OperationResult::Veto(msg) = default_result {
        return Ok(OperationResult::Veto(msg));
    }

    let default_value = default_result.value().unwrap().clone();
    context.operations.push(OperationRecord::DefaultValue {
        value: default_value.clone(),
    });
    context.operations.push(OperationRecord::FinalResult {
        value: default_value.clone(),
    });
    Ok(OperationResult::Value(default_value))
}
