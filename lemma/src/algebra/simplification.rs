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
            if !deduped
                .iter()
                .any(|existing| existing.semantically_equal(&term))
            {
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

    // 4. Apply absorption: A ∨ (A ∧ B) → A
    term_sets = apply_absorption(term_sets);

    // 5. Apply term combination: (A ∧ B) ∨ (A ∧ ¬B) → A
    term_sets = apply_term_combination(term_sets);

    // 6. Apply consensus elimination: (A ∧ B) ∨ (¬A ∧ C) ∨ (B ∧ C) → (A ∧ B) ∨ (¬A ∧ C)
    term_sets = apply_consensus_elimination(term_sets);

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
            if let (ExpressionKind::Literal(lval), ExpressionKind::Literal(rval)) =
                (&left.kind, &right.kind)
            {
                let result = match op {
                    ComparisonComputation::Equal(_) => lval == rval,
                    ComparisonComputation::NotEqual(_) => lval != rval,
                    _ => return vec![expression.clone()],
                };
                return if result {
                    vec![]
                } else {
                    vec![literal_false()]
                };
            }
            vec![expression.clone()]
        }
        // Simplify NOT(comparison) → opposite comparison
        ExpressionKind::LogicalNegation(inner, _negation_type) => {
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
    terms
        .into_iter()
        .filter(|term| {
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
        })
        .collect()
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
    if let (ExpressionKind::Comparison(la, oa, ra), ExpressionKind::Comparison(lb, ob, rb)) =
        (&a.kind, &b.kind)
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
        (
            ComparisonComputation::Equal(_),
            ComparisonComputation::NotEqual(_)
        ) | (
            ComparisonComputation::NotEqual(_),
            ComparisonComputation::Equal(_)
        ) | (
            ComparisonComputation::LessThan,
            ComparisonComputation::GreaterThanOrEqual
        ) | (
            ComparisonComputation::GreaterThanOrEqual,
            ComparisonComputation::LessThan
        ) | (
            ComparisonComputation::GreaterThan,
            ComparisonComputation::LessThanOrEqual
        ) | (
            ComparisonComputation::LessThanOrEqual,
            ComparisonComputation::GreaterThan
        )
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
        if !unique
            .iter()
            .any(|existing| terms_set_equal(existing, &branch))
        {
            unique.push(branch);
        }
    }
    unique
}

// ============================================================================
// Absorption: A ∨ (A ∧ B) → A
// ============================================================================

/// Apply absorption: if one branch is a subset of another, remove the larger one
fn apply_absorption(branches: Vec<Vec<Expression>>) -> Vec<Vec<Expression>> {
    let mut result: Vec<Vec<Expression>> = Vec::new();

    for branch in branches {
        // Check if this branch is absorbed by any existing branch
        let is_absorbed = result.iter().any(|existing| {
            // If existing is a subset of branch, branch absorbs existing
            // If branch is a subset of existing, existing absorbs branch
            terms_set_is_subset(existing, &branch) || terms_set_is_subset(&branch, existing)
        });

        if !is_absorbed {
            // Remove any existing branches that are absorbed by this one
            result.retain(|existing| !terms_set_is_subset(existing, &branch));
            result.push(branch);
        }
    }

    result
}

/// Check if term set A is a subset of term set B (all terms in A are in B)
fn terms_set_is_subset(a: &[Expression], b: &[Expression]) -> bool {
    a.iter()
        .all(|term_a| b.iter().any(|term_b| term_a.semantically_equal(term_b)))
}

// ============================================================================
// Term Combination: (A ∧ B) ∨ (A ∧ ¬B) → A
// ============================================================================

