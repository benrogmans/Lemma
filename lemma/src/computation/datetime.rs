//! DateTime operations
//!
//! Handles arithmetic and comparisons with dates and datetimes.
//! Returns OperationResult with Veto for errors instead of Result.

use crate::evaluation::OperationResult;
use crate::{
    ArithmeticComputation, ComparisonComputation, DateTimeValue, LiteralValue, TimeValue,
    TimezoneValue,
};
use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveDateTime,
    NaiveTime, TimeZone, Timelike,
};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

const SECONDS_PER_HOUR: i32 = 3600;
const SECONDS_PER_MINUTE: i32 = 60;
const MONTHS_PER_YEAR: u32 = 12;
const MILLISECONDS_PER_SECOND: f64 = 1000.0;

const EPOCH_YEAR: i32 = 1970;
const EPOCH_MONTH: u32 = 1;
const EPOCH_DAY: u32 = 1;

fn create_timezone_offset(timezone: &Option<TimezoneValue>) -> Result<FixedOffset, String> {
    if let Some(tz) = timezone {
        let offset_seconds = (tz.offset_hours as i32 * SECONDS_PER_HOUR)
            + (tz.offset_minutes as i32 * SECONDS_PER_MINUTE);
        FixedOffset::east_opt(offset_seconds).ok_or_else(|| {
            format!(
                "Invalid timezone offset: {}:{}",
                tz.offset_hours, tz.offset_minutes
            )
        })
    } else {
        FixedOffset::east_opt(0).ok_or_else(|| "Failed to create UTC offset".to_string())
    }
}

