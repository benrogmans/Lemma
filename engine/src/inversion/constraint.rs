//! Constraint type for inversion
//!
//! Represents boolean constraints over facts. Unlike `Expression`, this type:
//! - Does not require source location information
//! - Only represents the subset of expressions valid in constraints
//! - Makes invalid states unrepresentable
//!
//! Includes BDD-based simplification for contradiction detection.
//! For semantic analysis (e.g., `x == A and x != B`), use domain extraction.

use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, Expression, ExpressionKind, FactPath,
    LiteralValue, SemanticConversionTarget, ValueKind,
};
use crate::{LemmaError, LemmaResult, OperationResult};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::fmt;
use std::sync::Arc;

/// A boolean constraint over facts
///
/// Used internally by inversion to represent conditions under which
/// a solution applies. Converted from `Expression` at the boundary
/// when reading from the execution plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    /// Always true
    True,
    /// Always false (unsatisfiable)
    False,
    /// Comparison: fact op value (e.g., `age > 18`)
    Comparison {
        fact: FactPath,
        op: ComparisonComputation,
        value: Arc<LiteralValue>,
    },
    /// Boolean fact reference (e.g., `is_employee` meaning `is_employee == true`)
    Fact(FactPath),
    /// Logical AND of two constraints
    And(Box<Constraint>, Box<Constraint>),
    /// Logical OR of two constraints
    Or(Box<Constraint>, Box<Constraint>),
    /// Logical NOT of a constraint
    Not(Box<Constraint>),
}

impl Constraint {
    /// Check if this constraint is trivially true
    pub fn is_true(&self) -> bool {
        matches!(self, Constraint::True)
    }

    /// Check if this constraint is trivially false
    pub fn is_false(&self) -> bool {
        matches!(self, Constraint::False)
    }

    /// Combine two constraints with AND, applying short-circuit simplification
    pub fn and(self, other: Constraint) -> Constraint {
        if self.is_false() || other.is_false() {
            return Constraint::False;
        }
        if self.is_true() {
            return other;
        }
        if other.is_true() {
            return self;
        }
        Constraint::And(Box::new(self), Box::new(other))
    }

    /// Combine two constraints with OR, applying short-circuit simplification
    pub fn or(self, other: Constraint) -> Constraint {
        if self.is_true() || other.is_true() {
            return Constraint::True;
        }
        if self.is_false() {
            return other;
        }
        if other.is_false() {
            return self;
        }
        Constraint::Or(Box::new(self), Box::new(other))
    }

    /// Negate this constraint
    pub fn not(self) -> Constraint {
        match self {
            Constraint::True => Constraint::False,
            Constraint::False => Constraint::True,
            Constraint::Not(inner) => *inner,
            other => Constraint::Not(Box::new(other)),
        }
    }

    /// Simplify this constraint using BDD-based simplification
    ///
    /// This method:
    /// 1. Converts the constraint to a BDD expression
    /// 2. Simplifies using boolean algebra to detect contradictions
    ///
    /// The primary purpose is contradiction detection (returning `Constraint::False`).
    /// For actual output, use domains extracted from the constraint instead.
    pub fn simplify(self) -> LemmaResult<Constraint> {
        let mut atoms: Vec<Constraint> = Vec::new();
        if let Some(bexpr) = to_bool_expr(&self, &mut atoms) {
            const MAX_ATOMS: usize = 64;
            if atoms.len() <= MAX_ATOMS {
                // Inject numeric theory between comparison atoms on the same fact.
                // Without this, BDD simplification is boolean-only and can keep worlds like:
                //   (x > 5) AND NOT(x > 3)
                // which are propositionally satisfiable but numerically impossible.
                let theory = build_numeric_theory_closure(&atoms)?;
                let combined = boolean_expression::Expr::and(bexpr, theory);
                let simplified = combined.simplify_via_bdd();
                return Ok(from_bool_expr(&simplified, &atoms));
            }
        }

        Ok(self)
    }

