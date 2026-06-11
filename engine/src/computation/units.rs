//! Type casts (`as number`, `as text`, `as eur`, …).

use crate::computation::rational::{
    checked_div, checked_mul, convert_quantity_magnitude_rational, rational_new, rational_one,
    RationalInteger,
};
use crate::evaluation::operations::OperationResult;
use crate::parsing::ast::PrimitiveKind;
use crate::planning::semantics::{
    calendar_unit_factor, primitive_number_arc, primitive_text_arc, LiteralValue,
    SemanticCalendarUnit, SemanticConversionTarget, TypeSpecification, ValueKind,
};
use std::sync::Arc;

/// Describes what type-resolution infrastructure is available at call sites that
/// still resolve unit names from an expression-scope index (arithmetic naming,
/// explanation equivalence across families).
#[derive(Copy, Clone)]
pub enum UnitResolutionContext<'a> {
    WithIndex(
        &'a std::collections::HashMap<
            String,
            std::sync::Arc<crate::planning::semantics::LemmaType>,
        >,
    ),
    NamedQuantityOnly,
}

/// Apply a type cast (`as <target>`).
pub fn convert_unit(value: &LiteralValue, target: &SemanticConversionTarget) -> OperationResult {
    match target {
        SemanticConversionTarget::Type(PrimitiveKind::Number) => cast_to_number(value),
        SemanticConversionTarget::Type(PrimitiveKind::Text) => OperationResult::Value(
            LiteralValue::text_with_type(value.display_value(), primitive_text_arc().clone()),
        ),
        SemanticConversionTarget::Type(PrimitiveKind::Boolean) => {
            if value.lemma_type.is_boolean() {
                OperationResult::Value(value.clone())
            } else {
                unreachable!(
                    "BUG: boolean cast on non-boolean; planning should have rejected {:?}",
                    value.lemma_type.name()
                );
            }
        }
        SemanticConversionTarget::Type(target_kind) => {
            if same_primitive_kind(value, *target_kind) {
                OperationResult::Value(value.clone())
            } else {
                unreachable!(
                    "BUG: invalid identity cast {:?} -> {:?} reached runtime",
                    value.lemma_type.name(),
                    target_kind
                );
            }
        }
        SemanticConversionTarget::Unit {
            unit_name,
            owning_type,
        } => cast_to_unit(value, unit_name, owning_type),
    }
}

fn same_primitive_kind(value: &LiteralValue, target: PrimitiveKind) -> bool {
    matches!(
        (target, &value.lemma_type.specifications),
        (PrimitiveKind::Number, TypeSpecification::Number { .. })
            | (PrimitiveKind::Text, TypeSpecification::Text { .. })
            | (PrimitiveKind::Boolean, TypeSpecification::Boolean { .. })
            | (PrimitiveKind::Date, TypeSpecification::Date { .. })
            | (PrimitiveKind::Time, TypeSpecification::Time { .. })
            | (PrimitiveKind::Ratio, TypeSpecification::Ratio { .. })
            | (PrimitiveKind::Quantity, TypeSpecification::Quantity { .. })
    )
}

fn cast_to_unit(
    value: &LiteralValue,
    unit_name: &str,
    owning_type: &Arc<crate::planning::semantics::LemmaType>,
) -> OperationResult {
    match &value.value {
        ValueKind::Number(magnitude) => {
            cast_number_to_unit(magnitude.clone(), unit_name, owning_type)
        }
        ValueKind::Quantity(magnitude, _) => {
            cast_quantity_to_unit(magnitude.clone(), unit_name, owning_type)
        }
        ValueKind::Range(left, right) => {
            cast_range_span_to_unit(left, right, unit_name, owning_type)
        }
        ValueKind::Ratio(magnitude, from_unit) => cast_ratio_to_unit(
            value,
            magnitude.clone(),
            from_unit.as_deref(),
            unit_name,
            owning_type,
        ),
        other => unreachable!(
            "BUG: unit cast from {:?} should be rejected at planning",
            other
        ),
    }
}

