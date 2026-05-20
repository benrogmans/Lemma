//! Unit conversion system
//!
//! Handles quantity unit conversion and calendar conversions.
//! Uses only planning/semantic types; no dependency on parsing types.

use crate::computation::rational::{
    convert_quantity_magnitude_rational, rational_one, NumericFailure, RationalInteger,
};
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::planning::semantics::{
    primitive_number, LemmaType, LiteralValue, SemanticCalendarUnit, SemanticConversionTarget,
    ValueKind,
};
use std::collections::HashMap;

/// Describes what type-resolution infrastructure is available at a `convert_unit` call site.
#[derive(Copy, Clone)]
pub enum UnitResolutionContext<'a> {
    WithIndex(&'a HashMap<String, LemmaType>),
    NamedQuantityOnly,
}

fn quantity_unit_from_index<'a>(
    unit_index: &'a HashMap<String, LemmaType>,
    unit: &str,
    context: &str,
) -> &'a LemmaType {
    unit_index.get(unit).unwrap_or_else(|| {
        unreachable!(
            "BUG: unit '{}' missing from unit_index during {} after planning",
            unit, context
        )
    })
}

fn numeric_failure_to_veto(failure: NumericFailure) -> VetoType {
    VetoType::computation(failure.to_string())
}

fn convert_quantity_scalar_to_unit(
    value: &LiteralValue,
    from_unit: &str,
    to_unit: &str,
    resolution_context: UnitResolutionContext<'_>,
) -> OperationResult {
    let ValueKind::Quantity(magnitude, _, _) = &value.value else {
        panic!("BUG: convert_quantity_scalar_to_unit called with non-quantity value");
    };
    let (from_factor, to_factor) = match resolution_context {
        UnitResolutionContext::WithIndex(unit_index) => {
            if from_unit.is_empty() {
                let target_type =
                    quantity_unit_from_index(unit_index, to_unit, "quantity unit conversion");
                let to_factor = target_type.quantity_unit_factor(to_unit);
                if value.lemma_type.is_anonymous_quantity() {
                    (rational_one(), *to_factor)
                } else {
                    let from_unit_name = crate::evaluation::conversion_explanation::infer_anonymous_quantity_unit_name(
                        &value.lemma_type,
                        unit_index,
                        to_unit,
                    )
                    .unwrap_or_else(|| {
                        panic!(
                            "BUG: anonymous quantity unit name missing after planning for conversion to '{to_unit}'"
                        )
                    });
                    let from_factor = *target_type.quantity_unit_factor(&from_unit_name);
                    (from_factor, *to_factor)
                }
            } else {
                let from_factor = *value.lemma_type.quantity_unit_factor(from_unit);
                let to_factor = if !value.lemma_type.is_anonymous_quantity() {
                    *value.lemma_type.quantity_unit_factor(to_unit)
                } else {
                    let target_type =
                        quantity_unit_from_index(unit_index, to_unit, "quantity unit conversion");
                    *target_type.quantity_unit_factor(to_unit)
                };
                (from_factor, to_factor)
            }
        }
        UnitResolutionContext::NamedQuantityOnly => {
            let from_factor = if from_unit.is_empty() && value.lemma_type.is_anonymous_quantity() {
                rational_one()
            } else {
                *value.lemma_type.quantity_unit_factor(from_unit)
            };
            let to_factor = *value.lemma_type.quantity_unit_factor(to_unit);
            (from_factor, to_factor)
        }
    };
    let converted_magnitude =
        match convert_quantity_magnitude_rational(*magnitude, &from_factor, &to_factor) {
            Ok(decimal) => decimal,
            Err(failure) => return OperationResult::Veto(numeric_failure_to_veto(failure)),
        };
    let target_type = match resolution_context {
        UnitResolutionContext::WithIndex(unit_index) => {
            quantity_unit_from_index(unit_index, to_unit, "quantity unit conversion").clone()
        }
        UnitResolutionContext::NamedQuantityOnly => value.lemma_type.clone(),
    };
    OperationResult::Value(Box::new(LiteralValue::quantity_with_type(
        converted_magnitude,
        to_unit.to_string(),
        target_type,
    )))
}

