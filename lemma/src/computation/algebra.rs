//! Algebraic equation solving for expression trees.
//!
//! Solves single-variable linear equations by isolating the unknown
//! through inverse operations.

use crate::{ArithmeticComputation, Expression, ExpressionKind, FactReference, LemmaError};
use std::sync::Arc;

/// Solve for an unknown expression within a larger expression.
///
/// Given an expression tree and a target unknown, returns a new expression
/// that computes the unknown's value from a "value" placeholder.
///
/// The unknown must appear exactly once in the expression (linear equations only).
///
/// # Errors
///
/// Returns an error if:
/// - The unknown does not appear in the expression
/// - The unknown appears more than once (non-linear)
/// - The expression contains unsupported operations (Modulo, Power)
pub fn solve_for(expression: &Expression, unknown: &Expression) -> Result<Expression, LemmaError> {
    let occurrence_count = count_occurrences(expression, unknown);

    if occurrence_count == 0 {
        let loc = expression
            .source_location
            .as_ref()
            .or(unknown.source_location.as_ref())
            .expect("Expression or unknown must have source_location");
        let source_text = std::sync::Arc::from("");
        return Err(LemmaError::engine(
            "Unknown not found in expression",
            loc.span.clone(),
            loc.attribute.clone(),
            source_text,
            loc.doc_name.clone(),
            1,
            None::<String>,
        ));
    }

    if occurrence_count > 1 {
        let loc = expression
            .source_location
            .as_ref()
            .or(unknown.source_location.as_ref())
            .expect("Expression or unknown must have source_location");
        let source_text = std::sync::Arc::from("");
        return Err(LemmaError::engine(
            "Non-linear: unknown appears multiple times",
            loc.span.clone(),
            loc.attribute.clone(),
            source_text,
            loc.doc_name.clone(),
            1,
            None::<String>,
        ));
    }

    let value_placeholder = Expression::new(
        ExpressionKind::FactReference(FactReference::local("value".to_string())),
        None,
    );

    isolate(expression, unknown, value_placeholder)
}

/// Replace all occurrences of `from` with `to` in the expression tree.
pub fn substitute(expression: &Expression, from: &Expression, to: &Expression) -> Expression {
    if expression == from {
        return to.clone();
    }

    match &expression.kind {
        ExpressionKind::Arithmetic(left, operation, right) => {
            let substituted_left = substitute(left, from, to);
            let substituted_right = substitute(right, from, to);
            Expression::new(
                ExpressionKind::Arithmetic(
                    Arc::new(substituted_left),
                    operation.clone(),
                    Arc::new(substituted_right),
                ),
                None,
            )
        }
        _ => expression.clone(),
    }
}

/// Count how many times the unknown expression appears in the expression tree.
fn count_occurrences(expression: &Expression, unknown: &Expression) -> usize {
    if expression == unknown {
        return 1;
    }

    match &expression.kind {
        ExpressionKind::Arithmetic(left, _, right) => {
            count_occurrences(left, unknown) + count_occurrences(right, unknown)
        }
        _ => 0,
    }
}

/// Isolate the unknown by walking the tree and applying inverse operations.
///
/// At each arithmetic node, determines which side contains the unknown,
/// applies the inverse operation to the accumulated result, and recurses.
fn isolate(
    expression: &Expression,
    unknown: &Expression,
    result: Expression,
) -> Result<Expression, LemmaError> {
    if expression == unknown {
        return Ok(result);
    }

    match &expression.kind {
        ExpressionKind::Arithmetic(left, operation, right) => {
            let left_count = count_occurrences(left, unknown);

            if left_count > 0 {
                let new_result = inverse_left(operation.clone(), result, (**right).clone())?;
                isolate(left, unknown, new_result)
            } else {
                let new_result = inverse_right(operation.clone(), result, (**left).clone())?;
                isolate(right, unknown, new_result)
            }
        }
        _ => {
            let loc = expression
                .source_location
                .as_ref()
                .or(unknown.source_location.as_ref())
                .expect("Expression or unknown must have source_location");
            let source_text = std::sync::Arc::from("");
            Err(LemmaError::engine(
                "Unknown not found on this path",
                loc.span.clone(),
                loc.attribute.clone(),
                source_text,
                loc.doc_name.clone(),
                1,
                None::<String>,
            ))
        }
    }
}

