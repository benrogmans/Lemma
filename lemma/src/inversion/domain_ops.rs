//! Domain operations for constraint solving

use crate::evaluator::operations::comparison_operation;
use crate::{Bound, ComparisonOperator, Domain, LiteralValue};
use std::cmp::Ordering;

pub fn lit_cmp(a: &LiteralValue, b: &LiteralValue) -> i8 {
    use ComparisonOperator as Cmp;
    if let Ok(true) = comparison_operation(a, &Cmp::LessThan, b) {
        return -1;
    }
    if let Ok(true) = comparison_operation(a, &Cmp::Equal, b) {
        return 0;
    }
    1
}

pub fn value_within(v: &LiteralValue, min: &Bound, max: &Bound) -> bool {
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

pub fn bounds_contradict(min: &Bound, max: &Bound) -> bool {
    use Bound as B;
    match (min, max) {
        (B::Unbounded, _) | (_, B::Unbounded) => false,
        (B::Inclusive(a), B::Inclusive(b)) => lit_cmp(a, b) > 0,
        (B::Inclusive(a), B::Exclusive(b)) => lit_cmp(a, b) >= 0,
        (B::Exclusive(a), B::Inclusive(b)) => lit_cmp(a, b) >= 0,
        (B::Exclusive(a), B::Exclusive(b)) => lit_cmp(a, b) >= 0,
    }
}

pub fn domain_from_comparison(
    side: &str,
    op: &ComparisonOperator,
    val: LiteralValue,
) -> Option<Domain> {
    use ComparisonOperator as Cmp;
    let make_range = |min: Bound, max: Bound| Domain::Range { min, max };
    let fact_on_right = side == "right";

    match op {
        Cmp::LessThan => {
            if fact_on_right {
                Some(make_range(Bound::Exclusive(val), Bound::Unbounded))
            } else {
                Some(make_range(Bound::Unbounded, Bound::Exclusive(val)))
            }
        }
        Cmp::LessThanOrEqual => {
            if fact_on_right {
                Some(make_range(Bound::Inclusive(val), Bound::Unbounded))
            } else {
                Some(make_range(Bound::Unbounded, Bound::Inclusive(val)))
            }
        }
        Cmp::GreaterThan => {
            if fact_on_right {
                Some(make_range(Bound::Unbounded, Bound::Exclusive(val)))
            } else {
                Some(make_range(Bound::Exclusive(val), Bound::Unbounded))
            }
        }
        Cmp::GreaterThanOrEqual => {
            if fact_on_right {
                Some(make_range(Bound::Unbounded, Bound::Inclusive(val)))
            } else {
                Some(make_range(Bound::Inclusive(val), Bound::Unbounded))
            }
        }
        Cmp::Equal => Some(Domain::Enumeration(vec![val])),
        Cmp::NotEqual => Some(Domain::Complement(Box::new(Domain::Enumeration(vec![val])))),
        _ => None,
    }
}

pub fn domain_union(a: Domain, b: Domain) -> Domain {
    match (a, b) {
        (Domain::Union(mut v1), Domain::Union(v2)) => {
            v1.extend(v2);
            Domain::Union(v1)
        }
        (Domain::Union(mut v1), d2) => {
            v1.push(d2);
            Domain::Union(v1)
        }
        (d1, Domain::Union(mut v2)) => {
            v2.push(d1);
            Domain::Union(v2)
        }
        (d1, d2) => Domain::Union(vec![d1, d2]),
    }
}

pub fn domain_intersection(a: Domain, b: Domain) -> Option<Domain> {
    use Bound as B;
    use Domain as D;
    match (a, b) {
        (D::Unconstrained, d) | (d, D::Unconstrained) => Some(d),
        (
            D::Range {
                min: min1,
                max: max1,
            },
            D::Range {
                min: min2,
                max: max2,
            },
        ) => {
            let min = match (min1, min2) {
                (B::Unbounded, x) | (x, B::Unbounded) => x,
                (B::Inclusive(v1), B::Inclusive(v2)) => {
                    if lit_cmp(&v1, &v2) >= 0 {
                        B::Inclusive(v1)
                    } else {
                        B::Inclusive(v2)
                    }
                }
                (B::Inclusive(v1), B::Exclusive(v2)) => {
                    if lit_cmp(&v1, &v2) > 0 {
                        B::Inclusive(v1)
                    } else {
                        B::Exclusive(v2)
                    }
                }
                (B::Exclusive(v1), B::Inclusive(v2)) => {
                    if lit_cmp(&v1, &v2) > 0 {
                        B::Exclusive(v1)
                    } else {
                        B::Inclusive(v2)
                    }
                }
                (B::Exclusive(v1), B::Exclusive(v2)) => {
                    if lit_cmp(&v1, &v2) >= 0 {
                        B::Exclusive(v1)
                    } else {
                        B::Exclusive(v2)
                    }
                }
            };
            let max = match (max1, max2) {
                (B::Unbounded, x) | (x, B::Unbounded) => x,
                (B::Inclusive(v1), B::Inclusive(v2)) => {
                    if lit_cmp(&v1, &v2) <= 0 {
                        B::Inclusive(v1)
                    } else {
                        B::Inclusive(v2)
                    }
                }
                (B::Inclusive(v1), B::Exclusive(v2)) => {
                    if lit_cmp(&v1, &v2) < 0 {
                        B::Inclusive(v1)
                    } else {
                        B::Exclusive(v2)
                    }
                }
                (B::Exclusive(v1), B::Inclusive(v2)) => {
                    if lit_cmp(&v1, &v2) < 0 {
                        B::Exclusive(v1)
                    } else {
                        B::Inclusive(v2)
                    }
                }
                (B::Exclusive(v1), B::Exclusive(v2)) => {
                    if lit_cmp(&v1, &v2) <= 0 {
                        B::Exclusive(v1)
                    } else {
                        B::Exclusive(v2)
                    }
                }
            };
            if bounds_contradict(&min, &max) {
                None
            } else {
                Some(D::Range { min, max })
            }
        }
        (D::Enumeration(mut v1), D::Enumeration(v2)) => {
            v1.retain(|x| v2.contains(x));
            if v1.is_empty() {
                None
            } else {
                Some(D::Enumeration(v1))
            }
        }
        (D::Enumeration(vs), D::Range { min, max })
        | (D::Range { min, max }, D::Enumeration(vs)) => {
            let mut kept = Vec::new();
            for v in vs {
                if value_within(&v, &min, &max) {
                    kept.push(v);
                }
            }
            if kept.is_empty() {
                None
            } else {
                Some(D::Enumeration(kept))
            }
        }
        (D::Union(v1), D::Union(v2)) => {
            let mut acc: Vec<D> = Vec::new();
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
                Some(D::Union(acc))
            }
        }
        (D::Union(vs), d) | (d, D::Union(vs)) => {
            let mut acc: Vec<D> = Vec::new();
            for a in vs.into_iter() {
                if let Some(ix) = domain_intersection(a, d.clone()) {
                    acc.push(ix);
                }
            }
            if acc.is_empty() {
                None
            } else {
                Some(D::Union(acc))
            }
        }
        _ => None,
    }
}

