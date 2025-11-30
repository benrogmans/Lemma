//! Domain collapsing: converts symbolic expressions to concrete value sets
//!
//! This module provides:
//! - `Domain` and `Bound` types for representing concrete value constraints
//! - Domain operations: intersection, union, negation, normalization
//! - `shape_to_domains()`: collapses symbolic Shape expressions → concrete Domain value sets
//!
//! ## Architecture
//!
//! - **Shape** = Symbolic algebraic function (piecewise function with symbolic expressions)
//! - **Domain collapsing** = Concretization layer that:
//!   - Collapses symbolic expressions → concrete value sets (ranges, enumerations)
//!   - Detects numeric contradictions (empty domains) during collapse
//!   - Filters unsatisfiable branches
//!
//! Shape may contain branches that are symbolically valid but numerically contradictory.
//! Domain collapsing filters these out by detecting empty domains.

use crate::evaluation::operations::{comparison_operation, OperationResult};
use crate::{
    BooleanValue, ComparisonComputation, Expression, ExpressionKind, FactPath, LemmaError,
    LemmaResult, LiteralValue,
};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

use super::expansion;
use super::shape::Shape;
use super::solver;

/// Domain specification for valid values
#[derive(Debug, Clone, PartialEq)]
pub enum Domain {
    /// A single continuous range
    Range { min: Bound, max: Bound },

    /// Multiple disjoint ranges
    Union(Vec<Domain>),

    /// Specific enumerated values only
    Enumeration(Vec<LiteralValue>),

    /// Everything except these constraints
    Complement(Box<Domain>),

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

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
                    Bound::Inclusive(v) | Bound::Exclusive(v) => v.to_string(),
                };
                let max_str = match max {
                    Bound::Unbounded => "+inf".to_string(),
                    Bound::Inclusive(v) | Bound::Exclusive(v) => v.to_string(),
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
            Bound::Inclusive(v) => write!(f, "[{}", v),
            Bound::Exclusive(v) => write!(f, "({}", v),
        }
    }
}

impl Serialize for Domain {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
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

fn find_all_variables_in_expression(expr: &Expression) -> Vec<FactPath> {
    let mut variables = Vec::new();
    collect_fact_paths(expr, &mut variables);
    variables.sort_by(|a, b| {
        let a_facts: Vec<&String> = a.segments.iter().map(|s| &s.fact).collect();
        let b_facts: Vec<&String> = b.segments.iter().map(|s| &s.fact).collect();
        a_facts.cmp(&b_facts).then(a.fact.cmp(&b.fact))
    });
    variables.dedup();
    variables
}

fn collect_fact_paths(expr: &Expression, result: &mut Vec<FactPath>) {
    match &expr.kind {
        ExpressionKind::FactPath(fp) => {
            result.push(fp.clone());
        }
        ExpressionKind::Arithmetic(l, _, r) => {
            collect_fact_paths(l, result);
            collect_fact_paths(r, result);
        }
        ExpressionKind::Comparison(l, _, r) => {
            collect_fact_paths(l, result);
            collect_fact_paths(r, result);
        }
        ExpressionKind::LogicalAnd(l, r) => {
            collect_fact_paths(l, result);
            collect_fact_paths(r, result);
        }
        ExpressionKind::LogicalOr(l, r) => {
            collect_fact_paths(l, result);
            collect_fact_paths(r, result);
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            collect_fact_paths(inner, result);
        }
        ExpressionKind::UnitConversion(inner, _) => {
            collect_fact_paths(inner, result);
        }
        ExpressionKind::MathematicalComputation(_, inner) => {
            collect_fact_paths(inner, result);
        }
        _ => {}
    }
}

fn extract_domains_for_all_variables(
    condition: &Expression,
) -> LemmaResult<HashMap<FactPath, Domain>> {
    let variables = find_all_variables_in_expression(condition);
    let mut domains = HashMap::new();

    for var in variables {
        let domain = extract_domain_for_variable(condition, &var)?.unwrap_or(Domain::Unconstrained);
        domains.insert(var, domain);
    }

    Ok(domains)
}

/// Collapse a Shape into concrete domains for each free variable
///
/// Converts symbolic expressions in Shape branches to concrete value sets (domains).
/// Filters out branches with unsatisfiable conditions (detected as empty domains).
pub fn shape_to_domains(shape: &Shape) -> LemmaResult<Vec<HashMap<FactPath, Domain>>> {
    let mut result = Vec::new();

    for branch in &shape.branches {
        if let ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)) =
            &branch.condition.kind
        {
            continue;
        }