/// Apply inverse operation when unknown is on the left side of the operator.
///
/// Given `left op right = result`, solves for `left`.
fn inverse_left(
    operation: ArithmeticComputation,
    result: Expression,
    right: Expression,
) -> Result<Expression, LemmaError> {
    let inverse_operation = match operation {
        // left + right = result → left = result - right
        ArithmeticComputation::Add => ArithmeticComputation::Subtract,
        // left - right = result → left = result + right
        ArithmeticComputation::Subtract => ArithmeticComputation::Add,
        // left * right = result → left = result / right
        ArithmeticComputation::Multiply => ArithmeticComputation::Divide,
        // left / right = result → left = result * right
        ArithmeticComputation::Divide => ArithmeticComputation::Multiply,
        ArithmeticComputation::Modulo => {
            let loc = result
                .source_location
                .as_ref()
                .or(right.source_location.as_ref())
                .expect("Result or right expression must have source_location");
            let source_text = std::sync::Arc::from("");
            return Err(LemmaError::engine(
                "Modulo operation is not invertible",
                loc.span.clone(),
                loc.attribute.clone(),
                source_text,
                loc.doc_name.clone(),
                1,
                None::<String>,
            ));
        }
        ArithmeticComputation::Power => {
            let loc = result
                .source_location
                .as_ref()
                .or(right.source_location.as_ref())
                .expect("Result or right expression must have source_location");
            let source_text = std::sync::Arc::from("");
            return Err(LemmaError::engine(
                "Power operation is not invertible",
                loc.span.clone(),
                loc.attribute.clone(),
                source_text,
                loc.doc_name.clone(),
                1,
                None::<String>,
            ));
        }
    };

    Ok(Expression::new(
        ExpressionKind::Arithmetic(Arc::new(result), inverse_operation, Arc::new(right)),
        None,
    ))
}

