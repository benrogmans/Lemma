use super::Rule;
use crate::error::LemmaError;
use crate::parsing::ast::Span;
use crate::semantic::*;

use chrono::{Datelike, Timelike};
use pest::iterators::Pair;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;

pub(crate) fn parse_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    match pair.as_rule() {
        Rule::number_literal => parse_number_literal(pair),
        Rule::text_literal => parse_string_literal(pair),
        Rule::boolean_literal => parse_boolean_literal(pair),
        Rule::percent_literal => parse_percent_literal(pair),
        Rule::date_time_literal => parse_datetime_literal(pair),
        Rule::time_literal => parse_time_literal(pair),
        Rule::duration_literal => parse_duration_literal(pair),
        _ => Err(LemmaError::engine(
            format!("Unsupported literal type: {:?}", pair.as_rule()),
            Span::from_pest_span(pair.as_span()),
            "<unknown>",
            Arc::from(pair.as_str()),
            "<unknown>",
            1,
            None::<String>,
        )),
    }
}

fn parse_number_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let pair_str = pair.as_str();
    let span = Span::from_pest_span(pair.as_span());
    let mut inner = pair.into_inner();

    let number = match inner.next() {
        Some(inner_pair) => match inner_pair.as_rule() {
            Rule::scientific_number => parse_scientific_number(inner_pair)?,
            Rule::decimal_number => parse_decimal_number(inner_pair.as_str())?,
            _ => {
                return Err(LemmaError::engine(
                    "Unexpected number literal structure",
                    span,
                    "<unknown>",
                    Arc::from(pair_str),
                    "<unknown>",
                    1,
                    None::<String>,
                ));
            }
        },
        None => parse_decimal_number(pair_str)?,
    };

    Ok(LiteralValue::number(number))
}

fn parse_string_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let content = pair.as_str();
    let unquoted = &content[1..content.len() - 1];
    Ok(LiteralValue::text(unquoted.to_string()))
}

fn parse_boolean_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    use crate::BooleanValue;

    let boolean_value = match pair.as_str() {
        "true" => BooleanValue::True,
        "false" => BooleanValue::False,
        "yes" => BooleanValue::Yes,
        "no" => BooleanValue::No,
        "accept" => BooleanValue::Accept,
        "reject" => BooleanValue::Reject,
        _ => {
            let span = Span::from_pest_span(pair.as_span());
            return Err(LemmaError::engine(
                format!("Invalid boolean: '{}'\n             Expected one of: true, false, yes, no, accept, reject", pair.as_str()),
                span,
                "<unknown>",
                Arc::from(pair.as_str()),
                "<unknown>",
                1,
                None::<String>,
            ));
        }
    };

    Ok(LiteralValue::boolean(boolean_value))
}

fn parse_percent_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let pair_str = pair.as_str();
    let pair_span = Span::from_pest_span(pair.as_span());
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::number_literal {
            let inner_span = Span::from_pest_span(inner_pair.as_span());
            let percentage = parse_number_literal(inner_pair)?;
            match &percentage.value {
                Value::Number(n) => {
                    // Convert percent (50) to ratio (0.50) for storage
                    // The percent unit in standard_ratio() type will indicate this is a percent
                    use rust_decimal::Decimal;
                    let ratio_value = *n / Decimal::from(100);
                    return Ok(LiteralValue {
                        value: Value::Ratio(ratio_value, Some("percent".to_string())),
                        lemma_type: crate::semantic::standard_ratio().clone(),
                    });
                }
                _ => {
                    return Err(LemmaError::engine(
                        "Expected number in percent literal",
                        inner_span,
                        "<unknown>",
                        Arc::from(pair_str),
                        "<unknown>",
                        1,
                        None::<String>,
                    ));
                }
            }
        }
    }
    Err(LemmaError::engine(
        "Invalid percent literal: missing number",
        pair_span,
        "<unknown>",
        Arc::from(pair_str),
        "<unknown>",
        1,
        None::<String>,
    ))
}

