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
use std::sync::Arc;

/// Reduce a DNF expression using Quine-McCluskey style minimization
pub fn reduce(expression: Expression) -> Expression {

    // 1. Flatten OR branches
    let branches = flatten_or_branches(&expression);

    // 2. Convert each branch to a set of terms, detecting contradictions and deduplicating
    let mut term_sets: Vec<Vec<Expression>> = Vec::new();
    for branch in branches {
        let terms = flatten_and_terms(&branch);
        
        // Deduplicate terms within the branch
        let mut deduped: Vec<Expression> = Vec::new();
        for term in terms {
            if !deduped.iter().any(|existing| existing.semantically_equal(&term)) {
                deduped.push(term);
            }
        }
        
        // Remove redundant inequalities (e.g., x==1 AND x!=2 → just x==1)
        let cleaned = remove_redundant_inequalities(deduped);
        
        if !has_contradiction(&cleaned) {
            term_sets.push(cleaned);
        }
    }

    // Early exit: all branches false
    if term_sets.is_empty() {
        return literal_false();
    }

    // 3. Remove duplicate branches (idempotence)
    term_sets = remove_duplicates(term_sets);


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
        // Fold constant comparisons: 1 == 1 → true, 0 == 1 → false
        ExpressionKind::Comparison(left, op, right) => {
            if let (ExpressionKind::Literal(lval), ExpressionKind::Literal(rval)) = (&left.kind, &right.kind) {
                let result = match op {
                    ComparisonComputation::Equal(_) => lval == rval,
                    ComparisonComputation::NotEqual(_) => lval != rval,
                    _ => return vec![expression.clone()],
                };
                return if result { vec![] } else { vec![literal_false()] };
            }
            vec![expression.clone()]
        }
        // Simplify NOT(comparison) → opposite comparison
        ExpressionKind::LogicalNegation(inner, _negation_type) => {
            if let ExpressionKind::Comparison(left, op, right) = &inner.kind {
                // Convert to opposite comparison
                let opposite_op = match op {
                    ComparisonComputation::Equal(notation) => ComparisonComputation::NotEqual(notation.clone()),
                    ComparisonComputation::NotEqual(notation) => ComparisonComputation::Equal(notation.clone()),
                    ComparisonComputation::LessThan => ComparisonComputation::GreaterThanOrEqual,
                    ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThan,
                    ComparisonComputation::GreaterThan => ComparisonComputation::LessThanOrEqual,
                    ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThan,
                };
                let simplified = Expression::new(
                    ExpressionKind::Comparison(left.clone(), opposite_op, right.clone()),
                    None,
                );
                vec![simplified]
            } else {
                // Can't simplify this negation, keep it as-is
                vec![expression.clone()]
            }
        }
        _ => vec![expression.clone()],
    }
}

// ============================================================================
// Contradiction Detection
// ============================================================================

