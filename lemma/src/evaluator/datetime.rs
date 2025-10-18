//! DateTime operations
//!
//! Handles arithmetic and comparisons with dates and datetimes.

use crate::{
    ArithmeticOperation, ComparisonOperator, DateTimeValue, LemmaError, LemmaResult,
    LiteralValue, TimeValue, TimezoneValue,
};
use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveDateTime,
    NaiveTime, TimeZone, Timelike,
};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

// Time constants
const SECONDS_PER_HOUR: i32 = 3600;
const SECONDS_PER_MINUTE: i32 = 60;
const MONTHS_PER_YEAR: u32 = 12;
const MILLISECONDS_PER_SECOND: f64 = 1000.0;

// Reference date for time-only calculations (Unix epoch)
const EPOCH_YEAR: i32 = 1970;
const EPOCH_MONTH: u32 = 1;
const EPOCH_DAY: u32 = 1;

/// Create a timezone-aware FixedOffset from an optional TimezoneValue.
/// Defaults to UTC if no timezone is specified.
fn create_timezone_offset(timezone: &Option<TimezoneValue>) -> LemmaResult<FixedOffset> {
    if let Some(tz) = timezone {
        let offset_seconds = (tz.offset_hours as i32 * SECONDS_PER_HOUR)
                           + (tz.offset_minutes as i32 * SECONDS_PER_MINUTE);
        FixedOffset::east_opt(offset_seconds).ok_or_else(|| {
            LemmaError::Engine(format!(
                "Invalid timezone offset: {}:{}",
                tz.offset_hours, tz.offset_minutes
            ))
        })
    } else {
        // Default to UTC (zero offset)
        Ok(FixedOffset::east_opt(0).unwrap())
    }
}

/// Perform date/datetime arithmetic
pub fn datetime_arithmetic(
    left: &LiteralValue,
    op: &ArithmeticOperation,
    right: &LiteralValue,
) -> LemmaResult<LiteralValue> {
    match (left, right, op) {
        // Date + Duration
        (
            LiteralValue::Date(date),
            LiteralValue::Unit(crate::NumericUnit::Duration(value, unit)),
            ArithmeticOperation::Add,
        ) => {
            let dt = datetime_value_to_chrono(date)?;

            let new_dt = match unit {
                crate::DurationUnit::Month => {
                    let months = value.to_i32()
                        .ok_or_else(|| LemmaError::Engine("Month value too large".to_string()))?;
                    dt.checked_add_months(chrono::Months::new(months as u32))
                        .ok_or_else(|| LemmaError::Engine("Date overflow".to_string()))?
                }
                crate::DurationUnit::Year => {
                    let years = value.to_i32()
                        .ok_or_else(|| LemmaError::Engine("Year value too large".to_string()))?;
                    dt.checked_add_months(chrono::Months::new((years * MONTHS_PER_YEAR as i32) as u32))
                        .ok_or_else(|| LemmaError::Engine("Date overflow".to_string()))?
                }
                _ => {
                    let seconds = crate::parser::units::duration_to_seconds(*value, unit);
                    let duration = seconds_to_chrono_duration(seconds)?;
                    dt.checked_add_signed(duration)
                        .ok_or_else(|| LemmaError::Engine("Date overflow".to_string()))?
                }
            };

            Ok(LiteralValue::Date(chrono_to_datetime_value(new_dt)))
        }

        // Date - Duration
        (
            LiteralValue::Date(date),
            LiteralValue::Unit(crate::NumericUnit::Duration(value, unit)),
            ArithmeticOperation::Subtract,
        ) => {
            let dt = datetime_value_to_chrono(date)?;

            let new_dt = match unit {
                crate::DurationUnit::Month => {
                    let months = value.to_i32()
                        .ok_or_else(|| LemmaError::Engine("Month value too large".to_string()))?;
                    dt.checked_sub_months(chrono::Months::new(months as u32))
                        .ok_or_else(|| LemmaError::Engine("Date overflow".to_string()))?
                }
                crate::DurationUnit::Year => {
                    let years = value.to_i32()
                        .ok_or_else(|| LemmaError::Engine("Year value too large".to_string()))?;
                    dt.checked_sub_months(chrono::Months::new((years * MONTHS_PER_YEAR as i32) as u32))
                        .ok_or_else(|| LemmaError::Engine("Date overflow".to_string()))?
                }
                _ => {
                    let seconds = crate::parser::units::duration_to_seconds(*value, unit);
                    let duration = seconds_to_chrono_duration(seconds)?;
                    dt.checked_sub_signed(duration)
                        .ok_or_else(|| LemmaError::Engine("Date overflow".to_string()))?
                }
            };

            Ok(LiteralValue::Date(chrono_to_datetime_value(new_dt)))
        }

        // Date - Date = Duration (in seconds)
        (
            LiteralValue::Date(left_date),
            LiteralValue::Date(right_date),
            ArithmeticOperation::Subtract,
        ) => {
            let left_dt = datetime_value_to_chrono(left_date)?;
            let right_dt = datetime_value_to_chrono(right_date)?;
            let duration = left_dt - right_dt;

            let seconds = Decimal::from(duration.num_seconds());
            Ok(LiteralValue::Unit(crate::NumericUnit::Duration(seconds, crate::DurationUnit::Second)))
        }

        _ => Err(LemmaError::Engine(format!(
            "DateTime arithmetic operation {:?} not supported for these operand types",
            op
        ))),
    }
}

