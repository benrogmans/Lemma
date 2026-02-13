use super::Rule;
use crate::error::LemmaError;
use crate::parsing::ast::Span;
use crate::parsing::ast::*;
use crate::Source;

use chrono::{Datelike, Timelike};
use pest::iterators::Pair;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;

pub(crate) fn parse_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Value, LemmaError> {
    match pair.as_rule() {
        Rule::number_literal => parse_number_literal(pair, attribute, doc_name),
        Rule::number_unit_literal => {
            let (n, u) = parse_number_unit_literal(pair, attribute, doc_name)?;
            Ok(Value::Scale(n, u))
        }
        Rule::text_literal => parse_string_literal(pair),
        Rule::boolean_literal => parse_boolean_literal(pair, attribute, doc_name),
        Rule::percent_literal => parse_percent_literal(pair, attribute, doc_name),
        Rule::permille_literal => parse_permille_literal(pair, attribute, doc_name),
        Rule::date_time_literal => parse_datetime_literal(pair, attribute, doc_name),
        Rule::time_literal => parse_time_literal(pair, attribute, doc_name),
        Rule::duration_literal => parse_duration_literal(pair, attribute, doc_name),
        _ => Err(LemmaError::engine(
            format!("Unsupported literal type: {:?}", pair.as_rule()),
            Source::new(attribute, Span::from_pest_span(pair.as_span()), doc_name),
            Arc::from(pair.as_str()),
            None::<String>,
        )),
    }
}

fn parse_number_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Value, LemmaError> {
    let pair_str = pair.as_str();
    let span = Span::from_pest_span(pair.as_span());
    let mut inner = pair.into_inner();

    let number = match inner.next() {
        Some(inner_pair) => match inner_pair.as_rule() {
            Rule::scientific_number => parse_scientific_number(inner_pair, attribute, doc_name)?,
            Rule::decimal_number => {
                let inner_span = Span::from_pest_span(inner_pair.as_span());
                parse_decimal_number(
                    inner_pair.as_str(),
                    inner_span,
                    attribute,
                    doc_name,
                    Arc::from(pair_str),
                )?
            }
            _ => {
                return Err(LemmaError::engine(
                    "Unexpected number literal structure",
                    Source::new(attribute, span, doc_name),
                    Arc::from(pair_str),
                    None::<String>,
                ));
            }
        },
        None => parse_decimal_number(
            pair_str,
            span.clone(),
            attribute,
            doc_name,
            Arc::from(pair_str),
        )?,
    };

    Ok(Value::Number(number))
}

fn parse_string_literal(pair: Pair<Rule>) -> Result<Value, LemmaError> {
    let content = pair.as_str();
    let unquoted = &content[1..content.len() - 1];
    Ok(Value::Text(unquoted.to_string()))
}

fn parse_boolean_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Value, LemmaError> {
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
                Source::new(attribute, span, doc_name),
                Arc::from(pair.as_str()),
                None::<String>,
            ));
        }
    };

    Ok(Value::Boolean(boolean_value))
}

fn parse_percent_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Value, LemmaError> {
    let pair_str = pair.as_str();
    let pair_span = Span::from_pest_span(pair.as_span());
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::number_literal {
            let inner_span = Span::from_pest_span(inner_pair.as_span());
            let percentage_value = parse_number_literal(inner_pair, attribute, doc_name)?;
            match &percentage_value {
                Value::Number(n) => {
                    // Convert percent (50) to ratio (0.50) for storage
                    use rust_decimal::Decimal;
                    let ratio_value = *n / Decimal::from(100);
                    return Ok(Value::Ratio(ratio_value, Some("percent".to_string())));
                }
                _ => {
                    return Err(LemmaError::engine(
                        "Expected number in percent literal",
                        Source::new(attribute, inner_span, doc_name),
                        Arc::from(pair_str),
                        None::<String>,
                    ));
                }
            }
        }
    }
    Err(LemmaError::engine(
        "Invalid percent literal: missing number",
        Source::new(attribute, pair_span, doc_name),
        Arc::from(pair_str),
        None::<String>,
    ))
}

