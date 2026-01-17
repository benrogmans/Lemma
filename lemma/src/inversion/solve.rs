//! Algebraic equation solving for inversion
//!
//! Provides functions to solve equations algebraically for a single unknown fact.
//! Given an expression like `price * 5` and a target value `50`, this module can
//! determine that `price = 10`.
//!
//! Supports:
//! - Addition and subtraction
//! - Multiplication and division
//! - Power operations
//! - Exponential and logarithmic functions
//! - Unit conversions

use crate::{
    ArithmeticComputation, Expression, ExpressionKind, FactPath, LiteralValue,
    MathematicalComputation,
};
use std::collections::HashSet;
use std::sync::Arc;

/// Error types for algebraic solving
#[derive(Debug, Clone, PartialEq, Eq)]
enum SolveError {
    /// Unknown fact appears multiple times in the expression
    UnknownAppearsMultipleTimes { fact_path: FactPath, count: usize },
    /// Unsupported operation encountered
    UnsupportedOperation { description: String },
    /// Cannot isolate the unknown fact algebraically
    CannotIsolateUnknown,
    /// Rule reference found (should be substituted before solving)
    RuleReferenceFound,
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::UnknownAppearsMultipleTimes { fact_path, count } => {
                write!(
                    f,
                    "Unknown fact '{}' appears {} times in expression",
                    fact_path, count
                )
            }
            SolveError::UnsupportedOperation { description } => {
                write!(f, "Unsupported operation: {}", description)
            }
            SolveError::CannotIsolateUnknown => {
                write!(f, "Cannot isolate unknown fact algebraically")
            }
            SolveError::RuleReferenceFound => {
                write!(
                    f,
                    "Rule reference found - should be substituted before solving"
                )
            }
        }
    }
}

/// Result of algebraic solving
#[derive(Debug, Clone)]
struct SolveResult {
    /// The fact that was solved for
    pub fact_path: FactPath,
    /// The expression representing the solved value
    pub solved_expression: Expression,
}

/// Find all unknown facts in an expression (facts not in provided_facts)
fn find_unknown_facts(
    expression: &Expression,
    provided_facts: &HashSet<FactPath>,
) -> Vec<FactPath> {
    let mut unknown_facts = Vec::new();
    collect_unknown_facts(expression, provided_facts, &mut unknown_facts);
    unknown_facts.sort_by_key(|a| a.to_string());
    unknown_facts.dedup();
    unknown_facts
}

