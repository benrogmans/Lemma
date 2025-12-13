//! Equation building for execution plans
//!
//! Builds symbolic equations from rules by:
//! 1. Converting rule branches to (condition AND result) expressions
//! 2. Combining branches with OR
//! 3. Substituting rule references with their equations
//! 4. Reducing algebraically (compile-time optimization)

use crate::computation::expand;
use crate::semantic::{
    BooleanValue, Expression, ExpressionKind, LiteralValue, PathSegment, RulePath,
};
use std::collections::HashMap;

use super::Branch;

/// Build equations for all rules (linear pass in topological order)
///
/// Combines branches into: (cond_0 ∧ result_0) ∨ (cond_1 ∧ result_1) ∨ ...
/// Rule references in expressions are substituted with their equations from previous rules.
/// The equation is reduced algebraically at compile time.
pub fn build_equation(
    branches: &[Branch],
    rule_path: &RulePath,
    cache: &mut HashMap<RulePath, Expression>,
) -> Expression {
    let raw_equation = build_rule_equation(branches, cache);
    let equation = expand(raw_equation);
    cache.insert(rule_path.clone(), equation.clone());
    equation
}

/// Build equation for a single rule's branches
fn build_rule_equation(branches: &[Branch], cache: &HashMap<RulePath, Expression>) -> Expression {
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
fn build_branch_expression(branch: &Branch, cache: &HashMap<RulePath, Expression>) -> Expression {
    let condition = substitute_rule_references(&branch.condition, cache);
    let result = substitute_rule_references(&branch.result, cache);

    Expression::new(
        ExpressionKind::LogicalAnd(Box::new(condition), Box::new(result)),
        branch.source.clone(),
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
            ExpressionKind::LogicalOr(Box::new(acc), Box::new(expr)),
            None,
        )
    })
}

/// Substitute all rule references in an expression with their equations from cache
fn substitute_rule_references(
    expression: &Expression,
    cache: &HashMap<RulePath, Expression>,
) -> Expression {
    match &expression.kind {
        ExpressionKind::RulePath(rule_path) => {
            if let Some(equation) = cache.get(rule_path) {
                equation.clone()
            } else {
                unreachable!(
                    "Rule {:?} not in cache despite topological ordering",
                    rule_path
                )
            }
        }

        ExpressionKind::RuleReference(rule_ref) => {
            let rule_path = RulePath {
                segments: rule_ref
                    .segments
                    .iter()
                    .map(|s| PathSegment {
                        fact: s.clone(),
                        doc: String::new(),
                    })
                    .collect(),
                rule: rule_ref.rule.clone(),
            };
            if let Some(equation) = cache.get(&rule_path) {
                equation.clone()
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
                ExpressionKind::LogicalAnd(Box::new(new_left), Box::new(new_right)),
                expression.source.clone(),
            )
        }

        ExpressionKind::LogicalOr(left, right) => {
            let new_left = substitute_rule_references(left, cache);
            let new_right = substitute_rule_references(right, cache);
            Expression::new(
                ExpressionKind::LogicalOr(Box::new(new_left), Box::new(new_right)),
                expression.source.clone(),
            )
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let new_left = substitute_rule_references(left, cache);
            let new_right = substitute_rule_references(right, cache);
            Expression::new(
                ExpressionKind::Arithmetic(Box::new(new_left), op.clone(), Box::new(new_right)),
                expression.source.clone(),
            )
        }

        ExpressionKind::Comparison(left, op, right) => {
            let new_left = substitute_rule_references(left, cache);
            let new_right = substitute_rule_references(right, cache);
            Expression::new(
                ExpressionKind::Comparison(Box::new(new_left), op.clone(), Box::new(new_right)),
                expression.source.clone(),
            )
        }

        ExpressionKind::LogicalNegation(inner, neg_type) => {
            let new_inner = substitute_rule_references(inner, cache);
            Expression::new(
                ExpressionKind::LogicalNegation(Box::new(new_inner), neg_type.clone()),
                expression.source.clone(),
            )
        }

        ExpressionKind::UnitConversion(inner, target) => {
            let new_inner = substitute_rule_references(inner, cache);
            Expression::new(
                ExpressionKind::UnitConversion(Box::new(new_inner), target.clone()),
                expression.source.clone(),
            )
        }

        ExpressionKind::MathematicalComputation(op, inner) => {
            let new_inner = substitute_rule_references(inner, cache);
            Expression::new(
                ExpressionKind::MathematicalComputation(op.clone(), Box::new(new_inner)),
                expression.source.clone(),
            )
        }

        // Leaf nodes - no substitution needed
        ExpressionKind::Literal(_)
        | ExpressionKind::FactPath(_)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::Veto(_) => expression.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::RuleReference;

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

    fn rule_ref_expr(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::RuleReference(RuleReference {
                segments: vec![],
                rule: name.to_string(),
            }),
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
            Expression::new(
                ExpressionKind::LogicalAnd(
                    Box::new(literal_bool(true)),
                    Box::new(literal_num(100)),
                ),
                None,
            ),
        );

        let expr = rule_ref_expr("dep");
        let substituted = substitute_rule_references(&expr, &cache);

        // Should be replaced with the cached equation
        assert!(matches!(substituted.kind, ExpressionKind::LogicalAnd(_, _)));
    }

    #[test]
    fn test_substitute_in_comparison() {
        let mut cache = HashMap::new();
        cache.insert(rule_path("dep"), literal_num(50));

        // dep? == 50
        let expr = Expression::new(
            ExpressionKind::Comparison(
                Box::new(rule_ref_expr("dep")),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Box::new(literal_num(50)),
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

