//! Boolean expression simplification using Quine-McCluskey style minimization
//!
//! After expansion produces DNF, this module minimizes the expression:
//! - Fact contradiction detection: `fact == X ∧ fact == Y` → false
//! - Absorption: `A ∨ (A ∧ B)` → `A`
//! - Consensus elimination: `(A∧B) ∨ (¬A∧C) ∨ (B∧C)` → `(A∧B) ∨ (¬A∧C)`
//! - Term combination: branches differing in one complementary term merge

use crate::semantic::{
    BooleanValue, ComparisonComputation, Expression, ExpressionKind, FactPath, LiteralValue,
};
use std::collections::HashMap;

/// Reduce a DNF expression using Quine-McCluskey style minimization
pub fn reduce(expression: Expression) -> Expression {
    // 1. Flatten OR branches
    let branches = flatten_or_branches(&expression);

    // 2. Convert each branch to a set of terms, detecting contradictions
    let mut term_sets: Vec<Vec<Expression>> = Vec::new();
    for branch in branches {
        let terms = flatten_and_terms(&branch);
        if !has_contradiction(&terms) {
            term_sets.push(terms);
        }
    }

    // Early exit: all branches false
    if term_sets.is_empty() {
        return literal_false();
    }

    // 3. Remove duplicate branches (idempotence)
    term_sets = remove_duplicates(term_sets);

    // 4. Apply absorption: A ∨ (A ∧ B) → A
    term_sets = apply_absorption(term_sets);

    // 5. Apply term combination (QM core): (A ∧ B) ∨ (A ∧ ¬B) → A
    term_sets = apply_term_combination(term_sets);

    // 6. Apply consensus elimination: (A∧B) ∨ (¬A∧C) ∨ (B∧C) → (A∧B) ∨ (¬A∧C)
    term_sets = apply_consensus(term_sets);

    // 7. Check for tautology (empty branch = true)
    if term_sets.iter().any(|b| b.is_empty()) {
        return literal_true();
    }

    // 8. Rebuild expression
    rebuild_or_expression(term_sets)
}

// ============================================================================
// Flattening
// ============================================================================

fn flatten_or_branches(expression: &Expression) -> Vec<Expression> {
    match &expression.kind {
        ExpressionKind::LogicalOr(left, right) => {
            let mut branches = flatten_or_branches(left);
            branches.extend(flatten_or_branches(right));
            branches
        }
        _ => vec![expression.clone()],
    }
}

fn flatten_and_terms(expression: &Expression) -> Vec<Expression> {
    match &expression.kind {
        ExpressionKind::LogicalAnd(left, right) => {
            let mut terms = flatten_and_terms(left);
            terms.extend(flatten_and_terms(right));
            terms
        }
        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)) => vec![],
        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)) => {
            vec![literal_false()]
        }
        _ => vec![expression.clone()],
    }
}

// ============================================================================
// Contradiction Detection
// ============================================================================

fn has_contradiction(terms: &[Expression]) -> bool {
    // Check for explicit false
    if terms.iter().any(|t| t.is_boolean_false()) {
        return true;
    }

    // Check for fact == X ∧ fact == Y where X ≠ Y
    let mut fact_values: HashMap<FactPath, LiteralValue> = HashMap::new();
    for term in terms {
        if let Some((fact, value)) = extract_equality(term) {
            if let Some(existing) = fact_values.get(&fact) {
                if existing != &value {
                    return true;
                }
            } else {
                fact_values.insert(fact, value);
            }
        }
    }

    // Check for X ∧ ¬X
    for (i, term_a) in terms.iter().enumerate() {
        for term_b in terms.iter().skip(i + 1) {
            if are_complements(term_a, term_b) {
                return true;
            }
        }
    }

    false
}