fn collect_unknown_facts(
    expression: &Expression,
    provided_facts: &HashSet<FactPath>,
    result: &mut Vec<FactPath>,
) {
    match &expression.kind {
        ExpressionKind::FactPath(fact_path) => {
            if !provided_facts.contains(fact_path) {
                result.push(fact_path.clone());
            }
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            collect_unknown_facts(left, provided_facts, result);
            collect_unknown_facts(right, provided_facts, result);
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            collect_unknown_facts(inner, provided_facts, result);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Reference(_)
        | ExpressionKind::UnresolvedUnitLiteral(_, _)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::RuleReference(_)
        | ExpressionKind::RulePath(_) => {}
    }
}

/// Check if an expression can be solved algebraically for a specific unknown fact
fn can_solve_for_fact(expression: &Expression, unknown_fact: &FactPath) -> bool {
    let count = count_fact_occurrences(expression, unknown_fact);
    if count != 1 {
        return false;
    }

    if contains_rule_reference(expression) {
        return false;
    }

    has_supported_operations(expression)
}

/// Attempt to solve an equation for a specific unknown fact
fn solve_for_fact(
    expression: &Expression,
    unknown_fact: &FactPath,
    target: &Expression,
) -> Result<SolveResult, SolveError> {
    if contains_rule_reference(expression) {
        return Err(SolveError::RuleReferenceFound);
    }

    let count = count_fact_occurrences(expression, unknown_fact);
    if count == 0 {
        return Err(SolveError::CannotIsolateUnknown);
    }
    if count > 1 {
        return Err(SolveError::UnknownAppearsMultipleTimes {
            fact_path: unknown_fact.clone(),
            count,
        });
    }

    let solved_expression = solve_recursive(expression, unknown_fact, target)?;

    Ok(SolveResult {
        fact_path: unknown_fact.clone(),
        solved_expression,
    })
}

/// Try to solve an expression for any single unknown fact
fn try_solve_for_any_unknown(
    expression: &Expression,
    target: &Expression,
    provided_facts: &HashSet<FactPath>,
) -> Option<SolveResult> {
    let unknown_facts = find_unknown_facts(expression, provided_facts);

    for unknown_fact in unknown_facts {
        if can_solve_for_fact(expression, &unknown_fact) {
            if let Ok(result) = solve_for_fact(expression, &unknown_fact, target) {
                return Some(result);
            }
        }
    }

    None
}

fn solve_recursive(
    expression: &Expression,
    unknown_fact: &FactPath,
    target: &Expression,
) -> Result<Expression, SolveError> {
    match &expression.kind {
        ExpressionKind::FactPath(fact_path) => {
            if fact_path == unknown_fact {
                Ok(target.clone())
            } else {
                Err(SolveError::CannotIsolateUnknown)
            }
        }

        ExpressionKind::FactReference(_) => Err(SolveError::CannotIsolateUnknown),

        ExpressionKind::RuleReference(_) | ExpressionKind::RulePath(_) => {
            Err(SolveError::RuleReferenceFound)
        }

        ExpressionKind::UnitConversion(inner, target_unit) => {
            if !contains_fact(inner, unknown_fact) {
                return Err(SolveError::CannotIsolateUnknown);
            }

            let solved_inner = solve_recursive(inner, unknown_fact, target)?;
            Ok(Expression::new(
                ExpressionKind::UnitConversion(Arc::new(solved_inner), target_unit.clone()),
                expression.source_location.clone(),
            ))
        }

        ExpressionKind::MathematicalComputation(operation, inner) => {
            if !contains_fact(inner, unknown_fact) {
                return Err(SolveError::CannotIsolateUnknown);
            }

            let new_target = match operation {
                MathematicalComputation::Exp => Expression::new(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Log,
                        Arc::new(target.clone()),
                    ),
                    expression.source_location.clone(),
                ),
                MathematicalComputation::Log => Expression::new(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Exp,
                        Arc::new(target.clone()),
                    ),
                    expression.source_location.clone(),
                ),
                other => {
                    return Err(SolveError::UnsupportedOperation {
                        description: format!("Mathematical operation {:?}", other),
                    });
                }
            };

            solve_recursive(inner, unknown_fact, &new_target)
        }

        ExpressionKind::Arithmetic(left, operation, right) => {
            let left_contains = contains_fact(left, unknown_fact);
            let right_contains = contains_fact(right, unknown_fact);

            if left_contains && right_contains {
                let count = count_fact_occurrences(expression, unknown_fact);
                return Err(SolveError::UnknownAppearsMultipleTimes {
                    fact_path: unknown_fact.clone(),
                    count,
                });
            }

            if left_contains {
                solve_left_operand(expression, left, operation, right, unknown_fact, target)
            } else if right_contains {
                solve_right_operand(expression, left, operation, right, unknown_fact, target)
            } else {
                Err(SolveError::CannotIsolateUnknown)
            }
        }

        _ => Err(SolveError::CannotIsolateUnknown),
    }
}

