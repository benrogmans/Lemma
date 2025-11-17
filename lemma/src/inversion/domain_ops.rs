//! Domain operations for constraint solving

use crate::evaluator::operations::comparison_operation;
use crate::{Bound, ComparisonComputation, Domain, LiteralValue};
use std::cmp::Ordering;

pub fn lit_cmp(a: &LiteralValue, b: &LiteralValue) -> i8 {
    use ComparisonComputation;
    if let Ok(true) = comparison_operation(a, &ComparisonComputation::LessThan, b) {
        return -1;
    }
    if let Ok(true) = comparison_operation(a, &ComparisonComputation::Equal, b) {
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
    use Bound;
    match (min, max) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => false,
        (Bound::Inclusive(a), Bound::Inclusive(b)) => lit_cmp(a, b) > 0,
        (Bound::Inclusive(a), Bound::Exclusive(b)) => lit_cmp(a, b) >= 0,
        (Bound::Exclusive(a), Bound::Inclusive(b)) => lit_cmp(a, b) >= 0,
        (Bound::Exclusive(a), Bound::Exclusive(b)) => lit_cmp(a, b) >= 0,
    }
}

pub fn domain_intersection(a: Domain, b: Domain) -> Option<Domain> {
    use Bound;
    use Domain;
    match (a, b) {
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
    }
}

pub fn negate_domain(d: Domain) -> Domain {
    use Bound;
    use Domain;
    match d {
        Domain::Unconstrained => Domain::Complement(Box::new(Domain::Unconstrained)),
        Domain::Complement(inner) => normalize_domain(*inner),
        Domain::Enumeration(vals) => Domain::Complement(Box::new(Domain::Enumeration(vals))),
        Domain::Range { min, max } => {
            let mut parts: Vec<Domain> = Vec::new();
            match min {
                Bound::Unbounded => {}
                Bound::Inclusive(v) => parts.push(Domain::Range {
                    min: Bound::Unbounded,
                    max: Bound::Exclusive(v),
                }),
                Bound::Exclusive(v) => parts.push(Domain::Range {
                    min: Bound::Unbounded,
                    max: Bound::Inclusive(v),
                }),
            }
            match max {
                Bound::Unbounded => {}
                Bound::Inclusive(v) => parts.push(Domain::Range {
                    min: Bound::Exclusive(v),
                    max: Bound::Unbounded,
                }),
                Bound::Exclusive(v) => parts.push(Domain::Range {
                    min: Bound::Inclusive(v),
                    max: Bound::Unbounded,
                }),
            }
            if parts.is_empty() {
                Domain::Unconstrained
            } else if parts.len() == 1 {
                parts.remove(0)
            } else {
                Domain::Union(parts)
            }
        }
        Domain::Union(parts) => {
            let mut acc = Domain::Unconstrained;
            for p in parts {
                let np = negate_domain(p);
                acc = match domain_intersection(acc, np) {
                    Some(ix) => ix,
                    None => return Domain::Complement(Box::new(Domain::Unconstrained)),
                };
            }
            acc
        }
    }
}

pub fn normalize_domain(d: Domain) -> Domain {
    use Domain;
    match d {
        Domain::Union(mut parts) => {
            let mut flat: Vec<Domain> = Vec::new();
            for p in parts.drain(..) {
                match p {
                    Domain::Union(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }

            flat.sort_by(|a, b| match (a, b) {
                (Domain::Enumeration(_), Domain::Enumeration(_)) => Ordering::Equal,
                (Domain::Enumeration(_), _) => Ordering::Less,
                (_, Domain::Enumeration(_)) => Ordering::Greater,
                (Domain::Range { .. }, Domain::Range { .. }) => Ordering::Equal,
                (Domain::Range { .. }, Domain::Complement(_)) => Ordering::Less,
                (Domain::Range { .. }, Domain::Unconstrained) => Ordering::Less,
                (Domain::Complement(_), Domain::Range { .. }) => Ordering::Greater,
                (Domain::Unconstrained, Domain::Range { .. }) => Ordering::Greater,
                _ => Ordering::Equal,
            });

            for domain in &mut flat {
                if let Domain::Enumeration(ref mut values) = domain {
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
                Domain::Union(vec![])
            } else if flat.len() == 1 {
                flat.remove(0)
            } else {
                Domain::Union(flat)
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
    use Bound;
    use Domain;

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
    use Bound;
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
    use Bound;
    match (&r1.1, &r2.0) {
        (Bound::Unbounded, _) | (_, Bound::Unbounded) => true,
        (Bound::Inclusive(v1), Bound::Inclusive(v2))
        | (Bound::Inclusive(v1), Bound::Exclusive(v2)) => lit_cmp(v1, v2) >= 0,
        (Bound::Exclusive(v1), Bound::Inclusive(v2)) => lit_cmp(v1, v2) >= 0,
        (Bound::Exclusive(v1), Bound::Exclusive(v2)) => lit_cmp(v1, v2) > 0,
    }
}

fn min_bound(a: &Bound, b: &Bound) -> Bound {
    use Bound;
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
    use Bound;
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
