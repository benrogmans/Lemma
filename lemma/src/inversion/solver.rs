//! Algebraic and boolean expression solving
//!
//! Contains:
//! - Algebraic equation solving for single unknowns
//! - Boolean expression simplification using BDDs

use crate::parsing::ast::ExpressionId;
use crate::{Expression, ExpressionKind, FactPath, LiteralValue};

use super::expansion;

/// Error types for algebraic solving
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveError {
    /// Unknown fact appears multiple times in the expression
    UnknownAppearsMultipleTimes(usize),
    /// Unsupported operation encountered
    UnsupportedOperation(String),
    /// Cannot isolate the unknown fact algebraically
    CannotIsolateUnknown,
    /// Rule reference found (should never happen after Phase 1 expansion)
    RuleReferenceFound,
}

/// Check if an expression can be solved algebraically for a single unknown
///
/// Returns true if:
/// - Unknown appears exactly once
/// - Expression contains no rule references (defensive check)
/// - All operations are supported by algebraic_solve()
pub fn can_algebraically_solve(
    expr: &Expression,
    unknown: &(String, String),
    fact_matcher: &impl Fn(&FactPath, &str, &str) -> bool,
) -> bool {
    let count = count_unknown_occurrences(expr, unknown, fact_matcher);
    if count != 1 {
        return false;
    }

    if contains_rule_reference(expr) {
        return false;
    }

    has_supported_operations(expr)
}