fn solve_left_operand(
    expression: &Expression,
    left: &Expression,
    operation: &ArithmeticComputation,
    right: &Expression,
    unknown_fact: &FactPath,
    target: &Expression,
) -> Result<Expression, SolveError> {
    let new_target = match operation {
        ArithmeticComputation::Add => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(target.clone()),
                ArithmeticComputation::Subtract,
                Arc::new(right.clone()),
            ),
            expression.source_location.clone(),
        ),
        ArithmeticComputation::Subtract => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(target.clone()),
                ArithmeticComputation::Add,
                Arc::new(right.clone()),
            ),
            expression.source_location.clone(),
        ),
        ArithmeticComputation::Multiply => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(target.clone()),
                ArithmeticComputation::Divide,
                Arc::new(right.clone()),
            ),
            expression.source_location.clone(),
        ),
        ArithmeticComputation::Divide => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(target.clone()),
                ArithmeticComputation::Multiply,
                Arc::new(right.clone()),
            ),
            expression.source_location.clone(),
        ),
        ArithmeticComputation::Power => {
            let one = Expression::new(
                ExpressionKind::Literal(LiteralValue::number(rust_decimal::Decimal::ONE)),
                expression.source_location.clone(),
            );
            let inverse_exponent = Expression::new(
                ExpressionKind::Arithmetic(
                    Arc::new(one),
                    ArithmeticComputation::Divide,
                    Arc::new(right.clone()),
                ),
                expression.source_location.clone(),
            );
            Expression::new(
                ExpressionKind::Arithmetic(
                    Arc::new(target.clone()),
                    ArithmeticComputation::Power,
                    Arc::new(inverse_exponent),
                ),
                expression.source_location.clone(),
            )
        }
        other => {
            return Err(SolveError::UnsupportedOperation {
                description: format!("Arithmetic operation {:?}", other),
            });
        }
    };

    solve_recursive(left, unknown_fact, &new_target)
}

fn solve_right_operand(
    expression: &Expression,
    left: &Expression,
    operation: &ArithmeticComputation,
    right: &Expression,
    unknown_fact: &FactPath,
    target: &Expression,
) -> Result<Expression, SolveError> {
    let new_target = match operation {
        ArithmeticComputation::Add => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(target.clone()),
                ArithmeticComputation::Subtract,
                Arc::new(left.clone()),
            ),
            expression.source_location.clone(),
        ),
        ArithmeticComputation::Subtract => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(left.clone()),
                ArithmeticComputation::Subtract,
                Arc::new(target.clone()),
            ),
            expression.source_location.clone(),
        ),
        ArithmeticComputation::Multiply => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(target.clone()),
                ArithmeticComputation::Divide,
                Arc::new(left.clone()),
            ),
            expression.source_location.clone(),
        ),
        ArithmeticComputation::Divide => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(left.clone()),
                ArithmeticComputation::Divide,
                Arc::new(target.clone()),
            ),
            expression.source_location.clone(),
        ),
        ArithmeticComputation::Power => {
            let numerator = Expression::new(
                ExpressionKind::MathematicalComputation(
                    MathematicalComputation::Log,
                    Arc::new(target.clone()),
                ),
                expression.source_location.clone(),
            );
            let denominator = Expression::new(
                ExpressionKind::MathematicalComputation(
                    MathematicalComputation::Log,
                    Arc::new(left.clone()),
                ),
                expression.source_location.clone(),
            );
            Expression::new(
                ExpressionKind::Arithmetic(
                    Arc::new(numerator),
                    ArithmeticComputation::Divide,
                    Arc::new(denominator),
                ),
                expression.source_location.clone(),
            )
        }
        other => {
            return Err(SolveError::UnsupportedOperation {
                description: format!("Arithmetic operation {:?}", other),
            });
        }
    };

    solve_recursive(right, unknown_fact, &new_target)
}

/// Check if expression contains a specific fact path
fn contains_fact(expression: &Expression, fact_path: &FactPath) -> bool {
    match &expression.kind {
        ExpressionKind::FactPath(fp) => fp == fact_path,
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            contains_fact(left, fact_path) || contains_fact(right, fact_path)
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => contains_fact(inner, fact_path),
        _ => false,
    }
}

