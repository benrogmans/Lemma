use crate::error::Error;
use crate::parsing::ast::*;
use crate::Source;

use chrono::{Datelike, Timelike};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Parse a duration string (e.g. "10 hours", "120 hours") into Value::Duration.
/// Single implementation for both Lemma source and runtime fact values.
pub(crate) fn parse_duration_from_string(value_str: &str, source: &Source) -> Result<Value, Error> {
    let trimmed = value_str.trim();
    let mut parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(Error::validation(
            format!(
                "Invalid duration: '{}'. Expected format: <number> <unit> (e.g. 10 hours, 2 weeks)",
                value_str
            ),
            Some(source.clone()),
            None::<String>,
        ));
    }
    let unit_str = parts.pop().unwrap();
    let number_str = parts.join(" ").replace(['_', ','], "");
    let digit_count = number_str.chars().filter(|c| c.is_ascii_digit()).count();
    if digit_count > crate::limits::MAX_NUMBER_DIGITS {
        return Err(Error::validation(
            format!(
                "Number has too many digits (max {})",
                crate::limits::MAX_NUMBER_DIGITS
            ),
            Some(source.clone()),
            None::<String>,
        ));
    }
    let n = Decimal::from_str(&number_str).map_err(|_| {
        Error::validation(
            format!("Invalid duration number: '{}'", number_str),
            Some(source.clone()),
            None::<String>,
        )
    })?;
    let unit_lower = unit_str.to_lowercase();
    let unit = match unit_lower.as_str() {
        "year" | "years" => DurationUnit::Year,
        "month" | "months" => DurationUnit::Month,
        "week" | "weeks" => DurationUnit::Week,
        "day" | "days" => DurationUnit::Day,
        "hour" | "hours" => DurationUnit::Hour,
        "minute" | "minutes" => DurationUnit::Minute,
        "second" | "seconds" => DurationUnit::Second,
        "millisecond" | "milliseconds" => DurationUnit::Millisecond,
        "microsecond" | "microseconds" => DurationUnit::Microsecond,
        _ => {
            return Err(Error::validation(
                format!(
                    "Unknown duration unit: '{}'. Expected one of: years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds",
                    unit_str
                ),
                Some(source.clone()),
                None::<String>,
            ));
        }
    };
    Ok(Value::Duration(n, unit))
}

/// Parse a "number unit" string (e.g. "1 eur", "50 percent", "500 permille") into `(number, unit_name)`.
/// Does not validate the unit against any type; use `ScaleUnits::get()` or `RatioUnits::get()` for that.
/// Single canonical implementation used by both AST and runtime string parsing for scale and ratio.
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
    let digit_count = clean.chars().filter(|c| c.is_ascii_digit()).count();
    if digit_count > crate::limits::MAX_NUMBER_DIGITS {
        return Err(format!(
            "Number has too many digits (max {})",
            crate::limits::MAX_NUMBER_DIGITS
        ));
    }
    let n = Decimal::from_str(&clean).map_err(|_| format!("Invalid scale: '{}'", s))?;
    Ok((n, unit_part.to_string()))
}

pub(crate) fn parse_datetime_str(s: &str) -> Option<DateTimeValue> {
    if let Ok(dt) = s.parse::<chrono::DateTime<chrono::FixedOffset>>() {
        let offset = dt.offset().local_minus_utc();
        let microsecond = dt.nanosecond() / 1000 % 1_000_000;
        return Some(DateTimeValue {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            microsecond,
            timezone: Some(TimezoneValue {
                offset_hours: (offset / 3600) as i8,
                offset_minutes: ((offset % 3600) / 60) as u8,
            }),
        });
    }
    if let Ok(dt) = s.parse::<chrono::NaiveDateTime>() {
        let microsecond = dt.nanosecond() / 1000 % 1_000_000;
        return Some(DateTimeValue {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            microsecond,
            timezone: None,
        });
    }
    if let Ok(d) = s.parse::<chrono::NaiveDate>() {
        return Some(DateTimeValue {
            year: d.year(),
            month: d.month(),
            day: d.day(),
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        });
    }
    None
}

/// Parse a date string into a DateTimeValue (for type constraint parsing)
pub fn parse_date_string(s: &str) -> Result<DateTimeValue, String> {
    if let Some(dtv) = parse_datetime_str(s) {
        return Ok(dtv);
    }
    if let Some(dtv) = DateTimeValue::parse(s) {
        return Ok(dtv);
    }
    Err(format!("Invalid date format: '{}'", s))
}

/// Parse a time string into a TimeValue (for type constraint parsing)
pub fn parse_time_string(s: &str) -> Result<TimeValue, String> {
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
