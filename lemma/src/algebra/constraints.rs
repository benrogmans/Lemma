//! Constraint types and operations for fact domains
//!
//! Provides types for representing and manipulating constraints on fact values:
//! - `Bound`, `FactConstraint` - constraint specifications
//! - `FactBounds`, `ConstraintSet` - accumulating constraints during solving
//! - `DomainRestriction`, `UnsatReason` - solving results
//!
//! Used by planning (compile-time validation) and inversion (query-time solving).

use crate::computation::{comparison_operation, OperationResult};
use crate::algebra::expansion::reverse_comparison;
use crate::algebra::isolation::{try_isolate_comparison, IsolationResult};
use crate::semantic::{
    BooleanValue, ComparisonComputation, EqualityNotation, Expression, ExpressionKind, FactPath,
    LiteralValue,
};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

// ============================================================================
// Constraint Types
// ============================================================================

/// Constraint on a fact's valid values
#[derive(Debug, Clone, PartialEq)]
pub enum FactConstraint {
    /// A single continuous range
    Range { min: Bound, max: Bound },

    /// Multiple disjoint ranges (union)
    Union(Vec<FactConstraint>),

    /// Specific enumerated values only
    Enumeration(Vec<LiteralValue>),

    /// Everything except these constraints
    Complement(Box<FactConstraint>),

    /// Any value (no constraints)
    Unconstrained,
}

/// Bound specification for ranges
#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    /// Inclusive bound [value
    Inclusive(LiteralValue),

    /// Exclusive bound (value
    Exclusive(LiteralValue),

    /// Unbounded (-infinity or +infinity)
    Unbounded,
}

/// Restriction on a fact's domain where an expression is undefined
#[derive(Debug, Clone)]
pub struct DomainRestriction {
    /// The fact(s) involved in the restriction
    pub facts: Vec<FactPath>,

    /// Human-readable description of the restriction
    pub description: String,

    /// Source of the restriction (e.g., "tan undefined", "division by zero")
    pub source: String,
}

/// Reason why an equation is unsatisfiable
#[derive(Debug, Clone)]
pub enum UnsatReason {
    /// Bounds contradiction: min > max for a fact
    BoundsContradiction {
        fact: FactPath,
        min: Bound,
        max: Bound,
    },

    /// Value outside function's codomain (e.g., sin(x) > 2)
    FunctionRangeViolation {
        function: String,
        comparison_op: String,
        required_value: LiteralValue,
        valid_range_min: Option<LiteralValue>,
        valid_range_max: Option<LiteralValue>,
    },

    /// Conflicting exact values (e.g., x == "a" AND x == "b")
    EnumContradiction {
        fact: FactPath,
        value_a: LiteralValue,
        value_b: LiteralValue,
    },

    /// Exact value in excluded set (e.g., x == 5 AND x != 5)
    ExclusionContradiction { fact: FactPath, value: LiteralValue },

    /// Expression simplified to false
    SimplifiedToFalse,

    /// Arithmetic impossibility (e.g., x * 0 == 5)
    ArithmeticContradiction { message: String },
}

// ============================================================================
// Accumulated Bounds
// ============================================================================

/// Accumulated bounds for a single fact during solving
#[derive(Debug, Clone, Default)]
pub struct FactBounds {
    /// Lower bound: (value, is_inclusive)
    pub min: Option<(LiteralValue, bool)>,

    /// Upper bound: (value, is_inclusive)
    pub max: Option<(LiteralValue, bool)>,

    /// Exact value from equality constraint
    pub exact: Option<LiteralValue>,

    /// Excluded values from != constraints
    pub excluded: Vec<LiteralValue>,
}

