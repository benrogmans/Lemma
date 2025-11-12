use crate::inversion::{Bound, Domain, Shape};
use crate::semantic::FactReference;
use crate::{
    ComparisonOperator, Expression, ExpressionKind, LemmaError, LemmaResult, LiteralValue,
};
use std::collections::HashMap;

/// Convert a Shape into concrete domains for each free variable
///
/// Each branch in the shape represents a different solution where the rule produces
/// the target outcome. We analyze the condition of each branch to extract value
/// constraints for each free variable.
///
/// For complex conditions that we can't fully analyze, we include the solution with
/// unconstrained domains for those variables.
pub fn shape_to_domains(shape: &Shape) -> LemmaResult<Vec<HashMap<FactReference, Domain>>> {
    let mut result = Vec::new();

    for branch in &shape.branches {
        // Check if the branch condition is literally false (unsatisfiable)
        if let ExpressionKind::Literal(LiteralValue::Boolean(false)) = &branch.condition.kind {
            // Skip this branch - it's impossible
            continue;
        }

        let mut domains = HashMap::new();

        // Extract constraints for each free variable from the branch condition
        for var in &shape.free_variables {
            let domain = extract_domain_for_variable(&branch.condition, var)?
                .unwrap_or(Domain::Unconstrained);
            domains.insert(var.clone(), domain);
        }

        result.push(domains);
    }

    // If all branches were unsatisfiable, there are no solutions
    if result.is_empty() {
        return Err(LemmaError::Engine(
            "No valid solutions: all constraints are unsatisfiable".to_string(),
        ));
    }

    Ok(result)
}

/// Extract domain constraints for a specific variable from a condition expression
fn extract_domain_for_variable(
    condition: &Expression,
    var: &FactReference,
) -> LemmaResult<Option<Domain>> {
    // TODO: This is where the heavy lifting happens
    // We need to analyze the condition and extract constraints

    match &condition.kind {
        // Boolean literal
        ExpressionKind::Literal(lit) => {
            if let LiteralValue::Boolean(true) = lit {
                // Condition is always true - no constraints
                Ok(None)
            } else {
                // Condition is always false - empty domain (no valid values)
                Ok(Some(Domain::Enumeration(vec![])))
            }
        }

        // Comparison: extract bounds if comparing the variable
        ExpressionKind::Comparison(lhs, op, rhs) => {
            extract_comparison_constraint(lhs, op, rhs, var)
        }

        // Logical AND: intersection of constraints
        ExpressionKind::LogicalAnd(lhs, rhs) => {
            let left_domain = extract_domain_for_variable(lhs, var)?;
            let right_domain = extract_domain_for_variable(rhs, var)?;
            Ok(intersect_domains(left_domain, right_domain))
        }

        // Logical OR: union of constraints
        ExpressionKind::LogicalOr(lhs, rhs) => {
            let left_domain = extract_domain_for_variable(lhs, var)?;
            let right_domain = extract_domain_for_variable(rhs, var)?;
            Ok(union_domains(left_domain, right_domain))
        }

        // Logical NOT: complement
        ExpressionKind::LogicalNegation(inner, _neg_type) => {
            if let Some(domain) = extract_domain_for_variable(inner, var)? {
                Ok(Some(Domain::Complement(Box::new(domain))))
            } else {
                Ok(None)
            }
        }

        // Other expressions: can't extract constraints
        _ => Ok(None),
    }
}

