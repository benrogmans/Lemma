//! Expression normalization: algebraic simplification for drift-free evaluation.
//!
//! Internal [`NormalForm`] IR; public API is [`normalize_expression`].

use crate::computation::rational::{
    convert_quantity_magnitude_rational, rational_is_zero, rational_one, rational_operation,
    rational_zero, NumericFailure, NumericOperation, RationalInteger,
};
use crate::computation::UnitResolutionContext;
use crate::evaluation::conversion_explanation::infer_anonymous_quantity_unit_name;
use crate::parsing::ast::{CalendarPeriodUnit, DateCalendarKind, DateRelativeKind};
use crate::planning::semantics::{
    negated_comparison, primitive_number, ArithmeticComputation, ComparisonComputation, Expression,
    ExpressionKind, LiteralValue, MathematicalComputation, NegationType, SemanticConversionTarget,
    Source, ValueKind, VetoExpression,
};
use crate::Error;
use std::sync::Arc;

fn normalization_error(source: Option<Source>, failure: NumericFailure, context: &str) -> Error {
    Error::validation(format!("{context}: {failure}"), source, None::<String>)
}

fn literal_from_folded_rational(
    rational: RationalInteger,
    _source: Option<Source>,
) -> Result<LiteralValue, Error> {
    Ok(LiteralValue::number_with_type(
        rational,
        primitive_number().clone(),
    ))
}

/// Normalize a semantic expression to canonical form (fixed-point rewrite).
pub(crate) fn normalize_expression(
    expr: &Expression,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
) -> Result<Expression, Error> {
    let source = expr.source_location.clone();
    let mut nf = to_normal_form(expr);
    loop {
        let prev = nf.clone();
        nf = normalize_once(nf, unit_ctx, source.clone())?;
        if nf == prev {
            break;
        }
    }
    Ok(to_expression(&nf, source))
}

/// Unless-chain as nested `LogicalOr(LogicalAnd(cond, val), fallback)` (forward unless order).
/// Matches rule evaluation: last `unless` in source is checked first (outermost `Or` evaluates
/// left child first; left is `And(last_unless)`).
#[must_use]
pub(crate) fn build_unless_chain(branches: &[(Option<Expression>, Expression)]) -> Expression {
    assert!(
        !branches.is_empty(),
        "BUG: rule must have at least one branch"
    );
    if branches.len() == 1 {
        return branches[0].1.clone();
    }
    let default = branches[0].1.clone();
    let mut acc = default;
    for (condition, result) in branches.iter().skip(1) {
        let cond = condition
            .as_ref()
            .expect("BUG: non-default branch missing condition");
        let guarded = Expression::with_source(
            ExpressionKind::LogicalAnd(Arc::new(cond.clone()), Arc::new(result.clone())),
            None,
        );
        acc = Expression::with_source(
            ExpressionKind::LogicalOr(Arc::new(guarded), Arc::new(acc)),
            None,
        );
    }
    acc
}

