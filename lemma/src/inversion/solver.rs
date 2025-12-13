//! Constraint solver for inversion
//!
//! Target-aware constraint solving for determining what inputs produce desired outputs.
//! Uses the computation module for constraint types and operations.

use crate::computation::{
    collect_domain_restrictions, expand, reduce, reverse_comparison, ConstraintSet,
    DomainRestriction, FactConstraint, OperationResult, UnsatReason,
};
use crate::semantic::{
    ArithmeticComputation, BooleanValue, ComparisonComputation, EqualityNotation, Expression,
    ExpressionKind, FactPath, LiteralValue,
};
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};

use super::Target;

/// Result of solving an equation
#[derive(Debug, Clone)]
pub enum SolveResult {
    /// Fully solved to concrete domains
    Solved {
        fact_constraints: HashMap<FactPath, FactConstraint>,
    },

    /// Partially solved — some constraints remain symbolic
    Partial {
        fact_constraints: HashMap<FactPath, FactConstraint>,
        remaining_constraints: Vec<Expression>,
        domain_restrictions: Vec<DomainRestriction>,
    },

    /// Contradiction detected — no valid solution
    Unsatisfiable { reason: UnsatReason },
}

// ============================================================================
// Target application
// ============================================================================

/// Apply target constraint to an equation expression
///
/// The equation has structure: (cond_0 ∧ result_0) ∨ (cond_1 ∧ result_1) ∨ ...
/// This transforms each (cond ∧ result) into (cond ∧ (result matches target))
///
/// Special case: if the equation is boolean false, it means no valid branches
/// exist (equation is unsatisfiable). This is preserved as-is.
pub fn apply_target(equation: &Expression, target: &Target) -> Expression {
    // Boolean false means "no solution exists" - preserve it
    if equation.is_boolean_false() {
        return equation.clone();
    }

    // Boolean true means "any solution works" (unconditional rule)
    // This is handled as a result expression below

    match &equation.kind {
        ExpressionKind::LogicalOr(left, right) => {
            let left_applied = apply_target(left, target);
            let right_applied = apply_target(right, target);
            Expression::new(
                ExpressionKind::LogicalOr(Box::new(left_applied), Box::new(right_applied)),
                equation.source.clone(),
            )
        }

        ExpressionKind::LogicalAnd(condition, result) => {
            // This is a branch: (condition AND result)
            // Transform to: (condition AND (result matches target))
            let target_check = match_result_to_target(result, target);
            Expression::new(
                ExpressionKind::LogicalAnd(condition.clone(), Box::new(target_check)),
                equation.source.clone(),
            )
        }

        // Result expressions (rule with no conditions, or leaf of the equation)
        ExpressionKind::Literal(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Arithmetic(_, _, _)
        | ExpressionKind::FactPath(_)
        | ExpressionKind::RulePath(_)
        | ExpressionKind::MathematicalComputation(_, _)
        | ExpressionKind::UnitConversion(_, _)
        | ExpressionKind::LogicalNegation(_, _)
        | ExpressionKind::Comparison(_, _, _) => match_result_to_target(equation, target),

        // These should never appear as top-level equation structures
        ExpressionKind::FactReference(_) | ExpressionKind::RuleReference(_) => {
            unreachable!(
                "Unexpected equation structure in apply_target: {:?}",
                equation.kind
            )
        }
    }
}

/// Check if a result expression matches the target
fn match_result_to_target(result: &Expression, target: &Target) -> Expression {
    match &target.outcome {
        None => {
            // any_value - always matches if result exists
            Expression::new(
                ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                None,
            )
        }
        Some(OperationResult::Veto(None)) => {
            // any_veto - matches if result is any veto
            match &result.kind {
                ExpressionKind::Veto(_) => Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                    None,
                ),
                _ => Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
                    None,
                ),
            }
        }
        Some(OperationResult::Veto(Some(target_message))) => {
            // specific veto - matches if result is veto with same message
            match &result.kind {
                ExpressionKind::Veto(veto) => {
                    let matches = veto.message.as_ref() == Some(target_message);
                    Expression::new(
                        ExpressionKind::Literal(LiteralValue::Boolean(if matches {
                            BooleanValue::True
                        } else {
                            BooleanValue::False
                        })),
                        None,
                    )
                }
                _ => Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
                    None,
                ),
            }
        }
        Some(OperationResult::Value(target_value)) => {
            // Compare result to target value
            Expression::new(
                ExpressionKind::Comparison(
                    Box::new(result.clone()),
                    target.op.to_comparison(),
                    Box::new(Expression::new(
                        ExpressionKind::Literal(target_value.clone()),
                        None,
                    )),
                ),
                None,
            )
        }
    }
}


/// Flatten a DNF expression into a list of OR branches
///
/// Returns all top-level OR alternatives as separate expressions.
fn flatten_or(expression: Expression) -> Vec<Expression> {
    match expression.kind {
        ExpressionKind::LogicalOr(left, right) => {
            let mut branches = flatten_or(*left);
            branches.extend(flatten_or(*right));
            branches
        }
        _ => vec![expression],
    }
}

