//! Rule evaluation
//!
//! Handles evaluation of rules including default expressions and unless clauses.

use super::context::EvaluationContext;
use super::expression::evaluate_expression;
use crate::{LemmaError, LemmaResult, LemmaRule, LiteralValue};

/// Evaluate a rule to produce its final value
///
/// Unless clauses are evaluated in reverse order (last matching wins).
/// If no unless clause matches, evaluate the default expression.
pub fn evaluate_rule(
    rule: &LemmaRule,
    context: &mut EvaluationContext,
) -> LemmaResult<LiteralValue> {
    use crate::TraceStep;

    // Evaluate unless clauses in reverse order (last matching wins)
    for (index, unless_clause) in rule.unless_clauses.iter().enumerate().rev() {
        let condition_result = evaluate_expression(&unless_clause.condition, context)?;

        let matched = match condition_result {
            LiteralValue::Boolean(b) => b,
            _ => {
                return Err(LemmaError::Engine(
                    "Unless condition must evaluate to boolean".to_string(),
                ));
            }
        };

        if matched {
            let result = evaluate_expression(&unless_clause.result, context)?;
            context.trace.push(TraceStep::UnlessClauseEvaluated {
                index,
                matched: true,
                result_if_matched: Some(result.clone()),
            });
            context.trace.push(TraceStep::FinalResult {
                value: result.clone(),
            });
            return Ok(result);
        } else {
            context.trace.push(TraceStep::UnlessClauseEvaluated {
                index,
                matched: false,
                result_if_matched: None,
            });
        }
    }

    // No unless clause matched - evaluate default expression
    let default_value = evaluate_expression(&rule.expression, context)?;
    context.trace.push(TraceStep::DefaultValue {
        value: default_value.clone(),
    });
    context.trace.push(TraceStep::FinalResult {
        value: default_value.clone(),
    });
    Ok(default_value)
}
