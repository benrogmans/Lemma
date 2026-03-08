//! Domain types and operations for inversion
//!
//! Provides:
//! - `Domain` and `Bound` types for representing concrete value constraints
//! - Domain operations: intersection, union, normalization
//! - `extract_domains_from_constraint()`: extracts domains from constraints

use crate::planning::semantics::{
    ComparisonComputation, FactPath, LiteralValue, SemanticConversionTarget, ValueKind,
};
use crate::OperationResult;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::constraint::Constraint;

/// Domain specification for valid values
#[derive(Debug, Clone, PartialEq)]
pub enum Domain {
    /// A single continuous range
    Range { min: Bound, max: Bound },

    /// Multiple disjoint ranges
    Union(Arc<Vec<Domain>>),

    /// Specific enumerated values only
    Enumeration(Arc<Vec<LiteralValue>>),

    /// Everything except these constraints
    Complement(Box<Domain>),

    /// Any value (no constraints)
    Unconstrained,

    /// Empty domain (no valid values) - represents unsatisfiable constraints
    Empty,
}

impl Domain {
    /// Check if this domain is satisfiable (has at least one valid value)
    ///
    /// Returns false for Empty domains and empty Enumerations.
    pub fn is_satisfiable(&self) -> bool {
        match self {
            Domain::Empty => false,
            Domain::Enumeration(values) => !values.is_empty(),
            Domain::Union(parts) => parts.iter().any(|p| p.is_satisfiable()),
            Domain::Range { min, max } => !bounds_contradict(min, max),
            Domain::Complement(inner) => !matches!(inner.as_ref(), Domain::Unconstrained),
            Domain::Unconstrained => true,
        }
    }

    /// Check if this domain is empty (unsatisfiable)
    pub fn is_empty(&self) -> bool {
        !self.is_satisfiable()
    }

    /// Intersect this domain with another, returning Empty if no overlap.
    /// `domain_intersection` returns `None` exactly when the result is empty.
    pub fn intersect(&self, other: &Domain) -> Domain {
        match domain_intersection(self.clone(), other.clone()) {
            Some(d) => d,
            None => Domain::Empty,
        }
    }

    /// Check if a value is contained in this domain
    pub fn contains(&self, value: &LiteralValue) -> bool {
        match self {
            Domain::Empty => false,
            Domain::Unconstrained => true,
            Domain::Enumeration(values) => values.contains(value),
            Domain::Range { min, max } => value_within(value, min, max),
            Domain::Union(parts) => parts.iter().any(|p| p.contains(value)),
            Domain::Complement(inner) => !inner.contains(value),
        }
    }
}

/// Bound specification for ranges
#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    /// Inclusive bound [value
    Inclusive(Arc<LiteralValue>),

    /// Exclusive bound (value
    Exclusive(Arc<LiteralValue>),

    /// Unbounded (-infinity or +infinity)
    Unbounded,
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Domain::Empty => write!(f, "empty"),
            Domain::Unconstrained => write!(f, "any"),
            Domain::Enumeration(vals) => {
                write!(f, "{{")?;
                for (i, v) in vals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            Domain::Range { min, max } => {
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
                    Bound::Inclusive(v) | Bound::Exclusive(v) => v.as_ref().to_string(),
                };
                let max_str = match max {
                    Bound::Unbounded => "+inf".to_string(),
                    Bound::Inclusive(v) | Bound::Exclusive(v) => v.as_ref().to_string(),
                };
                write!(f, "{}{}, {}{}", l_bracket, min_str, max_str, r_bracket)
            }
            Domain::Union(parts) => {
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{}", p)?;
                }
                Ok(())
            }
            Domain::Complement(inner) => write!(f, "not ({})", inner),
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bound::Unbounded => write!(f, "inf"),
            Bound::Inclusive(v) => write!(f, "[{}", v.as_ref()),
            Bound::Exclusive(v) => write!(f, "({}", v.as_ref()),
        }
    }
}

impl Serialize for Domain {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Domain::Empty => {
                let mut st = serializer.serialize_struct("domain", 1)?;
                st.serialize_field("type", "empty")?;
                st.end()
            }
            Domain::Unconstrained => {
                let mut st = serializer.serialize_struct("domain", 1)?;
                st.serialize_field("type", "unconstrained")?;
                st.end()
            }
            Domain::Enumeration(vals) => {
                let mut st = serializer.serialize_struct("domain", 2)?;
                st.serialize_field("type", "enumeration")?;
                st.serialize_field("values", vals)?;
                st.end()
            }
            Domain::Range { min, max } => {
                let mut st = serializer.serialize_struct("domain", 3)?;
                st.serialize_field("type", "range")?;
                st.serialize_field("min", min)?;
                st.serialize_field("max", max)?;
                st.end()
            }
            Domain::Union(parts) => {
                let mut st = serializer.serialize_struct("domain", 2)?;
                st.serialize_field("type", "union")?;
                st.serialize_field("parts", parts)?;
                st.end()
            }
            Domain::Complement(inner) => {
                let mut st = serializer.serialize_struct("domain", 2)?;
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
                let mut st = serializer.serialize_struct("bound", 1)?;
                st.serialize_field("type", "unbounded")?;
                st.end()
            }
            Bound::Inclusive(v) => {
                let mut st = serializer.serialize_struct("bound", 2)?;
                st.serialize_field("type", "inclusive")?;
                st.serialize_field("value", v.as_ref())?;
                st.end()
            }
            Bound::Exclusive(v) => {
                let mut st = serializer.serialize_struct("bound", 2)?;
                st.serialize_field("type", "exclusive")?;
                st.serialize_field("value", v.as_ref())?;
                st.end()
            }
        }
    }
}

