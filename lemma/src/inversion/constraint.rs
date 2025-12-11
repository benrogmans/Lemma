//! RuleConstraint type for inversion
//!
//! Represents boolean constraints over facts. Unlike `Expression`, this type:
//! - Does not require source location information
//! - Only represents the subset of expressions valid in constraints
//! - Makes invalid states unrepresentable
//!
//! Includes BDD-based simplification for contradiction detection.
//! For semantic analysis (e.g., `x == A and x != B`), use domain extraction.

use crate::{ComparisonComputation, FactPath, LemmaError, LemmaResult, LiteralValue};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::fmt;

/// A boolean constraint over facts
///
/// Used internally by inversion to represent conditions under which
/// a solution applies. Converted from `Expression` at the boundary
/// when reading from the execution plan.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleConstraint {
    /// Always true
    True,
    /// Always false (unsatisfiable)
    False,
    /// Comparison: fact op value (e.g., `age > 18`)
    Comparison {
        fact: FactPath,
        op: ComparisonComputation,
        value: LiteralValue,
    },
    /// Boolean fact reference (e.g., `is_employee` meaning `is_employee == true`)
    Fact(FactPath),
    /// Logical AND of two constraints
    And(Box<RuleConstraint>, Box<RuleConstraint>),
    /// Logical OR of two constraints
    Or(Box<RuleConstraint>, Box<RuleConstraint>),
    /// Logical NOT of a constraint
    Not(Box<RuleConstraint>),
}

impl RuleConstraint {
    /// Check if this constraint is trivially true
    pub fn is_true(&self) -> bool {
        matches!(self, RuleConstraint::True)
    }

    /// Check if this constraint is trivially false
    pub fn is_false(&self) -> bool {
        matches!(self, RuleConstraint::False)
    }

    /// Combine two constraints with AND, applying short-circuit simplification
    pub fn and(self, other: RuleConstraint) -> RuleConstraint {
        if self.is_false() || other.is_false() {
            return RuleConstraint::False;
        }
        if self.is_true() {
            return other;
        }
        if other.is_true() {
            return self;
        }
        RuleConstraint::And(Box::new(self), Box::new(other))
    }

    /// Combine two constraints with OR, applying short-circuit simplification
    pub fn or(self, other: RuleConstraint) -> RuleConstraint {
        if self.is_true() || other.is_true() {
            return RuleConstraint::True;
        }
        if self.is_false() {
            return other;
        }
        if other.is_false() {
            return self;
        }
        RuleConstraint::Or(Box::new(self), Box::new(other))
    }

    /// Negate this constraint
    pub fn not(self) -> RuleConstraint {
        match self {
            RuleConstraint::True => RuleConstraint::False,
            RuleConstraint::False => RuleConstraint::True,
            RuleConstraint::Not(inner) => *inner,
            other => RuleConstraint::Not(Box::new(other)),
        }
    }

    /// Simplify this constraint using BDD-based simplification
    ///
    /// This method:
    /// 1. Converts the constraint to a BDD expression
    /// 2. Simplifies using boolean algebra to detect contradictions
    ///
    /// The primary purpose is contradiction detection (returning `RuleConstraint::False`).
    /// For actual output, use domains extracted from the constraint instead.
    pub fn simplify(self) -> LemmaResult<RuleConstraint> {
        let mut atoms: Vec<RuleConstraint> = Vec::new();
        if let Some(bexpr) = to_bool_expr(&self, &mut atoms) {
            const MAX_ATOMS: usize = 64;
            if atoms.len() <= MAX_ATOMS {
                let simplified = bexpr.simplify_via_bdd();
                return Ok(from_bool_expr(&simplified, &atoms));
            }
        }

        Ok(self)
    }

    /// Convert from an Expression to a RuleConstraint
    ///
    /// The expression must be a boolean expression containing only:
    /// - Comparisons between facts and literals
    /// - Boolean fact references
    /// - Logical operators (and, or, not)
    /// - Boolean literals
    pub fn from_expression(expr: &crate::Expression) -> LemmaResult<RuleConstraint> {
        use crate::ExpressionKind;

        match &expr.kind {
            ExpressionKind::Literal(LiteralValue::Boolean(bool_val)) => {
                if bool_val.into() {
                    Ok(RuleConstraint::True)
                } else {
                    Ok(RuleConstraint::False)
                }
            }

            ExpressionKind::FactPath(fact_path) => Ok(RuleConstraint::Fact(fact_path.clone())),

            ExpressionKind::Comparison(left, op, right) => Self::from_comparison(left, op, right),

            ExpressionKind::LogicalAnd(left, right) => {
                let left_constraint = Self::from_expression(left)?;
                let right_constraint = Self::from_expression(right)?;
                Ok(left_constraint.and(right_constraint))
            }

            ExpressionKind::LogicalOr(left, right) => {
                let left_constraint = Self::from_expression(left)?;
                let right_constraint = Self::from_expression(right)?;
                Ok(left_constraint.or(right_constraint))
            }

            ExpressionKind::LogicalNegation(inner, _) => {
                let inner_constraint = Self::from_expression(inner)?;
                Ok(inner_constraint.not())
            }

            other => Err(LemmaError::Engine(format!(
                "Cannot convert expression kind to constraint: {:?}",
                std::mem::discriminant(other)
            ))),
        }
    }