fn cast_number_to_unit(
    magnitude: RationalInteger,
    unit_name: &str,
    owning_type: &Arc<crate::planning::semantics::LemmaType>,
) -> OperationResult {
    if owning_type.is_ratio() {
        return OperationResult::Value(LiteralValue::ratio_with_type(
            magnitude,
            Some(unit_name.to_string()),
            Arc::clone(owning_type),
        ));
    }
    let factor = owning_type.quantity_unit_factor(unit_name).clone();
    let canonical = match checked_mul(&magnitude, &factor) {
        Ok(v) => v,
        Err(failure) => {
            return OperationResult::Veto(crate::evaluation::operations::VetoType::computation(
                failure.to_string(),
            ))
        }
    };
    OperationResult::Value(LiteralValue::quantity_with_type(
        canonical,
        unit_name.to_string(),
        Arc::clone(owning_type),
    ))
}

fn cast_quantity_to_unit(
    magnitude: RationalInteger,
    unit_name: &str,
    owning_type: &Arc<crate::planning::semantics::LemmaType>,
) -> OperationResult {
    OperationResult::Value(LiteralValue::quantity_with_type(
        magnitude,
        unit_name.to_string(),
        Arc::clone(owning_type),
    ))
}

fn cast_ratio_to_unit(
    value: &LiteralValue,
    magnitude: RationalInteger,
    from_unit: Option<&str>,
    unit_name: &str,
    owning_type: &Arc<crate::planning::semantics::LemmaType>,
) -> OperationResult {
    if value.lemma_type.same_quantity_family(owning_type.as_ref()) {
        let from_factor = from_unit
            .map(|u| value.lemma_type.quantity_unit_factor(u).clone())
            .unwrap_or(rational_one());
        let to_factor = owning_type.quantity_unit_factor(unit_name).clone();
        let converted =
            match convert_quantity_magnitude_rational(magnitude, &from_factor, &to_factor) {
                Ok(m) => m,
                Err(failure) => {
                    return OperationResult::Veto(
                        crate::evaluation::operations::VetoType::computation(failure.to_string()),
                    );
                }
            };
        OperationResult::Value(LiteralValue::ratio_with_type(
            converted,
            Some(unit_name.to_string()),
            Arc::clone(owning_type),
        ))
    } else {
        OperationResult::Value(LiteralValue::ratio_with_type(
            magnitude,
            Some(unit_name.to_string()),
            Arc::clone(owning_type),
        ))
    }
}

fn cast_range_span_to_unit(
    left: &LiteralValue,
    right: &LiteralValue,
    unit_name: &str,
    owning_type: &Arc<crate::planning::semantics::LemmaType>,
) -> OperationResult {
    if let (ValueKind::Date(left_date), ValueKind::Date(right_date)) = (&left.value, &right.value) {
        if calendar_unit_factor(unit_name).is_some() {
            return super::datetime::compute_date_calendar_difference(
                left_date,
                right_date,
                &semantic_calendar_unit(unit_name),
                Arc::clone(owning_type),
            );
        }
        let OperationResult::Value(span) = super::range::compute_span(left, right) else {
            return super::range::compute_span(left, right);
        };
        return convert_span_quantity_to_unit(&span, unit_name, owning_type);
    }

    let OperationResult::Value(span) = super::range::compute_span(left, right) else {
        return super::range::compute_span(left, right);
    };
    let span = &span;
    match &span.value {
        ValueKind::Quantity(_, _) => convert_span_quantity_to_unit(span, unit_name, owning_type),
        ValueKind::Ratio(_, _) => {
            let ValueKind::Ratio(magnitude, from_unit) = &span.value else {
                unreachable!("BUG: ratio span expected");
            };
            cast_ratio_to_unit(
                span,
                magnitude.clone(),
                from_unit.as_deref(),
                unit_name,
                owning_type,
            )
        }
        ValueKind::Number(magnitude) => {
            cast_number_to_unit(magnitude.clone(), unit_name, owning_type)
        }
        other => unreachable!("BUG: unexpected range span value kind for unit cast: {other:?}"),
    }
}

