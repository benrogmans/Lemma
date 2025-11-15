//! Algebraic equation solving for single unknowns

use crate::{Expression, ExpressionId, ExpressionKind, FactReference, LiteralValue};

/// Attempt to solve an equation algebraically for a single unknown fact
///
/// Given an expression containing an unknown fact and a target value,
/// attempts to rearrange the equation to isolate the unknown.
///
/// Supports: +, -, *, /, ^ (power), exp, log
///
/// Returns None if:
/// - The unknown appears multiple times (can't isolate)
/// - Unsupported operations are used
/// - The equation cannot be algebraically rearranged
pub fn algebraic_solve(
    expr: &Expression,
    unknown: &(String, String),
    target: &Expression,
    fact_matcher: &impl Fn(&FactReference, &str, &str) -> bool,
) -> Option<Expression> {
    match &expr.kind {
        ExpressionKind::FactReference(fr) => {
            if fact_matcher(fr, &unknown.0, &unknown.1) {
                return Some(target.clone());
            }
            None
        }
        ExpressionKind::MathematicalComputation(op, inner) => {
            use crate::MathematicalComputation as M;
            if !contains_unknown(inner, unknown, fact_matcher) {
                return None;
            }

            let new_target = match op {
                // exp(u) = t  =>  u = log(t)
                M::Exp => Expression::new(
                    ExpressionKind::MathematicalComputation(M::Log, Box::new(target.clone())),
                    None,
                    ExpressionId::new(0),
                ),
                // log(u) = t  =>  u = exp(t)
                M::Log => Expression::new(
                    ExpressionKind::MathematicalComputation(M::Exp, Box::new(target.clone())),
                    None,
                    ExpressionId::new(0),
                ),
                _ => return None,
            };

            algebraic_solve(inner, unknown, &new_target, fact_matcher)
        }
        ExpressionKind::Arithmetic(l, op, r) => {
            let l_contains = contains_unknown(l, unknown, fact_matcher);
            let r_contains = contains_unknown(r, unknown, fact_matcher);

            if l_contains && !r_contains {
                // Unknown on left
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
                        // (u ^ c) = t  =>  u = t ^ (1 / c)
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
                    _ => return None,
                };
                algebraic_solve(l, unknown, &new_target, fact_matcher)
            } else if r_contains && !l_contains {
                // Unknown on right
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
                    crate::ArithmeticComputation::Subtract => {
                        // left - x = target => x = left - target
                        Expression::new(
                            ExpressionKind::Arithmetic(
                                Box::new((**l).clone()),
                                crate::ArithmeticComputation::Subtract,
                                Box::new(target.clone()),
                            ),
                            None,
                            ExpressionId::new(0),
                        )
                    }
                    crate::ArithmeticComputation::Multiply => Expression::new(
                        ExpressionKind::Arithmetic(
                            Box::new(target.clone()),
                            crate::ArithmeticComputation::Divide,
                            Box::new((**l).clone()),
                        ),
                        None,
                        ExpressionId::new(0),
                    ),
                    crate::ArithmeticComputation::Divide => {
                        // left / x = target => x = left / target
                        Expression::new(
                            ExpressionKind::Arithmetic(
                                Box::new((**l).clone()),
                                crate::ArithmeticComputation::Divide,
                                Box::new(target.clone()),
                            ),
                            None,
                            ExpressionId::new(0),
                        )
                    }
                    crate::ArithmeticComputation::Power => {
                        // (c ^ u) = t  =>  u = log(t) / log(c)
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
                    _ => return None,
                };
                algebraic_solve(r, unknown, &new_target, fact_matcher)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if an expression contains a reference to an unknown fact
pub fn contains_unknown(
    expr: &Expression,
    unknown: &(String, String),
    fact_matcher: &impl Fn(&FactReference, &str, &str) -> bool,
) -> bool {
    match &expr.kind {
        ExpressionKind::FactReference(fr) => fact_matcher(fr, &unknown.0, &unknown.1),
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