impl FactBounds {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert to FactConstraint
    pub fn to_constraint(&self) -> FactConstraint {
        if let Some(exact_value) = &self.exact {
            return FactConstraint::Enumeration(vec![exact_value.clone()]);
        }

        let min_bound = match &self.min {
            Some((value, true)) => Bound::Inclusive(value.clone()),
            Some((value, false)) => Bound::Exclusive(value.clone()),
            None => Bound::Unbounded,
        };

        let max_bound = match &self.max {
            Some((value, true)) => Bound::Inclusive(value.clone()),
            Some((value, false)) => Bound::Exclusive(value.clone()),
            None => Bound::Unbounded,
        };

        let base_constraint = FactConstraint::Range {
            min: min_bound,
            max: max_bound,
        };

        if self.excluded.is_empty() {
            base_constraint
        } else {
            let excluded_constraint =
                FactConstraint::Complement(Box::new(FactConstraint::Enumeration(
                    self.excluded.clone(),
                )));
            base_constraint.intersect(&excluded_constraint)
        }
    }
}

// ============================================================================
// Constraint Set
// ============================================================================

/// Set of constraints accumulated during solving
#[derive(Debug, Clone)]
pub struct ConstraintSet {
    /// Bounds accumulated per fact
    pub facts: HashMap<FactPath, FactBounds>,

    /// Relational constraints between facts (for transitivity)
    pub relations: Vec<(FactPath, ComparisonComputation, FactPath)>,

    /// Constraints that couldn't be reduced to single-fact bounds
    pub symbolic: Vec<Expression>,

    /// Domain restrictions from function domains
    pub restrictions: Vec<DomainRestriction>,

    /// Has a contradiction been detected?
    pub contradiction: Option<UnsatReason>,
}

impl ConstraintSet {
    pub fn new() -> Self {
        Self {
            facts: HashMap::new(),
            relations: Vec::new(),
            symbolic: Vec::new(),
            restrictions: Vec::new(),
            contradiction: None,
        }
    }

    /// Get or create FactBounds for a fact
    pub fn get_or_create_bounds(&mut self, fact: &FactPath) -> &mut FactBounds {
        self.facts.entry(fact.clone()).or_insert_with(FactBounds::new)
    }

    /// Add a comparison constraint for a single fact
    pub fn add_comparison(
        &mut self,
        fact: FactPath,
        op: &ComparisonComputation,
        value: LiteralValue,
    ) {
        if self.contradiction.is_some() {
            return;
        }

        let bounds = self.get_or_create_bounds(&fact);

        match op {
            ComparisonComputation::Equal(_) => {
                if let Some(existing_exact) = &bounds.exact {
                    if existing_exact != &value {
                        self.contradiction = Some(UnsatReason::EnumContradiction {
                            fact,
                            value_a: existing_exact.clone(),
                            value_b: value,
                        });
                        return;
                    }
                }
                if bounds.excluded.contains(&value) {
                    self.contradiction = Some(UnsatReason::ExclusionContradiction {
                        fact,
                        value: value.clone(),
                    });
                    return;
                }
                bounds.exact = Some(value);
            }

            ComparisonComputation::NotEqual(_) => {
                if let Some(existing_exact) = &bounds.exact {
                    if existing_exact == &value {
                        self.contradiction = Some(UnsatReason::ExclusionContradiction {
                            fact,
                            value: value.clone(),
                        });
                        return;
                    }
                }
                if !bounds.excluded.contains(&value) {
                    bounds.excluded.push(value);
                }
            }

            ComparisonComputation::LessThan => {
                self.update_max_bound(&fact, value, false);
            }

            ComparisonComputation::LessThanOrEqual => {
                self.update_max_bound(&fact, value, true);
            }

            ComparisonComputation::GreaterThan => {
                self.update_min_bound(&fact, value, false);
            }

            ComparisonComputation::GreaterThanOrEqual => {
                self.update_min_bound(&fact, value, true);
            }
        }
    }

