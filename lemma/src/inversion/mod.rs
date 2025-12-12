//! Constraint-based inverse reasoning for Lemma rules
//!
//! Determines what inputs produce desired outputs by building and solving constraint equations.
//!
//! The main entry point is [`invert()`], which returns an [`InversionResponse`]
//! containing all valid solutions with their domains.
//!
//! ## Algorithm
//!
//! 1. **Build equations**: During planning, equations are built and simplified.
//!
//! 2. **Apply target**: Transform equation with target constraint.
//!
//! 3. **Solve**: Constraint solving with function range checking, domain restrictions,
//!    and exact symbolic preservation.

mod response;
mod solver;

pub use crate::computation::{Bound, DomainRestriction, FactConstraint, UnsatReason};
pub use response::{InversionResponse, Solution};
pub use solver::SolveResult;

use crate::computation::OperationResult;
use crate::planning::ExecutionPlan;
use crate::semantic::{ComparisonComputation, EqualityNotation};
use crate::{LemmaError, LemmaResult};

use solver::solve;

/// Target specification for an inversion query
///
/// Use the `invert()` function's string operator for simpler API,
/// or construct a Target directly for Engine methods.
#[derive(Debug, Clone)]
pub struct Target {
    /// Comparison operator
    pub op: TargetOp,

    /// Desired outcome (value or veto), or None for any_value
    pub outcome: Option<OperationResult>,
}

impl Target {
    /// Create a target for equality with a specific value
    pub fn value(value: crate::LiteralValue) -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Value(value)),
        }
    }

    /// Create a target for any value (all possible outcomes)
    pub fn any_value() -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: None,
        }
    }

    /// Create a target that matches any veto outcome (regardless of message)
    pub fn any_veto() -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Veto(None)),
        }
    }

    /// Create a target that matches a veto with a specific message
    pub fn veto(message: String) -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Veto(Some(message))),
        }
    }

    /// Create a target with a custom operator
    pub fn with_op(op: TargetOp, outcome: OperationResult) -> Self {
        Self {
            op,
            outcome: Some(outcome),
        }
    }
}

/// Comparison operators for targets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOp {
    /// Equal to (=)
    Eq,
    /// Not equal to (!=)
    Neq,
    /// Less than (<)
    Lt,
    /// Less than or equal to (<=)
    Lte,
    /// Greater than (>)
    Gt,
    /// Greater than or equal to (>=)
    Gte,
}

impl TargetOp {
    /// Parse operator from string
    fn from_str(op: &str) -> Result<Self, LemmaError> {
        match op {
            "=" | "==" => Ok(TargetOp::Eq),
            "!=" | "<>" => Ok(TargetOp::Neq),
            "<" => Ok(TargetOp::Lt),
            "<=" => Ok(TargetOp::Lte),
            ">" => Ok(TargetOp::Gt),
            ">=" => Ok(TargetOp::Gte),
            _ => Err(LemmaError::Engine(format!(
                "Invalid comparison operator: {}. Expected one of: =, !=, <, <=, >, >=",
                op
            ))),
        }
    }

    /// Convert to ComparisonComputation
    pub(crate) fn to_comparison(&self) -> ComparisonComputation {
        match self {
            TargetOp::Eq => ComparisonComputation::Equal(EqualityNotation::Symbol),
            TargetOp::Neq => ComparisonComputation::NotEqual(EqualityNotation::Symbol),
            TargetOp::Lt => ComparisonComputation::LessThan,
            TargetOp::Lte => ComparisonComputation::LessThanOrEqual,
            TargetOp::Gt => ComparisonComputation::GreaterThan,
            TargetOp::Gte => ComparisonComputation::GreaterThanOrEqual,
        }
    }
}

