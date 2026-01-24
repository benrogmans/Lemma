use crate::error::LemmaError;
use crate::parsing::ast::Span;
use crate::semantic::{ConversionTarget, DurationUnit, LiteralValue};
use rust_decimal::Decimal;
use std::sync::Arc;

pub fn resolve_unit(
    value: Decimal,
    unit_str: &str,
    span: Span,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<LiteralValue, LemmaError> {
    let unit_lower = unit_str.to_lowercase();

    if let Some(unit) = try_parse_duration_unit(&unit_lower) {
        return Ok(LiteralValue::duration(value, unit));
    }

    Err(LemmaError::engine(
        format!("Unknown duration unit: '{}'. Expected one of: years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds", unit_str),
        span,
        attribute,
        source_text,
        doc_name,
        1,
        None::<String>,
    ))
}

fn try_parse_duration_unit(s: &str) -> Option<DurationUnit> {
    match s {
        "year" | "years" => Some(DurationUnit::Year),
        "month" | "months" => Some(DurationUnit::Month),
        "week" | "weeks" => Some(DurationUnit::Week),
        "day" | "days" => Some(DurationUnit::Day),
        "hour" | "hours" => Some(DurationUnit::Hour),
        "minute" | "minutes" => Some(DurationUnit::Minute),
        "second" | "seconds" => Some(DurationUnit::Second),
        "millisecond" | "milliseconds" => Some(DurationUnit::Millisecond),
        "microsecond" | "microseconds" => Some(DurationUnit::Microsecond),
        _ => None,
    }
}

pub fn resolve_conversion_target(unit_str: &str) -> Result<ConversionTarget, LemmaError> {
    let unit_lower = unit_str.to_lowercase();

    if unit_lower == "percent" {
        return Ok(ConversionTarget::Percentage);
    }

    if let Some(unit) = try_parse_duration_unit(&unit_lower) {
        return Ok(ConversionTarget::Duration(unit));
    }

    Ok(ConversionTarget::ScaleUnit(unit_lower))
}