    fn update_min_bound(&mut self, fact: &FactPath, value: LiteralValue, inclusive: bool) {
        let bounds = self.get_or_create_bounds(fact);

        let should_update = match &bounds.min {
            None => true,
            Some((existing_value, existing_inclusive)) => {
                let cmp = lit_cmp(&value, existing_value);
                cmp > 0 || (cmp == 0 && !inclusive && *existing_inclusive)
            }
        };

        if should_update {
            bounds.min = Some((value.clone(), inclusive));
        }

        self.check_bounds_contradiction(fact);
    }

    fn update_max_bound(&mut self, fact: &FactPath, value: LiteralValue, inclusive: bool) {
        let bounds = self.get_or_create_bounds(fact);

        let should_update = match &bounds.max {
            None => true,
            Some((existing_value, existing_inclusive)) => {
                let cmp = lit_cmp(&value, existing_value);
                cmp < 0 || (cmp == 0 && !inclusive && *existing_inclusive)
            }
        };

        if should_update {
            bounds.max = Some((value.clone(), inclusive));
        }

        self.check_bounds_contradiction(fact);
    }

    fn check_bounds_contradiction(&mut self, fact: &FactPath) {
        if self.contradiction.is_some() {
            return;
        }

        let bounds = match self.facts.get(fact) {
            Some(b) => b,
            None => return,
        };

        if let (Some((min_val, min_inc)), Some((max_val, max_inc))) = (&bounds.min, &bounds.max) {
            let cmp = lit_cmp(min_val, max_val);

            let is_contradiction = match (min_inc, max_inc) {
                (true, true) => cmp > 0,
                _ => cmp >= 0,
            };

            if is_contradiction {
                let min_bound = if *min_inc {
                    Bound::Inclusive(min_val.clone())
                } else {
                    Bound::Exclusive(min_val.clone())
                };
                let max_bound = if *max_inc {
                    Bound::Inclusive(max_val.clone())
                } else {
                    Bound::Exclusive(max_val.clone())
                };

                self.contradiction = Some(UnsatReason::BoundsContradiction {
                    fact: fact.clone(),
                    min: min_bound,
                    max: max_bound,
                });
            }
        }
    }

    /// Add a relational constraint between two facts
    pub fn add_relation(&mut self, left: FactPath, op: ComparisonComputation, right: FactPath) {
        self.relations.push((left, op, right));
    }

    /// Add a symbolic constraint that couldn't be reduced
    pub fn add_symbolic(&mut self, expression: Expression) {
        self.symbolic.push(expression);
    }

    /// Add a domain restriction
    pub fn add_restriction(&mut self, restriction: DomainRestriction) {
        self.restrictions.push(restriction);
    }

    /// Convert accumulated constraints to fact constraints
    pub fn to_fact_constraints(&self) -> HashMap<FactPath, FactConstraint> {
        let mut result = HashMap::new();

        for (fact_path, bounds) in &self.facts {
            let constraint = bounds.to_constraint();
            if !matches!(
                constraint,
                FactConstraint::Range {
                    min: Bound::Unbounded,
                    max: Bound::Unbounded
                }
            ) {
                result.insert(fact_path.clone(), constraint);
            }
        }

        result
    }
}

impl Default for ConstraintSet {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// FactConstraint Implementation
// ============================================================================

impl FactConstraint {
    /// Check if this constraint is satisfiable (has at least one valid value)
    pub fn is_satisfiable(&self) -> bool {
        match self {
            FactConstraint::Enumeration(values) => !values.is_empty(),
            FactConstraint::Union(parts) => parts.iter().any(|p| p.is_satisfiable()),
            FactConstraint::Range { min, max } => !bounds_contradict(min, max),
            FactConstraint::Complement(inner) => {
                !matches!(inner.as_ref(), FactConstraint::Unconstrained)
            }
            FactConstraint::Unconstrained => true,
        }
    }

    /// Intersect this constraint with another
    pub fn intersect(&self, other: &FactConstraint) -> FactConstraint {
        intersect_constraints(self.clone(), other.clone())
    }

    /// Check if constraint represents a single exact value
    pub fn is_exact(&self) -> bool {
        matches!(self, FactConstraint::Enumeration(vals) if vals.len() == 1)
    }

