//! Algebraic isolation for solving equations
//!
//! Provides functions to isolate facts from arithmetic expressions for constraint solving.

use crate::computation::constraints::UnsatReason;
use crate::semantic::{
    ArithmeticComputation, ComparisonComputation, EqualityNotation, Expression, ExpressionKind,
    FactPath, LiteralValue,
};
use rust_decimal::Decimal;
use std::collections::HashSet;
use std::sync::Arc;

/// Result of attempting to isolate a fact from an arithmetic expression
#[derive(Debug)]
pub enum IsolationResult {
    /// Successfully isolated: fact op value
    Isolated {
        fact: FactPath,
        op: ComparisonComputation,
        value: LiteralValue,
    },
    /// The constraint is satisfied for any value of the fact
    Unconstrained,
    /// The constraint can never be satisfied
    Unsatisfiable(UnsatReason),
    /// Multiple facts present - return simplified expression
    MultipleUnknowns(Expression),
    /// Cannot isolate (unsupported structure)
    Symbolic,
}

/// Collect all fact paths from an expression
pub fn collect_facts(expression: &Expression) -> HashSet<FactPath> {
    let mut facts = HashSet::new();
    collect_facts_recursive(expression, &mut facts);
    facts
}

fn collect_facts_recursive(expression: &Expression, facts: &mut HashSet<FactPath>) {
    match &expression.kind {
        ExpressionKind::FactPath(path) => {
            facts.insert(path.clone());
        }
        ExpressionKind::Arithmetic(left, _, right) => {
            collect_facts_recursive(left, facts);
            collect_facts_recursive(right, facts);
        }
        ExpressionKind::MathematicalComputation(_, inner) => {
            collect_facts_recursive(inner, facts);
        }
        ExpressionKind::Comparison(left, _, right) => {
            collect_facts_recursive(left, facts);
            collect_facts_recursive(right, facts);
        }
        ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
            collect_facts_recursive(left, facts);
            collect_facts_recursive(right, facts);
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            collect_facts_recursive(inner, facts);
        }
        ExpressionKind::UnitConversion(inner, _) => {
            collect_facts_recursive(inner, facts);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::RulePath(_) => {}

        ExpressionKind::RuleReference(_) | ExpressionKind::FactReference(_) => {
            unreachable!(
                "bug: FactReference/RuleReference in isolation - \
                 should have been converted to FactPath/RulePath during graph building"
            )
        }
    }
}

/// Check if an expression contains a specific fact
pub fn contains_fact(expression: &Expression, target: &FactPath) -> bool {
    match &expression.kind {
        ExpressionKind::FactPath(path) => path == target,
        ExpressionKind::Arithmetic(left, _, right) => {
            contains_fact(left, target) || contains_fact(right, target)
        }
        ExpressionKind::MathematicalComputation(_, inner) => contains_fact(inner, target),
        ExpressionKind::UnitConversion(inner, _) => contains_fact(inner, target),
        _ => false,
    }
}

/// Try to algebraically isolate a fact from a comparison
///
/// Given `expr op literal`, attempts to solve for the single fact.
pub fn try_isolate_comparison(
    left: &Expression,
    op: &ComparisonComputation,
    right: &Expression,
) -> IsolationResult {
    // Check if right side is a literal
    let target_value = match &right.kind {
        ExpressionKind::Literal(value) => value.clone(),
        _ => return IsolationResult::Symbolic,
    };

    // Collect facts from left side
    let facts = collect_facts(left);

    match facts.len() {
        0 => {
            // No facts - this is a constant comparison, should have been reduced
            IsolationResult::Symbolic
        }
        1 => {
            // Single fact - try to isolate
            let fact_path = facts.into_iter().next().expect("checked len == 1");
            isolate_single_fact(left, &fact_path, op, &target_value)
        }
        _ => {
            // Multiple facts - try to simplify constants
            match try_simplify_constants(left, op, &target_value) {
                Some(simplified) => IsolationResult::MultipleUnknowns(simplified),
                None => IsolationResult::Symbolic,
            }
        }
    }
}

/// Isolate a single fact from an arithmetic expression
fn isolate_single_fact(
    expression: &Expression,
    target_fact: &FactPath,
    op: &ComparisonComputation,
    target_value: &LiteralValue,
) -> IsolationResult {
    match &expression.kind {
        ExpressionKind::FactPath(path) if path == target_fact => {
            // Already isolated
            IsolationResult::Isolated {
                fact: path.clone(),
                op: op.clone(),
                value: target_value.clone(),
            }
        }

        ExpressionKind::Arithmetic(left, arith_op, right) => {
            let left_has_fact = contains_fact(left, target_fact);
            let right_has_fact = contains_fact(right, target_fact);

            if left_has_fact && !right_has_fact {
                // Fact is on left: (fact_expr arith_op constant) cmp_op target
                isolate_from_left(left, arith_op, right, target_fact, op, target_value)
            } else if right_has_fact && !left_has_fact {
                // Fact is on right: (constant arith_op fact_expr) cmp_op target
                isolate_from_right(left, arith_op, right, target_fact, op, target_value)
            } else {
                // Both sides have the fact (non-linear) or neither (shouldn't happen)
                IsolationResult::Symbolic
            }
        }

        _ => IsolationResult::Symbolic,
    }
}

