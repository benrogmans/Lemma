//! FactConstraint types and operations for inversion
//!
//! Provides:
//! - `FactConstraint` and `Bound` types for representing concrete value constraints
//! - FactConstraint operations: intersection, union, normalization
//! - `extract_fact_constraints_from_rule_constraint()`: extracts fact constraints from rule constraints

use crate::computation::{comparison_operation, OperationResult};
use crate::{BooleanValue, ComparisonComputation, FactPath, LemmaError, LemmaResult, LiteralValue};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

use super::constraint::RuleConstraint;

/// Constraint on a fact's valid values
#[derive(Debug, Clone, PartialEq)]
pub enum FactConstraint {
    /// A single continuous range
    Range { min: Bound, max: Bound },

    /// Multiple disjoint ranges
    Union(Vec<FactConstraint>),

    /// Specific enumerated values only
    Enumeration(Vec<LiteralValue>),

    /// Everything except these constraints
    Complement(Box<FactConstraint>),

    /// Any value (no constraints)
    Unconstrained,

    /// Empty domain (no valid values) - represents unsatisfiable constraints
    Empty,
}

impl FactConstraint {
    /// Check if this domain is satisfiable (has at least one valid value)
    ///
    /// Returns false for Empty domains and empty Enumerations.
    pub fn is_satisfiable(&self) -> bool {
        match self {
            FactConstraint::Empty => false,
            FactConstraint::Enumeration(values) => !values.is_empty(),
            FactConstraint::Union(parts) => parts.iter().any(|p| p.is_satisfiable()),
            FactConstraint::Range { min, max } => !bounds_contradict(min, max),
            FactConstraint::Complement(inner) => !matches!(inner.as_ref(), FactConstraint::Unconstrained),
            FactConstraint::Unconstrained => true,
        }
    }

    /// Check if this domain is empty (unsatisfiable)
    pub fn is_empty(&self) -> bool {
        !self.is_satisfiable()
    }

    /// Intersect this domain with another, returning Empty if no overlap
    pub fn intersect(&self, other: &FactConstraint) -> FactConstraint {
        domain_intersection(self.clone(), other.clone()).unwrap_or(FactConstraint::Empty)
    }
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

impl fmt::Display for FactConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FactConstraint::Empty => write!(f, "empty"),
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

impl Serialize for FactConstraint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            FactConstraint::Empty => {
                let mut st = serializer.serialize_struct("domain", 1)?;
                st.serialize_field("type", "empty")?;
                st.end()
            }
            FactConstraint::Unconstrained => {
                let mut st = serializer.serialize_struct("domain", 1)?;
                st.serialize_field("type", "unconstrained")?;
                st.end()
            }
            FactConstraint::Enumeration(vals) => {
                let mut st = serializer.serialize_struct("domain", 2)?;
                st.serialize_field("type", "enumeration")?;
                st.serialize_field("values", vals)?;
                st.end()
            }
            FactConstraint::Range { min, max } => {
                let mut st = serializer.serialize_struct("domain", 3)?;
                st.serialize_field("type", "range")?;
                st.serialize_field("min", min)?;
                st.serialize_field("max", max)?;
                st.end()
            }
            FactConstraint::Union(parts) => {
                let mut st = serializer.serialize_struct("domain", 2)?;
                st.serialize_field("type", "union")?;
                st.serialize_field("parts", parts)?;
                st.end()
            }
            FactConstraint::Complement(inner) => {
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
                st.serialize_field("value", v)?;
                st.end()
            }
            Bound::Exclusive(v) => {
                let mut st = serializer.serialize_struct("bound", 2)?;
                st.serialize_field("type", "exclusive")?;
                st.serialize_field("value", v)?;
                st.end()
            }
        }
    }
}

/// Extract domains for all facts mentioned in a constraint
pub fn extract_fact_constraints_from_rule_constraint(
    constraint: &RuleConstraint,
) -> LemmaResult<HashMap<FactPath, FactConstraint>> {
    let all_facts = constraint.collect_facts();
    let mut domains = HashMap::new();

    for fact_path in all_facts {
        let domain =
            extract_domain_for_fact(constraint, &fact_path)?.unwrap_or(FactConstraint::Unconstrained);
        domains.insert(fact_path, domain);
    }

    Ok(domains)
}