    /// Create constraint for exact value
    pub fn exact(value: LiteralValue) -> Self {
        FactConstraint::Enumeration(vec![value])
    }

    /// Check if a value satisfies this constraint
    pub fn contains(&self, value: &LiteralValue) -> bool {
        match self {
            FactConstraint::Unconstrained => true,
            FactConstraint::Enumeration(vals) => vals.contains(value),
            FactConstraint::Range { min, max } => {
                value_in_bounds(value, min) && value_in_bounds(value, max)
            }
            FactConstraint::Union(parts) => parts.iter().any(|p| p.contains(value)),
            FactConstraint::Complement(inner) => !inner.contains(value),
        }
    }
}

/// Check if a value satisfies a bound constraint
fn value_in_bounds(value: &LiteralValue, bound: &Bound) -> bool {
    match bound {
        Bound::Unbounded => true,
        Bound::Inclusive(b) => lit_cmp(value, b) <= 0,
        Bound::Exclusive(b) => lit_cmp(value, b) < 0,
    }
}

// ============================================================================
// Display Implementations
// ============================================================================

impl fmt::Display for FactConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FactConstraint::Unconstrained => write!(f, "any"),
            FactConstraint::Enumeration(vals) => {
                write!(f, "{{")?;
                for (i, v) in vals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            FactConstraint::Range { min, max } => {
                let (l_bracket, r_bracket) = match (min, max) {
                    (Bound::Inclusive(_), Bound::Inclusive(_)) => ('[', ']'),
                    (Bound::Inclusive(_), Bound::Exclusive(_)) => ('[', ')'),
                    (Bound::Exclusive(_), Bound::Inclusive(_)) => ('(', ']'),
                    (Bound::Exclusive(_), Bound::Exclusive(_)) => ('(', ')'),
                    (Bound::Unbounded, Bound::Inclusive(_)) => ('(', ']'),
                    (Bound::Unbounded, Bound::Exclusive(_)) => ('(', ')'),
                    (Bound::Inclusive(_), Bound::Unbounded) => ('[', ')'),
                    (Bound::Exclusive(_), Bound::Unbounded) => ('(', ')'),
                    (Bound::Unbounded, Bound::Unbounded) => ('(', ')'),
                };

                let min_str = match min {
                    Bound::Unbounded => "-inf".to_string(),
                    Bound::Inclusive(v) | Bound::Exclusive(v) => v.to_string(),
                };
                let max_str = match max {
                    Bound::Unbounded => "+inf".to_string(),
                    Bound::Inclusive(v) | Bound::Exclusive(v) => v.to_string(),
                };
                write!(f, "{}{}, {}{}", l_bracket, min_str, max_str, r_bracket)
            }
            FactConstraint::Union(parts) => {
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{}", p)?;
                }
                Ok(())
            }
            FactConstraint::Complement(inner) => write!(f, "not ({})", inner),
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bound::Unbounded => write!(f, "inf"),
            Bound::Inclusive(v) => write!(f, "[{}", v),
            Bound::Exclusive(v) => write!(f, "({}", v),
        }
    }
}

// ============================================================================
// Serialization
// ============================================================================

impl Serialize for FactConstraint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            FactConstraint::Unconstrained => {
                let mut st = serializer.serialize_struct("FactConstraint", 1)?;
                st.serialize_field("type", "unconstrained")?;
                st.end()
            }
            FactConstraint::Enumeration(vals) => {
                let mut st = serializer.serialize_struct("FactConstraint", 2)?;
                st.serialize_field("type", "enumeration")?;
                st.serialize_field("values", vals)?;
                st.end()
            }
            FactConstraint::Range { min, max } => {
                let mut st = serializer.serialize_struct("FactConstraint", 3)?;
                st.serialize_field("type", "range")?;
                st.serialize_field("min", min)?;
                st.serialize_field("max", max)?;
                st.end()
            }
            FactConstraint::Union(parts) => {
                let mut st = serializer.serialize_struct("FactConstraint", 2)?;
                st.serialize_field("type", "union")?;
                st.serialize_field("parts", parts)?;
                st.end()
            }
            FactConstraint::Complement(inner) => {
                let mut st = serializer.serialize_struct("FactConstraint", 2)?;
                st.serialize_field("type", "complement")?;
                st.serialize_field("inner", inner)?;
                st.end()
            }
        }
    }
}