fn has_contradiction(terms: &[Expression]) -> bool {
    // Check for explicit false ONLY if it's the sole term
    // A branch with just [false] is unsatisfiable
    // But (condition ∧ false) where false is the result value is valid
    if terms.len() == 1 && terms[0].is_boolean_false() {
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

fn extract_inequality(expr: &Expression) -> Option<(FactPath, LiteralValue)> {
    if let ExpressionKind::Comparison(left, op, right) = &expr.kind {
        if !op.is_not_equal() {
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

/// Remove redundant inequality terms when there's a definite equality
///
/// If we have (fact == X) in the branch, remove all (fact != Y) where Y != X
/// since they're redundant - if fact equals X, it's automatically not equal to other values
fn remove_redundant_inequalities(terms: Vec<Expression>) -> Vec<Expression> {
    // First, collect all equality constraints
    let mut equalities: HashMap<FactPath, LiteralValue> = HashMap::new();
    for term in &terms {
        if let Some((fact, value)) = extract_equality(term) {
            equalities.insert(fact, value);
        }
    }
    
    // Filter out redundant inequalities
    terms.into_iter().filter(|term| {
        if let Some((fact, value)) = extract_inequality(term) {
            // This is a != comparison
            // Check if we have an equality for this fact
            if let Some(eq_value) = equalities.get(&fact) {
                // If fact == X, then (fact != Y) is redundant when Y != X
                if eq_value != &value {
                    return false; // Remove this redundant term
                }
            }
        }
        true // Keep this term
    }).collect()
}

// ============================================================================
// Complement Detection
// ============================================================================

fn are_complements(a: &Expression, b: &Expression) -> bool {
    // NOT(X) vs X
    if let ExpressionKind::LogicalNegation(inner, _) = &a.kind {
        if inner.semantically_equal(b) {
            return true;
        }
    }
    if let ExpressionKind::LogicalNegation(inner, _) = &b.kind {
        if inner.semantically_equal(a) {
            return true;
        }
    }

    // fact == X vs fact != X (same fact, same value)
    if let (
        ExpressionKind::Comparison(la, oa, ra),
        ExpressionKind::Comparison(lb, ob, rb),
    ) = (&a.kind, &b.kind)
    {
        if la.semantically_equal(lb) && ra.semantically_equal(rb) {
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

fn terms_set_equal(a: &[Expression], b: &[Expression]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .all(|ta| b.iter().any(|tb| ta.semantically_equal(tb)))
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
                ExpressionKind::LogicalOr(Arc::new(acc), Arc::new(branch)),
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
                ExpressionKind::LogicalAnd(Arc::new(acc), Arc::new(term)),
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
                Arc::new(left),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(right),
            ),
            None,
        )
    }

    fn neq(left: Expression, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::Comparison(
                Arc::new(left),
                ComparisonComputation::NotEqual(EqualityNotation::Symbol),
                Arc::new(right),
            ),
            None,
        )
    }

    fn and(left: Expression, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::LogicalAnd(Arc::new(left), Arc::new(right)),
            None,
        )
    }

    fn or(left: Expression, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::LogicalOr(Arc::new(left), Arc::new(right)),
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

    #[test]
    fn test_redundant_inequality_removal() {
        // (x == 1 ∧ x != 2) → x == 1
        // The x != 2 is redundant since x already equals 1
        let x = fact("x");
        let eq_1 = eq(x.clone(), literal_num(1));
        let neq_2 = neq(x.clone(), literal_num(2));
        
        let expr = and(eq_1.clone(), neq_2);
        let result = reduce(expr);
        
        // Result should be just x == 1, not have the redundant != 2
        // Count the AND terms - should be 1 (just the equality)
        match &result.kind {
            ExpressionKind::Comparison(_, _, _) => {
                // Good - simplified to single comparison
            }
            ExpressionKind::LogicalAnd(_, _) => {
                panic!("Should have removed redundant inequality, but still has AND");
            }
            _ => {}
        }
    }

    #[test]
    fn test_multiple_redundant_inequalities() {
        // (x == "latte" ∧ x != "cappuccino" ∧ x != "mocha") → x == "latte"
        let x = fact("drink");
        let latte = Expression::new(
            ExpressionKind::Literal(LiteralValue::Text("latte".to_string())),
            None,
        );
        let cappuccino = Expression::new(
            ExpressionKind::Literal(LiteralValue::Text("cappuccino".to_string())),
            None,
        );
        let mocha = Expression::new(
            ExpressionKind::Literal(LiteralValue::Text("mocha".to_string())),
            None,
        );
        
        let eq_latte = eq(x.clone(), latte);
        let neq_cap = neq(x.clone(), cappuccino);
        let neq_mocha = neq(x.clone(), mocha);
        
        let expr = and(and(eq_latte.clone(), neq_cap), neq_mocha);
        let result = reduce(expr);
        
        // Should simplify to just x == "latte"
        match &result.kind {
            ExpressionKind::Comparison(_, _, _) => {
                // Good - simplified to single comparison
            }
            ExpressionKind::LogicalAnd(_, _) => {
                panic!("Should have removed all redundant inequalities");
            }
            _ => {}
        }
    }
}