    /// Convert a comparison expression to a constraint
    fn from_comparison(
        left: &crate::Expression,
        op: &ComparisonComputation,
        right: &crate::Expression,
    ) -> LemmaResult<RuleConstraint> {
        use crate::BooleanValue;
        use crate::ExpressionKind;

        // Case 1: fact op literal (e.g., age > 18)
        if let ExpressionKind::FactPath(fact_path) = &left.kind {
            if let ExpressionKind::Literal(value) = &right.kind {
                return Ok(RuleConstraint::Comparison {
                    fact: fact_path.clone(),
                    op: op.clone(),
                    value: value.clone(),
                });
            }
        }

        // Case 2: literal op fact (e.g., 18 < age) - flip the comparison
        if let ExpressionKind::Literal(value) = &left.kind {
            if let ExpressionKind::FactPath(fact_path) = &right.kind {
                let flipped_op = flip_comparison_operator(op);
                return Ok(RuleConstraint::Comparison {
                    fact: fact_path.clone(),
                    op: flipped_op,
                    value: value.clone(),
                });
            }
        }

        // Case 3: literal op literal (e.g., "bronze" == "silver") - evaluate directly
        if let ExpressionKind::Literal(left_val) = &left.kind {
            if let ExpressionKind::Literal(right_val) = &right.kind {
                if let Some(result) = evaluate_literal_comparison(left_val, op, right_val) {
                    return Ok(if result {
                        RuleConstraint::True
                    } else {
                        RuleConstraint::False
                    });
                }
            }
        }

        // Case 4: comparison == boolean (e.g., (age > 18) == false)
        if op.is_equal() {
            if let ExpressionKind::Comparison(inner_left, inner_op, inner_right) = &left.kind {
                if let ExpressionKind::Literal(LiteralValue::Boolean(bool_val)) = &right.kind {
                    let inner_constraint =
                        Self::from_comparison(inner_left, inner_op, inner_right)?;
                    if bool_val == &BooleanValue::True {
                        return Ok(inner_constraint);
                    } else {
                        return Ok(inner_constraint.not());
                    }
                }
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(bool_val)) = &left.kind {
                if let ExpressionKind::Comparison(inner_left, inner_op, inner_right) = &right.kind {
                    let inner_constraint =
                        Self::from_comparison(inner_left, inner_op, inner_right)?;
                    if bool_val == &BooleanValue::True {
                        return Ok(inner_constraint);
                    } else {
                        return Ok(inner_constraint.not());
                    }
                }
            }
        }

        // Case 5: veto compared with anything
        // A veto is never equal to any literal value
        if matches!(&left.kind, ExpressionKind::Veto(_))
            || matches!(&right.kind, ExpressionKind::Veto(_))
        {
            return Ok(if op.is_not_equal() {
                RuleConstraint::True
            } else {
                RuleConstraint::False
            });
        }

        Err(LemmaError::Engine(format!(
            "Cannot convert comparison to constraint: {} {} {}",
            left, op, right
        )))
    }

    /// Collect all fact paths referenced in this constraint
    pub fn collect_facts(&self) -> Vec<FactPath> {
        let mut facts = Vec::new();
        self.collect_facts_recursive(&mut facts);
        facts.sort_by_key(|a| a.to_string());
        facts.dedup();
        facts
    }

    fn collect_facts_recursive(&self, facts: &mut Vec<FactPath>) {
        match self {
            RuleConstraint::True | RuleConstraint::False => {}
            RuleConstraint::Comparison { fact, .. } => {
                facts.push(fact.clone());
            }
            RuleConstraint::Fact(fact_path) => {
                facts.push(fact_path.clone());
            }
            RuleConstraint::And(left, right) | RuleConstraint::Or(left, right) => {
                left.collect_facts_recursive(facts);
                right.collect_facts_recursive(facts);
            }
            RuleConstraint::Not(inner) => {
                inner.collect_facts_recursive(facts);
            }
        }
    }
}