/// Extract domains for all facts mentioned in a constraint
pub fn extract_domains_from_constraint(
    constraint: &Constraint,
) -> Result<HashMap<FactPath, Domain>, crate::Error> {
    let all_facts = constraint.collect_facts();
    let mut domains = HashMap::new();

    for fact_path in all_facts {
        // None means the fact appears in the constraint but has no extractable
        // bound (e.g. only used in equality with another fact). Treating it as
        // Unconstrained is correct: the solver will enumerate values.
        let domain =
            extract_domain_for_fact(constraint, &fact_path)?.unwrap_or(Domain::Unconstrained);
        domains.insert(fact_path, domain);
    }

    Ok(domains)
}

fn extract_domain_for_fact(
    constraint: &Constraint,
    fact_path: &FactPath,
) -> Result<Option<Domain>, crate::Error> {
    let domain = match constraint {
        Constraint::True => return Ok(None),
        Constraint::False => Some(Domain::Enumeration(Arc::new(vec![]))),

        Constraint::Comparison { fact, op, value } => {
            if fact == fact_path {
                Some(comparison_to_domain(op, value.as_ref())?)
            } else {
                None
            }
        }

        Constraint::Fact(fp) => {
            if fp == fact_path {
                Some(Domain::Enumeration(Arc::new(vec![
                    LiteralValue::from_bool(true),
                ])))
            } else {
                None
            }
        }

        Constraint::And(left, right) => {
            let left_domain = extract_domain_for_fact(left, fact_path)?;
            let right_domain = extract_domain_for_fact(right, fact_path)?;
            match (left_domain, right_domain) {
                (None, None) => None,
                (Some(d), None) | (None, Some(d)) => Some(normalize_domain(d)),
                (Some(a), Some(b)) => match domain_intersection(a, b) {
                    Some(domain) => Some(domain),
                    None => Some(Domain::Enumeration(Arc::new(vec![]))),
                },
            }
        }

        Constraint::Or(left, right) => {
            let left_domain = extract_domain_for_fact(left, fact_path)?;
            let right_domain = extract_domain_for_fact(right, fact_path)?;
            union_optional_domains(left_domain, right_domain)
        }

        Constraint::Not(inner) => {
            // Handle not (fact == value)
            if let Constraint::Comparison { fact, op, value } = inner.as_ref() {
                if fact == fact_path && op.is_equal() {
                    return Ok(Some(normalize_domain(Domain::Complement(Box::new(
                        Domain::Enumeration(Arc::new(vec![value.as_ref().clone()])),
                    )))));
                }
            }

            // Handle not (boolean_fact)
            if let Constraint::Fact(fp) = inner.as_ref() {
                if fp == fact_path {
                    return Ok(Some(Domain::Enumeration(Arc::new(vec![
                        LiteralValue::from_bool(false),
                    ]))));
                }
            }

            extract_domain_for_fact(inner, fact_path)?
                .map(|domain| normalize_domain(Domain::Complement(Box::new(domain))))
        }
    };

    Ok(domain.map(normalize_domain))
}

fn comparison_to_domain(
    op: &ComparisonComputation,
    value: &LiteralValue,
) -> Result<Domain, crate::Error> {
    if op.is_equal() {
        return Ok(Domain::Enumeration(Arc::new(vec![value.clone()])));
    }
    if op.is_not_equal() {
        return Ok(Domain::Complement(Box::new(Domain::Enumeration(Arc::new(
            vec![value.clone()],
        )))));
    }
    match op {
        ComparisonComputation::LessThan => Ok(Domain::Range {
            min: Bound::Unbounded,
            max: Bound::Exclusive(Arc::new(value.clone())),
        }),
        ComparisonComputation::LessThanOrEqual => Ok(Domain::Range {
            min: Bound::Unbounded,
            max: Bound::Inclusive(Arc::new(value.clone())),
        }),
        ComparisonComputation::GreaterThan => Ok(Domain::Range {
            min: Bound::Exclusive(Arc::new(value.clone())),
            max: Bound::Unbounded,
        }),
        ComparisonComputation::GreaterThanOrEqual => Ok(Domain::Range {
            min: Bound::Inclusive(Arc::new(value.clone())),
            max: Bound::Unbounded,
        }),
        _ => unreachable!(
            "BUG: unsupported comparison operator for domain extraction: {:?}",
            op
        ),
    }
}