/// Check if expression contains any rule references (defensive check)
fn contains_rule_reference(expr: &Expression) -> bool {
    match &expr.kind {
        ExpressionKind::RuleReference(_) | ExpressionKind::RulePath(_) => true,
        ExpressionKind::Arithmetic(l, _, r)
        | ExpressionKind::LogicalAnd(l, r)
        | ExpressionKind::LogicalOr(l, r)
        | ExpressionKind::Comparison(l, _, r) => {
            contains_rule_reference(l) || contains_rule_reference(r)
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => contains_rule_reference(inner),
        _ => false,
    }
}

/// Check if expression only contains operations supported by algebraic_solve
fn has_supported_operations(expr: &Expression) -> bool {
    match &expr.kind {
        ExpressionKind::FactPath(_)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::Literal(_)
        | ExpressionKind::Veto(_) => true,
        ExpressionKind::Arithmetic(l, op, r) => {
            matches!(
                op,
                crate::ArithmeticComputation::Add
                    | crate::ArithmeticComputation::Subtract
                    | crate::ArithmeticComputation::Multiply
                    | crate::ArithmeticComputation::Divide
                    | crate::ArithmeticComputation::Power
            ) && has_supported_operations(l)
                && has_supported_operations(r)
        }
        ExpressionKind::MathematicalComputation(op, inner) => {
            matches!(
                op,
                crate::MathematicalComputation::Exp | crate::MathematicalComputation::Log
            ) && has_supported_operations(inner)
        }
        ExpressionKind::UnitConversion(inner, _) => has_supported_operations(inner),
        ExpressionKind::LogicalAnd(l, r)
        | ExpressionKind::LogicalOr(l, r)
        | ExpressionKind::Comparison(l, _, r) => {
            has_supported_operations(l) && has_supported_operations(r)
        }
        ExpressionKind::LogicalNegation(inner, _) => has_supported_operations(inner),
        _ => false,
    }
}

/// Attempt to solve an equation algebraically for a single unknown fact
///
/// Given an expression containing an unknown fact and a target value,
/// attempts to rearrange the equation to isolate the unknown.
///
/// Supports: +, -, *, /, ^ (power), exp, log, unit conversions
///
/// Returns Err if:
/// - The unknown appears multiple times (can't isolate)
/// - Unsupported operations are used
/// - The equation cannot be algebraically rearranged
/// - Rule references are found (defensive check)
pub fn algebraic_solve(
    expr: &Expression,
    unknown: &(String, String),
    target: &Expression,
    fact_matcher: &impl Fn(&FactPath, &str, &str) -> bool,
) -> Result<Expression, SolveError> {
    if contains_rule_reference(expr) {
        return Err(SolveError::RuleReferenceFound);
    }

    match &expr.kind {
        ExpressionKind::FactPath(fp) => {
            if fact_matcher(fp, &unknown.0, &unknown.1) {
                return Ok(target.clone());
            }
            Err(SolveError::CannotIsolateUnknown)
        }
        ExpressionKind::FactReference(_) => Err(SolveError::CannotIsolateUnknown),
        ExpressionKind::RuleReference(_) | ExpressionKind::RulePath(_) => {
            Err(SolveError::RuleReferenceFound)
        }
        ExpressionKind::UnitConversion(inner, target_unit) => {
            if !contains_unknown(inner, unknown, fact_matcher) {
                return Err(SolveError::CannotIsolateUnknown);
            }

            let solved_inner = algebraic_solve(inner, unknown, target, fact_matcher)?;
            Ok(Expression::new(
                ExpressionKind::UnitConversion(Box::new(solved_inner), target_unit.clone()),
                None,
                ExpressionId::new(0),
            ))
        }
        ExpressionKind::MathematicalComputation(op, inner) => {
            use crate::MathematicalComputation;
            if !contains_unknown(inner, unknown, fact_matcher) {
                return Err(SolveError::CannotIsolateUnknown);
            }

            let new_target = match op {
                MathematicalComputation::Exp => Expression::new(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Log,
                        Box::new(target.clone()),
                    ),
                    None,
                    ExpressionId::new(0),
                ),
                MathematicalComputation::Log => Expression::new(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Exp,
                        Box::new(target.clone()),
                    ),
                    None,
                    ExpressionId::new(0),
                ),
                _ => {
                    return Err(SolveError::UnsupportedOperation(format!(
                        "Mathematical operation {:?}",
                        op
                    )));
                }
            };

            algebraic_solve(inner, unknown, &new_target, fact_matcher)
        }
        ExpressionKind::Arithmetic(l, op, r) => {
            let l_contains = contains_unknown(l, unknown, fact_matcher);
            let r_contains = contains_unknown(r, unknown, fact_matcher);

            if l_contains && !r_contains {
                let new_target = match op {
                    crate::ArithmeticComputation::Add => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new(target.clone()),
                            crate::ArithmeticComputation::Subtract,
                            Box::new((**r).clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Subtract => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new(target.clone()),
                            crate::ArithmeticComputation::Add,
                            Box::new((**r).clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Multiply => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new(target.clone()),
                            crate::ArithmeticComputation::Divide,
                            Box::new((**r).clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Divide => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new(target.clone()),
                            crate::ArithmeticComputation::Multiply,
                            Box::new((**r).clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Power => {
                        let one = Expression::new(
                            ExpressionKind::Literal(LiteralValue::Number(
                                rust_decimal::Decimal::ONE,
                            )),
                            None,
                            ExpressionId::new(0),
                        );
                        let inv_exp = Expression::new(
                            ExpressionKind::Arithmetic(
                                Box::new(one),
                                crate::ArithmeticComputation::Divide,
                                Box::new((**r).clone()),
                            ),
                            None,
                            ExpressionId::new(0),
                        );
                        Expression::new(
                            ExpressionKind::Arithmetic(
                                Box::new(target.clone()),
                                crate::ArithmeticComputation::Power,
                                Box::new(inv_exp),
                            ),
                            None,
                            ExpressionId::new(0),
                        )
                    }
                    _ => {
                        return Err(SolveError::UnsupportedOperation(format!(
                            "Arithmetic operation {:?}",
                            op
                        )));
                    }
                };
                algebraic_solve(l, unknown, &new_target, fact_matcher)
            } else if r_contains && !l_contains {
                let new_target = match op {
                    crate::ArithmeticComputation::Add => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new(target.clone()),
                            crate::ArithmeticComputation::Subtract,
                            Box::new((**l).clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Subtract => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new((**l).clone()),
                            crate::ArithmeticComputation::Subtract,
                            Box::new(target.clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Multiply => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new(target.clone()),
                            crate::ArithmeticComputation::Divide,
                            Box::new((**l).clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Divide => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new((**l).clone()),
                            crate::ArithmeticComputation::Divide,
                            Box::new(target.clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Power => {
                        let num = Expression::new(
                            ExpressionKind::MathematicalComputation(
                                crate::MathematicalComputation::Log,
                                Box::new(target.clone()),
                            ),
                            None,
                            ExpressionId::new(0),
                        );
                        let den = Expression::new(
                            ExpressionKind::MathematicalComputation(
                                crate::MathematicalComputation::Log,
                                Box::new((**l).clone()),
                            ),
                            None,
                            ExpressionId::new(0),
                        );
                        Expression::new(
                            ExpressionKind::Arithmetic(
                                Box::new(num),
                                crate::ArithmeticComputation::Divide,
                                Box::new(den),
                            ),
                            None,
                            ExpressionId::new(0),
                        )
                    }
                    _ => {
                        return Err(SolveError::UnsupportedOperation(format!(
                            "Arithmetic operation {:?}",
                            op
                        )));
                    }
                };
                algebraic_solve(r, unknown, &new_target, fact_matcher)
            } else if l_contains && r_contains {
                let count = count_unknown_occurrences(expr, unknown, fact_matcher);
                Err(SolveError::UnknownAppearsMultipleTimes(count))
            } else {
                Err(SolveError::CannotIsolateUnknown)
            }
        }
        _ => Err(SolveError::CannotIsolateUnknown),
    }
}

/// Check if an expression contains a reference to an unknown fact
pub fn contains_unknown(
    expr: &Expression,
    unknown: &(String, String),
    fact_matcher: &impl Fn(&FactPath, &str, &str) -> bool,
) -> bool {
    match &expr.kind {
        ExpressionKind::FactPath(fp) => fact_matcher(fp, &unknown.0, &unknown.1),
        ExpressionKind::FactReference(_) => false,
        ExpressionKind::Arithmetic(l, _, r)
        | ExpressionKind::LogicalAnd(l, r)
        | ExpressionKind::LogicalOr(l, r)
        | ExpressionKind::Comparison(l, _, r) => {
            contains_unknown(l, unknown, fact_matcher) || contains_unknown(r, unknown, fact_matcher)
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            contains_unknown(inner, unknown, fact_matcher)
        }
        _ => false,
    }
}

/// Count how many times an unknown fact appears in an expression
fn count_unknown_occurrences(
    expr: &Expression,
    unknown: &(String, String),
    fact_matcher: &impl Fn(&FactPath, &str, &str) -> bool,
) -> usize {
    match &expr.kind {
        ExpressionKind::FactPath(fp) => {
            if fact_matcher(fp, &unknown.0, &unknown.1) {
                1
            } else {
                0
            }
        }
        ExpressionKind::FactReference(_) => 0,
        ExpressionKind::Arithmetic(l, _, r)
        | ExpressionKind::LogicalAnd(l, r)
        | ExpressionKind::LogicalOr(l, r)
        | ExpressionKind::Comparison(l, _, r) => {
            count_unknown_occurrences(l, unknown, fact_matcher)
                + count_unknown_occurrences(r, unknown, fact_matcher)
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            count_unknown_occurrences(inner, unknown, fact_matcher)
        }
        _ => 0,
    }
}

/// Simplify a boolean expression using BDD-based simplification
pub fn simplify_boolean(
    expr: &Expression,
    expr_eq: &impl Fn(&Expression, &Expression) -> bool,
) -> crate::LemmaResult<Expression> {
    let folded = expansion::try_constant_fold(expr).unwrap_or_else(|| expr.clone());

    let mut atoms: Vec<Expression> = Vec::new();
    if let Some(bexpr) = to_bool_expr(&folded, &mut atoms, expr_eq) {
        const MAX_ATOMS: usize = 64;
        if atoms.len() <= MAX_ATOMS {
            let simplified = bexpr.simplify_via_bdd();
            let rebuilt = from_bool_expr(&simplified, &atoms);
            return Ok(expansion::try_constant_fold(&rebuilt).unwrap_or(rebuilt));
        }
    }

    Ok(folded)
}

/// Simplify OR expressions using BDD
pub fn simplify_or_expression(
    expr: &Expression,
    expr_eq: &impl Fn(&Expression, &Expression) -> bool,
) -> Expression {
    let folded = expansion::try_constant_fold(expr).unwrap_or_else(|| expr.clone());

    let mut atoms: Vec<Expression> = Vec::new();
    if let Some(bexpr) = to_bool_expr(&folded, &mut atoms, expr_eq) {
        const MAX_ATOMS: usize = 64;
        if atoms.len() <= MAX_ATOMS {
            let simplified = bexpr.simplify_via_bdd();
            let rebuilt = from_bool_expr(&simplified, &atoms);
            return expansion::try_constant_fold(&rebuilt).unwrap_or(rebuilt);
        }
    }

    folded
}

fn to_bool_expr(
    expr: &Expression,
    atoms: &mut Vec<Expression>,
    expr_eq: &impl Fn(&Expression, &Expression) -> bool,
) -> Option<boolean_expression::Expr<usize>> {
    use boolean_expression::Expr;

    match &expr.kind {
        ExpressionKind::Literal(LiteralValue::Boolean(b)) => Some(Expr::Const(b.into())),
        ExpressionKind::LogicalAnd(l, r) => {
            let lbe = to_bool_expr(l, atoms, expr_eq)?;
            let rbe = to_bool_expr(r, atoms, expr_eq)?;
            Some(Expr::and(lbe, rbe))
        }
        ExpressionKind::LogicalOr(l, r) => {
            let lbe = to_bool_expr(l, atoms, expr_eq)?;
            let rbe = to_bool_expr(r, atoms, expr_eq)?;
            Some(Expr::or(lbe, rbe))
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            let ibe = to_bool_expr(inner, atoms, expr_eq)?;
            Some(Expr::not(ibe))
        }
        ExpressionKind::Comparison(_, _, _) => {
            let mut idx_opt = None;
            for (i, a) in atoms.iter().enumerate() {
                if expr_eq(a, expr) {
                    idx_opt = Some(i);
                    break;
                }
            }
            let idx = match idx_opt {
                Some(i) => i,
                None => {
                    atoms.push(expr.clone());
                    atoms.len() - 1
                }
            };
            Some(Expr::Terminal(idx))
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Arithmetic(_, _, _)
        | ExpressionKind::UnitConversion(_, _)
        | ExpressionKind::MathematicalComputation(_, _)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::RuleReference(_)
        | ExpressionKind::FactPath(_)
        | ExpressionKind::RulePath(_)
        | ExpressionKind::Veto(_) => None,
    }
}

fn from_bool_expr(be: &boolean_expression::Expr<usize>, atoms: &[Expression]) -> Expression {
    use boolean_expression::Expr;

    match be {
        Expr::Const(b) => Expression::new(
            ExpressionKind::Literal(LiteralValue::Boolean((*b).into())),
            None,
            ExpressionId::new(0),
        ),
        Expr::Terminal(i) => atoms.get(*i).cloned().unwrap_or_else(|| {
            Expression::new(
                ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False)),
                None,
                ExpressionId::new(0),
            )
        }),
        Expr::Not(inner) => {
            let inner_expr = from_bool_expr(inner, atoms);
            Expression::new(
                ExpressionKind::LogicalNegation(Box::new(inner_expr), crate::NegationType::Not),
                None,
                ExpressionId::new(0),
            )
        }
        Expr::And(l, r) => {
            let l_expr = from_bool_expr(l, atoms);
            let r_expr = from_bool_expr(r, atoms);
            Expression::new(
                ExpressionKind::LogicalAnd(Box::new(l_expr), Box::new(r_expr)),
                None,
                ExpressionId::new(0),
            )
        }
        Expr::Or(l, r) => {
            let l_expr = from_bool_expr(l, atoms);
            let r_expr = from_bool_expr(r, atoms);
            Expression::new(
                ExpressionKind::LogicalOr(Box::new(l_expr), Box::new(r_expr)),
                None,
                ExpressionId::new(0),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArithmeticComputation, ConversionTarget, ExpressionKind, FactPath, MassUnit};

    fn fact_path(name: &str) -> FactPath {
        FactPath::local(name.to_string())
    }

    fn literal_expr(val: LiteralValue) -> Expression {
        Expression::new(ExpressionKind::Literal(val), None, ExpressionId::new(0))
    }

    fn fact_expr(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::FactPath(fact_path(name)),
            None,
            ExpressionId::new(0),
        )
    }

    fn fact_matcher(fp: &FactPath, doc: &str, name: &str) -> bool {
        if fp.is_local() {
            fp.fact == name
        } else if fp.segments.len() == 1 {
            fp.segments[0].fact == doc && fp.fact == name
        } else {
            false
        }
    }

    #[test]
    fn test_count_unknown_occurrences_single() {
        let unknown = ("test".to_string(), "x".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("x")),
                ArithmeticComputation::Multiply,
                Box::new(literal_expr(LiteralValue::Number(
                    rust_decimal::Decimal::from(5),
                ))),
            ),
            None,
            ExpressionId::new(0),
        );

        let count = count_unknown_occurrences(&expr, &unknown, &|fp, d, n| fact_matcher(fp, d, n));
        assert_eq!(count, 1);
    }

    #[test]
    fn test_count_unknown_occurrences_multiple() {
        let unknown = ("test".to_string(), "x".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("x")),
                ArithmeticComputation::Add,
                Box::new(fact_expr("x")),
            ),
            None,
            ExpressionId::new(0),
        );

        let count = count_unknown_occurrences(&expr, &unknown, &|fp, d, n| fact_matcher(fp, d, n));
        assert_eq!(count, 2);
    }

    #[test]
    fn test_count_unknown_occurrences_zero() {
        let unknown = ("test".to_string(), "x".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("y")),
                ArithmeticComputation::Multiply,
                Box::new(literal_expr(LiteralValue::Number(
                    rust_decimal::Decimal::from(5),
                ))),
            ),
            None,
            ExpressionId::new(0),
        );

        let count = count_unknown_occurrences(&expr, &unknown, &|fp, d, n| fact_matcher(fp, d, n));
        assert_eq!(count, 0);
    }

    #[test]
    fn test_can_algebraically_solve_simple() {
        let unknown = ("test".to_string(), "price".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("price")),
                ArithmeticComputation::Multiply,
                Box::new(literal_expr(LiteralValue::Number(
                    rust_decimal::Decimal::from(5),
                ))),
            ),
            None,
            ExpressionId::new(0),
        );

        assert!(can_algebraically_solve(&expr, &unknown, &|fp, d, n| {
            fact_matcher(fp, d, n)
        }));
    }

    #[test]
    fn test_can_algebraically_solve_multiple_unknowns() {
        let unknown = ("test".to_string(), "price".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("price")),
                ArithmeticComputation::Multiply,
                Box::new(fact_expr("quantity")),
            ),
            None,
            ExpressionId::new(0),
        );

        let fact_matcher_impl = |fp: &FactPath, _doc: &str, name: &str| -> bool {
            if fp.is_local() {
                fp.fact == name
            } else {
                false
            }
        };

        let count = count_unknown_occurrences(&expr, &unknown, &fact_matcher_impl);
        assert_eq!(count, 1, "price should appear once");
        assert!(
            can_algebraically_solve(&expr, &unknown, &fact_matcher_impl),
            "price * quantity is solvable for price if quantity is known (not checked here)"
        );
    }

    #[test]
    fn test_can_algebraically_solve_duplicate_unknown() {
        let unknown = ("test".to_string(), "price".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("price")),
                ArithmeticComputation::Add,
                Box::new(fact_expr("price")),
            ),
            None,
            ExpressionId::new(0),
        );

        assert!(!can_algebraically_solve(&expr, &unknown, &|fp, d, n| {
            fact_matcher(fp, d, n)
        }));
    }

    #[test]
    fn test_algebraic_solve_simple_multiplication() {
        let unknown = ("test".to_string(), "price".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("price")),
                ArithmeticComputation::Multiply,
                Box::new(literal_expr(LiteralValue::Number(
                    rust_decimal::Decimal::from(5),
                ))),
            ),
            None,
            ExpressionId::new(0),
        );
        let target = literal_expr(LiteralValue::Number(rust_decimal::Decimal::from(50)));

        let result = algebraic_solve(&expr, &unknown, &target, &|fp, d, n| fact_matcher(fp, d, n));
        assert!(result.is_ok());
        let solved = result.unwrap();
        let folded = expansion::try_constant_fold(&solved).unwrap_or(solved);

        if let ExpressionKind::Literal(LiteralValue::Number(val)) = folded.kind {
            assert_eq!(val, rust_decimal::Decimal::from(10));
        } else {
            panic!("Expected literal number 10, got {:?}", folded.kind);
        }
    }

    #[test]
    fn test_algebraic_solve_chained_multiplication() {
        let unknown = ("test".to_string(), "hours".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(Expression::new(
                    ExpressionKind::Arithmetic(
                        Box::new(fact_expr("hours")),
                        ArithmeticComputation::Multiply,
                        Box::new(literal_expr(LiteralValue::Number(
                            rust_decimal::Decimal::from(25),
                        ))),
                    ),
                    None,
                    ExpressionId::new(0),
                )),
                ArithmeticComputation::Multiply,
                Box::new(literal_expr(LiteralValue::Number(
                    rust_decimal::Decimal::new(8, 1),
                ))),
            ),
            None,
            ExpressionId::new(0),
        );
        let target = literal_expr(LiteralValue::Number(rust_decimal::Decimal::from(800)));

        let result = algebraic_solve(&expr, &unknown, &target, &|fp, d, n| fact_matcher(fp, d, n));
        assert!(result.is_ok());
        let solved = result.unwrap();
        let folded = expansion::try_constant_fold(&solved).unwrap_or(solved);

        if let ExpressionKind::Literal(LiteralValue::Number(val)) = folded.kind {
            assert_eq!(val, rust_decimal::Decimal::from(40));
        } else {
            panic!("Expected literal number 40, got {:?}", folded.kind);
        }
    }

    #[test]
    fn test_algebraic_solve_chained_addition_subtraction() {
        let unknown = ("test".to_string(), "x".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(Expression::new(
                    ExpressionKind::Arithmetic(
                        Box::new(fact_expr("x")),
                        ArithmeticComputation::Add,
                        Box::new(literal_expr(LiteralValue::Number(
                            rust_decimal::Decimal::from(5),
                        ))),
                    ),
                    None,
                    ExpressionId::new(0),
                )),
                ArithmeticComputation::Subtract,
                Box::new(literal_expr(LiteralValue::Number(
                    rust_decimal::Decimal::from(3),
                ))),
            ),
            None,
            ExpressionId::new(0),
        );
        let target = literal_expr(LiteralValue::Number(rust_decimal::Decimal::from(17)));

        let result = algebraic_solve(&expr, &unknown, &target, &|fp, d, n| fact_matcher(fp, d, n));
        assert!(result.is_ok());
        let solved = result.unwrap();
        let folded = expansion::try_constant_fold(&solved).unwrap_or(solved);

        if let ExpressionKind::Literal(LiteralValue::Number(val)) = folded.kind {
            assert_eq!(val, rust_decimal::Decimal::from(15));
        } else {
            panic!("Expected literal number 15, got {:?}", folded.kind);
        }
    }

    #[test]
    fn test_algebraic_solve_unit_conversion() {
        let unknown = ("test".to_string(), "weight".to_string());
        let inner = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("weight")),
                ArithmeticComputation::Multiply,
                Box::new(literal_expr(LiteralValue::Number(
                    rust_decimal::Decimal::new(22, 1),
                ))),
            ),
            None,
            ExpressionId::new(0),
        );
        let expr = Expression::new(
            ExpressionKind::UnitConversion(
                Box::new(inner),
                ConversionTarget::Mass(MassUnit::Kilogram),
            ),
            None,
            ExpressionId::new(0),
        );
        let target = literal_expr(LiteralValue::Number(rust_decimal::Decimal::from(100)));

        let result = algebraic_solve(&expr, &unknown, &target, &|fp, d, n| fact_matcher(fp, d, n));
        assert!(result.is_ok());
        let solved = result.unwrap();
        let folded = expansion::try_constant_fold(&solved).unwrap_or(solved);

        if let ExpressionKind::UnitConversion(inner_solved, unit) = &folded.kind {
            assert_eq!(unit, &ConversionTarget::Mass(MassUnit::Kilogram));
            let inner_folded =
                expansion::try_constant_fold(inner_solved).unwrap_or((**inner_solved).clone());
            if let ExpressionKind::Literal(LiteralValue::Number(val)) = inner_folded.kind {
                let expected = rust_decimal::Decimal::new(45454545, 6);
                let diff = (val - expected).abs();
                assert!(
                    diff < rust_decimal::Decimal::new(1, 4),
                    "Expected ~45.45, got {}",
                    val
                );
            } else {
                panic!(
                    "Expected literal number in unit conversion result, got {:?}",
                    inner_folded.kind
                );
            }
        } else {
            panic!("Expected UnitConversion, got {:?}", folded.kind);
        }
    }

    #[test]
    fn test_algebraic_solve_error_multiple_unknowns() {
        let unknown = ("test".to_string(), "price".to_string());
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(fact_expr("price")),
                ArithmeticComputation::Add,
                Box::new(fact_expr("price")),
            ),
            None,
            ExpressionId::new(0),
        );
        let target = literal_expr(LiteralValue::Number(rust_decimal::Decimal::from(50)));

        let result = algebraic_solve(&expr, &unknown, &target, &|fp, d, n| fact_matcher(fp, d, n));
        assert!(result.is_err());
        match result.unwrap_err() {
            SolveError::UnknownAppearsMultipleTimes(count) => {
                assert_eq!(count, 2);
            }
            e => panic!("Expected UnknownAppearsMultipleTimes, got {:?}", e),
        }
    }

    #[test]
    fn test_algebraic_solve_error_rule_reference() {
        let unknown = ("test".to_string(), "price".to_string());
        let expr = Expression::new(
            ExpressionKind::RulePath(crate::RulePath::local("rule".to_string())),
            None,
            ExpressionId::new(0),
        );
        let target = literal_expr(LiteralValue::Number(rust_decimal::Decimal::from(50)));

        let result = algebraic_solve(&expr, &unknown, &target, &|fp, d, n| fact_matcher(fp, d, n));
        assert!(result.is_err());
        match result.unwrap_err() {
            SolveError::RuleReferenceFound => {}
            e => panic!("Expected RuleReferenceFound, got {:?}", e),
        }
    }
}
