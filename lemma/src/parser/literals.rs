use crate::error::LemmaError;
use crate::parser::Rule;
use crate::semantic::*;

use chrono::{Datelike, Timelike};
use pest::iterators::Pair;
use regex;
use rust_decimal::Decimal;
use std::str::FromStr;

pub(crate) fn parse_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    match pair.as_rule() {
        Rule::number_literal => parse_number_literal(pair),
        Rule::string_literal => parse_string_literal(pair),
        Rule::boolean_literal => parse_boolean_literal(pair),
        Rule::percentage_literal => parse_percentage_literal(pair),
        Rule::regex_literal => parse_regex_literal(pair),
        Rule::date_time_literal => parse_datetime_literal(pair),
        Rule::time_literal => parse_time_literal(pair),
        Rule::unit_literal => parse_unit_literal(pair),
        _ => Err(LemmaError::Engine(format!(
            "Unsupported literal type: {:?}",
            pair.as_rule()
        ))),
    }
}

fn parse_number_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let pair_str = pair.as_str();
    let mut inner = pair.into_inner();

    let number = match inner.next() {
        Some(inner_pair) => match inner_pair.as_rule() {
            Rule::scientific_number => parse_scientific_number(inner_pair)?,
            Rule::decimal_number => parse_decimal_number(inner_pair.as_str())?,
            _ => {
                return Err(LemmaError::Engine(
                    "Unexpected number literal structure".to_string(),
                ))
            }
        },
        None => parse_decimal_number(pair_str)?,
    };

    Ok(LiteralValue::Number(number))
}

fn parse_string_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let content = pair.as_str();
    let unquoted = &content[1..content.len() - 1];
    Ok(LiteralValue::Text(unquoted.to_string()))
}

/// Parse boolean literals.
/// Accepts: true, false, yes, no, accept, reject (case-sensitive)
fn parse_boolean_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let boolean = match pair.as_str() {
        "true" | "yes" | "accept" => true,
        "false" | "no" | "reject" => false,
        _ => {
            return Err(LemmaError::Engine(format!(
                "Invalid boolean: '{}'\n\
             Expected one of: true, false, yes, no, accept, reject",
                pair.as_str()
            )))
        }
    };
    Ok(LiteralValue::Boolean(boolean))
}

fn parse_percentage_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::number_literal {
            let percentage = parse_number_literal(inner_pair)?;
            match percentage {
                LiteralValue::Number(n) => return Ok(LiteralValue::Percentage(n)),
                _ => {
                    return Err(LemmaError::Engine(
                        "Expected number in percentage literal".to_string(),
                    ))
                }
            }
        }
    }
    Err(LemmaError::Engine(
        "Invalid percentage literal: missing number".to_string(),
    ))
}

/// Parse regex literals enclosed in forward slashes (e.g., /pattern/)
/// Validates that the pattern is a valid regular expression
fn parse_regex_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let regex_str = pair.as_str().to_string();
    let mut pattern_parts = Vec::new();
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::regex_char {
            pattern_parts.push(inner_pair.as_str());
        }
    }
    let pattern = pattern_parts.join("");
    match regex::Regex::new(&pattern) {
        Ok(_) => Ok(LiteralValue::Regex(regex_str)),
        Err(e) => Err(LemmaError::Engine(format!(
            "Invalid regex pattern in '{}': {}\n\
             Note: Use /pattern/ syntax, escape forward slashes as \\/",
            regex_str, e
        ))),
    }
}

// Complex Literals

fn parse_unit_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let mut number = None;
    let mut unit_str = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::number_literal => {
                let lit = parse_number_literal(inner_pair)?;
                match lit {
                    LiteralValue::Number(n) => number = Some(n),
                    _ => {
                        return Err(LemmaError::Engine(
                            "Expected number in unit literal".to_string(),
                        ))
                    }
                }
            }
            Rule::unit_word => {
                unit_str = Some(inner_pair.as_str());
            }
            _ => {}
        }
    }

    let value =
        number.ok_or_else(|| LemmaError::Engine("Missing number in unit literal".to_string()))?;
    let unit =
        unit_str.ok_or_else(|| LemmaError::Engine("Missing unit in unit literal".to_string()))?;

    // Resolve the unit string to a LiteralValue
    super::units::resolve_unit(value, unit)
}

