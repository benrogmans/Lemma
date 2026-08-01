//! API value for a rule result, `ShowData.prefilled`, or `ShowData.suggestion`.
//!
//! This is the single API-facing value representation shared by all three sites, expanded
//! into every declared unit for measure/ratio. It sits at the crate root (not under
//! `evaluation/`) because `planning::execution_plan::ShowData` needs it too, and planning
//! must not import evaluation. The plan/eval-internal representation is the canonical
//! `planning::semantics::LiteralValue`; this module is the boundary between the two.

use crate::computation::rational::{checked_div, checked_mul, NumericFailure};
use crate::literals::rational_from_parsed_decimal;
use crate::planning::semantics::{
    range_element_type_specification, semantic_calendar_unit_from_measure_signature, LemmaType,
    LiteralUnitMapFailure, LiteralValue, SemanticDateTime, SemanticTime, TypeSpecification,
    ValueKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Calendar value (a measure whose unit is a calendar unit, e.g. `3 months`) on a result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarResult {
    pub value: String,
    pub unit: String,
}

/// Both endpoints of a range result.
///
/// Each endpoint is itself a [`RuleResultValue`], but an endpoint's own `range` field is
/// always `None` — a range endpoint must never itself be a range. Building a
/// [`RuleResultValue`] and reconstructing a literal both panic if this invariant is
/// violated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RangeResult {
    pub from: RuleResultValue,
    pub to: RuleResultValue,
}

/// API value shared by flattened [`crate::evaluation::response::RuleResult`],
/// `ShowData.prefilled`, and `ShowData.suggestion`.
///
/// When present: always `display` (from [`LiteralValue::display_value`]), plus exactly
/// one typed field for a non-range value; `range` is set instead for a range value, and
/// every other typed field stays `None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuleResultValue {
    /// Engine-rendered string for UI (`LiteralValue::display_value`). Present whenever
    /// this value is present, including range endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boolean: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<SemanticDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<SemanticTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<CalendarResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Box<RangeResult>>,
}

/// Why building a [`RuleResultValue`] from a canonical [`LiteralValue`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleResultValueFailure {
    DecimalLimit,
    NumericOverflow,
    OutOfMemory,
}

/// Human-readable veto message for a [`RuleResultValueFailure`].
pub fn rule_result_value_failure_message(failure: RuleResultValueFailure) -> &'static str {
    match failure {
        RuleResultValueFailure::DecimalLimit => "Calculated result exceeds decimal value limit",
        RuleResultValueFailure::NumericOverflow => "numeric overflow",
        RuleResultValueFailure::OutOfMemory => "out of memory",
    }
}

fn map_numeric_to_rule_result_value_failure(failure: NumericFailure) -> RuleResultValueFailure {
    match failure {
        NumericFailure::Overflow => RuleResultValueFailure::DecimalLimit,
        NumericFailure::OutOfMemory => RuleResultValueFailure::OutOfMemory,
        NumericFailure::DivisionByZero => {
            panic!(
                "BUG: decimal commit encountered division by zero while building RuleResultValue"
            )
        }
        NumericFailure::Irrational => {
            panic!(
                "BUG: decimal commit encountered irrational result while building RuleResultValue"
            )
        }
    }
}

fn map_unit_conversion_failure(failure: NumericFailure) -> RuleResultValueFailure {
    match failure {
        NumericFailure::Overflow => RuleResultValueFailure::NumericOverflow,
        NumericFailure::OutOfMemory => RuleResultValueFailure::OutOfMemory,
        NumericFailure::DivisionByZero => {
            panic!(
                "BUG: unit conversion encountered division by zero while building RuleResultValue"
            )
        }
        NumericFailure::Irrational => {
            panic!(
                "BUG: unit conversion encountered irrational result while building RuleResultValue"
            )
        }
    }
}