fn extract_domain_for_fact(
    constraint: &RuleConstraint,
    fact_path: &FactPath,
) -> LemmaResult<Option<FactConstraint>> {
    let domain = match constraint {
        RuleConstraint::True => return Ok(None),
        RuleConstraint::False => Some(FactConstraint::Enumeration(vec![])),

        RuleConstraint::Comparison { fact, op, value } => {
            if fact == fact_path {
                Some(comparison_to_domain(op, value)?)
            } else {
                None
            }
        }

        RuleConstraint::Fact(fp) => {
            if fp == fact_path {
                Some(FactConstraint::Enumeration(vec![LiteralValue::Boolean(
                    BooleanValue::True,
                )]))
            } else {
                None
            }
        }

        RuleConstraint::And(left, right) => {
            let left_domain = extract_domain_for_fact(left, fact_path)?;
            let right_domain = extract_domain_for_fact(right, fact_path)?;
            match (left_domain, right_domain) {
                (None, None) => None,
                (Some(d), None) | (None, Some(d)) => Some(normalize_domain(d)),
                (Some(a), Some(b)) => match domain_intersection(a, b) {
                    Some(domain) => Some(domain),
                    None => Some(FactConstraint::Enumeration(vec![])),
                },
            }
        }

        RuleConstraint::Or(left, right) => {
            let left_domain = extract_domain_for_fact(left, fact_path)?;
            let right_domain = extract_domain_for_fact(right, fact_path)?;
            union_optional_domains(left_domain, right_domain)
        }

        RuleConstraint::Not(inner) => {
            // Handle not (fact == value)
            if let RuleConstraint::Comparison { fact, op, value } = inner.as_ref() {
                if fact == fact_path && op.is_equal() {
                    return Ok(Some(normalize_domain(FactConstraint::Complement(Box::new(
                        FactConstraint::Enumeration(vec![value.clone()]),
                    )))));
                }
            }

            // Handle not (boolean_fact)
            if let RuleConstraint::Fact(fp) = inner.as_ref() {
                if fp == fact_path {
                    return Ok(Some(FactConstraint::Enumeration(vec![LiteralValue::Boolean(
                        BooleanValue::False,
                    )])));
                }
            }

            extract_domain_for_fact(inner, fact_path)?
                .map(|domain| normalize_domain(FactConstraint::Complement(Box::new(domain))))
        }
    };

    Ok(domain.map(normalize_domain))
}

fn comparison_to_domain(op: &ComparisonComputation, value: &LiteralValue) -> LemmaResult<FactConstraint> {
    if op.is_equal() {
        return Ok(FactConstraint::Enumeration(vec![value.clone()]));
    }
    if op.is_not_equal() {
        return Ok(FactConstraint::Complement(Box::new(FactConstraint::Enumeration(vec![
            value.clone(),
        ]))));
    }
    match op {
        ComparisonComputation::LessThan => Ok(FactConstraint::Range {
            min: Bound::Unbounded,
            max: Bound::Exclusive(value.clone()),
        }),
        ComparisonComputation::LessThanOrEqual => Ok(FactConstraint::Range {
            min: Bound::Unbounded,
            max: Bound::Inclusive(value.clone()),
        }),
        ComparisonComputation::GreaterThan => Ok(FactConstraint::Range {
            min: Bound::Exclusive(value.clone()),
            max: Bound::Unbounded,
        }),
        ComparisonComputation::GreaterThanOrEqual => Ok(FactConstraint::Range {
            min: Bound::Inclusive(value.clone()),
            max: Bound::Unbounded,
        }),
        _ => Err(LemmaError::Engine(format!(
            "Unsupported comparison operator for domain extraction: {:?}",
            op
        ))),
    }
}

fn union_optional_domains(a: Option<FactConstraint>, b: Option<FactConstraint>) -> Option<FactConstraint> {
    match (a, b) {
        (None, None) => None,
        (Some(d), None) | (None, Some(d)) => Some(d),
        (Some(a), Some(b)) => Some(normalize_domain(FactConstraint::Union(vec![a, b]))),
    }
}