/// Apply term combination: if two branches differ only in complementary terms, merge them
fn apply_term_combination(branches: Vec<Vec<Expression>>) -> Vec<Vec<Expression>> {
    let mut result: Vec<Vec<Expression>> = Vec::new();
    let mut processed = vec![false; branches.len()];

    for i in 0..branches.len() {
        if processed[i] {
            continue;
        }

        let mut combined = branches[i].clone();
        let mut found_combination = false;

        // Look for branches that differ only in complementary terms
        for j in (i + 1)..branches.len() {
            if processed[j] {
                continue;
            }

            if let Some(common_terms) = try_combine_complementary(&branches[i], &branches[j]) {
                combined = common_terms;
                processed[j] = true;
                found_combination = true;
                break;
            }
        }

        if found_combination {
            // Recursively try to combine the merged result with other branches
            let mut remaining: Vec<Vec<Expression>> = branches
                .iter()
                .enumerate()
                .filter(|(idx, _)| !processed[*idx] && *idx != i)
                .map(|(_, branch)| branch.clone())
                .collect();
            remaining.insert(0, combined);
            return apply_term_combination(remaining);
        }

        result.push(branches[i].clone());
    }

    result
}

/// Try to combine two branches that differ only in complementary terms
/// Returns Some(common_terms) if they can be combined, None otherwise
fn try_combine_complementary(a: &[Expression], b: &[Expression]) -> Option<Vec<Expression>> {
    // Find terms that are in both branches (common terms)
    let mut common: Vec<Expression> = Vec::new();
    let mut only_in_a: Vec<Expression> = Vec::new();
    let mut only_in_b: Vec<Expression> = Vec::new();

    // Classify terms in a
    for term_a in a {
        if b.iter().any(|term_b| term_a.semantically_equal(term_b)) {
            common.push(term_a.clone());
        } else {
            only_in_a.push(term_a.clone());
        }
    }

    // Classify terms in b
    for term_b in b {
        if !a.iter().any(|term_a| term_a.semantically_equal(term_b)) {
            only_in_b.push(term_b.clone());
        }
    }

    // If they differ only in complementary terms, we can combine
    // Check if only_in_a and only_in_b are complementary pairs
    if only_in_a.len() == 1 && only_in_b.len() == 1 {
        if are_complements(&only_in_a[0], &only_in_b[0]) {
            return Some(common);
        }
    }

    None
}

// ============================================================================
// Consensus Elimination: (A ∧ B) ∨ (¬A ∧ C) ∨ (B ∧ C) → (A ∧ B) ∨ (¬A ∧ C)
// ============================================================================

/// Apply consensus elimination: remove redundant branches created by consensus
fn apply_consensus_elimination(branches: Vec<Vec<Expression>>) -> Vec<Vec<Expression>> {
    let mut result: Vec<Vec<Expression>> = Vec::new();

    for branch in branches {
        // Check if this branch is redundant due to consensus with two other branches
        let is_redundant = is_consensus_redundant(&branch, &result);

        if !is_redundant {
            result.push(branch);
        }
    }

    result
}