impl Serialize for Bound {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Bound::Unbounded => {
                let mut st = serializer.serialize_struct("Bound", 1)?;
                st.serialize_field("type", "unbounded")?;
                st.end()
            }
            Bound::Inclusive(v) => {
                let mut st = serializer.serialize_struct("Bound", 2)?;
                st.serialize_field("type", "inclusive")?;
                st.serialize_field("value", v)?;
                st.end()
            }
            Bound::Exclusive(v) => {
                let mut st = serializer.serialize_struct("Bound", 2)?;
                st.serialize_field("type", "exclusive")?;
                st.serialize_field("value", v)?;
                st.end()
            }
        }
    }
}

// ============================================================================
// Constraint Operations
// ============================================================================

/// Intersect two constraints (for AND)
pub fn intersect_constraints(a: FactConstraint, b: FactConstraint) -> FactConstraint {
    let a = normalize_constraint(a);
    let b = normalize_constraint(b);

    match (a, b) {
        (FactConstraint::Unconstrained, other) | (other, FactConstraint::Unconstrained) => other,

        (
            FactConstraint::Range {
                min: min1,
                max: max1,
            },
            FactConstraint::Range {
                min: min2,
                max: max2,
            },
        ) => {
            let min = compute_intersection_min(min1, min2);
            let max = compute_intersection_max(max1, max2);

            if bounds_contradict(&min, &max) {
                FactConstraint::Enumeration(vec![])
            } else {
                FactConstraint::Range { min, max }
            }
        }

        (FactConstraint::Enumeration(mut v1), FactConstraint::Enumeration(v2)) => {
            v1.retain(|x| v2.contains(x));
            FactConstraint::Enumeration(v1)
        }

        (FactConstraint::Enumeration(vs), FactConstraint::Range { min, max })
        | (FactConstraint::Range { min, max }, FactConstraint::Enumeration(vs)) => {
            let kept: Vec<LiteralValue> = vs
                .into_iter()
                .filter(|v| value_within(v, &min, &max))
                .collect();
            FactConstraint::Enumeration(kept)
        }

        (FactConstraint::Enumeration(vs), FactConstraint::Complement(inner))
        | (FactConstraint::Complement(inner), FactConstraint::Enumeration(vs)) => {
            if let FactConstraint::Enumeration(excluded) = *inner {
                let kept: Vec<LiteralValue> =
                    vs.into_iter().filter(|v| !excluded.contains(v)).collect();
                FactConstraint::Enumeration(kept)
            } else {
                FactConstraint::Union(vec![])
            }
        }

        (FactConstraint::Union(v1), FactConstraint::Union(v2)) => {
            let mut acc: Vec<FactConstraint> = Vec::new();
            for a_part in v1.into_iter() {
                for b_part in v2.iter() {
                    let intersection = intersect_constraints(a_part.clone(), b_part.clone());
                    if intersection.is_satisfiable() {
                        acc.push(intersection);
                    }
                }
            }
            if acc.is_empty() {
                FactConstraint::Enumeration(vec![])
            } else if acc.len() == 1 {
                acc.remove(0)
            } else {
                FactConstraint::Union(acc)
            }
        }

        (FactConstraint::Union(vs), other) | (other, FactConstraint::Union(vs)) => {
            let mut acc: Vec<FactConstraint> = Vec::new();
            for v in vs {
                let intersection = intersect_constraints(v, other.clone());
                if intersection.is_satisfiable() {
                    acc.push(intersection);
                }
            }
            if acc.is_empty() {
                FactConstraint::Enumeration(vec![])
            } else if acc.len() == 1 {
                acc.remove(0)
            } else {
                FactConstraint::Union(acc)
            }
        }

        (FactConstraint::Complement(inner), other) | (other, FactConstraint::Complement(inner)) => {
            let normalized_inner = normalize_constraint(*inner);
            intersect_constraints(other, normalized_inner)
        }
    }
}