/// Compute the domain for a single comparison-atom used by inversion constraints.
///
/// This is used by numeric-aware constraint simplification to derive implications/exclusions
/// between comparison atoms on the same fact.
pub(crate) fn domain_for_comparison_atom(
    op: &ComparisonComputation,
    value: &LiteralValue,
) -> Result<Domain, crate::Error> {
    comparison_to_domain(op, value)
}

impl Domain {
    /// Proven subset check for the atom-domain forms we generate from comparisons:
    /// - Range
    /// - Enumeration
    /// - Complement(Enumeration) (used for != / is not)
    ///
    /// Returns false when the relationship cannot be proven with these forms.
    pub(crate) fn is_subset_of(&self, other: &Domain) -> bool {
        match (self, other) {
            (Domain::Empty, _) => true,
            (_, Domain::Unconstrained) => true,
            (Domain::Unconstrained, _) => false,

            (Domain::Enumeration(a), Domain::Enumeration(b)) => a.iter().all(|v| b.contains(v)),
            (Domain::Enumeration(vals), Domain::Range { min, max }) => {
                vals.iter().all(|v| value_within(v, min, max))
            }

            (
                Domain::Range {
                    min: amin,
                    max: amax,
                },
                Domain::Range {
                    min: bmin,
                    max: bmax,
                },
            ) => range_within_range(amin, amax, bmin, bmax),

            // Range ⊆ not({p}) when the range does not include p (for all excluded points)
            (Domain::Range { min, max }, Domain::Complement(inner)) => match inner.as_ref() {
                Domain::Enumeration(excluded) => {
                    excluded.iter().all(|p| !value_within(p, min, max))
                }
                _ => false,
            },

            // {v} ⊆ not({p}) when v is not excluded
            (Domain::Enumeration(vals), Domain::Complement(inner)) => match inner.as_ref() {
                Domain::Enumeration(excluded) => vals.iter().all(|v| !excluded.contains(v)),
                _ => false,
            },

            // not(A) ⊆ not(B)  iff  B ⊆ A  (for enumeration complements)
            (Domain::Complement(a_inner), Domain::Complement(b_inner)) => {
                match (a_inner.as_ref(), b_inner.as_ref()) {
                    (Domain::Enumeration(excluded_a), Domain::Enumeration(excluded_b)) => {
                        excluded_b.iter().all(|v| excluded_a.contains(v))
                    }
                    _ => false,
                }
            }

            _ => false,
        }
    }
}

fn range_within_range(amin: &Bound, amax: &Bound, bmin: &Bound, bmax: &Bound) -> bool {
    lower_bound_geq(amin, bmin) && upper_bound_leq(amax, bmax)
}

fn lower_bound_geq(a: &Bound, b: &Bound) -> bool {
    match (a, b) {
        (_, Bound::Unbounded) => true,
        (Bound::Unbounded, _) => false,
        (Bound::Inclusive(av), Bound::Inclusive(bv)) => lit_cmp(av.as_ref(), bv.as_ref()) >= 0,
        (Bound::Exclusive(av), Bound::Exclusive(bv)) => lit_cmp(av.as_ref(), bv.as_ref()) >= 0,
        (Bound::Exclusive(av), Bound::Inclusive(bv)) => {
            let c = lit_cmp(av.as_ref(), bv.as_ref());
            c >= 0
        }
        (Bound::Inclusive(av), Bound::Exclusive(bv)) => {
            // a >= (b) only if a's value > b's value
            lit_cmp(av.as_ref(), bv.as_ref()) > 0
        }
    }
}

fn upper_bound_leq(a: &Bound, b: &Bound) -> bool {
    match (a, b) {
        (Bound::Unbounded, Bound::Unbounded) => true,
        (_, Bound::Unbounded) => true,
        (Bound::Unbounded, _) => false,
        (Bound::Inclusive(av), Bound::Inclusive(bv)) => lit_cmp(av.as_ref(), bv.as_ref()) <= 0,
        (Bound::Exclusive(av), Bound::Exclusive(bv)) => lit_cmp(av.as_ref(), bv.as_ref()) <= 0,
        (Bound::Exclusive(av), Bound::Inclusive(bv)) => {
            // (a) <= [b] when a <= b
            lit_cmp(av.as_ref(), bv.as_ref()) <= 0
        }
        (Bound::Inclusive(av), Bound::Exclusive(bv)) => {
            // [a] <= (b) only if a < b
            lit_cmp(av.as_ref(), bv.as_ref()) < 0
        }
    }
}

fn union_optional_domains(a: Option<Domain>, b: Option<Domain>) -> Option<Domain> {
    match (a, b) {
        (None, None) => None,
        (Some(d), None) | (None, Some(d)) => Some(d),
        (Some(a), Some(b)) => Some(normalize_domain(Domain::Union(Arc::new(vec![a, b])))),
    }
}