/// Perform date/datetime arithmetic, returning OperationResult (Veto on error)
pub fn datetime_arithmetic(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (left, right, op) {
        (
            LiteralValue::Date(date),
            LiteralValue::Unit(crate::NumericUnit::Duration(value, unit)),
            ArithmeticComputation::Add,
        ) => {
            let dt = match datetime_value_to_chrono(date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            let new_dt = match unit {
                crate::DurationUnit::Month => {
                    let months = match value.to_i32() {
                        Some(m) => m,
                        None => {
                            return OperationResult::Veto(Some("Month value too large".to_string()))
                        }
                    };
                    match dt.checked_add_months(chrono::Months::new(months as u32)) {
                        Some(d) => d,
                        None => return OperationResult::Veto(Some("Date overflow".to_string())),
                    }
                }
                crate::DurationUnit::Year => {
                    let years = match value.to_i32() {
                        Some(y) => y,
                        None => {
                            return OperationResult::Veto(Some("Year value too large".to_string()))
                        }
                    };
                    match dt.checked_add_months(chrono::Months::new(
                        (years * MONTHS_PER_YEAR as i32) as u32,
                    )) {
                        Some(d) => d,
                        None => return OperationResult::Veto(Some("Date overflow".to_string())),
                    }
                }
                _ => {
                    let seconds = crate::parsing::units::duration_to_seconds(*value, unit);
                    let duration = match seconds_to_chrono_duration(seconds) {
                        Ok(d) => d,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    };
                    match dt.checked_add_signed(duration) {
                        Some(d) => d,
                        None => return OperationResult::Veto(Some("Date overflow".to_string())),
                    }
                }
            };

            OperationResult::Value(LiteralValue::Date(chrono_to_datetime_value(new_dt)))
        }

        (
            LiteralValue::Date(date),
            LiteralValue::Unit(crate::NumericUnit::Duration(value, unit)),
            ArithmeticComputation::Subtract,
        ) => {
            let dt = match datetime_value_to_chrono(date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            let new_dt = match unit {
                crate::DurationUnit::Month => {
                    let months = match value.to_i32() {
                        Some(m) => m,
                        None => {
                            return OperationResult::Veto(Some("Month value too large".to_string()))
                        }
                    };
                    match dt.checked_sub_months(chrono::Months::new(months as u32)) {
                        Some(d) => d,
                        None => return OperationResult::Veto(Some("Date overflow".to_string())),
                    }
                }
                crate::DurationUnit::Year => {
                    let years = match value.to_i32() {
                        Some(y) => y,
                        None => {
                            return OperationResult::Veto(Some("Year value too large".to_string()))
                        }
                    };
                    match dt.checked_sub_months(chrono::Months::new(
                        (years * MONTHS_PER_YEAR as i32) as u32,
                    )) {
                        Some(d) => d,
                        None => return OperationResult::Veto(Some("Date overflow".to_string())),
                    }
                }
                _ => {
                    let seconds = crate::parsing::units::duration_to_seconds(*value, unit);
                    let duration = match seconds_to_chrono_duration(seconds) {
                        Ok(d) => d,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    };
                    match dt.checked_sub_signed(duration) {
                        Some(d) => d,
                        None => return OperationResult::Veto(Some("Date overflow".to_string())),
                    }
                }
            };

            OperationResult::Value(LiteralValue::Date(chrono_to_datetime_value(new_dt)))
        }

        (
            LiteralValue::Date(left_date),
            LiteralValue::Date(right_date),
            ArithmeticComputation::Subtract,
        ) => {
            let left_dt = match datetime_value_to_chrono(left_date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let right_dt = match datetime_value_to_chrono(right_date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let duration = left_dt - right_dt;

            let seconds = Decimal::from(duration.num_seconds());
            OperationResult::Value(LiteralValue::Unit(crate::NumericUnit::Duration(
                seconds,
                crate::DurationUnit::Second,
            )))
        }

        _ => OperationResult::Veto(Some(format!(
            "DateTime arithmetic operation {:?} not supported for these operand types",
            op
        ))),
    }
}

fn datetime_value_to_chrono(date: &DateTimeValue) -> Result<DateTime<FixedOffset>, String> {
    let naive_date = NaiveDate::from_ymd_opt(date.year, date.month, date.day)
        .ok_or_else(|| format!("Invalid date: {}-{}-{}", date.year, date.month, date.day))?;

    let naive_time =
        NaiveTime::from_hms_opt(date.hour, date.minute, date.second).ok_or_else(|| {
            format!(
                "Invalid time: {}:{}:{}",
                date.hour, date.minute, date.second
            )
        })?;

    let naive_dt = NaiveDateTime::new(naive_date, naive_time);

    let offset = create_timezone_offset(&date.timezone)?;
    offset
        .from_local_datetime(&naive_dt)
        .single()
        .ok_or_else(|| "Ambiguous or invalid datetime for timezone".to_string())
}

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

fn seconds_to_chrono_duration(seconds: Decimal) -> Result<ChronoDuration, String> {
    let seconds_f64 = seconds
        .to_f64()
        .ok_or_else(|| "Duration conversion failed".to_string())?;

    let milliseconds = (seconds_f64 * MILLISECONDS_PER_SECOND) as i64;
    Ok(ChronoDuration::milliseconds(milliseconds))
}

/// Perform date/datetime comparisons, returning OperationResult (Veto on error)
pub fn datetime_comparison(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (left, right) {
        (LiteralValue::Date(l), LiteralValue::Date(r)) => {
            let l_dt = match datetime_value_to_chrono(l) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let r_dt = match datetime_value_to_chrono(r) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            let l_utc = l_dt.naive_utc();
            let r_utc = r_dt.naive_utc();

            let result = match op {
                ComparisonComputation::GreaterThan => l_utc > r_utc,
                ComparisonComputation::LessThan => l_utc < r_utc,
                ComparisonComputation::GreaterThanOrEqual => l_utc >= r_utc,
                ComparisonComputation::LessThanOrEqual => l_utc <= r_utc,
                ComparisonComputation::Equal | ComparisonComputation::Is => l_utc == r_utc,
                ComparisonComputation::NotEqual | ComparisonComputation::IsNot => l_utc != r_utc,
            };

            OperationResult::Value(LiteralValue::Boolean(result.into()))
        }

        _ => OperationResult::Veto(Some("Invalid datetime comparison operands".to_string())),
    }
}

/// Perform time arithmetic operations, returning OperationResult (Veto on error)
pub fn time_arithmetic(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (left, right, op) {
        (
            LiteralValue::Time(time),
            LiteralValue::Unit(crate::NumericUnit::Duration(value, unit)),
            ArithmeticComputation::Add,
        ) => {
            let seconds = crate::parsing::units::duration_to_seconds(*value, unit);
            let time_aware = match time_value_to_chrono_datetime(time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let duration = match seconds_to_chrono_duration(seconds) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let result_dt = time_aware + duration;
            OperationResult::Value(LiteralValue::Time(chrono_datetime_to_time_value(result_dt)))
        }

        (
            LiteralValue::Time(time),
            LiteralValue::Unit(crate::NumericUnit::Duration(value, unit)),
            ArithmeticComputation::Subtract,
        ) => {
            let seconds = crate::parsing::units::duration_to_seconds(*value, unit);
            let time_aware = match time_value_to_chrono_datetime(time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let duration = match seconds_to_chrono_duration(seconds) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let result_dt = time_aware - duration;
            OperationResult::Value(LiteralValue::Time(chrono_datetime_to_time_value(result_dt)))
        }

        (
            LiteralValue::Time(left_time),
            LiteralValue::Time(right_time),
            ArithmeticComputation::Subtract,
        ) => {
            let left_dt = match time_value_to_chrono_datetime(left_time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let right_dt = match time_value_to_chrono_datetime(right_time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            let diff = left_dt.naive_utc() - right_dt.naive_utc();
            let diff_seconds = diff.num_seconds();
            let seconds = Decimal::from(diff_seconds);

            OperationResult::Value(LiteralValue::Unit(crate::NumericUnit::Duration(
                seconds,
                crate::DurationUnit::Second,
            )))
        }

        _ => OperationResult::Veto(Some(format!(
            "Time arithmetic operation {:?} not supported for these operand types",
            op
        ))),
    }
}

fn time_value_to_chrono_datetime(time: &TimeValue) -> Result<DateTime<FixedOffset>, String> {
    let naive_date =
        NaiveDate::from_ymd_opt(EPOCH_YEAR, EPOCH_MONTH, EPOCH_DAY).ok_or_else(|| {
            format!(
                "Invalid epoch date: {}-{}-{}",
                EPOCH_YEAR, EPOCH_MONTH, EPOCH_DAY
            )
        })?;
    let naive_time =
        NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, time.second as u32)
            .ok_or_else(|| {
                format!(
                    "Invalid time: {}:{}:{}",
                    time.hour, time.minute, time.second
                )
            })?;

    let naive_dt = NaiveDateTime::new(naive_date, naive_time);

    let offset = create_timezone_offset(&time.timezone)?;
    offset
        .from_local_datetime(&naive_dt)
        .single()
        .ok_or_else(|| "Ambiguous or invalid time for timezone".to_string())
}

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