fn map_literal_unit_map_failure(failure: LiteralUnitMapFailure) -> RuleResultValueFailure {
    match failure {
        LiteralUnitMapFailure::Commit(nf) => map_numeric_to_rule_result_value_failure(nf),
        LiteralUnitMapFailure::UnitConversion(nf) => map_unit_conversion_failure(nf),
    }
}

fn measure_to_unit_map(
    literal: &LiteralValue,
    result_type: &LemmaType,
) -> Result<BTreeMap<String, String>, RuleResultValueFailure> {
    result_type
        .measure_literal_in_all_units(literal)
        .map_err(map_literal_unit_map_failure)
}

fn ratio_to_unit_map(
    literal: &LiteralValue,
    result_type: &LemmaType,
) -> Result<BTreeMap<String, String>, RuleResultValueFailure> {
    result_type
        .ratio_literal_in_all_units(literal)
        .map_err(map_literal_unit_map_failure)
}

fn element_type_from_range_rule(rule_type: &LemmaType) -> Option<LemmaType> {
    range_element_type_specification(&rule_type.specifications).map(LemmaType::primitive)
}

/// A range endpoint uses its own declared type when it carries one
/// (unit-scoped range endpoints), falling back to the range's element type otherwise.
fn range_endpoint_type(endpoint: &LiteralValue, range_element_type: &LemmaType) -> LemmaType {
    if endpoint.lemma_type.measure_unit_names().is_some() {
        endpoint.lemma_type.as_ref().clone()
    } else {
        range_element_type.clone()
    }
}

/// Build a [`RuleResultValue`] from a canonical [`LiteralValue`].
///
/// Measure and ratio expand into every unit declared on `rule_type`. A range value
/// builds both endpoints; an endpoint that is itself a range is a planning bug
/// (ranges do not nest) and panics rather than being silently flattened.
pub fn rule_result_value_from_literal(
    literal: &LiteralValue,
    rule_type: &LemmaType,
) -> Result<RuleResultValue, RuleResultValueFailure> {
    match &literal.value {
        ValueKind::Range(from, to) => {
            let endpoint_type =
                element_type_from_range_rule(rule_type).unwrap_or_else(|| rule_type.clone());
            let from_type = range_endpoint_type(from, &endpoint_type);
            let to_type = range_endpoint_type(to, &endpoint_type);
            let from_value = rule_result_value_from_range_endpoint(from, &from_type)?;
            let to_value = rule_result_value_from_range_endpoint(to, &to_type)?;
            Ok(RuleResultValue {
                display: Some(literal.display_value()),
                range: Some(Box::new(RangeResult {
                    from: from_value,
                    to: to_value,
                })),
                ..RuleResultValue::default()
            })
        }
        _ => rule_result_value_from_non_range_literal(literal, rule_type),
    }
}

fn rule_result_value_from_range_endpoint(
    endpoint: &LiteralValue,
    endpoint_type: &LemmaType,
) -> Result<RuleResultValue, RuleResultValueFailure> {
    if matches!(&endpoint.value, ValueKind::Range(_, _)) {
        panic!("BUG: range endpoint must not itself be a range");
    }
    rule_result_value_from_non_range_literal(endpoint, endpoint_type)
}