fn lit_cmp(a: &LiteralValue, b: &LiteralValue) -> i8 {
    use std::cmp::Ordering;

    match (&a.value, &b.value) {
        (ValueKind::Number(la), ValueKind::Number(lb)) => match la.cmp(lb) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },

        (ValueKind::Boolean(la), ValueKind::Boolean(lb)) => match la.cmp(lb) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },

        (ValueKind::Text(la), ValueKind::Text(lb)) => match la.cmp(lb) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },

        (ValueKind::Date(la), ValueKind::Date(lb)) => match la.cmp(lb) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },

        (ValueKind::Time(la), ValueKind::Time(lb)) => match la.cmp(lb) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },

        (ValueKind::Duration(la, lua), ValueKind::Duration(lb, lub)) => {
            let a_sec = crate::computation::units::duration_to_seconds(*la, lua);
            let b_sec = crate::computation::units::duration_to_seconds(*lb, lub);
            match a_sec.cmp(&b_sec) {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            }
        }

        (ValueKind::Ratio(la, _), ValueKind::Ratio(lb, _)) => match la.cmp(lb) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },

        (ValueKind::Scale(la, lua), ValueKind::Scale(lb, lub)) => {
            if a.lemma_type != b.lemma_type {
                unreachable!(
                    "BUG: lit_cmp compared different scale types ({} vs {})",
                    a.lemma_type.name(),
                    b.lemma_type.name()
                );
            }

            if lua.eq_ignore_ascii_case(lub) {
                return match la.cmp(lb) {
                    Ordering::Less => -1,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                };
            }

            // Convert b to a's unit for comparison
            let target = SemanticConversionTarget::ScaleUnit(lua.clone());
            let converted = crate::computation::convert_unit(b, &target);
            let converted_value = match converted {
                OperationResult::Value(lit) => match lit.value {
                    ValueKind::Scale(v, _) => v,
                    _ => unreachable!("BUG: scale unit conversion returned non-scale value"),
                },
                OperationResult::Veto(msg) => {
                    unreachable!("BUG: scale unit conversion vetoed unexpectedly: {:?}", msg)
                }
            };

            match la.cmp(&converted_value) {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            }
        }

        _ => unreachable!(
            "BUG: lit_cmp cannot compare different literal kinds ({:?} vs {:?})",
            a.get_type(),
            b.get_type()
        ),
    }
}

fn value_within(v: &LiteralValue, min: &Bound, max: &Bound) -> bool {
    let ge_min = match min {
        Bound::Unbounded => true,
        Bound::Inclusive(m) => lit_cmp(v, m.as_ref()) >= 0,
        Bound::Exclusive(m) => lit_cmp(v, m.as_ref()) > 0,
    };
    let le_max = match max {
        Bound::Unbounded => true,
        Bound::Inclusive(m) => lit_cmp(v, m.as_ref()) <= 0,
        Bound::Exclusive(m) => lit_cmp(v, m.as_ref()) < 0,
    };
    ge_min && le_max
}

fn bounds_contradict(min: &Bound, max: &Bound) -> bool {
    match (min, max) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => false,
        (Bound::Inclusive(a), Bound::Inclusive(b)) => lit_cmp(a.as_ref(), b.as_ref()) > 0,
        (Bound::Inclusive(a), Bound::Exclusive(b)) => lit_cmp(a.as_ref(), b.as_ref()) >= 0,
        (Bound::Exclusive(a), Bound::Inclusive(b)) => lit_cmp(a.as_ref(), b.as_ref()) >= 0,
        (Bound::Exclusive(a), Bound::Exclusive(b)) => lit_cmp(a.as_ref(), b.as_ref()) >= 0,
    }
}