/// Count occurrences of a specific fact path in an expression
fn count_fact_occurrences(expression: &Expression, fact_path: &FactPath) -> usize {
    match &expression.kind {
        ExpressionKind::FactPath(fp) => {
            if fp == fact_path {
                1
            } else {
                0
            }
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            count_fact_occurrences(left, fact_path) + count_fact_occurrences(right, fact_path)
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            count_fact_occurrences(inner, fact_path)
        }
        _ => 0,
    }
}

/// Check if expression contains any rule references
fn contains_rule_reference(expression: &Expression) -> bool {
    match &expression.kind {
        ExpressionKind::RuleReference(_) | ExpressionKind::RulePath(_) => true,
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            contains_rule_reference(left) || contains_rule_reference(right)
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => contains_rule_reference(inner),
        _ => false,
    }
}

/// Check if expression only contains operations supported by the solver
fn has_supported_operations(expression: &Expression) -> bool {
    match &expression.kind {
        ExpressionKind::FactPath(_)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::Literal(_)
        | ExpressionKind::Veto(_) => true,

        ExpressionKind::Arithmetic(left, operation, right) => {
            let supported_operation = matches!(
                operation,
                ArithmeticComputation::Add
                    | ArithmeticComputation::Subtract
                    | ArithmeticComputation::Multiply
                    | ArithmeticComputation::Divide
                    | ArithmeticComputation::Power
            );
            supported_operation && has_supported_operations(left) && has_supported_operations(right)
        }

        ExpressionKind::MathematicalComputation(operation, inner) => {
            let supported_operation = matches!(
                operation,
                MathematicalComputation::Exp | MathematicalComputation::Log
            );
            supported_operation && has_supported_operations(inner)
        }

        ExpressionKind::UnitConversion(inner, _) => has_supported_operations(inner),

        ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            has_supported_operations(left) && has_supported_operations(right)
        }

        ExpressionKind::LogicalNegation(inner, _) => has_supported_operations(inner),

        _ => false,
    }
}

/// Evaluate a solved expression to a literal value if possible
fn evaluate_to_literal(expression: &Expression) -> Option<LiteralValue> {
    let folded = super::world::try_constant_fold_expression(expression)?;
    match folded.kind {
        ExpressionKind::Literal(literal) => Some(literal),
        _ => None,
    }
}