fn extract_equality(expr: &Expression) -> Option<(FactPath, LiteralValue)> {
    if let ExpressionKind::Comparison(left, op, right) = &expr.kind {
        if !op.is_equal() {
            return None;
        }
        if let ExpressionKind::FactPath(fact) = &left.kind {
            if let ExpressionKind::Literal(value) = &right.kind {
                return Some((fact.clone(), value.clone()));
            }
        }
        if let ExpressionKind::Literal(value) = &left.kind {
            if let ExpressionKind::FactPath(fact) = &right.kind {
                return Some((fact.clone(), value.clone()));
            }
        }
    }
    None
}

// ============================================================================
// Complement Detection
// ============================================================================

fn are_complements(a: &Expression, b: &Expression) -> bool {
    // NOT(X) vs X
    if let ExpressionKind::LogicalNegation(inner, _) = &a.kind {
        if expressions_equal(inner, b) {
            return true;
        }
    }
    if let ExpressionKind::LogicalNegation(inner, _) = &b.kind {
        if expressions_equal(inner, a) {
            return true;
        }
    }

    // fact == X vs fact != X (same fact, same value)
    if let (
        ExpressionKind::Comparison(la, oa, ra),
        ExpressionKind::Comparison(lb, ob, rb),
    ) = (&a.kind, &b.kind)
    {
        if expressions_equal(la, lb) && expressions_equal(ra, rb) {
            return ops_are_complementary(oa, ob);
        }
    }

    false
}

fn ops_are_complementary(a: &ComparisonComputation, b: &ComparisonComputation) -> bool {
    matches!(
        (a, b),
        (ComparisonComputation::Equal(_), ComparisonComputation::NotEqual(_))
            | (ComparisonComputation::NotEqual(_), ComparisonComputation::Equal(_))
            | (ComparisonComputation::LessThan, ComparisonComputation::GreaterThanOrEqual)
            | (ComparisonComputation::GreaterThanOrEqual, ComparisonComputation::LessThan)
            | (ComparisonComputation::GreaterThan, ComparisonComputation::LessThanOrEqual)
            | (ComparisonComputation::LessThanOrEqual, ComparisonComputation::GreaterThan)
    )
}


// ============================================================================
// Expression Equality
// ============================================================================

fn expressions_equal(a: &Expression, b: &Expression) -> bool {
    match (&a.kind, &b.kind) {
        (ExpressionKind::Literal(la), ExpressionKind::Literal(lb)) => la == lb,
        (ExpressionKind::FactPath(fa), ExpressionKind::FactPath(fb)) => fa == fb,
        (
            ExpressionKind::Comparison(la, oa, ra),
            ExpressionKind::Comparison(lb, ob, rb),
        ) => oa == ob && expressions_equal(la, lb) && expressions_equal(ra, rb),
        (
            ExpressionKind::LogicalNegation(ia, _),
            ExpressionKind::LogicalNegation(ib, _),
        ) => expressions_equal(ia, ib),
        (
            ExpressionKind::LogicalAnd(la, ra),
            ExpressionKind::LogicalAnd(lb, rb),
        ) => expressions_equal(la, lb) && expressions_equal(ra, rb),
        (
            ExpressionKind::LogicalOr(la, ra),
            ExpressionKind::LogicalOr(lb, rb),
        ) => expressions_equal(la, lb) && expressions_equal(ra, rb),
        (
            ExpressionKind::Arithmetic(la, oa, ra),
            ExpressionKind::Arithmetic(lb, ob, rb),
        ) => oa == ob && expressions_equal(la, lb) && expressions_equal(ra, rb),
        _ => false,
    }
}

fn terms_set_equal(a: &[Expression], b: &[Expression]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .all(|ta| b.iter().any(|tb| expressions_equal(ta, tb)))
}

// ============================================================================
// Idempotence (Duplicate Removal)
// ============================================================================

fn remove_duplicates(branches: Vec<Vec<Expression>>) -> Vec<Vec<Expression>> {
    let mut unique: Vec<Vec<Expression>> = Vec::new();
    for branch in branches {
        if !unique.iter().any(|existing| terms_set_equal(existing, &branch)) {
            unique.push(branch);
        }
    }
    unique
}