        let domains = extract_domains_for_all_variables(&branch.condition)?;

        if domains.values().any(is_empty_domain) {
            continue;
        }

        result.push(domains);
    }

    if result.is_empty() {
        return Err(LemmaError::Engine(format!(
            "No valid solutions: all {} branch constraint(s) are unsatisfiable",
            shape.branches.len()
        )));
    }

    Ok(result)
}

fn extract_domain_for_variable(
    condition: &Expression,
    var: &FactPath,
) -> LemmaResult<Option<Domain>> {
    match &condition.kind {
        ExpressionKind::Literal(lit) => {
            if let LiteralValue::Boolean(BooleanValue::True) = lit {
                Ok(None)
            } else {
                Ok(Some(Domain::Enumeration(vec![])))
            }
        }

        ExpressionKind::Comparison(lhs, op, rhs) => {
            extract_comparison_constraint(lhs, op, rhs, var)
        }

        ExpressionKind::LogicalAnd(lhs, rhs) => {
            let left_domain = extract_domain_for_variable(lhs, var)?;
            let right_domain = extract_domain_for_variable(rhs, var)?;
            match (left_domain, right_domain) {
                (None, None) => Ok(None),
                (Some(d), None) | (None, Some(d)) => Ok(Some(normalize_domain(d))),
                (Some(a), Some(b)) => {
                    let normalized_a = normalize_domain(a);
                    let normalized_b = normalize_domain(b);
                    match domain_intersection(normalized_a, normalized_b) {
                        Some(domain) => Ok(Some(domain)),
                        None => Ok(Some(Domain::Enumeration(vec![]))),
                    }
                }
            }
        }

        ExpressionKind::LogicalOr(lhs, rhs) => {
            let left_domain = extract_domain_for_variable(lhs, var)?;
            let right_domain = extract_domain_for_variable(rhs, var)?;
            Ok(union_optional_domains(left_domain, right_domain))
        }

        ExpressionKind::LogicalNegation(inner, _) => {
            if let ExpressionKind::Comparison(lhs, ComparisonComputation::Equal, rhs) = &inner.kind
            {
                if matches!(&lhs.kind, ExpressionKind::FactPath(fp) if fp == var) {
                    if let ExpressionKind::Literal(lit) = &rhs.kind {
                        return Ok(Some(Domain::Complement(Box::new(Domain::Enumeration(
                            vec![lit.clone()],
                        )))));
                    }
                }
            }

            if let Some(domain) = extract_domain_for_variable(inner, var)? {
                Ok(Some(normalize_domain(Domain::Complement(Box::new(domain)))))
            } else {
                Ok(None)
            }
        }

        _ => Ok(None),
    }
}

fn extract_comparison_constraint(
    lhs: &Expression,
    op: &ComparisonComputation,
    rhs: &Expression,
    var: &FactPath,
) -> LemmaResult<Option<Domain>> {
    let is_var_directly_on_left = matches!(&lhs.kind, ExpressionKind::FactPath(fp) if fp == var);
    let is_var_directly_on_right = matches!(&rhs.kind, ExpressionKind::FactPath(fp) if fp == var);

    if is_var_directly_on_left {
        if let ExpressionKind::Literal(lit) = &rhs.kind {
            return Ok(Some(comparison_to_domain(op, lit, false)?));
        }
    } else if is_var_directly_on_right {
        if let ExpressionKind::Literal(lit) = &lhs.kind {
            return Ok(Some(comparison_to_domain(op, lit, true)?));
        }
    }

    let unknown = if var.is_local() {
        (String::new(), var.fact.clone())
    } else if var.segments.len() == 1 {
        (var.segments[0].fact.clone(), var.fact.clone())
    } else {
        return Ok(None);
    };

    let fact_matcher = |fp: &FactPath, doc: &str, name: &str| -> bool {
        if fp.is_local() {
            fp.fact == name && doc.is_empty()
        } else if fp.segments.len() == 1 {
            fp.segments[0].fact == doc && fp.fact == name
        } else {
            false
        }
    };

    if let ExpressionKind::Literal(target_lit) = &rhs.kind {
        if solver::contains_unknown(lhs, &unknown, &fact_matcher)
            && !is_var_directly_on_left
            && solver::can_algebraically_solve(lhs, &unknown, &fact_matcher)
        {
            let target_expr = Expression::new(
                ExpressionKind::Literal(target_lit.clone()),
                None,
                crate::parsing::ast::ExpressionId::new(0),
            );
            if let Ok(solved) = solver::algebraic_solve(lhs, &unknown, &target_expr, &fact_matcher)
            {
                let folded = expansion::try_constant_fold(&solved).unwrap_or(solved);
                if let ExpressionKind::Literal(lit) = &folded.kind {
                    return Ok(Some(comparison_to_domain(op, lit, false)?));
                }
            }
        }
    }

    if let ExpressionKind::Literal(target_lit) = &lhs.kind {
        if solver::contains_unknown(rhs, &unknown, &fact_matcher)
            && !is_var_directly_on_right
            && solver::can_algebraically_solve(rhs, &unknown, &fact_matcher)
        {
            let target_expr = Expression::new(
                ExpressionKind::Literal(target_lit.clone()),
                None,
                crate::parsing::ast::ExpressionId::new(0),
            );
            if let Ok(solved) = solver::algebraic_solve(rhs, &unknown, &target_expr, &fact_matcher)
            {
                let folded = expansion::try_constant_fold(&solved).unwrap_or(solved);
                if let ExpressionKind::Literal(lit) = &folded.kind {
                    return Ok(Some(comparison_to_domain(op, lit, true)?));
                }
            }
        }
    }

    Ok(None)
}