/// Isolate fact from: (fact_expr arith_op constant) cmp_op target
fn isolate_from_left(
    fact_expr: &Expression,
    arith_op: &ArithmeticComputation,
    constant_expr: &Expression,
    target_fact: &FactPath,
    cmp_op: &ComparisonComputation,
    target_value: &LiteralValue,
) -> IsolationResult {
    let constant = match &constant_expr.kind {
        ExpressionKind::Literal(LiteralValue::Number(n)) => *n,
        _ => return IsolationResult::Symbolic,
    };

    let target_num = match target_value {
        LiteralValue::Number(n) => *n,
        _ => return IsolationResult::Symbolic,
    };

    match arith_op {
        ArithmeticComputation::Add => {
            // (x + c) op t → x op (t - c)
            let new_target = target_num - constant;
            isolate_single_fact(
                fact_expr,
                target_fact,
                cmp_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Subtract => {
            // (x - c) op t → x op (t + c)
            let new_target = target_num + constant;
            isolate_single_fact(
                fact_expr,
                target_fact,
                cmp_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Multiply => {
            if constant == Decimal::ZERO {
                // x * 0 op t
                if cmp_op.is_equal() {
                    if target_num == Decimal::ZERO {
                        // x * 0 == 0 → true for any x
                        return IsolationResult::Unconstrained;
                    } else {
                        // x * 0 == non-zero → false
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("x * 0 cannot equal {}", target_num),
                        });
                    }
                } else {
                    // x * 0 > t, x * 0 < t, etc.
                    let zero = Decimal::ZERO;
                    let satisfiable = match cmp_op {
                        ComparisonComputation::GreaterThan => zero > target_num,
                        ComparisonComputation::GreaterThanOrEqual => zero >= target_num,
                        ComparisonComputation::LessThan => zero < target_num,
                        ComparisonComputation::LessThanOrEqual => zero <= target_num,
                        _ => return IsolationResult::Symbolic,
                    };
                    if satisfiable {
                        return IsolationResult::Unconstrained;
                    } else {
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("0 {} {} is always false", cmp_op.symbol(), target_num),
                        });
                    }
                }
            }

            // x * c op t → x op (t / c), flip if c < 0
            let new_target = target_num / constant;
            let new_op = if constant < Decimal::ZERO {
                flip_comparison(cmp_op)
            } else {
                cmp_op.clone()
            };
            isolate_single_fact(
                fact_expr,
                target_fact,
                &new_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Divide => {
            if constant == Decimal::ZERO {
                // x / 0 - division by zero, should have been caught earlier
                return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                    message: "Division by zero".to_string(),
                });
            }

            // x / c op t → x op (t * c), flip if c < 0
            let new_target = target_num * constant;
            let new_op = if constant < Decimal::ZERO {
                flip_comparison(cmp_op)
            } else {
                cmp_op.clone()
            };
            isolate_single_fact(
                fact_expr,
                target_fact,
                &new_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Modulo | ArithmeticComputation::Power => {
            // Not supported for simple isolation
            IsolationResult::Symbolic
        }
    }
}

/// Isolate fact from: (constant arith_op fact_expr) cmp_op target
fn isolate_from_right(
    constant_expr: &Expression,
    arith_op: &ArithmeticComputation,
    fact_expr: &Expression,
    target_fact: &FactPath,
    cmp_op: &ComparisonComputation,
    target_value: &LiteralValue,
) -> IsolationResult {
    let constant = match &constant_expr.kind {
        ExpressionKind::Literal(LiteralValue::Number(n)) => *n,
        _ => return IsolationResult::Symbolic,
    };

    let target_num = match target_value {
        LiteralValue::Number(n) => *n,
        _ => return IsolationResult::Symbolic,
    };

    match arith_op {
        ArithmeticComputation::Add => {
            // (c + x) op t → x op (t - c)
            let new_target = target_num - constant;
            isolate_single_fact(
                fact_expr,
                target_fact,
                cmp_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Subtract => {
            // (c - x) op t → -x op (t - c) → x op (c - t), flip comparison
            let new_target = constant - target_num;
            let new_op = flip_comparison(cmp_op);
            isolate_single_fact(
                fact_expr,
                target_fact,
                &new_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Multiply => {
            if constant == Decimal::ZERO {
                // 0 * x op t
                if cmp_op.is_equal() {
                    if target_num == Decimal::ZERO {
                        return IsolationResult::Unconstrained;
                    } else {
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("0 * x cannot equal {}", target_num),
                        });
                    }
                } else {
                    let zero = Decimal::ZERO;
                    let satisfiable = match cmp_op {
                        ComparisonComputation::GreaterThan => zero > target_num,
                        ComparisonComputation::GreaterThanOrEqual => zero >= target_num,
                        ComparisonComputation::LessThan => zero < target_num,
                        ComparisonComputation::LessThanOrEqual => zero <= target_num,
                        _ => return IsolationResult::Symbolic,
                    };
                    if satisfiable {
                        return IsolationResult::Unconstrained;
                    } else {
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("0 {} {} is always false", cmp_op.symbol(), target_num),
                        });
                    }
                }
            }

            // c * x op t → x op (t / c), flip if c < 0
            let new_target = target_num / constant;
            let new_op = if constant < Decimal::ZERO {
                flip_comparison(cmp_op)
            } else {
                cmp_op.clone()
            };
            isolate_single_fact(
                fact_expr,
                target_fact,
                &new_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Divide => {
            // c / x op t
            if target_num == Decimal::ZERO {
                if cmp_op.is_equal() {
                    if constant == Decimal::ZERO {
                        // 0 / x == 0 → true for any x != 0 (domain restriction)
                        // Return symbolic to preserve the domain restriction
                        return IsolationResult::Symbolic;
                    } else {
                        // c / x == 0 where c != 0 → impossible
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("{} / x cannot equal 0", constant),
                        });
                    }
                }
            }

            // c / x op t → x op (c / t), flip comparison (dividing changes direction)
            // But we need t != 0
            if target_num == Decimal::ZERO {
                return IsolationResult::Symbolic;
            }

            let new_target = constant / target_num;
            // When dividing by positive target, direction depends on sign
            // c / x = t means x = c / t
            // For inequalities: c / x > t is complex, keep as symbolic for now
            if !cmp_op.is_equal() {
                return IsolationResult::Symbolic;
            }
            isolate_single_fact(
                fact_expr,
                target_fact,
                cmp_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Modulo | ArithmeticComputation::Power => {
            IsolationResult::Symbolic
        }
    }
}

/// Flip comparison direction (for negative multiplier/divisor or subtraction from constant)
fn flip_comparison(op: &ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::Equal(notation) => ComparisonComputation::Equal(notation.clone()),
        ComparisonComputation::NotEqual(notation) => {
            ComparisonComputation::NotEqual(notation.clone())
        }
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThan,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThanOrEqual,
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThan,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThanOrEqual,
    }
}

/// Try to simplify constants in an expression with multiple unknowns
///
/// For `x + y + 10 == 100`, produce `x + y == 90`
fn try_simplify_constants(
    expression: &Expression,
    op: &ComparisonComputation,
    target_value: &LiteralValue,
) -> Option<Expression> {
    // Only handle equality for now
    if !op.is_equal() {
        return None;
    }

    let target_num = match target_value {
        LiteralValue::Number(n) => *n,
        _ => return None,
    };

    // Try to collect and combine constants from the expression
    let (facts_expr, constants_sum) = extract_constant_sum(expression)?;

    // New target: original target minus the constants
    let new_target = target_num - constants_sum;

    // Build simplified comparison: facts_expr == new_target
    Some(Expression::new(
        ExpressionKind::Comparison(
            Arc::new(facts_expr),
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            Arc::new(Expression::new(
                ExpressionKind::Literal(LiteralValue::Number(new_target)),
                None,
            )),
        ),
        None,
    ))
}

/// Extract the sum of constants from an additive expression
///
/// Returns (expression with constants removed, sum of constants)
/// Only handles simple additive chains for now.
fn extract_constant_sum(expression: &Expression) -> Option<(Expression, Decimal)> {
    match &expression.kind {
        ExpressionKind::Literal(LiteralValue::Number(_)) => {
            // Pure constant - shouldn't happen with multiple facts
            None
        }

        ExpressionKind::FactPath(_) => {
            // Single fact, no constants
            Some((expression.clone(), Decimal::ZERO))
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            match op {
                ArithmeticComputation::Add => {
                    // Check if right is a constant
                    if let ExpressionKind::Literal(LiteralValue::Number(n)) = &right.kind {
                        let (inner_expr, inner_sum) = extract_constant_sum(left)?;
                        return Some((inner_expr, inner_sum + *n));
                    }
                    // Check if left is a constant
                    if let ExpressionKind::Literal(LiteralValue::Number(n)) = &left.kind {
                        let (inner_expr, inner_sum) = extract_constant_sum(right)?;
                        return Some((inner_expr, inner_sum + *n));
                    }
                    // Both sides have facts - recurse on both
                    let (left_expr, left_sum) = extract_constant_sum(left)?;
                    let (right_expr, right_sum) = extract_constant_sum(right)?;
                    let combined = Expression::new(
                        ExpressionKind::Arithmetic(
                            Arc::new(left_expr),
                            ArithmeticComputation::Add,
                            Arc::new(right_expr),
                        ),
                        None,
                    );
                    Some((combined, left_sum + right_sum))
                }

                ArithmeticComputation::Subtract => {
                    // x - c → (x, -c)
                    if let ExpressionKind::Literal(LiteralValue::Number(n)) = &right.kind {
                        let (inner_expr, inner_sum) = extract_constant_sum(left)?;
                        return Some((inner_expr, inner_sum - *n));
                    }
                    // c - x is more complex, skip for now
                    None
                }

                _ => None,
            }
        }

        _ => None,
    }
}