fn lit_cmp(a: &LiteralValue, b: &LiteralValue) -> i8 {
    use crate::EqualityNotation;
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

fn domain_intersection(a: FactConstraint, b: FactConstraint) -> Option<FactConstraint> {
    let a = normalize_domain(a);
    let b = normalize_domain(b);

    let result = match (a, b) {
        (FactConstraint::Unconstrained, d) | (d, FactConstraint::Unconstrained) => Some(d),
        (FactConstraint::Empty, _) | (_, FactConstraint::Empty) => None,

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
                None
            } else {
                Some(FactConstraint::Range { min, max })
            }
        }
        (FactConstraint::Enumeration(mut v1), FactConstraint::Enumeration(v2)) => {
            v1.retain(|x| v2.contains(x));
            if v1.is_empty() {
                None
            } else {
                Some(FactConstraint::Enumeration(v1))
            }
        }
        (FactConstraint::Enumeration(vs), FactConstraint::Range { min, max })
        | (FactConstraint::Range { min, max }, FactConstraint::Enumeration(vs)) => {
            let mut kept = Vec::new();
            for v in vs {
                if value_within(&v, &min, &max) {
                    kept.push(v);
                }
            }
            if kept.is_empty() {
                None
            } else {
                Some(FactConstraint::Enumeration(kept))
            }
        }
        (FactConstraint::Enumeration(vs), FactConstraint::Complement(inner))
        | (FactConstraint::Complement(inner), FactConstraint::Enumeration(vs)) => match *inner.clone() {
            FactConstraint::Enumeration(excluded) => {
                let mut kept = Vec::new();
                for v in vs {
                    if !excluded.contains(&v) {
                        kept.push(v);
                    }
                }
                if kept.is_empty() {
                    None
                } else {
                    Some(FactConstraint::Enumeration(kept))
                }
            }
            _ => None,
        },
        (FactConstraint::Union(v1), FactConstraint::Union(v2)) => {
            let mut acc: Vec<FactConstraint> = Vec::new();
            for a in v1.into_iter() {
                for b in v2.iter() {
                    if let Some(ix) = domain_intersection(a.clone(), b.clone()) {
                        acc.push(ix);
                    }
                }
            }
            if acc.is_empty() {
                None
            } else {
                Some(FactConstraint::Union(acc))
            }
        }
        (FactConstraint::Union(vs), d) | (d, FactConstraint::Union(vs)) => {
            let mut acc: Vec<FactConstraint> = Vec::new();
            for a in vs.into_iter() {
                if let Some(ix) = domain_intersection(a, d.clone()) {
                    acc.push(ix);
                }
            }
            if acc.is_empty() {
                None
            } else if acc.len() == 1 {
                Some(acc.remove(0))
            } else {
                Some(FactConstraint::Union(acc))
            }
        }
        (FactConstraint::Complement(inner), other) | (other, FactConstraint::Complement(inner)) => {
            let normalized_complement = normalize_domain(*inner);
            domain_intersection(other, normalized_complement)
        }
        #[allow(unreachable_patterns)]
        _ => None,
    };
    result.map(normalize_domain)
}

fn invert_bound(bound: Bound) -> Bound {
    match bound {
        Bound::Unbounded => Bound::Unbounded,
        Bound::Inclusive(v) => Bound::Exclusive(v),
        Bound::Exclusive(v) => Bound::Inclusive(v),
    }
}