/// Combine a list of expressions with OR
fn combine_with_or(mut branches: Vec<Expression>) -> Expression {
    if branches.is_empty() {
        return Expression::new(
            ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
            None,
        );
    }

    if branches.len() == 1 {
        return branches.remove(0);
    }

    let first = branches.remove(0);
    branches
        .into_iter()
        .fold(first, |accumulated, branch| {
            Expression::new(
                ExpressionKind::LogicalOr(Box::new(accumulated), Box::new(branch)),
                None,
            )
        })
}

// ============================================================================
// Main solver entry point
// ============================================================================

/// Solve an equation for the given target
///
/// Takes an equation expression and returns the constraints on facts
/// that make the equation satisfy the target. Returns multiple solutions
/// when the equation contains OR branches.
pub fn solve(equation: Expression, target: &Target) -> Vec<SolveResult> {
    // 1. Apply target constraint to the equation
    let constrained = apply_target(&equation, target);

    // 2. Expand to DNF: distribution, De Morgan, constant folding, veto handling
    let expanded = expand(constrained);

    // 3. Reduce: boolean minimization (QM-style)
    let reduced = reduce(expanded);

    // 4. Flatten OR branches and solve each independently
    let branches = flatten_or(reduced);

    // 5. Solve each branch
    let mut results: Vec<SolveResult> = Vec::new();
    for branch in branches {
        let result = solve_single_branch(branch);

        // Filter out unsatisfiable branches
        if !matches!(result, SolveResult::Unsatisfiable { .. }) {
            results.push(result);
        }
    }

    // If all branches were unsatisfiable, return single Unsatisfiable
    if results.is_empty() {
        return vec![SolveResult::Unsatisfiable {
            reason: UnsatReason::SimplifiedToFalse,
        }];
    }

    results
}

/// Solve a single branch (conjunction of constraints)
///
/// This function handles a single AND-branch from the DNF form.
/// OR expressions should not appear here after DNF conversion.
fn solve_single_branch(expression: Expression) -> SolveResult {
    let mut constraint_set = ConstraintSet::new();

    // Check for trivial true
    if expression.is_boolean_true() {
        return SolveResult::Solved {
            fact_constraints: HashMap::new(),
        };
    }

    // Check for trivial false
    if expression.is_boolean_false() {
        return SolveResult::Unsatisfiable {
            reason: UnsatReason::SimplifiedToFalse,
        };
    }

    // Collect domain restrictions from the expression
    let restrictions = collect_domain_restrictions(&expression);
    for restriction in restrictions {
        constraint_set.add_restriction(restriction);
    }

    // Extract constraints from the expression
    extract_constraints(&expression, &mut constraint_set);

    // Check for contradictions
    if let Some(reason) = constraint_set.contradiction.take() {
        return SolveResult::Unsatisfiable { reason };
    }

    // Convert to result
    let fact_constraints = constraint_set.to_fact_constraints();

    if constraint_set.symbolic.is_empty() && constraint_set.restrictions.is_empty() {
        SolveResult::Solved { fact_constraints }
    } else {
        SolveResult::Partial {
            fact_constraints,
            remaining_constraints: constraint_set.symbolic,
            domain_restrictions: constraint_set.restrictions,
        }
    }
}

