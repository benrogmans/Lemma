//! Structured explanation steps for unit conversions (`as`).

use crate::computation::rational::{checked_div, RationalInteger};
use crate::computation::UnitResolutionContext;
use crate::evaluation::evaluation_trace::{ConversionTraceRole, ConversionTraceStep};
use crate::planning::semantics::{
    compare_semantic_dates, DataPath, LemmaType, LiteralValue, SemanticConversionTarget,
    TypeSpecification, ValueKind,
};
use std::cmp::Ordering;

/// Build ordered explanation steps (outcome → rule → source) after a successful unit conversion.
///
/// When the source and target unit are the same (identity conversion), the Rule step is omitted
/// and the result contains exactly two steps: Outcome and Source.
pub fn build_conversion_steps(
    value: &LiteralValue,
    target: &SemanticConversionTarget,
    result: &LiteralValue,
    data_ref: Option<&DataPath>,
    resolution_context: UnitResolutionContext<'_>,
) -> Vec<ConversionTraceStep> {
    let mut steps = Vec::new();
    steps.push(ConversionTraceStep {
        role: ConversionTraceRole::Outcome,
        text: result.to_string(),
        data_ref: None,
    });

    if let Some(rule_text) = conversion_rule_step_text(value, target, result, resolution_context) {
        steps.push(ConversionTraceStep {
            role: ConversionTraceRole::Rule,
            text: rule_text,
            data_ref: None,
        });
    }

    steps.push(ConversionTraceStep {
        role: ConversionTraceRole::Source,
        text: conversion_source_step_text(value, data_ref),
        data_ref: data_ref.cloned(),
    });

    steps
}

fn conversion_source_step_text(operand: &LiteralValue, data_ref: Option<&DataPath>) -> String {
    let type_name = type_specification_display_name(&operand.lemma_type);
    let value_display = operand.to_string();
    match data_ref {
        Some(path) => format!("The {type_name} of {path} is {value_display}"),
        None => format!("The {type_name} is {value_display}"),
    }
}

fn type_specification_display_name(lemma_type: &LemmaType) -> &'static str {
    match &lemma_type.specifications {
        TypeSpecification::Boolean { .. } => "boolean",
        TypeSpecification::Quantity { .. } => "quantity",
        TypeSpecification::QuantityRange { .. } => "quantity range",
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
    resolution_context: UnitResolutionContext<'_>,
) -> Option<String> {
    match &value.value {
        ValueKind::Range(left, right) => range_span_rule_step_text(left, right, result),
        ValueKind::Quantity(_, from_signature) if !value.lemma_type.is_calendar_like() => {
            match target {
                SemanticConversionTarget::Unit { unit_name } => {
                    quantity_unit_equivalence_step_text(
                        from_signature,
                        unit_name,
                        &value.lemma_type,
                        resolution_context,
                    )
                }
                _ => None,
            }
        }
        ValueKind::Number(_) => None,
        ValueKind::Ratio(_, _) => None,
        ValueKind::Quantity(_, _) if value.lemma_type.is_calendar_like() => None,
        _ => None,
    }
}

fn format_explanation_multiplier(rational: &RationalInteger) -> String {
    let reduced = rational.reduced();
    if *reduced.denom() == 1 {
        reduced.numer().to_string()
    } else {
        format!("{}/{}", reduced.numer(), reduced.denom())
    }
}

