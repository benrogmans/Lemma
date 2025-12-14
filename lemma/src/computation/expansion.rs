//! Pure algebraic expression expansion via distribution
//!
//! Transforms expressions into Disjunctive Normal Form (DNF) through:
//! - **OR distribution**: `(A ∨ B) op C → (A op C) ∨ (B op C)`
//! - **Operator pushing**: `(cond ∧ result) op value → cond ∧ (result op value)`
//!
//! No simplification or constant folding happens here - expansion only applies
//! algebraic distribution rules. Simplification is handled separately.
//!
//! Uses fixed-point iteration to handle nested cases correctly.

use crate::parsing::source::Source;
use crate::semantic::{
    ArithmeticComputation, ComparisonComputation, Expression, ExpressionKind,
};
use std::sync::Arc;

/// Check if expression has OR at top level
fn has_or_at_top(expr: &Expression) -> bool {
    matches!(expr.kind, ExpressionKind::LogicalOr(_, _))
}

/// Check if expression has AND at top level
fn has_and_at_top(expr: &Expression) -> bool {
    matches!(expr.kind, ExpressionKind::LogicalAnd(_, _))
}

/// Pure constructor for arithmetic - no folding or simplification
fn make_arithmetic(
    left: Expression,
    op: ArithmeticComputation,
    right: Expression,
    source: Option<Source>,
) -> Expression {
    Expression::new(
        ExpressionKind::Arithmetic(Arc::new(left), op, Arc::new(right)),
        source,
    )
}

/// Pure constructor for comparison
fn make_comparison(
    left: Expression,
    op: ComparisonComputation,
    right: Expression,
    source: Option<Source>,
) -> Expression {
    Expression::new(
        ExpressionKind::Comparison(Arc::new(left), op, Arc::new(right)),
        source,
    )
}

/// Pure constructor for OR
fn make_or(
    left: Expression,
    right: Expression,
    source: Option<Source>,
) -> Expression {
    Expression::new(
        ExpressionKind::LogicalOr(Arc::new(left), Arc::new(right)),
        source,
    )
}

/// Pure constructor for AND
fn make_and(
    left: Expression,
    right: Expression,
    source: Option<Source>,
) -> Expression {
    Expression::new(
        ExpressionKind::LogicalAnd(Arc::new(left), Arc::new(right)),
        source,
    )
}

/// Flatten OR tree into a flat list of branches
/// 
/// Converts nested OR expressions like ((A ∨ B) ∨ C) into [A, B, C]
fn flatten_or(expr: Expression) -> Vec<Expression> {
    match expr.kind {
        ExpressionKind::LogicalOr(left, right) => {
            let mut result = flatten_or(Arc::unwrap_or_clone(left));
            result.extend(flatten_or(Arc::unwrap_or_clone(right)));
            result
        }
        _ => vec![expr],
    }
}

/// Combine multiple expressions with OR
///
/// Takes a list of expressions and combines them with OR operators.
/// Returns false literal if the list is empty.
fn combine_with_or(expressions: Vec<Expression>) -> Expression {
    if expressions.is_empty() {
        return Expression::new(
            ExpressionKind::Literal(crate::semantic::LiteralValue::Boolean(
                crate::semantic::BooleanValue::False,
            )),
            None,
        );
    }

    let mut iter = expressions.into_iter();
    let first = iter.next().expect("checked non-empty above");

    iter.fold(first, |acc, expr| {
        Expression::new(
            ExpressionKind::LogicalOr(Arc::new(acc), Arc::new(expr)),
            None,
        )
    })
}