/// Convert a value to a target unit (for `as` operator).
pub fn convert_unit(
    value: &LiteralValue,
    target: &SemanticConversionTarget,
    resolution_context: UnitResolutionContext<'_>,
) -> OperationResult {
    match &value.value {
        ValueKind::Range(left, right) => match (&left.value, &right.value, target) {
            (ValueKind::Date(_), ValueKind::Date(_), SemanticConversionTarget::Number) => {
                let span = super::range::compute_span(left, right);
                let OperationResult::Value(span_value) = span else {
                    return span;
                };
                let magnitude = match &span_value.value {
                    ValueKind::Number(n) => *n,
                    ValueKind::Quantity(magnitude, _, _) => *magnitude,
                    other => unreachable!(
                        "BUG: date range span for number conversion must be number or quantity, got {other:?}"
                    ),
                };
                OperationResult::Value(Box::new(LiteralValue::number_with_type(
                    magnitude,
                    primitive_number().clone(),
                )))
            }
            (
                ValueKind::Date(_),
                ValueKind::Date(_),
                SemanticConversionTarget::QuantityUnit(_),
            ) => {
                let span = super::range::compute_span(left, right);
                let OperationResult::Value(span_value) = span else {
                    return span;
                };
                convert_unit(span_value.as_ref(), target, resolution_context)
            }
            (
                ValueKind::Date(left_date),
                ValueKind::Date(right_date),
                SemanticConversionTarget::Calendar(target_unit),
            ) => super::datetime::compute_date_calendar_difference(
                left_date,
                right_date,
                target_unit,
            ),
            (ValueKind::Date(..), ValueKind::Date(..), _) => unreachable!(
                "BUG: unsupported date range conversion target; planning should have rejected this"
            ),
            _ => {
                let span = super::range::compute_span(left, right);
                let OperationResult::Value(span_value) = span else {
                    return span;
                };
                convert_unit(span_value.as_ref(), target, resolution_context)
            }
        },

        ValueKind::Calendar(v, from) => match target {
            SemanticConversionTarget::Number => OperationResult::Value(Box::new(
                LiteralValue::number_with_type(*v, primitive_number().clone()),
            )),
            SemanticConversionTarget::Calendar(to) => {
                let val = convert_calendar(*v, from, to);
                OperationResult::Value(Box::new(LiteralValue::calendar_with_type(
                    val,
                    to.clone(),
                    value.lemma_type.clone(),
                )))
            }
            _ => unreachable!(
                "BUG: invalid conversion target {:?} for calendar; this should be rejected during planning",
                target
            ),
        },

        ValueKind::Number(number) => match target {
            SemanticConversionTarget::Number => OperationResult::Value(Box::new(
                LiteralValue::number_with_type(*number, primitive_number().clone()),
            )),
            SemanticConversionTarget::Calendar(unit) => {
                OperationResult::Value(Box::new(LiteralValue::calendar(*number, unit.clone())))
            }
            SemanticConversionTarget::QuantityUnit(unit_name) => {
                match resolution_context {
                    UnitResolutionContext::WithIndex(unit_index) => {
                        let target_type = quantity_unit_from_index(
                            unit_index,
                            unit_name,
                            "number-to-quantity conversion",
                        );
                        OperationResult::Value(Box::new(LiteralValue::quantity_with_type(
                            *number,
                            unit_name.clone(),
                            target_type.clone(),
                        )))
                    }
                    UnitResolutionContext::NamedQuantityOnly => unreachable!(
                        "BUG: number-to-quantity-unit conversion for '{}' reached a call site \
                         that declared NamedQuantityOnly context. This is a planning inconsistency.",
                        unit_name
                    ),
                }
            }
            SemanticConversionTarget::RatioUnit(unit) => {
                OperationResult::Value(Box::new(LiteralValue::ratio(*number, Some(unit.clone()))))
            }
        },

        ValueKind::Ratio(rational_value, _from_unit_opt) => match target {
            SemanticConversionTarget::Number => OperationResult::Value(Box::new(
                LiteralValue::number_with_type(*rational_value, primitive_number().clone()),
            )),
            SemanticConversionTarget::RatioUnit(to_unit) => OperationResult::Value(Box::new(
                LiteralValue::ratio(*rational_value, Some(to_unit.clone())),
            )),
            _ => unreachable!(
                "BUG: invalid conversion target {:?} for ratio; this should be rejected during planning",
                target
            ),
        },

        ValueKind::Quantity(_, from_unit, _) => match target {
            SemanticConversionTarget::Number => {
                let ValueKind::Quantity(magnitude, _, _) = &value.value else {
                    unreachable!("BUG: matched Quantity arm with non-quantity value");
                };
                OperationResult::Value(Box::new(LiteralValue::number_with_type(
                    *magnitude,
                    primitive_number().clone(),
                )))
            }
            SemanticConversionTarget::QuantityUnit(to_unit) => {
                convert_quantity_scalar_to_unit(value, from_unit, to_unit, resolution_context)
            }
            SemanticConversionTarget::Calendar(_) => unreachable!(
                "BUG: cannot convert quantity to calendar; this should be rejected during planning"
            ),
            SemanticConversionTarget::RatioUnit(_) => unreachable!(
                "BUG: cannot convert quantity to ratio unit; this should be rejected during planning"
            ),
        },

        _ => unreachable!(
            "BUG: unsupported unit conversion during evaluation: {} -> {}",
            value,
            target
        ),
    }
}

pub(crate) fn convert_calendar_magnitude(
    value: RationalInteger,
    from: &SemanticCalendarUnit,
    to: &SemanticCalendarUnit,
) -> RationalInteger {
    convert_calendar(value, from, to)
}

fn convert_calendar(
    value: RationalInteger,
    from: &SemanticCalendarUnit,
    to: &SemanticCalendarUnit,
) -> RationalInteger {
    use crate::computation::rational::{rational_one, RationalInteger};
    if from == to {
        return value;
    }
    let from_factor = match from {
        SemanticCalendarUnit::Month => rational_one(),
        SemanticCalendarUnit::Year => RationalInteger::new(12, 1),
    };
    let to_factor = match to {
        SemanticCalendarUnit::Month => rational_one(),
        SemanticCalendarUnit::Year => RationalInteger::new(12, 1),
    };
    convert_quantity_magnitude_rational(value, &from_factor, &to_factor)
        .expect("BUG: calendar unit conversion failed after planning")
}
