//! Equation building for execution plans
//!
//! Builds symbolic equations from rules by:
//! 1. Converting rule branches to (condition AND result) expressions
//! 2. Combining branches with OR
//! 3. Simplifying to remove redundant branches (pre-expansion)
//! 4. Expanding to DNF via distribution
//! 5. Simplifying again (post-expansion)
//!
//! This two-phase simplification drastically reduces expression complexity.
//!
//! Uses Arc<Expression> in the cache for structural sharing to avoid
//! exponential memory growth when rules reference other rules.

use crate::computation::{expand, simplification};
use crate::semantic::{
    BooleanValue, Expression, ExpressionKind, LiteralValue, RulePath,
};
use std::collections::HashMap;
use std::sync::Arc;

use super::Branch;

/// Build equations for all rules (linear pass in topological order)
///
/// Combines branches into: (cond_0 ∧ result_0) ∨ (cond_1 ∧ result_1) ∨ ...
/// Rule references in expressions are substituted with their equations from previous rules.
/// The equation is reduced algebraically at compile time through:
/// 1. Simplification (pre-expansion) - removes redundant branches from unless chains
/// 2. Expansion - distributes operators through OR to create DNF (only if has dependencies)
/// 3. Simplification (post-expansion) - minimizes the final DNF expression
///
/// Cache uses Arc<Expression> for structural sharing - when rules reference other rules,
/// we avoid deep cloning by sharing subtrees via reference counting.
pub fn build_equation(
    branches: &[Branch],
    rule_path: &RulePath,
    cache: &mut HashMap<RulePath, Arc<Expression>>,
    _has_dependencies: bool,
) -> Expression {
    // Step 1: Build raw equation from branches
    let raw_equation = build_rule_equation(branches, cache);
    
    // Step 2: Normalize the equation
    let clean_equation = simplification::reduce(raw_equation);
    
    // Step 3: Expand to DNF via algebraic distribution
    // Always expand to ensure proper DNF form - even standalone rules can have
    // complex conditions with nested ORs that need distribution
    let dnf_equation = expand(clean_equation);
    
    // Step 4: Normalize the equation for the final DNF
    let equation = simplification::reduce(dnf_equation);
    
    cache.insert(rule_path.clone(), Arc::new(equation.clone()));
    equation
}

/// Build equation for a single rule's branches
fn build_rule_equation(branches: &[Branch], cache: &HashMap<RulePath, Arc<Expression>>) -> Expression {
    if branches.is_empty() {
        return Expression::new(
            ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
            None,
        );
    }

    let branch_expressions: Vec<Expression> = branches
        .iter()
        .map(|branch| build_branch_expression(branch, cache))
        .collect();

    combine_with_or(branch_expressions)
}

/// Build expression for a single branch: (condition AND result)
fn build_branch_expression(branch: &Branch, cache: &HashMap<RulePath, Arc<Expression>>) -> Expression {
    let condition = substitute_rule_references(&branch.condition, cache);
    let result = substitute_rule_references(&branch.result, cache);

    Expression::new(
        ExpressionKind::LogicalAnd(Arc::new(condition), Arc::new(result)),
        None,
    )
}