/// Cross-multiply two lists of expressions with an arithmetic operator
///
/// Creates all combinations: [A, B] × [C, D] with op → [A op C, A op D, B op C, B op D]
/// Each product is recursively expanded.
fn cross_multiply_arithmetic(
    left_branches: Vec<Expression>,
    op: ArithmeticComputation,
    right_branches: Vec<Expression>,
    source: Option<Source>,
) -> Vec<Expression> {
    let mut results = Vec::new();
    for left in left_branches {
        for right in &right_branches {
            let product = make_arithmetic(left.clone(), op.clone(), right.clone(), source.clone());
            results.push(product);
        }
    }
    results
}

/// Cross-multiply two lists of expressions with a comparison operator
///
/// Creates all combinations: [A, B] × [C, D] with op → [A op C, A op D, B op C, B op D]
/// Each product is recursively expanded.
fn cross_multiply_comparison(
    left_branches: Vec<Expression>,
    op: ComparisonComputation,
    right_branches: Vec<Expression>,
    source: Option<Source>,
) -> Vec<Expression> {
    let mut results = Vec::new();
    for left in left_branches {
        for right in &right_branches {
            let product = make_comparison(left.clone(), op.clone(), right.clone(), source.clone());
            results.push(product);
        }
    }
    results
}

/// Distribute OR through arithmetic: (A ∨ B) op C → (A op C) ∨ (B op C)
///
/// Uses flatten-then-cross-multiply to avoid redundant expansion of shared subexpressions
fn distribute_or_through_arithmetic(
    or_expr: Expression,
    op: ArithmeticComputation,
    right: Expression,
    source: Option<Source>,
) -> Expression {
    let left_branches = flatten_or(or_expr);
    let right_branches = if has_or_at_top(&right) {
        flatten_or(right)
    } else {
        vec![right]
    };
    
    let products = cross_multiply_arithmetic(left_branches, op, right_branches, source);
    combine_with_or(products)
}

/// Distribute OR through arithmetic (right side): C op (A ∨ B) → (C op A) ∨ (C op B)
///
/// Uses flatten-then-cross-multiply to avoid redundant expansion of shared subexpressions
fn distribute_or_through_arithmetic_right(
    left: Expression,
    op: ArithmeticComputation,
    or_expr: Expression,
    source: Option<Source>,
) -> Expression {
    let left_branches = if has_or_at_top(&left) {
        flatten_or(left)
    } else {
        vec![left]
    };
    let right_branches = flatten_or(or_expr);
    
    let products = cross_multiply_arithmetic(left_branches, op, right_branches, source);
    combine_with_or(products)
}

/// Distribute OR through comparison: (A ∨ B) op C → (A op C) ∨ (B op C)
///
/// Uses flatten-then-cross-multiply to avoid redundant expansion of shared subexpressions
fn distribute_or_through_comparison(
    or_expr: Expression,
    op: ComparisonComputation,
    right: Expression,
    source: Option<Source>,
) -> Expression {
    let left_branches = flatten_or(or_expr);
    let right_branches = if has_or_at_top(&right) {
        flatten_or(right)
    } else {
        vec![right]
    };
    
    let products = cross_multiply_comparison(left_branches, op, right_branches, source);
    combine_with_or(products)
}

/// Distribute OR through comparison (right side): C op (A ∨ B) → (C op A) ∨ (C op B)
///
/// Uses flatten-then-cross-multiply to avoid redundant expansion of shared subexpressions
fn distribute_or_through_comparison_right(
    left: Expression,
    op: ComparisonComputation,
    or_expr: Expression,
    source: Option<Source>,
) -> Expression {
    let left_branches = if has_or_at_top(&left) {
        flatten_or(left)
    } else {
        vec![left]
    };
    let right_branches = flatten_or(or_expr);
    
    let products = cross_multiply_comparison(left_branches, op, right_branches, source);
    combine_with_or(products)
}