fn rule_result_value_from_non_range_literal(
    literal: &LiteralValue,
    result_type: &LemmaType,
) -> Result<RuleResultValue, RuleResultValueFailure> {
    let display = Some(literal.display_value());
    match &literal.value {
        ValueKind::Measure(rational, sig) if literal.lemma_type.is_calendar_like() => {
            let unit = semantic_calendar_unit_from_measure_signature(sig);
            let value = literal
                .lemma_type
                .try_rational_as_decimal_string(rational)
                .map_err(map_numeric_to_rule_result_value_failure)?;
            Ok(RuleResultValue {
                display,
                calendar: Some(CalendarResult {
                    value,
                    unit: unit.to_string(),
                }),
                ..RuleResultValue::default()
            })
        }
        ValueKind::Measure(_, _) => Ok(RuleResultValue {
            display,
            measure: Some(measure_to_unit_map(literal, result_type)?),
            ..RuleResultValue::default()
        }),
        ValueKind::Ratio(_, _) => Ok(RuleResultValue {
            display,
            ratio: Some(ratio_to_unit_map(literal, result_type)?),
            ..RuleResultValue::default()
        }),
        ValueKind::Number(rational) => {
            let number = result_type
                .try_rational_as_decimal_string(rational)
                .map_err(map_numeric_to_rule_result_value_failure)?;
            Ok(RuleResultValue {
                display,
                number: Some(number),
                ..RuleResultValue::default()
            })
        }
        ValueKind::Boolean(b) => Ok(RuleResultValue {
            display,
            boolean: Some(*b),
            ..RuleResultValue::default()
        }),
        ValueKind::Text(s) => Ok(RuleResultValue {
            display,
            text: Some(s.clone()),
            ..RuleResultValue::default()
        }),
        ValueKind::Date(d) => Ok(RuleResultValue {
            display,
            date: Some(d.clone()),
            ..RuleResultValue::default()
        }),
        ValueKind::Time(t) => Ok(RuleResultValue {
            display,
            time: Some(t.clone()),
            ..RuleResultValue::default()
        }),
        ValueKind::Range(_, _) => {
            unreachable!("BUG: range must be handled by rule_result_value_from_literal")
        }
    }
}

fn decimal_from_api_string(value: &str) -> rust_decimal::Decimal {
    use std::str::FromStr;
    rust_decimal::Decimal::from_str(value)
        .unwrap_or_else(|_| panic!("BUG: rule result API decimal string must parse as decimal"))
}

fn literal_from_measure_map(
    measure: &BTreeMap<String, String>,
    rule_type: &LemmaType,
) -> LiteralValue {
    let unit_names = rule_type
        .measure_unit_names()
        .expect("BUG: measure rule result must have declared units");
    let unit_name = unit_names
        .first()
        .expect("BUG: measure rule result type must declare at least one unit");
    let display = measure
        .get(*unit_name)
        .unwrap_or_else(|| panic!("BUG: measure map missing unit '{unit_name}'"));
    let rational = rational_from_parsed_decimal(decimal_from_api_string(display))
        .expect("BUG: measure rule result value must lift to rational");
    let factor = rule_type.measure_unit_factor(unit_name);
    let canonical = checked_mul(&rational, factor).unwrap_or_else(|failure| {
        panic!("BUG: measure canonicalization from RuleResultValue fields failed: {failure}")
    });
    LiteralValue::measure_with_type(
        canonical,
        (*unit_name).to_string(),
        Arc::new(rule_type.clone()),
    )
}

fn literal_from_ratio_map(ratio: &BTreeMap<String, String>, rule_type: &LemmaType) -> LiteralValue {
    let units = match &rule_type.specifications {
        TypeSpecification::Ratio { units, .. } => units,
        TypeSpecification::RatioRange { .. } => {
            let element = range_element_type_specification(&rule_type.specifications)
                .expect("BUG: ratio range rule type must have ratio element specification");
            let TypeSpecification::Ratio { units, .. } = element else {
                panic!("BUG: ratio range element spec must be Ratio");
            };
            return literal_from_ratio_map(
                ratio,
                &LemmaType::primitive(TypeSpecification::Ratio {
                    minimum: None,
                    maximum: None,
                    decimals: None,
                    units,
                    help: String::new(),
                }),
            );
        }
        _ => panic!(
            "BUG: ratio rule result type must be Ratio, got {}",
            rule_type.name()
        ),
    };
    let unit = units
        .iter()
        .next()
        .expect("BUG: ratio rule result type must declare at least one unit");
    let display = ratio
        .get(&unit.name)
        .unwrap_or_else(|| panic!("BUG: ratio map missing unit '{}'", unit.name));
    let display_rational = rational_from_parsed_decimal(decimal_from_api_string(display))
        .expect("BUG: ratio rule result value must lift to rational");
    let canonical = checked_div(&display_rational, &unit.value).unwrap_or_else(|failure| {
        panic!("BUG: ratio canonicalization from RuleResultValue fields failed: {failure}")
    });
    LiteralValue::ratio_with_type(canonical, None, Arc::new(rule_type.clone()))
}