/// Replace each `RulePath` with the referenced rule's normalized result expression.
#[must_use]
pub(crate) fn inline_rule_refs(
    expr: &Expression,
    normalized_rules: &std::collections::HashMap<crate::planning::semantics::RulePath, Expression>,
) -> Expression {
    match &expr.kind {
        ExpressionKind::RulePath(path) => normalized_rules
            .get(path)
            .cloned()
            .unwrap_or_else(|| expr.clone()),
        ExpressionKind::Literal(_)
        | ExpressionKind::DataPath(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Now => expr.clone(),
        ExpressionKind::LogicalAnd(left, right) => Expression::with_source(
            ExpressionKind::LogicalAnd(
                Arc::new(inline_rule_refs(left, normalized_rules)),
                Arc::new(inline_rule_refs(right, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::LogicalOr(left, right) => Expression::with_source(
            ExpressionKind::LogicalOr(
                Arc::new(inline_rule_refs(left, normalized_rules)),
                Arc::new(inline_rule_refs(right, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::Arithmetic(left, op, right) => Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(inline_rule_refs(left, normalized_rules)),
                op.clone(),
                Arc::new(inline_rule_refs(right, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::Comparison(left, op, right) => Expression::with_source(
            ExpressionKind::Comparison(
                Arc::new(inline_rule_refs(left, normalized_rules)),
                op.clone(),
                Arc::new(inline_rule_refs(right, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::UnitConversion(inner, target) => Expression::with_source(
            ExpressionKind::UnitConversion(
                Arc::new(inline_rule_refs(inner, normalized_rules)),
                target.clone(),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::LogicalNegation(inner, neg) => Expression::with_source(
            ExpressionKind::LogicalNegation(
                Arc::new(inline_rule_refs(inner, normalized_rules)),
                neg.clone(),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::MathematicalComputation(op, inner) => Expression::with_source(
            ExpressionKind::MathematicalComputation(
                op.clone(),
                Arc::new(inline_rule_refs(inner, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::DateRelative(kind, inner) => Expression::with_source(
            ExpressionKind::DateRelative(
                *kind,
                Arc::new(inline_rule_refs(inner, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::DateCalendar(kind, unit, inner) => Expression::with_source(
            ExpressionKind::DateCalendar(
                *kind,
                *unit,
                Arc::new(inline_rule_refs(inner, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::RangeLiteral(left, right) => Expression::with_source(
            ExpressionKind::RangeLiteral(
                Arc::new(inline_rule_refs(left, normalized_rules)),
                Arc::new(inline_rule_refs(right, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::PastFutureRange(kind, inner) => Expression::with_source(
            ExpressionKind::PastFutureRange(
                *kind,
                Arc::new(inline_rule_refs(inner, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::RangeContainment(left, right) => Expression::with_source(
            ExpressionKind::RangeContainment(
                Arc::new(inline_rule_refs(left, normalized_rules)),
                Arc::new(inline_rule_refs(right, normalized_rules)),
            ),
            expr.source_location.clone(),
        ),
        ExpressionKind::ResultIsVeto(operand) => Expression::with_source(
            ExpressionKind::ResultIsVeto(operand.clone()),
            expr.source_location.clone(),
        ),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum NormalForm {
    Leaf(LeafKind),
    Sum(Vec<NormalForm>),
    Product(Vec<NormalForm>),
    Subtract(Arc<NormalForm>, Arc<NormalForm>),
    Divide(Arc<NormalForm>, Arc<NormalForm>),
    Power(Arc<NormalForm>, Arc<NormalForm>),
    Modulo(Arc<NormalForm>, Arc<NormalForm>),
    Negate(Arc<NormalForm>),
    Reciprocal(Arc<NormalForm>),
    Comparison(Arc<NormalForm>, ComparisonComputation, Arc<NormalForm>),
    And(Vec<NormalForm>),
    Or(Vec<NormalForm>),
    Not(Arc<NormalForm>),
    MathOp(MathematicalComputation, Arc<NormalForm>),
    UnitConversion(Arc<NormalForm>, SemanticConversionTarget),
    Veto(VetoExpression),
    DateRelative(DateRelativeKind, Arc<NormalForm>),
    DateCalendar(DateCalendarKind, CalendarPeriodUnit, Arc<NormalForm>),
    RangeLiteral(Arc<NormalForm>, Arc<NormalForm>),
    PastFutureRange(DateRelativeKind, Arc<NormalForm>),
    RangeContainment(Arc<NormalForm>, Arc<NormalForm>),
    ResultIsVeto(Arc<NormalForm>),
    Now,
}

#[derive(Clone, Debug, PartialEq)]
enum LeafKind {
    Literal(Arc<LiteralValue>),
    DataPath(crate::planning::semantics::DataPath),
    RulePath(crate::planning::semantics::RulePath),
}

fn normalize_once(
    nf: NormalForm,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalForm, Error> {
    let nf = normalize_children(nf, unit_ctx, source.clone())?;
    simplify(nf, unit_ctx, source)
}

fn normalize_children(
    nf: NormalForm,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalForm, Error> {
    let normalize_vec = |children: Vec<NormalForm>| -> Result<Vec<NormalForm>, Error> {
        children
            .into_iter()
            .map(|child| normalize_once(child, unit_ctx, source.clone()))
            .collect()
    };
    match nf {
        NormalForm::Sum(children) => Ok(NormalForm::Sum(normalize_vec(children)?)),
        NormalForm::Product(children) => Ok(NormalForm::Product(normalize_vec(children)?)),
        NormalForm::Subtract(a, b) => Ok(NormalForm::Subtract(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Divide(a, b) => Ok(NormalForm::Divide(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Power(a, b) => Ok(NormalForm::Power(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Modulo(a, b) => Ok(NormalForm::Modulo(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Negate(x) => Ok(NormalForm::Negate(Arc::new(normalize_once(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::Reciprocal(x) => Ok(NormalForm::Reciprocal(Arc::new(normalize_once(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::Comparison(a, op, b) => Ok(NormalForm::Comparison(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            op,
            Arc::new(normalize_once((*b).clone(), unit_ctx, source)?),
        )),
        NormalForm::And(children) => Ok(NormalForm::And(normalize_vec(children)?)),
        NormalForm::Or(children) => Ok(NormalForm::Or(normalize_vec(children)?)),
        NormalForm::Not(x) => Ok(NormalForm::Not(Arc::new(normalize_once(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::MathOp(op, x) => Ok(NormalForm::MathOp(
            op,
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::UnitConversion(x, target) => Ok(NormalForm::UnitConversion(
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
            target,
        )),
        NormalForm::DateRelative(kind, x) => Ok(NormalForm::DateRelative(
            kind,
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::DateCalendar(kind, unit, x) => Ok(NormalForm::DateCalendar(
            kind,
            unit,
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::RangeLiteral(a, b) => Ok(NormalForm::RangeLiteral(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source)?),
        )),
        NormalForm::PastFutureRange(kind, x) => Ok(NormalForm::PastFutureRange(
            kind,
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::RangeContainment(a, b) => Ok(NormalForm::RangeContainment(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source)?),
        )),
        NormalForm::ResultIsVeto(operand) => Ok(NormalForm::ResultIsVeto(Arc::new(
            normalize_once((*operand).clone(), unit_ctx, source)?,
        ))),
        leaf @ (NormalForm::Leaf(_) | NormalForm::Veto(_) | NormalForm::Now) => Ok(leaf),
    }
}

fn simplify(
    nf: NormalForm,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalForm, Error> {
    let nf = expand_numeric_subtract_divide(nf);
    let nf = flatten_associative(nf);
    let nf = eliminate_identities(nf);
    let nf = double_negate_reciprocal(nf);
    let nf = power_laws(nf);
    let nf = constant_fold(nf, source.clone())?;
    let nf = fold_unit_literals(nf, unit_ctx, source)?;
    let nf = demorgan(nf);
    let nf = logical_flatten(nf);
    let nf = logical_short_circuit(nf);
    let nf = logical_idempotency(nf);
    let nf = negated_comparisons(nf);
    let nf = math_identities(nf);
    Ok(canonical_order(nf))
}

fn fold_quantity_unit_literal(
    literal: &LiteralValue,
    target_unit: &str,
    unit_context: UnitResolutionContext<'_>,
) -> Result<LiteralValue, Error> {
    let ValueKind::Quantity(magnitude, from_unit, _) = &literal.value else {
        panic!("BUG: fold_quantity_unit_literal called with non-quantity literal");
    };
    let (from_factor, to_factor, target_lemma_type) = match unit_context {
        UnitResolutionContext::WithIndex(unit_index) => {
            if from_unit.is_empty() {
                let target_type = unit_index.get(target_unit).ok_or_else(|| {
                    Error::validation(
                        format!(
                            "cannot fold unit conversion: target unit '{target_unit}' is not in the unit index"
                        ),
                        None,
                        None::<String>,
                    )
                })?;
                let from_unit_name = infer_anonymous_quantity_unit_name(
                    &literal.lemma_type,
                    unit_index,
                    target_unit,
                )
                .ok_or_else(|| {
                    Error::validation(
                        "cannot fold anonymous quantity unit conversion".to_string(),
                        None,
                        None::<String>,
                    )
                })?;
                let from_factor = target_type.quantity_unit_factor(&from_unit_name);
                let to_factor = target_type.quantity_unit_factor(target_unit);
                (from_factor, to_factor, target_type.clone())
            } else {
                let from_factor = literal.lemma_type.quantity_unit_factor(from_unit);
                let to_factor = literal.lemma_type.quantity_unit_factor(target_unit);
                (from_factor, to_factor, literal.lemma_type.clone())
            }
        }
        UnitResolutionContext::NamedQuantityOnly => {
            let from_factor = literal.lemma_type.quantity_unit_factor(from_unit);
            let to_factor = literal.lemma_type.quantity_unit_factor(target_unit);
            (from_factor, to_factor, literal.lemma_type.clone())
        }
    };
    let converted_magnitude =
        convert_quantity_magnitude_rational(*magnitude, from_factor, to_factor)
            .map_err(|failure| normalization_error(None, failure, "quantity unit literal fold"))?;
    Ok(LiteralValue::quantity_with_type(
        converted_magnitude,
        target_unit.to_string(),
        target_lemma_type,
    ))
}

fn fold_unit_literals(
    nf: NormalForm,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalForm, Error> {
    let fold_vec = |children: Vec<NormalForm>| -> Result<Vec<NormalForm>, Error> {
        children
            .into_iter()
            .map(|child| fold_unit_literals(child, unit_ctx, source.clone()))
            .collect()
    };
    match nf {
        NormalForm::Sum(children) => Ok(NormalForm::Sum(fold_vec(children)?)),
        NormalForm::Product(children) => Ok(NormalForm::Product(fold_vec(children)?)),
        NormalForm::Subtract(a, b) => Ok(NormalForm::Subtract(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Divide(a, b) => Ok(NormalForm::Divide(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Power(a, b) => Ok(NormalForm::Power(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Modulo(a, b) => Ok(NormalForm::Modulo(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Negate(x) => Ok(NormalForm::Negate(Arc::new(fold_unit_literals(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::Reciprocal(x) => Ok(NormalForm::Reciprocal(Arc::new(fold_unit_literals(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::Comparison(a, op, b) => Ok(NormalForm::Comparison(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            op,
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source)?),
        )),
        NormalForm::And(children) => Ok(NormalForm::And(fold_vec(children)?)),
        NormalForm::Or(children) => Ok(NormalForm::Or(fold_vec(children)?)),
        NormalForm::Not(x) => Ok(NormalForm::Not(Arc::new(fold_unit_literals(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::MathOp(op, x) => Ok(NormalForm::MathOp(
            op,
            Arc::new(fold_unit_literals((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::UnitConversion(inner, target) => {
            let inner_done = fold_unit_literals((*inner).clone(), unit_ctx, source.clone())?;
            if let (Some(unit_context), NormalForm::Leaf(LeafKind::Literal(literal))) =
                (unit_ctx, &inner_done)
            {
                match (&literal.value, &target) {
                    (
                        ValueKind::Quantity(_, _, _),
                        SemanticConversionTarget::QuantityUnit(target_unit),
                    ) => {
                        let converted = fold_quantity_unit_literal(
                            literal.as_ref(),
                            target_unit.as_str(),
                            *unit_context,
                        )?;
                        return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(converted))));
                    }
                    (ValueKind::Number(number), SemanticConversionTarget::Number) => {
                        return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                            LiteralValue::number_with_type(*number, primitive_number().clone()),
                        ))));
                    }
                    (
                        ValueKind::Number(number),
                        SemanticConversionTarget::QuantityUnit(target_unit),
                    ) => {
                        let target_type = match unit_context {
                            UnitResolutionContext::WithIndex(unit_index) => unit_index
                                .get(target_unit.as_str())
                                .ok_or_else(|| {
                                    Error::validation(
                                        format!(
                                            "cannot fold number to quantity unit '{target_unit}'"
                                        ),
                                        source.clone(),
                                        None::<String>,
                                    )
                                })?
                                .clone(),
                            UnitResolutionContext::NamedQuantityOnly => {
                                return Ok(NormalForm::UnitConversion(
                                    Arc::new(inner_done),
                                    target.clone(),
                                ));
                            }
                        };
                        let to_factor = target_type.quantity_unit_factor(target_unit.as_str());
                        let _ = to_factor;
                        return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                            LiteralValue::quantity_with_type(
                                *number,
                                target_unit.clone(),
                                target_type,
                            ),
                        ))));
                    }
                    _ => {}
                }
            }
            Ok(NormalForm::UnitConversion(Arc::new(inner_done), target))
        }
        NormalForm::DateRelative(kind, x) => Ok(NormalForm::DateRelative(
            kind,
            Arc::new(fold_unit_literals((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::DateCalendar(kind, unit, x) => Ok(NormalForm::DateCalendar(
            kind,
            unit,
            Arc::new(fold_unit_literals((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::RangeLiteral(a, b) => Ok(NormalForm::RangeLiteral(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source)?),
        )),
        NormalForm::PastFutureRange(kind, x) => Ok(NormalForm::PastFutureRange(
            kind,
            Arc::new(fold_unit_literals((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::RangeContainment(a, b) => Ok(NormalForm::RangeContainment(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source)?),
        )),
        leaf @ (NormalForm::Leaf(_)
        | NormalForm::Veto(_)
        | NormalForm::ResultIsVeto(_)
        | NormalForm::Now) => Ok(leaf),
    }
}

fn to_normal_form(expr: &Expression) -> NormalForm {
    match &expr.kind {
        ExpressionKind::Literal(lit) => {
            NormalForm::Leaf(LeafKind::Literal(Arc::new((**lit).clone())))
        }
        ExpressionKind::DataPath(p) => NormalForm::Leaf(LeafKind::DataPath(p.clone())),
        ExpressionKind::RulePath(p) => NormalForm::Leaf(LeafKind::RulePath(p.clone())),
        ExpressionKind::LogicalAnd(left, right) => {
            NormalForm::And(vec![to_normal_form(left), to_normal_form(right)])
        }
        ExpressionKind::LogicalOr(left, right) => {
            NormalForm::Or(vec![to_normal_form(left), to_normal_form(right)])
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Subtract, right) => {
            NormalForm::Subtract(
                Arc::new(to_normal_form(left)),
                Arc::new(to_normal_form(right)),
            )
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Divide, right) => {
            NormalForm::Divide(
                Arc::new(to_normal_form(left)),
                Arc::new(to_normal_form(right)),
            )
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Add, right) => {
            NormalForm::Sum(vec![to_normal_form(left), to_normal_form(right)])
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Multiply, right) => {
            NormalForm::Product(vec![to_normal_form(left), to_normal_form(right)])
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Power, right) => NormalForm::Power(
            Arc::new(to_normal_form(left)),
            Arc::new(to_normal_form(right)),
        ),
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Modulo, right) => {
            NormalForm::Modulo(
                Arc::new(to_normal_form(left)),
                Arc::new(to_normal_form(right)),
            )
        }
        ExpressionKind::Comparison(left, op, right) => NormalForm::Comparison(
            Arc::new(to_normal_form(left)),
            op.clone(),
            Arc::new(to_normal_form(right)),
        ),
        ExpressionKind::UnitConversion(inner, target) => {
            NormalForm::UnitConversion(Arc::new(to_normal_form(inner)), target.clone())
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            NormalForm::Not(Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::MathematicalComputation(op, inner) => {
            NormalForm::MathOp(op.clone(), Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::Veto(v) => NormalForm::Veto(v.clone()),
        ExpressionKind::Now => NormalForm::Now,
        ExpressionKind::DateRelative(kind, inner) => {
            NormalForm::DateRelative(*kind, Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::DateCalendar(kind, unit, inner) => {
            NormalForm::DateCalendar(*kind, *unit, Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::RangeLiteral(left, right) => NormalForm::RangeLiteral(
            Arc::new(to_normal_form(left)),
            Arc::new(to_normal_form(right)),
        ),
        ExpressionKind::PastFutureRange(kind, inner) => {
            NormalForm::PastFutureRange(*kind, Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::RangeContainment(left, right) => NormalForm::RangeContainment(
            Arc::new(to_normal_form(left)),
            Arc::new(to_normal_form(right)),
        ),
        ExpressionKind::ResultIsVeto(operand) => {
            NormalForm::ResultIsVeto(Arc::new(to_normal_form(operand)))
        }
    }
}

fn to_expression(nf: &NormalForm, source: Option<Source>) -> Expression {
    let kind = nf_to_kind(nf, source.clone());
    Expression::with_source(kind, source)
}

fn nf_to_kind(nf: &NormalForm, source: Option<Source>) -> ExpressionKind {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(lit)) => {
            ExpressionKind::Literal(Box::new((**lit).clone()))
        }
        NormalForm::Leaf(LeafKind::DataPath(p)) => ExpressionKind::DataPath(p.clone()),
        NormalForm::Leaf(LeafKind::RulePath(p)) => ExpressionKind::RulePath(p.clone()),
        NormalForm::Sum(children) => sum_to_kind(children, source),
        NormalForm::Product(children) => product_to_kind(children, source),
        NormalForm::Subtract(a, b) => ExpressionKind::Arithmetic(
            Arc::new(to_expression(a, source.clone())),
            ArithmeticComputation::Subtract,
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::Divide(a, b) => ExpressionKind::Arithmetic(
            Arc::new(to_expression(a, source.clone())),
            ArithmeticComputation::Divide,
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::Power(base, exp) => ExpressionKind::Arithmetic(
            Arc::new(to_expression(base, source.clone())),
            ArithmeticComputation::Power,
            Arc::new(to_expression(exp, source.clone())),
        ),
        NormalForm::Modulo(a, b) => ExpressionKind::Arithmetic(
            Arc::new(to_expression(a, source.clone())),
            ArithmeticComputation::Modulo,
            Arc::new(to_expression(b, source.clone())),
        ),
        NormalForm::Negate(x) => {
            let zero = Expression::with_source(
                ExpressionKind::Literal(Box::new(LiteralValue::number(rational_zero()))),
                source.clone(),
            );
            ExpressionKind::Arithmetic(
                Arc::new(zero),
                ArithmeticComputation::Subtract,
                Arc::new(to_expression(x, source)),
            )
        }
        NormalForm::Reciprocal(x) => {
            let one = Expression::with_source(
                ExpressionKind::Literal(Box::new(LiteralValue::number(rational_one()))),
                source.clone(),
            );
            ExpressionKind::Arithmetic(
                Arc::new(one),
                ArithmeticComputation::Divide,
                Arc::new(to_expression(x, source)),
            )
        }
        NormalForm::Comparison(a, op, b) => ExpressionKind::Comparison(
            Arc::new(to_expression(a, source.clone())),
            op.clone(),
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::And(children) => and_to_kind(children, source),
        NormalForm::Or(children) => or_to_kind(children, source),
        NormalForm::Not(x) => {
            ExpressionKind::LogicalNegation(Arc::new(to_expression(x, source)), NegationType::Not)
        }
        NormalForm::MathOp(op, x) => {
            ExpressionKind::MathematicalComputation(op.clone(), Arc::new(to_expression(x, source)))
        }
        NormalForm::UnitConversion(x, target) => {
            ExpressionKind::UnitConversion(Arc::new(to_expression(x, source)), target.clone())
        }
        NormalForm::Veto(v) => ExpressionKind::Veto(v.clone()),
        NormalForm::Now => ExpressionKind::Now,
        NormalForm::DateRelative(kind, x) => {
            ExpressionKind::DateRelative(*kind, Arc::new(to_expression(x, source)))
        }
        NormalForm::DateCalendar(kind, unit, x) => {
            ExpressionKind::DateCalendar(*kind, *unit, Arc::new(to_expression(x, source)))
        }
        NormalForm::RangeLiteral(a, b) => ExpressionKind::RangeLiteral(
            Arc::new(to_expression(a, source.clone())),
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::PastFutureRange(kind, x) => {
            ExpressionKind::PastFutureRange(*kind, Arc::new(to_expression(x, source)))
        }
        NormalForm::RangeContainment(a, b) => ExpressionKind::RangeContainment(
            Arc::new(to_expression(a, source.clone())),
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::ResultIsVeto(operand) => {
            ExpressionKind::ResultIsVeto(Arc::new(to_expression(operand, source)))
        }
    }
}

fn sum_to_kind(children: &[NormalForm], source: Option<Source>) -> ExpressionKind {
    match children {
        [] => ExpressionKind::Literal(Box::new(LiteralValue::number(rational_zero()))),
        [one] => nf_to_kind(one, source),
        [first, rest @ ..] => {
            let mut acc =
                Expression::with_source(nf_to_kind(first, source.clone()), source.clone());
            for c in rest {
                acc = Expression::with_source(
                    ExpressionKind::Arithmetic(
                        Arc::new(acc),
                        ArithmeticComputation::Add,
                        Arc::new(to_expression(c, source.clone())),
                    ),
                    source.clone(),
                );
            }
            acc.kind
        }
    }
}

fn product_to_kind(children: &[NormalForm], source: Option<Source>) -> ExpressionKind {
    match children {
        [] => ExpressionKind::Literal(Box::new(LiteralValue::number(rational_one()))),
        [one] => nf_to_kind(one, source),
        [first, rest @ ..] => {
            let mut acc =
                Expression::with_source(nf_to_kind(first, source.clone()), source.clone());
            for c in rest {
                acc = Expression::with_source(
                    ExpressionKind::Arithmetic(
                        Arc::new(acc),
                        ArithmeticComputation::Multiply,
                        Arc::new(to_expression(c, source.clone())),
                    ),
                    source.clone(),
                );
            }
            acc.kind
        }
    }
}

fn and_to_kind(children: &[NormalForm], source: Option<Source>) -> ExpressionKind {
    match children {
        [] => ExpressionKind::Literal(Box::new(LiteralValue::from_bool(true))),
        [one] => nf_to_kind(one, source),
        [first, rest @ ..] => {
            let mut acc =
                Expression::with_source(nf_to_kind(first, source.clone()), source.clone());
            for c in rest {
                acc = Expression::with_source(
                    ExpressionKind::LogicalAnd(
                        Arc::new(acc),
                        Arc::new(to_expression(c, source.clone())),
                    ),
                    source.clone(),
                );
            }
            acc.kind
        }
    }
}

fn or_to_kind(children: &[NormalForm], source: Option<Source>) -> ExpressionKind {
    match children {
        [] => ExpressionKind::Literal(Box::new(LiteralValue::from_bool(false))),
        [one] => nf_to_kind(one, source),
        [first, rest @ ..] => {
            let mut acc =
                Expression::with_source(nf_to_kind(first, source.clone()), source.clone());
            for c in rest {
                acc = Expression::with_source(
                    ExpressionKind::LogicalOr(
                        Arc::new(acc),
                        Arc::new(to_expression(c, source.clone())),
                    ),
                    source.clone(),
                );
            }
            acc.kind
        }
    }
}

fn expand_numeric_subtract_divide(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Subtract(a, b) if is_numeric_only(&a) && is_numeric_only(&b) => {
            NormalForm::Sum(vec![(*a).clone(), NormalForm::Negate(b)])
        }
        NormalForm::Divide(a, b) if is_numeric_only(&a) && is_numeric_only(&b) => {
            NormalForm::Product(vec![(*a).clone(), NormalForm::Reciprocal(b)])
        }
        NormalForm::Sum(children) => NormalForm::Sum(
            children
                .into_iter()
                .map(expand_numeric_subtract_divide)
                .collect(),
        ),
        NormalForm::Product(children) => NormalForm::Product(
            children
                .into_iter()
                .map(expand_numeric_subtract_divide)
                .collect(),
        ),
        NormalForm::Subtract(a, b) => NormalForm::Subtract(
            Arc::new(expand_numeric_subtract_divide((*a).clone())),
            Arc::new(expand_numeric_subtract_divide((*b).clone())),
        ),
        NormalForm::Divide(a, b) => NormalForm::Divide(
            Arc::new(expand_numeric_subtract_divide((*a).clone())),
            Arc::new(expand_numeric_subtract_divide((*b).clone())),
        ),
        NormalForm::Power(a, b) => NormalForm::Power(
            Arc::new(expand_numeric_subtract_divide((*a).clone())),
            Arc::new(expand_numeric_subtract_divide((*b).clone())),
        ),
        NormalForm::Negate(x) => {
            NormalForm::Negate(Arc::new(expand_numeric_subtract_divide((*x).clone())))
        }
        NormalForm::Reciprocal(x) => {
            NormalForm::Reciprocal(Arc::new(expand_numeric_subtract_divide((*x).clone())))
        }
        NormalForm::And(children) => NormalForm::And(
            children
                .into_iter()
                .map(expand_numeric_subtract_divide)
                .collect(),
        ),
        NormalForm::Or(children) => NormalForm::Or(
            children
                .into_iter()
                .map(expand_numeric_subtract_divide)
                .collect(),
        ),
        NormalForm::Not(x) => {
            NormalForm::Not(Arc::new(expand_numeric_subtract_divide((*x).clone())))
        }
        NormalForm::Comparison(a, op, b) => NormalForm::Comparison(
            Arc::new(expand_numeric_subtract_divide((*a).clone())),
            op,
            Arc::new(expand_numeric_subtract_divide((*b).clone())),
        ),
        NormalForm::MathOp(op, x) => {
            NormalForm::MathOp(op, Arc::new(expand_numeric_subtract_divide((*x).clone())))
        }
        NormalForm::UnitConversion(x, target) => NormalForm::UnitConversion(
            Arc::new(expand_numeric_subtract_divide((*x).clone())),
            target,
        ),
        other => other,
    }
}

fn is_numeric_only(nf: &NormalForm) -> bool {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(lit)) => {
            matches!(lit.value, crate::planning::semantics::ValueKind::Number(_))
        }
        NormalForm::Sum(children) | NormalForm::Product(children) => {
            children.iter().all(is_numeric_only)
        }
        NormalForm::Subtract(a, b) | NormalForm::Divide(a, b) => {
            is_numeric_only(a) && is_numeric_only(b)
        }
        NormalForm::Power(a, b) => is_numeric_only(a) && is_numeric_only(b),
        NormalForm::Negate(x) | NormalForm::Reciprocal(x) => is_numeric_only(x),
        _ => false,
    }
}

fn flatten_associative(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Sum(children) => {
            let mut flat = Vec::new();
            for c in children {
                match flatten_associative(c) {
                    NormalForm::Sum(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }
            NormalForm::Sum(flat)
        }
        NormalForm::Product(children) => {
            let mut flat = Vec::new();
            for c in children {
                match flatten_associative(c) {
                    NormalForm::Product(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }
            NormalForm::Product(flat)
        }
        // Do not flatten nested And: guard chains are `And(cond, val)`; merging would yield
        // `And(And(cond, val), …)` rebuilds as left-assoc and leaves a non-boolean on the
        // left of an outer `LogicalAnd` (invalid for evaluator / unless semantics).
        NormalForm::And(children) => {
            NormalForm::And(children.into_iter().map(flatten_associative).collect())
        }
        NormalForm::Or(children) => {
            let mut flat = Vec::new();
            for c in children {
                match flatten_associative(c) {
                    NormalForm::Or(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }
            NormalForm::Or(flat)
        }
        other => other,
    }
}

fn eliminate_identities(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Sum(children) => {
            let children: Vec<_> = children
                .into_iter()
                .filter_map(|c| {
                    if is_numeric_zero(&c) {
                        None
                    } else {
                        Some(eliminate_identities(c))
                    }
                })
                .collect();
            match children.len() {
                0 => NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_zero(),
                )))),
                1 => children.into_iter().next().expect("BUG: len 1"),
                _ => NormalForm::Sum(children),
            }
        }
        NormalForm::Product(children) => {
            if children.iter().any(is_numeric_zero) {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_zero(),
                ))));
            }
            let children: Vec<_> = children
                .into_iter()
                .filter_map(|c| {
                    if is_numeric_one(&c) {
                        None
                    } else {
                        Some(eliminate_identities(c))
                    }
                })
                .collect();
            match children.len() {
                0 => NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_one(),
                )))),
                1 => children.into_iter().next().expect("BUG: len 1"),
                _ => NormalForm::Product(children),
            }
        }
        NormalForm::Power(base, exp) => {
            let base = Arc::new(eliminate_identities((*base).clone()));
            let exp = Arc::new(eliminate_identities((*exp).clone()));
            if is_numeric_one(&exp) {
                return (*base).clone();
            }
            if is_numeric_zero(&exp) {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_one(),
                ))));
            }
            NormalForm::Power(base, exp)
        }
        NormalForm::Negate(x) => {
            let inner = eliminate_identities((*x).clone());
            if is_numeric_zero(&inner) {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_zero(),
                ))));
            }
            NormalForm::Negate(Arc::new(inner))
        }
        NormalForm::Subtract(a, b) => {
            let a = eliminate_identities((*a).clone());
            let b = eliminate_identities((*b).clone());
            if is_numeric_zero(&a) {
                return NormalForm::Negate(Arc::new(b));
            }
            if is_numeric_zero(&b) {
                return a;
            }
            NormalForm::Subtract(Arc::new(a), Arc::new(b))
        }
        NormalForm::Divide(a, b) => {
            let a = eliminate_identities((*a).clone());
            let b = eliminate_identities((*b).clone());
            if is_numeric_one(&b) {
                return a;
            }
            if is_numeric_one(&a) {
                return NormalForm::Reciprocal(Arc::new(b));
            }
            NormalForm::Divide(Arc::new(a), Arc::new(b))
        }
        other => other,
    }
}

fn double_negate_reciprocal(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Negate(x) => match (*x).clone() {
            NormalForm::Negate(y) => (*y).clone(),
            other => NormalForm::Negate(Arc::new(double_negate_reciprocal(other))),
        },
        NormalForm::Reciprocal(x) => match (*x).clone() {
            NormalForm::Reciprocal(y) => (*y).clone(),
            other => NormalForm::Reciprocal(Arc::new(double_negate_reciprocal(other))),
        },
        NormalForm::Not(x) => match (*x).clone() {
            NormalForm::Not(y) => (*y).clone(),
            other => NormalForm::Not(Arc::new(double_negate_reciprocal(other))),
        },
        other => other,
    }
}

fn power_laws(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Power(base, exp) => {
            let base_simplified = power_laws((*base).clone());
            let exp_simplified = power_laws((*exp).clone());
            if let NormalForm::Power(inner_base, inner_exp) = &base_simplified {
                if let Some(new_exp) = try_multiply_nf_rational(inner_exp, &exp_simplified) {
                    return NormalForm::Power(Arc::clone(inner_base), Arc::new(new_exp));
                }
            }
            NormalForm::Power(Arc::new(base_simplified), Arc::new(exp_simplified))
        }
        NormalForm::Product(children) => {
            let children: Vec<_> = children.into_iter().map(power_laws).collect();
            collect_like_base_powers(children)
        }
        other => other,
    }
}

fn collect_like_base_powers(children: Vec<NormalForm>) -> NormalForm {
    let mut powers: Vec<(NormalForm, NormalForm)> = Vec::new();
    let mut other = Vec::new();
    for c in children {
        if let NormalForm::Power(base, exp) = c {
            let base_val = (*base).clone();
            if let Some((_, stored_exp)) = powers.iter_mut().find(|(b, _)| *b == base_val) {
                if let Some(sum_exp) = try_add_nf_rational(stored_exp, &exp) {
                    *stored_exp = sum_exp;
                    continue;
                }
            }
            powers.push((base_val, (*exp).clone()));
        } else {
            other.push(c);
        }
    }
    for (base, exp) in powers {
        other.push(NormalForm::Power(Arc::new(base), Arc::new(exp)));
    }
    match other.len() {
        0 => NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
            rational_one(),
        )))),
        1 => other.into_iter().next().expect("BUG: single product term"),
        _ => NormalForm::Product(other),
    }
}

fn try_multiply_nf_rational(a: &NormalForm, b: &NormalForm) -> Option<NormalForm> {
    let ra = as_rational_literal(a)?;
    let rb = as_rational_literal(b)?;
    let rational = rational_operation(&ra, NumericOperation::Multiply, &rb).ok()?;
    let literal = literal_from_folded_rational(rational, None).ok()?;
    Some(NormalForm::Leaf(LeafKind::Literal(Arc::new(literal))))
}

fn try_add_nf_rational(a: &NormalForm, b: &NormalForm) -> Option<NormalForm> {
    let ra = as_rational_literal(a)?;
    let rb = as_rational_literal(b)?;
    let rational = rational_operation(&ra, NumericOperation::Add, &rb).ok()?;
    let literal = literal_from_folded_rational(rational, None).ok()?;
    Some(NormalForm::Leaf(LeafKind::Literal(Arc::new(literal))))
}

fn constant_fold(nf: NormalForm, source: Option<Source>) -> Result<NormalForm, Error> {
    match nf {
        NormalForm::Sum(children) => {
            if children.iter().all(|c| as_rational_literal(c).is_some()) {
                let mut acc = RationalInteger::new(0, 1);
                for child in &children {
                    let rational = as_rational_literal(child).expect("BUG: all numeric");
                    acc = rational_operation(&acc, NumericOperation::Add, &rational).map_err(
                        |failure| normalization_error(source.clone(), failure, "constant fold sum"),
                    )?;
                }
                return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                    literal_from_folded_rational(acc, source.clone())?,
                ))));
            }
            Ok(NormalForm::Sum(
                children
                    .into_iter()
                    .map(|child| constant_fold(child, source.clone()))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        NormalForm::Product(children) => {
            if children.iter().all(|c| as_rational_literal(c).is_some()) {
                let mut acc = RationalInteger::new(1, 1);
                for child in &children {
                    let rational = as_rational_literal(child).expect("BUG: all numeric");
                    acc = rational_operation(&acc, NumericOperation::Multiply, &rational).map_err(
                        |failure| {
                            normalization_error(source.clone(), failure, "constant fold product")
                        },
                    )?;
                }
                return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                    literal_from_folded_rational(acc, source.clone())?,
                ))));
            }
            Ok(NormalForm::Product(
                children
                    .into_iter()
                    .map(|child| constant_fold(child, source.clone()))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        NormalForm::Power(base, exp) => {
            let base = constant_fold((*base).clone(), source.clone())?;
            let exp = constant_fold((*exp).clone(), source.clone())?;
            if let (Some(base_rational), Some(exp_rational)) =
                (as_rational_literal(&base), as_rational_literal(&exp))
            {
                if let Ok(rational) =
                    rational_operation(&base_rational, NumericOperation::Power, &exp_rational)
                {
                    return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(rational, source.clone())?,
                    ))));
                }
            }
            Ok(NormalForm::Power(Arc::new(base), Arc::new(exp)))
        }
        NormalForm::Negate(x) => {
            let inner = constant_fold((*x).clone(), source.clone())?;
            if let Some(rational) = as_rational_literal(&inner) {
                let zero = RationalInteger::new(0, 1);
                let negated = rational_operation(&zero, NumericOperation::Subtract, &rational)
                    .map_err(|failure| {
                        normalization_error(source.clone(), failure, "constant fold negate")
                    })?;
                return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                    literal_from_folded_rational(negated, source.clone())?,
                ))));
            }
            Ok(NormalForm::Negate(Arc::new(inner)))
        }
        NormalForm::Not(x) => {
            let inner = constant_fold((*x).clone(), source.clone())?;
            if let NormalForm::Leaf(LeafKind::Literal(literal)) = &inner {
                if let ValueKind::Boolean(boolean) = &literal.value {
                    return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                        LiteralValue::from_bool(!boolean),
                    ))));
                }
            }
            Ok(NormalForm::Not(Arc::new(inner)))
        }
        NormalForm::MathOp(op, x) => {
            let inner = constant_fold((*x).clone(), source.clone())?;
            if let Some(rational) = as_rational_literal(&inner) {
                if let Some(folded) = fold_math_op(&op, &rational) {
                    return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(folded, source.clone())?,
                    ))));
                }
            }
            Ok(NormalForm::MathOp(op, Arc::new(inner)))
        }
        other => Ok(other),
    }
}

fn fold_math_op(
    op: &MathematicalComputation,
    operand: &RationalInteger,
) -> Option<RationalInteger> {
    let zero = RationalInteger::new(0, 1);
    let one = RationalInteger::new(1, 1);
    match op {
        MathematicalComputation::Sin if *operand == zero => Some(zero),
        MathematicalComputation::Cos if *operand == zero => Some(one),
        MathematicalComputation::Tan if *operand == zero => Some(zero),
        MathematicalComputation::Log if *operand == one => Some(zero),
        MathematicalComputation::Exp if *operand == zero => Some(one),
        _ => None,
    }
}

fn demorgan(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Not(x) => match (*x).clone() {
            NormalForm::And(children) => NormalForm::Or(
                children
                    .into_iter()
                    .map(|c| NormalForm::Not(Arc::new(c)))
                    .map(demorgan)
                    .collect(),
            ),
            NormalForm::Or(children) => NormalForm::And(
                children
                    .into_iter()
                    .map(|c| NormalForm::Not(Arc::new(c)))
                    .map(demorgan)
                    .collect(),
            ),
            NormalForm::Comparison(a, op, b) => {
                NormalForm::Comparison(a, negated_comparison(op), b)
            }
            other => NormalForm::Not(Arc::new(demorgan(other))),
        },
        other => other,
    }
}

fn logical_flatten(nf: NormalForm) -> NormalForm {
    flatten_associative(nf)
}

fn logical_short_circuit(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::And(children) => {
            for c in &children {
                if matches!(
                    c,
                    NormalForm::Leaf(LeafKind::Literal(l))
                        if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
                ) {
                    return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                        false,
                    ))));
                }
            }
            NormalForm::And(children)
        }
        NormalForm::Or(children) => {
            for c in &children {
                if matches!(
                    c,
                    NormalForm::Leaf(LeafKind::Literal(l))
                        if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
                ) {
                    return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                        true,
                    ))));
                }
            }
            NormalForm::Or(children)
        }
        other => other,
    }
}

fn logical_idempotency(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::And(children) => {
            let mut unique = Vec::new();
            for c in children {
                if !unique.contains(&c) {
                    unique.push(c);
                }
            }
            NormalForm::And(unique)
        }
        NormalForm::Or(children) => {
            let mut unique = Vec::new();
            for c in children {
                if !unique.contains(&c) {
                    unique.push(c);
                }
            }
            NormalForm::Or(unique)
        }
        other => other,
    }
}

fn negated_comparisons(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Not(x) => match (*x).clone() {
            NormalForm::Comparison(a, op, b) => {
                NormalForm::Comparison(a, negated_comparison(op), b)
            }
            other => NormalForm::Not(Arc::new(negated_comparisons(other))),
        },
        other => other,
    }
}

fn math_identities(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::MathOp(MathematicalComputation::Abs, x) => match (*x).clone() {
            NormalForm::MathOp(MathematicalComputation::Abs, inner) => {
                NormalForm::MathOp(MathematicalComputation::Abs, inner)
            }
            other => NormalForm::MathOp(
                MathematicalComputation::Abs,
                Arc::new(math_identities(other)),
            ),
        },
        NormalForm::MathOp(MathematicalComputation::Exp, x) => match (*x).clone() {
            NormalForm::MathOp(MathematicalComputation::Log, inner) => (*inner).clone(),
            other => NormalForm::MathOp(
                MathematicalComputation::Exp,
                Arc::new(math_identities(other)),
            ),
        },
        NormalForm::MathOp(MathematicalComputation::Log, x) => match (*x).clone() {
            NormalForm::MathOp(MathematicalComputation::Exp, inner) => (*inner).clone(),
            other => NormalForm::MathOp(
                MathematicalComputation::Log,
                Arc::new(math_identities(other)),
            ),
        },
        NormalForm::MathOp(MathematicalComputation::Sqrt, x) => {
            let base = math_identities((*x).clone());
            let half = NormalForm::Leaf(LeafKind::Literal(Arc::new(
                literal_from_folded_rational(RationalInteger::new(1, 2), None)
                    .expect("BUG: literal 1/2 must commit at normalize"),
            )));
            NormalForm::Power(Arc::new(base), Arc::new(half))
        }
        other => other,
    }
}

fn canonical_order(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Sum(mut children) => {
            children.sort_by_cached_key(sort_key);
            NormalForm::Sum(children)
        }
        NormalForm::Product(mut children) => {
            children.sort_by_cached_key(sort_key);
            NormalForm::Product(children)
        }
        // And/Or: operand order is semantically significant (guards, unless-chain / short-circuit).
        NormalForm::And(children) => {
            NormalForm::And(children.into_iter().map(canonical_order).collect())
        }
        NormalForm::Or(children) => {
            NormalForm::Or(children.into_iter().map(canonical_order).collect())
        }
        other => other,
    }
}

fn sort_key(nf: &NormalForm) -> u8 {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(_)) => 0,
        _ => 1,
    }
}

fn is_numeric_zero(nf: &NormalForm) -> bool {
    as_rational_literal(nf).is_some_and(|r| rational_is_zero(&r))
}

fn is_numeric_one(nf: &NormalForm) -> bool {
    as_rational_literal(nf).is_some_and(|r| r == RationalInteger::new(1, 1))
}

fn as_rational_literal(nf: &NormalForm) -> Option<RationalInteger> {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(literal)) => match &literal.value {
            ValueKind::Number(number) => Some(*number),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::RationalInteger;
    use crate::computation::UnitResolutionContext;
    use crate::planning::semantics::{
        ComparisonComputation, DataPath, ExpressionKind, NegationType, SemanticConversionTarget,
    };

    fn num_expr(n: i64) -> Expression {
        Expression::with_source(
            ExpressionKind::Literal(Box::new(LiteralValue::number(RationalInteger::new(
                n as i128, 1,
            )))),
            None,
        )
    }

    fn bool_expr(b: bool) -> Expression {
        Expression::with_source(
            ExpressionKind::Literal(Box::new(LiteralValue::from_bool(b))),
            None,
        )
    }

    fn dx() -> Expression {
        Expression::with_source(
            ExpressionKind::DataPath(DataPath::new(vec![], "x".into())),
            None,
        )
    }

    fn add_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Add, Arc::new(b)),
            None,
        )
    }

    fn mul_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Multiply, Arc::new(b)),
            None,
        )
    }

    fn pow_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Power, Arc::new(b)),
            None,
        )
    }

    fn and_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(ExpressionKind::LogicalAnd(Arc::new(a), Arc::new(b)), None)
    }

    fn or_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(ExpressionKind::LogicalOr(Arc::new(a), Arc::new(b)), None)
    }

    fn not_expr(inner: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::LogicalNegation(Arc::new(inner), NegationType::Not),
            None,
        )
    }

    fn lt_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Comparison(Arc::new(a), ComparisonComputation::LessThan, Arc::new(b)),
            None,
        )
    }

    #[test]
    fn flatten_associative_sum_nested() {
        let inner = add_expr(num_expr(1), num_expr(2));
        let expr = add_expr(inner, num_expr(3));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(6, 1))
        ));
    }

    #[test]
    fn flatten_associative_product_nested() {
        let inner = mul_expr(num_expr(2), num_expr(3));
        let expr = mul_expr(inner, num_expr(4));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(24, 1))
        ));
    }

    #[test]
    fn flatten_associative_or_nested() {
        let inner = or_expr(bool_expr(false), bool_expr(false));
        let expr = or_expr(inner, bool_expr(true));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn nested_and_is_not_associative_flattened() {
        // Guard-shaped inner And must stay grouped; flattening inner clauses into one n-ary And
        // would break LogicalAnd evaluation (non-boolean left).
        let inner = and_expr(bool_expr(true), num_expr(7));
        let outer = and_expr(inner, bool_expr(false));
        let nf = to_normal_form(&outer);
        let NormalForm::And(top) = nf else {
            panic!("expected And(..), got {nf:?}");
        };
        assert_eq!(
            top.len(),
            2,
            "nested And must remain 2 children at NF level, got {top:?}"
        );
        assert!(matches!(&top[0], NormalForm::And(inner) if inner.len() == 2));
    }

    #[test]
    fn identity_add_zero() {
        let expr = add_expr(dx(), num_expr(0));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn identity_mul_one() {
        let expr = mul_expr(dx(), num_expr(1));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn identity_mul_zero() {
        let expr = mul_expr(dx(), num_expr(0));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(0, 1))
        ));
    }

    #[test]
    fn identity_pow_one_and_zero() {
        let p1 = normalize_expression(&pow_expr(dx(), num_expr(1)), None).expect("normalize");
        assert!(matches!(p1.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        let p0 = normalize_expression(&pow_expr(dx(), num_expr(0)), None).expect("normalize");
        assert!(matches!(
            p0.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(1, 1))
        ));
    }

    #[test]
    fn double_logical_negation() {
        let norm =
            normalize_expression(&not_expr(not_expr(bool_expr(true))), None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn double_numeric_negation() {
        let zero = num_expr(0);
        let inner = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(zero.clone()),
                ArithmeticComputation::Subtract,
                Arc::new(dx()),
            ),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(zero),
                ArithmeticComputation::Subtract,
                Arc::new(inner),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn double_reciprocal() {
        let one = num_expr(1);
        let inner = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(one.clone()),
                ArithmeticComputation::Divide,
                Arc::new(dx()),
            ),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(one),
                ArithmeticComputation::Divide,
                Arc::new(inner),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn sqrt_squared_numeric_folds_to_exact_rational() {
        let sqrt2 = Expression::with_source(
            ExpressionKind::MathematicalComputation(
                MathematicalComputation::Sqrt,
                Arc::new(num_expr(2)),
            ),
            None,
        );
        let expr = pow_expr(sqrt2, num_expr(2));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(2, 1))
        ));
    }

    /// `2^(1/2)` has no exact rational result: constant_fold must not substitute a decimal.
    #[test]
    fn literal_power_with_irrational_result_stays_power() {
        let half = Expression::with_source(
            ExpressionKind::Literal(Box::new(
                literal_from_folded_rational(RationalInteger::new(1, 2), None)
                    .expect("BUG: literal 1/2 must commit at normalize"),
            )),
            None,
        );
        let expr = pow_expr(num_expr(2), half);
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(
            matches!(
                norm.kind,
                ExpressionKind::Arithmetic(_, ArithmeticComputation::Power, _)
            ),
            "expected symbolic Power, got {:?}",
            norm.kind
        );
    }

    #[test]
    fn de_morgan_not_and() {
        fn dy() -> Expression {
            Expression::with_source(
                ExpressionKind::DataPath(DataPath::new(vec![], "y".into())),
                None,
            )
        }
        let expr = not_expr(and_expr(dx(), dy()));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::LogicalOr(left, right) = norm.kind else {
            panic!("expected Or, got {:?}", norm.kind);
        };
        assert!(matches!(left.kind, ExpressionKind::LogicalNegation(_, _)));
        assert!(matches!(right.kind, ExpressionKind::LogicalNegation(_, _)));
    }

    #[test]
    fn power_law_nested_power() {
        let expr = pow_expr(pow_expr(dx(), num_expr(2)), num_expr(3));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Arithmetic(_, ArithmeticComputation::Power, _)
        ));
        let ExpressionKind::Arithmetic(base, ArithmeticComputation::Power, exp) = norm.kind else {
            unreachable!();
        };
        assert!(matches!(
            base.kind,
            ExpressionKind::DataPath(ref p) if p.data == "x"
        ));
        assert!(matches!(
            exp.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(6, 1))
        ));
    }

    #[test]
    fn power_law_like_base_product() {
        let expr = mul_expr(pow_expr(dx(), num_expr(2)), pow_expr(dx(), num_expr(3)));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::Arithmetic(base, ArithmeticComputation::Power, exp) = norm.kind else {
            panic!("expected single power, got {:?}", norm.kind);
        };
        assert!(matches!(base.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            exp.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(5, 1))
        ));
    }

    #[test]
    fn negated_comparison_not_less_than() {
        let cmp = lt_expr(dx(), num_expr(0));
        let norm = normalize_expression(&not_expr(cmp), None).expect("normalize");
        let ExpressionKind::Comparison(_, op, _) = norm.kind else {
            panic!("expected Comparison, got {:?}", norm.kind);
        };
        assert_eq!(op, ComparisonComputation::GreaterThanOrEqual);
    }

    #[test]
    fn logical_short_circuit_and_false() {
        let expr = and_expr(bool_expr(false), dx());
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
        ));
    }

    #[test]
    fn logical_idempotency_and_duplicate_paths() {
        let a = dx();
        let expr = and_expr(a.clone(), a);
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn unit_conversion_fold_number_to_number() {
        let inner = num_expr(42);
        let expr = Expression::with_source(
            ExpressionKind::UnitConversion(Arc::new(inner), SemanticConversionTarget::Number),
            None,
        );
        let ctx = UnitResolutionContext::NamedQuantityOnly;
        let norm = normalize_expression(&expr, Some(&ctx)).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(42, 1))
        ));
    }

    #[test]
    fn unless_chain_shape_single_branch() {
        let branches = vec![(None as Option<Expression>, num_expr(9))];
        let c = build_unless_chain(&branches);
        assert!(matches!(c.kind, ExpressionKind::Literal(_)));
    }

    #[test]
    fn unless_chain_shape_ordered_or_and() {
        let c_big = lt_expr(num_expr(10), dx());
        let branches = vec![
            (None, num_expr(0)),
            (Some(c_big.clone()), num_expr(100)),
            (Some(lt_expr(num_expr(5), dx())), num_expr(50)),
        ];
        let c = build_unless_chain(&branches);
        // Or( And(c5, 50), Or(And(c10, 100), 0) ) with forward unless iteration
        let ExpressionKind::LogicalOr(outer_left, outer_right) = c.kind else {
            panic!("root Or");
        };
        assert!(matches!(outer_left.kind, ExpressionKind::LogicalAnd(_, _)));
        let ExpressionKind::LogicalOr(ref mid_left, ref mid_right) = outer_right.kind else {
            panic!("right subtree Or");
        };
        assert!(matches!(mid_left.kind, ExpressionKind::LogicalAnd(_, _)));
        assert!(matches!(mid_right.kind, ExpressionKind::Literal(_)));
    }

    #[test]
    fn power_laws_sqrt_squared() {
        let x = Expression::with_source(
            ExpressionKind::DataPath(DataPath::new(vec![], "x".into())),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(Expression::with_source(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Sqrt,
                        Arc::new(x.clone()),
                    ),
                    None,
                )),
                ArithmeticComputation::Multiply,
                Arc::new(Expression::with_source(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Sqrt,
                        Arc::new(x),
                    ),
                    None,
                )),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        let nf = to_normal_form(&norm);
        assert!(
            matches!(
                nf,
                NormalForm::Leaf(LeafKind::Literal(_)) | NormalForm::Leaf(LeafKind::DataPath(_))
            ),
            "sqrt(x)*sqrt(x) should simplify to x, got {:?}",
            format!("{:?}", nf)
        );
    }

    #[test]
    fn exp_log_identity() {
        let inner = Expression::with_source(
            ExpressionKind::MathematicalComputation(MathematicalComputation::Log, Arc::new(dx())),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::MathematicalComputation(MathematicalComputation::Exp, Arc::new(inner)),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn constant_fold_add() {
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(num_expr(2)),
                ArithmeticComputation::Add,
                Arc::new(num_expr(3)),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if *n == RationalInteger::new(5, 1))
        ));
    }
}