/// Convert DateTimeValue to chrono DateTime, handling timezone if present
fn datetime_value_to_chrono(date: &DateTimeValue) -> LemmaResult<DateTime<FixedOffset>> {
    let naive_date = NaiveDate::from_ymd_opt(date.year, date.month, date.day).ok_or_else(|| {
        LemmaError::Engine(format!(
            "Invalid date: {}-{}-{}",
            date.year, date.month, date.day
        ))
    })?;

    let naive_time = chrono::NaiveTime::from_hms_opt(date.hour, date.minute, date.second)
        .ok_or_else(|| {
            LemmaError::Engine(format!(
                "Invalid time: {}:{}:{}",
                date.hour, date.minute, date.second
            ))
        })?;

    let naive_dt = NaiveDateTime::new(naive_date, naive_time);

    let offset = create_timezone_offset(&date.timezone)?;
    offset
        .from_local_datetime(&naive_dt)
        .single()
        .ok_or_else(|| LemmaError::Engine("Ambiguous or invalid datetime for timezone".to_string()))
}

/// Convert chrono DateTime back to DateTimeValue
fn chrono_to_datetime_value(dt: DateTime<FixedOffset>) -> DateTimeValue {
    let offset_seconds = dt.offset().local_minus_utc();
    let offset_hours = (offset_seconds / SECONDS_PER_HOUR) as i8;
    let offset_minutes = ((offset_seconds.abs() % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE) as u8;

    DateTimeValue {
        year: dt.year(),
        month: dt.month(),
        day: dt.day(),
        hour: dt.hour(),
        minute: dt.minute(),
        second: dt.second(),
        timezone: Some(TimezoneValue {
            offset_hours,
            offset_minutes,
        }),
    }
}

/// Convert seconds (Decimal) to chrono Duration
fn seconds_to_chrono_duration(seconds: Decimal) -> LemmaResult<ChronoDuration> {
    let seconds_f64 = seconds
        .to_f64()
        .ok_or_else(|| LemmaError::Engine("Duration conversion failed".to_string()))?;

    // Handle fractional seconds by converting to milliseconds for precision
    let milliseconds = (seconds_f64 * MILLISECONDS_PER_SECOND) as i64;
    Ok(ChronoDuration::milliseconds(milliseconds))
}

/// Perform date/datetime comparisons
pub fn datetime_comparison(
    left: &LiteralValue,
    op: &ComparisonOperator,
    right: &LiteralValue,
) -> LemmaResult<bool> {
    match (left, right) {
        // Date comparisons - convert both to UTC for fair comparison
        (LiteralValue::Date(l), LiteralValue::Date(r)) => {
            let l_dt = datetime_value_to_chrono(l)?;
            let r_dt = datetime_value_to_chrono(r)?;

            // Convert to UTC for comparison
            let l_utc = l_dt.naive_utc();
            let r_utc = r_dt.naive_utc();

            Ok(match op {
                ComparisonOperator::GreaterThan => l_utc > r_utc,
                ComparisonOperator::LessThan => l_utc < r_utc,
                ComparisonOperator::GreaterThanOrEqual => l_utc >= r_utc,
                ComparisonOperator::LessThanOrEqual => l_utc <= r_utc,
                ComparisonOperator::Equal | ComparisonOperator::Is => l_utc == r_utc,
                ComparisonOperator::NotEqual | ComparisonOperator::IsNot => l_utc != r_utc,
            })
        }

        _ => Err(LemmaError::Engine(
            "Invalid datetime comparison operands".to_string(),
        )),
    }
}

/// Perform time arithmetic operations
pub fn time_arithmetic(
    left: &LiteralValue,
    op: &ArithmeticOperation,
    right: &LiteralValue,
) -> LemmaResult<LiteralValue> {
    match (left, right, op) {
        // Time + Duration = Time
        (
            LiteralValue::Time(time),
            LiteralValue::Unit(crate::NumericUnit::Duration(value, unit)),
            ArithmeticOperation::Add,
        ) => {
            let seconds = crate::parser::units::duration_to_seconds(*value, unit);
            let time_aware = time_value_to_chrono_datetime(time)?;
            let duration = seconds_to_chrono_duration(seconds)?;
            let result_dt = time_aware + duration;
            Ok(LiteralValue::Time(chrono_datetime_to_time_value(result_dt)))
        }

        // Time - Duration = Time
        (
            LiteralValue::Time(time),
            LiteralValue::Unit(crate::NumericUnit::Duration(value, unit)),
            ArithmeticOperation::Subtract,
        ) => {
            let seconds = crate::parser::units::duration_to_seconds(*value, unit);
            let time_aware = time_value_to_chrono_datetime(time)?;
            let duration = seconds_to_chrono_duration(seconds)?;
            let result_dt = time_aware - duration;
            Ok(LiteralValue::Time(chrono_datetime_to_time_value(result_dt)))
        }

        // Time - Time = Duration (in seconds)
        (
            LiteralValue::Time(left_time),
            LiteralValue::Time(right_time),
            ArithmeticOperation::Subtract,
        ) => {
            let left_dt = time_value_to_chrono_datetime(left_time)?;
            let right_dt = time_value_to_chrono_datetime(right_time)?;

            // Convert to UTC and get difference in seconds
            let diff = left_dt.naive_utc() - right_dt.naive_utc();
            let diff_seconds = diff.num_seconds();
            let seconds = Decimal::from(diff_seconds);

            Ok(LiteralValue::Unit(crate::NumericUnit::Duration(seconds, crate::DurationUnit::Second)))
        }

        _ => Err(LemmaError::Engine(format!(
            "Time arithmetic operation {:?} not supported for these operand types",
            op
        ))),
    }
}

/// Convert TimeValue to timezone-aware DateTime (using epoch date for calculation)
fn time_value_to_chrono_datetime(time: &TimeValue) -> LemmaResult<DateTime<FixedOffset>> {
    // Use Unix epoch as reference date for time-only arithmetic
    let naive_date = NaiveDate::from_ymd_opt(EPOCH_YEAR, EPOCH_MONTH, EPOCH_DAY).unwrap();
    let naive_time =
        NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, time.second as u32)
            .ok_or_else(|| {
                LemmaError::Engine(format!(
                    "Invalid time: {}:{}:{}",
                    time.hour, time.minute, time.second
                ))
            })?;

    let naive_dt = NaiveDateTime::new(naive_date, naive_time);

    let offset = create_timezone_offset(&time.timezone)?;
    offset
        .from_local_datetime(&naive_dt)
        .single()
        .ok_or_else(|| LemmaError::Engine("Ambiguous or invalid time for timezone".to_string()))
}

/// Convert chrono DateTime back to TimeValue
fn chrono_datetime_to_time_value(dt: DateTime<FixedOffset>) -> TimeValue {
    let offset_seconds = dt.offset().local_minus_utc();
    let offset_hours = (offset_seconds / SECONDS_PER_HOUR) as i8;
    let offset_minutes = ((offset_seconds.abs() % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE) as u8;

    TimeValue {
        hour: dt.hour() as u8,
        minute: dt.minute() as u8,
        second: dt.second() as u8,
        timezone: Some(TimezoneValue {
            offset_hours,
            offset_minutes,
        }),
    }
}