impl RuleResultValue {
    /// Reconstruct the [`LiteralValue`] from this API value's fields.
    ///
    /// Panics if the fields cannot reconstruct a literal, or if a range endpoint is
    /// itself a range (ranges do not nest — enforced here, not by convention).
    pub fn to_literal(&self, rule_type: &LemmaType) -> LiteralValue {
        if let Some(range) = &self.range {
            if range.from.range.is_some() || range.to.range.is_some() {
                panic!("BUG: range endpoint must not itself be a range");
            }
            let endpoint_type =
                element_type_from_range_rule(rule_type).unwrap_or_else(|| rule_type.clone());
            let left = range.from.to_literal(&endpoint_type);
            let right = range.to.to_literal(&endpoint_type);
            return LiteralValue::range(left, right);
        }

        let owned_rule_type = Arc::new(rule_type.clone());
        if let Some(b) = self.boolean {
            return LiteralValue {
                value: ValueKind::Boolean(b),
                lemma_type: owned_rule_type,
            };
        }
        if let Some(number) = &self.number {
            return LiteralValue::number_with_type_from_decimal(
                decimal_from_api_string(number),
                owned_rule_type,
            );
        }
        if let Some(calendar) = &self.calendar {
            let rational = rational_from_parsed_decimal(decimal_from_api_string(&calendar.value))
                .expect("BUG: calendar rule result value must lift to rational");
            return LiteralValue::measure_with_type(
                rational,
                calendar.unit.clone(),
                owned_rule_type,
            );
        }
        if let Some(measure) = &self.measure {
            return literal_from_measure_map(measure, rule_type);
        }
        if let Some(ratio) = &self.ratio {
            return literal_from_ratio_map(ratio, rule_type);
        }
        if let Some(date) = &self.date {
            return LiteralValue {
                value: ValueKind::Date(date.clone()),
                lemma_type: owned_rule_type,
            };
        }
        if let Some(time) = &self.time {
            return LiteralValue {
                value: ValueKind::Time(time.clone()),
                lemma_type: owned_rule_type,
            };
        }
        if let Some(text) = &self.text {
            return LiteralValue {
                value: ValueKind::Text(text.clone()),
                lemma_type: owned_rule_type,
            };
        }
        panic!("BUG: rule result value fields cannot reconstruct literal");
    }
}

fn format_unit_map(map: &BTreeMap<String, String>) -> String {
    map.iter()
        .map(|(unit, value)| format!("{value} {unit}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for RuleResultValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(range) = &self.range {
            return write!(f, "{}...{}", range.from, range.to);
        }
        if let Some(measure) = &self.measure {
            return write!(f, "{}", format_unit_map(measure));
        }
        if let Some(ratio) = &self.ratio {
            return write!(f, "{}", format_unit_map(ratio));
        }
        if let Some(number) = &self.number {
            return write!(f, "{number}");
        }
        if let Some(b) = self.boolean {
            return write!(f, "{b}");
        }
        if let Some(text) = &self.text {
            return write!(f, "{text}");
        }
        if let Some(date) = &self.date {
            return write!(f, "{date}");
        }
        if let Some(time) = &self.time {
            return write!(f, "{time}");
        }
        if let Some(calendar) = &self.calendar {
            return write!(f, "{} {}", calendar.value, calendar.unit);
        }
        panic!("BUG: rule result value has no field set to display");
    }
}