fn compute_intersection_min(min1: Bound, min2: Bound) -> Bound {
    match (min1, min2) {
        (Bound::Unbounded, x) | (x, Bound::Unbounded) => x,
        (Bound::Inclusive(v1), Bound::Inclusive(v2)) => {
            if lit_cmp(v1.as_ref(), v2.as_ref()) >= 0 {
                Bound::Inclusive(v1)
            } else {
                Bound::Inclusive(v2)
            }
        }
        (Bound::Inclusive(v1), Bound::Exclusive(v2)) => {
            if lit_cmp(v1.as_ref(), v2.as_ref()) > 0 {
                Bound::Inclusive(v1)
            } else {
                Bound::Exclusive(v2)
            }
        }
        (Bound::Exclusive(v1), Bound::Inclusive(v2)) => {
            if lit_cmp(v1.as_ref(), v2.as_ref()) > 0 {
                Bound::Exclusive(v1)
            } else {
                Bound::Inclusive(v2)
            }
        }
        (Bound::Exclusive(v1), Bound::Exclusive(v2)) => {
            if lit_cmp(v1.as_ref(), v2.as_ref()) >= 0 {
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
            if lit_cmp(v1.as_ref(), v2.as_ref()) <= 0 {
                Bound::Inclusive(v1)
            } else {
                Bound::Inclusive(v2)
            }
        }
        (Bound::Inclusive(v1), Bound::Exclusive(v2)) => {
            if lit_cmp(v1.as_ref(), v2.as_ref()) < 0 {
                Bound::Inclusive(v1)
            } else {
                Bound::Exclusive(v2)
            }
        }
        (Bound::Exclusive(v1), Bound::Inclusive(v2)) => {
            if lit_cmp(v1.as_ref(), v2.as_ref()) < 0 {
                Bound::Exclusive(v1)
            } else {
                Bound::Inclusive(v2)
            }
        }
        (Bound::Exclusive(v1), Bound::Exclusive(v2)) => {
            if lit_cmp(v1.as_ref(), v2.as_ref()) <= 0 {
                Bound::Exclusive(v1)
            } else {
                Bound::Exclusive(v2)
            }
        }
    }
}

fn domain_intersection(a: Domain, b: Domain) -> Option<Domain> {
    let a = normalize_domain(a);
    let b = normalize_domain(b);

    let result = match (a, b) {
        (Domain::Unconstrained, d) | (d, Domain::Unconstrained) => Some(d),
        (Domain::Empty, _) | (_, Domain::Empty) => None,

        (
            Domain::Range {
                min: min1,
                max: max1,
            },
            Domain::Range {
                min: min2,
                max: max2,
            },
        ) => {
            let min = compute_intersection_min(min1, min2);
            let max = compute_intersection_max(max1, max2);

            if bounds_contradict(&min, &max) {
                None
            } else {
                Some(Domain::Range { min, max })
            }
        }
        (Domain::Enumeration(v1), Domain::Enumeration(v2)) => {
            let filtered: Vec<LiteralValue> =
                v1.iter().filter(|x| v2.contains(x)).cloned().collect();
            if filtered.is_empty() {
                None
            } else {
                Some(Domain::Enumeration(Arc::new(filtered)))
            }
        }
        (Domain::Enumeration(vs), Domain::Range { min, max })
        | (Domain::Range { min, max }, Domain::Enumeration(vs)) => {
            let mut kept = Vec::new();
            for v in vs.iter() {
                if value_within(v, &min, &max) {
                    kept.push(v.clone());
                }
            }
            if kept.is_empty() {
                None
            } else {
                Some(Domain::Enumeration(Arc::new(kept)))
            }
        }
        (Domain::Enumeration(vs), Domain::Complement(inner))
        | (Domain::Complement(inner), Domain::Enumeration(vs)) => {
            match *inner.clone() {
                Domain::Enumeration(excluded) => {
                    let mut kept = Vec::new();
                    for v in vs.iter() {
                        if !excluded.contains(v) {
                            kept.push(v.clone());
                        }
                    }
                    if kept.is_empty() {
                        None
                    } else {
                        Some(Domain::Enumeration(Arc::new(kept)))
                    }
                }
                Domain::Range { min, max } => {
                    // Filter enumeration values that are NOT in the range
                    let mut kept = Vec::new();
                    for v in vs.iter() {
                        if !value_within(v, &min, &max) {
                            kept.push(v.clone());
                        }
                    }
                    if kept.is_empty() {
                        None
                    } else {
                        Some(Domain::Enumeration(Arc::new(kept)))
                    }
                }
                _ => {
                    // For other complement types, normalize and recurse
                    let normalized = normalize_domain(Domain::Complement(Box::new(*inner)));
                    domain_intersection(Domain::Enumeration(vs.clone()), normalized)
                }
            }
        }
        (Domain::Union(v1), Domain::Union(v2)) => {
            let mut acc: Vec<Domain> = Vec::new();
            for a in v1.iter() {
                for b in v2.iter() {
                    if let Some(ix) = domain_intersection(a.clone(), b.clone()) {
                        acc.push(ix);
                    }
                }
            }
            if acc.is_empty() {
                None
            } else {
                Some(Domain::Union(Arc::new(acc)))
            }
        }
        (Domain::Union(vs), d) | (d, Domain::Union(vs)) => {
            let mut acc: Vec<Domain> = Vec::new();
            for a in vs.iter() {
                if let Some(ix) = domain_intersection(a.clone(), d.clone()) {
                    acc.push(ix);
                }
            }
            if acc.is_empty() {
                None
            } else if acc.len() == 1 {
                Some(acc.remove(0))
            } else {
                Some(Domain::Union(Arc::new(acc)))
            }
        }
        // Range ∩ not({p1,p2,...})  =>  Range with excluded points removed (as union of ranges)
        (Domain::Range { min, max }, Domain::Complement(inner))
        | (Domain::Complement(inner), Domain::Range { min, max }) => match inner.as_ref() {
            Domain::Enumeration(excluded) => range_minus_excluded_points(min, max, excluded),
            _ => {
                // Normalize the complement (not just the inner value) and recurse.
                // If normalization doesn't change it, we must not recurse infinitely.
                let normalized_complement = normalize_domain(Domain::Complement(inner));
                if matches!(&normalized_complement, Domain::Complement(_)) {
                    None
                } else {
                    domain_intersection(Domain::Range { min, max }, normalized_complement)
                }
            }
        },
        (Domain::Complement(a_inner), Domain::Complement(b_inner)) => {
            match (a_inner.as_ref(), b_inner.as_ref()) {
                (Domain::Enumeration(a_ex), Domain::Enumeration(b_ex)) => {
                    // not(A) ∩ not(B) == not(A ∪ B)
                    let mut excluded: Vec<LiteralValue> = a_ex.iter().cloned().collect();
                    excluded.extend(b_ex.iter().cloned());
                    Some(normalize_domain(Domain::Complement(Box::new(
                        Domain::Enumeration(Arc::new(excluded)),
                    ))))
                }
                _ => None,
            }
        }
    };
    result.map(normalize_domain)
}

fn range_minus_excluded_points(
    min: Bound,
    max: Bound,
    excluded: &Arc<Vec<LiteralValue>>,
) -> Option<Domain> {
    // Start with a single range and iteratively split on excluded points that fall within it.
    let mut parts: Vec<(Bound, Bound)> = vec![(min, max)];

    for p in excluded.iter() {
        let mut next: Vec<(Bound, Bound)> = Vec::new();

        for (rmin, rmax) in parts {
            if !value_within(p, &rmin, &rmax) {
                next.push((rmin, rmax));
                continue;
            }

            // Left part: [rmin, p) or [rmin, p] depending on rmin and exclusion
            let left_max = Bound::Exclusive(Arc::new(p.clone()));
            if !bounds_contradict(&rmin, &left_max) {
                next.push((rmin.clone(), left_max));
            }

            // Right part: (p, rmax)
            let right_min = Bound::Exclusive(Arc::new(p.clone()));
            if !bounds_contradict(&right_min, &rmax) {
                next.push((right_min, rmax.clone()));
            }
        }

        parts = next;
        if parts.is_empty() {
            return None;
        }
    }

    if parts.is_empty() {
        None
    } else if parts.len() == 1 {
        let (min, max) = parts.remove(0);
        Some(Domain::Range { min, max })
    } else {
        Some(Domain::Union(Arc::new(
            parts
                .into_iter()
                .map(|(min, max)| Domain::Range { min, max })
                .collect(),
        )))
    }
}

fn invert_bound(bound: Bound) -> Bound {
    match bound {
        Bound::Unbounded => Bound::Unbounded,
        Bound::Inclusive(v) => Bound::Exclusive(v.clone()),
        Bound::Exclusive(v) => Bound::Inclusive(v.clone()),
    }
}

fn normalize_domain(d: Domain) -> Domain {
    match d {
        Domain::Complement(inner) => {
            let normalized_inner = normalize_domain(*inner);
            match normalized_inner {
                Domain::Complement(double_inner) => *double_inner,
                Domain::Range { min, max } => match (&min, &max) {
                    (Bound::Unbounded, Bound::Unbounded) => Domain::Enumeration(Arc::new(vec![])),
                    (Bound::Unbounded, max) => Domain::Range {
                        min: invert_bound(max.clone()),
                        max: Bound::Unbounded,
                    },
                    (min, Bound::Unbounded) => Domain::Range {
                        min: Bound::Unbounded,
                        max: invert_bound(min.clone()),
                    },
                    (min, max) => Domain::Union(Arc::new(vec![
                        Domain::Range {
                            min: Bound::Unbounded,
                            max: invert_bound(min.clone()),
                        },
                        Domain::Range {
                            min: invert_bound(max.clone()),
                            max: Bound::Unbounded,
                        },
                    ])),
                },
                Domain::Enumeration(vals) => {
                    if vals.len() == 1 {
                        if let Some(lit) = vals.first() {
                            if let ValueKind::Boolean(true) = &lit.value {
                                return Domain::Enumeration(Arc::new(vec![
                                    LiteralValue::from_bool(false),
                                ]));
                            }
                            if let ValueKind::Boolean(false) = &lit.value {
                                return Domain::Enumeration(Arc::new(vec![
                                    LiteralValue::from_bool(true),
                                ]));
                            }
                        }
                    }
                    Domain::Complement(Box::new(Domain::Enumeration(vals.clone())))
                }
                Domain::Unconstrained => Domain::Empty,
                Domain::Empty => Domain::Unconstrained,
                Domain::Union(parts) => Domain::Complement(Box::new(Domain::Union(parts.clone()))),
            }
        }
        Domain::Empty => Domain::Empty,
        Domain::Union(parts) => {
            let mut flat: Vec<Domain> = Vec::new();
            for p in parts.iter().cloned() {
                let normalized = normalize_domain(p);
                match normalized {
                    Domain::Union(inner) => flat.extend(inner.iter().cloned()),
                    Domain::Unconstrained => return Domain::Unconstrained,
                    Domain::Enumeration(vals) if vals.is_empty() => {}
                    other => flat.push(other),
                }
            }

            let mut all_enum_values: Vec<LiteralValue> = Vec::new();
            let mut ranges: Vec<Domain> = Vec::new();
            let mut others: Vec<Domain> = Vec::new();

            for domain in flat {
                match domain {
                    Domain::Enumeration(vals) => all_enum_values.extend(vals.iter().cloned()),
                    Domain::Range { .. } => ranges.push(domain),
                    other => others.push(other),
                }
            }

            all_enum_values.sort_by(|a, b| match lit_cmp(a, b) {
                -1 => Ordering::Less,
                0 => Ordering::Equal,
                _ => Ordering::Greater,
            });
            all_enum_values.dedup();

            all_enum_values.retain(|v| {
                !ranges.iter().any(|r| {
                    if let Domain::Range { min, max } = r {
                        value_within(v, min, max)
                    } else {
                        false
                    }
                })
            });

            let mut result: Vec<Domain> = Vec::new();
            result.extend(ranges);
            result = merge_ranges(result);

            if !all_enum_values.is_empty() {
                result.push(Domain::Enumeration(Arc::new(all_enum_values)));
            }
            result.extend(others);

            result.sort_by(|a, b| match (a, b) {
                (Domain::Range { .. }, Domain::Range { .. }) => Ordering::Equal,
                (Domain::Range { .. }, _) => Ordering::Less,
                (_, Domain::Range { .. }) => Ordering::Greater,
                (Domain::Enumeration(_), Domain::Enumeration(_)) => Ordering::Equal,
                (Domain::Enumeration(_), _) => Ordering::Less,
                (_, Domain::Enumeration(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            });

            if result.is_empty() {
                Domain::Enumeration(Arc::new(vec![]))
            } else if result.len() == 1 {
                result.remove(0)
            } else {
                Domain::Union(Arc::new(result))
            }
        }
        Domain::Enumeration(values) => {
            let mut sorted: Vec<LiteralValue> = values.iter().cloned().collect();
            sorted.sort_by(|a, b| match lit_cmp(a, b) {
                -1 => Ordering::Less,
                0 => Ordering::Equal,
                _ => Ordering::Greater,
            });
            sorted.dedup();
            Domain::Enumeration(Arc::new(sorted))
        }
        other => other,
    }
}

fn merge_ranges(domains: Vec<Domain>) -> Vec<Domain> {
    let mut result = Vec::new();
    let mut ranges: Vec<(Bound, Bound)> = Vec::new();
    let mut others = Vec::new();

    for d in domains {
        match d {
            Domain::Range { min, max } => ranges.push((min, max)),
            other => others.push(other),
        }
    }

    if ranges.is_empty() {
        return others;
    }

    ranges.sort_by(|a, b| compare_bounds(&a.0, &b.0));

    let mut merged: Vec<(Bound, Bound)> = Vec::new();
    let mut current = ranges[0].clone();

    for next in ranges.iter().skip(1) {
        if ranges_adjacent_or_overlap(&current, next) {
            current = (
                min_bound(&current.0, &next.0),
                max_bound(&current.1, &next.1),
            );
        } else {
            merged.push(current);
            current = next.clone();
        }
    }
    merged.push(current);

    for (min, max) in merged {
        result.push(Domain::Range { min, max });
    }
    result.extend(others);

    result
}

fn compare_bounds(a: &Bound, b: &Bound) -> Ordering {
    match (a, b) {
        (Bound::Unbounded, Bound::Unbounded) => Ordering::Equal,
        (Bound::Unbounded, _) => Ordering::Less,
        (_, Bound::Unbounded) => Ordering::Greater,
        (Bound::Inclusive(v1), Bound::Inclusive(v2))
        | (Bound::Exclusive(v1), Bound::Exclusive(v2)) => match lit_cmp(v1.as_ref(), v2.as_ref()) {
            -1 => Ordering::Less,
            0 => Ordering::Equal,
            _ => Ordering::Greater,
        },
        (Bound::Inclusive(v1), Bound::Exclusive(v2))
        | (Bound::Exclusive(v1), Bound::Inclusive(v2)) => match lit_cmp(v1.as_ref(), v2.as_ref()) {
            -1 => Ordering::Less,
            0 => {
                if matches!(a, Bound::Inclusive(_)) {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            _ => Ordering::Greater,
        },
    }
}

fn ranges_adjacent_or_overlap(r1: &(Bound, Bound), r2: &(Bound, Bound)) -> bool {
    match (&r1.1, &r2.0) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => true,
        (Bound::Inclusive(v1), Bound::Inclusive(v2))
        | (Bound::Inclusive(v1), Bound::Exclusive(v2)) => lit_cmp(v1.as_ref(), v2.as_ref()) >= 0,
        (Bound::Exclusive(v1), Bound::Inclusive(v2)) => lit_cmp(v1.as_ref(), v2.as_ref()) >= 0,
        (Bound::Exclusive(v1), Bound::Exclusive(v2)) => lit_cmp(v1.as_ref(), v2.as_ref()) > 0,
    }
}

fn min_bound(a: &Bound, b: &Bound) -> Bound {
    match (a, b) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => Bound::Unbounded,
        _ => {
            if matches!(compare_bounds(a, b), Ordering::Less | Ordering::Equal) {
                a.clone()
            } else {
                b.clone()
            }
        }
    }
}

fn max_bound(a: &Bound, b: &Bound) -> Bound {
    match (a, b) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => Bound::Unbounded,
        _ => {
            if matches!(compare_bounds(a, b), Ordering::Greater) {
                a.clone()
            } else {
                b.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn num(n: i64) -> LiteralValue {
        LiteralValue::number(Decimal::from(n))
    }

    fn fact(name: &str) -> FactPath {
        FactPath::new(vec![], name.to_string())
    }

    #[test]
    fn test_normalize_double_complement() {
        let inner = Domain::Enumeration(Arc::new(vec![num(5)]));
        let double = Domain::Complement(Box::new(Domain::Complement(Box::new(inner.clone()))));
        let normalized = normalize_domain(double);
        assert_eq!(normalized, inner);
    }

    #[test]
    fn test_normalize_union_absorbs_unconstrained() {
        let union = Domain::Union(Arc::new(vec![
            Domain::Range {
                min: Bound::Inclusive(Arc::new(num(0))),
                max: Bound::Inclusive(Arc::new(num(10))),
            },
            Domain::Unconstrained,
        ]));
        let normalized = normalize_domain(union);
        assert_eq!(normalized, Domain::Unconstrained);
    }

    #[test]
    fn test_domain_display() {
        let range = Domain::Range {
            min: Bound::Inclusive(Arc::new(num(10))),
            max: Bound::Exclusive(Arc::new(num(20))),
        };
        assert_eq!(format!("{}", range), "[10, 20)");

        let enumeration = Domain::Enumeration(Arc::new(vec![num(1), num(2), num(3)]));
        assert_eq!(format!("{}", enumeration), "{1, 2, 3}");
    }

    #[test]
    fn test_extract_domain_from_comparison() {
        let constraint = Constraint::Comparison {
            fact: fact("age"),
            op: ComparisonComputation::GreaterThan,
            value: Arc::new(num(18)),
        };

        let domains = extract_domains_from_constraint(&constraint).unwrap();
        let age_domain = domains.get(&fact("age")).unwrap();

        assert_eq!(
            *age_domain,
            Domain::Range {
                min: Bound::Exclusive(Arc::new(num(18))),
                max: Bound::Unbounded,
            }
        );
    }

    #[test]
    fn test_extract_domain_from_and() {
        let constraint = Constraint::And(
            Box::new(Constraint::Comparison {
                fact: fact("age"),
                op: ComparisonComputation::GreaterThan,
                value: Arc::new(num(18)),
            }),
            Box::new(Constraint::Comparison {
                fact: fact("age"),
                op: ComparisonComputation::LessThan,
                value: Arc::new(num(65)),
            }),
        );

        let domains = extract_domains_from_constraint(&constraint).unwrap();
        let age_domain = domains.get(&fact("age")).unwrap();

        assert_eq!(
            *age_domain,
            Domain::Range {
                min: Bound::Exclusive(Arc::new(num(18))),
                max: Bound::Exclusive(Arc::new(num(65))),
            }
        );
    }

    #[test]
    fn test_extract_domain_from_equality() {
        let constraint = Constraint::Comparison {
            fact: fact("status"),
            op: ComparisonComputation::Equal,
            value: Arc::new(LiteralValue::text("active".to_string())),
        };

        let domains = extract_domains_from_constraint(&constraint).unwrap();
        let status_domain = domains.get(&fact("status")).unwrap();

        assert_eq!(
            *status_domain,
            Domain::Enumeration(Arc::new(vec![LiteralValue::text("active".to_string())]))
        );
    }

    #[test]
    fn test_extract_domain_from_boolean_fact() {
        let constraint = Constraint::Fact(fact("is_active"));

        let domains = extract_domains_from_constraint(&constraint).unwrap();
        let is_active_domain = domains.get(&fact("is_active")).unwrap();

        assert_eq!(
            *is_active_domain,
            Domain::Enumeration(Arc::new(vec![LiteralValue::from_bool(true)]))
        );
    }

    #[test]
    fn test_extract_domain_from_not_boolean_fact() {
        let constraint = Constraint::Not(Box::new(Constraint::Fact(fact("is_active"))));

        let domains = extract_domains_from_constraint(&constraint).unwrap();
        let is_active_domain = domains.get(&fact("is_active")).unwrap();

        assert_eq!(
            *is_active_domain,
            Domain::Enumeration(Arc::new(vec![LiteralValue::from_bool(false)]))
        );
    }
}