pub fn negate_domain(d: Domain) -> Domain {
    use Bound as B;
    use Domain as D;
    match d {
        D::Unconstrained => D::Complement(Box::new(D::Unconstrained)),
        D::Complement(inner) => normalize_domain(*inner),
        D::Enumeration(vals) => D::Complement(Box::new(D::Enumeration(vals))),
        D::Range { min, max } => {
            let mut parts: Vec<D> = Vec::new();
            match min {
                B::Unbounded => {}
                B::Inclusive(v) => parts.push(D::Range {
                    min: B::Unbounded,
                    max: B::Exclusive(v),
                }),
                B::Exclusive(v) => parts.push(D::Range {
                    min: B::Unbounded,
                    max: B::Inclusive(v),
                }),
            }
            match max {
                B::Unbounded => {}
                B::Inclusive(v) => parts.push(D::Range {
                    min: B::Exclusive(v),
                    max: B::Unbounded,
                }),
                B::Exclusive(v) => parts.push(D::Range {
                    min: B::Inclusive(v),
                    max: B::Unbounded,
                }),
            }
            if parts.is_empty() {
                D::Unconstrained
            } else if parts.len() == 1 {
                parts.remove(0)
            } else {
                D::Union(parts)
            }
        }
        D::Union(parts) => {
            let mut acc = D::Unconstrained;
            for p in parts {
                let np = negate_domain(p);
                acc = match domain_intersection(acc, np) {
                    Some(ix) => ix,
                    None => return D::Complement(Box::new(D::Unconstrained)),
                };
            }
            acc
        }
    }
}

