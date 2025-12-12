//! Constraint solver for inversion
//!
//! Target-aware constraint solving for determining what inputs produce desired outputs.
//! Uses the computation module for constraint types and operations.

use crate::computation::{
    collect_domain_restrictions, reduce, reverse_comparison, ConstraintSet, DomainRestriction,
    FactConstraint, OperationResult, UnsatReason,
};
use crate::semantic::{
    BooleanValue, ComparisonComputation, Expression, ExpressionKind, FactPath, LiteralValue,
};
use std::collections::HashMap;

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

// ============================================================================
// DNF Conversion (Disjunctive Normal Form)
// ============================================================================

/// Convert an expression to Disjunctive Normal Form (OR of ANDs)
///
/// Distributes AND over OR: `A ∧ (B ∨ C)` → `(A ∧ B) ∨ (A ∧ C)`
/// This enables solving each OR branch independently.
fn to_dnf(expression: Expression) -> Expression {
    match expression.kind {
        ExpressionKind::LogicalAnd(left, right) => {
            let left_dnf = to_dnf(*left);
            let right_dnf = to_dnf(*right);

            // Distribute AND over OR
            // (A ∨ B) ∧ (C ∨ D) → (A ∧ C) ∨ (A ∧ D) ∨ (B ∧ C) ∨ (B ∧ D)
            let left_branches = flatten_or(left_dnf);
            let right_branches = flatten_or(right_dnf);

            let mut result_branches: Vec<Expression> = Vec::new();

            for left_branch in &left_branches {
                for right_branch in &right_branches {
                    let combined = Expression::new(
                        ExpressionKind::LogicalAnd(
                            Box::new(left_branch.clone()),
                            Box::new(right_branch.clone()),
                        ),
                        None,
                    );
                    result_branches.push(combined);
                }
            }

            combine_with_or(result_branches)
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_dnf = to_dnf(*left);
            let right_dnf = to_dnf(*right);
            Expression::new(
                ExpressionKind::LogicalOr(Box::new(left_dnf), Box::new(right_dnf)),
                expression.source,
            )
        }

        ExpressionKind::LogicalNegation(inner, negation_type) => {
            // For negation, we need to push it down (De Morgan's laws)
            // NOT(A ∨ B) → NOT(A) ∧ NOT(B)
            // NOT(A ∧ B) → NOT(A) ∨ NOT(B)
            match inner.kind {
                ExpressionKind::LogicalOr(left, right) => {
                    // NOT(A ∨ B) → NOT(A) ∧ NOT(B)
                    let not_left = Expression::new(
                        ExpressionKind::LogicalNegation(left, negation_type.clone()),
                        None,
                    );
                    let not_right = Expression::new(
                        ExpressionKind::LogicalNegation(right, negation_type),
                        None,
                    );
                    to_dnf(Expression::new(
                        ExpressionKind::LogicalAnd(Box::new(not_left), Box::new(not_right)),
                        expression.source,
                    ))
                }
                ExpressionKind::LogicalAnd(left, right) => {
                    // NOT(A ∧ B) → NOT(A) ∨ NOT(B)
                    let not_left = Expression::new(
                        ExpressionKind::LogicalNegation(left, negation_type.clone()),
                        None,
                    );
                    let not_right = Expression::new(
                        ExpressionKind::LogicalNegation(right, negation_type),
                        None,
                    );
                    to_dnf(Expression::new(
                        ExpressionKind::LogicalOr(Box::new(not_left), Box::new(not_right)),
                        expression.source,
                    ))
                }
                ExpressionKind::LogicalNegation(double_inner, _) => {
                    // NOT(NOT(A)) → A
                    to_dnf(*double_inner)
                }
                _ => {
                    // Negation of non-logical expression stays as-is
                    Expression::new(
                        ExpressionKind::LogicalNegation(inner, negation_type),
                        expression.source,
                    )
                }
            }
        }

        // Non-logical expressions are already in DNF (single term)
        _ => expression,
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

    // 2. Reduce the expression (algebraic + function range checks)
    let reduced = reduce(constrained);

    // 3. Convert to DNF to handle OR inside AND
    let dnf = to_dnf(reduced);

    // 4. Reduce again after DNF conversion (may simplify further)
    let dnf_reduced = reduce(dnf);

    // 5. Flatten OR branches and solve each independently
    let branches = flatten_or(dnf_reduced);

    // 6. Solve each branch
    let mut results: Vec<SolveResult> = Vec::new();
    for branch in branches {
        let branch_reduced = reduce(branch);
        let result = solve_single_branch(branch_reduced);

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
            // Try to extract fact op literal
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

        // After reduction, `true ∨ X` becomes `true`, so we get one unconstrained solution
        // The reduce() function should simplify this before we get to solve
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

    /// Test to_dnf distributes AND over OR correctly
    #[test]
    fn test_to_dnf_distributes_and_over_or() {
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

        let dnf = to_dnf(a_and_b_or_c);
        let branches = flatten_or(dnf);

        // Should have 2 branches after distribution
        assert_eq!(
            branches.len(),
            2,
            "A ∧ (B ∨ C) should produce 2 DNF branches"
        );
    }
}