/// Combine expressions with OR
fn combine_with_or(expressions: Vec<Expression>) -> Expression {
    if expressions.is_empty() {
        return Expression::new(
            ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
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

/// Substitute all rule references in an expression with their equations from cache
///
/// Cache uses Arc<Expression> for structural sharing. When we substitute a RulePath,
/// we dereference the Arc and clone the expression. This is still a deep clone, but
/// the Arc ensures we only store one copy of each rule's equation in the cache.
fn substitute_rule_references(
    expression: &Expression,
    cache: &HashMap<RulePath, Arc<Expression>>,
) -> Expression {
    match &expression.kind {
        ExpressionKind::RulePath(rule_path) => {
            if let Some(equation_arc) = cache.get(rule_path) {
                (**equation_arc).clone()
            } else {
                unreachable!(
                    "Rule {:?} not in cache despite topological ordering",
                    rule_path
                )
            }
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let new_left = substitute_rule_references(left, cache);
            let new_right = substitute_rule_references(right, cache);
            Expression::new(
                ExpressionKind::LogicalAnd(Arc::new(new_left), Arc::new(new_right)),
                None,
            )
        }

        ExpressionKind::LogicalOr(left, right) => {
            let new_left = substitute_rule_references(left, cache);
            let new_right = substitute_rule_references(right, cache);
            Expression::new(
                ExpressionKind::LogicalOr(Arc::new(new_left), Arc::new(new_right)),
                None,
            )
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let new_left = substitute_rule_references(left, cache);
            let new_right = substitute_rule_references(right, cache);
            Expression::new(
                ExpressionKind::Arithmetic(Arc::new(new_left), op.clone(), Arc::new(new_right)),
                None,
            )
        }

        ExpressionKind::Comparison(left, op, right) => {
            let new_left = substitute_rule_references(left, cache);
            let new_right = substitute_rule_references(right, cache);
            Expression::new(
                ExpressionKind::Comparison(Arc::new(new_left), op.clone(), Arc::new(new_right)),
                None,
            )
        }

        ExpressionKind::LogicalNegation(inner, neg_type) => {
            let new_inner = substitute_rule_references(inner, cache);
            Expression::new(
                ExpressionKind::LogicalNegation(Arc::new(new_inner), neg_type.clone()),
                None,
            )
        }

        ExpressionKind::UnitConversion(inner, target) => {
            let new_inner = substitute_rule_references(inner, cache);
            Expression::new(
                ExpressionKind::UnitConversion(Arc::new(new_inner), target.clone()),
                None,
            )
        }

        ExpressionKind::MathematicalComputation(op, inner) => {
            let new_inner = substitute_rule_references(inner, cache);
            Expression::new(
                ExpressionKind::MathematicalComputation(op.clone(), Arc::new(new_inner)),
                None,
            )
        }

        // Leaf nodes - reconstruct without source
        ExpressionKind::Literal(_)
        | ExpressionKind::FactPath(_)
        | ExpressionKind::Veto(_) => {
            Expression::new(expression.kind.clone(), None)
        }

        ExpressionKind::FactReference(_)
        | ExpressionKind::RuleReference(_) => {
            unreachable!("Fact and rule references must have been substituted in the graph")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::from(n))),
            None,
        )
    }

    fn rule_path(name: &str) -> RulePath {
        RulePath::local(name.to_string())
    }

    fn rule_path_expr(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::RulePath(RulePath::local(name.to_string())),
            None,
        )
    }

    #[test]
    fn test_build_rule_equation_single_branch() {
        let branches = vec![Branch {
            condition: literal_bool(true),
            result: literal_num(42),
            source: None,
        }];

        let cache = HashMap::new();
        let equation = build_rule_equation(&branches, &cache);

        // Should be: true AND 42
        match equation.kind {
            ExpressionKind::LogicalAnd(left, right) => {
                assert!(left.is_boolean_true());
                assert!(matches!(
                    right.kind,
                    ExpressionKind::Literal(LiteralValue::Number(_))
                ));
            }
            _ => panic!("Expected LogicalAnd"),
        }
    }

    #[test]
    fn test_build_rule_equation_multiple_branches() {
        let branches = vec![
            Branch {
                condition: literal_bool(true),
                result: literal_num(10),
                source: None,
            },
            Branch {
                condition: literal_bool(false),
                result: literal_num(20),
                source: None,
            },
        ];

        let cache = HashMap::new();
        let equation = build_rule_equation(&branches, &cache);

        // Should be: (true AND 10) OR (false AND 20)
        assert!(matches!(equation.kind, ExpressionKind::LogicalOr(_, _)));
    }

    #[test]
    fn test_substitute_rule_references() {
        let mut cache = HashMap::new();
        cache.insert(
            rule_path("dep"),
            Arc::new(Expression::new(
                ExpressionKind::LogicalAnd(
                    Arc::new(literal_bool(true)),
                    Arc::new(literal_num(100)),
                ),
                None,
            )),
        );

        let expr = rule_path_expr("dep");
        let substituted = substitute_rule_references(&expr, &cache);

        // Should be replaced with the cached equation
        assert!(matches!(substituted.kind, ExpressionKind::LogicalAnd(_, _)));
    }

    #[test]
    fn test_substitute_in_comparison() {
        let mut cache = HashMap::new();
        cache.insert(rule_path("dep"), Arc::new(literal_num(50)));

        // dep? == 50
        let expr = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(rule_path_expr("dep")),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(literal_num(50)),
            ),
            None,
        );

        let substituted = substitute_rule_references(&expr, &cache);

        // Should be: 50 == 50
        match substituted.kind {
            ExpressionKind::Comparison(left, _, right) => {
                assert!(matches!(left.kind, ExpressionKind::Literal(_)));
                assert!(matches!(right.kind, ExpressionKind::Literal(_)));
            }
            _ => panic!("Expected Comparison"),
        }
    }
}

