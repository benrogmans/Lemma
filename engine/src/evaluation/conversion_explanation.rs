//! Structured explanation steps for unit conversions (`as`).

use crate::computation::rational::{checked_div, RationalInteger};
use crate::computation::UnitResolutionContext;
use crate::evaluation::explanation::{ConversionExplanationRole, ConversionExplanationStep};
use crate::planning::semantics::{
    compare_semantic_dates, DataPath, LemmaType, LiteralValue, SemanticConversionTarget,
    TypeSpecification, ValueKind,
};
use std::cmp::Ordering;

/// Build ordered explanation steps (outcome → rule → source) after a successful unit conversion.
pub fn build_conversion_steps(
    value: &LiteralValue,
    target: &SemanticConversionTarget,
    result: &LiteralValue,
    data_ref: Option<&DataPath>,
    resolution_context: UnitResolutionContext<'_>,
) -> Vec<ConversionExplanationStep> {
    let mut steps = Vec::new();
    steps.push(ConversionExplanationStep {
        role: ConversionExplanationRole::Outcome,
        text: result.to_string(),
        data_ref: None,
    });

    if let Some(rule_text) = conversion_rule_step_text(value, target, result, resolution_context) {
        steps.push(ConversionExplanationStep {
            role: ConversionExplanationRole::Rule,
            text: rule_text,
            data_ref: None,
        });
    }

    steps.push(ConversionExplanationStep {
        role: ConversionExplanationRole::Source,
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
        TypeSpecification::Time { .. } => "time",
        TypeSpecification::Calendar { .. } => "calendar",
        TypeSpecification::CalendarRange { .. } => "calendar range",
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
        ValueKind::Range(left, right) => {
            range_span_rule_step_text(left, right, target, result, resolution_context)
        }
        ValueKind::Quantity(_, from_unit, _) => match target {
            SemanticConversionTarget::QuantityUnit(to_unit) => quantity_unit_equivalence_step_text(
                from_unit,
                to_unit,
                &value.lemma_type,
                resolution_context,
            ),
            _ => None,
        },
        ValueKind::Number(_) => None,
        ValueKind::Ratio(_, _) => match target {
            SemanticConversionTarget::RatioUnit(_) | SemanticConversionTarget::Number => None,
            _ => None,
        },
        ValueKind::Calendar(_, _) => match target {
            SemanticConversionTarget::Calendar(_) | SemanticConversionTarget::Number => None,
            _ => None,
        },
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

fn quantity_unit_equivalence_step_text(
    from_unit: &str,
    to_unit: &str,
    lemma_type: &LemmaType,
    resolution_context: UnitResolutionContext<'_>,
) -> Option<String> {
    if from_unit.is_empty() {
        let UnitResolutionContext::WithIndex(unit_index) = resolution_context else {
            return None;
        };
        let target_type = unit_index.get(to_unit)?;
        let to_factor = target_type.quantity_unit_factor(to_unit);
        let from_unit_name = infer_anonymous_quantity_unit_name(lemma_type, unit_index, to_unit)?;
        let from_factor = target_type.quantity_unit_factor(&from_unit_name);
        let multiplier = checked_div(from_factor, to_factor).ok()?;
        let mult_display = format_explanation_multiplier(&multiplier);
        if mult_display == "1" {
            return None;
        }
        return Some(format!("1 {from_unit_name} is {mult_display} {to_unit}"));
    }

    let from_factor = lemma_type.quantity_unit_factor(from_unit);
    let to_factor = lemma_type.quantity_unit_factor(to_unit);
    let multiplier = checked_div(from_factor, to_factor).ok()?;
    let mult_display = format_explanation_multiplier(&multiplier);
    if mult_display == "1" {
        return None;
    }
    Some(format!("1 {from_unit} is {mult_display} {to_unit}"))
}

pub(crate) fn infer_anonymous_quantity_unit_name(
    lemma_type: &LemmaType,
    unit_index: &std::collections::HashMap<String, LemmaType>,
    target_unit: &str,
) -> Option<String> {
    let target_type = unit_index.get(target_unit)?;
    let TypeSpecification::Quantity { units, .. } = &target_type.specifications else {
        return None;
    };
    for unit in &units.0 {
        if unit.name != target_unit {
            return Some(unit.name.clone());
        }
    }
    let TypeSpecification::Quantity { units, .. } = &lemma_type.specifications else {
        return None;
    };
    units
        .0
        .iter()
        .find(|unit| unit.name != target_unit)
        .map(|unit| unit.name.clone())
}

fn range_span_rule_step_text(
    left: &LiteralValue,
    right: &LiteralValue,
    target: &SemanticConversionTarget,
    result: &LiteralValue,
    _resolution_context: UnitResolutionContext<'_>,
) -> Option<String> {
    match (&left.value, &right.value) {
        (ValueKind::Date(left_date), ValueKind::Date(right_date)) => {
            let (lower, upper) = ordered_date_pair(left_date, right_date);
            let lower_lit = LiteralValue::date(lower.clone());
            let upper_lit = LiteralValue::date(upper.clone());
            match target {
                SemanticConversionTarget::QuantityUnit(_) => {
                    Some(format!("{upper_lit} − {lower_lit} = {result}"))
                }
                SemanticConversionTarget::Number => {
                    Some(format!("{upper_lit} − {lower_lit} = {result}"))
                }
                SemanticConversionTarget::Calendar(_) => {
                    Some(format!("{upper_lit} − {lower_lit} = {result}"))
                }
                _ => None,
            }
        }
        (ValueKind::Number(_), ValueKind::Number(_)) => {
            let (lower, upper) = ordered_number_pair(left, right);
            Some(format!("{upper} − {lower} = {result}"))
        }
        (ValueKind::Quantity(_, _, _), ValueKind::Quantity(_, _, _)) => {
            let (lower, upper) = ordered_quantity_pair(left, right);
            match target {
                SemanticConversionTarget::QuantityUnit(_) => {
                    Some(format!("{upper} − {lower} = {result}"))
                }
                _ => Some(format!("{upper} − {lower} = {result}")),
            }
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
    if compare_semantic_dates(left, right) == Ordering::Greater {
        (right, left)
    } else {
        (left, right)
    }
}

fn ordered_number_pair<'a>(
    left: &'a LiteralValue,
    right: &'a LiteralValue,
) -> (&'a LiteralValue, &'a LiteralValue) {
    let left_number = left
        .value
        .number_amount()
        .expect("BUG: ordered_number_pair requires numbers");
    let right_number = right
        .value
        .number_amount()
        .expect("BUG: ordered_number_pair requires numbers");
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
    let left_canonical = match &left.value {
        crate::planning::semantics::ValueKind::Quantity(val, unit, _) => {
            crate::computation::arithmetic::quantity_canonical_magnitude_rational(
                *val,
                unit,
                &left.lemma_type,
            )
            .expect("BUG: quantity canonical magnitude must succeed after planning")
        }
        _ => unreachable!("BUG: ordered_quantity_pair requires quantities"),
    };
    let right_canonical = match &right.value {
        crate::planning::semantics::ValueKind::Quantity(val, unit, _) => {
            crate::computation::arithmetic::quantity_canonical_magnitude_rational(
                *val,
                unit,
                &right.lemma_type,
            )
            .expect("BUG: quantity canonical magnitude must succeed after planning")
        }
        _ => unreachable!("BUG: ordered_quantity_pair requires quantities"),
    };
    if left_canonical <= right_canonical {
        (left, right)
    } else {
        (right, left)
    }
}

trait ValueKindParts {
    fn number_amount(&self) -> Option<rust_decimal::Decimal>;
}

impl ValueKindParts for ValueKind {
    fn number_amount(&self) -> Option<rust_decimal::Decimal> {
        match self {
            ValueKind::Number(number) => {
                crate::computation::rational::commit_rational_to_decimal(number).ok()
            }
            _ => None,
        }
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

    #[test]
    fn conversion_source_step_text_with_data_reference() {
        let operand = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "kilogram".to_string(),
            LemmaType::primitive(TypeSpecification::quantity()),
        );
        let path = DataPath::local("mass".to_string());
        let line = conversion_source_step_text(&operand, Some(&path));
        assert_eq!(line, "The quantity of mass is 2 kilogram");
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
        let lemma_type = LemmaType::primitive(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units,
            traits: vec![],
            decomposition: Default::default(),
            canonical_unit: "kilogram".to_string(),
            help: String::new(),
        });
        let operand = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "kilogram".to_string(),
            lemma_type.clone(),
        );
        let result = LiteralValue::quantity_with_type(
            RationalInteger::new(2000, 1),
            "gram".to_string(),
            lemma_type,
        );
        let path = DataPath::local("mass".to_string());
        let steps = build_conversion_steps(
            &operand,
            &SemanticConversionTarget::QuantityUnit("gram".to_string()),
            &result,
            Some(&path),
            UnitResolutionContext::NamedQuantityOnly,
        );
        assert_eq!(steps.len(), 3);
        assert!(matches!(steps[0].role, ConversionExplanationRole::Outcome));
        assert_eq!(steps[0].text, "2000 gram");
        assert!(matches!(steps[1].role, ConversionExplanationRole::Rule));
        assert_eq!(steps[1].text, "1 kilogram is 1000 gram");
        assert!(matches!(steps[2].role, ConversionExplanationRole::Source));
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
            lemma_type: LemmaType::primitive(TypeSpecification::date_range()),
        };
        let result = LiteralValue::quantity_with_type(
            RationalInteger::new(14, 1),
            "days".to_string(),
            LemmaType::primitive(TypeSpecification::quantity()),
        );
        let path = DataPath::local("age".to_string());
        let steps = build_conversion_steps(
            &range,
            &SemanticConversionTarget::QuantityUnit("days".to_string()),
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
        let lemma_type = LemmaType::primitive(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units,
            traits: vec![],
            decomposition: Default::default(),
            canonical_unit: "kilogram".to_string(),
            help: String::new(),
        });
        let operand = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "kilogram".to_string(),
            lemma_type.clone(),
        );
        let result = LiteralValue::quantity_with_type(
            RationalInteger::new(2, 1),
            "kilogram".to_string(),
            lemma_type,
        );
        let steps = build_conversion_steps(
            &operand,
            &SemanticConversionTarget::QuantityUnit("kilogram".to_string()),
            &result,
            None,
            UnitResolutionContext::NamedQuantityOnly,
        );
        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[0].role, ConversionExplanationRole::Outcome));
        assert!(matches!(steps[1].role, ConversionExplanationRole::Source));
    }

    #[test]
    fn conversion_explanation_step_roundtrip() {
        let step = ConversionExplanationStep {
            role: ConversionExplanationRole::Rule,
            text: "1 kilogram is 1000 gram".to_string(),
            data_ref: Some(DataPath::local("mass".to_string())),
        };
        let json = serde_json::to_string(&step).expect("serialize");
        let restored: ConversionExplanationStep = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.text, step.text);
        assert!(matches!(restored.role, ConversionExplanationRole::Rule));
    }
}