fn convert_span_quantity_to_unit(
    span: &LiteralValue,
    unit_name: &str,
    owning_type: &Arc<crate::planning::semantics::LemmaType>,
) -> OperationResult {
    let ValueKind::Quantity(magnitude, _) = &span.value else {
        unreachable!("BUG: span quantity expected");
    };
    cast_quantity_to_unit(magnitude.clone(), unit_name, owning_type)
}

fn semantic_calendar_unit(unit_name: &str) -> SemanticCalendarUnit {
    match unit_name {
        "month" | "months" => SemanticCalendarUnit::Month,
        "year" | "years" => SemanticCalendarUnit::Year,
        other => unreachable!("BUG: unknown calendar unit '{other}' after planning"),
    }
}

fn cast_to_number(value: &LiteralValue) -> OperationResult {
    match &value.value {
        ValueKind::Range(left, right) => {
            let span = super::range::compute_span(left, right);
            let OperationResult::Value(span_value) = span else {
                return span;
            };
            cast_to_number(&span_value)
        }
        ValueKind::Quantity(magnitude, signature) if value.lemma_type.is_calendar_like() => {
            let unit_name = signature
                .first()
                .map(|(name, _)| name.as_str())
                .expect("BUG: calendar quantity must carry a unit signature");
            let factor = value.lemma_type.quantity_unit_factor(unit_name).clone();
            let in_unit = checked_div(magnitude, &factor)
                .expect("BUG: calendar de-canonicalization by unit factor must not fail");
            OperationResult::Value(LiteralValue::number_with_type(
                in_unit,
                primitive_number_arc().clone(),
            ))
        }
        ValueKind::Number(number) => OperationResult::Value(LiteralValue::number_with_type(
            number.clone(),
            primitive_number_arc().clone(),
        )),
        ValueKind::Boolean(b) => {
            let n = if *b {
                rational_new(1, 1)
            } else {
                rational_new(0, 1)
            };
            OperationResult::Value(LiteralValue::number_with_type(
                n,
                primitive_number_arc().clone(),
            ))
        }
        ValueKind::Ratio(rational_value, _) => OperationResult::Value(
            LiteralValue::number_with_type(rational_value.clone(), primitive_number_arc().clone()),
        ),
        ValueKind::Quantity(magnitude, signature) => {
            let factor = match signature.as_slice() {
                [(unit_name, 1)] => value.lemma_type.quantity_unit_factor(unit_name).clone(),
                [] => rational_one(),
                _ => panic!(
                    "BUG: cast_to_number with compound signature must be rejected at planning"
                ),
            };
            let in_unit = checked_div(magnitude, &factor)
                .expect("BUG: de-canonicalization by unit factor must not fail");
            OperationResult::Value(LiteralValue::number_with_type(
                in_unit,
                primitive_number_arc().clone(),
            ))
        }
        ValueKind::Text(_) | ValueKind::Date(_) | ValueKind::Time(_) => unreachable!(
            "BUG: cast to number from {:?} should be rejected at planning",
            value.lemma_type.name()
        ),
    }
}

pub(crate) fn conversion_target_declares_unit(
    target: &SemanticConversionTarget,
) -> Option<(&str, &Arc<crate::planning::semantics::LemmaType>)> {
    match target {
        SemanticConversionTarget::Unit {
            unit_name,
            owning_type,
        } => Some((unit_name.as_str(), owning_type)),
        SemanticConversionTarget::Type(_) => None,
    }
}

pub(crate) fn owning_type_declares_unit_name(
    owning_type: &crate::planning::semantics::LemmaType,
    unit_name: &str,
) -> bool {
    match &owning_type.specifications {
        TypeSpecification::Quantity { units, .. } => units.get(unit_name).is_ok(),
        TypeSpecification::Ratio { units, .. } => units.get(unit_name).is_ok(),
        _ => false,
    }
}
