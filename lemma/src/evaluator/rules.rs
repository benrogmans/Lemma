//! Rule evaluation
//!
//! Handles evaluation of rules including default expressions and unless clauses.

use super::context::EvaluationContext;
use super::expression::evaluate_expression;
use crate::{LemmaError, LemmaResult, LemmaRule, LiteralValue, UnlessClause};

/// Evaluate a rule to produce its final value
///
/// Rules have a default expression and optional unless clauses.
/// Unless clauses are evaluated in reverse order (last matching wins).
pub fn evaluate_rule(
    rule: &LemmaRule,
    context: &mut EvaluationContext,
) -> LemmaResult<LiteralValue> {
    let default_value = evaluate_expression(&rule.expression, context)?;

    if rule.unless_clauses.is_empty() {
        // Trace: using default value
        use crate::TraceStep;
        context.trace.push(TraceStep::DefaultValue {
            value: default_value.clone(),
        });
        context.trace.push(TraceStep::FinalResult {
            value: default_value.clone(),
        });
        return Ok(default_value);
    }

    let result = evaluate_unless_clauses(&rule.unless_clauses, default_value.clone(), context)?;

    // Trace: final result
    use crate::TraceStep;
    context.trace.push(TraceStep::FinalResult {
        value: result.clone(),
    });

    Ok(result)
}

/// Evaluate unless clauses in REVERSE order (last matching wins)
///
/// Iterate from the END, return on FIRST match.
/// Trace ALL clauses for complete observability.
fn evaluate_unless_clauses(
    unless_clauses: &[UnlessClause],
    default_value: LiteralValue,
    context: &mut EvaluationContext,
) -> LemmaResult<LiteralValue> {
    use crate::TraceStep;

    // First pass: evaluate ALL unless clauses in FORWARD order for tracing
    let mut evaluations = Vec::new();
    for (index, unless_clause) in unless_clauses.iter().enumerate() {
        let condition_result = evaluate_expression(&unless_clause.condition, context)?;

        let (matched, result) = match condition_result {
            LiteralValue::Boolean(true) => {
                let result = evaluate_expression(&unless_clause.result, context)?;
                (true, Some(result))
            }
            LiteralValue::Boolean(false) => (false, None),
            _ => {
                return Err(LemmaError::Engine(
                    "Unless condition must evaluate to boolean".to_string(),
                ));
            }
        };

        // Trace EVERY unless clause evaluation (both true and false)
        context.trace.push(TraceStep::UnlessClauseEvaluated {
            index,
            matched,
            result_if_matched: result.clone(),
        });

        evaluations.push((matched, result));
    }

    // Second pass: find LAST matching clause (reverse iteration)
    for (matched, result) in evaluations.iter().rev() {
        if *matched {
            return Ok(result.clone().unwrap());
        }
    }

    // No unless clause matched, use default value
    context.trace.push(TraceStep::DefaultValue {
        value: default_value.clone(),
    });
    Ok(default_value)
}