/// Complement a constraint (for NOT)
pub fn complement_constraint(constraint: FactConstraint) -> FactConstraint {
    match constraint {
        FactConstraint::Unconstrained => FactConstraint::Enumeration(vec![]),
        FactConstraint::Enumeration(vals) if vals.is_empty() => FactConstraint::Unconstrained,
        FactConstraint::Complement(inner) => *inner,
        other => FactConstraint::Complement(Box::new(other)),
    }
}

fn normalize_constraint(constraint: FactConstraint) -> FactConstraint {
    match constraint {
        FactConstraint::Complement(inner) => {
            let normalized_inner = normalize_constraint(*inner);
            match normalized_inner {
                FactConstraint::Complement(double_inner) => *double_inner,
                FactConstraint::Range { min, max } => match (&min, &max) {
                    (Bound::Unbounded, Bound::Unbounded) => FactConstraint::Enumeration(vec![]),
                    (Bound::Unbounded, bound_max) => FactConstraint::Range {
                        min: invert_bound(bound_max.clone()),
                        max: Bound::Unbounded,
                    },
                    (bound_min, Bound::Unbounded) => FactConstraint::Range {
                        min: Bound::Unbounded,
                        max: invert_bound(bound_min.clone()),
                    },
                    (bound_min, bound_max) => FactConstraint::Union(vec![
                        FactConstraint::Range {
                            min: Bound::Unbounded,
                            max: invert_bound(bound_min.clone()),
                        },
                        FactConstraint::Range {
                            min: invert_bound(bound_max.clone()),
                            max: Bound::Unbounded,
                        },
                    ]),
                },
                FactConstraint::Enumeration(vals) => {
                    if vals.len() == 1 {
                        if let Some(LiteralValue::Boolean(BooleanValue::True)) = vals.first() {
                            return FactConstraint::Enumeration(vec![LiteralValue::Boolean(
                                BooleanValue::False,
                            )]);
                        }
                        if let Some(LiteralValue::Boolean(BooleanValue::False)) = vals.first() {
                            return FactConstraint::Enumeration(vec![LiteralValue::Boolean(
                                BooleanValue::True,
                            )]);
                        }
                    }
                    FactConstraint::Complement(Box::new(FactConstraint::Enumeration(vals)))
                }
                FactConstraint::Unconstrained => FactConstraint::Enumeration(vec![]),
                other => FactConstraint::Complement(Box::new(other)),
            }
        }

        FactConstraint::Union(mut parts) => {
            let mut flat: Vec<FactConstraint> = Vec::new();
            for p in parts.drain(..) {
                let normalized = normalize_constraint(p);
                match normalized {
                    FactConstraint::Union(inner) => flat.extend(inner),
                    FactConstraint::Unconstrained => return FactConstraint::Unconstrained,
                    FactConstraint::Enumeration(vals) if vals.is_empty() => {}
                    other => flat.push(other),
                }
            }

            if flat.is_empty() {
                FactConstraint::Enumeration(vec![])
            } else if flat.len() == 1 {
                flat.remove(0)
            } else {
                FactConstraint::Union(flat)
            }
        }

        FactConstraint::Enumeration(mut values) => {
            values.sort_by(|a, b| match lit_cmp(a, b) {
                -1 => Ordering::Less,
                0 => Ordering::Equal,
                _ => Ordering::Greater,
            });
            values.dedup();
            FactConstraint::Enumeration(values)
        }

        other => other,
    }
}

