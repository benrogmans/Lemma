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

pub use crate::algebra::constraints::{Bound, DomainRestriction, FactConstraint, UnsatReason};
pub use response::{InversionResponse, Solution};

use crate::computation::OperationResult;
use crate::planning::ExecutionPlan;
use crate::semantic::{Expression, ExpressionKind, FactValue};
use crate::{LemmaError, LemmaResult};
use std::sync::Arc;

/// Substitute provided fact values into an equation
fn substitute_fact_values(expression: Expression, plan: &ExecutionPlan) -> Expression {
    let source = expression.source.clone();
    
    match &expression.kind {
        ExpressionKind::FactPath(fact_path) => {
            // Check if this fact has a literal value in the plan
            if let Some(fact) = plan.facts.get(fact_path) {
                if let FactValue::Literal(lit_val) = &fact.value {
                    // Replace with literal value
                    return Expression::new(
                        ExpressionKind::Literal(lit_val.clone()),
                        source,
                    );
                }
            }
            // No substitution, return as-is
            expression
        }
        
        // Recursively substitute in compound expressions
        ExpressionKind::LogicalAnd(left, right) => {
            let left_sub = substitute_fact_values(Arc::unwrap_or_clone(left.clone()), plan);
            let right_sub = substitute_fact_values(Arc::unwrap_or_clone(right.clone()), plan);
            Expression::new(
                ExpressionKind::LogicalAnd(Arc::new(left_sub), Arc::new(right_sub)),
                source,
            )
        }
        
        ExpressionKind::LogicalOr(left, right) => {
            let left_sub = substitute_fact_values(Arc::unwrap_or_clone(left.clone()), plan);
            let right_sub = substitute_fact_values(Arc::unwrap_or_clone(right.clone()), plan);
            Expression::new(
                ExpressionKind::LogicalOr(Arc::new(left_sub), Arc::new(right_sub)),
                source,
            )
        }
        
        ExpressionKind::LogicalNegation(inner, negation_type) => {
            let inner_sub = substitute_fact_values(Arc::unwrap_or_clone(inner.clone()), plan);
            Expression::new(
                ExpressionKind::LogicalNegation(Arc::new(inner_sub), negation_type.clone()),
                source,
            )
        }
        
        ExpressionKind::Arithmetic(left, op, right) => {
            let left_sub = substitute_fact_values(Arc::unwrap_or_clone(left.clone()), plan);
            let right_sub = substitute_fact_values(Arc::unwrap_or_clone(right.clone()), plan);
            Expression::new(
                ExpressionKind::Arithmetic(Arc::new(left_sub), op.clone(), Arc::new(right_sub)),
                source,
            )
        }
        
        ExpressionKind::Comparison(left, op, right) => {
            let left_sub = substitute_fact_values(Arc::unwrap_or_clone(left.clone()), plan);
            let right_sub = substitute_fact_values(Arc::unwrap_or_clone(right.clone()), plan);
            Expression::new(
                ExpressionKind::Comparison(Arc::new(left_sub), op.clone(), Arc::new(right_sub)),
                source,
            )
        }
        
        ExpressionKind::UnitConversion(inner, target) => {
            let inner_sub = substitute_fact_values(Arc::unwrap_or_clone(inner.clone()), plan);
            Expression::new(
                ExpressionKind::UnitConversion(Arc::new(inner_sub), target.clone()),
                source,
            )
        }
        
        ExpressionKind::MathematicalComputation(op, inner) => {
            let inner_sub = substitute_fact_values(Arc::unwrap_or_clone(inner.clone()), plan);
            Expression::new(
                ExpressionKind::MathematicalComputation(op.clone(), Arc::new(inner_sub)),
                source,
            )
        }
        
        // These don't contain facts to substitute
        ExpressionKind::Literal(_) 
        | ExpressionKind::Veto(_)
        | ExpressionKind::RulePath(_)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::RuleReference(_) => expression,
    }
}

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
    todo!("Inversion broken - Phase 1 deletion")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{Branch, ExecutableRule};
    use crate::semantic::{BooleanValue, Expression, ExpressionKind, LiteralValue, RulePath};
    use rust_decimal::Decimal;
    use std::sync::Arc;
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
                Arc::new(literal_bool(true)),
                Arc::new(literal_num(42)),
            ),
            None,
        );
        let rule = ExecutableRule {
            path: rule_path("test"),
            name: "test".to_string(),
            branches: vec![Branch {
                condition: literal_bool(true),
                optimized_condition: None,
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
                Arc::new(literal_bool(true)),
                Arc::new(literal_num(42)),
            ),
            None,
        );
        let rule = ExecutableRule {
            path: rule_path("test"),
            name: "test".to_string(),
            branches: vec![Branch {
                condition: literal_bool(true),
                optimized_condition: None,
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