fn normalize_domain(d: FactConstraint) -> FactConstraint {
    match d {
        FactConstraint::Complement(inner) => {
            let normalized_inner = normalize_domain(*inner);
            match normalized_inner {
                FactConstraint::Complement(double_inner) => *double_inner,
                FactConstraint::Range { min, max } => match (&min, &max) {
                    (Bound::Unbounded, Bound::Unbounded) => FactConstraint::Enumeration(vec![]),
                    (Bound::Unbounded, max) => FactConstraint::Range {
                        min: invert_bound(max.clone()),
                        max: Bound::Unbounded,
                    },
                    (min, Bound::Unbounded) => FactConstraint::Range {
                        min: Bound::Unbounded,
                        max: invert_bound(min.clone()),
                    },
                    (min, max) => FactConstraint::Union(vec![
                        FactConstraint::Range {
                            min: Bound::Unbounded,
                            max: invert_bound(min.clone()),
                        },
                        FactConstraint::Range {
                            min: invert_bound(max.clone()),
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
                FactConstraint::Unconstrained => FactConstraint::Empty,
                FactConstraint::Empty => FactConstraint::Unconstrained,
                FactConstraint::Union(parts) => FactConstraint::Complement(Box::new(FactConstraint::Union(parts))),
            }
        }
        FactConstraint::Empty => FactConstraint::Empty,
        FactConstraint::Union(mut parts) => {
            let mut flat: Vec<FactConstraint> = Vec::new();
            for p in parts.drain(..) {
                let normalized = normalize_domain(p);
                match normalized {
                    FactConstraint::Union(inner) => flat.extend(inner),
                    FactConstraint::Unconstrained => return FactConstraint::Unconstrained,
                    FactConstraint::Enumeration(vals) if vals.is_empty() => {}
                    other => flat.push(other),
                }
            }

            let mut all_enum_values: Vec<LiteralValue> = Vec::new();
            let mut ranges: Vec<FactConstraint> = Vec::new();
            let mut others: Vec<FactConstraint> = Vec::new();

            for domain in flat {
                match domain {
                    FactConstraint::Enumeration(vals) => all_enum_values.extend(vals),
                    FactConstraint::Range { .. } => ranges.push(domain),
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
                    if let FactConstraint::Range { min, max } = r {
                        value_within(v, min, max)
                    } else {
                        false
                    }
                })
            });

            let mut result: Vec<FactConstraint> = Vec::new();
            result.extend(ranges);
            result = merge_ranges(result);

            if !all_enum_values.is_empty() {
                result.push(FactConstraint::Enumeration(all_enum_values));
            }
            result.extend(others);

            result.sort_by(|a, b| match (a, b) {
                (FactConstraint::Range { .. }, FactConstraint::Range { .. }) => Ordering::Equal,
                (FactConstraint::Range { .. }, _) => Ordering::Less,
                (_, FactConstraint::Range { .. }) => Ordering::Greater,
                (FactConstraint::Enumeration(_), FactConstraint::Enumeration(_)) => Ordering::Equal,
                (FactConstraint::Enumeration(_), _) => Ordering::Less,
                (_, FactConstraint::Enumeration(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            });

            if result.is_empty() {
                FactConstraint::Enumeration(vec![])
            } else if result.len() == 1 {
                result.remove(0)
            } else {
                FactConstraint::Union(result)
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

fn merge_ranges(domains: Vec<FactConstraint>) -> Vec<FactConstraint> {
    let mut result = Vec::new();
    let mut ranges: Vec<(Bound, Bound)> = Vec::new();
    let mut others = Vec::new();

    for d in domains {
        match d {
            FactConstraint::Range { min, max } => ranges.push((min, max)),
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
        result.push(FactConstraint::Range { min, max });
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
        | (Bound::Exclusive(v1), Bound::Exclusive(v2)) => match lit_cmp(v1, v2) {
            -1 => Ordering::Less,
            0 => Ordering::Equal,
            _ => Ordering::Greater,
        },
        (Bound::Inclusive(v1), Bound::Exclusive(v2))
        | (Bound::Exclusive(v1), Bound::Inclusive(v2)) => match lit_cmp(v1, v2) {
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
        | (Bound::Inclusive(v1), Bound::Exclusive(v2)) => lit_cmp(v1, v2) >= 0,
        (Bound::Exclusive(v1), Bound::Inclusive(v2)) => lit_cmp(v1, v2) >= 0,
        (Bound::Exclusive(v1), Bound::Exclusive(v2)) => lit_cmp(v1, v2) > 0,
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
        LiteralValue::Number(Decimal::from(n))
    }

    fn fact(name: &str) -> FactPath {
        FactPath::local(name.to_string())
    }

    #[test]
    fn test_normalize_double_complement() {
        let inner = FactConstraint::Enumeration(vec![num(5)]);
        let double = FactConstraint::Complement(Box::new(FactConstraint::Complement(Box::new(inner.clone()))));
        let normalized = normalize_domain(double);
        assert_eq!(normalized, inner);
    }

    #[test]
    fn test_normalize_union_absorbs_unconstrained() {
        let union = FactConstraint::Union(vec![
            FactConstraint::Range {
                min: Bound::Inclusive(num(0)),
                max: Bound::Inclusive(num(10)),
            },
            FactConstraint::Unconstrained,
        ]);
        let normalized = normalize_domain(union);
        assert_eq!(normalized, FactConstraint::Unconstrained);
    }

    #[test]
    fn test_domain_display() {
        let range = FactConstraint::Range {
            min: Bound::Inclusive(num(10)),
            max: Bound::Exclusive(num(20)),
        };
        assert_eq!(format!("{}", range), "[10, 20)");

        let enumeration = FactConstraint::Enumeration(vec![num(1), num(2), num(3)]);
        assert_eq!(format!("{}", enumeration), "{1, 2, 3}");
    }

    #[test]
    fn test_extract_domain_from_comparison() {
        let constraint = RuleConstraint::Comparison {
            fact: fact("age"),
            op: ComparisonComputation::GreaterThan,
            value: num(18),
        };

        let domains = extract_fact_constraints_from_rule_constraint(&constraint).unwrap();
        let age_domain = domains.get(&fact("age")).unwrap();

        assert_eq!(
            *age_domain,
            FactConstraint::Range {
                min: Bound::Exclusive(num(18)),
                max: Bound::Unbounded,
            }
        );
    }

    #[test]
    fn test_extract_domain_from_and() {
        let constraint = RuleConstraint::And(
            Box::new(RuleConstraint::Comparison {
                fact: fact("age"),
                op: ComparisonComputation::GreaterThan,
                value: num(18),
            }),
            Box::new(RuleConstraint::Comparison {
                fact: fact("age"),
                op: ComparisonComputation::LessThan,
                value: num(65),
            }),
        );

        let domains = extract_fact_constraints_from_rule_constraint(&constraint).unwrap();
        let age_domain = domains.get(&fact("age")).unwrap();

        assert_eq!(
            *age_domain,
            FactConstraint::Range {
                min: Bound::Exclusive(num(18)),
                max: Bound::Exclusive(num(65)),
            }
        );
    }

    #[test]
    fn test_extract_domain_from_equality() {
        use crate::EqualityNotation;
        let constraint = RuleConstraint::Comparison {
            fact: fact("status"),
            op: ComparisonComputation::Equal(EqualityNotation::Symbol),
            value: LiteralValue::Text("active".to_string()),
        };

        let domains = extract_fact_constraints_from_rule_constraint(&constraint).unwrap();
        let status_domain = domains.get(&fact("status")).unwrap();

        assert_eq!(
            *status_domain,
            FactConstraint::Enumeration(vec![LiteralValue::Text("active".to_string())])
        );
    }

    #[test]
    fn test_extract_domain_from_boolean_fact() {
        let constraint = RuleConstraint::Fact(fact("is_active"));

        let domains = extract_fact_constraints_from_rule_constraint(&constraint).unwrap();
        let is_active_domain = domains.get(&fact("is_active")).unwrap();

        assert_eq!(
            *is_active_domain,
            FactConstraint::Enumeration(vec![LiteralValue::Boolean(BooleanValue::True)])
        );
    }

    #[test]
    fn test_extract_domain_from_not_boolean_fact() {
        let constraint = RuleConstraint::Not(Box::new(RuleConstraint::Fact(fact("is_active"))));

        let domains = extract_fact_constraints_from_rule_constraint(&constraint).unwrap();
        let is_active_domain = domains.get(&fact("is_active")).unwrap();

        assert_eq!(
            *is_active_domain,
            FactConstraint::Enumeration(vec![LiteralValue::Boolean(BooleanValue::False)])
        );
    }
}