/// Invert a rule to find input fact values that produce a desired outcome.
///
/// Given an execution plan and rule name, determines what values the unknown
/// facts must have to produce the target outcome.
///
/// # Arguments
///
/// * `plan` - The execution plan containing the rule
/// * `rule_name` - Name of the rule to invert (local rule in main document)
/// * `operator` - Comparison operator: "=", "!=", "<", "<=", ">", ">="
/// * `outcome` - Desired result, or None for any_value (returns all possible outcomes)
///
/// # Returns
///
/// An `InversionResponse` containing all valid solutions, each with:
/// - The outcome it produces
/// - Fact constraints that produce that outcome
///
/// # Examples
///
/// ```ignore
/// // Find inputs where rate == 15%
/// let response = invert(&plan, "rate", "=", Some(OperationResult::Value(fifteen_percent)))?;
///
/// // Find all possible outcomes
/// let response = invert(&plan, "rate", "=", None)?;
///
/// // Find inputs where discount > 100
/// let response = invert(&plan, "discount", ">", Some(OperationResult::Value(hundred)))?;
///
/// // Find inputs that cause veto
/// let response = invert(&plan, "can_ship", "=", Some(OperationResult::Veto(None)))?;
/// ```
pub fn invert(
    plan: &ExecutionPlan,
    rule_name: &str,
    operator: &str,
    outcome: Option<OperationResult>,
) -> LemmaResult<InversionResponse> {
    // Parse operator
    let op = TargetOp::from_str(operator)?;

    // Create target
    let target = Target { op, outcome };

    // Find the target rule
    let target_rule = plan.get_rule(rule_name).ok_or_else(|| {
        LemmaError::Engine(format!("Rule not found: {}.{}", plan.doc_name, rule_name))
    })?;

    // Solve the equation with the target constraint
    // Returns multiple solutions when equation contains OR branches
    let solve_results = solve(target_rule.equation.clone(), &target);

    // Convert each solve result to a solution
    let mut solutions: Vec<Solution> = Vec::new();

    for solve_result in solve_results {
        match solve_result {
            SolveResult::Solved { fact_constraints } => {
                let solution = Solution::new(
                    target.outcome.clone().unwrap_or(OperationResult::Value(
                        crate::LiteralValue::Boolean(crate::BooleanValue::True),
                    )),
                    fact_constraints,
                );
                solutions.push(solution);
            }
            SolveResult::Partial {
                fact_constraints,
                remaining_constraints: _,
                domain_restrictions: _,
            } => {
                let solution = Solution::new(
                    target.outcome.clone().unwrap_or(OperationResult::Value(
                        crate::LiteralValue::Boolean(crate::BooleanValue::True),
                    )),
                    fact_constraints,
                );
                solutions.push(solution);
            }
            SolveResult::Unsatisfiable { .. } => {
                // Unsatisfiable branches are filtered out (OR-3)
                // If this is the only result, we'll return error below
            }
        }
    }

    // If no valid solutions, return error
    if solutions.is_empty() {
        return Err(LemmaError::Engine(format!(
            "No solution: rule '{}' cannot produce the requested outcome",
            rule_name
        )));
    }

    Ok(InversionResponse::new(solutions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{Branch, ExecutableRule};
    use crate::semantic::{BooleanValue, Expression, ExpressionKind, LiteralValue, RulePath};
    use rust_decimal::Decimal;
    use std::collections::{HashMap, HashSet};

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
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(n))),
            None,
        )
    }

    fn rule_path(name: &str) -> RulePath {
        RulePath::local(name.to_string())
    }

    #[test]
    fn test_target_op_from_str() {
        assert_eq!(TargetOp::from_str("=").unwrap(), TargetOp::Eq);
        assert_eq!(TargetOp::from_str("==").unwrap(), TargetOp::Eq);
        assert_eq!(TargetOp::from_str("!=").unwrap(), TargetOp::Neq);
        assert_eq!(TargetOp::from_str("<").unwrap(), TargetOp::Lt);
        assert_eq!(TargetOp::from_str("<=").unwrap(), TargetOp::Lte);
        assert_eq!(TargetOp::from_str(">").unwrap(), TargetOp::Gt);
        assert_eq!(TargetOp::from_str(">=").unwrap(), TargetOp::Gte);
        assert!(TargetOp::from_str("invalid").is_err());
    }

    #[test]
    fn test_invert_simple_rule() {
        // Create a simple rule: result = 42 (always)
        // Equation: true AND 42
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(literal_bool(true)),
                Box::new(literal_num(42)),
            ),
            None,
        );
        let rule = ExecutableRule {
            path: rule_path("test"),
            name: "test".to_string(),
            branches: vec![Branch {
                condition: literal_bool(true),
                result: literal_num(42),
                source: None,
            }],
            needs_facts: HashSet::new(),
            equation,
            source: None,
        };

        let plan = ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![rule],
            crate::planning::graph::Graph::empty(),
        );

        // Invert: find inputs where test == 42
        let response = invert(
            &plan,
            "test",
            "=",
            Some(OperationResult::Value(LiteralValue::Number(Decimal::from(
                42,
            )))),
        )
        .unwrap();

        // Should have one solution with no constraints (any input works)
        assert!(response.has_solutions());
    }

    #[test]
    fn test_invert_rule_not_found() {
        let plan = ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![],
            crate::planning::graph::Graph::empty(),
        );

        let result = invert(
            &plan,
            "nonexistent",
            "=",
            Some(OperationResult::Value(LiteralValue::Number(Decimal::from(
                42,
            )))),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_invert_invalid_operator() {
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(literal_bool(true)),
                Box::new(literal_num(42)),
            ),
            None,
        );
        let rule = ExecutableRule {
            path: rule_path("test"),
            name: "test".to_string(),
            branches: vec![Branch {
                condition: literal_bool(true),
                result: literal_num(42),
                source: None,
            }],
            needs_facts: HashSet::new(),
            equation,
            source: None,
        };

        let plan = ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![rule],
            crate::planning::graph::Graph::empty(),
        );

        let result = invert(
            &plan,
            "test",
            "~=", // Invalid operator
            Some(OperationResult::Value(LiteralValue::Number(Decimal::from(
                42,
            )))),
        );

        assert!(result.is_err());
    }
}
