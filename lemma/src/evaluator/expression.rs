//! Expression evaluation
//!
//! Recursively evaluates expressions to produce literal values.

use super::context::EvaluationContext;
use crate::{
    ast::Span, ArithmeticOperation, Expression, ExpressionKind, LemmaError, LiteralValue,
    MathematicalOperator, OperationRecord, OperationResult,
};
use rust_decimal::Decimal;
use std::sync::Arc;

/// Evaluate an expression to produce an operation result
///
/// This is the core of the evaluator - recursively processes expressions
/// and records operations for every step.
pub fn evaluate_expression(
    expr: &Expression,
    context: &mut EvaluationContext,
) -> Result<OperationResult, LemmaError> {
    match &expr.kind {
        ExpressionKind::Literal(lit) => {
            // Literals evaluate to themselves
            Ok(OperationResult::Value(lit.clone()))
        }

        ExpressionKind::FactReference(fact_ref) => {
            // Look up fact in context
            let fact_name = fact_ref.reference.join(".");
            let value = context
                .facts
                .get(&fact_name)
                .ok_or_else(|| LemmaError::Engine(format!("Missing fact: {}", fact_name)))?;

            // Record operation
            context.operations.push(OperationRecord::FactUsed {
                name: fact_name,
                value: value.clone(),
            });

            Ok(OperationResult::Value(value.clone()))
        }

        ExpressionKind::RuleReference(rule_ref) => {
            // Look up already-computed rule result
            // Topological sort ensures this rule was computed before us
            let rule_name = rule_ref.reference.join(".");

            // Check if rule has a result
            if let Some(result) = context.rule_results.get(&rule_name) {
                match result {
                    OperationResult::Veto(msg) => {
                        // Rule was vetoed - the veto applies to this rule too
                        return Ok(OperationResult::Veto(msg.clone()));
                    }
                    OperationResult::Value(value) => {
                        // Record operation
                        context.operations.push(OperationRecord::RuleUsed {
                            name: rule_name,
                            value: value.clone(),
                        });
                        return Ok(OperationResult::Value(value.clone()));
                    }
                }
            }

            // Rule not computed yet
            Err(LemmaError::Engine(format!(
                "Rule {} not yet computed",
                rule_name
            )))
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_result = evaluate_expression(left, context)?;
            let right_result = evaluate_expression(right, context)?;

            // If either operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = left_result {
                return Ok(OperationResult::Veto(msg));
            }
            if let OperationResult::Veto(msg) = right_result {
                return Ok(OperationResult::Veto(msg));
            }

            let left_val = left_result.value().unwrap();
            let right_val = right_result.value().unwrap();

            // Convert Engine errors to Runtime errors with source location
            let result = super::operations::arithmetic_operation(left_val, op, right_val)
                .map_err(|e| convert_engine_error_to_runtime(e, expr, context))?;

            // Record operation
            let op_name = match op {
                ArithmeticOperation::Add => "add",
                ArithmeticOperation::Subtract => "subtract",
                ArithmeticOperation::Multiply => "multiply",
                ArithmeticOperation::Divide => "divide",
                ArithmeticOperation::Modulo => "modulo",
                ArithmeticOperation::Power => "power",
            };

            context.operations.push(OperationRecord::OperationExecuted {
                operation: op_name.to_string(),
                inputs: vec![left_val.clone(), right_val.clone()],
                result: result.clone(),
                unless_clause_index: None,
            });

            Ok(OperationResult::Value(result))
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_result = evaluate_expression(left, context)?;
            let right_result = evaluate_expression(right, context)?;

            // If either operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = left_result {
                return Ok(OperationResult::Veto(msg));
            }
            if let OperationResult::Veto(msg) = right_result {
                return Ok(OperationResult::Veto(msg));
            }

            let left_val = left_result.value().unwrap();
            let right_val = right_result.value().unwrap();

            let result = super::operations::comparison_operation(left_val, op, right_val)?;

            // Record operation
            let op_name = match op {
                crate::ComparisonOperator::GreaterThan => "greater_than",
                crate::ComparisonOperator::LessThan => "less_than",
                crate::ComparisonOperator::GreaterThanOrEqual => "greater_than_or_equal",
                crate::ComparisonOperator::LessThanOrEqual => "less_than_or_equal",
                crate::ComparisonOperator::Equal => "equal",
                crate::ComparisonOperator::NotEqual => "not_equal",
                crate::ComparisonOperator::Is => "is",
                crate::ComparisonOperator::IsNot => "is_not",
            };

            context.operations.push(OperationRecord::OperationExecuted {
                operation: op_name.to_string(),
                inputs: vec![left_val.clone(), right_val.clone()],
                result: LiteralValue::Boolean(result),
                unless_clause_index: None,
            });

            Ok(OperationResult::Value(LiteralValue::Boolean(result)))
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_result = evaluate_expression(left, context)?;
            let right_result = evaluate_expression(right, context)?;

            // If either operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = left_result {
                return Ok(OperationResult::Veto(msg));
            }
            if let OperationResult::Veto(msg) = right_result {
                return Ok(OperationResult::Veto(msg));
            }

            let left_val = left_result.value().unwrap();
            let right_val = right_result.value().unwrap();

            match (left_val, right_val) {
                (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => {
                    // No operation record for logical operations - only record sub-expressions
                    Ok(OperationResult::Value(LiteralValue::Boolean(*l && *r)))
                }
                _ => Err(LemmaError::Engine(
                    "Logical AND requires boolean operands".to_string(),
                )),
            }
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_result = evaluate_expression(left, context)?;
            let right_result = evaluate_expression(right, context)?;

            // If either operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = left_result {
                return Ok(OperationResult::Veto(msg));
            }
            if let OperationResult::Veto(msg) = right_result {
                return Ok(OperationResult::Veto(msg));
            }

            let left_val = left_result.value().unwrap();
            let right_val = right_result.value().unwrap();

            match (left_val, right_val) {
                (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => {
                    // No operation record for logical operations - only record sub-expressions
                    Ok(OperationResult::Value(LiteralValue::Boolean(*l || *r)))
                }
                _ => Err(LemmaError::Engine(
                    "Logical OR requires boolean operands".to_string(),
                )),
            }
        }

        ExpressionKind::LogicalNegation(inner, _negation_type) => {
            let result = evaluate_expression(inner, context)?;

            // If the operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = result {
                return Ok(OperationResult::Veto(msg));
            }

            let value = result.value().unwrap();

            match value {
                LiteralValue::Boolean(b) => Ok(OperationResult::Value(LiteralValue::Boolean(!b))),
                _ => Err(LemmaError::Engine(
                    "Logical NOT requires boolean operand".to_string(),
                )),
            }
        }

        ExpressionKind::UnitConversion(value_expr, target) => {
            let result = evaluate_expression(value_expr, context)?;

            // If the operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = result {
                return Ok(OperationResult::Veto(msg));
            }

            let value = result.value().unwrap();
            let converted = super::units::convert_unit(value, target)?;
            Ok(OperationResult::Value(converted))
        }

        ExpressionKind::MathematicalOperator(op, operand) => {
            evaluate_mathematical_operator(op, operand, context)
        }

        ExpressionKind::Veto(veto_expr) => Ok(OperationResult::Veto(veto_expr.message.clone())),

        ExpressionKind::FactHasAnyValue(fact_ref) => {
            // Check if fact exists and has a value
            let fact_name = fact_ref.reference.join(".");
            let has_value = context.facts.contains_key(&fact_name);
            Ok(OperationResult::Value(LiteralValue::Boolean(has_value)))
        }
    }
}

