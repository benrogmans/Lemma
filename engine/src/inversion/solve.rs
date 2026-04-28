//! Algebraic equation solving for inversion
//!
//! Provides functions to solve equations algebraically for a single unknown data.
//! Given an expression like `price * 5` and a target value `50`, this module can
//! determine that `price = 10`.
//!
//! Supports:
//! - Addition and subtraction
//! - Multiplication and division
//! - Power operations
//! - Exponential and logarithmic functions
//! - Unit conversions

use crate::planning::semantics::{
    ArithmeticComputation, DataPath, Expression, ExpressionKind, LiteralValue,
    MathematicalComputation,
};
use std::collections::HashSet;
use std::sync::Arc;

/// Error types for algebraic solving
#[derive(Debug, Clone, PartialEq, Eq)]
enum SolveError {
    /// Unknown data appears multiple times in the expression
    UnknownAppearsMultipleTimes { data_path: DataPath, count: usize },
    /// Unsupported operation encountered
    UnsupportedOperation { description: String },
    /// Cannot isolate the unknown data algebraically
    CannotIsolateUnknown,
    /// Rule reference found (should be substituted before solving)
    RuleReferenceFound,
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::UnknownAppearsMultipleTimes { data_path, count } => {
                write!(
                    f,
                    "Unknown data '{}' appears {} times in expression",
                    data_path, count
                )
            }
            SolveError::UnsupportedOperation { description } => {
                write!(f, "Unsupported operation: {}", description)
            }
            SolveError::CannotIsolateUnknown => {
                write!(f, "Cannot isolate unknown data algebraically")
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
    /// The data that was solved for
    pub data_path: DataPath,
    /// The expression representing the solved value
    pub solved_expression: Expression,
}

/// Find all unknown data in an expression (data not in provided_data)
fn find_unknown_data(expression: &Expression, provided_data: &HashSet<DataPath>) -> Vec<DataPath> {
    let mut unknown_data = Vec::new();
    collect_unknown_data(expression, provided_data, &mut unknown_data);
    unknown_data.sort_by_key(|a| a.to_string());
    unknown_data.dedup();
    unknown_data
}

fn collect_unknown_data(
    expression: &Expression,
    provided_data: &HashSet<DataPath>,
    result: &mut Vec<DataPath>,
) {
    match &expression.kind {
        ExpressionKind::DataPath(data_path) => {
            if !provided_data.contains(data_path) {
                result.push(data_path.clone());
            }
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right) => {
            collect_unknown_data(left, provided_data, result);
            collect_unknown_data(right, provided_data, result);
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            collect_unknown_data(inner, provided_data, result);
        }
        ExpressionKind::DateRelative(_, date_expr, tolerance) => {
            collect_unknown_data(date_expr, provided_data, result);
            if let Some(tol) = tolerance {
                collect_unknown_data(tol, provided_data, result);
            }
        }
        ExpressionKind::DateCalendar(_, _, date_expr) => {
            collect_unknown_data(date_expr, provided_data, result);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::RulePath(_)
        | ExpressionKind::Now => {}
    }
}

/// Check if an expression can be solved algebraically for a specific unknown data
fn can_solve_for_data(expression: &Expression, unknown_data: &DataPath) -> bool {
    let count = count_data_occurrences(expression, unknown_data);
    if count != 1 {
        return false;
    }

    if contains_rule_reference(expression) {
        return false;
    }

    has_supported_operations(expression)
}

/// Attempt to solve an equation for a specific unknown data
fn solve_for_data(
    expression: &Expression,
    unknown_data: &DataPath,
    target: &Expression,
) -> Result<SolveResult, SolveError> {
    if contains_rule_reference(expression) {
        return Err(SolveError::RuleReferenceFound);
    }

    let count = count_data_occurrences(expression, unknown_data);
    if count == 0 {
        return Err(SolveError::CannotIsolateUnknown);
    }
    if count > 1 {
        return Err(SolveError::UnknownAppearsMultipleTimes {
            data_path: unknown_data.clone(),
            count,
        });
    }

    let solved_expression = solve_recursive(expression, unknown_data, target)?;

    Ok(SolveResult {
        data_path: unknown_data.clone(),
        solved_expression,
    })
}

/// Try to solve an expression for any single unknown data
fn try_solve_for_any_unknown(
    expression: &Expression,
    target: &Expression,
    provided_data: &HashSet<DataPath>,
) -> Option<SolveResult> {
    let unknown_data = find_unknown_data(expression, provided_data);

    for unknown_data in unknown_data {
        if can_solve_for_data(expression, &unknown_data) {
            if let Ok(result) = solve_for_data(expression, &unknown_data, target) {
                return Some(result);
            }
        }
    }

    None
}

fn solve_recursive(
    expression: &Expression,
    unknown_data: &DataPath,
    target: &Expression,
) -> Result<Expression, SolveError> {
    match &expression.kind {
        ExpressionKind::DataPath(data_path) => {
            if data_path == unknown_data {
                Ok(target.clone())
            } else {
                Err(SolveError::CannotIsolateUnknown)
            }
        }

        ExpressionKind::RulePath(_) => Err(SolveError::RuleReferenceFound),

        ExpressionKind::UnitConversion(inner, target_unit) => {
            if !contains_data(inner, unknown_data) {
                return Err(SolveError::CannotIsolateUnknown);
            }

            let solved_inner = solve_recursive(inner, unknown_data, target)?;
            Ok(Expression {
                kind: ExpressionKind::UnitConversion(Arc::new(solved_inner), target_unit.clone()),
                source_location: None,
            })
        }

        ExpressionKind::MathematicalComputation(operation, inner) => {
            if !contains_data(inner, unknown_data) {
                return Err(SolveError::CannotIsolateUnknown);
            }

            let new_target = match operation {
                MathematicalComputation::Exp => Expression {
                    kind: ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Log,
                        Arc::new(target.clone()),
                    ),
                    source_location: None,
                },
                MathematicalComputation::Log => Expression {
                    kind: ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Exp,
                        Arc::new(target.clone()),
                    ),
                    source_location: None,
                },
                other => {
                    return Err(SolveError::UnsupportedOperation {
                        description: format!("Mathematical operation {:?}", other),
                    });
                }
            };

            solve_recursive(inner, unknown_data, &new_target)
        }

        ExpressionKind::Arithmetic(left, operation, right) => {
            let left_contains = contains_data(left, unknown_data);
            let right_contains = contains_data(right, unknown_data);

            if left_contains && right_contains {
                let count = count_data_occurrences(expression, unknown_data);
                return Err(SolveError::UnknownAppearsMultipleTimes {
                    data_path: unknown_data.clone(),
                    count,
                });
            }

            if left_contains {
                let new_target = invert_operation(operation, target, right, true)?;
                solve_recursive(left, unknown_data, &new_target)
            } else if right_contains {
                let new_target = invert_operation(operation, target, left, false)?;
                solve_recursive(right, unknown_data, &new_target)
            } else {
                Err(SolveError::CannotIsolateUnknown)
            }
        }

        _ => Err(SolveError::CannotIsolateUnknown),
    }
}