/// Evaluate a comparison between two literals, returning the boolean result
fn evaluate_literal_comparison(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> Option<bool> {
    match (left, right) {
        // Text equality
        (LiteralValue::Text(l), LiteralValue::Text(r)) => {
            if op.is_equal() {
                Some(l == r)
            } else if op.is_not_equal() {
                Some(l != r)
            } else {
                None
            }
        }
        // Boolean equality
        (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => {
            if op.is_equal() {
                Some(l == r)
            } else if op.is_not_equal() {
                Some(l != r)
            } else {
                None
            }
        }
        // Number comparisons
        (LiteralValue::Number(l), LiteralValue::Number(r)) => match op {
            ComparisonComputation::Equal(_) => Some(l == r),
            ComparisonComputation::NotEqual(_) => Some(l != r),
            ComparisonComputation::LessThan => Some(l < r),
            ComparisonComputation::LessThanOrEqual => Some(l <= r),
            ComparisonComputation::GreaterThan => Some(l > r),
            ComparisonComputation::GreaterThanOrEqual => Some(l >= r),
        },
        // Percentage comparisons
        (LiteralValue::Percentage(l), LiteralValue::Percentage(r)) => match op {
            ComparisonComputation::Equal(_) => Some(l == r),
            ComparisonComputation::NotEqual(_) => Some(l != r),
            ComparisonComputation::LessThan => Some(l < r),
            ComparisonComputation::LessThanOrEqual => Some(l <= r),
            ComparisonComputation::GreaterThan => Some(l > r),
            ComparisonComputation::GreaterThanOrEqual => Some(l >= r),
        },
        _ => None,
    }
}

/// Flip a comparison operator (for converting `literal op fact` to `fact flipped_op literal`)
fn flip_comparison_operator(op: &ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::Equal(n) => ComparisonComputation::Equal(*n),
        ComparisonComputation::NotEqual(n) => ComparisonComputation::NotEqual(*n),
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThan,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThanOrEqual,
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThan,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThanOrEqual,
    }
}

// ============================================================================
// BDD-based simplification
// ============================================================================

/// Convert a constraint to a BDD expression
fn to_bool_expr(
    constraint: &RuleConstraint,
    atoms: &mut Vec<RuleConstraint>,
) -> Option<boolean_expression::Expr<usize>> {
    use boolean_expression::Expr;

    match constraint {
        RuleConstraint::True => Some(Expr::Const(true)),
        RuleConstraint::False => Some(Expr::Const(false)),
        RuleConstraint::And(left, right) => {
            let left_expr = to_bool_expr(left, atoms)?;
            let right_expr = to_bool_expr(right, atoms)?;
            Some(Expr::and(left_expr, right_expr))
        }
        RuleConstraint::Or(left, right) => {
            let left_expr = to_bool_expr(left, atoms)?;
            let right_expr = to_bool_expr(right, atoms)?;
            Some(Expr::or(left_expr, right_expr))
        }
        RuleConstraint::Not(inner) => {
            let inner_expr = to_bool_expr(inner, atoms)?;
            Some(Expr::not(inner_expr))
        }
        RuleConstraint::Comparison { .. } | RuleConstraint::Fact(_) => {
            // Find or add this constraint as an atom
            let mut idx_opt = None;
            for (i, atom) in atoms.iter().enumerate() {
                if constraints_structurally_equal(atom, constraint) {
                    idx_opt = Some(i);
                    break;
                }
            }
            let idx = match idx_opt {
                Some(i) => i,
                None => {
                    atoms.push(constraint.clone());
                    atoms.len() - 1
                }
            };
            Some(Expr::Terminal(idx))
        }
    }
}

/// Check if two constraints are structurally equal (for atom deduplication)
fn constraints_structurally_equal(a: &RuleConstraint, b: &RuleConstraint) -> bool {
    match (a, b) {
        (RuleConstraint::True, RuleConstraint::True) => true,
        (RuleConstraint::False, RuleConstraint::False) => true,
        (
            RuleConstraint::Comparison {
                fact: f1,
                op: o1,
                value: v1,
            },
            RuleConstraint::Comparison {
                fact: f2,
                op: o2,
                value: v2,
            },
        ) => f1 == f2 && o1 == o2 && v1 == v2,
        (RuleConstraint::Fact(f1), RuleConstraint::Fact(f2)) => f1 == f2,
        _ => false,
    }
}

/// Convert a BDD expression back to a constraint
fn from_bool_expr(bool_expr: &boolean_expression::Expr<usize>, atoms: &[RuleConstraint]) -> RuleConstraint {
    use boolean_expression::Expr;

    match bool_expr {
        Expr::Const(true) => RuleConstraint::True,
        Expr::Const(false) => RuleConstraint::False,
        Expr::Terminal(i) => atoms.get(*i).cloned().unwrap_or(RuleConstraint::False),
        Expr::Not(inner) => {
            let inner_constraint = from_bool_expr(inner, atoms);
            inner_constraint.not()
        }
        Expr::And(left, right) => {
            let left_constraint = from_bool_expr(left, atoms);
            let right_constraint = from_bool_expr(right, atoms);
            left_constraint.and(right_constraint)
        }
        Expr::Or(left, right) => {
            let left_constraint = from_bool_expr(left, atoms);
            let right_constraint = from_bool_expr(right, atoms);
            left_constraint.or(right_constraint)
        }
    }
}

// ============================================================================
// Display and Serialize implementations
// ============================================================================

impl fmt::Display for RuleConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleConstraint::True => write!(f, "true"),
            RuleConstraint::False => write!(f, "false"),
            RuleConstraint::Comparison { fact, op, value } => {
                write!(f, "{} {} {}", fact, op, value)
            }
            RuleConstraint::Fact(fact_path) => write!(f, "{}", fact_path),
            RuleConstraint::And(left, right) => {
                let left_str = format_with_parens(left, self);
                let right_str = format_with_parens(right, self);
                write!(f, "{} and {}", left_str, right_str)
            }
            RuleConstraint::Or(left, right) => {
                let left_str = format_with_parens(left, self);
                let right_str = format_with_parens(right, self);
                write!(f, "{} or {}", left_str, right_str)
            }
            RuleConstraint::Not(inner) => match inner.as_ref() {
                RuleConstraint::And(_, _) | RuleConstraint::Or(_, _) => {
                    write!(f, "not ({})", inner)
                }
                _ => write!(f, "not {}", inner),
            },
        }
    }
}

