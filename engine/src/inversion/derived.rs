//! Derived expressions for inversion
//!
//! Expressions created during solving have no source location.
//! They are derived from plan expressions, not parsed from user input.
//! Strong separation: Expression (planning) has source; DerivedExpression (inversion) does not.

use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, DataPath, LiteralValue, MathematicalComputation,
    NegationType, RulePath, SemanticConversionTarget, VetoExpression,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// Expression derived during inversion/solving. No source location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedExpression {
    pub kind: DerivedExpressionKind,
}

impl DerivedExpression {
    pub fn collect_data_paths(&self, data: &mut HashSet<DataPath>) {
        self.kind.collect_data_paths(data);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedExpressionKind {
    /// Boxed to keep enum size small (LiteralValue is large)
    Literal(Box<LiteralValue>),
    DataPath(DataPath),
    RulePath(RulePath),
    LogicalAnd(Arc<DerivedExpression>, Arc<DerivedExpression>),
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
    fn collect_data_paths(&self, data: &mut HashSet<DataPath>) {
        match self {
            DerivedExpressionKind::DataPath(fp) => {
                data.insert(fp.clone());
            }
            DerivedExpressionKind::LogicalAnd(left, right)
            | DerivedExpressionKind::Arithmetic(left, _, right)
            | DerivedExpressionKind::Comparison(left, _, right) => {
                left.collect_data_paths(data);
                right.collect_data_paths(data);
            }
            DerivedExpressionKind::UnitConversion(inner, _)
            | DerivedExpressionKind::LogicalNegation(inner, _)
            | DerivedExpressionKind::MathematicalComputation(_, inner) => {
                inner.collect_data_paths(data);
            }
            DerivedExpressionKind::Literal(_)
            | DerivedExpressionKind::RulePath(_)
            | DerivedExpressionKind::Veto(_) => {}
        }
    }
}