/// Check if a branch is redundant due to consensus
/// A branch (B ∧ C) is redundant if there exist branches (A ∧ B) and (¬A ∧ C)
fn is_consensus_redundant(branch: &[Expression], all_branches: &[Vec<Expression>]) -> bool {
    // For consensus elimination, we need:
    // - Two branches that have complementary terms
    // - A third branch that contains the consensus terms

    // Check if branch is the consensus of two other branches
    for i in 0..all_branches.len() {
        for j in (i + 1)..all_branches.len() {
            let branch1 = &all_branches[i];
            let branch2 = &all_branches[j];

            // Find complementary terms between branch1 and branch2
            if let Some((complementary_term1, complementary_term2, consensus_terms)) =
                find_consensus_terms(branch1, branch2)
            {
                // Check if branch contains exactly the consensus terms
                // The consensus is redundant if it appears as a separate branch
                if terms_set_equal(branch, &consensus_terms) {
                    // Verify that branch1 has the complementary term and branch2 has its complement
                    let branch1_has_complement = branch1
                        .iter()
                        .any(|t| t.semantically_equal(&complementary_term1));
                    let branch2_has_complement = branch2
                        .iter()
                        .any(|t| t.semantically_equal(&complementary_term2));

                    if branch1_has_complement && branch2_has_complement {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Find consensus terms between two branches with complementary terms
/// Returns (complementary_term1, complementary_term2, consensus_terms) if found
/// Consensus is the union of all non-complementary terms from both branches
fn find_consensus_terms(
    branch1: &[Expression],
    branch2: &[Expression],
) -> Option<(Expression, Expression, Vec<Expression>)> {
    // Find complementary terms (one in branch1, complement in branch2)
    for term1 in branch1 {
        for term2 in branch2 {
            if are_complements(term1, term2) {
                // Found complementary pair - compute consensus
                // Consensus = all terms from both branches except the complementary pair
                let mut consensus: Vec<Expression> = Vec::new();

                // Add all terms from branch1 except term1
                for t in branch1 {
                    if !t.semantically_equal(term1) {
                        consensus.push(t.clone());
                    }
                }

                // Add all terms from branch2 except term2
                for t in branch2 {
                    if !t.semantically_equal(term2) {
                        // Avoid duplicates
                        if !consensus.iter().any(|c| c.semantically_equal(t)) {
                            consensus.push(t.clone());
                        }
                    }
                }

                return Some((term1.clone(), term2.clone(), consensus));
            }
        }
    }

    None
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
        let expr = and(
            eq(x.clone(), literal_num(1)),
            neq(x.clone(), literal_num(1)),
        );

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
    fn test_discount_code_simplification_pattern() {
        // Pattern from bdd_partial_simplification test:
        // (discount_code is "SAVE30" and member_level is "platinum")
        // or (discount_code is "SAVE30" and not (member_level is "platinum"))
        // Should simplify to: discount_code is "SAVE30"
        use crate::semantic::LiteralValue;

        let discount_code = fact("discount_code");
        let member_level = fact("member_level");
        let save30 = Expression::new(
            ExpressionKind::Literal(LiteralValue::Text("SAVE30".to_string())),
            None,
        );
        let platinum = Expression::new(
            ExpressionKind::Literal(LiteralValue::Text("platinum".to_string())),
            None,
        );

        let a = eq(discount_code.clone(), save30.clone());
        let b = eq(member_level.clone(), platinum.clone());
        let not_b = neq(member_level.clone(), platinum.clone());

        let branch1 = and(a.clone(), b);
        let branch2 = and(a.clone(), not_b);

        let expr = or(branch1, branch2);
        let result = reduce(expr);

        // Should simplify to single branch with just discount_code constraint
        assert_eq!(
            count_or_branches(&result),
            1,
            "Pattern (A&B)|(A&!B) should simplify to A"
        );

        // Verify the result contains discount_code constraint but not member_level
        // by checking the structure - it should be a single comparison or AND with discount_code
        match &result.kind {
            ExpressionKind::Comparison(left, _, right) => {
                // Single comparison - check if it's discount_code
                assert!(
                    matches!(&left.kind, ExpressionKind::FactPath(f) if f.fact == "discount_code")
                        || matches!(&right.kind, ExpressionKind::FactPath(f) if f.fact == "discount_code"),
                    "Simplified result should be discount_code comparison"
                );
            }
            ExpressionKind::LogicalAnd(left, right) => {
                // AND expression - should only contain discount_code, not member_level
                let has_discount = matches!(&left.kind, ExpressionKind::Comparison(l, _, _) if matches!(&l.kind, ExpressionKind::FactPath(f) if f.fact == "discount_code"))
                    || matches!(&right.kind, ExpressionKind::Comparison(l, _, _) if matches!(&l.kind, ExpressionKind::FactPath(f) if f.fact == "discount_code"));
                let has_member = matches!(&left.kind, ExpressionKind::Comparison(l, _, _) if matches!(&l.kind, ExpressionKind::FactPath(f) if f.fact == "member_level"))
                    || matches!(&right.kind, ExpressionKind::Comparison(l, _, _) if matches!(&l.kind, ExpressionKind::FactPath(f) if f.fact == "member_level"));

                assert!(
                    has_discount,
                    "Simplified result should contain discount_code constraint"
                );
                assert!(
                    !has_member,
                    "Simplified result should NOT contain member_level constraint"
                );
            }
            _ => {
                panic!("Unexpected simplified result structure: {:?}", result.kind);
            }
        }
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