// ============================================================================
// Absorption: A ∨ (A ∧ B) → A
// ============================================================================

fn apply_absorption(branches: Vec<Vec<Expression>>) -> Vec<Vec<Expression>> {
    let mut result: Vec<Vec<Expression>> = Vec::new();

    for (i, branch) in branches.iter().enumerate() {
        let absorbed = branches.iter().enumerate().any(|(j, other)| {
            i != j && is_subset(other, branch)
        });
        if !absorbed {
            result.push(branch.clone());
        }
    }

    result
}

fn is_subset(smaller: &[Expression], larger: &[Expression]) -> bool {
    if smaller.len() >= larger.len() {
        return false;
    }
    smaller
        .iter()
        .all(|term| larger.iter().any(|other| expressions_equal(term, other)))
}

// ============================================================================
// QM Term Combination: (A ∧ B) ∨ (A ∧ ¬B) → A
// ============================================================================

fn apply_term_combination(mut branches: Vec<Vec<Expression>>) -> Vec<Vec<Expression>> {
    let mut changed = true;

    while changed {
        changed = false;
        let mut new_branches: Vec<Vec<Expression>> = Vec::new();
        let mut used: Vec<bool> = vec![false; branches.len()];

        for i in 0..branches.len() {
            if used[i] {
                continue;
            }

            let mut combined = false;
            for j in (i + 1)..branches.len() {
                if used[j] {
                    continue;
                }

                if let Some(merged) = try_combine(&branches[i], &branches[j]) {
                    new_branches.push(merged);
                    used[i] = true;
                    used[j] = true;
                    combined = true;
                    changed = true;
                    break;
                }
            }

            if !combined && !used[i] {
                new_branches.push(branches[i].clone());
            }
        }

        // Add any remaining unused branches
        for (i, branch) in branches.iter().enumerate() {
            if !used[i] && !new_branches.iter().any(|b| terms_set_equal(b, branch)) {
                new_branches.push(branch.clone());
            }
        }

        branches = remove_duplicates(new_branches);
    }

    branches
}

/// Try to combine two branches that differ in exactly one complementary term
/// (A ∧ B ∧ X) ∨ (A ∧ B ∧ ¬X) → (A ∧ B)
fn try_combine(branch_a: &[Expression], branch_b: &[Expression]) -> Option<Vec<Expression>> {
    if branch_a.len() != branch_b.len() {
        return None;
    }

    // Find terms in A not in B, and terms in B not in A
    let mut only_in_a: Vec<&Expression> = Vec::new();
    let mut only_in_b: Vec<&Expression> = Vec::new();

    for term in branch_a {
        if !branch_b.iter().any(|t| expressions_equal(term, t)) {
            only_in_a.push(term);
        }
    }

    for term in branch_b {
        if !branch_a.iter().any(|t| expressions_equal(term, t)) {
            only_in_b.push(term);
        }
    }

    // Must differ in exactly one term each
    if only_in_a.len() != 1 || only_in_b.len() != 1 {
        return None;
    }

    // The differing terms must be complements
    if !are_complements(only_in_a[0], only_in_b[0]) {
        return None;
    }

    // Combine: remove the complementary term
    let combined: Vec<Expression> = branch_a
        .iter()
        .filter(|t| !expressions_equal(t, only_in_a[0]))
        .cloned()
        .collect();

    Some(combined)
}

// ============================================================================
// Consensus Elimination: (A∧B) ∨ (¬A∧C) ∨ (B∧C) → (A∧B) ∨ (¬A∧C)
// ============================================================================