fn invert_bound(bound: Bound) -> Bound {
    match bound {
        Bound::Unbounded => Bound::Unbounded,
        Bound::Inclusive(v) => Bound::Exclusive(v),
        Bound::Exclusive(v) => Bound::Inclusive(v),
    }
}

/// Compare two literal values (-1 if a < b, 0 if equal, 1 if a > b)
pub fn lit_cmp(a: &LiteralValue, b: &LiteralValue) -> i8 {
    if let OperationResult::Value(LiteralValue::Boolean(BooleanValue::True)) =
        comparison_operation(a, &ComparisonComputation::LessThan, b)
    {
        return -1;
    }
    if let OperationResult::Value(LiteralValue::Boolean(BooleanValue::True)) = comparison_operation(
        a,
        &ComparisonComputation::Equal(EqualityNotation::Symbol),
        b,
    ) {
        return 0;
    }
    1
}

fn value_within(v: &LiteralValue, min: &Bound, max: &Bound) -> bool {
    let ge_min = match min {
        Bound::Unbounded => true,
        Bound::Inclusive(m) => lit_cmp(v, m) >= 0,
        Bound::Exclusive(m) => lit_cmp(v, m) > 0,
    };
    let le_max = match max {
        Bound::Unbounded => true,
        Bound::Inclusive(m) => lit_cmp(v, m) <= 0,
        Bound::Exclusive(m) => lit_cmp(v, m) < 0,
    };
    ge_min && le_max
}

fn bounds_contradict(min: &Bound, max: &Bound) -> bool {
    match (min, max) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => false,
        (Bound::Inclusive(a), Bound::Inclusive(b)) => lit_cmp(a, b) > 0,
        (Bound::Inclusive(a), Bound::Exclusive(b)) => lit_cmp(a, b) >= 0,
        (Bound::Exclusive(a), Bound::Inclusive(b)) => lit_cmp(a, b) >= 0,
        (Bound::Exclusive(a), Bound::Exclusive(b)) => lit_cmp(a, b) >= 0,
    }
}

fn compute_intersection_min(min1: Bound, min2: Bound) -> Bound {
    match (min1, min2) {
        (Bound::Unbounded, x) | (x, Bound::Unbounded) => x,
        (Bound::Inclusive(v1), Bound::Inclusive(v2)) => {
            if lit_cmp(&v1, &v2) >= 0 {
                Bound::Inclusive(v1)
            } else {
                Bound::Inclusive(v2)
            }
        }
        (Bound::Inclusive(v1), Bound::Exclusive(v2)) => {
            if lit_cmp(&v1, &v2) > 0 {
                Bound::Inclusive(v1)
            } else {
                Bound::Exclusive(v2)
            }
        }
        (Bound::Exclusive(v1), Bound::Inclusive(v2)) => {
            if lit_cmp(&v1, &v2) > 0 {
                Bound::Exclusive(v1)
            } else {
                Bound::Inclusive(v2)
            }
        }
        (Bound::Exclusive(v1), Bound::Exclusive(v2)) => {
            if lit_cmp(&v1, &v2) >= 0 {
                Bound::Exclusive(v1)
            } else {
                Bound::Exclusive(v2)
            }
        }
    }
}