/// Extract constraints from an expression into the constraint set
fn extract_constraints(expression: &Expression, constraint_set: &mut ConstraintSet) {
    match &expression.kind {
        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)) => {
            // true contributes no constraints
        }

        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)) => {
            // false means unsatisfiable
            constraint_set.contradiction = Some(UnsatReason::SimplifiedToFalse);
        }

        ExpressionKind::LogicalAnd(left, right) => {
            extract_constraints(left, constraint_set);
            extract_constraints(right, constraint_set);
        }

        ExpressionKind::LogicalOr(left, right) => {
            // OR represents alternative branches - add as symbolic for now
            constraint_set.add_symbolic(expression.clone());
            let _ = (left, right);
        }

        ExpressionKind::Comparison(left, op, right) => {
            // Try to extract fact op literal (direct case)
            if let ExpressionKind::FactPath(fact_path) = &left.kind {
                if let ExpressionKind::Literal(value) = &right.kind {
                    constraint_set.add_comparison(fact_path.clone(), op, value.clone());
                    return;
                }
            }

            // Try reversed: literal op fact
            if let ExpressionKind::FactPath(fact_path) = &right.kind {
                if let ExpressionKind::Literal(value) = &left.kind {
                    let reversed_op = reverse_comparison(op);
                    constraint_set.add_comparison(fact_path.clone(), &reversed_op, value.clone());
                    return;
                }
            }

            // Try fact op fact (relational constraint)
            if let (ExpressionKind::FactPath(left_fact), ExpressionKind::FactPath(right_fact)) =
                (&left.kind, &right.kind)
            {
                constraint_set.add_relation(left_fact.clone(), op.clone(), right_fact.clone());
                return;
            }

            // Try algebraic isolation for arithmetic expressions
            match try_isolate_comparison(left, op, right) {
                IsolationResult::Isolated { fact, op, value } => {
                    constraint_set.add_comparison(fact, &op, value);
                    return;
                }
                IsolationResult::Unconstrained => {
                    // No constraint needed - expression is always true
                    return;
                }
                IsolationResult::Unsatisfiable(reason) => {
                    constraint_set.contradiction = Some(reason);
                    return;
                }
                IsolationResult::MultipleUnknowns(simplified) => {
                    // Add the simplified expression as symbolic
                    constraint_set.add_symbolic(simplified);
                    return;
                }
                IsolationResult::Symbolic => {
                    // Fall through to add as symbolic
                }
            }

            // Also try with reversed operands (literal op arithmetic_expr)
            if let ExpressionKind::Literal(_) = &left.kind {
                let reversed_op = reverse_comparison(op);
                match try_isolate_comparison(right, &reversed_op, left) {
                    IsolationResult::Isolated { fact, op, value } => {
                        constraint_set.add_comparison(fact, &op, value);
                        return;
                    }
                    IsolationResult::Unconstrained => {
                        return;
                    }
                    IsolationResult::Unsatisfiable(reason) => {
                        constraint_set.contradiction = Some(reason);
                        return;
                    }
                    IsolationResult::MultipleUnknowns(simplified) => {
                        constraint_set.add_symbolic(simplified);
                        return;
                    }
                    IsolationResult::Symbolic => {
                        // Fall through
                    }
                }
            }

            // Complex comparison - add as symbolic
            constraint_set.add_symbolic(expression.clone());
        }

        ExpressionKind::LogicalNegation(inner, _) => {
            // NOT(fact == value) means fact != value
            if let ExpressionKind::Comparison(left, op, right) = &inner.kind {
                if op.is_equal() {
                    if let ExpressionKind::FactPath(fact_path) = &left.kind {
                        if let ExpressionKind::Literal(value) = &right.kind {
            constraint_set.add_comparison(
                fact_path.clone(),
                                &ComparisonComputation::NotEqual(
                                    crate::semantic::EqualityNotation::Symbol,
                                ),
                                value.clone(),
                            );
                            return;
                        }
                    }
                }
            }

            // NOT(fact) means fact == false
            if let ExpressionKind::FactPath(fact_path) = &inner.kind {
                constraint_set.add_comparison(
                    fact_path.clone(),
                    &ComparisonComputation::Equal(crate::semantic::EqualityNotation::Symbol),
                    LiteralValue::Boolean(BooleanValue::False),
                );
                return;
            }

            // Complex negation - add as symbolic
            constraint_set.add_symbolic(expression.clone());
        }

        ExpressionKind::FactPath(fact_path) => {
            // Bare fact reference means fact == true
                            constraint_set.add_comparison(
                                fact_path.clone(),
                &ComparisonComputation::Equal(crate::semantic::EqualityNotation::Symbol),
                LiteralValue::Boolean(BooleanValue::True),
            );
        }

        // Other expression types - add as symbolic
        _ => {
            constraint_set.add_symbolic(expression.clone());
        }
    }
}

// ============================================================================
// Algebraic Isolation
// ============================================================================

/// Result of attempting to isolate a fact from an arithmetic expression
#[derive(Debug)]
enum IsolationResult {
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
fn collect_facts(expression: &Expression) -> HashSet<FactPath> {
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
        | ExpressionKind::RulePath(_)
        | ExpressionKind::RuleReference(_)
        | ExpressionKind::FactReference(_) => {}
    }
}

