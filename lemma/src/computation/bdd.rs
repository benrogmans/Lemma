//! BDD-style expression reduction
//!
//! Algebraic reduction of expressions:
//! - Boolean identity: `true AND x → x`, `false OR x → x`
//! - Constant folding: `5 + 3 → 8`, `true AND false → false`
//! - Double negation: `NOT(NOT(x)) → x`
//!
//! Used during planning (compile-time) and inversion (query-time).

use super::{
    arithmetic_operation, check_function_range_violation, comparison_operation, OperationResult,
};
use crate::semantic::{
    BooleanValue, ComparisonComputation, Expression, ExpressionKind, LiteralValue,
};

/// Reduce an expression algebraically
///
/// Performs:
/// - Boolean identity reduction
/// - Constant folding for arithmetic and comparisons
/// - Double negation elimination
pub fn reduce(expression: Expression) -> Expression {
    match expression.kind {
        ExpressionKind::LogicalAnd(left, right) => {
            let left_reduced = reduce(*left);
            let right_reduced = reduce(*right);

            // X ∧ false → false
            if left_reduced.is_boolean_false() || right_reduced.is_boolean_false() {
                return Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
                    None,
                );
            }

            // X ∧ true → X
            if left_reduced.is_boolean_true() {
                return right_reduced;
            }
            if right_reduced.is_boolean_true() {
                return left_reduced;
            }

            Expression::new(
                ExpressionKind::LogicalAnd(Box::new(left_reduced), Box::new(right_reduced)),
                expression.source,
            )
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_reduced = reduce(*left);
            let right_reduced = reduce(*right);

            // X ∨ true → true
            if left_reduced.is_boolean_true() || right_reduced.is_boolean_true() {
                return Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                    None,
                );
            }

            // X ∨ false → X
            if left_reduced.is_boolean_false() {
                return right_reduced;
            }
            if right_reduced.is_boolean_false() {
                return left_reduced;
            }

            Expression::new(
                ExpressionKind::LogicalOr(Box::new(left_reduced), Box::new(right_reduced)),
                expression.source,
            )
        }

        ExpressionKind::LogicalNegation(inner, negation_type) => {
            let inner_reduced = reduce(*inner);

            // NOT(true) → false
            if inner_reduced.is_boolean_true() {
                return Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
                    None,
                );
            }

            // NOT(false) → true
            if inner_reduced.is_boolean_false() {
                return Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                    None,
                );
            }

            // NOT(NOT(X)) → X
            if let ExpressionKind::LogicalNegation(double_inner, _) = inner_reduced.kind {
                return *double_inner;
            }

            Expression::new(
                ExpressionKind::LogicalNegation(Box::new(inner_reduced), negation_type),
                expression.source,
            )
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_reduced = reduce(*left);
            let right_reduced = reduce(*right);

            // (A ∨ B) op C → (A op C) ∨ (B op C)
            if let ExpressionKind::LogicalOr(or_left, or_right) = left_reduced.kind {
                let left_comparison = Expression::new(
                    ExpressionKind::Comparison(or_left, op.clone(), Box::new(right_reduced.clone())),
                    None,
                );
                let right_comparison = Expression::new(
                    ExpressionKind::Comparison(or_right, op, Box::new(right_reduced)),
                    None,
                );
                return reduce(Expression::new(
                    ExpressionKind::LogicalOr(Box::new(left_comparison), Box::new(right_comparison)),
                    expression.source,
                ));
            }

            // C op (A ∨ B) → (C op A) ∨ (C op B)
            // C op (A ∨ B) → (C op A) ∨ (C op B)
            if let ExpressionKind::LogicalOr(or_left, or_right) = right_reduced.kind {
                let left_comparison = Expression::new(
                    ExpressionKind::Comparison(Box::new(left_reduced.clone()), op.clone(), or_left),
                    None,
                );
                let right_comparison = Expression::new(
                    ExpressionKind::Comparison(Box::new(left_reduced), op, or_right),
                    None,
                );
                return reduce(Expression::new(
                    ExpressionKind::LogicalOr(Box::new(left_comparison), Box::new(right_comparison)),
                    expression.source,
                ));
            }

            // Distribute comparison into AND (equation branch structure)
            // (cond ∧ result) op value → cond ∧ (result op value)
            if let ExpressionKind::LogicalAnd(and_left, and_right) = left_reduced.kind {
                let inner_comparison = Expression::new(
                    ExpressionKind::Comparison(and_right, op, Box::new(right_reduced)),
                    None,
                );
                return reduce(Expression::new(
                    ExpressionKind::LogicalAnd(and_left, Box::new(inner_comparison)),
                    expression.source,
                ));
            }

            // Symmetric: value op (cond ∧ result) → cond ∧ (value op result)
            if let ExpressionKind::LogicalAnd(and_left, and_right) = right_reduced.kind {
                let inner_comparison = Expression::new(
                    ExpressionKind::Comparison(Box::new(left_reduced), op, and_right),
                    None,
                );
                return reduce(Expression::new(
                    ExpressionKind::LogicalAnd(and_left, Box::new(inner_comparison)),
                    expression.source,
                ));
            }

            // If both sides are literals, evaluate the comparison
            if let (ExpressionKind::Literal(left_literal), ExpressionKind::Literal(right_literal)) =
                (&left_reduced.kind, &right_reduced.kind)
            {
                let result = comparison_operation(left_literal, &op, right_literal);
                if let OperationResult::Value(LiteralValue::Boolean(boolean_value)) = result {
                    return Expression::new(
                        ExpressionKind::Literal(LiteralValue::Boolean(boolean_value)),
                        None,
                    );
                }
            }

            // Check function range violations (e.g., sin(x) > 2 is always false)
            if check_range_violation(&left_reduced, &op, &right_reduced) {
                return Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
                    None,
                );
            }

            Expression::new(
                ExpressionKind::Comparison(
                    Box::new(left_reduced),
                    op,
                    Box::new(right_reduced),
                ),
                expression.source,
            )
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_reduced = reduce(*left);
            let right_reduced = reduce(*right);

            // (A ∨ B) op C → (A op C) ∨ (B op C)
            // (A ∨ B) op C → (A op C) ∨ (B op C)
            if let ExpressionKind::LogicalOr(or_left, or_right) = left_reduced.kind {
                let left_arithmetic = Expression::new(
                    ExpressionKind::Arithmetic(or_left, op.clone(), Box::new(right_reduced.clone())),
                    None,
                );
                let right_arithmetic = Expression::new(
                    ExpressionKind::Arithmetic(or_right, op, Box::new(right_reduced)),
                    None,
                );
                return reduce(Expression::new(
                    ExpressionKind::LogicalOr(Box::new(left_arithmetic), Box::new(right_arithmetic)),
                    expression.source,
                ));
            }

            // C op (A ∨ B) → (C op A) ∨ (C op B)
            // C op (A ∨ B) → (C op A) ∨ (C op B)
            if let ExpressionKind::LogicalOr(or_left, or_right) = right_reduced.kind {
                let left_arithmetic = Expression::new(
                    ExpressionKind::Arithmetic(Box::new(left_reduced.clone()), op.clone(), or_left),
                    None,
                );
                let right_arithmetic = Expression::new(
                    ExpressionKind::Arithmetic(Box::new(left_reduced), op, or_right),
                    None,
                );
                return reduce(Expression::new(
                    ExpressionKind::LogicalOr(Box::new(left_arithmetic), Box::new(right_arithmetic)),
                    expression.source,
                ));
            }

            // Distribute arithmetic into AND (equation branch structure)
            // (cond ∧ result) op value → cond ∧ (result op value)
            if let ExpressionKind::LogicalAnd(and_left, and_right) = left_reduced.kind {
                let inner_arithmetic = Expression::new(
                    ExpressionKind::Arithmetic(and_right, op, Box::new(right_reduced)),
                    None,
                );
                return reduce(Expression::new(
                    ExpressionKind::LogicalAnd(and_left, Box::new(inner_arithmetic)),
                    expression.source,
                ));
            }

            // Symmetric: value op (cond ∧ result) → cond ∧ (value op result)
            if let ExpressionKind::LogicalAnd(and_left, and_right) = right_reduced.kind {
                let inner_arithmetic = Expression::new(
                    ExpressionKind::Arithmetic(Box::new(left_reduced), op, and_right),
                    None,
                );
                return reduce(Expression::new(
                    ExpressionKind::LogicalAnd(and_left, Box::new(inner_arithmetic)),
                    expression.source,
                ));
            }

            // If both sides are literals, evaluate the arithmetic
            if let (ExpressionKind::Literal(left_literal), ExpressionKind::Literal(right_literal)) =
                (&left_reduced.kind, &right_reduced.kind)
            {
                let result = arithmetic_operation(left_literal, &op, right_literal);
                if let OperationResult::Value(value) = result {
                    return Expression::new(ExpressionKind::Literal(value), None);
                }
            }

            Expression::new(
                ExpressionKind::Arithmetic(
                    Box::new(left_reduced),
                    op,
                    Box::new(right_reduced),
                ),
                expression.source,
            )
        }

        ExpressionKind::UnitConversion(inner, target) => {
            let inner_reduced = reduce(*inner);
            Expression::new(
                ExpressionKind::UnitConversion(Box::new(inner_reduced), target),
                expression.source,
            )
        }

        ExpressionKind::MathematicalComputation(op, inner) => {
            let inner_reduced = reduce(*inner);
            Expression::new(
                ExpressionKind::MathematicalComputation(op, Box::new(inner_reduced)),
                expression.source,
            )
        }

        // Leaf nodes - no reduction
        _ => expression,
    }
}