/// Invert an arithmetic operation to isolate the unknown operand.
/// `target` is the desired result, `known` is the operand whose value is known.
/// `unknown_is_left`: true when the unknown is on the left side of the original operation.
///
/// For commutative ops (add, multiply): target - known / target / known
/// For non-commutative ops (subtract, divide, power): the inversion differs by side.
fn invert_operation(
    operation: &ArithmeticComputation,
    target: &Expression,
    known: &Expression,
    unknown_is_left: bool,
) -> Result<Expression, SolveError> {
    let expr = |left: Expression, op: ArithmeticComputation, right: Expression| Expression {
        kind: ExpressionKind::Arithmetic(Arc::new(left), op, Arc::new(right)),
        source_location: None,
    };

    let result = match (operation, unknown_is_left) {
        // a + b = target  =>  unknown = target - known
        (ArithmeticComputation::Add, _) => expr(
            target.clone(),
            ArithmeticComputation::Subtract,
            known.clone(),
        ),
        // unknown - known = target  =>  unknown = target + known
        (ArithmeticComputation::Subtract, true) => {
            expr(target.clone(), ArithmeticComputation::Add, known.clone())
        }
        // known - unknown = target  =>  unknown = known - target
        (ArithmeticComputation::Subtract, false) => expr(
            known.clone(),
            ArithmeticComputation::Subtract,
            target.clone(),
        ),
        // a * b = target  =>  unknown = target / known
        (ArithmeticComputation::Multiply, _) => {
            expr(target.clone(), ArithmeticComputation::Divide, known.clone())
        }
        // unknown / known = target  =>  unknown = target * known
        (ArithmeticComputation::Divide, true) => expr(
            target.clone(),
            ArithmeticComputation::Multiply,
            known.clone(),
        ),
        // known / unknown = target  =>  unknown = known / target
        (ArithmeticComputation::Divide, false) => {
            expr(known.clone(), ArithmeticComputation::Divide, target.clone())
        }
        // unknown ^ known = target  =>  unknown = target ^ (1 / known)
        (ArithmeticComputation::Power, true) => {
            let one = Expression {
                kind: ExpressionKind::Literal(Box::new(LiteralValue::number(
                    rust_decimal::Decimal::ONE,
                ))),
                source_location: None,
            };
            let inverse_exponent = expr(one, ArithmeticComputation::Divide, known.clone());
            expr(
                target.clone(),
                ArithmeticComputation::Power,
                inverse_exponent,
            )
        }
        // known ^ unknown = target  =>  unknown = log(target) / log(known)
        (ArithmeticComputation::Power, false) => {
            let log_target = Expression {
                kind: ExpressionKind::MathematicalComputation(
                    MathematicalComputation::Log,
                    Arc::new(target.clone()),
                ),
                source_location: None,
            };
            let log_known = Expression {
                kind: ExpressionKind::MathematicalComputation(
                    MathematicalComputation::Log,
                    Arc::new(known.clone()),
                ),
                source_location: None,
            };
            expr(log_target, ArithmeticComputation::Divide, log_known)
        }
        (other, _) => {
            return Err(SolveError::UnsupportedOperation {
                description: format!("Arithmetic operation {:?}", other),
            });
        }
    };

    Ok(result)
}