fn apply_consensus(mut branches: Vec<Vec<Expression>>) -> Vec<Vec<Expression>> {
    let mut changed = true;

    while changed {
        changed = false;
        let mut to_remove: Vec<usize> = Vec::new();

        for k in 0..branches.len() {
            if to_remove.contains(&k) {
                continue;
            }

            for i in 0..branches.len() {
                if i == k || to_remove.contains(&i) {
                    continue;
                }

                for j in (i + 1)..branches.len() {
                    if j == k || to_remove.contains(&j) {
                        continue;
                    }

                    if is_consensus_term(&branches[i], &branches[j], &branches[k]) {
                        to_remove.push(k);
                        changed = true;
                        break;
                    }
                }

                if to_remove.contains(&k) {
                    break;
                }
            }
        }

        // Remove in reverse order to preserve indices
        to_remove.sort();
        to_remove.reverse();
        for idx in to_remove {
            branches.remove(idx);
        }
    }

    branches
}

/// Check if branch_c is the consensus term of branch_a and branch_b
/// branch_a = (X ∧ rest_a), branch_b = (¬X ∧ rest_b)
/// consensus = (rest_a ∧ rest_b)
fn is_consensus_term(
    branch_a: &[Expression],
    branch_b: &[Expression],
    branch_c: &[Expression],
) -> bool {
    for term_a in branch_a {
        for term_b in branch_b {
            if are_complements(term_a, term_b) {
                // Found X in a and ¬X in b
                let rest_a: Vec<&Expression> = branch_a
                    .iter()
                    .filter(|t| !expressions_equal(t, term_a))
                    .collect();
                let rest_b: Vec<&Expression> = branch_b
                    .iter()
                    .filter(|t| !expressions_equal(t, term_b))
                    .collect();

                // Expected consensus = rest_a ∪ rest_b
                let expected_len = rest_a.len() + rest_b.len();

                if branch_c.len() != expected_len {
                    continue;
                }

                // Check if branch_c contains exactly rest_a ∪ rest_b
                let all_rest_a_in_c = rest_a
                    .iter()
                    .all(|t| branch_c.iter().any(|c| expressions_equal(t, c)));
                let all_rest_b_in_c = rest_b
                    .iter()
                    .all(|t| branch_c.iter().any(|c| expressions_equal(t, c)));

                if all_rest_a_in_c && all_rest_b_in_c {
                    return true;
                }
            }
        }
    }
    false
}

// ============================================================================
// Rebuilding
// ============================================================================

fn rebuild_or_expression(branches: Vec<Vec<Expression>>) -> Expression {
    if branches.is_empty() {
        return literal_false();
    }

    let rebuilt: Vec<Expression> = branches.into_iter().map(rebuild_and_expression).collect();

    rebuilt
        .into_iter()
        .reduce(|acc, branch| {
            Expression::new(
                ExpressionKind::LogicalOr(Box::new(acc), Box::new(branch)),
                None,
            )
        })
        .unwrap_or_else(literal_false)
}

fn rebuild_and_expression(terms: Vec<Expression>) -> Expression {
    if terms.is_empty() {
        return literal_true();
    }

    terms
        .into_iter()
        .reduce(|acc, term| {
            Expression::new(
                ExpressionKind::LogicalAnd(Box::new(acc), Box::new(term)),
                None,
            )
        })
        .unwrap_or_else(literal_true)
}

fn literal_true() -> Expression {
    Expression::new(
        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
        None,
    )
}