fn parse_permille_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Value, LemmaError> {
    let pair_str = pair.as_str();
    let pair_span = Span::from_pest_span(pair.as_span());
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::number_literal {
            let inner_span = Span::from_pest_span(inner_pair.as_span());
            let permille_value = parse_number_literal(inner_pair, attribute, doc_name)?;
            match &permille_value {
                Value::Number(n) => {
                    // Convert permille (5) to ratio (0.005) for storage
                    use rust_decimal::Decimal;
                    let ratio_value = *n / Decimal::from(1000);
                    return Ok(Value::Ratio(ratio_value, Some("permille".to_string())));
                }
                _ => {
                    return Err(LemmaError::engine(
                        "Expected number in permille literal",
                        Source::new(attribute, inner_span, doc_name),
                        Arc::from(pair_str),
                        None::<String>,
                    ));
                }
            }
        }
    }
    Err(LemmaError::engine(
        "Invalid permille literal: missing number",
        Source::new(attribute, pair_span, doc_name),
        Arc::from(pair_str),
        None::<String>,
    ))
}

fn parse_duration_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Value, LemmaError> {
    let pair_str = pair.as_str();
    let pair_span = Span::from_pest_span(pair.as_span());
    let mut number = None;
    let mut unit_str = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::number_literal => {
                let inner_span = Span::from_pest_span(inner_pair.as_span());
                let lit_value = parse_number_literal(inner_pair, attribute, doc_name)?;
                match &lit_value {
                    Value::Number(n) => number = Some(*n),
                    _ => {
                        return Err(LemmaError::engine(
                            "Expected number in duration literal",
                            Source::new(attribute, inner_span, doc_name),
                            Arc::from(pair_str),
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
            Source::new(attribute, span.clone(), doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;
    let unit_string = unit_str.ok_or_else(|| {
        LemmaError::engine(
            "Missing unit in duration literal",
            Source::new(attribute, span, doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;

    // Parse the duration unit string to DurationUnit enum
    let unit = parse_duration_unit_string(
        unit_string,
        pair_span,
        attribute,
        doc_name,
        Arc::from(pair_str),
    )?;

    Ok(Value::Duration(value, unit))
}

pub(crate) fn parse_duration_unit_string(
    unit_str: &str,
    span: Span,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<DurationUnit, LemmaError> {
    let unit_lower = unit_str.to_lowercase();

    match unit_lower.as_str() {
        "year" | "years" => Ok(DurationUnit::Year),
        "month" | "months" => Ok(DurationUnit::Month),
        "week" | "weeks" => Ok(DurationUnit::Week),
        "day" | "days" => Ok(DurationUnit::Day),
        "hour" | "hours" => Ok(DurationUnit::Hour),
        "minute" | "minutes" => Ok(DurationUnit::Minute),
        "second" | "seconds" => Ok(DurationUnit::Second),
        "millisecond" | "milliseconds" => Ok(DurationUnit::Millisecond),
        "microsecond" | "microseconds" => Ok(DurationUnit::Microsecond),
        _ => Err(LemmaError::engine(
            format!("Unknown duration unit: '{}'. Expected one of: years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds", unit_str),
            Source::new(attribute, span, doc_name),
            source_text,
            None::<String>,
        )),
    }
}

/// Parse a "number unit" string (e.g. "1 eur", "50 percent", "500 permille") into `(number, unit_name)`.
/// Does not validate the unit against any type; use `ScaleUnits::get()` or `RatioUnits::get()` for that.
/// Single canonical implementation used by both AST (Pest) and runtime string parsing for scale and ratio.
pub(crate) fn parse_number_unit_string(s: &str) -> Result<(Decimal, String), String> {
    let trimmed = s.trim();
    let mut parts = trimmed.split_whitespace();
    let number_part = parts.next().ok_or_else(|| {
        if trimmed.is_empty() {
            "Scale value cannot be empty. Use a number followed by a unit (e.g. '10 eur')."
                .to_string()
        } else {
            format!(
                "Invalid scale value: '{}'. Scale value must be a number followed by a unit (e.g. '10 eur').",
                s
            )
        }
    })?;
    let unit_part = parts.next().ok_or_else(|| {
        format!(
            "Scale value must include a unit (e.g. '{} eur').",
            number_part
        )
    })?;
    let clean = number_part.replace(['_', ','], "");
    let n = Decimal::from_str(&clean).map_err(|_| format!("Invalid scale: '{}'", s))?;
    Ok((n, unit_part.to_string()))
}

/// Parse a number+unit literal from AST (e.g. fact value "1 eur" in source).
/// Uses the same logic as `parse_scale_number_unit_string`; only the source (pair.as_str()) comes from Pest.
pub(crate) fn parse_number_unit_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<(Decimal, String), LemmaError> {
    let s = pair.as_str();
    let span = Span::from_pest_span(pair.as_span());
    parse_number_unit_string(s).map_err(|msg| {
        LemmaError::engine(
            msg,
            Source::new(attribute, span, doc_name),
            Arc::from(s),
            None::<String>,
        )
    })
}

fn parse_datetime_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Value, LemmaError> {
    let datetime_str = pair.as_str();

    if let Ok(dt) = datetime_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = dt.offset().local_minus_utc();
        return Ok(Value::Date(DateTimeValue {
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
        return Ok(Value::Date(DateTimeValue {
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
        return Ok(Value::Date(DateTimeValue {
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
        Source::new(
            attribute,
            Span::from_pest_span(pair.as_span()),
            doc_name,
        )
,
        Arc::from(datetime_str),
        None::<String>,
    ))
}

fn parse_time_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Value, LemmaError> {
    let time_str = pair.as_str();

    if let Ok(t) = time_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = t.offset().local_minus_utc();
        return Ok(Value::Time(TimeValue {
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
        return Ok(Value::Time(TimeValue {
            hour: t.hour() as u8,
            minute: t.minute() as u8,
            second: t.second() as u8,
            timezone: None,
        }));
    }

    Err(LemmaError::engine(
        format!("Invalid time format: '{}'\n         Expected: HH:MM or HH:MM:SS (e.g., 14:30 or 14:30:00)\n         With timezone: HH:MM:SSZ or +HH:MM (e.g., 14:30:00Z or 14:30:00+01:00)\n         Note: Hours must be 0-23, minutes and seconds must be 0-59", time_str),
        Source::new(
            attribute,
            Span::from_pest_span(pair.as_span()),
            doc_name,
        )
,
        Arc::from(time_str),
        None::<String>,
    ))
}

const MAX_DECIMAL_EXPONENT: i32 = 28;

fn parse_scientific_number(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Decimal, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let pair_str = pair.as_str();
    let mut inner = pair.into_inner();

    let mantissa_pair = inner.next().ok_or_else(|| {
        LemmaError::engine(
            "Missing mantissa in scientific notation",
            Source::new(attribute, span.clone(), doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;
    let exponent_pair = inner.next().ok_or_else(|| {
        LemmaError::engine(
            "Missing exponent in scientific notation",
            Source::new(attribute, span.clone(), doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;

    let mantissa = parse_decimal_number(
        mantissa_pair.as_str(),
        Span::from_pest_span(mantissa_pair.as_span()),
        attribute,
        doc_name,
        Arc::from(pair_str),
    )?;
    let exponent_span = Span::from_pest_span(exponent_pair.as_span());
    let exponent: i32 = exponent_pair.as_str().parse().map_err(|_| {
        LemmaError::engine(
            format!(
                "Invalid exponent: '{}'\n             Expected an integer between -{} and +{}",
                exponent_pair.as_str(),
                MAX_DECIMAL_EXPONENT,
                MAX_DECIMAL_EXPONENT
            ),
            Source::new(attribute, exponent_span.clone(), doc_name),
            Arc::from(exponent_pair.as_str()),
            None::<String>,
        )
    })?;

    let power_of_ten = decimal_pow10(exponent).ok_or_else(|| {
        LemmaError::engine(
            format!("Exponent {} is out of range\n             Maximum supported exponent is ±{} (values up to ~10^28)", exponent, MAX_DECIMAL_EXPONENT),
            Source::new(attribute, exponent_span, doc_name),
            Arc::from(exponent_pair.as_str()),
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
                Source::new(attribute, span, doc_name),
                Arc::from(pair_str),
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
                Source::new(attribute, span, doc_name),
                Arc::from(pair_str),
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

fn parse_decimal_number(
    number_str: &str,
    span: Span,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<Decimal, LemmaError> {
    let clean_number = number_str.replace(['_', ','], "");
    Decimal::from_str(&clean_number).map_err(|_| {
        LemmaError::engine(
            format!("Invalid number: '{}'\n             Expected a valid decimal number (e.g., 42, 3.14, 1_000_000, 1,000,000)\n             Note: Use underscores or commas as thousand separators if needed", number_str),
            Source::new(attribute, span, doc_name),
            source_text,
            None::<String>,
        )
    })
}

// ============================================================================
// String parsing helpers (for type constraint parsing)
// ============================================================================

/// Parse a date string into a DateTimeValue (for type constraint parsing)
pub fn parse_date_string(s: &str) -> Result<DateTimeValue, String> {
    use chrono::{Datelike, Timelike};

    if let Ok(dt) = s.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = dt.offset().local_minus_utc();
        return Ok(DateTimeValue {
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
        });
    }

    if let Ok(dt) = s.parse::<chrono::NaiveDateTime>() {
        return Ok(DateTimeValue {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            timezone: None,
        });
    }

    if let Ok(d) = s.parse::<chrono::NaiveDate>() {
        return Ok(DateTimeValue {
            year: d.year(),
            month: d.month(),
            day: d.day(),
            hour: 0,
            minute: 0,
            second: 0,
            timezone: None,
        });
    }

    Err(format!("Invalid date format: '{}'", s))
}

/// Parse a time string into a TimeValue (for type constraint parsing)
pub fn parse_time_string(s: &str) -> Result<TimeValue, String> {
    use chrono::Timelike;

    if let Ok(t) = s.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = t.offset().local_minus_utc();
        return Ok(TimeValue {
            hour: t.hour() as u8,
            minute: t.minute() as u8,
            second: t.second() as u8,
            timezone: Some(TimezoneValue {
                offset_hours: (offset / 3600) as i8,
                offset_minutes: ((offset % 3600) / 60) as u8,
            }),
        });
    }

    if let Ok(t) = s.parse::<chrono::NaiveTime>() {
        return Ok(TimeValue {
            hour: t.hour() as u8,
            minute: t.minute() as u8,
            second: t.second() as u8,
            timezone: None,
        });
    }

    Err(format!("Invalid time format: '{}'", s))
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

    #[test]
    fn parse_permille_double_percent_syntax() {
        use crate::parsing::ast::Value;
        use rust_decimal::Decimal;
        use std::str::FromStr;

        let input = "doc test\nrule x = 5%%";
        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let rule = &docs[0].rules[0];
        match &rule.expression.kind {
            crate::parsing::ast::ExpressionKind::Literal(Value::Ratio(n, Some(unit))) => {
                assert_eq!(*n, Decimal::from_str("0.005").unwrap());
                assert_eq!(unit, "permille");
            }
            other => panic!("Expected Ratio permille literal, got {:?}", other),
        }
    }

    #[test]
    fn parse_permille_word_syntax() {
        use crate::parsing::ast::Value;
        use rust_decimal::Decimal;
        use std::str::FromStr;

        let input = "doc test\nrule x = 5 permille";
        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let rule = &docs[0].rules[0];
        match &rule.expression.kind {
            crate::parsing::ast::ExpressionKind::Literal(Value::Ratio(n, Some(unit))) => {
                assert_eq!(*n, Decimal::from_str("0.005").unwrap());
                assert_eq!(unit, "permille");
            }
            other => panic!("Expected Ratio permille literal, got {:?}", other),
        }
    }

    #[test]
    fn parse_rejects_permille_literal_with_trailing_digits() {
        let input = "doc test\nfact x = 10%%5";
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(
            result.is_err(),
            "Permille literals like `10%%5` must be rejected"
        );
    }
}