/// Push operator into AND: (C ∧ R) op V → C ∧ (R op V)
fn push_arithmetic_into_and(
    and_expr: Expression,
    op: ArithmeticComputation,
    value: Expression,
    source: Option<Source>,
) -> Expression {
    if let ExpressionKind::LogicalAnd(cond, result) = and_expr.kind {
        let inner_op = make_arithmetic(Arc::unwrap_or_clone(result), op, value, source.clone());
        make_and(Arc::unwrap_or_clone(cond), inner_op, source)
    } else {
        unreachable!("push_arithmetic_into_and requires AND expression")
    }
}

/// Push operator into AND (right side): V op (C ∧ R) → C ∧ (V op R)
fn push_arithmetic_into_and_right(
    value: Expression,
    op: ArithmeticComputation,
    and_expr: Expression,
    source: Option<Source>,
) -> Expression {
    if let ExpressionKind::LogicalAnd(cond, result) = and_expr.kind {
        let inner_op = make_arithmetic(value, op, Arc::unwrap_or_clone(result), source.clone());
        make_and(Arc::unwrap_or_clone(cond), inner_op, source)
    } else {
        unreachable!("push_arithmetic_into_and_right requires AND expression")
    }
}

/// Push operator into AND: (C ∧ R) op V → C ∧ (R op V)
fn push_comparison_into_and(
    and_expr: Expression,
    op: ComparisonComputation,
    value: Expression,
    source: Option<Source>,
) -> Expression {
    if let ExpressionKind::LogicalAnd(cond, result) = and_expr.kind {
        let inner_op = make_comparison(Arc::unwrap_or_clone(result), op, value, source.clone());
        make_and(Arc::unwrap_or_clone(cond), inner_op, source)
    } else {
        unreachable!("push_comparison_into_and requires AND expression")
    }
}

/// Push operator into AND (right side): V op (C ∧ R) → C ∧ (V op R)
fn push_comparison_into_and_right(
    value: Expression,
    op: ComparisonComputation,
    and_expr: Expression,
    source: Option<Source>,
) -> Expression {
    if let ExpressionKind::LogicalAnd(cond, result) = and_expr.kind {
        let inner_op = make_comparison(value, op, Arc::unwrap_or_clone(result), source.clone());
        make_and(Arc::unwrap_or_clone(cond), inner_op, source)
    } else {
        unreachable!("push_comparison_into_and_right requires AND expression")
    }
}