fn check_range_violation(
    left: &Expression,
    op: &ComparisonComputation,
    right: &Expression,
) -> bool {
    // Check left is math function, right is literal
    if let ExpressionKind::MathematicalComputation(math_op, _) = &left.kind {
        if let ExpressionKind::Literal(value) = &right.kind {
            return check_function_range_violation(math_op, op, value).is_some();
        }
    }

    // Check right is math function, left is literal
    if let ExpressionKind::MathematicalComputation(math_op, _) = &right.kind {
        if let ExpressionKind::Literal(value) = &left.kind {
            let reversed = reverse_comparison(op);
            return check_function_range_violation(math_op, &reversed, value).is_some();
        }
    }

    false
}

pub fn reverse_comparison(op: &ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThan,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThanOrEqual,
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThan,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThanOrEqual,
        ComparisonComputation::Equal(n) => ComparisonComputation::Equal(*n),
        ComparisonComputation::NotEqual(n) => ComparisonComputation::NotEqual(*n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::ArithmeticComputation;
    use rust_decimal::Decimal;

    fn literal_bool(value: bool) -> Expression {
        Expression::new(
            ExpressionKind::Literal(LiteralValue::Boolean(if value {
                BooleanValue::True
            } else {
                BooleanValue::False
            })),
            None,
        )
    }

    fn literal_num(n: i64) -> Expression {
        Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(n))),
            None,
        )
    }

    #[test]
    fn test_reduce_and_with_true() {
        // true AND x → x
        let expr = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(literal_num(42))),
            None,
        );
        let simplified = reduce(expr);
        assert!(matches!(
            simplified.kind,
            ExpressionKind::Literal(LiteralValue::Number(_))
        ));
    }

    #[test]
    fn test_reduce_and_with_false() {
        // false AND x → false
        let expr = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(false)), Box::new(literal_num(42))),
            None,
        );
        let simplified = reduce(expr);
        assert!(simplified.is_boolean_false());
    }

    #[test]
    fn test_reduce_or_with_true() {
        // true OR x → true
        let expr = Expression::new(
            ExpressionKind::LogicalOr(Box::new(literal_bool(true)), Box::new(literal_num(42))),
            None,
        );
        let simplified = reduce(expr);
        assert!(simplified.is_boolean_true());
    }

    #[test]
    fn test_reduce_or_with_false() {
        // false OR x → x
        let expr = Expression::new(
            ExpressionKind::LogicalOr(Box::new(literal_bool(false)), Box::new(literal_num(42))),
            None,
        );
        let simplified = reduce(expr);
        assert!(matches!(
            simplified.kind,
            ExpressionKind::Literal(LiteralValue::Number(_))
        ));
    }

    #[test]
    fn test_reduce_arithmetic() {
        // 5 + 3 → 8
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(literal_num(5)),
                ArithmeticComputation::Add,
                Box::new(literal_num(3)),
            ),
            None,
        );
        let simplified = reduce(expr);
        match simplified.kind {
            ExpressionKind::Literal(LiteralValue::Number(n)) => {
                assert_eq!(n, Decimal::from(8));
            }
            _ => panic!("Expected number literal"),
        }
    }

    #[test]
    fn test_reduce_comparison_literals() {
        use crate::semantic::{ComparisonComputation, EqualityNotation};

        // 5 == 5 → true
        let expr = Expression::new(
            ExpressionKind::Comparison(
                Box::new(literal_num(5)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(literal_num(5)),
            ),
            None,
        );
        let simplified = reduce(expr);
        assert!(simplified.is_boolean_true());

        // 5 == 3 → false
        let expr = Expression::new(
            ExpressionKind::Comparison(
                Box::new(literal_num(5)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(literal_num(3)),
            ),
            None,
        );
        let simplified = reduce(expr);
        assert!(simplified.is_boolean_false());
    }

    // ========================================================================
    // Distribution Tests
    // ========================================================================

    /// Helper: check if expression is a LogicalOr
    fn is_or(expr: &Expression) -> bool {
        matches!(expr.kind, ExpressionKind::LogicalOr(_, _))
    }

    /// Helper: count OR branches in a DNF expression
    fn count_or_branches(expr: &Expression) -> usize {
        match &expr.kind {
            ExpressionKind::LogicalOr(left, right) => {
                count_or_branches(left) + count_or_branches(right)
            }
            _ => 1,
        }
    }

    /// `((c₀ ∧ 10) ∨ (c₁ ∧ 20)) == 15` → false after reduction
    ///
    /// Both branches produce values (10 and 20) that don't equal 15.
    #[test]
    fn test_or_in_comparison_reduces_to_false() {
        use crate::semantic::{ComparisonComputation, EqualityNotation, FactPath};

        let c0 = Expression::new(
            ExpressionKind::FactPath(FactPath::local("c0".to_string())),
            None,
        );
        let c1 = Expression::new(
            ExpressionKind::FactPath(FactPath::local("c1".to_string())),
            None,
        );

        // (c₀ ∧ 10)
        let branch0 = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(c0), Box::new(literal_num(10))),
            None,
        );

        // (c₁ ∧ 20)
        let branch1 = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(c1), Box::new(literal_num(20))),
            None,
        );

        // ((c₀ ∧ 10) ∨ (c₁ ∧ 20))
        let or_expr = Expression::new(
            ExpressionKind::LogicalOr(Box::new(branch0), Box::new(branch1)),
            None,
        );

        // ((c₀ ∧ 10) ∨ (c₁ ∧ 20)) == 15
        let comparison = Expression::new(
            ExpressionKind::Comparison(
                Box::new(or_expr),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(literal_num(15)),
            ),
            None,
        );

        let reduced = reduce(comparison);

        // Should reduce to false because:
        // (c₀ ∧ 10 == 15) → (c₀ ∧ false) → false
        // (c₁ ∧ 20 == 15) → (c₁ ∧ false) → false
        // false ∨ false → false
        assert!(
            reduced.is_boolean_false(),
            "((c₀ ∧ 10) ∨ (c₁ ∧ 20)) == 15 should reduce to false, got {:?}",
            reduced.kind
        );
    }

    /// `((c₀ ∧ 10) ∨ (c₁ ∧ 20)) * x == 100` distributes into 2 branches
    #[test]
    fn test_or_in_arithmetic_distributes() {
        use crate::semantic::{ComparisonComputation, EqualityNotation, FactPath};

        let c0 = Expression::new(
            ExpressionKind::FactPath(FactPath::local("c0".to_string())),
            None,
        );
        let c1 = Expression::new(
            ExpressionKind::FactPath(FactPath::local("c1".to_string())),
            None,
        );
        let x = Expression::new(
            ExpressionKind::FactPath(FactPath::local("x".to_string())),
            None,
        );

        // (c₀ ∧ 10)
        let branch0 = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(c0), Box::new(literal_num(10))),
            None,
        );

        // (c₁ ∧ 20)
        let branch1 = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(c1), Box::new(literal_num(20))),
            None,
        );

        // ((c₀ ∧ 10) ∨ (c₁ ∧ 20))
        let or_expr = Expression::new(
            ExpressionKind::LogicalOr(Box::new(branch0), Box::new(branch1)),
            None,
        );

        // ((c₀ ∧ 10) ∨ (c₁ ∧ 20)) * x
        let multiplication = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(or_expr),
                ArithmeticComputation::Multiply,
                Box::new(x),
            ),
            None,
        );

        // ((c₀ ∧ 10) ∨ (c₁ ∧ 20)) * x == 100
        let comparison = Expression::new(
            ExpressionKind::Comparison(
                Box::new(multiplication),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(literal_num(100)),
            ),
            None,
        );

        let reduced = reduce(comparison);

        // Should distribute to OR of two branches
        assert!(
            is_or(&reduced),
            "should produce OR expression, got {:?}",
            reduced.kind
        );

        assert_eq!(
            count_or_branches(&reduced),
            2,
            "should have 2 OR branches"
        );
    }

    /// `x + ((a ∧ 5) ∨ (b ∧ 10)) == 20` distributes
    #[test]
    fn test_or_in_right_operand_distributes() {
        use crate::semantic::{ComparisonComputation, EqualityNotation, FactPath};

        let a = Expression::new(
            ExpressionKind::FactPath(FactPath::local("a".to_string())),
            None,
        );
        let b = Expression::new(
            ExpressionKind::FactPath(FactPath::local("b".to_string())),
            None,
        );
        let x = Expression::new(
            ExpressionKind::FactPath(FactPath::local("x".to_string())),
            None,
        );

        // (a ∧ 5)
        let branch_a = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(a), Box::new(literal_num(5))),
            None,
        );

        // (b ∧ 10)
        let branch_b = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(b), Box::new(literal_num(10))),
            None,
        );

        // ((a ∧ 5) ∨ (b ∧ 10))
        let or_expr = Expression::new(
            ExpressionKind::LogicalOr(Box::new(branch_a), Box::new(branch_b)),
            None,
        );

        // x + ((a ∧ 5) ∨ (b ∧ 10))
        let addition = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(x),
                ArithmeticComputation::Add,
                Box::new(or_expr),
            ),
            None,
        );

        // x + ((a ∧ 5) ∨ (b ∧ 10)) == 20
        let comparison = Expression::new(
            ExpressionKind::Comparison(
                Box::new(addition),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(literal_num(20)),
            ),
            None,
        );

        let reduced = reduce(comparison);

        // Should distribute to OR of two branches
        assert!(
            is_or(&reduced),
            "should produce OR expression, got {:?}",
            reduced.kind
        );

        assert_eq!(
            count_or_branches(&reduced),
            2,
            "should have 2 OR branches"
        );
    }

    /// `((A ∨ B) + (C ∨ D)) == 10` → 4 branches after full distribution
    #[test]
    fn test_nested_or_produces_four_branches() {
        use crate::semantic::{ComparisonComputation, EqualityNotation, FactPath};

        let a = Expression::new(
            ExpressionKind::FactPath(FactPath::local("a".to_string())),
            None,
        );
        let b = Expression::new(
            ExpressionKind::FactPath(FactPath::local("b".to_string())),
            None,
        );
        let c = Expression::new(
            ExpressionKind::FactPath(FactPath::local("c".to_string())),
            None,
        );
        let d = Expression::new(
            ExpressionKind::FactPath(FactPath::local("d".to_string())),
            None,
        );

        // (A ∨ B)
        let a_or_b = Expression::new(
            ExpressionKind::LogicalOr(Box::new(a), Box::new(b)),
            None,
        );

        // (C ∨ D)
        let c_or_d = Expression::new(
            ExpressionKind::LogicalOr(Box::new(c), Box::new(d)),
            None,
        );

        // (A ∨ B) + (C ∨ D)
        let addition = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(a_or_b),
                ArithmeticComputation::Add,
                Box::new(c_or_d),
            ),
            None,
        );

        // ((A ∨ B) + (C ∨ D)) == 10
        let comparison = Expression::new(
            ExpressionKind::Comparison(
                Box::new(addition),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(literal_num(10)),
            ),
            None,
        );

        let reduced = reduce(comparison);

        // Should produce 4 branches: (A+C), (A+D), (B+C), (B+D)
        assert!(
            is_or(&reduced),
            "should produce OR expression, got {:?}",
            reduced.kind
        );

        assert_eq!(
            count_or_branches(&reduced),
            4,
            "should have 4 OR branches after full distribution"
        );
    }

    /// `((a ∧ 3) ∨ (b ∧ 5)) * 5 == 25`
    ///
    /// After distribution and reduction:
    /// - (a ∧ 3*5 == 25) → (a ∧ 15 == 25) → (a ∧ false) → false
    /// - (b ∧ 5*5 == 25) → (b ∧ 25 == 25) → (b ∧ true) → b
    /// Result: false ∨ b → b
    #[test]
    fn test_constant_folding_with_distribution() {
        use crate::semantic::{ComparisonComputation, EqualityNotation, FactPath};

        let a = Expression::new(
            ExpressionKind::FactPath(FactPath::local("a".to_string())),
            None,
        );
        let b = Expression::new(
            ExpressionKind::FactPath(FactPath::local("b".to_string())),
            None,
        );

        // (a ∧ 3)
        let branch_a = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(a), Box::new(literal_num(3))),
            None,
        );

        // (b ∧ 5)
        let branch_b = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(b.clone()), Box::new(literal_num(5))),
            None,
        );

        // ((a ∧ 3) ∨ (b ∧ 5))
        let or_expr = Expression::new(
            ExpressionKind::LogicalOr(Box::new(branch_a), Box::new(branch_b)),
            None,
        );

        // ((a ∧ 3) ∨ (b ∧ 5)) * 5
        let multiplication = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(or_expr),
                ArithmeticComputation::Multiply,
                Box::new(literal_num(5)),
            ),
            None,
        );

        // ((a ∧ 3) ∨ (b ∧ 5)) * 5 == 25
        let comparison = Expression::new(
            ExpressionKind::Comparison(
                Box::new(multiplication),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(literal_num(25)),
            ),
            None,
        );

        let reduced = reduce(comparison);

        // After distribution and constant folding:
        // - (a ∧ 3) * 5 == 25 → (a ∧ 15) == 25 → distributes → (a == 25 ∧ 15 == 25)
        // Actually the AND is inside, so: (a ∧ (3 * 5 == 25)) → (a ∧ false) → false
        // - (b ∧ 5) * 5 == 25 → (b ∧ (5 * 5 == 25)) → (b ∧ true) → b
        // Result: false ∨ b → b

        // The result should be just `b` (a FactPath)
        assert!(
            matches!(reduced.kind, ExpressionKind::FactPath(_)),
            "should reduce to just fact 'b', got {:?}",
            reduced.kind
        );
    }

    /// Test that distribution is applied recursively
    #[test]
    fn test_distribution_recursive() {
        use crate::semantic::{ComparisonComputation, EqualityNotation, FactPath};

        let a = Expression::new(
            ExpressionKind::FactPath(FactPath::local("a".to_string())),
            None,
        );
        let b = Expression::new(
            ExpressionKind::FactPath(FactPath::local("b".to_string())),
            None,
        );

        // (a ∨ b)
        let a_or_b = Expression::new(
            ExpressionKind::LogicalOr(Box::new(a), Box::new(b)),
            None,
        );

        // ((a ∨ b) + 1) - first level
        let add_1 = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(a_or_b),
                ArithmeticComputation::Add,
                Box::new(literal_num(1)),
            ),
            None,
        );

        // (((a ∨ b) + 1) * 2) - nested arithmetic with OR inside
        let mul_2 = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(add_1),
                ArithmeticComputation::Multiply,
                Box::new(literal_num(2)),
            ),
            None,
        );

        // ((((a ∨ b) + 1) * 2) == 10
        let comparison = Expression::new(
            ExpressionKind::Comparison(
                Box::new(mul_2),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(literal_num(10)),
            ),
            None,
        );

        let reduced = reduce(comparison);

        // Should produce 2 branches after all distributions
        assert!(
            is_or(&reduced),
            "should produce OR expression after recursive distribution"
        );

        assert_eq!(
            count_or_branches(&reduced),
            2,
            "should have 2 OR branches after recursive distribution"
        );
    }
}