/// Parse date/time literals with comprehensive error messages.
/// Supports formats:
/// - Date only: YYYY-MM-DD (e.g., 2024-01-15)
/// - DateTime: YYYY-MM-DDTHH:MM:SS (e.g., 2024-01-15T14:30:00)
/// - With timezone: YYYY-MM-DDTHH:MM:SSZ or YYYY-MM-DDTHH:MM:SS+HH:MM
fn parse_datetime_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let datetime_str = pair.as_str();

    // Try datetime with timezone first
    if let Ok(dt) = datetime_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = dt.offset().local_minus_utc();
        return Ok(LiteralValue::Date(DateTimeValue {
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

    // Try datetime without timezone
    if let Ok(dt) = datetime_str.parse::<chrono::NaiveDateTime>() {
        return Ok(LiteralValue::Date(DateTimeValue {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            timezone: None,
        }));
    }

    // Try date only
    if let Ok(d) = datetime_str.parse::<chrono::NaiveDate>() {
        return Ok(LiteralValue::Date(DateTimeValue {
            year: d.year(),
            month: d.month(),
            day: d.day(),
            hour: 0,
            minute: 0,
            second: 0,
            timezone: None,
        }));
    }

    // Provide helpful error message
    Err(LemmaError::Engine(format!(
        "Invalid date/time format: '{}'\n\
         Expected one of:\n\
         - Date: YYYY-MM-DD (e.g., 2024-01-15)\n\
         - DateTime: YYYY-MM-DDTHH:MM:SS (e.g., 2024-01-15T14:30:00)\n\
         - With timezone: YYYY-MM-DDTHH:MM:SSZ or +HH:MM (e.g., 2024-01-15T14:30:00Z)\n\
         Note: Month must be 1-12, day must be valid for the month (no Feb 30), hours 0-23, minutes/seconds 0-59",
        datetime_str
    )))
}

/// Parse time literals with comprehensive error messages.
/// Supports formats:
/// - Time: HH:MM or HH:MM:SS (e.g., 14:30 or 14:30:00)
/// - With timezone: HH:MM:SSZ or HH:MM:SS+HH:MM
fn parse_time_literal(pair: Pair<Rule>) -> Result<LiteralValue, LemmaError> {
    let time_str = pair.as_str();

    // Try time with timezone first
    if let Ok(t) = time_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = t.offset().local_minus_utc();
        return Ok(LiteralValue::Time(TimeValue {
            hour: t.hour() as u8,
            minute: t.minute() as u8,
            second: t.second() as u8,
            timezone: Some(TimezoneValue {
                offset_hours: (offset / 3600) as i8,
                offset_minutes: ((offset % 3600) / 60) as u8,
            }),
        }));
    }

    // Try time without timezone
    if let Ok(t) = time_str.parse::<chrono::NaiveTime>() {
        return Ok(LiteralValue::Time(TimeValue {
            hour: t.hour() as u8,
            minute: t.minute() as u8,
            second: t.second() as u8,
            timezone: None,
        }));
    }

    // Provide helpful error message
    Err(LemmaError::Engine(format!(
        "Invalid time format: '{}'\n\
         Expected: HH:MM or HH:MM:SS (e.g., 14:30 or 14:30:00)\n\
         With timezone: HH:MM:SSZ or +HH:MM (e.g., 14:30:00Z or 14:30:00+01:00)\n\
         Note: Hours must be 0-23, minutes and seconds must be 0-59",
        time_str
    )))
}

// rust_decimal limits: max value ~10^28 (fits in 96 bits), max scale 28 decimal places
// This means we can safely handle exponents from -28 to +28
const MAX_DECIMAL_EXPONENT: i32 = 28;

/// Parse scientific notation numbers (e.g., 1.23e+5, 5.67E-3, 1e10).
/// Converts mantissa * 10^exponent to a Decimal value.
fn parse_scientific_number(pair: Pair<Rule>) -> Result<Decimal, LemmaError> {
    let mut inner = pair.into_inner();

    let mantissa_pair = inner
        .next()
        .ok_or_else(|| LemmaError::Engine("Missing mantissa in scientific notation".to_string()))?;
    let exponent_pair = inner
        .next()
        .ok_or_else(|| LemmaError::Engine("Missing exponent in scientific notation".to_string()))?;

    let mantissa = parse_decimal_number(mantissa_pair.as_str())?;
    let exponent: i32 = exponent_pair.as_str().parse().map_err(|_| {
        LemmaError::Engine(format!(
            "Invalid exponent: '{}'\n\
             Expected an integer between -{} and +{}",
            exponent_pair.as_str(),
            MAX_DECIMAL_EXPONENT,
            MAX_DECIMAL_EXPONENT
        ))
    })?;

    let power_of_ten = decimal_pow10(exponent).ok_or_else(|| {
        LemmaError::Engine(format!(
            "Exponent {} is out of range\n\
             Maximum supported exponent is ±{} (values up to ~10^28)",
            exponent, MAX_DECIMAL_EXPONENT
        ))
    })?;

    // For positive exponents, multiply (1e3 = 1000)
    // For negative exponents, divide (1e-3 = 0.001)
    if exponent >= 0 {
        mantissa.checked_mul(power_of_ten).ok_or_else(|| {
            LemmaError::Engine(format!(
                "Number overflow: result of {}e{} exceeds maximum value (~10^28)",
                mantissa, exponent
            ))
        })
    } else {
        mantissa.checked_div(power_of_ten).ok_or_else(|| {
            LemmaError::Engine(format!(
                "Precision error: result of {}e{} has too many decimal places (max 28)",
                mantissa, exponent
            ))
        })
    }
}

/// Calculate 10^exp as a Decimal value
/// Returns None if the exponent exceeds Decimal's precision limits
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

/// Parse a decimal number, supporting underscores as digit separators.
/// Examples: 42, 3.14, 1_000_000, -5.5
fn parse_decimal_number(number_str: &str) -> Result<Decimal, LemmaError> {
    let clean_number = number_str.replace('_', "");
    Decimal::from_str(&clean_number).map_err(|_| {
        LemmaError::Engine(format!(
            "Invalid number: '{}'\n\
             Expected a valid decimal number (e.g., 42, 3.14, 1_000_000)\n\
             Note: Use underscores as thousand separators if needed",
            number_str
        ))
    })
}