/// Check if an expression contains a specific fact
fn contains_fact(expression: &Expression, target: &FactPath) -> bool {
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
fn try_isolate_comparison(
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
            Box::new(facts_expr),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
            Box::new(Expression::new(
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
                            Box::new(left_expr),
                            ArithmeticComputation::Add,
                            Box::new(right_expr),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::ConstraintSet;
    use crate::semantic::EqualityNotation;
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

    fn num(n: i64) -> LiteralValue {
        LiteralValue::Number(Decimal::from(n))
    }

    #[test]
    fn test_solve_trivial_true() {
        use crate::inversion::Target;

        let equation = literal_bool(true);
        let target = Target::any_value();
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], SolveResult::Solved { .. }));
    }

    #[test]
    fn test_solve_trivial_false() {
        use crate::inversion::Target;

        let equation = literal_bool(false);
        let target = Target::any_value();
        let results = solve(equation, &target);

        // All branches unsatisfiable returns single Unsatisfiable
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], SolveResult::Unsatisfiable { .. }));
    }

    #[test]
    fn test_solve_simple_comparison() {
        use crate::inversion::Target;

        // fact == 42
        let fact_path = FactPath::local("x".to_string());
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(literal_bool(true)),
                Box::new(Expression::new(
                    ExpressionKind::FactPath(fact_path.clone()),
                    None,
                )),
            ),
            None,
        );
        let target = Target::value(num(42));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Solved { fact_constraints } | SolveResult::Partial { fact_constraints, .. } => {
                assert!(fact_constraints.contains_key(&fact_path));
            }
            _ => panic!("Expected solved or partial result"),
        }
    }

    #[test]
    fn test_constraint_set_bounds() {
        let mut constraint_set = ConstraintSet::new();
        let fact = FactPath::local("x".to_string());

        constraint_set.add_comparison(
            fact.clone(),
            &ComparisonComputation::GreaterThanOrEqual,
            num(10),
        );

        constraint_set.add_comparison(fact.clone(), &ComparisonComputation::LessThan, num(100));

        assert!(constraint_set.contradiction.is_none());

        let bounds = constraint_set.facts.get(&fact).unwrap();
        assert!(matches!(bounds.min, Some((LiteralValue::Number(_), true))));
        assert!(matches!(bounds.max, Some((LiteralValue::Number(_), false))));
    }

    #[test]
    fn test_constraint_set_contradiction() {
        let mut constraint_set = ConstraintSet::new();
        let fact = FactPath::local("x".to_string());

        constraint_set.add_comparison(
            fact.clone(),
            &ComparisonComputation::GreaterThanOrEqual,
            num(100),
        );

        constraint_set.add_comparison(fact.clone(), &ComparisonComputation::LessThan, num(50));

        assert!(constraint_set.contradiction.is_some());
    }

    #[test]
    fn test_constraint_set_exact_value_contradiction() {
        let mut constraint_set = ConstraintSet::new();
        let fact = FactPath::local("x".to_string());

        constraint_set.add_comparison(
            fact.clone(),
            &ComparisonComputation::Equal(EqualityNotation::Symbol),
            num(10),
        );

        constraint_set.add_comparison(
            fact.clone(),
            &ComparisonComputation::Equal(EqualityNotation::Symbol),
            num(20),
        );

        assert!(matches!(
            constraint_set.contradiction,
            Some(UnsatReason::EnumContradiction { .. })
        ));
    }

    // ========================================================================
    // OR Handling Tests
    // ========================================================================

    /// `false ∨ (x > 10)` → Single solution: `x > 10`
    #[test]
    fn test_false_branch_filtered() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        // false ∨ (x > 10)
        let equation = Expression::new(
            ExpressionKind::LogicalOr(
                Box::new(literal_bool(false)),
                Box::new(Expression::new(
                    ExpressionKind::Comparison(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(fact_x.clone()),
                            None,
                        )),
                        ComparisonComputation::GreaterThan,
                        Box::new(Expression::new(
                            ExpressionKind::Literal(num(10)),
                            None,
                        )),
                    ),
                    None,
                )),
            ),
            None,
        );

        let target = Target::any_value();
        let results = solve(equation, &target);

        // Should have exactly 1 solution (false branch filtered out)
        assert_eq!(results.len(), 1, "false branch should be filtered, expected 1 solution");
        assert!(
            matches!(&results[0], SolveResult::Solved { .. } | SolveResult::Partial { .. }),
            "should have a valid solution"
        );
    }

    /// `false ∨ false` → Unsatisfiable
    #[test]
    fn test_all_false_unsatisfiable() {
        use crate::inversion::Target;

        // false ∨ false
        let equation = Expression::new(
            ExpressionKind::LogicalOr(
                Box::new(literal_bool(false)),
                Box::new(literal_bool(false)),
            ),
            None,
        );

        let target = Target::any_value();
        let results = solve(equation, &target);

        // Should return single Unsatisfiable
        assert_eq!(results.len(), 1, "all-false should return single result");
        assert!(
            matches!(&results[0], SolveResult::Unsatisfiable { .. }),
            "should be Unsatisfiable"
        );
    }

    /// `true ∨ (x > 10)` → Single solution: unconstrained (true absorbs)
    #[test]
    fn test_true_absorbs() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        // true ∨ (x > 10)
        let equation = Expression::new(
            ExpressionKind::LogicalOr(
                Box::new(literal_bool(true)),
                Box::new(Expression::new(
            ExpressionKind::Comparison(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(fact_x.clone()),
                            None,
                        )),
                        ComparisonComputation::GreaterThan,
                        Box::new(Expression::new(
                            ExpressionKind::Literal(num(10)),
                            None,
                        )),
                    ),
                    None,
                )),
            ),
            None,
        );

        let target = Target::any_value();
        let results = solve(equation, &target);

        // After expansion, `true ∨ X` becomes `true`, so we get one unconstrained solution
        // The expand() function should simplify this before we get to solve
        assert!(
            results.len() >= 1,
            "should have at least one solution"
        );

        // At least one solution should be unconstrained (no fact constraints)
        let has_unconstrained = results.iter().any(|r| {
            matches!(r, SolveResult::Solved { fact_constraints } if fact_constraints.is_empty())
        });
        assert!(
            has_unconstrained,
            "true branch should produce unconstrained solution"
        );
    }

    /// Nested OR `(a ∨ b) ∨ c` → Three branches flattened
    ///
    /// Uses proper equation structure: (condition ∧ result) branches
    /// Each branch has condition (a==1, b==2, c==3) and result (true literal)
    #[test]
    fn test_nested_or_flattened() {
        use crate::inversion::Target;

        let fact_a = FactPath::local("a".to_string());
        let fact_b = FactPath::local("b".to_string());
        let fact_c = FactPath::local("c".to_string());

        // Branch 1: (a == 1) ∧ true
        let a_eq_1 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(ExpressionKind::FactPath(fact_a.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(Expression::new(ExpressionKind::Literal(num(1)), None)),
            ),
            None,
        );
        let branch_a = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(a_eq_1),
                Box::new(literal_bool(true)),
            ),
            None,
        );

        // Branch 2: (b == 2) ∧ true
        let b_eq_2 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(ExpressionKind::FactPath(fact_b.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(Expression::new(ExpressionKind::Literal(num(2)), None)),
            ),
            None,
        );
        let branch_b = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(b_eq_2),
                Box::new(literal_bool(true)),
            ),
            None,
        );

        // Branch 3: (c == 3) ∧ true
        let c_eq_3 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(ExpressionKind::FactPath(fact_c.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(Expression::new(ExpressionKind::Literal(num(3)), None)),
            ),
            None,
        );
        let branch_c = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(c_eq_3),
                Box::new(literal_bool(true)),
            ),
            None,
        );

        // Nested: ((branch_a) ∨ (branch_b)) ∨ (branch_c)
        let a_or_b = Expression::new(
            ExpressionKind::LogicalOr(Box::new(branch_a), Box::new(branch_b)),
            None,
        );
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Box::new(a_or_b), Box::new(branch_c)),
            None,
        );

        // Target: result == true
        let target = Target::value(LiteralValue::Boolean(BooleanValue::True));
        let results = solve(equation, &target);

        // Should have 3 solutions (one for each branch)
        assert_eq!(
            results.len(),
            3,
            "nested OR should produce 3 solutions, got {}",
            results.len()
        );

        // Each solution should constrain exactly one fact
        for (i, result) in results.iter().enumerate() {
        match result {
                SolveResult::Solved { fact_constraints } | SolveResult::Partial { fact_constraints, .. } => {
                    assert!(
                        !fact_constraints.is_empty(),
                        "solution {} should have constraints",
                        i
                    );
                }
                SolveResult::Unsatisfiable { .. } => {
                    panic!("solution {} should not be unsatisfiable", i);
                }
            }
        }
    }

    /// OR inside AND: `x > 0 ∧ (y = 1 ∨ y = 2)` → Two solutions
    ///
    /// Uses proper equation structure with result value.
    /// The condition contains AND-over-OR which must be distributed.
    #[test]
    fn test_or_inside_and_distributed() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());
        let fact_y = FactPath::local("y".to_string());

        // x > 0
        let x_gt_0 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(ExpressionKind::FactPath(fact_x.clone()), None)),
                ComparisonComputation::GreaterThan,
                Box::new(Expression::new(ExpressionKind::Literal(num(0)), None)),
            ),
            None,
        );

        // y = 1
        let y_eq_1 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(ExpressionKind::FactPath(fact_y.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(Expression::new(ExpressionKind::Literal(num(1)), None)),
            ),
            None,
        );

        // y = 2
        let y_eq_2 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(ExpressionKind::FactPath(fact_y.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(Expression::new(ExpressionKind::Literal(num(2)), None)),
            ),
            None,
        );

        // (y = 1 ∨ y = 2)
        let y_or = Expression::new(
            ExpressionKind::LogicalOr(Box::new(y_eq_1), Box::new(y_eq_2)),
            None,
        );

        // Condition: x > 0 ∧ (y = 1 ∨ y = 2)
        let condition = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(x_gt_0), Box::new(y_or)),
            None,
        );

        // Equation: condition ∧ result (where result is a literal value 42)
        let result_value = Expression::new(
            ExpressionKind::Literal(num(42)),
            None,
        );
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(condition), Box::new(result_value)),
            None,
        );

        // Target: result == 42
        let target = Target::value(num(42));
        let results = solve(equation, &target);

        // Should have 2 solutions:
        // 1. {x > 0, y = 1}
        // 2. {x > 0, y = 2}
        assert_eq!(
            results.len(),
            2,
            "AND-over-OR should produce 2 solutions, got {}",
            results.len()
        );

        // Both solutions should have constraints on both x and y
        for (i, result) in results.iter().enumerate() {
            match result {
                SolveResult::Solved { fact_constraints } | SolveResult::Partial { fact_constraints, .. } => {
                    assert!(
                        fact_constraints.contains_key(&fact_x),
                        "solution {} should have x constraint",
                        i
                    );
                    assert!(
                        fact_constraints.contains_key(&fact_y),
                        "solution {} should have y constraint",
                        i
                    );
                }
                SolveResult::Unsatisfiable { .. } => {
                    panic!("solution {} should not be unsatisfiable", i);
                }
            }
        }
    }

    /// Test flatten_or produces correct number of branches
    #[test]
    fn test_flatten_or_basic() {
        let a = literal_bool(true);
        let b = literal_bool(false);
        let c = Expression::new(
            ExpressionKind::FactPath(FactPath::local("x".to_string())),
            None,
        );

        // Single expression
        let branches = flatten_or(a.clone());
        assert_eq!(branches.len(), 1);

        // Simple OR
        let or_expr = Expression::new(
            ExpressionKind::LogicalOr(Box::new(a.clone()), Box::new(b.clone())),
            None,
        );
        let branches = flatten_or(or_expr);
        assert_eq!(branches.len(), 2);

        // Nested OR: (a ∨ b) ∨ c
        let nested = Expression::new(
            ExpressionKind::LogicalOr(
                Box::new(Expression::new(
                    ExpressionKind::LogicalOr(Box::new(a.clone()), Box::new(b.clone())),
                    None,
                )),
                Box::new(c),
            ),
            None,
        );
        let branches = flatten_or(nested);
        assert_eq!(branches.len(), 3);
    }

    /// Test expand distributes AND over OR correctly (DNF)
    #[test]
    fn test_expand_distributes_and_over_or() {
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

        // A ∧ (B ∨ C) should become (A ∧ B) ∨ (A ∧ C)
        let b_or_c = Expression::new(
            ExpressionKind::LogicalOr(Box::new(b), Box::new(c)),
            None,
        );
        let a_and_b_or_c = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(a), Box::new(b_or_c)),
            None,
        );

        let dnf = expand(a_and_b_or_c);
        let branches = flatten_or(dnf);

        // Should have 2 branches after distribution
        assert_eq!(
            branches.len(),
            2,
            "A ∧ (B ∨ C) should produce 2 DNF branches"
        );
    }

    // ========================================================================
    // Algebraic Solving Tests
    // ========================================================================

    fn fact_expr(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::FactPath(FactPath::local(name.to_string())),
            None,
        )
    }

    fn num_expr(n: i64) -> Expression {
        Expression::new(ExpressionKind::Literal(num(n)), None)
    }

    fn arith(left: Expression, op: ArithmeticComputation, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::Arithmetic(Box::new(left), op, Box::new(right)),
            None,
        )
    }

    /// `x + 5 == 10` → `x == 5`
    #[test]
    fn test_isolate_addition() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        // Equation: (true ∧ (x + 5))
        let x_plus_5 = arith(fact_expr("x"), ArithmeticComputation::Add, num_expr(5));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(x_plus_5)),
            None,
        );

        // Target: result == 10
        let target = Target::value(num(10));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1, "should have 1 solution");
        match &results[0] {
            SolveResult::Solved { fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(5), "x should equal 5");
                    }
                    _ => panic!("expected enumeration constraint, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", results[0]),
        }
    }

    /// `x - 3 == 7` → `x == 10`
    #[test]
    fn test_isolate_subtraction() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        let x_minus_3 = arith(fact_expr("x"), ArithmeticComputation::Subtract, num_expr(3));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(x_minus_3)),
            None,
        );

        let target = Target::value(num(7));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Solved { fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(10), "x should equal 10");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", results[0]),
        }
    }

    /// `x * 3 == 15` → `x == 5`
    #[test]
    fn test_isolate_multiplication() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        let x_times_3 = arith(fact_expr("x"), ArithmeticComputation::Multiply, num_expr(3));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(x_times_3)),
            None,
        );

        let target = Target::value(num(15));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Solved { fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(5), "x should equal 5");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", results[0]),
        }
    }

    /// `x / 2 == 10` → `x == 20`
    #[test]
    fn test_isolate_division() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        let x_div_2 = arith(fact_expr("x"), ArithmeticComputation::Divide, num_expr(2));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(x_div_2)),
            None,
        );

        let target = Target::value(num(10));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Solved { fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(20), "x should equal 20");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", results[0]),
        }
    }

    /// `10 - x == 3` → `x == 7`
    #[test]
    fn test_isolate_subtraction_from_constant() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        // 10 - x
        let ten_minus_x = arith(num_expr(10), ArithmeticComputation::Subtract, fact_expr("x"));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(ten_minus_x)),
            None,
        );

        let target = Target::value(num(3));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Solved { fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(7), "x should equal 7");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", results[0]),
        }
    }

    /// `x * 0 == 5` → unsatisfiable
    #[test]
    fn test_multiply_by_zero_nonzero_target() {
        use crate::inversion::Target;

        let x_times_0 = arith(fact_expr("x"), ArithmeticComputation::Multiply, num_expr(0));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(x_times_0)),
            None,
        );

        let target = Target::value(num(5));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], SolveResult::Unsatisfiable { .. }),
            "x * 0 == 5 should be unsatisfiable, got {:?}",
            results[0]
        );
    }

    /// `x * 0 == 0` → unconstrained
    #[test]
    fn test_multiply_by_zero_zero_target() {
        use crate::inversion::Target;

        let x_times_0 = arith(fact_expr("x"), ArithmeticComputation::Multiply, num_expr(0));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(x_times_0)),
            None,
        );

        let target = Target::value(num(0));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Solved { fact_constraints } => {
                // Unconstrained means no constraints on x
                assert!(
                    fact_constraints.is_empty(),
                    "x * 0 == 0 should be unconstrained, got constraints: {:?}",
                    fact_constraints
                );
            }
            _ => panic!("expected Solved (unconstrained), got {:?}", results[0]),
        }
    }

    /// `(x + 5) * 2 == 30` → `x == 10`
    #[test]
    fn test_nested_arithmetic() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        // (x + 5) * 2
        let x_plus_5 = arith(fact_expr("x"), ArithmeticComputation::Add, num_expr(5));
        let nested = arith(x_plus_5, ArithmeticComputation::Multiply, num_expr(2));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(nested)),
            None,
        );

        let target = Target::value(num(30));
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Solved { fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(10), "x should equal 10");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", results[0]),
        }
    }

    /// `x * (-2) > 10` → `x < -5` (inequality flipping)
    #[test]
    fn test_inequality_with_negative_multiplier() {
        use crate::computation::Bound;
        use crate::inversion::Target;
        use crate::inversion::TargetOp;

        let fact_x = FactPath::local("x".to_string());

        // x * (-2)
        let x_times_neg2 = arith(fact_expr("x"), ArithmeticComputation::Multiply, num_expr(-2));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(x_times_neg2)),
            None,
        );

        // Target: result > 10
        let target = Target {
            outcome: Some(OperationResult::Value(num(10))),
            op: TargetOp::Gt,
        };
        let results = solve(equation, &target);

        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Solved { fact_constraints } | SolveResult::Partial { fact_constraints, .. } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Range { max, .. } => {
                        // x < -5 means max is Exclusive(-5)
                        match max {
                            Bound::Exclusive(val) => {
                                assert_eq!(*val, num(-5), "max should be -5");
                            }
                            _ => panic!("expected exclusive bound, got {:?}", max),
                        }
                    }
                    _ => panic!("expected range constraint, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved or Partial, got {:?}", results[0]),
        }
    }

    /// Test multiple unknowns: `x + y + 10 == 100` → simplified constraint `x + y == 90`
    #[test]
    fn test_multiple_unknowns_constant_simplification() {
        use crate::inversion::Target;

        // x + y + 10
        let x_plus_y = arith(fact_expr("x"), ArithmeticComputation::Add, fact_expr("y"));
        let x_plus_y_plus_10 = arith(x_plus_y, ArithmeticComputation::Add, num_expr(10));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Box::new(literal_bool(true)), Box::new(x_plus_y_plus_10)),
            None,
        );

        let target = Target::value(num(100));
        let results = solve(equation, &target);

        // Should have a partial result with simplified constraint
        assert_eq!(results.len(), 1);
        match &results[0] {
            SolveResult::Partial {
                remaining_constraints,
                ..
            } => {
                // Should have a simplified constraint x + y == 90
                assert!(
                    !remaining_constraints.is_empty(),
                    "should have remaining constraint"
                );
                // The constraint should be a comparison with 90 on the right
                let constraint = &remaining_constraints[0];
                if let ExpressionKind::Comparison(_, _, right) = &constraint.kind {
                    if let ExpressionKind::Literal(LiteralValue::Number(n)) = &right.kind {
                        assert_eq!(*n, Decimal::from(90), "should have simplified to x + y == 90");
                    } else {
                        panic!("right side should be literal 90");
                    }
                } else {
                    panic!("should be a comparison expression");
                }
            }
            SolveResult::Solved { .. } => {
                // If it's fully solved, that's also acceptable
                // (means both facts are unconstrained which shouldn't happen here)
                panic!("should be Partial with simplified constraint, not Solved");
            }
            _ => panic!("expected Partial, got {:?}", results[0]),
        }
    }

    /// Test collect_facts
    #[test]
    fn test_collect_facts() {
        let expr = arith(
            arith(fact_expr("x"), ArithmeticComputation::Add, num_expr(5)),
            ArithmeticComputation::Multiply,
            fact_expr("y"),
        );

        let facts = collect_facts(&expr);
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&FactPath::local("x".to_string())));
        assert!(facts.contains(&FactPath::local("y".to_string())));
    }

    /// Test contains_fact
    #[test]
    fn test_contains_fact() {
        let fact_x = FactPath::local("x".to_string());
        let fact_y = FactPath::local("y".to_string());
        let fact_z = FactPath::local("z".to_string());

        let expr = arith(fact_expr("x"), ArithmeticComputation::Add, num_expr(5));

        assert!(contains_fact(&expr, &fact_x));
        assert!(!contains_fact(&expr, &fact_y));
        assert!(!contains_fact(&expr, &fact_z));
    }

    // ========================================================================
    // Veto Target Integration Tests
    // ========================================================================

    /// Test that veto branches are eliminated when solving for a value target.
    /// Equation: (y >= 0 ∧ veto) ∨ (y < 0 ∧ 25)
    /// Target: x == 25
    /// Expected: veto branch eliminated, only y < 0 branch remains
    #[test]
    fn test_veto_branch_eliminated_for_value_target() {
        use crate::inversion::Target;
        use crate::semantic::VetoExpression;

        let fact_y = FactPath::local("y".to_string());

        // Branch 1: y >= 0 ∧ veto
        let y_gte_0 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::GreaterThanOrEqual,
                Box::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let veto_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(y_gte_0),
                Box::new(Expression::new(
                    ExpressionKind::Veto(VetoExpression { message: None }),
                    None,
                )),
            ),
            None,
        );

        // Branch 2: y < 0 ∧ 25
        let y_lt_0 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::LessThan,
                Box::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let value_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(y_lt_0),
                Box::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::from(25))),
                    None,
                )),
            ),
            None,
        );

        // Equation: branch1 ∨ branch2
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Box::new(veto_branch), Box::new(value_branch)),
            None,
        );

        let target = Target::value(LiteralValue::Number(Decimal::from(25)));
        let results = solve(equation, &target);

        // Veto branch should be eliminated, only value branch remains
        assert_eq!(
            results.len(),
            1,
            "should have exactly one solution (veto branch eliminated)"
        );
        assert!(
            !matches!(results[0], SolveResult::Unsatisfiable { .. }),
            "should find a valid solution"
        );
    }

    /// Test that any_veto target returns only veto branches.
    /// Equation: (y >= 0 ∧ veto "A") ∨ (y < 0 ∧ 25)
    /// Target: any_veto
    /// Expected: only y >= 0 branch remains (the veto branch)
    #[test]
    fn test_any_veto_target_returns_veto_branches() {
        use crate::inversion::Target;
        use crate::semantic::VetoExpression;

        let fact_y = FactPath::local("y".to_string());

        // Branch 1: y >= 0 ∧ veto "A"
        let y_gte_0 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::GreaterThanOrEqual,
                Box::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let veto_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(y_gte_0),
                Box::new(Expression::new(
                    ExpressionKind::Veto(VetoExpression {
                        message: Some("A".to_string()),
                    }),
                    None,
                )),
            ),
            None,
        );

        // Branch 2: y < 0 ∧ 25
        let y_lt_0 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::LessThan,
                Box::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let value_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(y_lt_0),
                Box::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::from(25))),
                    None,
                )),
            ),
            None,
        );

        // Equation: branch1 ∨ branch2
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Box::new(veto_branch), Box::new(value_branch)),
            None,
        );

        let target = Target::any_veto();
        let results = solve(equation, &target);

        // Value branch should be eliminated, only veto branch remains
        assert_eq!(
            results.len(),
            1,
            "should have exactly one solution (value branch eliminated)"
        );
        assert!(
            !matches!(results[0], SolveResult::Unsatisfiable { .. }),
            "should find a valid solution"
        );
    }

    /// Test that specific veto target matches only vetos with the correct message.
    /// Equation: (y < 0 ∧ veto "A") ∨ (y >= 0 ∧ veto "B")
    /// Target: veto("A")
    /// Expected: only y < 0 branch remains (matching message)
    #[test]
    fn test_specific_veto_target_matches_message() {
        use crate::inversion::Target;
        use crate::semantic::VetoExpression;

        let fact_y = FactPath::local("y".to_string());

        // Branch 1: y < 0 ∧ veto "A"
        let y_lt_0 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::LessThan,
                Box::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let veto_a_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(y_lt_0),
                Box::new(Expression::new(
                    ExpressionKind::Veto(VetoExpression {
                        message: Some("A".to_string()),
                    }),
                    None,
                )),
            ),
            None,
        );

        // Branch 2: y >= 0 ∧ veto "B"
        let y_gte_0 = Expression::new(
            ExpressionKind::Comparison(
                Box::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::GreaterThanOrEqual,
                Box::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let veto_b_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(y_gte_0),
                Box::new(Expression::new(
                    ExpressionKind::Veto(VetoExpression {
                        message: Some("B".to_string()),
                    }),
                    None,
                )),
            ),
            None,
        );

        // Equation: branch1 ∨ branch2
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Box::new(veto_a_branch), Box::new(veto_b_branch)),
            None,
        );

        let target = Target::veto("A".to_string());
        let results = solve(equation, &target);

        // Veto "B" branch should be eliminated, only veto "A" branch remains
        assert_eq!(
            results.len(),
            1,
            "should have exactly one solution (non-matching veto branch eliminated)"
        );
        assert!(
            !matches!(results[0], SolveResult::Unsatisfiable { .. }),
            "should find a valid solution"
        );
    }
}
