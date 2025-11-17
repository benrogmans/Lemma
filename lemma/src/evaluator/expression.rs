//! Expression evaluation
//!
//! Recursively evaluates expressions to produce literal values.

use super::context::EvaluationContext;
use crate::{
    ComputationKind, Expression, ExpressionKind, FactReference, LemmaError, LiteralValue,
    MathematicalComputation, OperationRecord, OperationResult,
};
use rust_decimal::Decimal;
use std::sync::Arc;

/// Evaluate an expression to produce an operation result
///
/// This is the core of the evaluator - recursively processes expressions
/// and records operations for every step.
///
/// When evaluating a rule from a document referenced by a fact (e.g., `employee.some_rule?`
/// where `employee` is a fact with value `doc other_doc`), pass the fact path via `fact_prefix`
/// to qualify fact lookups within that rule. For local rules, pass an empty slice.
pub fn evaluate_expression(
    expr: &Expression,
    rule_doc: &crate::LemmaDoc,
    context: &mut EvaluationContext,
    fact_prefix: &[String],
) -> Result<OperationResult, LemmaError> {
    // Check timeout at the start of every expression evaluation
    context.check_timeout()?;

    match &expr.kind {
        ExpressionKind::Literal(lit) => {
            // Literals evaluate to themselves
            Ok(OperationResult::Value(lit.clone()))
        }

        ExpressionKind::FactReference(fact_ref) => {
            // Look up fact in context, prepending the prefix when evaluating a rule from a referenced document
            let lookup_ref = if !fact_prefix.is_empty() {
                // Evaluating a rule from a document referenced by a fact: prepend the fact path
                // E.g., if `employee` references `doc hr_doc` and we're evaluating `employee.salary?`,
                // fact references within that rule need the `employee` prefix
                let mut qualified_reference = fact_prefix.to_vec();
                qualified_reference.extend_from_slice(&fact_ref.reference);
                FactReference {
                    reference: qualified_reference,
                }
            } else {
                // Local rule: use fact reference as-is
                fact_ref.clone()
            };

            let value = context
                .facts
                .get(&lookup_ref)
                .ok_or_else(|| LemmaError::MissingFact(lookup_ref.clone()))?
                .clone();

            // Record operation
            context.push_operation(
                crate::OperationKind::FactUsed {
                    fact_ref: lookup_ref.clone(),
                    value: value.clone(),
                },
                expr.id,
            );

            Ok(OperationResult::Value(value))
        }
        ExpressionKind::RuleReference(rule_ref) => {
            // Look up already-computed rule result
            // Topological sort ensures this rule was computed before us
            let relative_rule_path = crate::RulePath::from_reference(
                &rule_ref.reference,
                rule_doc,
                context.all_documents,
            )?;

            // If evaluating a nested rule, prepend fact_prefix to create full path
            let lookup_path = if fact_prefix.is_empty() {
                relative_rule_path.clone()
            } else {
                // Build prefix segments by traversing the fact chain
                let mut prefix_segments = Vec::new();
                let mut current_doc = context.current_doc;

                for fact_name in fact_prefix {
                    // Find the fact in the current document
                    let fact = current_doc
                        .facts
                        .iter()
                        .find(|f| matches!(&f.fact_type, crate::FactType::Local(name) if name == fact_name))
                        .ok_or_else(|| {
                            crate::LemmaError::Engine(format!(
                                "Fact {} not found in document {}",
                                fact_name, current_doc.name
                            ))
                        })?;

                    // Get the target document name
                    let target_doc_name = match &fact.value {
                        crate::FactValue::DocumentReference(name) => name.clone(),
                        _ => {
                            return Err(crate::LemmaError::Engine(format!(
                                "Fact {} is not a document reference",
                                fact_name
                            )))
                        }
                    };

                    prefix_segments.push(crate::RulePathSegment {
                        fact: fact_name.clone(),
                        doc: target_doc_name.clone(),
                    });

                    // Move to the next document
                    current_doc = context.all_documents.get(&target_doc_name).ok_or_else(|| {
                        crate::LemmaError::Engine(format!("Document {} not found", target_doc_name))
                    })?;
                }

                let mut full_segments = prefix_segments;
                full_segments.extend_from_slice(&relative_rule_path.segments);

                crate::RulePath {
                    rule: relative_rule_path.rule.clone(),
                    segments: full_segments,
                }
            };

            // Check if rule has a result
            if let Some(result) = context.rule_results.get(&lookup_path).cloned() {
                match result {
                    OperationResult::Veto(msg) => {
                        // Rule was vetoed - the veto applies to this rule too
                        // Record the operation so proof builder can find it
                        context.push_operation(
                            crate::OperationKind::RuleUsed {
                                rule_ref: rule_ref.clone(),
                                rule_path: lookup_path.clone(),
                                result: OperationResult::Veto(msg.clone()),
                            },
                            expr.id,
                        );
                        return Ok(OperationResult::Veto(msg));
                    }
                    OperationResult::Value(value) => {
                        // Record that we used this rule, including the full path
                        context.push_operation(
                            crate::OperationKind::RuleUsed {
                                rule_ref: rule_ref.clone(),
                                rule_path: lookup_path.clone(),
                                result: OperationResult::Value(value.clone()),
                            },
                            expr.id,
                        );

                        return Ok(OperationResult::Value(value));
                    }
                }
            }

            // Rule not computed yet or doesn't exist
            // Note: If the rule failed due to missing facts, it should already be in rule_results with a Veto
            // If it's not in rule_results, it either doesn't exist or hasn't been evaluated yet
            Err(LemmaError::Engine(format!(
                "Rule {} not found",
                lookup_path
            )))
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_result = evaluate_expression(left, rule_doc, context, fact_prefix)?;
            let right_result = evaluate_expression(right, rule_doc, context, fact_prefix)?;

            // If either operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = left_result {
                return Ok(OperationResult::Veto(msg));
            }
            if let OperationResult::Veto(msg) = right_result {
                return Ok(OperationResult::Veto(msg));
            }

            // Both operands must have values at this point
            let left_val = left_result.expect_value("arithmetic left operand")?;
            let right_val = right_result.expect_value("arithmetic right operand")?;

            // Convert Engine errors to Runtime errors with source location
            let result = super::operations::arithmetic_operation(left_val, op, right_val)
                .map_err(|e| convert_engine_error_to_runtime(e, expr, context))?;

            // Extract the original expression text from the source
            let expr_text = expr.get_source_text(context.sources);

            context.push_operation(
                crate::OperationKind::Computation {
                    kind: ComputationKind::Arithmetic(op.clone()),
                    inputs: vec![left_val.clone(), right_val.clone()],
                    result: result.clone(),
                    expr: expr_text,
                },
                expr.id,
            );

            Ok(OperationResult::Value(result))
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_result = evaluate_expression(left, rule_doc, context, fact_prefix)?;
            let right_result = evaluate_expression(right, rule_doc, context, fact_prefix)?;

            // If either operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = left_result {
                return Ok(OperationResult::Veto(msg));
            }
            if let OperationResult::Veto(msg) = right_result {
                return Ok(OperationResult::Veto(msg));
            }

            // Both operands must have values at this point
            let left_val = left_result.expect_value("comparison left operand")?;
            let right_val = right_result.expect_value("comparison right operand")?;

            let result = super::operations::comparison_operation(left_val, op, right_val)?;

            // Extract the original expression text from the source
            let expr_text = expr.get_source_text(context.sources);

            let result_value = LiteralValue::Boolean(result.into());

            context.push_operation(
                crate::OperationKind::Computation {
                    kind: ComputationKind::Comparison(op.clone()),
                    inputs: vec![left_val.clone(), right_val.clone()],
                    result: result_value.clone(),
                    expr: expr_text,
                },
                expr.id,
            );

            Ok(OperationResult::Value(result_value))
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_result = evaluate_expression(left, rule_doc, context, fact_prefix)?;
            let right_result = evaluate_expression(right, rule_doc, context, fact_prefix)?;

            // If either operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = left_result {
                return Ok(OperationResult::Veto(msg));
            }
            if let OperationResult::Veto(msg) = right_result {
                return Ok(OperationResult::Veto(msg));
            }

            // Both operands must have boolean values at this point
            let left_val = left_result.expect_value("logical AND left operand")?;
            let right_val = right_result.expect_value("logical AND right operand")?;

            match (left_val, right_val) {
                (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => {
                    // No operation record for logical operations - only record sub-expressions
                    let result = l.into() && r.into();
                    Ok(OperationResult::Value(LiteralValue::Boolean(result.into())))
                }
                _ => Err(LemmaError::Engine(
                    "Logical AND requires boolean operands".to_string(),
                )),
            }
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_result = evaluate_expression(left, rule_doc, context, fact_prefix)?;
            let right_result = evaluate_expression(right, rule_doc, context, fact_prefix)?;

            // If either operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = left_result {
                return Ok(OperationResult::Veto(msg));
            }
            if let OperationResult::Veto(msg) = right_result {
                return Ok(OperationResult::Veto(msg));
            }

            // Both operands must have boolean values at this point
            let left_val = left_result.expect_value("logical OR left operand")?;
            let right_val = right_result.expect_value("logical OR right operand")?;

            match (left_val, right_val) {
                (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => {
                    // No operation record for logical operations - only record sub-expressions
                    let result = l.into() || r.into();
                    Ok(OperationResult::Value(LiteralValue::Boolean(result.into())))
                }
                _ => Err(LemmaError::Engine(
                    "Logical OR requires boolean operands".to_string(),
                )),
            }
        }

        ExpressionKind::LogicalNegation(operand, _negation_type) => {
            let result = evaluate_expression(operand, rule_doc, context, fact_prefix)?;

            // If the operand is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = result {
                return Ok(OperationResult::Veto(msg));
            }

            // Operand must have a value at this point
            let value = result.expect_value("logical negation operand")?;

            match value {
                LiteralValue::Boolean(b) => {
                    let result = !bool::from(b);
                    let result_value = LiteralValue::Boolean(result.into());
                    Ok(OperationResult::Value(result_value))
                }
                _ => Err(LemmaError::Engine(
                    "Logical NOT requires boolean operand".to_string(),
                )),
            }
        }

        ExpressionKind::UnitConversion(value_expr, target) => {
            let result = evaluate_expression(value_expr, rule_doc, context, fact_prefix)?;

            // If the value is vetoed, propagate the veto
            if let OperationResult::Veto(msg) = result {
                return Ok(OperationResult::Veto(msg));
            }

            // Value must exist at this point
            let value = result.expect_value("unit conversion operand")?;
            let converted = super::units::convert_unit(value, target)?;
            Ok(OperationResult::Value(converted))
        }

        ExpressionKind::MathematicalComputation(op, operand) => {
            let expr_text = expr.get_source_text(context.sources);
            let result = evaluate_mathematical_operator(
                op,
                operand,
                expr.id,
                rule_doc,
                context,
                fact_prefix,
            )?;

            // Inject the expression text into the last operation record
            if let Some(OperationRecord {
                kind:
                    crate::OperationKind::Computation {
                        expr: expr_field, ..
                    },
                ..
            }) = context.operations.last_mut()
            {
                *expr_field = expr_text;
            }

            Ok(result)
        }

        ExpressionKind::Veto(veto_expr) => Ok(OperationResult::Veto(veto_expr.message.clone())),

        ExpressionKind::FactHasAnyValue(fact_ref) => {
            // Check if fact exists and has a value, with path prefix applied
            let lookup_ref = if !fact_prefix.is_empty() {
                let mut qualified_reference = fact_prefix.to_vec();
                qualified_reference.extend_from_slice(&fact_ref.reference);
                FactReference {
                    reference: qualified_reference,
                }
            } else {
                fact_ref.clone()
            };
            let has_value = context.facts.contains_key(&lookup_ref);
            Ok(OperationResult::Value(LiteralValue::Boolean(
                has_value.into(),
            )))
        }
    }
}