    /// Convert from an Expression to a Constraint
    ///
    /// The expression must be a boolean expression containing only:
    /// - Comparisons between facts and literals
    /// - Boolean fact references
    /// - Logical operators (and, or, not)
    /// - Boolean literals
    pub fn from_expression(expr: &Expression) -> LemmaResult<Constraint> {
        use ExpressionKind;

        enum WorkItem {
            Process(usize),
            BuildAnd,
            BuildOr,
            ApplyNot,
        }

        let mut expr_pool: Vec<Expression> = Vec::new();
        let mut work_stack: Vec<WorkItem> = Vec::new();
        let mut constraint_stack: Vec<Constraint> = Vec::new();

        let root_idx = expr_pool.len();
        expr_pool.push(expr.clone());
        work_stack.push(WorkItem::Process(root_idx));

        while let Some(work) = work_stack.pop() {
            match work {
                WorkItem::Process(expr_idx) => {
                    let current_expr = &expr_pool[expr_idx];
                    let expr_kind = current_expr.kind.clone();
                    let s = &current_expr.source_location;
                    let expr_source = (s.span.clone(), s.attribute.clone(), s.doc_name.clone());

                    match expr_kind {
                        ExpressionKind::Literal(lit) => match &lit.value {
                            ValueKind::Boolean(bool_val) => {
                                if *bool_val {
                                    constraint_stack.push(Constraint::True);
                                } else {
                                    constraint_stack.push(Constraint::False);
                                }
                            }
                            _ => {
                                return Err(LemmaError::engine(
                                    "Constraint expression must be boolean",
                                    crate::Source::new(
                                        expr_source.1.clone(),
                                        expr_source.0.clone(),
                                        expr_source.2.clone(),
                                    ),
                                    Arc::from(""),
                                    None::<String>,
                                ));
                            }
                        },
                        ExpressionKind::FactPath(fact_path) => {
                            constraint_stack.push(Constraint::Fact(fact_path.clone()));
                        }
                        ExpressionKind::Comparison(left, op, right) => {
                            match Self::from_comparison(&left, &op, &right) {
                                Ok(comparison_constraint) => {
                                    constraint_stack.push(comparison_constraint);
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        ExpressionKind::LogicalAnd(left, right) => {
                            let left_idx = expr_pool.len();
                            expr_pool.push((*left).clone());
                            let right_idx = expr_pool.len();
                            expr_pool.push((*right).clone());

                            work_stack.push(WorkItem::BuildAnd);
                            work_stack.push(WorkItem::Process(right_idx));
                            work_stack.push(WorkItem::Process(left_idx));
                        }
                        ExpressionKind::LogicalOr(left, right) => {
                            let left_idx = expr_pool.len();
                            expr_pool.push((*left).clone());
                            let right_idx = expr_pool.len();
                            expr_pool.push((*right).clone());

                            work_stack.push(WorkItem::BuildOr);
                            work_stack.push(WorkItem::Process(right_idx));
                            work_stack.push(WorkItem::Process(left_idx));
                        }
                        ExpressionKind::LogicalNegation(inner, _) => {
                            let inner_idx = expr_pool.len();
                            expr_pool.push((*inner).clone());

                            work_stack.push(WorkItem::ApplyNot);
                            work_stack.push(WorkItem::Process(inner_idx));
                        }
                        other => {
                            let s = &current_expr.source_location;
                            let expr_source =
                                (s.span.clone(), s.attribute.clone(), s.doc_name.clone());
                            return Err(LemmaError::engine(
                                format!(
                                    "Cannot convert expression kind to constraint: {:?}",
                                    std::mem::discriminant(&other)
                                ),
                                crate::Source::new(
                                    expr_source.1.clone(),
                                    expr_source.0.clone(),
                                    expr_source.2.clone(),
                                ),
                                Arc::from(""),
                                None::<String>,
                            ));
                        }
                    }
                }
                WorkItem::BuildAnd => {
                    let right = constraint_stack
                        .pop()
                        .expect("Internal error: missing right constraint for And");
                    let left = constraint_stack
                        .pop()
                        .expect("Internal error: missing left constraint for And");
                    constraint_stack.push(left.and(right));
                }
                WorkItem::BuildOr => {
                    let right = constraint_stack
                        .pop()
                        .expect("Internal error: missing right constraint for Or");
                    let left = constraint_stack
                        .pop()
                        .expect("Internal error: missing left constraint for Or");
                    constraint_stack.push(left.or(right));
                }
                WorkItem::ApplyNot => {
                    let inner = constraint_stack
                        .pop()
                        .expect("Internal error: missing constraint for Not");
                    constraint_stack.push(inner.not());
                }
            }
        }

        Ok(constraint_stack
            .pop()
            .expect("Internal error: no constraint result from expression conversion"))
    }

    /// Convert a comparison expression to a constraint
    fn from_comparison(
        left: &Expression,
        op: &ComparisonComputation,
        right: &Expression,
    ) -> LemmaResult<Constraint> {
        use ExpressionKind;

        fn inversion_err_for(
            left: &Expression,
            message: impl Into<String>,
            suggestion: Option<String>,
        ) -> LemmaError {
            LemmaError::inversion(message, &left.source_location, suggestion)
        }

        // Case 1: fact op literal (e.g., age > 18)
        if let ExpressionKind::FactPath(fact_path) = &left.kind {
            if let ExpressionKind::Literal(value) = &right.kind {
                return Ok(Constraint::Comparison {
                    fact: fact_path.clone(),
                    op: op.clone(),
                    value: Arc::new(value.as_ref().clone()),
                });
            }
        }

        // Case 2: literal op fact (e.g., 18 < age) - flip the comparison
        if let ExpressionKind::Literal(value) = &left.kind {
            if let ExpressionKind::FactPath(fact_path) = &right.kind {
                let flipped_op = flip_comparison_operator(op);
                return Ok(Constraint::Comparison {
                    fact: fact_path.clone(),
                    op: flipped_op,
                    value: Arc::new(value.as_ref().clone()),
                });
            }
        }

        // Case 3: literal op literal (e.g., "bronze" == "silver") - evaluate directly
        if let ExpressionKind::Literal(left_val) = &left.kind {
            if let ExpressionKind::Literal(right_val) = &right.kind {
                if let Some(result) = evaluate_literal_comparison(left_val, op, right_val) {
                    return Ok(if result {
                        Constraint::True
                    } else {
                        Constraint::False
                    });
                }
            }
        }

        // Case 4: comparison == boolean or comparison != boolean
        // (age > 18) == true  -> age > 18
        // (age > 18) == false -> not (age > 18)
        // (age > 18) != true  -> not (age > 18)
        // (age > 18) != false -> age > 18
        if op.is_equal() || op.is_not_equal() {
            if let ExpressionKind::Comparison(inner_left, inner_op, inner_right) = &left.kind {
                if let ExpressionKind::Literal(lit) = &right.kind {
                    if let ValueKind::Boolean(bool_val) = &lit.value {
                        let inner_constraint =
                            Self::from_comparison(inner_left, inner_op, inner_right)?;
                        // For ==: true means keep, false means negate
                        // For !=: true means negate, false means keep
                        let should_negate = if op.is_equal() { !*bool_val } else { *bool_val };
                        if should_negate {
                            return Ok(inner_constraint.not());
                        } else {
                            return Ok(inner_constraint);
                        }
                    }
                }
            }
            if let ExpressionKind::Literal(lit) = &left.kind {
                if let ValueKind::Boolean(bool_val) = &lit.value {
                    if let ExpressionKind::Comparison(inner_left, inner_op, inner_right) =
                        &right.kind
                    {
                        let inner_constraint =
                            Self::from_comparison(inner_left, inner_op, inner_right)?;
                        let should_negate = if op.is_equal() { !*bool_val } else { *bool_val };
                        if should_negate {
                            return Ok(inner_constraint.not());
                        } else {
                            return Ok(inner_constraint);
                        }
                    }
                }
            }
        }

        // Case 5: veto compared with anything
        // A veto is never equal to any literal value
        if matches!(&left.kind, ExpressionKind::Veto(_))
            || matches!(&right.kind, ExpressionKind::Veto(_))
        {
            return Ok(if op.is_not_equal() {
                Constraint::True
            } else {
                Constraint::False
            });
        }

        // Extended: try to rewrite richer comparison shapes into an atomic
        // `fact op literal` constraint by isolating a single unknown fact.
        if let Some(rewritten) = try_rewrite_comparison_to_atomic(left, op, right) {
            return Ok(rewritten);
        }

        Err(inversion_err_for(
            left,
            format!(
                "Cannot invert condition yet: unsupported comparison shape: {:?} {:?} {:?}",
                left, op, right
            ),
            Some(
                "Try rewriting the unless condition into a simple comparison between a single fact and a literal (e.g. x > 10)."
                    .to_string(),
            ),
        ))
    }

    /// Collect all fact paths referenced in this constraint
    pub fn collect_facts(&self) -> Vec<FactPath> {
        let mut facts = Vec::new();
        let mut stack = vec![self];

        while let Some(constraint) = stack.pop() {
            match constraint {
                Constraint::True | Constraint::False => {}
                Constraint::Comparison { fact, .. } => {
                    facts.push(fact.clone());
                }
                Constraint::Fact(fact_path) => {
                    facts.push(fact_path.clone());
                }
                Constraint::And(left, right) | Constraint::Or(left, right) => {
                    stack.push(left.as_ref());
                    stack.push(right.as_ref());
                }
                Constraint::Not(inner) => {
                    stack.push(inner.as_ref());
                }
            }
        }

        facts.sort_by_key(|a| a.to_string());
        facts.dedup();
        facts
    }
}

// =============================================================================
// Comparison rewriting for inversion (Option B)
// =============================================================================

fn try_rewrite_comparison_to_atomic(
    left: &Expression,
    op: &ComparisonComputation,
    right: &Expression,
) -> Option<Constraint> {
    use ExpressionKind;

    // Prefer constant-folding to reduce expression complexity.
    let left = constant_fold_expression(left).unwrap_or_else(|| left.clone());
    let right = constant_fold_expression(right).unwrap_or_else(|| right.clone());

    // Strip a top-level unit conversion wrapper when comparing against a literal.
    // This is safe because scale/duration comparisons are unit-normalized during evaluation/domain checks.
    let (left, right) = match (&left.kind, &right.kind) {
        (ExpressionKind::UnitConversion(inner, target), ExpressionKind::Literal(_)) => {
            if is_monotone_unit_conversion_target(target) {
                ((**inner).clone(), right.clone())
            } else {
                (left.clone(), right.clone())
            }
        }
        (ExpressionKind::Literal(_), ExpressionKind::UnitConversion(inner, target)) => {
            if is_monotone_unit_conversion_target(target) {
                (left.clone(), (**inner).clone())
            } else {
                (left.clone(), right.clone())
            }
        }
        _ => (left.clone(), right.clone()),
    };

    // We can only rewrite comparisons where one side is a literal and the other side contains facts.
    let (expr, mut op_norm, lit) = match (&left.kind, &right.kind) {
        (ExpressionKind::Literal(l), _) => {
            // literal op expr  =>  expr flip(op) literal
            let flipped = flip_comparison_operator(op);
            (right.clone(), flipped, l.as_ref().clone())
        }
        (_, ExpressionKind::Literal(r)) => (left.clone(), op.clone(), r.as_ref().clone()),
        _ => return None,
    };

    // Identify the single unknown fact.
    let mut facts = Vec::new();
    collect_fact_paths(&expr, &mut facts);
    facts.sort_by_key(|fp| fp.to_string());
    facts.dedup();
    if facts.len() != 1 {
        return None;
    }
    let fact = facts[0].clone();

    // Try to isolate the fact for this comparison.
    let (new_op, new_value) = isolate_linear_comparison(&expr, &fact, &op_norm, &lit)?;
    op_norm = new_op;

    Some(Constraint::Comparison {
        fact,
        op: op_norm,
        value: Arc::new(new_value),
    })
}

fn is_monotone_unit_conversion_target(target: &SemanticConversionTarget) -> bool {
    matches!(
        target,
        SemanticConversionTarget::Duration(_)
            | SemanticConversionTarget::ScaleUnit(_)
            | SemanticConversionTarget::RatioUnit(_)
    )
}

fn collect_fact_paths(expr: &Expression, out: &mut Vec<FactPath>) {
    use ExpressionKind;
    let mut stack: Vec<&Expression> = vec![expr];
    while let Some(e) = stack.pop() {
        match &e.kind {
            ExpressionKind::FactPath(fp) => out.push(fp.clone()),
            ExpressionKind::Arithmetic(l, _, r)
            | ExpressionKind::Comparison(l, _, r)
            | ExpressionKind::LogicalAnd(l, r)
            | ExpressionKind::LogicalOr(l, r) => {
                stack.push(l.as_ref());
                stack.push(r.as_ref());
            }
            ExpressionKind::LogicalNegation(inner, _)
            | ExpressionKind::UnitConversion(inner, _)
            | ExpressionKind::MathematicalComputation(_, inner) => {
                stack.push(inner.as_ref());
            }
            ExpressionKind::Literal(_) | ExpressionKind::Veto(_) | ExpressionKind::RulePath(_) => {}
        }
    }
}

fn contains_fact(expr: &Expression, fact: &FactPath) -> bool {
    let mut found = false;
    let mut facts = Vec::new();
    collect_fact_paths(expr, &mut facts);
    for fp in facts {
        if &fp == fact {
            found = true;
            break;
        }
    }
    found
}

fn constant_fold_expression(expr: &Expression) -> Option<Expression> {
    use ExpressionKind;

    match &expr.kind {
        ExpressionKind::Literal(_) => Some(expr.clone()),
        ExpressionKind::FactPath(_) => None,

        ExpressionKind::UnitConversion(inner, target) => {
            let folded_inner = constant_fold_expression(inner)?;
            if let ExpressionKind::Literal(lit) = &folded_inner.kind {
                match crate::computation::convert_unit(lit.as_ref(), target) {
                    OperationResult::Value(v) => Some(Expression::new(
                        ExpressionKind::Literal(Box::new(v.as_ref().clone())),
                        expr.source_location.clone(),
                    )),
                    _ => None,
                }
            } else {
                None
            }
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_folded = constant_fold_expression(left)?;
            let right_folded = constant_fold_expression(right)?;
            match (&left_folded.kind, &right_folded.kind) {
                (ExpressionKind::Literal(l), ExpressionKind::Literal(r)) => {
                    match crate::computation::arithmetic_operation(l.as_ref(), op, r.as_ref()) {
                        OperationResult::Value(v) => Some(Expression::new(
                            ExpressionKind::Literal(Box::new(v.as_ref().clone())),
                            expr.source_location.clone(),
                        )),
                        _ => None,
                    }
                }
                _ => None,
            }
        }

        // We only need folding for arithmetic/unit conversion for Option B.
        _ => None,
    }
}

fn flip_inequality(op: &ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThan,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThanOrEqual,
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThan,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThanOrEqual,
        ComparisonComputation::Equal | ComparisonComputation::Is => op.clone(),
        ComparisonComputation::NotEqual | ComparisonComputation::IsNot => op.clone(),
    }
}

fn isolate_linear_comparison(
    expr: &Expression,
    unknown: &FactPath,
    op: &ComparisonComputation,
    bound: &LiteralValue,
) -> Option<(ComparisonComputation, LiteralValue)> {
    use ExpressionKind;

    match &expr.kind {
        ExpressionKind::FactPath(fp) if fp == unknown => Some((op.clone(), bound.clone())),

        // Strip top-level monotone unit conversion wrappers.
        ExpressionKind::UnitConversion(inner, target)
            if is_monotone_unit_conversion_target(target) =>
        {
            isolate_linear_comparison(inner, unknown, op, bound)
        }

        ExpressionKind::Arithmetic(left, arithmetic_op, right) => {
            let left_contains = contains_fact(left, unknown);
            let right_contains = contains_fact(right, unknown);
            if left_contains && right_contains {
                return None;
            }

            // Fold the non-unknown side to a literal.
            if left_contains {
                let right_lit = constant_fold_expression(right)?;
                let ExpressionKind::Literal(c) = right_lit.kind else {
                    return None;
                };
                isolate_through_arithmetic_left(left, arithmetic_op, c.as_ref(), op, bound, unknown)
            } else if right_contains {
                let left_lit = constant_fold_expression(left)?;
                let ExpressionKind::Literal(c) = left_lit.kind else {
                    return None;
                };
                isolate_through_arithmetic_right(
                    right,
                    arithmetic_op,
                    c.as_ref(),
                    op,
                    bound,
                    unknown,
                )
            } else {
                None
            }
        }

        _ => None,
    }
}

fn isolate_through_arithmetic_left(
    inner_with_unknown: &Expression,
    operation: &ArithmeticComputation,
    constant: &LiteralValue,
    op: &ComparisonComputation,
    bound: &LiteralValue,
    unknown: &FactPath,
) -> Option<(ComparisonComputation, LiteralValue)> {
    match operation {
        ArithmeticComputation::Add => {
            // (x + c) op b  =>  x op (b - c)
            let new_bound = lit_sub(bound, constant)?;
            isolate_linear_comparison(inner_with_unknown, unknown, op, &new_bound)
        }
        ArithmeticComputation::Subtract => {
            // (x - c) op b  =>  x op (b + c)
            let new_bound = lit_add(bound, constant)?;
            isolate_linear_comparison(inner_with_unknown, unknown, op, &new_bound)
        }
        ArithmeticComputation::Multiply => {
            // (x * c) op b  =>  x op' (b / c)
            let c = constant_as_number(constant)?;
            if c.is_zero() {
                return None;
            }
            let mut new_op = op.clone();
            if c.is_sign_negative() && !op.is_equal() && !op.is_not_equal() {
                new_op = flip_inequality(&new_op);
            }
            let new_bound = lit_div_number(bound, c)?;
            isolate_linear_comparison(inner_with_unknown, unknown, &new_op, &new_bound)
        }
        ArithmeticComputation::Divide => {
            // (x / c) op b  =>  x op' (b * c)
            let c = constant_as_number(constant)?;
            if c.is_zero() {
                return None;
            }
            let mut new_op = op.clone();
            if c.is_sign_negative() && !op.is_equal() && !op.is_not_equal() {
                new_op = flip_inequality(&new_op);
            }
            let new_bound = lit_mul_number(bound, c)?;
            isolate_linear_comparison(inner_with_unknown, unknown, &new_op, &new_bound)
        }
        _ => None,
    }
}

fn isolate_through_arithmetic_right(
    inner_with_unknown: &Expression,
    operation: &ArithmeticComputation,
    constant: &LiteralValue,
    op: &ComparisonComputation,
    bound: &LiteralValue,
    unknown: &FactPath,
) -> Option<(ComparisonComputation, LiteralValue)> {
    match operation {
        ArithmeticComputation::Add => {
            // (c + x) op b  =>  x op (b - c)
            let new_bound = lit_sub(bound, constant)?;
            isolate_linear_comparison(inner_with_unknown, unknown, op, &new_bound)
        }
        ArithmeticComputation::Subtract => {
            // (c - x) op b  =>  x op' (c - b)
            let new_bound = lit_sub(constant, bound)?;
            let new_op = if op.is_equal() || op.is_not_equal() {
                op.clone()
            } else {
                flip_inequality(op)
            };
            isolate_linear_comparison(inner_with_unknown, unknown, &new_op, &new_bound)
        }
        ArithmeticComputation::Multiply => {
            // (c * x) op b  =>  x op' (b / c)
            let c = constant_as_number(constant)?;
            if c.is_zero() {
                return None;
            }
            let mut new_op = op.clone();
            if c.is_sign_negative() && !op.is_equal() && !op.is_not_equal() {
                new_op = flip_inequality(&new_op);
            }
            let new_bound = lit_div_number(bound, c)?;
            isolate_linear_comparison(inner_with_unknown, unknown, &new_op, &new_bound)
        }
        // (c / x) is non-linear (reciprocal)
        ArithmeticComputation::Divide => None,
        _ => None,
    }
}

fn constant_as_number(lit: &LiteralValue) -> Option<rust_decimal::Decimal> {
    match &lit.value {
        ValueKind::Number(n) => Some(*n),
        _ => None,
    }
}

fn lit_add(a: &LiteralValue, b: &LiteralValue) -> Option<LiteralValue> {
    match (&a.value, &b.value) {
        (ValueKind::Number(la), ValueKind::Number(lb)) => Some(LiteralValue::number_with_type(
            *la + *lb,
            a.lemma_type.clone(),
        )),
        (ValueKind::Scale(la, lua), ValueKind::Scale(lb, lub))
            if a.lemma_type == b.lemma_type && lua == lub =>
        {
            Some(LiteralValue::scale_with_type(
                *la + *lb,
                lua.clone(),
                a.lemma_type.clone(),
            ))
        }
        (ValueKind::Duration(la, lua), ValueKind::Duration(lb, lub))
            if a.lemma_type == b.lemma_type && lua == lub =>
        {
            Some(LiteralValue::duration_with_type(
                *la + *lb,
                lua.clone(),
                a.lemma_type.clone(),
            ))
        }
        _ => None,
    }
}

fn lit_sub(a: &LiteralValue, b: &LiteralValue) -> Option<LiteralValue> {
    match (&a.value, &b.value) {
        (ValueKind::Number(la), ValueKind::Number(lb)) => Some(LiteralValue::number_with_type(
            *la - *lb,
            a.lemma_type.clone(),
        )),
        (ValueKind::Scale(la, lua), ValueKind::Scale(lb, lub))
            if a.lemma_type == b.lemma_type && lua == lub =>
        {
            Some(LiteralValue::scale_with_type(
                *la - *lb,
                lua.clone(),
                a.lemma_type.clone(),
            ))
        }
        (ValueKind::Duration(la, lua), ValueKind::Duration(lb, lub))
            if a.lemma_type == b.lemma_type && lua == lub =>
        {
            Some(LiteralValue::duration_with_type(
                *la - *lb,
                lua.clone(),
                a.lemma_type.clone(),
            ))
        }
        _ => None,
    }
}

fn lit_mul_number(a: &LiteralValue, c: rust_decimal::Decimal) -> Option<LiteralValue> {
    match &a.value {
        ValueKind::Number(n) => Some(LiteralValue::number_with_type(*n * c, a.lemma_type.clone())),
        ValueKind::Scale(n, u) => Some(LiteralValue::scale_with_type(
            *n * c,
            u.clone(),
            a.lemma_type.clone(),
        )),
        ValueKind::Duration(n, u) => Some(LiteralValue::duration_with_type(
            *n * c,
            u.clone(),
            a.lemma_type.clone(),
        )),
        _ => None,
    }
}

fn lit_div_number(a: &LiteralValue, c: rust_decimal::Decimal) -> Option<LiteralValue> {
    if c.is_zero() {
        return None;
    }
    match &a.value {
        ValueKind::Number(n) => Some(LiteralValue::number_with_type(*n / c, a.lemma_type.clone())),
        ValueKind::Scale(n, u) => Some(LiteralValue::scale_with_type(
            *n / c,
            u.clone(),
            a.lemma_type.clone(),
        )),
        ValueKind::Duration(n, u) => Some(LiteralValue::duration_with_type(
            *n / c,
            u.clone(),
            a.lemma_type.clone(),
        )),
        _ => None,
    }
}

fn build_numeric_theory_closure(
    atoms: &[Constraint],
) -> LemmaResult<boolean_expression::Expr<usize>> {
    use boolean_expression::Expr;

    // Group indices of comparison atoms by fact path.
    let mut by_fact: std::collections::HashMap<FactPath, Vec<usize>> =
        std::collections::HashMap::new();
    for (idx, atom) in atoms.iter().enumerate() {
        if let Constraint::Comparison { fact, .. } = atom {
            by_fact.entry(fact.clone()).or_default().push(idx);
        }
    }

    let mut theory = Expr::Const(true);

    for idxs in by_fact.values() {
        for i in 0..idxs.len() {
            for j in (i + 1)..idxs.len() {
                let a_idx = idxs[i];
                let b_idx = idxs[j];

                let a = atoms.get(a_idx).unwrap();
                let b = atoms.get(b_idx).unwrap();

                let (a_dom, b_dom) = match (a, b) {
                    (
                        Constraint::Comparison {
                            op: a_op,
                            value: a_val,
                            ..
                        },
                        Constraint::Comparison {
                            op: b_op,
                            value: b_val,
                            ..
                        },
                    ) => (
                        crate::inversion::domain::domain_for_comparison_atom(a_op, a_val.as_ref())?,
                        crate::inversion::domain::domain_for_comparison_atom(b_op, b_val.as_ref())?,
                    ),
                    _ => continue,
                };

                // Implications (both directions as applicable)
                if a_dom.is_subset_of(&b_dom) {
                    // A -> B  ==  (!A) OR B
                    theory = Expr::and(
                        theory,
                        Expr::or(Expr::not(Expr::Terminal(a_idx)), Expr::Terminal(b_idx)),
                    );
                }
                if b_dom.is_subset_of(&a_dom) {
                    theory = Expr::and(
                        theory,
                        Expr::or(Expr::not(Expr::Terminal(b_idx)), Expr::Terminal(a_idx)),
                    );
                }

                // Mutual exclusion when disjoint
                if a_dom.intersect(&b_dom).is_empty() {
                    // not(A and B)
                    theory = Expr::and(
                        theory,
                        Expr::not(Expr::and(Expr::Terminal(a_idx), Expr::Terminal(b_idx))),
                    );
                }
            }
        }
    }

    Ok(theory)
}

/// Evaluate a comparison between two literals, returning the boolean result
fn evaluate_literal_comparison(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> Option<bool> {
    match (&left.value, &right.value) {
        // Text equality
        (ValueKind::Text(l), ValueKind::Text(r)) => {
            if op.is_equal() {
                Some(l == r)
            } else if op.is_not_equal() {
                Some(l != r)
            } else {
                None
            }
        }
        // Boolean equality
        (ValueKind::Boolean(l), ValueKind::Boolean(r)) => {
            if op.is_equal() {
                Some(l == r)
            } else if op.is_not_equal() {
                Some(l != r)
            } else {
                None
            }
        }
        // Number comparisons
        (ValueKind::Number(l), ValueKind::Number(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => Some(l == r),
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => Some(l != r),
            ComparisonComputation::LessThan => Some(l < r),
            ComparisonComputation::LessThanOrEqual => Some(l <= r),
            ComparisonComputation::GreaterThan => Some(l > r),
            ComparisonComputation::GreaterThanOrEqual => Some(l >= r),
        },
        // Ratio comparisons
        (ValueKind::Ratio(l, _), ValueKind::Ratio(r, _)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => Some(l == r),
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => Some(l != r),
            ComparisonComputation::LessThan => Some(l < r),
            ComparisonComputation::LessThanOrEqual => Some(l <= r),
            ComparisonComputation::GreaterThan => Some(l > r),
            ComparisonComputation::GreaterThanOrEqual => Some(l >= r),
        },
        _ => None,
    }
}

/// Flip a comparison operator (for converting `literal op fact` to `fact flipped_op literal`)
fn flip_comparison_operator(op: &ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::Equal => ComparisonComputation::Equal,
        ComparisonComputation::NotEqual => ComparisonComputation::NotEqual,
        ComparisonComputation::Is => ComparisonComputation::Is,
        ComparisonComputation::IsNot => ComparisonComputation::IsNot,
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThan,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThanOrEqual,
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThan,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThanOrEqual,
    }
}

// ============================================================================
// BDD-based simplification
// ============================================================================

/// Find an atom in the atoms vector, or add it if not found
/// Returns the index of the atom in the atoms vector
fn find_or_add_atom(constraint: &Constraint, atoms: &mut Vec<Constraint>) -> usize {
    for (i, atom) in atoms.iter().enumerate() {
        if constraints_structurally_equal(atom, constraint) {
            return i;
        }
    }
    atoms.push(constraint.clone());
    atoms.len() - 1
}

/// Convert a constraint to a BDD expression
fn to_bool_expr(
    constraint: &Constraint,
    atoms: &mut Vec<Constraint>,
) -> Option<boolean_expression::Expr<usize>> {
    use boolean_expression::Expr;

    enum WorkItem {
        Visit(Box<Constraint>),
        BuildAnd,
        BuildOr,
        BuildNot,
    }

    let mut stack = vec![WorkItem::Visit(Box::new(constraint.clone()))];
    let mut expr_stack: Vec<Expr<usize>> = Vec::new();

    while let Some(work) = stack.pop() {
        match work {
            WorkItem::Visit(c) => match c.as_ref() {
                Constraint::True => expr_stack.push(Expr::Const(true)),
                Constraint::False => expr_stack.push(Expr::Const(false)),
                Constraint::And(left, right) => {
                    stack.push(WorkItem::BuildAnd);
                    stack.push(WorkItem::Visit(right.clone()));
                    stack.push(WorkItem::Visit(left.clone()));
                }
                Constraint::Or(left, right) => {
                    stack.push(WorkItem::BuildOr);
                    stack.push(WorkItem::Visit(right.clone()));
                    stack.push(WorkItem::Visit(left.clone()));
                }
                Constraint::Not(inner) => {
                    stack.push(WorkItem::BuildNot);
                    stack.push(WorkItem::Visit(inner.clone()));
                }
                Constraint::Comparison { .. } | Constraint::Fact(_) => {
                    let idx = find_or_add_atom(c.as_ref(), atoms);
                    expr_stack.push(Expr::Terminal(idx));
                }
            },
            WorkItem::BuildAnd => {
                let right = expr_stack.pop()?;
                let left = expr_stack.pop()?;
                expr_stack.push(Expr::and(left, right));
            }
            WorkItem::BuildOr => {
                let right = expr_stack.pop()?;
                let left = expr_stack.pop()?;
                expr_stack.push(Expr::or(left, right));
            }
            WorkItem::BuildNot => {
                let inner = expr_stack.pop()?;
                expr_stack.push(Expr::not(inner));
            }
        }
    }

    expr_stack.pop()
}

/// Check if two constraints are structurally equal (for atom deduplication)
fn constraints_structurally_equal(a: &Constraint, b: &Constraint) -> bool {
    match (a, b) {
        (Constraint::True, Constraint::True) => true,
        (Constraint::False, Constraint::False) => true,
        (
            Constraint::Comparison {
                fact: f1,
                op: o1,
                value: v1,
            },
            Constraint::Comparison {
                fact: f2,
                op: o2,
                value: v2,
            },
        ) => f1 == f2 && o1 == o2 && v1 == v2,
        (Constraint::Fact(f1), Constraint::Fact(f2)) => f1 == f2,
        _ => false,
    }
}

/// Convert a BDD expression back to a constraint
fn from_bool_expr(bool_expr: &boolean_expression::Expr<usize>, atoms: &[Constraint]) -> Constraint {
    use boolean_expression::Expr;

    enum Work {
        Process(Expr<usize>),
        CombineAnd,
        CombineOr,
        ApplyNot,
    }

    let mut work_stack = vec![Work::Process(bool_expr.clone())];
    let mut constraint_stack: Vec<Constraint> = Vec::new();

    while let Some(work) = work_stack.pop() {
        match work {
            Work::Process(expr) => match expr {
                Expr::Const(true) => constraint_stack.push(Constraint::True),
                Expr::Const(false) => constraint_stack.push(Constraint::False),
                Expr::Terminal(i) => {
                    constraint_stack.push(atoms.get(i).cloned().unwrap_or(Constraint::False));
                }
                Expr::Not(inner) => {
                    work_stack.push(Work::ApplyNot);
                    work_stack.push(Work::Process(*inner));
                }
                Expr::And(left, right) => {
                    work_stack.push(Work::CombineAnd);
                    work_stack.push(Work::Process(*right));
                    work_stack.push(Work::Process(*left));
                }
                Expr::Or(left, right) => {
                    work_stack.push(Work::CombineOr);
                    work_stack.push(Work::Process(*right));
                    work_stack.push(Work::Process(*left));
                }
            },
            Work::CombineAnd => {
                let right = constraint_stack.pop().unwrap_or(Constraint::False);
                let left = constraint_stack.pop().unwrap_or(Constraint::False);
                constraint_stack.push(left.and(right));
            }
            Work::CombineOr => {
                let right = constraint_stack.pop().unwrap_or(Constraint::False);
                let left = constraint_stack.pop().unwrap_or(Constraint::False);
                constraint_stack.push(left.or(right));
            }
            Work::ApplyNot => {
                let inner = constraint_stack.pop().unwrap_or(Constraint::False);
                constraint_stack.push(inner.not());
            }
        }
    }

    constraint_stack.pop().unwrap_or(Constraint::False)
}

// ============================================================================
// Display and Serialize implementations
// ============================================================================

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Constraint::True => write!(f, "true"),
            Constraint::False => write!(f, "false"),
            Constraint::Comparison { fact, op, value } => {
                write!(f, "{} {} {}", fact, op, value)
            }
            Constraint::Fact(fact_path) => write!(f, "{}", fact_path),
            Constraint::And(left, right) => {
                let left_str = format_with_parens(left, self);
                let right_str = format_with_parens(right, self);
                write!(f, "{} and {}", left_str, right_str)
            }
            Constraint::Or(left, right) => {
                let left_str = format_with_parens(left, self);
                let right_str = format_with_parens(right, self);
                write!(f, "{} or {}", left_str, right_str)
            }
            Constraint::Not(inner) => match inner.as_ref() {
                Constraint::And(_, _) | Constraint::Or(_, _) => {
                    write!(f, "not ({})", inner)
                }
                _ => write!(f, "not {}", inner),
            },
        }
    }
}