/// Produce the "1 {source_unit} is {multiplier} {target_unit}" Rule step text.
///
/// Returns `None` when the multiplier is 1 (identity conversion — no Rule step needed)
/// or when the required unit factors cannot be resolved from the context.
fn quantity_unit_equivalence_step_text(
    from_signature: &[(String, i32)],
    to_unit: &str,
    lemma_type: &LemmaType,
    resolution_context: UnitResolutionContext<'_>,
) -> Option<String> {
    let from_unit = from_signature
        .first()
        .map(|(name, _)| name.as_str())
        .unwrap_or("");

    let both_units_in_lemma_type = match &lemma_type.specifications {
        TypeSpecification::Quantity { units, .. } => {
            !from_unit.is_empty()
                && from_signature.len() == 1
                && units.get(from_unit).is_ok()
                && units.get(to_unit).is_ok()
        }
        _ => false,
    };

    if both_units_in_lemma_type {
        let from_factor = lemma_type.quantity_unit_factor(from_unit);
        let to_factor = lemma_type.quantity_unit_factor(to_unit);
        let multiplier = checked_div(from_factor, to_factor).ok()?;
        let multiplier_display = format_explanation_multiplier(&multiplier);
        if multiplier_display == "1" {
            return None;
        }
        return Some(format!("1 {from_unit} is {multiplier_display} {to_unit}"));
    }

    let UnitResolutionContext::WithIndex(unit_index) = resolution_context else {
        return None;
    };
    let target_type = unit_index.get(to_unit)?;
    let to_factor = *target_type.quantity_unit_factor(to_unit);
    let from_factor =
        crate::planning::semantics::signature_factor(from_signature, unit_index, None);
    let multiplier = checked_div(&from_factor, &to_factor).ok()?;
    let multiplier_display = format_explanation_multiplier(&multiplier);
    if multiplier_display == "1" {
        return None;
    }
    let source_label = crate::planning::semantics::format_signature_operator_style(from_signature);
    Some(format!(
        "1 {source_label} is {multiplier_display} {to_unit}"
    ))
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
            Some(format!("{upper_literal} − {lower_literal} = {result}"))
        }
        (ValueKind::Number(_), ValueKind::Number(_)) => {
            let (lower, upper) = ordered_number_pair(left, right);
            Some(format!("{upper} − {lower} = {result}"))
        }
        (ValueKind::Quantity(_, _), ValueKind::Quantity(_, _)) => {
            let (lower, upper) = ordered_quantity_pair(left, right);
            Some(format!("{upper} − {lower} = {result}"))
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

fn ordered_quantity_pair<'a>(
    left: &'a LiteralValue,
    right: &'a LiteralValue,
) -> (&'a LiteralValue, &'a LiteralValue) {
    let ValueKind::Quantity(left_magnitude, _) = &left.value else {
        unreachable!("BUG: ordered_quantity_pair called with non-quantity operand");
    };
    let ValueKind::Quantity(right_magnitude, _) = &right.value else {
        unreachable!("BUG: ordered_quantity_pair called with non-quantity operand");
    };
    if *left_magnitude <= *right_magnitude {
        (left, right)
    } else {
        (right, left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::RationalInteger;
    use crate::literals::QuantityUnit;
    use crate::parsing::ast::DateTimeValue;
    use crate::planning::semantics::{date_time_to_semantic, QuantityUnits};
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn conversion_source_step_text_with_data_reference() {
        let operand = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "kilogram".to_string(),
            Arc::new(LemmaType::primitive(TypeSpecification::quantity())),
        );
        let path = DataPath::local("mass".to_string());
        let text = conversion_source_step_text(&operand, Some(&path));
        assert_eq!(text, "The quantity of mass is 2 kilogram");
    }

    #[test]
    fn build_conversion_steps_scalar_quantity() {
        let mut units = QuantityUnits::new();
        units.0.push(
            QuantityUnit::from_decimal_factor("kilogram".to_string(), Decimal::ONE, vec![])
                .unwrap(),
        );
        units.0.push(
            QuantityUnit::from_decimal_factor("gram".to_string(), Decimal::new(1, 3), vec![])
                .unwrap(),
        );
        let lemma_type = Arc::new(LemmaType::primitive(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units,
            traits: vec![],
            decomposition: Default::default(),
            help: String::new(),
        }));
        let operand = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "kilogram".to_string(),
            Arc::clone(&lemma_type),
        );
        let result = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "gram".to_string(),
            lemma_type,
        );
        let path = DataPath::local("mass".to_string());
        let steps = build_conversion_steps(
            &operand,
            &SemanticConversionTarget::Unit {
                unit_name: "gram".to_string(),
            },
            &result,
            Some(&path),
            UnitResolutionContext::NamedQuantityOnly,
        );
        assert_eq!(steps.len(), 3);
        assert!(matches!(steps[0].role, ConversionTraceRole::Outcome));
        assert_eq!(steps[0].text, "2 kilogram");
        assert!(matches!(steps[1].role, ConversionTraceRole::Rule));
        assert_eq!(steps[1].text, "1 kilogram is 1000 gram");
        assert!(matches!(steps[2].role, ConversionTraceRole::Source));
        assert_eq!(steps[2].text, "The quantity of mass is 2 kilogram");
        assert_eq!(steps[2].data_ref, Some(path));
    }

    #[test]
    fn build_conversion_steps_date_range() {
        let left = LiteralValue::date(date_time_to_semantic(&DateTimeValue {
            year: 2024,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        }));
        let right = LiteralValue::date(date_time_to_semantic(&DateTimeValue {
            year: 2024,
            month: 6,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        }));
        let range = LiteralValue {
            value: ValueKind::Range(Box::new(left), Box::new(right)),
            lemma_type: Arc::new(LemmaType::primitive(TypeSpecification::date_range())),
        };
        let result = LiteralValue::quantity_with_type(
            RationalInteger::new(14, 1),
            "days".to_string(),
            Arc::new(LemmaType::primitive(TypeSpecification::quantity())),
        );
        let path = DataPath::local("age".to_string());
        let steps = build_conversion_steps(
            &range,
            &SemanticConversionTarget::Unit {
                unit_name: "days".to_string(),
            },
            &result,
            Some(&path),
            UnitResolutionContext::WithIndex(&HashMap::new()),
        );
        assert_eq!(steps.len(), 3);
        assert!(steps[1].text.contains('−'));
        assert!(steps[1].text.contains("2024-06-15"));
        assert!(steps[1].text.contains("2024-06-01"));
        assert!(steps[1].text.contains("14"));
        assert!(steps[2].text.contains("The date range of age is"));
    }

    #[test]
    fn build_conversion_steps_identity_omits_rule() {
        let mut units = QuantityUnits::new();
        units.0.push(
            QuantityUnit::from_decimal_factor("kilogram".to_string(), Decimal::ONE, vec![])
                .unwrap(),
        );
        let lemma_type = Arc::new(LemmaType::primitive(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units,
            traits: vec![],
            decomposition: Default::default(),
            help: String::new(),
        }));
        let operand = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "kilogram".to_string(),
            Arc::clone(&lemma_type),
        );
        let result = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "kilogram".to_string(),
            lemma_type,
        );
        let steps = build_conversion_steps(
            &operand,
            &SemanticConversionTarget::Unit {
                unit_name: "kilogram".to_string(),
            },
            &result,
            None,
            UnitResolutionContext::NamedQuantityOnly,
        );
        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[0].role, ConversionTraceRole::Outcome));
        assert!(matches!(steps[1].role, ConversionTraceRole::Source));
    }

    #[test]
    fn conversion_trace_step_roundtrip() {
        let step = ConversionTraceStep {
            role: ConversionTraceRole::Rule,
            text: "1 kilogram is 1000 gram".to_string(),
            data_ref: Some(DataPath::local("mass".to_string())),
        };
        assert_eq!(step.text, "1 kilogram is 1000 gram");
        assert!(matches!(step.role, ConversionTraceRole::Rule));
    }

    /// A Quantity value with a compound anonymous signature (e.g. `eur*hour/minute`) converted
    /// `as eur` must produce an explanation whose multiplier is computed via `signature_factor`,
    /// not via a single canonical-unit lookup. This test exercises the anonymous path in
    /// `quantity_unit_equivalence_step_text`.
    #[test]
    fn explanation_for_compound_signature_uses_signature_factor() {
        use crate::engine::Engine;
        use crate::parsing::source::SourceType;
        use std::path::PathBuf;
        let code = r#"spec t
uses lemma units
data money: quantity
  -> unit eur 1
data rate: quantity
  -> unit eur_per_minute eur/minute
data r: 40 eur_per_minute
data h: 2 hour
rule cost: (r * h) as eur
"#;
        let mut engine = Engine::new();
        engine
            .load(code, SourceType::Path(Arc::new(PathBuf::from("t.lemma"))))
            .expect("must load");
        let response = engine
            .run(None, "t", None, HashMap::new(), false)
            .expect("must eval");
        let cost_result = response.results.get("cost").expect("rule must exist");
        let display = cost_result
            .display
            .as_deref()
            .expect("must have display value");
        assert!(
            display.contains("4800") && display.contains("eur"),
            "expected 4800 eur, got: {display}"
        );
    }
}