fn comparison_to_domain(
    op: &ComparisonComputation,
    value: &LiteralValue,
    flipped: bool,
) -> LemmaResult<Domain> {
    let effective_op = if flipped {
        flip_operator(op)
    } else {
        op.clone()
    };

    match effective_op {
        ComparisonComputation::Equal | ComparisonComputation::Is => {
            Ok(Domain::Enumeration(vec![value.clone()]))
        }
        ComparisonComputation::NotEqual => {
            Ok(Domain::Complement(Box::new(Domain::Enumeration(vec![
                value.clone(),
            ]))))
        }
        ComparisonComputation::LessThan => Ok(Domain::Range {
            min: Bound::Unbounded,
            max: Bound::Exclusive(value.clone()),
        }),
        ComparisonComputation::LessThanOrEqual => Ok(Domain::Range {
            min: Bound::Unbounded,
            max: Bound::Inclusive(value.clone()),
        }),
        ComparisonComputation::GreaterThan => Ok(Domain::Range {
            min: Bound::Exclusive(value.clone()),
            max: Bound::Unbounded,
        }),
        ComparisonComputation::GreaterThanOrEqual => Ok(Domain::Range {
            min: Bound::Inclusive(value.clone()),
            max: Bound::Unbounded,
        }),
        _ => Err(LemmaError::Engine(format!(
            "Unsupported comparison operator for domain extraction: {:?}",
            effective_op
        ))),
    }
}

fn flip_operator(op: &ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::Equal => ComparisonComputation::Equal,
        ComparisonComputation::NotEqual => ComparisonComputation::NotEqual,
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThan,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThanOrEqual,
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThan,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThanOrEqual,
        _ => op.clone(),
    }
}

fn union_optional_domains(a: Option<Domain>, b: Option<Domain>) -> Option<Domain> {
    match (a, b) {
        (None, None) => None,
        (Some(d), None) | (None, Some(d)) => Some(d),
        (Some(a), Some(b)) => Some(normalize_domain(Domain::Union(vec![a, b]))),
    }
}