fn compute_intersection_max(max1: Bound, max2: Bound) -> Bound {
    match (max1, max2) {
        (Bound::Unbounded, x) | (x, Bound::Unbounded) => x,
        (Bound::Inclusive(v1), Bound::Inclusive(v2)) => {
            if lit_cmp(&v1, &v2) <= 0 {
                Bound::Inclusive(v1)
            } else {
                Bound::Inclusive(v2)
            }
        }
        (Bound::Inclusive(v1), Bound::Exclusive(v2)) => {
            if lit_cmp(&v1, &v2) < 0 {
                Bound::Inclusive(v1)
            } else {
                Bound::Exclusive(v2)
            }
        }
        (Bound::Exclusive(v1), Bound::Inclusive(v2)) => {
            if lit_cmp(&v1, &v2) < 0 {
                Bound::Exclusive(v1)
            } else {
                Bound::Inclusive(v2)
            }
        }
        (Bound::Exclusive(v1), Bound::Exclusive(v2)) => {
            if lit_cmp(&v1, &v2) <= 0 {
                Bound::Exclusive(v1)
            } else {
                Bound::Exclusive(v2)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn num(n: i64) -> LiteralValue {
        LiteralValue::Number(Decimal::from(n))
    }

    #[test]
    fn test_constraint_display() {
        let range = FactConstraint::Range {
            min: Bound::Inclusive(num(10)),
            max: Bound::Exclusive(num(20)),
        };
        assert_eq!(format!("{}", range), "[10, 20)");

        let enumeration = FactConstraint::Enumeration(vec![num(1), num(2), num(3)]);
        assert_eq!(format!("{}", enumeration), "{1, 2, 3}");
    }

    #[test]
    fn test_range_intersection() {
        let r1 = FactConstraint::Range {
            min: Bound::Inclusive(num(0)),
            max: Bound::Inclusive(num(100)),
        };
        let r2 = FactConstraint::Range {
            min: Bound::Inclusive(num(50)),
            max: Bound::Inclusive(num(150)),
        };
        let intersection = intersect_constraints(r1, r2);
        assert_eq!(
            intersection,
            FactConstraint::Range {
                min: Bound::Inclusive(num(50)),
                max: Bound::Inclusive(num(100)),
            }
        );
    }

    #[test]
    fn test_enumeration_intersection() {
        let e1 = FactConstraint::Enumeration(vec![num(1), num(2), num(3)]);
        let e2 = FactConstraint::Enumeration(vec![num(2), num(3), num(4)]);
        let intersection = intersect_constraints(e1, e2);
        assert_eq!(
            intersection,
            FactConstraint::Enumeration(vec![num(2), num(3)])
        );
    }

    #[test]
    fn test_complement_constraint() {
        let c = FactConstraint::Enumeration(vec![num(5)]);
        let complemented = complement_constraint(c.clone());
        assert_eq!(complemented, FactConstraint::Complement(Box::new(c)));

        let double_comp = complement_constraint(complemented);
        assert_eq!(double_comp, FactConstraint::Enumeration(vec![num(5)]));
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
}

/// Extract constraints from an expression into a ConstraintSet
///
/// Converts an optimized condition (already in DNF) into fact constraints.
pub fn extract_constraints(expression: &Expression, constraint_set: &mut ConstraintSet) {
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
            // NOT(comparison) → opposite comparison
            if let ExpressionKind::Comparison(left, op, right) = &inner.kind {
                // Convert to opposite comparison
                let opposite_op = match op {
                    ComparisonComputation::Equal(notation) => {
                        ComparisonComputation::NotEqual(notation.clone())
                    }
                    ComparisonComputation::NotEqual(notation) => {
                        ComparisonComputation::Equal(notation.clone())
                    }
                    ComparisonComputation::LessThan => ComparisonComputation::GreaterThanOrEqual,
                    ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThan,
                    ComparisonComputation::GreaterThan => ComparisonComputation::LessThanOrEqual,
                    ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThan,
                };
                let opposite_comparison = Expression::new(
                    ExpressionKind::Comparison(left.clone(), opposite_op, right.clone()),
                    None,
                );
                extract_constraints(&opposite_comparison, constraint_set);
                return;
            }

            // NOT(fact) means fact == false
            if let ExpressionKind::FactPath(fact_path) = &inner.kind {
                constraint_set.add_comparison(
                    fact_path.clone(),
                    &ComparisonComputation::Equal(EqualityNotation::Symbol),
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
                &ComparisonComputation::Equal(EqualityNotation::Symbol),
                LiteralValue::Boolean(BooleanValue::True),
            );
        }

        // Other expression types - add as symbolic
        _ => {
            constraint_set.add_symbolic(expression.clone());
        }
    }
}