/// Expand expression to DNF via single-pass recursion
///
/// Applies distribution rules recursively, checking for newly exposed ORs
/// after each recursion step. Uses flatten-then-cross-multiply to avoid
/// redundant expansion of shared subexpressions.
pub fn expand(expr: Expression) -> Expression {
    let source = expr.source.clone();

    match expr.kind {
        ExpressionKind::Arithmetic(left, op, right) => {
            // Priority 1: OR distribution (must happen before recursion)
            if has_or_at_top(&left) {
                return distribute_or_through_arithmetic(Arc::unwrap_or_clone(left), op, Arc::unwrap_or_clone(right), source);
            }
            if has_or_at_top(&right) {
                return distribute_or_through_arithmetic_right(Arc::unwrap_or_clone(left), op, Arc::unwrap_or_clone(right), source);
            }

            // Priority 2: Push into AND (equation branch structure)
            if has_and_at_top(&left) {
                return push_arithmetic_into_and(Arc::unwrap_or_clone(left), op, Arc::unwrap_or_clone(right), source);
            }
            if has_and_at_top(&right) {
                return push_arithmetic_into_and_right(Arc::unwrap_or_clone(left), op, Arc::unwrap_or_clone(right), source);
            }

            // Priority 3: Recurse into children
            let left_expanded = expand(Arc::unwrap_or_clone(left));
            let right_expanded = expand(Arc::unwrap_or_clone(right));
            
            // Priority 4: Check if recursion exposed ORs at the top level
            // This handles nested cases like (X + (A ∨ B)) where the OR becomes
            // visible only after expanding the child expression
            if has_or_at_top(&left_expanded) {
                return distribute_or_through_arithmetic(left_expanded, op, right_expanded, source);
            }
            if has_or_at_top(&right_expanded) {
                return distribute_or_through_arithmetic_right(left_expanded, op, right_expanded, source);
            }
            
            make_arithmetic(left_expanded, op, right_expanded, source)
        }

        ExpressionKind::Comparison(left, op, right) => {
            // Priority 1: OR distribution
            if has_or_at_top(&left) {
                return distribute_or_through_comparison(Arc::unwrap_or_clone(left), op, Arc::unwrap_or_clone(right), source);
            }
            if has_or_at_top(&right) {
                return distribute_or_through_comparison_right(Arc::unwrap_or_clone(left), op, Arc::unwrap_or_clone(right), source);
            }

            // Priority 2: Push into AND
            if has_and_at_top(&left) {
                return push_comparison_into_and(Arc::unwrap_or_clone(left), op, Arc::unwrap_or_clone(right), source);
            }
            if has_and_at_top(&right) {
                return push_comparison_into_and_right(Arc::unwrap_or_clone(left), op, Arc::unwrap_or_clone(right), source);
            }

            // Priority 3: Recurse into children
            let left_expanded = expand(Arc::unwrap_or_clone(left));
            let right_expanded = expand(Arc::unwrap_or_clone(right));
            
            // Priority 4: Check if recursion exposed ORs or ANDs at the top level
            // This handles nested cases where structure becomes visible after expansion
            if has_or_at_top(&left_expanded) {
                return distribute_or_through_comparison(left_expanded, op, right_expanded, source);
            }
            if has_or_at_top(&right_expanded) {
                return distribute_or_through_comparison_right(left_expanded, op, right_expanded, source);
            }
            
            // Also check for AND after recursion
            if has_and_at_top(&left_expanded) {
                return push_comparison_into_and(left_expanded, op, right_expanded, source);
            }
            if has_and_at_top(&right_expanded) {
                return push_comparison_into_and_right(left_expanded, op, right_expanded, source);
            }
            
            make_comparison(left_expanded, op, right_expanded, source)
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_expanded = expand(Arc::unwrap_or_clone(left));
            let right_expanded = expand(Arc::unwrap_or_clone(right));
            
            // Distribute OR through AND for DNF: (A ∨ B) ∧ C → (A ∧ C) ∨ (B ∧ C)
            if has_or_at_top(&left_expanded) || has_or_at_top(&right_expanded) {
                let left_branches = if has_or_at_top(&left_expanded) {
                    flatten_or(left_expanded)
                } else {
                    vec![left_expanded]
                };
                let right_branches = if has_or_at_top(&right_expanded) {
                    flatten_or(right_expanded)
                } else {
                    vec![right_expanded]
                };
                
                // Cross product: combine each left with each right
                let mut products = Vec::new();
                for l in left_branches {
                    for r in &right_branches {
                        products.push(make_and(l.clone(), r.clone(), source.clone()));
                    }
                }
                return combine_with_or(products);
            }
            
            make_and(left_expanded, right_expanded, source)
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_expanded = expand(Arc::unwrap_or_clone(left));
            let right_expanded = expand(Arc::unwrap_or_clone(right));
            make_or(left_expanded, right_expanded, source)
        }

        ExpressionKind::LogicalNegation(inner, negation_type) => {
            let inner_expanded = expand(Arc::unwrap_or_clone(inner));
            
            // Apply De Morgan's law for DNF conversion
            match &inner_expanded.kind {
                ExpressionKind::LogicalAnd(left, right) => {
                    // NOT(A AND B) → (NOT A) OR (NOT B)
                    let not_left = Expression::new(
                        ExpressionKind::LogicalNegation(left.clone(), negation_type.clone()),
                        source.clone(),
                    );
                    let not_right = Expression::new(
                        ExpressionKind::LogicalNegation(right.clone(), negation_type),
                        source.clone(),
                    );
                    expand(make_or(not_left, not_right, source))
                }
                ExpressionKind::LogicalOr(left, right) => {
                    // NOT(A OR B) → (NOT A) AND (NOT B)
                    let not_left = Expression::new(
                        ExpressionKind::LogicalNegation(left.clone(), negation_type.clone()),
                        source.clone(),
                    );
                    let not_right = Expression::new(
                        ExpressionKind::LogicalNegation(right.clone(), negation_type),
                        source.clone(),
                    );
                    expand(make_and(not_left, not_right, source))
                }
                _ => Expression::new(
                    ExpressionKind::LogicalNegation(Arc::new(inner_expanded), negation_type),
                source,
                ),
            }
        }

        ExpressionKind::UnitConversion(inner, target) => {
            let inner_expanded = expand(Arc::unwrap_or_clone(inner));
            Expression::new(
                ExpressionKind::UnitConversion(Arc::new(inner_expanded), target),
                source,
            )
        }

        ExpressionKind::MathematicalComputation(op, inner) => {
            let inner_expanded = expand(Arc::unwrap_or_clone(inner));
            Expression::new(
                ExpressionKind::MathematicalComputation(op, Arc::new(inner_expanded)),
                source,
            )
        }

        // Leaf nodes - no expansion needed
        _ => expr,
    }
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
    use crate::semantic::{EqualityNotation, FactPath, LiteralValue};
    use rust_decimal::Decimal;

    fn literal_num(n: i64) -> Expression {
        Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(n))),
            None,
        )
    }

    fn fact(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::FactPath(FactPath::local(name.to_string())),
            None,
        )
    }

    fn count_or_branches(expr: &Expression) -> usize {
        match &expr.kind {
            ExpressionKind::LogicalOr(left, right) => {
                count_or_branches(left) + count_or_branches(right)
            }
            _ => 1,
        }
    }

    fn is_or(expr: &Expression) -> bool {
        matches!(expr.kind, ExpressionKind::LogicalOr(_, _))
    }

    #[test]
    fn test_no_expansion_needed_for_simple_arithmetic() {
        let expr = make_arithmetic(
            literal_num(5),
            ArithmeticComputation::Add,
            literal_num(3),
            None,
        );
        let expanded = expand(expr.clone());
        assert!(expr.semantically_equal(&expanded));
    }

    #[test]
    fn test_distribute_or_through_arithmetic_left() {
        // (a ∨ b) + 1 → (a + 1) ∨ (b + 1)
        let a = fact("a");
        let b = fact("b");
        let a_or_b = make_or(a, b, None);
        let expr = make_arithmetic(a_or_b, ArithmeticComputation::Add, literal_num(1), None);

        let expanded = expand(expr);
        assert!(is_or(&expanded), "should produce OR expression");
        assert_eq!(count_or_branches(&expanded), 2, "should have 2 branches");
    }

    #[test]
    fn test_distribute_or_through_arithmetic_right() {
        // 1 + (a ∨ b) → (1 + a) ∨ (1 + b)
        let a = fact("a");
        let b = fact("b");
        let a_or_b = make_or(a, b, None);
        let expr = make_arithmetic(literal_num(1), ArithmeticComputation::Add, a_or_b, None);

        let expanded = expand(expr);
        assert!(is_or(&expanded), "should produce OR expression");
        assert_eq!(count_or_branches(&expanded), 2, "should have 2 branches");
    }

    #[test]
    fn test_distribute_or_through_comparison() {
        // (a ∨ b) == 5 → (a == 5) ∨ (b == 5)
        let a = fact("a");
        let b = fact("b");
        let a_or_b = make_or(a, b, None);
        let expr = make_comparison(
            a_or_b,
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            literal_num(5),
            None,
        );

        let expanded = expand(expr);
        assert!(is_or(&expanded), "should produce OR expression");
        assert_eq!(count_or_branches(&expanded), 2, "should have 2 branches");
    }

    #[test]
    fn test_nested_or_expands_via_iteration() {
        // ((a ∨ b) + 1) * 2
        // First iteration: (a + 1) ∨ (b + 1)
        // Second iteration: ((a + 1) * 2) ∨ ((b + 1) * 2)
        let a = fact("a");
        let b = fact("b");
        let a_or_b = make_or(a, b, None);
        let add = make_arithmetic(a_or_b, ArithmeticComputation::Add, literal_num(1), None);
        let mul = make_arithmetic(add, ArithmeticComputation::Multiply, literal_num(2), None);

        let expanded = expand(mul);
        assert!(is_or(&expanded), "should produce OR expression");
        assert_eq!(
            count_or_branches(&expanded),
            2,
            "should have 2 branches after full expansion"
        );
    }

    #[test]
    fn test_double_or_produces_four_branches() {
        // (a ∨ b) + (c ∨ d) should produce 4 branches
        let a = fact("a");
        let b = fact("b");
        let c = fact("c");
        let d = fact("d");

        let a_or_b = make_or(a, b, None);
        let c_or_d = make_or(c, d, None);
        let expr = make_arithmetic(a_or_b, ArithmeticComputation::Add, c_or_d, None);

        let expanded = expand(expr);
        assert!(is_or(&expanded), "should produce OR expression");
        assert_eq!(
            count_or_branches(&expanded),
            4,
            "should have 4 branches: (a+c), (a+d), (b+c), (b+d)"
        );
    }

    #[test]
    fn test_push_operator_into_and() {
        // (cond ∧ 10) + 5 → cond ∧ (10 + 5)
        let cond = fact("cond");
        let and_expr = make_and(cond, literal_num(10), None);
        let expr = make_arithmetic(and_expr, ArithmeticComputation::Add, literal_num(5), None);

        let expanded = expand(expr);
        
        // Should be AND at top level
        assert!(
            matches!(expanded.kind, ExpressionKind::LogicalAnd(_, _)),
            "should have AND at top level"
        );
    }

    #[test]
    fn test_push_comparison_into_and() {
        // (cond ∧ 10) == 5 → cond ∧ (10 == 5)
        let cond = fact("cond");
        let and_expr = make_and(cond, literal_num(10), None);
        let expr = make_comparison(
            and_expr,
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            literal_num(5),
            None,
        );

        let expanded = expand(expr);
        
        // Should be AND at top level
        assert!(
            matches!(expanded.kind, ExpressionKind::LogicalAnd(_, _)),
            "should have AND at top level"
        );
    }

    #[test]
    fn test_complex_nested_expansion() {
        // ((a ∨ b) * (c ∨ d)) == 100
        // Should produce 4 branches with comparison pushed into each
        let a = fact("a");
        let b = fact("b");
        let c = fact("c");
        let d = fact("d");

        let a_or_b = make_or(a, b, None);
        let c_or_d = make_or(c, d, None);
        let mul = make_arithmetic(a_or_b, ArithmeticComputation::Multiply, c_or_d, None);
        let expr = make_comparison(
            mul,
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            literal_num(100),
            None,
        );

        let expanded = expand(expr);
        assert!(is_or(&expanded), "should produce OR at top level");
        assert_eq!(
            count_or_branches(&expanded),
            4,
            "should have 4 branches after full expansion"
        );
    }

    #[test]
    fn test_or_with_and_branches() {
        // (c0 ∧ 10) ∨ (c1 ∧ 20) should stay as is (already in DNF)
        let c0 = fact("c0");
        let c1 = fact("c1");
        
        let branch0 = make_and(c0, literal_num(10), None);
        let branch1 = make_and(c1, literal_num(20), None);
        let expr = make_or(branch0, branch1, None);

        let expanded = expand(expr.clone());
        
        // Should be unchanged (already in DNF form)
        assert!(expr.semantically_equal(&expanded));
    }
}