/// Solve a batch of arithmetic solutions, returning solved values and domains
///
/// For each arithmetic solution with an expression outcome, attempts to algebraically
/// solve for unknown facts to determine what values produce the target.
pub fn solve_arithmetic_batch(
    arithmetic_solutions: Vec<super::world::WorldArithmeticSolution>,
    target_value: &LiteralValue,
    provided_facts: &HashSet<FactPath>,
) -> Vec<(
    super::world::WorldArithmeticSolution,
    LiteralValue,
    std::collections::HashMap<FactPath, super::Domain>,
)> {
    let mut results = Vec::new();

    let target_expression = Expression::new(ExpressionKind::Literal(target_value.clone()), None);

    for arithmetic_solution in arithmetic_solutions {
        if let Some(solve_result) = try_solve_for_any_unknown(
            &arithmetic_solution.outcome_expression,
            &target_expression,
            provided_facts,
        ) {
            if let Some(solved_literal) = evaluate_to_literal(&solve_result.solved_expression) {
                let mut solved_domains = std::collections::HashMap::new();
                solved_domains.insert(
                    solve_result.fact_path,
                    super::Domain::Enumeration(Arc::new(vec![solved_literal.clone()])),
                );

                results.push((arithmetic_solution, target_value.clone(), solved_domains));
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn literal_expression(value: LiteralValue) -> Expression {
        Expression::new(ExpressionKind::Literal(value), None)
    }

    fn fact_expression(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::FactPath(FactPath::local(name.to_string())),
            None,
        )
    }

    fn number(n: i64) -> LiteralValue {
        LiteralValue::number(Decimal::from(n))
    }

    #[test]
    fn test_find_unknown_facts() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("price")),
                ArithmeticComputation::Multiply,
                Arc::new(fact_expression("quantity")),
            ),
            None,
        );

        let mut provided = HashSet::new();
        provided.insert(FactPath::local("quantity".to_string()));

        let unknowns = find_unknown_facts(&expression, &provided);
        assert_eq!(unknowns.len(), 1);
        assert_eq!(unknowns[0].fact, "price");
    }

    #[test]
    fn test_can_solve_single_unknown() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("price")),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(number(5))),
            ),
            None,
        );

        let unknown = FactPath::local("price".to_string());
        assert!(can_solve_for_fact(&expression, &unknown));
    }

    #[test]
    fn test_cannot_solve_multiple_occurrences() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("price")),
                ArithmeticComputation::Add,
                Arc::new(fact_expression("price")),
            ),
            None,
        );

        let unknown = FactPath::local("price".to_string());
        assert!(!can_solve_for_fact(&expression, &unknown));
    }

    #[test]
    fn test_solve_simple_multiplication() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("price")),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(number(5))),
            ),
            None,
        );

        let unknown = FactPath::local("price".to_string());
        let target = literal_expression(number(50));

        let result = solve_for_fact(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(10));
    }

    #[test]
    fn test_solve_simple_addition() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("x")),
                ArithmeticComputation::Add,
                Arc::new(literal_expression(number(10))),
            ),
            None,
        );

        let unknown = FactPath::local("x".to_string());
        let target = literal_expression(number(25));

        let result = solve_for_fact(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(15));
    }

    #[test]
    fn test_solve_simple_subtraction() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("x")),
                ArithmeticComputation::Subtract,
                Arc::new(literal_expression(number(5))),
            ),
            None,
        );

        let unknown = FactPath::local("x".to_string());
        let target = literal_expression(number(20));

        let result = solve_for_fact(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(25));
    }

    #[test]
    fn test_solve_simple_division() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("x")),
                ArithmeticComputation::Divide,
                Arc::new(literal_expression(number(2))),
            ),
            None,
        );

        let unknown = FactPath::local("x".to_string());
        let target = literal_expression(number(10));

        let result = solve_for_fact(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(20));
    }

    #[test]
    fn test_solve_chained_operations() {
        let inner = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("hours")),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(number(25))),
            ),
            None,
        );

        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(inner),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(LiteralValue::number(Decimal::new(8, 1)))),
            ),
            None,
        );

        let unknown = FactPath::local("hours".to_string());
        let target = literal_expression(number(800));

        let result = solve_for_fact(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(40));
    }

    #[test]
    fn test_solve_right_operand_subtraction() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(literal_expression(number(100))),
                ArithmeticComputation::Subtract,
                Arc::new(fact_expression("discount")),
            ),
            None,
        );

        let unknown = FactPath::local("discount".to_string());
        let target = literal_expression(number(70));

        let result = solve_for_fact(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(30));
    }

    #[test]
    fn test_try_solve_for_any_unknown() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("price")),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(number(5))),
            ),
            None,
        );

        let target = literal_expression(number(50));
        let provided = HashSet::new();

        let result = try_solve_for_any_unknown(&expression, &target, &provided);
        assert!(result.is_some());

        let solve_result = result.unwrap();
        assert_eq!(solve_result.fact_path.fact, "price");

        let solved_value =
            evaluate_to_literal(&solve_result.solved_expression).expect("should evaluate");
        assert_eq!(solved_value, number(10));
    }

    #[test]
    fn test_error_multiple_occurrences() {
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(fact_expression("x")),
                ArithmeticComputation::Add,
                Arc::new(fact_expression("x")),
            ),
            None,
        );

        let unknown = FactPath::local("x".to_string());
        let target = literal_expression(number(20));

        let result = solve_for_fact(&expression, &unknown, &target);
        assert!(matches!(
            result,
            Err(SolveError::UnknownAppearsMultipleTimes { count: 2, .. })
        ));
    }
}