/// Apply inverse operation when unknown is on the right side of the operator.
///
/// Given `left op right = result`, solves for `right`.
///
/// Note: For non-commutative operations (subtract, divide), the inverse
/// is different than when unknown is on the left.
fn inverse_right(
    operation: ArithmeticComputation,
    result: Expression,
    left: Expression,
) -> Result<Expression, LemmaError> {
    match operation {
        // left + right = result → right = result - left
        ArithmeticComputation::Add => Ok(Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(result),
                ArithmeticComputation::Subtract,
                Arc::new(left),
            ),
            None,
        )),
        // left - right = result → right = left - result (different!)
        ArithmeticComputation::Subtract => Ok(Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(left),
                ArithmeticComputation::Subtract,
                Arc::new(result),
            ),
            None,
        )),
        // left * right = result → right = result / left
        ArithmeticComputation::Multiply => Ok(Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(result),
                ArithmeticComputation::Divide,
                Arc::new(left),
            ),
            None,
        )),
        // left / right = result → right = left / result (different!)
        ArithmeticComputation::Divide => Ok(Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(left),
                ArithmeticComputation::Divide,
                Arc::new(result),
            ),
            None,
        )),
        ArithmeticComputation::Modulo => {
            let loc = result
                .source_location
                .as_ref()
                .or(left.source_location.as_ref())
                .expect("Result or left expression must have source_location");
            let source_text = std::sync::Arc::from("");
            Err(LemmaError::engine(
                "Modulo operation is not invertible",
                loc.span.clone(),
                loc.attribute.clone(),
                source_text,
                loc.doc_name.clone(),
                1,
                None::<String>,
            ))
        }
        ArithmeticComputation::Power => {
            let loc = result
                .source_location
                .as_ref()
                .or(left.source_location.as_ref())
                .expect("Result or left expression must have source_location");
            let source_text = std::sync::Arc::from("");
            Err(LemmaError::engine(
                "Power operation is not invertible",
                loc.span.clone(),
                loc.attribute.clone(),
                source_text,
                loc.doc_name.clone(),
                1,
                None::<String>,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;
    use crate::LiteralValue;

    fn placeholder(name: &str) -> Expression {
        use crate::parsing::ast::Span;
        use crate::Source;
        Expression::new(
            ExpressionKind::FactReference(FactReference::local(name.to_string())),
            Some(Source::new(
                "<test>",
                Span {
                    start: 0,
                    end: name.len(),
                    line: 1,
                    col: 0,
                },
                "test",
            )),
        )
    }

    fn number(value: rust_decimal::Decimal) -> Expression {
        use crate::parsing::ast::Span;
        use crate::Source;
        Expression::new(
            ExpressionKind::Literal(LiteralValue::number(value)),
            Some(Source::new(
                "<test>",
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "test",
            )),
        )
    }

    fn arithmetic(
        left: Expression,
        operation: ArithmeticComputation,
        right: Expression,
    ) -> Expression {
        use crate::parsing::ast::Span;
        use crate::Source;
        Expression::new(
            ExpressionKind::Arithmetic(Arc::new(left), operation, Arc::new(right)),
            Some(Source::new(
                "<test>",
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "test",
            )),
        )
    }

    #[test]
    fn solve_multiply_left() {
        // x * 3 = value → x = value / 3
        let x = placeholder("x");
        let expression = arithmetic(
            x.clone(),
            ArithmeticComputation::Multiply,
            number(Decimal::from(3)),
        );

        let result = solve_for(&expression, &x).unwrap();

        let expected = arithmetic(
            placeholder("value"),
            ArithmeticComputation::Divide,
            number(Decimal::from(3)),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn solve_multiply_right() {
        // 3 * x = value → x = value / 3
        let x = placeholder("x");
        let expression = arithmetic(
            number(Decimal::from(3)),
            ArithmeticComputation::Multiply,
            x.clone(),
        );

        let result = solve_for(&expression, &x).unwrap();

        let expected = arithmetic(
            placeholder("value"),
            ArithmeticComputation::Divide,
            number(Decimal::from(3)),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn solve_divide_left() {
        // x / 3 = value → x = value * 3
        let x = placeholder("x");
        let expression = arithmetic(
            x.clone(),
            ArithmeticComputation::Divide,
            number(Decimal::from(3)),
        );

        let result = solve_for(&expression, &x).unwrap();

        let expected = arithmetic(
            placeholder("value"),
            ArithmeticComputation::Multiply,
            number(Decimal::from(3)),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn solve_divide_right() {
        // 3 / x = value → x = 3 / value
        let x = placeholder("x");
        let expression = arithmetic(
            number(Decimal::from(3)),
            ArithmeticComputation::Divide,
            x.clone(),
        );

        let result = solve_for(&expression, &x).unwrap();

        let expected = arithmetic(
            number(Decimal::from(3)),
            ArithmeticComputation::Divide,
            placeholder("value"),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn solve_add_left() {
        // x + 3 = value → x = value - 3
        let x = placeholder("x");
        let expression = arithmetic(
            x.clone(),
            ArithmeticComputation::Add,
            number(Decimal::from(3)),
        );

        let result = solve_for(&expression, &x).unwrap();

        let expected = arithmetic(
            placeholder("value"),
            ArithmeticComputation::Subtract,
            number(Decimal::from(3)),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn solve_subtract_left() {
        // x - 3 = value → x = value + 3
        let x = placeholder("x");
        let expression = arithmetic(
            x.clone(),
            ArithmeticComputation::Subtract,
            number(Decimal::from(3)),
        );

        let result = solve_for(&expression, &x).unwrap();

        let expected = arithmetic(
            placeholder("value"),
            ArithmeticComputation::Add,
            number(Decimal::from(3)),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn solve_subtract_right() {
        // 3 - x = value → x = 3 - value
        let x = placeholder("x");
        let expression = arithmetic(
            number(Decimal::from(3)),
            ArithmeticComputation::Subtract,
            x.clone(),
        );

        let result = solve_for(&expression, &x).unwrap();

        let expected = arithmetic(
            number(Decimal::from(3)),
            ArithmeticComputation::Subtract,
            placeholder("value"),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn solve_compound_fahrenheit_to_celsius() {
        // fahrenheit = celsius * 9/5 + 32
        // Solve for celsius: celsius = (value - 32) * 5/9
        let celsius = placeholder("celsius");
        let nine_fifths = arithmetic(
            number(Decimal::from(9)),
            ArithmeticComputation::Divide,
            number(Decimal::from(5)),
        );
        let expression = arithmetic(
            arithmetic(
                celsius.clone(),
                ArithmeticComputation::Multiply,
                nine_fifths,
            ),
            ArithmeticComputation::Add,
            number(Decimal::from(32)),
        );

        let result = solve_for(&expression, &celsius).unwrap();

        // Expected: (value - 32) / (9/5)
        let expected_nine_fifths = arithmetic(
            number(Decimal::from(9)),
            ArithmeticComputation::Divide,
            number(Decimal::from(5)),
        );
        let expected = arithmetic(
            arithmetic(
                placeholder("value"),
                ArithmeticComputation::Subtract,
                number(Decimal::from(32)),
            ),
            ArithmeticComputation::Divide,
            expected_nine_fifths,
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn solve_with_fact_reference() {
        // x * 9/5 + offset = value → x = (value - offset) / (9/5)
        let x = placeholder("x");
        let offset = placeholder("offset");
        let nine_fifths = arithmetic(
            number(Decimal::from(9)),
            ArithmeticComputation::Divide,
            number(Decimal::from(5)),
        );
        let expression = arithmetic(
            arithmetic(x.clone(), ArithmeticComputation::Multiply, nine_fifths),
            ArithmeticComputation::Add,
            offset.clone(),
        );

        let result = solve_for(&expression, &x).unwrap();

        // Expected: (value - offset) / (9/5)
        let expected_nine_fifths = arithmetic(
            number(Decimal::from(9)),
            ArithmeticComputation::Divide,
            number(Decimal::from(5)),
        );
        let expected = arithmetic(
            arithmetic(
                placeholder("value"),
                ArithmeticComputation::Subtract,
                offset,
            ),
            ArithmeticComputation::Divide,
            expected_nine_fifths,
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn error_unknown_not_found() {
        let x = placeholder("x");
        let y = placeholder("y");
        let expression = arithmetic(y, ArithmeticComputation::Multiply, number(Decimal::from(3)));

        let result = solve_for(&expression, &x);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown not found"));
    }

    #[test]
    fn error_non_linear() {
        // x * x is non-linear
        let x = placeholder("x");
        let expression = arithmetic(x.clone(), ArithmeticComputation::Multiply, x.clone());

        let result = solve_for(&expression, &x);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Non-linear")
                || error_msg.contains("non-linear")
                || error_msg.contains("multiple times")
        );
    }

    #[test]
    fn error_modulo_not_invertible() {
        let x = placeholder("x");
        let expression = arithmetic(
            x.clone(),
            ArithmeticComputation::Modulo,
            number(Decimal::from(3)),
        );

        let result = solve_for(&expression, &x);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Modulo operation is not invertible")
                || error_msg.contains("not invertible")
        );
    }

    #[test]
    fn error_power_not_invertible() {
        let x = placeholder("x");
        let expression = arithmetic(
            x.clone(),
            ArithmeticComputation::Power,
            number(Decimal::from(2)),
        );

        let result = solve_for(&expression, &x);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Power operation is not invertible")
                || error_msg.contains("not invertible")
        );
    }

    #[test]
    fn substitute_simple() {
        let x = placeholder("x");
        let replacement = number(Decimal::from(5));

        let expression = arithmetic(
            x.clone(),
            ArithmeticComputation::Multiply,
            number(Decimal::from(3)),
        );

        let result = substitute(&expression, &x, &replacement);

        let expected = arithmetic(
            number(Decimal::from(5)),
            ArithmeticComputation::Multiply,
            number(Decimal::from(3)),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn substitute_nested() {
        // (x + 2) * 3 with x replaced by 5 → (5 + 2) * 3
        let x = placeholder("x");
        let replacement = number(Decimal::from(5));

        let inner = arithmetic(
            x.clone(),
            ArithmeticComputation::Add,
            number(Decimal::from(2)),
        );
        let expression = arithmetic(
            inner,
            ArithmeticComputation::Multiply,
            number(Decimal::from(3)),
        );

        let result = substitute(&expression, &x, &replacement);

        let expected_inner = arithmetic(
            number(Decimal::from(5)),
            ArithmeticComputation::Add,
            number(Decimal::from(2)),
        );
        let expected = arithmetic(
            expected_inner,
            ArithmeticComputation::Multiply,
            number(Decimal::from(3)),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn substitute_chained_units() {
        // milligram = kilogram / 1000000
        // kilogram = 1000 * gram
        // Substitute kilogram → (1000 * gram) / 1000000
        let kilogram = placeholder("kilogram");
        let gram = placeholder("gram");

        let kilogram_definition = arithmetic(
            number(Decimal::from(1000)),
            ArithmeticComputation::Multiply,
            gram.clone(),
        );
        let milligram_expression = arithmetic(
            kilogram.clone(),
            ArithmeticComputation::Divide,
            number(Decimal::from(1_000_000)),
        );

        let result = substitute(&milligram_expression, &kilogram, &kilogram_definition);

        let expected = arithmetic(
            arithmetic(
                number(Decimal::from(1000)),
                ArithmeticComputation::Multiply,
                gram,
            ),
            ArithmeticComputation::Divide,
            number(Decimal::from(1_000_000)),
        );
        assert_eq!(result, expected);
    }
}
