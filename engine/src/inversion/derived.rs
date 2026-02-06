//! Derived expressions for inversion
//!
//! Expressions created during solving have no source location.
//! They are derived from plan expressions, not parsed from user input.
//! Strong separation: Expression (planning) has source; DerivedExpression (inversion) does not.

use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, FactPath, LiteralValue, MathematicalComputation,
    NegationType, RulePath, SemanticConversionTarget, VetoExpression,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Expression derived during inversion/solving. No source location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedExpression {
    pub kind: DerivedExpressionKind,
}

impl DerivedExpression {
    pub fn collect_fact_paths(&self, facts: &mut HashSet<FactPath>) {
        self.kind.collect_fact_paths(facts);
    }

    pub fn semantic_hash<H: Hasher>(&self, state: &mut H) {
        self.kind.semantic_hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedExpressionKind {
    /// Boxed to keep enum size small (LiteralValue is large)
    Literal(Box<LiteralValue>),
    FactPath(FactPath),
    RulePath(RulePath),
    LogicalAnd(Arc<DerivedExpression>, Arc<DerivedExpression>),
    LogicalOr(Arc<DerivedExpression>, Arc<DerivedExpression>),
    Arithmetic(
        Arc<DerivedExpression>,
        ArithmeticComputation,
        Arc<DerivedExpression>,
    ),
    Comparison(
        Arc<DerivedExpression>,
        ComparisonComputation,
        Arc<DerivedExpression>,
    ),
    UnitConversion(Arc<DerivedExpression>, SemanticConversionTarget),
    LogicalNegation(Arc<DerivedExpression>, NegationType),
    MathematicalComputation(MathematicalComputation, Arc<DerivedExpression>),
    Veto(VetoExpression),
}

impl DerivedExpressionKind {
    fn collect_fact_paths(&self, facts: &mut HashSet<FactPath>) {
        match self {
            DerivedExpressionKind::FactPath(fp) => {
                facts.insert(fp.clone());
            }
            DerivedExpressionKind::LogicalAnd(left, right)
            | DerivedExpressionKind::LogicalOr(left, right)
            | DerivedExpressionKind::Arithmetic(left, _, right)
            | DerivedExpressionKind::Comparison(left, _, right) => {
                left.collect_fact_paths(facts);
                right.collect_fact_paths(facts);
            }
            DerivedExpressionKind::UnitConversion(inner, _)
            | DerivedExpressionKind::LogicalNegation(inner, _)
            | DerivedExpressionKind::MathematicalComputation(_, inner) => {
                inner.collect_fact_paths(facts);
            }
            DerivedExpressionKind::Literal(_)
            | DerivedExpressionKind::RulePath(_)
            | DerivedExpressionKind::Veto(_) => {}
        }
    }

    fn semantic_hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            DerivedExpressionKind::Literal(lit) => lit.hash(state),
            DerivedExpressionKind::FactPath(fp) => fp.hash(state),
            DerivedExpressionKind::RulePath(rp) => rp.hash(state),
            DerivedExpressionKind::LogicalAnd(left, right)
            | DerivedExpressionKind::LogicalOr(left, right) => {
                left.semantic_hash(state);
                right.semantic_hash(state);
            }
            DerivedExpressionKind::Arithmetic(left, op, right) => {
                left.semantic_hash(state);
                op.hash(state);
                right.semantic_hash(state);
            }
            DerivedExpressionKind::Comparison(left, op, right) => {
                left.semantic_hash(state);
                op.hash(state);
                right.semantic_hash(state);
            }
            DerivedExpressionKind::UnitConversion(expr, target) => {
                expr.semantic_hash(state);
                target.hash(state);
            }
            DerivedExpressionKind::LogicalNegation(expr, neg_type) => {
                expr.semantic_hash(state);
                neg_type.hash(state);
            }
            DerivedExpressionKind::MathematicalComputation(op, expr) => {
                op.hash(state);
                expr.semantic_hash(state);
            }
            DerivedExpressionKind::Veto(v) => v.message.hash(state),
        }
    }
}

impl Eq for DerivedExpression {}
impl Hash for DerivedExpression {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.semantic_hash(state);
    }
}

impl Eq for DerivedExpressionKind {}
impl Hash for DerivedExpressionKind {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.semantic_hash(state);
    }
}