/// Extract domain constraint from a comparison expression
fn extract_comparison_constraint(
    lhs: &Expression,
    op: &ComparisonOperator,
    rhs: &Expression,
    var: &FactReference,
) -> LemmaResult<Option<Domain>> {
    // Check if LHS is the variable we're looking for
    let is_var_on_left = matches!(&lhs.kind, ExpressionKind::FactReference(fr)
        if &FactReference { reference: fr.reference.clone() } == var);

    // Check if RHS is the variable
    let is_var_on_right = matches!(&rhs.kind, ExpressionKind::FactReference(fr)
        if &FactReference { reference: fr.reference.clone() } == var);

    if is_var_on_left {
        // var OP value
        if let ExpressionKind::Literal(lit) = &rhs.kind {
            return Ok(Some(comparison_to_domain(op, lit, false)?));
        }
    } else if is_var_on_right {
        // value OP var (need to flip operator)
        if let ExpressionKind::Literal(lit) = &lhs.kind {
            return Ok(Some(comparison_to_domain(op, lit, true)?));
        }
    }

    // Not a simple comparison with the variable
    Ok(None)
}

/// Convert a comparison operator and literal to a domain constraint
fn comparison_to_domain(
    op: &ComparisonOperator,
    value: &LiteralValue,
    flipped: bool,
) -> LemmaResult<Domain> {
    // When flipped, we need to reverse the operator
    // e.g., "5 < x" becomes "x > 5"
    let effective_op = if flipped {
        flip_operator(op)
    } else {
        op.clone()
    };

    match effective_op {
        ComparisonOperator::Equal | ComparisonOperator::Is => {
            Ok(Domain::Enumeration(vec![value.clone()]))
        }
        ComparisonOperator::NotEqual => {
            Ok(Domain::Complement(Box::new(Domain::Enumeration(vec![
                value.clone(),
            ]))))
        }
        ComparisonOperator::LessThan => Ok(Domain::Range {
            min: Bound::Unbounded,
            max: Bound::Exclusive(value.clone()),
        }),
        ComparisonOperator::LessThanOrEqual => Ok(Domain::Range {
            min: Bound::Unbounded,
            max: Bound::Inclusive(value.clone()),
        }),
        ComparisonOperator::GreaterThan => Ok(Domain::Range {
            min: Bound::Exclusive(value.clone()),
            max: Bound::Unbounded,
        }),
        ComparisonOperator::GreaterThanOrEqual => Ok(Domain::Range {
            min: Bound::Inclusive(value.clone()),
            max: Bound::Unbounded,
        }),
        _ => Err(LemmaError::Engine(format!(
            "Unsupported comparison operator for domain extraction: {:?}",
            effective_op
        ))),
    }
}

/// Flip a comparison operator for when operands are reversed
fn flip_operator(op: &ComparisonOperator) -> ComparisonOperator {
    match op {
        ComparisonOperator::Equal => ComparisonOperator::Equal,
        ComparisonOperator::NotEqual => ComparisonOperator::NotEqual,
        ComparisonOperator::LessThan => ComparisonOperator::GreaterThan,
        ComparisonOperator::LessThanOrEqual => ComparisonOperator::GreaterThanOrEqual,
        ComparisonOperator::GreaterThan => ComparisonOperator::LessThan,
        ComparisonOperator::GreaterThanOrEqual => ComparisonOperator::LessThanOrEqual,
        _ => op.clone(),
    }
}

/// Intersect two optional domains
/// Uses De Morgan's law: A ∩ B = ¬(¬A ∪ ¬B)
fn intersect_domains(a: Option<Domain>, b: Option<Domain>) -> Option<Domain> {
    match (a, b) {
        (None, None) => None,
        (Some(d), None) | (None, Some(d)) => Some(d),
        (Some(a), Some(b)) => {
            // Intersection: complement of union of complements
            Some(Domain::Complement(Box::new(Domain::Union(vec![
                Domain::Complement(Box::new(a)),
                Domain::Complement(Box::new(b)),
            ]))))
        }
    }
}

/// Union two optional domains
fn union_domains(a: Option<Domain>, b: Option<Domain>) -> Option<Domain> {
    match (a, b) {
        (None, None) => None,
        (Some(d), None) | (None, Some(d)) => Some(d),
        (Some(a), Some(b)) => Some(Domain::Union(vec![a, b])),
    }
}