fn parse_duration_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let pair_str = pair.as_str();
    let pair_span = Span::from_pest_span(pair.as_span());
    let mut number = None;
    let mut unit_str = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::number_literal => {
                let inner_span = Span::from_pest_span(inner_pair.as_span());
                let lit = parse_number_literal(inner_pair)?;
                match &lit.value {
                    Value::Number(n) => number = Some(*n),
                    _ => {
                        return Err(LemmaError::engine(
                            "Expected number in duration literal",
                            inner_span,
                            "<unknown>",
                            Arc::from(pair_str),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                }
            }
            Rule::duration_unit => {
                unit_str = Some(inner_pair.as_str());
            }
            _ => {}
        }
    }

    let span = pair_span.clone();
    let value = number.ok_or_else(|| {
        LemmaError::engine(
            "Missing number in duration literal",
            span.clone(),
            "<unknown>",
            Arc::from(pair_str),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;
    let unit = unit_str.ok_or_else(|| {
        LemmaError::engine(
            "Missing unit in duration literal",
            span,
            "<unknown>",
            Arc::from(pair_str),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;

    super::units::resolve_unit(value, unit)
}

/// Parse a number+unit literal (e.g., "5 celsius")
/// Returns (number, unit_name) tuple for later resolution during semantic analysis
pub(crate) fn parse_number_unit_literal(
    pair: Pair<Rule>,
) -> Result<(rust_decimal::Decimal, String), LemmaError> {
    let pair_str = pair.as_str();
    let pair_span = Span::from_pest_span(pair.as_span());
    let mut number = None;
    let mut unit_name = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::number_literal => {
                let inner_span = Span::from_pest_span(inner_pair.as_span());
                let lit = parse_number_literal(inner_pair)?;
                match &lit.value {
                    Value::Number(n) => number = Some(*n),
                    _ => {
                        return Err(LemmaError::engine(
                            "Expected number in number+unit literal",
                            inner_span,
                            "<unknown>",
                            Arc::from(pair_str),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                }
            }
            Rule::unit_name => {
                unit_name = Some(inner_pair.as_str().to_string());
            }
            _ => {}
        }
    }

    let span = pair_span.clone();
    let value = number.ok_or_else(|| {
        LemmaError::engine(
            "Missing number in number+unit literal",
            span.clone(),
            "<unknown>",
            Arc::from(pair_str),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;
    let unit = unit_name.ok_or_else(|| {
        LemmaError::engine(
            "Missing unit name in number+unit literal",
            pair_span,
            "<unknown>",
            Arc::from(pair_str),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;

    Ok((value, unit))
}

fn parse_datetime_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let datetime_str = pair.as_str();

    if let Ok(dt) = datetime_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = dt.offset().local_minus_utc();
        return Ok(LiteralValue::date(DateTimeValue {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            timezone: Some(TimezoneValue {
                offset_hours: (offset / 3600) as i8,
                offset_minutes: ((offset % 3600) / 60) as u8,
            }),
        }));
    }

    if let Ok(dt) = datetime_str.parse::<chrono::NaiveDateTime>() {
        return Ok(LiteralValue::date(DateTimeValue {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            timezone: None,
        }));
    }

    if let Ok(d) = datetime_str.parse::<chrono::NaiveDate>() {
        return Ok(LiteralValue::date(DateTimeValue {
            year: d.year(),
            month: d.month(),
            day: d.day(),
            hour: 0,
            minute: 0,
            second: 0,
            timezone: None,
        }));
    }

    Err(LemmaError::engine(
        format!("Invalid date/time format: '{}'\n         Expected one of:\n         - Date: YYYY-MM-DD (e.g., 2024-01-15)\n         - DateTime: YYYY-MM-DDTHH:MM:SS (e.g., 2024-01-15T14:30:00)\n         - With timezone: YYYY-MM-DDTHH:MM:SSZ or +HH:MM (e.g., 2024-01-15T14:30:00Z)\n         Note: Month must be 1-12, day must be valid for the month (no Feb 30), hours 0-23, minutes/seconds 0-59", datetime_str),
        Span::from_pest_span(pair.as_span()),
        "<unknown>",
        Arc::from(datetime_str),
        "<unknown>",
        1,
        None::<String>,
    ))
}

fn parse_time_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let time_str = pair.as_str();

    if let Ok(t) = time_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = t.offset().local_minus_utc();
        return Ok(LiteralValue::time(TimeValue {
            hour: t.hour() as u8,
            minute: t.minute() as u8,
            second: t.second() as u8,
            timezone: Some(TimezoneValue {
                offset_hours: (offset / 3600) as i8,
                offset_minutes: ((offset % 3600) / 60) as u8,
            }),
        }));
    }

    if let Ok(t) = time_str.parse::<chrono::NaiveTime>() {
        return Ok(LiteralValue::time(TimeValue {
            hour: t.hour() as u8,
            minute: t.minute() as u8,
            second: t.second() as u8,
            timezone: None,
        }));
    }

    Err(LemmaError::engine(
        format!("Invalid time format: '{}'\n         Expected: HH:MM or HH:MM:SS (e.g., 14:30 or 14:30:00)\n         With timezone: HH:MM:SSZ or +HH:MM (e.g., 14:30:00Z or 14:30:00+01:00)\n         Note: Hours must be 0-23, minutes and seconds must be 0-59", time_str),
        Span::from_pest_span(pair.as_span()),
        "<unknown>",
        Arc::from(time_str),
        "<unknown>",
        1,
        None::<String>,
    ))
}

const MAX_DECIMAL_EXPONENT: i32 = 28;

fn parse_scientific_number(pair: Pair<Rule>) -> Result<Decimal, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let pair_str = pair.as_str();
    let mut inner = pair.into_inner();

    let mantissa_pair = inner.next().ok_or_else(|| {
        LemmaError::engine(
            "Missing mantissa in scientific notation",
            span.clone(),
            "<unknown>",
            Arc::from(pair_str),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;
    let exponent_pair = inner.next().ok_or_else(|| {
        LemmaError::engine(
            "Missing exponent in scientific notation",
            span.clone(),
            "<unknown>",
            Arc::from(pair_str),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;

    let mantissa = parse_decimal_number(mantissa_pair.as_str())?;
    let exponent_span = Span::from_pest_span(exponent_pair.as_span());
    let exponent: i32 = exponent_pair.as_str().parse().map_err(|_| {
        LemmaError::engine(
            format!(
                "Invalid exponent: '{}'\n             Expected an integer between -{} and +{}",
                exponent_pair.as_str(),
                MAX_DECIMAL_EXPONENT,
                MAX_DECIMAL_EXPONENT
            ),
            exponent_span.clone(),
            "<unknown>",
            Arc::from(exponent_pair.as_str()),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;

    let power_of_ten = decimal_pow10(exponent).ok_or_else(|| {
        LemmaError::engine(
            format!("Exponent {} is out of range\n             Maximum supported exponent is ±{} (values up to ~10^28)", exponent, MAX_DECIMAL_EXPONENT),
            exponent_span,
            "<unknown>",
            Arc::from(exponent_pair.as_str()),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;

    if exponent >= 0 {
        mantissa.checked_mul(power_of_ten).ok_or_else(|| {
            LemmaError::engine(
                format!(
                    "Number overflow: result of {}e{} exceeds maximum value (~10^28)",
                    mantissa, exponent
                ),
                span,
                "<unknown>",
                Arc::from(pair_str),
                "<unknown>",
                1,
                None::<String>,
            )
        })
    } else {
        mantissa.checked_div(power_of_ten).ok_or_else(|| {
            LemmaError::engine(
                format!(
                    "Precision error: result of {}e{} has too many decimal places (max 28)",
                    mantissa, exponent
                ),
                span,
                "<unknown>",
                Arc::from(pair_str),
                "<unknown>",
                1,
                None::<String>,
            )
        })
    }
}

fn decimal_pow10(exp: i32) -> Option<Decimal> {
    let abs_exp = exp.abs();
    if abs_exp > MAX_DECIMAL_EXPONENT {
        return None;
    }

    let mut result = Decimal::ONE;
    let ten = Decimal::from(10);

    for _ in 0..abs_exp {
        result = result.checked_mul(ten)?;
    }

    Some(result)
}

fn parse_decimal_number(number_str: &str) -> Result<Decimal, LemmaError> {
    let clean_number = number_str.replace(['_', ','], "");
    Decimal::from_str(&clean_number).map_err(|_| {
        LemmaError::engine(
            format!("Invalid number: '{}'\n             Expected a valid decimal number (e.g., 42, 3.14, 1_000_000, 1,000,000)\n             Note: Use underscores or commas as thousand separators if needed", number_str),
            Span { start: 0, end: 0, line: 1, col: 0 },
            "<unknown>",
            Arc::from(number_str),
            "<unknown>",
            1,
            None::<String>,
        )
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::parsing::parse;
    use crate::ResourceLimits;

    #[test]
    fn parse_rejects_percent_literal_with_trailing_digits() {
        // Guard against tokenization bugs around percent literals.
        // The grammar comment says '%' must be directly followed by a non-digit or EOI.
        let input = r#"doc test
fact x = 10%5"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(
            result.is_err(),
            "Percent literals like `10%5` must be rejected"
        );
    }
}
