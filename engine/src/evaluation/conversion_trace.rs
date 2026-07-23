//! Structured explanation steps for unit conversions (`as`).

use crate::computation::rational::{checked_div, RationalInteger};
use crate::planning::explanation::{ConversionTraceRole, SerializedConversionTraceStep};
use crate::planning::semantics::{
    compare_semantic_dates, DataPath, LemmaType, LiteralValue, SemanticConversionTarget,
    TypeSpecification, ValueKind,
};
use std::cmp::Ordering;

/// Build ordered explanation steps (outcome → rule → source) after a successful unit conversion.
///
/// When the source and target unit are the same (identity), the Rule step is omitted.
pub(crate) fn build_conversion_steps(
    value: &LiteralValue,
    target: &SemanticConversionTarget,
    result: &LiteralValue,
    data_ref: Option<&DataPath>,
) -> Vec<SerializedConversionTraceStep> {
    let mut steps = Vec::new();
    steps.push(SerializedConversionTraceStep {
        role: ConversionTraceRole::Outcome,
        text: result.display_value(),
    });

    if let Some(rule_text) = conversion_rule_step_text(value, target, result) {
        steps.push(SerializedConversionTraceStep {
            role: ConversionTraceRole::Rule,
            text: rule_text,
        });
    }

    steps.push(SerializedConversionTraceStep {
        role: ConversionTraceRole::Source,
        text: conversion_source_step_text(value, data_ref),
    });

    steps
}

fn conversion_source_step_text(operand: &LiteralValue, data_ref: Option<&DataPath>) -> String {
    let type_name = type_specification_display_name(&operand.lemma_type);
    let value_display = operand.display_value();
    match data_ref {
        Some(path) => format!("The {type_name} of {path} is {value_display}"),
        None => format!("The {type_name} is {value_display}"),
    }
}

fn type_specification_display_name(lemma_type: &LemmaType) -> &'static str {
    match &lemma_type.specifications {
        TypeSpecification::Boolean { .. } => "boolean",
        TypeSpecification::Measure { .. } => "measure",
        TypeSpecification::MeasureRange { .. } => "measure range",
        TypeSpecification::Number { .. } => "number",
        TypeSpecification::NumberRange { .. } => "number range",
        TypeSpecification::Text { .. } => "text",
        TypeSpecification::Date { .. } => "date",
        TypeSpecification::DateRange { .. } => "date range",
        TypeSpecification::TimeRange { .. } => "time range",
        TypeSpecification::Time { .. } => "time",
        TypeSpecification::Ratio { .. } => "ratio",
        TypeSpecification::RatioRange { .. } => "ratio range",
        TypeSpecification::Veto { .. } => "veto",
        TypeSpecification::Undetermined => "undetermined",
    }
}

fn conversion_rule_step_text(
    value: &LiteralValue,
    target: &SemanticConversionTarget,
    result: &LiteralValue,
) -> Option<String> {
    match &value.value {
        ValueKind::Range(left, right) => range_span_rule_step_text(left, right, result),
        ValueKind::Measure(_, from_signature) if !value.lemma_type.is_calendar_like() => {
            match target {
                SemanticConversionTarget::Unit {
                    unit_name,
                    owning_type,
                } => measure_unit_equivalence_step_text(from_signature, unit_name, owning_type),
                _ => None,
            }
        }
        ValueKind::Number(_) | ValueKind::Ratio(_, _) => None,
        ValueKind::Measure(_, _) if value.lemma_type.is_calendar_like() => None,
        _ => None,
    }
}

fn range_span_rule_step_text(
    left: &LiteralValue,
    right: &LiteralValue,
    result: &LiteralValue,
) -> Option<String> {
    match (&left.value, &right.value) {
        (ValueKind::Date(left_date), ValueKind::Date(right_date)) => {
            let (lower, upper) = ordered_date_pair(left_date, right_date);
            let lower_literal = LiteralValue::date(lower.clone());
            let upper_literal = LiteralValue::date(upper.clone());
            Some(format!(
                "{} − {} = {}",
                upper_literal.display_value(),
                lower_literal.display_value(),
                result.display_value()
            ))
        }
        (ValueKind::Number(_), ValueKind::Number(_)) => {
            let (lower, upper) = ordered_number_pair(left, right);
            Some(format!(
                "{} − {} = {}",
                upper.display_value(),
                lower.display_value(),
                result.display_value()
            ))
        }
        (ValueKind::Measure(_, _), ValueKind::Measure(_, _)) => {
            let (lower, upper) = ordered_measure_pair(left, right);
            Some(format!(
                "{} − {} = {}",
                upper.display_value(),
                lower.display_value(),
                result.display_value()
            ))
        }
        _ => None,
    }
}

fn ordered_date_pair<'a>(
    left: &'a crate::planning::semantics::SemanticDateTime,
    right: &'a crate::planning::semantics::SemanticDateTime,
) -> (
    &'a crate::planning::semantics::SemanticDateTime,
    &'a crate::planning::semantics::SemanticDateTime,
) {
    match compare_semantic_dates(left, right) {
        Ordering::Less | Ordering::Equal => (left, right),
        Ordering::Greater => (right, left),
    }
}

fn ordered_number_pair<'a>(
    left: &'a LiteralValue,
    right: &'a LiteralValue,
) -> (&'a LiteralValue, &'a LiteralValue) {
    let ValueKind::Number(left_number) = &left.value else {
        unreachable!("BUG: ordered_number_pair called with non-number operand");
    };
    let ValueKind::Number(right_number) = &right.value else {
        unreachable!("BUG: ordered_number_pair called with non-number operand");
    };
    if left_number <= right_number {
        (left, right)
    } else {
        (right, left)
    }
}

fn ordered_measure_pair<'a>(
    left: &'a LiteralValue,
    right: &'a LiteralValue,
) -> (&'a LiteralValue, &'a LiteralValue) {
    let ValueKind::Measure(left_magnitude, _) = &left.value else {
        unreachable!("BUG: ordered_measure_pair called with non-measure operand");
    };
    let ValueKind::Measure(right_magnitude, _) = &right.value else {
        unreachable!("BUG: ordered_measure_pair called with non-measure operand");
    };
    if *left_magnitude <= *right_magnitude {
        (left, right)
    } else {
        (right, left)
    }
}

fn format_explanation_multiplier(rational: &RationalInteger) -> String {
    rational.display_str()
}

/// Produce the "1 {source_unit} is {multiplier} {target_unit}" Rule step text.
///
/// Same-family conversions only: when `from_unit` is absent from `owning_type`
/// (cross-family relabel), there is no factor equivalence to narrate.
fn measure_unit_equivalence_step_text(
    from_signature: &[(String, i32)],
    to_unit: &str,
    owning_type: &LemmaType,
) -> Option<String> {
    let from_unit = from_signature
        .first()
        .map(|(name, _)| name.as_str())
        .unwrap_or("");
    if from_unit.is_empty() || from_signature.len() != 1 {
        return None;
    }

    let units = match &owning_type.specifications {
        TypeSpecification::Measure { units, .. }
        | TypeSpecification::MeasureRange { units, .. } => units,
        _ => return None,
    };
    let from_factor = &units.get(from_unit).ok()?.factor;
    let to_factor = &units.get(to_unit).ok()?.factor;
    let multiplier = checked_div(from_factor, to_factor).ok()?;
    let multiplier_display = format_explanation_multiplier(&multiplier);
    if multiplier_display == "1" {
        return None;
    }
    Some(format!("1 {from_unit} is {multiplier_display} {to_unit}"))
}