/// Evaluate a mathematical operator (sqrt, sin, cos, etc.)
fn evaluate_mathematical_operator(
    op: &MathematicalComputation,
    operand: &Expression,
    expression_id: crate::ExpressionId,
    rule_doc: &crate::LemmaDoc,
    context: &mut EvaluationContext,
    fact_prefix: &[String],
) -> Result<OperationResult, LemmaError> {
    let result = evaluate_expression(operand, rule_doc, context, fact_prefix)?;

    // If the operand is vetoed, propagate the veto
    if let OperationResult::Veto(msg) = result {
        return Ok(OperationResult::Veto(msg));
    }

    // Operand must have a numeric value at this point
    let value = result.expect_value("mathematical operator operand")?;

    match value {
        LiteralValue::Number(n) => {
            use rust_decimal::prelude::ToPrimitive;
            let float_val = n.to_f64().ok_or_else(|| {
                LemmaError::Engine("Cannot convert to float for mathematical operation".to_string())
            })?;

            match op {
                // Float-based functions
                MathematicalComputation::Sqrt
                | MathematicalComputation::Sin
                | MathematicalComputation::Cos
                | MathematicalComputation::Tan
                | MathematicalComputation::Asin
                | MathematicalComputation::Acos
                | MathematicalComputation::Atan
                | MathematicalComputation::Log
                | MathematicalComputation::Exp => {
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
                        _ => unreachable!(),
                    };
                    let decimal_result =
                        Decimal::from_f64_retain(math_result).ok_or_else(|| {
                            LemmaError::Engine(
                                "Mathematical operation result cannot be represented".to_string(),
                            )
                        })?;
                    let result_value = LiteralValue::Number(decimal_result);
                    context.push_operation(
                        crate::OperationKind::Computation {
                            kind: ComputationKind::Mathematical(op.clone()),
                            inputs: vec![value.clone()],
                            result: result_value.clone(),
                            expr: None,
                        },
                        expression_id,
                    );
                    Ok(OperationResult::Value(result_value))
                }
                // Decimal-native functions
                MathematicalComputation::Abs => {
                    let result_value = LiteralValue::Number(n.abs());
                    context.push_operation(
                        crate::OperationKind::Computation {
                            kind: ComputationKind::Mathematical(op.clone()),
                            inputs: vec![value.clone()],
                            result: result_value.clone(),
                            expr: None,
                        },
                        expression_id,
                    );
                    Ok(OperationResult::Value(result_value))
                }
                MathematicalComputation::Floor => {
                    let result_value = LiteralValue::Number(n.floor());
                    context.push_operation(
                        crate::OperationKind::Computation {
                            kind: ComputationKind::Mathematical(op.clone()),
                            inputs: vec![value.clone()],
                            result: result_value.clone(),
                            expr: None,
                        },
                        expression_id,
                    );
                    Ok(OperationResult::Value(result_value))
                }
                MathematicalComputation::Ceil => {
                    let result_value = LiteralValue::Number(n.ceil());
                    context.push_operation(
                        crate::OperationKind::Computation {
                            kind: ComputationKind::Mathematical(op.clone()),
                            inputs: vec![value.clone()],
                            result: result_value.clone(),
                            expr: None,
                        },
                        expression_id,
                    );
                    Ok(OperationResult::Value(result_value))
                }
                MathematicalComputation::Round => {
                    let result_value = LiteralValue::Number(n.round());
                    context.push_operation(
                        crate::OperationKind::Computation {
                            kind: ComputationKind::Mathematical(op.clone()),
                            inputs: vec![value.clone()],
                            result: result_value.clone(),
                            expr: None,
                        },
                        expression_id,
                    );
                    Ok(OperationResult::Value(result_value))
                }
            }
        }
        _ => Err(LemmaError::Engine(
            "Mathematical operators require number operands".to_string(),
        )),
    }
}

/// Convert an Engine error to a Runtime error with proper source location
///
/// This is used to add span information to errors that occur during expression evaluation.
/// If source location information is not available, the error remains as an Engine error.
fn convert_engine_error_to_runtime(
    error: LemmaError,
    expr: &Expression,
    context: &EvaluationContext,
) -> LemmaError {
    match error {
        LemmaError::Engine(msg) => {
            // Only convert to Runtime error if we have proper source location
            if let Some(source_location) = &expr.source_location {
                let source_text: Arc<str> = context
                    .sources
                    .get(&source_location.source_id)
                    .map(|s| Arc::from(s.as_str()))
                    .unwrap_or_default();

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
                    source_location: source_location.clone(),
                    source_text,
                    doc_start_line: context.current_doc.start_line,
                    suggestion,
                }))
            } else {
                // No source location available - keep as Engine error
                LemmaError::Engine(msg)
            }
        }
        other => other,
    }
}