pub fn normalize_domain(d: Domain) -> Domain {
    use Domain as D;
    match d {
        D::Union(mut parts) => {
            let mut flat: Vec<D> = Vec::new();
            for p in parts.drain(..) {
                match p {
                    D::Union(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }

            flat.sort_by(|a, b| match (a, b) {
                (D::Enumeration(_), D::Enumeration(_)) => Ordering::Equal,
                (D::Enumeration(_), _) => Ordering::Less,
                (_, D::Enumeration(_)) => Ordering::Greater,
                (D::Range { .. }, D::Range { .. }) => Ordering::Equal,
                (D::Range { .. }, D::Complement(_)) => Ordering::Less,
                (D::Range { .. }, D::Unconstrained) => Ordering::Less,
                (D::Complement(_), D::Range { .. }) => Ordering::Greater,
                (D::Unconstrained, D::Range { .. }) => Ordering::Greater,
                _ => Ordering::Equal,
            });

            for domain in &mut flat {
                if let D::Enumeration(ref mut values) = domain {
                    values.sort_by(|a, b| match lit_cmp(a, b) {
                        -1 => Ordering::Less,
                        0 => Ordering::Equal,
                        _ => Ordering::Greater,
                    });
                    values.dedup();
                }
            }

            flat = merge_ranges(flat);

            if flat.is_empty() {
                D::Union(vec![])
            } else if flat.len() == 1 {
                flat.remove(0)
            } else {
                D::Union(flat)
            }
        }
        D::Enumeration(mut values) => {
            values.sort_by(|a, b| match lit_cmp(a, b) {
                -1 => Ordering::Less,
                0 => Ordering::Equal,
                _ => Ordering::Greater,
            });
            values.dedup();
            D::Enumeration(values)
        }
        other => other,
    }
}

fn merge_ranges(domains: Vec<Domain>) -> Vec<Domain> {
    use Bound as B;
    use Domain as D;

    let mut result = Vec::new();
    let mut ranges: Vec<(B, B)> = Vec::new();
    let mut others = Vec::new();

    for d in domains {
        match d {
            D::Range { min, max } => ranges.push((min, max)),
            other => others.push(other),
        }
    }

    if ranges.is_empty() {
        return others;
    }

    ranges.sort_by(|a, b| compare_bounds(&a.0, &b.0));

    let mut merged: Vec<(B, B)> = Vec::new();
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
        result.push(D::Range { min, max });
    }
    result.extend(others);

    result
}

fn compare_bounds(a: &Bound, b: &Bound) -> Ordering {
    use Bound as B;
    match (a, b) {
        (B::Unbounded, B::Unbounded) => Ordering::Equal,
        (B::Unbounded, _) => Ordering::Less,
        (_, B::Unbounded) => Ordering::Greater,
        (B::Inclusive(v1), B::Inclusive(v2)) | (B::Exclusive(v1), B::Exclusive(v2)) => {
            match lit_cmp(v1, v2) {
                -1 => Ordering::Less,
                0 => Ordering::Equal,
                _ => Ordering::Greater,
            }
        }
        (B::Inclusive(v1), B::Exclusive(v2)) | (B::Exclusive(v1), B::Inclusive(v2)) => {
            match lit_cmp(v1, v2) {
                -1 => Ordering::Less,
                0 => {
                    if matches!(a, B::Inclusive(_)) {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                }
                _ => Ordering::Greater,
            }
        }
    }
}

fn ranges_adjacent_or_overlap(r1: &(Bound, Bound), r2: &(Bound, Bound)) -> bool {
    use Bound as B;
    match (&r1.1, &r2.0) {
        (B::Unbounded, _) | (_, B::Unbounded) => true,
        (B::Inclusive(v1), B::Inclusive(v2)) | (B::Inclusive(v1), B::Exclusive(v2)) => {
            lit_cmp(v1, v2) >= 0
        }
        (B::Exclusive(v1), B::Inclusive(v2)) => lit_cmp(v1, v2) >= 0,
        (B::Exclusive(v1), B::Exclusive(v2)) => lit_cmp(v1, v2) > 0,
    }
}

fn min_bound(a: &Bound, b: &Bound) -> Bound {
    use Bound as B;
    match (a, b) {
        (B::Unbounded, _) | (_, B::Unbounded) => B::Unbounded,
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
    use Bound as B;
    match (a, b) {
        (B::Unbounded, _) | (_, B::Unbounded) => B::Unbounded,
        _ => {
            if matches!(compare_bounds(a, b), Ordering::Greater) {
                a.clone()
            } else {
                b.clone()
            }
        }
    }
}