fn literal_false() -> Expression {
    Expression::new(
        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
        None,
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::{EqualityNotation, FactPath};
    use rust_decimal::Decimal;

    fn fact(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::FactPath(FactPath::local(name.to_string())),
            None,
        )
    }

    fn literal_num(n: i64) -> Expression {
        Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(n))),
            None,
        )
    }

    fn eq(left: Expression, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::Comparison(
                Box::new(left),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Box::new(right),
            ),
            None,
        )
    }

    fn neq(left: Expression, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::Comparison(
                Box::new(left),
                ComparisonComputation::NotEqual(EqualityNotation::Symbol),
                Box::new(right),
            ),
            None,
        )
    }

    fn and(left: Expression, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::LogicalAnd(Box::new(left), Box::new(right)),
            None,
        )
    }

    fn or(left: Expression, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::LogicalOr(Box::new(left), Box::new(right)),
            None,
        )
    }

    fn count_or_branches(expr: &Expression) -> usize {
        match &expr.kind {
            ExpressionKind::LogicalOr(left, right) => {
                count_or_branches(left) + count_or_branches(right)
            }
            _ => 1,
        }
    }

    #[test]
    fn test_fact_contradiction() {
        // (x == 1 ∧ x == 2) → false
        let x = fact("x");
        let expr = and(eq(x.clone(), literal_num(1)), eq(x.clone(), literal_num(2)));

        let result = reduce(expr);
        assert!(result.is_boolean_false());
    }

    #[test]
    fn test_absorption() {
        // A ∨ (A ∧ B) → A
        let a = eq(fact("x"), literal_num(1));
        let b = eq(fact("y"), literal_num(2));
        let expr = or(a.clone(), and(a.clone(), b));

        let result = reduce(expr);
        assert_eq!(count_or_branches(&result), 1);
    }

    #[test]
    fn test_term_combination() {
        // (A ∧ B) ∨ (A ∧ ¬B) → A
        let a = eq(fact("x"), literal_num(1));
        let b = eq(fact("y"), literal_num(2));
        let not_b = neq(fact("y"), literal_num(2));

        let expr = or(and(a.clone(), b), and(a.clone(), not_b));

        let result = reduce(expr);
        assert_eq!(count_or_branches(&result), 1);
    }

    #[test]
    fn test_consensus_elimination() {
        // (A ∧ B) ∨ (¬A ∧ C) ∨ (B ∧ C) → (A ∧ B) ∨ (¬A ∧ C)
        let a = eq(fact("x"), literal_num(1));
        let not_a = neq(fact("x"), literal_num(1));
        let b = eq(fact("y"), literal_num(2));
        let c = eq(fact("z"), literal_num(3));

        let branch1 = and(a.clone(), b.clone());
        let branch2 = and(not_a.clone(), c.clone());
        let branch3 = and(b.clone(), c.clone());

        let expr = or(or(branch1, branch2), branch3);

        let result = reduce(expr);
        assert_eq!(
            count_or_branches(&result),
            2,
            "consensus should reduce 3 branches to 2"
        );
    }

    #[test]
    fn test_idempotence() {
        // A ∨ A → A
        let a = eq(fact("x"), literal_num(1));
        let expr = or(a.clone(), a.clone());

        let result = reduce(expr);
        assert_eq!(count_or_branches(&result), 1);
    }

    #[test]
    fn test_x_and_not_x_contradiction() {
        // (x == 1 ∧ x != 1) → false
        let x = fact("x");
        let expr = and(eq(x.clone(), literal_num(1)), neq(x.clone(), literal_num(1)));

        let result = reduce(expr);
        assert!(result.is_boolean_false());
    }

    #[test]
    fn test_false_branch_eliminated() {
        // false ∨ A → A
        let a = eq(fact("x"), literal_num(1));
        let expr = or(literal_false(), a.clone());

        let result = reduce(expr);
        assert!(!result.is_boolean_false());
        assert_eq!(count_or_branches(&result), 1);
    }

    #[test]
    fn test_complex_combination() {
        // (A ∧ B ∧ C) ∨ (A ∧ B ∧ ¬C) → (A ∧ B)
        let a = eq(fact("x"), literal_num(1));
        let b = eq(fact("y"), literal_num(2));
        let c = eq(fact("z"), literal_num(3));
        let not_c = neq(fact("z"), literal_num(3));

        let branch1 = and(and(a.clone(), b.clone()), c);
        let branch2 = and(and(a.clone(), b.clone()), not_c);

        let expr = or(branch1, branch2);

        let result = reduce(expr);
        assert_eq!(
            count_or_branches(&result),
            1,
            "should combine to single branch"
        );
    }
}