/// Format a constraint with parentheses if needed for precedence
fn format_with_parens(inner: &RuleConstraint, parent: &RuleConstraint) -> String {
    let needs_parens = matches!(
        (parent, inner),
        (RuleConstraint::And(_, _), RuleConstraint::Or(_, _))
    );

    if needs_parens {
        format!("({})", inner)
    } else {
        inner.to_string()
    }
}

impl Serialize for RuleConstraint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            RuleConstraint::True => {
                let mut state = serializer.serialize_struct("RuleConstraint", 1)?;
                state.serialize_field("type", "true")?;
                state.end()
            }
            RuleConstraint::False => {
                let mut state = serializer.serialize_struct("RuleConstraint", 1)?;
                state.serialize_field("type", "false")?;
                state.end()
            }
            RuleConstraint::Comparison { fact, op, value } => {
                let mut state = serializer.serialize_struct("RuleConstraint", 4)?;
                state.serialize_field("type", "comparison")?;
                state.serialize_field("fact", &fact.to_string())?;
                state.serialize_field("op", &op.to_string())?;
                state.serialize_field("value", value)?;
                state.end()
            }
            RuleConstraint::Fact(fact_path) => {
                let mut state = serializer.serialize_struct("RuleConstraint", 2)?;
                state.serialize_field("type", "fact")?;
                state.serialize_field("fact", &fact_path.to_string())?;
                state.end()
            }
            RuleConstraint::And(left, right) => {
                let mut state = serializer.serialize_struct("RuleConstraint", 3)?;
                state.serialize_field("type", "and")?;
                state.serialize_field("left", left)?;
                state.serialize_field("right", right)?;
                state.end()
            }
            RuleConstraint::Or(left, right) => {
                let mut state = serializer.serialize_struct("RuleConstraint", 3)?;
                state.serialize_field("type", "or")?;
                state.serialize_field("left", left)?;
                state.serialize_field("right", right)?;
                state.end()
            }
            RuleConstraint::Not(inner) => {
                let mut state = serializer.serialize_struct("RuleConstraint", 2)?;
                state.serialize_field("type", "not")?;
                state.serialize_field("inner", inner)?;
                state.end()
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EqualityNotation;
    use rust_decimal::Decimal;

    fn num(n: i64) -> LiteralValue {
        LiteralValue::Number(Decimal::from(n))
    }

    fn fact(name: &str) -> FactPath {
        FactPath::local(name.to_string())
    }

    fn comparison(fact_name: &str, op: ComparisonComputation, val: i64) -> RuleConstraint {
        RuleConstraint::Comparison {
            fact: fact(fact_name),
            op,
            value: num(val),
        }
    }

    // Basic constraint tests

    #[test]
    fn test_constraint_and_short_circuit() {
        let c1 = RuleConstraint::True;
        let c2 = RuleConstraint::Fact(fact("x"));
        assert!(matches!(c1.and(c2.clone()), RuleConstraint::Fact(_)));

        let c3 = RuleConstraint::False;
        assert!(matches!(c3.and(c2), RuleConstraint::False));
    }

    #[test]
    fn test_constraint_or_short_circuit() {
        let c1 = RuleConstraint::False;
        let c2 = RuleConstraint::Fact(fact("x"));
        assert!(matches!(c1.or(c2.clone()), RuleConstraint::Fact(_)));

        let c3 = RuleConstraint::True;
        assert!(matches!(c3.or(c2), RuleConstraint::True));
    }

    #[test]
    fn test_constraint_not_double_negation() {
        let c = RuleConstraint::Fact(fact("x"));
        let not_c = c.clone().not();
        let not_not_c = not_c.not();
        assert_eq!(c, not_not_c);
    }

    #[test]
    fn test_constraint_display_simple() {
        let c = RuleConstraint::Comparison {
            fact: fact("age"),
            op: ComparisonComputation::GreaterThan,
            value: num(18),
        };
        assert_eq!(c.to_string(), "age > 18");
    }

    #[test]
    fn test_constraint_display_and() {
        let c1 = RuleConstraint::Comparison {
            fact: fact("age"),
            op: ComparisonComputation::GreaterThan,
            value: num(18),
        };
        let c2 = RuleConstraint::Fact(fact("is_employee"));
        let combined = RuleConstraint::And(Box::new(c1), Box::new(c2.not()));
        assert_eq!(combined.to_string(), "age > 18 and not is_employee");
    }

    #[test]
    fn test_collect_facts() {
        let c = RuleConstraint::And(
            Box::new(RuleConstraint::Comparison {
                fact: fact("age"),
                op: ComparisonComputation::GreaterThan,
                value: num(18),
            }),
            Box::new(RuleConstraint::Fact(fact("is_employee"))),
        );
        let facts = c.collect_facts();
        assert_eq!(facts.len(), 2);
    }

    // Simplification tests

    #[test]
    fn test_simplify_tautology() {
        // (A and B) or (A and not B) = A
        // This should simplify to just A (x > 10)
        let a = comparison("x", ComparisonComputation::GreaterThan, 10);
        let b = RuleConstraint::Fact(fact("flag"));

        let expr = a.clone().and(b.clone()).or(a.clone().and(b.not()));
        let simplified = expr.simplify().unwrap();

        // Verify the simplified constraint is exactly "x > 10" (just A, without B)
        // Check structure directly, not just string representation
        let expected_fact = fact("x");
        match simplified {
            RuleConstraint::Comparison { fact: fact_path, op, value } => {
                assert_eq!(fact_path, expected_fact);
                assert_eq!(op, ComparisonComputation::GreaterThan);
                assert_eq!(value, num(10));
            }
            other => panic!(
                "Simplified constraint should be 'x > 10' (Comparison), got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_simplify_contradiction() {
        // x == 1 and x == 2 cannot both be true
        // BDD simplification treats these as different atoms, so it doesn't
        // automatically detect semantic contradictions. The constraint remains as-is.
        // Contradiction detection for semantic equality is handled by domain extraction.
        let c1 = comparison(
            "x",
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            1,
        );
        let c2 = comparison(
            "x",
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            2,
        );

        let expr = c1.and(c2);
        let simplified = expr.simplify().unwrap();

        // BDD cannot detect this semantic contradiction (different atoms)
        assert!(!simplified.is_false(), "BDD simplification should not detect semantic contradictions");

        // FactConstraint extraction SHOULD detect the contradiction
        // x == 1 gives domain {1}, x == 2 gives domain {2}
        // Intersection of {1} and {2} should be Empty domain
        use crate::inversion::domain::extract_fact_constraints_from_rule_constraint;
        let domains = extract_fact_constraints_from_rule_constraint(&simplified).expect("Should extract domains");
        let x_domain = domains.get(&fact("x")).expect("Should have domain for x");
        
        // The domain should be Empty (unsatisfiable) because x cannot be both 1 and 2
        assert!(
            x_domain.is_empty(),
            "FactConstraint extraction should detect contradiction: x cannot be both 1 and 2. Got constraint: {:?}",
            x_domain
        );
    }
}