/// Format a constraint with parentheses if needed for precedence
fn format_with_parens(inner: &Constraint, parent: &Constraint) -> String {
    let needs_parens = matches!(
        (parent, inner),
        (Constraint::And(_, _), Constraint::Or(_, _))
    );

    if needs_parens {
        format!("({})", inner)
    } else {
        inner.to_string()
    }
}

impl Serialize for Constraint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Constraint::True => {
                let mut state = serializer.serialize_struct("Constraint", 1)?;
                state.serialize_field("type", "true")?;
                state.end()
            }
            Constraint::False => {
                let mut state = serializer.serialize_struct("Constraint", 1)?;
                state.serialize_field("type", "false")?;
                state.end()
            }
            Constraint::Comparison { fact, op, value } => {
                let mut state = serializer.serialize_struct("Constraint", 4)?;
                state.serialize_field("type", "comparison")?;
                state.serialize_field("fact", &fact.to_string())?;
                state.serialize_field("op", &op.to_string())?;
                state.serialize_field("value", value)?;
                state.end()
            }
            Constraint::Fact(fact_path) => {
                let mut state = serializer.serialize_struct("Constraint", 2)?;
                state.serialize_field("type", "fact")?;
                state.serialize_field("fact", &fact_path.to_string())?;
                state.end()
            }
            Constraint::And(left, right) => {
                let mut state = serializer.serialize_struct("Constraint", 3)?;
                state.serialize_field("type", "and")?;
                state.serialize_field("left", left)?;
                state.serialize_field("right", right)?;
                state.end()
            }
            Constraint::Or(left, right) => {
                let mut state = serializer.serialize_struct("Constraint", 3)?;
                state.serialize_field("type", "or")?;
                state.serialize_field("left", left)?;
                state.serialize_field("right", right)?;
                state.end()
            }
            Constraint::Not(inner) => {
                let mut state = serializer.serialize_struct("Constraint", 2)?;
                state.serialize_field("type", "not")?;
                state.serialize_field("inner", inner)?;
                state.end()
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

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

    fn comparison(fact_name: &str, op: ComparisonComputation, val: i64) -> Constraint {
        Constraint::Comparison {
            fact: fact(fact_name),
            op,
            value: Arc::new(num(val)),
        }
    }

    // Basic constraint tests

    #[test]
    fn test_constraint_and_short_circuit() {
        let c1 = Constraint::True;
        let c2 = Constraint::Fact(fact("x"));
        assert!(matches!(c1.and(c2.clone()), Constraint::Fact(_)));

        let c3 = Constraint::False;
        assert!(matches!(c3.and(c2), Constraint::False));
    }

    #[test]
    fn test_constraint_or_short_circuit() {
        let c1 = Constraint::False;
        let c2 = Constraint::Fact(fact("x"));
        assert!(matches!(c1.or(c2.clone()), Constraint::Fact(_)));

        let c3 = Constraint::True;
        assert!(matches!(c3.or(c2), Constraint::True));
    }

    #[test]
    fn test_constraint_not_double_negation() {
        let c = Constraint::Fact(fact("x"));
        let not_c = c.clone().not();
        let not_not_c = not_c.not();
        assert_eq!(c, not_not_c);
    }

    #[test]
    fn test_constraint_display_simple() {
        let c = Constraint::Comparison {
            fact: fact("age"),
            op: ComparisonComputation::GreaterThan,
            value: Arc::new(num(18)),
        };
        assert_eq!(c.to_string(), "age > 18");
    }

    #[test]
    fn test_constraint_display_and() {
        let c1 = Constraint::Comparison {
            fact: fact("age"),
            op: ComparisonComputation::GreaterThan,
            value: Arc::new(num(18)),
        };
        let c2 = Constraint::Fact(fact("is_employee"));
        let combined = Constraint::And(Box::new(c1), Box::new(c2.not()));
        assert_eq!(combined.to_string(), "age > 18 and not is_employee");
    }

    #[test]
    fn test_collect_facts() {
        let c = Constraint::And(
            Box::new(Constraint::Comparison {
                fact: fact("age"),
                op: ComparisonComputation::GreaterThan,
                value: Arc::new(num(18)),
            }),
            Box::new(Constraint::Fact(fact("is_employee"))),
        );
        let facts = c.collect_facts();
        assert_eq!(facts.len(), 2);
    }

    // Simplification tests

    #[test]
    fn test_simplify_tautology() {
        // (A and B) or (A and not B) = A
        let a = comparison("x", ComparisonComputation::GreaterThan, 10);
        let b = Constraint::Fact(fact("flag"));

        let expr = a.clone().and(b.clone()).or(a.clone().and(b.not()));
        let simplified = expr.simplify().unwrap();

        assert_eq!(simplified.to_string(), "x > 10");
    }

    #[test]
    fn test_simplify_contradiction() {
        // x == 1 and x == 2 cannot both be true.
        let c1 = comparison("x", ComparisonComputation::Equal, 1);
        let c2 = comparison("x", ComparisonComputation::Equal, 2);

        let expr = c1.and(c2);
        let simplified = expr.simplify().unwrap();

        assert!(
            simplified.is_false(),
            "Expected contradiction to simplify to false, got: {}",
            simplified
        );
    }

    #[test]
    fn test_simplify_detects_ordering_implication_contradiction() {
        // Numerically: x > 5 implies x > 3, so (x > 5) and not(x > 3) is impossible.
        let a = comparison("x", ComparisonComputation::GreaterThan, 5);
        let b = comparison("x", ComparisonComputation::GreaterThan, 3);
        let expr = a.and(b.not());
        let simplified = expr.simplify().unwrap();
        assert!(
            simplified.is_false(),
            "Expected contradiction to simplify to false, got: {}",
            simplified
        );
    }

    #[test]
    fn test_simplify_detects_neq_contradiction() {
        // x == 5 and x != 5 cannot both be true.
        let eq = comparison("x", ComparisonComputation::Equal, 5);
        let neq = comparison("x", ComparisonComputation::NotEqual, 5);
        let simplified = eq.and(neq).simplify().unwrap();
        assert!(
            simplified.is_false(),
            "Expected contradiction to simplify to false, got: {}",
            simplified
        );
    }
}