/// Evaluate a mathematical operator (sqrt, sin, cos, etc.)
fn evaluate_mathematical_operator(
    op: &MathematicalOperator,
    operand: &Expression,
    context: &mut EvaluationContext,
) -> Result<OperationResult, LemmaError> {
    let result = evaluate_expression(operand, context)?;

    // If the operand is vetoed, propagate the veto
    if let OperationResult::Veto(msg) = result {
        return Ok(OperationResult::Veto(msg));
    }

    let value = result.value().unwrap();

    match value {
        LiteralValue::Number(n) => {
            use rust_decimal::prelude::ToPrimitive;
            let float_val = n.to_f64().ok_or_else(|| {
                LemmaError::Engine("Cannot convert to float for mathematical operation".to_string())
            })?;

            let math_result = match op {
                MathematicalOperator::Sqrt => float_val.sqrt(),
                MathematicalOperator::Sin => float_val.sin(),
                MathematicalOperator::Cos => float_val.cos(),
                MathematicalOperator::Tan => float_val.tan(),
                MathematicalOperator::Asin => float_val.asin(),
                MathematicalOperator::Acos => float_val.acos(),
                MathematicalOperator::Atan => float_val.atan(),
                MathematicalOperator::Log => float_val.ln(),
                MathematicalOperator::Exp => float_val.exp(),
            };

            let decimal_result = Decimal::from_f64_retain(math_result).ok_or_else(|| {
                LemmaError::Engine(
                    "Mathematical operation result cannot be represented".to_string(),
                )
            })?;

            Ok(OperationResult::Value(LiteralValue::Number(decimal_result)))
        }
        _ => Err(LemmaError::Engine(
            "Mathematical operators require number operands".to_string(),
        )),
    }
}

/// Convert an Engine error to a Runtime error with proper source location
///
/// This is used to add span information to errors that occur during expression evaluation.
fn convert_engine_error_to_runtime(
    error: LemmaError,
    expr: &Expression,
    context: &EvaluationContext,
) -> LemmaError {
    match error {
        LemmaError::Engine(msg) => {
            let span = expr.span.clone().unwrap_or(Span {
                start: 0,
                end: 0,
                line: 0,
                col: 0,
            });

            let source_id = context
                .current_doc
                .source
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "<input>".to_string());

            let source_text: Arc<str> = context
                .sources
                .get(&source_id)
                .map(|s| Arc::from(s.as_str()))
                .unwrap_or_else(|| Arc::from(""));

            let suggestion = if msg.contains("division") || msg.contains("zero") {
                Some(
                    "Consider using an 'unless' clause to guard against division by zero"
                        .to_string(),
                )
            } else if msg.contains("type") || msg.contains("mismatch") {
                Some("Check that operands have compatible types".to_string())
            } else {
                None
            };

            LemmaError::Runtime(Box::new(crate::error::ErrorDetails {
                message: msg,
                span,
                source_id,
                source_text,
                doc_name: context.current_doc.name.clone(),
                doc_start_line: context.current_doc.start_line,
                suggestion,
            }))
        }
        other => other,
    }
}