/// Check if expression contains a specific data path
fn contains_data(expression: &Expression, data_path: &DataPath) -> bool {
    match &expression.kind {
        ExpressionKind::DataPath(fp) => fp == data_path,
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right) => {
            contains_data(left, data_path) || contains_data(right, data_path)
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => contains_data(inner, data_path),
        _ => false,
    }
}

/// Count occurrences of a specific data path in an expression
fn count_data_occurrences(expression: &Expression, data_path: &DataPath) -> usize {
    match &expression.kind {
        ExpressionKind::DataPath(fp) => {
            if fp == data_path {
                1
            } else {
                0
            }
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right) => {
            count_data_occurrences(left, data_path) + count_data_occurrences(right, data_path)
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            count_data_occurrences(inner, data_path)
        }
        _ => 0,
    }
}

/// Check if expression contains any rule references
fn contains_rule_reference(expression: &Expression) -> bool {
    match &expression.kind {
        ExpressionKind::RulePath(_) => true,
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right) => {
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
        ExpressionKind::DataPath(_) | ExpressionKind::Literal(_) | ExpressionKind::Veto(_) => true,

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

        ExpressionKind::Comparison(left, _, right) | ExpressionKind::LogicalAnd(left, right) => {
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
        ExpressionKind::Literal(literal) => Some(literal.as_ref().clone()),
        _ => None,
    }
}

/// Solve a batch of arithmetic solutions, returning solved values and domains
///
/// For each arithmetic solution with an expression outcome, attempts to algebraically
/// solve for unknown data to determine what values produce the target.
pub(super) fn solve_arithmetic_batch(
    arithmetic_solutions: Vec<super::world::WorldArithmeticSolution>,
    target_value: &LiteralValue,
    provided_data: &HashSet<DataPath>,
) -> Vec<(
    super::world::WorldArithmeticSolution,
    LiteralValue,
    std::collections::HashMap<DataPath, super::Domain>,
)> {
    let mut results = Vec::new();

    let target_expression = Expression {
        kind: ExpressionKind::Literal(Box::new(target_value.clone())),
        source_location: None,
    };

    for arithmetic_solution in arithmetic_solutions {
        if let Some(solve_result) = try_solve_for_any_unknown(
            &arithmetic_solution.outcome_expression,
            &target_expression,
            provided_data,
        ) {
            if let Some(solved_literal) = evaluate_to_literal(&solve_result.solved_expression) {
                let mut solved_domains = std::collections::HashMap::new();
                solved_domains.insert(
                    solve_result.data_path,
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
        Expression {
            kind: ExpressionKind::Literal(Box::new(value)),
            source_location: None,
        }
    }

    fn data_expression(name: &str) -> Expression {
        Expression {
            kind: ExpressionKind::DataPath(DataPath::new(vec![], name.to_string())),
            source_location: None,
        }
    }

    fn number(n: i64) -> LiteralValue {
        LiteralValue::number(Decimal::from(n))
    }

    #[test]
    fn test_find_unknown_data() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("price")),
                ArithmeticComputation::Multiply,
                Arc::new(data_expression("quantity")),
            ),
            source_location: None,
        };

        let mut provided = HashSet::new();
        provided.insert(DataPath::new(vec![], "quantity".to_string()));

        let unknowns = find_unknown_data(&expression, &provided);
        assert_eq!(unknowns.len(), 1);
        assert_eq!(unknowns[0].data, "price");
    }

    #[test]
    fn test_can_solve_single_unknown() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("price")),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(number(5))),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "price".to_string());
        assert!(can_solve_for_data(&expression, &unknown));
    }

    #[test]
    fn test_cannot_solve_multiple_occurrences() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("price")),
                ArithmeticComputation::Add,
                Arc::new(data_expression("price")),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "price".to_string());
        assert!(!can_solve_for_data(&expression, &unknown));
    }

    #[test]
    fn test_solve_simple_multiplication() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("price")),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(number(5))),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "price".to_string());
        let target = literal_expression(number(50));

        let result = solve_for_data(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(10));
    }

    #[test]
    fn test_solve_simple_addition() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("x")),
                ArithmeticComputation::Add,
                Arc::new(literal_expression(number(10))),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "x".to_string());
        let target = literal_expression(number(25));

        let result = solve_for_data(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(15));
    }

    #[test]
    fn test_solve_simple_subtraction() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("x")),
                ArithmeticComputation::Subtract,
                Arc::new(literal_expression(number(5))),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "x".to_string());
        let target = literal_expression(number(20));

        let result = solve_for_data(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(25));
    }

    #[test]
    fn test_solve_simple_division() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("x")),
                ArithmeticComputation::Divide,
                Arc::new(literal_expression(number(2))),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "x".to_string());
        let target = literal_expression(number(10));

        let result = solve_for_data(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(20));
    }

    #[test]
    fn test_solve_chained_operations() {
        let inner = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("hours")),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(number(25))),
            ),
            source_location: None,
        };

        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(inner),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(LiteralValue::number(Decimal::new(8, 1)))),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "hours".to_string());
        let target = literal_expression(number(800));

        let result = solve_for_data(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(40));
    }

    #[test]
    fn test_solve_subtraction_unknown_on_right() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(literal_expression(number(100))),
                ArithmeticComputation::Subtract,
                Arc::new(data_expression("discount")),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "discount".to_string());
        let target = literal_expression(number(70));

        let result = solve_for_data(&expression, &unknown, &target).expect("should solve");
        let solved_value = evaluate_to_literal(&result.solved_expression).expect("should evaluate");

        assert_eq!(solved_value, number(30));
    }

    #[test]
    fn test_try_solve_for_any_unknown() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("price")),
                ArithmeticComputation::Multiply,
                Arc::new(literal_expression(number(5))),
            ),
            source_location: None,
        };

        let target = literal_expression(number(50));
        let provided = HashSet::new();

        let result = try_solve_for_any_unknown(&expression, &target, &provided);
        assert!(result.is_some());

        let solve_result = result.unwrap();
        assert_eq!(solve_result.data_path.data, "price");

        let solved_value =
            evaluate_to_literal(&solve_result.solved_expression).expect("should evaluate");
        assert_eq!(solved_value, number(10));
    }

    #[test]
    fn test_error_multiple_occurrences() {
        let expression = Expression {
            kind: ExpressionKind::Arithmetic(
                Arc::new(data_expression("x")),
                ArithmeticComputation::Add,
                Arc::new(data_expression("x")),
            ),
            source_location: None,
        };

        let unknown = DataPath::new(vec![], "x".to_string());
        let target = literal_expression(number(20));

        let result = solve_for_data(&expression, &unknown, &target);
        assert!(matches!(
            result,
            Err(SolveError::UnknownAppearsMultipleTimes { count: 2, .. })
        ));
    }
}