fn lit_cmp(a: &LiteralValue, b: &LiteralValue) -> i8 {
    if let OperationResult::Value(LiteralValue::Boolean(BooleanValue::True)) =
        comparison_operation(a, &ComparisonComputation::LessThan, b)
    {
        return -1;
    }
    if let OperationResult::Value(LiteralValue::Boolean(BooleanValue::True)) =
        comparison_operation(a, &ComparisonComputation::Equal, b)
    {
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

fn is_empty_domain(domain: &Domain) -> bool {
    match domain {
        Domain::Enumeration(vals) => vals.is_empty(),
        Domain::Range { min, max } => bounds_contradict(min, max),
        Domain::Union(parts) => parts.is_empty() || parts.iter().all(is_empty_domain),
        Domain::Complement(_) => false,
        Domain::Unconstrained => false,
    }
}

fn domain_intersection(a: Domain, b: Domain) -> Option<Domain> {
    let result = match (a, b) {
        (Domain::Unconstrained, d) | (d, Domain::Unconstrained) => Some(d),
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
            let min = match (min1, min2) {
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
            };
            let max = match (max1, max2) {
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
            };
            if bounds_contradict(&min, &max) {
                None
            } else {
                Some(Domain::Range { min, max })
            }
        }
        (Domain::Enumeration(mut v1), Domain::Enumeration(v2)) => {
            v1.retain(|x| v2.contains(x));
            if v1.is_empty() {
                None
            } else {
                Some(Domain::Enumeration(v1))
            }
        }
        (Domain::Enumeration(vs), Domain::Range { min, max })
        | (Domain::Range { min, max }, Domain::Enumeration(vs)) => {
            let mut kept = Vec::new();
            for v in vs {
                if value_within(&v, &min, &max) {
                    kept.push(v);
                }
            }
            if kept.is_empty() {
                None
            } else {
                Some(Domain::Enumeration(kept))
            }
        }
        (Domain::Enumeration(vs), Domain::Complement(inner))
        | (Domain::Complement(inner), Domain::Enumeration(vs)) => {
            // Intersection: Enumeration ∩ Complement(Enumeration) = values in first but not in second
            match *inner.clone() {
                Domain::Enumeration(excluded) => {
                    let mut kept = Vec::new();
                    for v in vs {
                        if !excluded.contains(&v) {
                            kept.push(v);
                        }
                    }
                    if kept.is_empty() {
                        None
                    } else {
                        Some(Domain::Enumeration(kept))
                    }
                }
                _ => {
                    // For other Complement types, we can't easily compute intersection
                    // Return None to indicate we can't handle this case
                    None
                }
            }
        }
        (Domain::Union(v1), Domain::Union(v2)) => {
            let mut acc: Vec<Domain> = Vec::new();
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
                Some(Domain::Union(acc))
            }
        }
        (Domain::Union(vs), d) | (d, Domain::Union(vs)) => {
            let mut acc: Vec<Domain> = Vec::new();
            for a in vs.into_iter() {
                if let Some(ix) = domain_intersection(a, d.clone()) {
                    acc.push(ix);
                }
            }
            if acc.is_empty() {
                None
            } else {
                Some(Domain::Union(acc))
            }
        }
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

fn normalize_domain(d: Domain) -> Domain {
    match d {
        Domain::Complement(inner) => {
            let normalized_inner = normalize_domain(*inner);
            match normalized_inner {
                Domain::Complement(double_inner) => *double_inner,
                Domain::Range { min, max } => Domain::Range {
                    min: invert_bound(max),
                    max: invert_bound(min),
                },
                Domain::Enumeration(vals) => {
                    Domain::Complement(Box::new(Domain::Enumeration(vals)))
                }
                Domain::Unconstrained => Domain::Enumeration(vec![]),
                Domain::Union(parts) => Domain::Complement(Box::new(Domain::Union(parts))),
            }
        }
        Domain::Union(mut parts) => {
            let mut flat: Vec<Domain> = Vec::new();
            for p in parts.drain(..) {
                let normalized = normalize_domain(p);
                match normalized {
                    Domain::Union(inner) => flat.extend(inner),
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
                    Domain::Enumeration(vals) => all_enum_values.extend(vals),
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
                result.push(Domain::Enumeration(all_enum_values));
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
                Domain::Enumeration(vec![])
            } else if result.len() == 1 {
                result.remove(0)
            } else {
                Domain::Union(result)
            }
        }
        Domain::Enumeration(mut values) => {
            values.sort_by(|a, b| match lit_cmp(a, b) {
                -1 => Ordering::Less,
                0 => Ordering::Equal,
                _ => Ordering::Greater,
            });
            values.dedup();
            Domain::Enumeration(values)
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

    #[test]
    fn normalize_double_complement() {
        let inner = Domain::Enumeration(vec![num(5)]);
        let double = Domain::Complement(Box::new(Domain::Complement(Box::new(inner.clone()))));
        let normalized = normalize_domain(double);
        assert_eq!(normalized, inner);
    }

    #[test]
    fn normalize_union_absorbs_unconstrained() {
        let union = Domain::Union(vec![
            Domain::Range {
                min: Bound::Inclusive(num(0)),
                max: Bound::Inclusive(num(10)),
            },
            Domain::Unconstrained,
        ]);
        let normalized = normalize_domain(union);
        assert_eq!(normalized, Domain::Unconstrained);
    }

    #[test]
    fn normalize_union_removes_empty_enumerations() {
        let union = Domain::Union(vec![
            Domain::Enumeration(vec![]),
            Domain::Enumeration(vec![num(5)]),
        ]);
        let normalized = normalize_domain(union);
        assert_eq!(normalized, Domain::Enumeration(vec![num(5)]));
    }

    #[test]
    fn normalize_union_merges_enumerations() {
        let union = Domain::Union(vec![
            Domain::Enumeration(vec![num(1), num(3)]),
            Domain::Enumeration(vec![num(2), num(3)]),
        ]);
        let normalized = normalize_domain(union);
        assert_eq!(
            normalized,
            Domain::Enumeration(vec![num(1), num(2), num(3)])
        );
    }

    #[test]
    fn normalize_union_absorbs_enum_values_in_ranges() {
        let union = Domain::Union(vec![
            Domain::Range {
                min: Bound::Inclusive(num(0)),
                max: Bound::Inclusive(num(10)),
            },
            Domain::Enumeration(vec![num(5), num(15)]),
        ]);
        let normalized = normalize_domain(union);
        match normalized {
            Domain::Union(parts) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(&parts[0], Domain::Range { .. }));
                if let Domain::Enumeration(vals) = &parts[1] {
                    assert_eq!(vals.len(), 1);
                    assert_eq!(vals[0], num(15));
                } else {
                    panic!("Expected enumeration");
                }
            }
            _ => panic!("Expected union"),
        }
    }

    #[test]
    fn normalize_merges_adjacent_ranges() {
        let union = Domain::Union(vec![
            Domain::Range {
                min: Bound::Inclusive(num(0)),
                max: Bound::Inclusive(num(10)),
            },
            Domain::Range {
                min: Bound::Inclusive(num(10)),
                max: Bound::Inclusive(num(20)),
            },
        ]);
        let normalized = normalize_domain(union);
        match normalized {
            Domain::Range { min, max } => {
                assert_eq!(min, Bound::Inclusive(num(0)));
                assert_eq!(max, Bound::Inclusive(num(20)));
            }
            _ => panic!("Expected single merged range, got {:?}", normalized),
        }
    }

    #[test]
    fn intersection_normalizes_result() {
        let a = Domain::Union(vec![
            Domain::Enumeration(vec![num(1)]),
            Domain::Enumeration(vec![num(2)]),
        ]);
        let b = Domain::Unconstrained;
        let result = domain_intersection(a, b);
        match result {
            Some(Domain::Enumeration(vals)) => {
                assert_eq!(vals, vec![num(1), num(2)]);
            }
            other => panic!("Expected merged enumeration, got {:?}", other),
        }
    }

    #[test]
    fn normalize_complement_of_range() {
        let complement = Domain::Complement(Box::new(Domain::Range {
            min: Bound::Exclusive(num(100)),
            max: Bound::Unbounded,
        }));
        let normalized = normalize_domain(complement);
        match normalized {
            Domain::Range { min, max } => {
                assert_eq!(min, Bound::Unbounded);
                assert_eq!(max, Bound::Inclusive(num(100)));
            }
            other => panic!("Expected Range(-inf, 100], got {:?}", other),
        }
    }

    #[test]
    fn normalize_complement_of_range_inclusive() {
        let complement = Domain::Complement(Box::new(Domain::Range {
            min: Bound::Inclusive(num(100)),
            max: Bound::Inclusive(num(200)),
        }));
        let normalized = normalize_domain(complement);
        match normalized {
            Domain::Range { min, max } => {
                assert_eq!(min, Bound::Exclusive(num(200)));
                assert_eq!(max, Bound::Exclusive(num(100)));
            }
            other => panic!("Expected Range(200, 100), got {:?}", other),
        }
    }

    #[test]
    fn normalize_complement_of_unconstrained() {
        let complement = Domain::Complement(Box::new(Domain::Unconstrained));
        let normalized = normalize_domain(complement);
        assert_eq!(normalized, Domain::Enumeration(vec![]));
    }

    #[test]
    fn normalize_complement_of_enumeration() {
        let complement = Domain::Complement(Box::new(Domain::Enumeration(vec![num(5), num(10)])));
        let normalized = normalize_domain(complement);
        match normalized {
            Domain::Complement(inner) => {
                if let Domain::Enumeration(vals) = *inner {
                    assert_eq!(vals, vec![num(5), num(10)]);
                } else {
                    panic!("Expected Complement(Enumeration), got {:?}", inner);
                }
            }
            other => panic!("Expected Complement, got {:?}", other),
        }
    }
}
