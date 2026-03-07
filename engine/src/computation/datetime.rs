//! DateTime operations
//!
//! Handles arithmetic and comparisons with dates and datetimes.
//! Returns OperationResult with Veto for errors instead of Result.

use crate::evaluation::OperationResult;
use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, LiteralValue, SemanticDateTime,
    SemanticDurationUnit, SemanticTime, SemanticTimezone, ValueKind,
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

fn create_semantic_timezone_offset(
    timezone: &Option<SemanticTimezone>,
) -> Result<FixedOffset, String> {
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
    match (&left.value, &right.value, op) {
        (ValueKind::Date(date), ValueKind::Duration(value, unit), ArithmeticComputation::Add) => {
            let dt = match semantic_datetime_to_chrono(date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            let new_dt = match unit {
                SemanticDurationUnit::Month => {
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
                SemanticDurationUnit::Year => {
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
                    let seconds = super::units::duration_to_seconds(*value, unit);
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

            OperationResult::Value(Box::new(LiteralValue::date_with_type(
                chrono_to_semantic_datetime(new_dt),
                left.lemma_type.clone(),
            )))
        }

        (
            ValueKind::Date(date),
            ValueKind::Duration(value, unit),
            ArithmeticComputation::Subtract,
        ) => {
            let dt = match semantic_datetime_to_chrono(date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            let new_dt = match unit {
                SemanticDurationUnit::Month => {
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
                SemanticDurationUnit::Year => {
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
                    let seconds = super::units::duration_to_seconds(*value, unit);
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

            OperationResult::Value(Box::new(LiteralValue::date_with_type(
                chrono_to_semantic_datetime(new_dt),
                left.lemma_type.clone(),
            )))
        }

        (
            ValueKind::Date(left_date),
            ValueKind::Date(right_date),
            ArithmeticComputation::Subtract,
        ) => {
            let left_dt = match semantic_datetime_to_chrono(left_date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let right_dt = match semantic_datetime_to_chrono(right_date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let duration = left_dt - right_dt;

            let seconds = Decimal::from(duration.num_seconds());
            OperationResult::Value(Box::new(LiteralValue::duration(
                seconds,
                SemanticDurationUnit::Second,
            )))
        }

        // Duration + Date → Date
        (ValueKind::Duration(value, unit), ValueKind::Date(date), ArithmeticComputation::Add) => {
            let dt = match semantic_datetime_to_chrono(date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            let new_dt = match unit {
                SemanticDurationUnit::Month => {
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
                SemanticDurationUnit::Year => {
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
                    let seconds = super::units::duration_to_seconds(*value, unit);
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

            OperationResult::Value(Box::new(LiteralValue::date_with_type(
                chrono_to_semantic_datetime(new_dt),
                right.lemma_type.clone(),
            )))
        }

        (ValueKind::Date(date), ValueKind::Time(time), ArithmeticComputation::Subtract) => {
            // Date - Time: Create a datetime from the date's date components and the time's time components
            // Then subtract to get the duration
            let date_dt = match semantic_datetime_to_chrono(date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            // Create a datetime using the date's date components and the time's time components
            let naive_date = match NaiveDate::from_ymd_opt(date.year, date.month, date.day) {
                Some(d) => d,
                None => {
                    return OperationResult::Veto(Some(format!(
                        "Invalid date: {}-{}-{}",
                        date.year, date.month, date.day
                    )))
                }
            };
            let naive_time = match NaiveTime::from_hms_opt(time.hour, time.minute, time.second) {
                Some(t) => t,
                None => {
                    return OperationResult::Veto(Some(format!(
                        "Invalid time: {}:{}:{}",
                        time.hour, time.minute, time.second
                    )))
                }
            };
            let naive_dt = NaiveDateTime::new(naive_date, naive_time);

            // Use the date's timezone, or UTC if not specified
            let offset = match create_semantic_timezone_offset(&date.timezone) {
                Ok(o) => o,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let time_dt = match offset.from_local_datetime(&naive_dt).single() {
                Some(dt) => dt,
                None => {
                    return OperationResult::Veto(Some(
                        "Ambiguous or invalid datetime for timezone".to_string(),
                    ))
                }
            };

            let duration = date_dt - time_dt;
            let seconds = Decimal::from(duration.num_seconds());
            OperationResult::Value(Box::new(LiteralValue::duration(
                seconds,
                SemanticDurationUnit::Second,
            )))
        }

        _ => OperationResult::Veto(Some(format!(
            "DateTime arithmetic operation {:?} not supported for these operand types",
            op
        ))),
    }
}

fn semantic_datetime_to_chrono(date: &SemanticDateTime) -> Result<DateTime<FixedOffset>, String> {
    let naive_date = NaiveDate::from_ymd_opt(date.year, date.month, date.day)
        .ok_or_else(|| format!("Invalid date: {}-{}-{}", date.year, date.month, date.day))?;

    let naive_time =
        NaiveTime::from_hms_micro_opt(date.hour, date.minute, date.second, date.microsecond)
            .ok_or_else(|| {
                format!(
                    "Invalid time: {}:{}:{}.{}",
                    date.hour, date.minute, date.second, date.microsecond
                )
            })?;

    let naive_dt = NaiveDateTime::new(naive_date, naive_time);

    let offset = create_semantic_timezone_offset(&date.timezone)?;
    offset
        .from_local_datetime(&naive_dt)
        .single()
        .ok_or_else(|| "Ambiguous or invalid datetime for timezone".to_string())
}

fn chrono_to_semantic_datetime(dt: DateTime<FixedOffset>) -> SemanticDateTime {
    let offset_seconds = dt.offset().local_minus_utc();
    let offset_hours = (offset_seconds / SECONDS_PER_HOUR) as i8;
    let offset_minutes = ((offset_seconds.abs() % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE) as u8;

    SemanticDateTime {
        year: dt.year(),
        month: dt.month(),
        day: dt.day(),
        hour: dt.hour(),
        minute: dt.minute(),
        second: dt.second(),
        microsecond: dt.nanosecond() / 1000 % 1_000_000,
        timezone: Some(SemanticTimezone {
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
    match (&left.value, &right.value) {
        (ValueKind::Date(l), ValueKind::Date(r)) => {
            let l_dt = match semantic_datetime_to_chrono(l) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let r_dt = match semantic_datetime_to_chrono(r) {
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

            OperationResult::Value(Box::new(LiteralValue::from_bool(result)))
        }

        _ => OperationResult::Veto(Some("Invalid datetime comparison operands".to_string())),
    }
}

/// Perform time comparisons, returning OperationResult (Veto on error)
pub fn time_comparison(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (&left.value, &right.value) {
        (ValueKind::Time(l), ValueKind::Time(r)) => {
            let l_dt = match semantic_time_to_chrono_datetime(l) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let r_dt = match semantic_time_to_chrono_datetime(r) {
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

            OperationResult::Value(Box::new(LiteralValue::from_bool(result)))
        }
        _ => unreachable!(
            "BUG: time_comparison called with non-time operands; this should be enforced by planning and dispatch"
        ),
    }
}

/// Perform time arithmetic operations, returning OperationResult (Veto on error)
pub fn time_arithmetic(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (&left.value, &right.value, op) {
        (ValueKind::Time(time), ValueKind::Duration(value, unit), ArithmeticComputation::Add) => {
            let seconds = super::units::duration_to_seconds(*value, unit);
            let time_aware = match semantic_time_to_chrono_datetime(time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let duration = match seconds_to_chrono_duration(seconds) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let result_dt = time_aware + duration;
            OperationResult::Value(Box::new(LiteralValue::time_with_type(
                chrono_datetime_to_semantic_time(result_dt),
                left.lemma_type.clone(),
            )))
        }

        (
            ValueKind::Time(time),
            ValueKind::Duration(value, unit),
            ArithmeticComputation::Subtract,
        ) => {
            let seconds = super::units::duration_to_seconds(*value, unit);
            let time_aware = match semantic_time_to_chrono_datetime(time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let duration = match seconds_to_chrono_duration(seconds) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let result_dt = time_aware - duration;
            OperationResult::Value(Box::new(LiteralValue::time_with_type(
                chrono_datetime_to_semantic_time(result_dt),
                left.lemma_type.clone(),
            )))
        }

        (
            ValueKind::Time(left_time),
            ValueKind::Time(right_time),
            ArithmeticComputation::Subtract,
        ) => {
            let left_dt = match semantic_time_to_chrono_datetime(left_time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let right_dt = match semantic_time_to_chrono_datetime(right_time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            let diff = left_dt.naive_utc() - right_dt.naive_utc();
            let diff_seconds = diff.num_seconds();
            let seconds = Decimal::from(diff_seconds);

            OperationResult::Value(Box::new(LiteralValue::duration(
                seconds,
                SemanticDurationUnit::Second,
            )))
        }

        // Duration + Time → Time
        (ValueKind::Duration(value, unit), ValueKind::Time(time), ArithmeticComputation::Add) => {
            let seconds = super::units::duration_to_seconds(*value, unit);
            let time_aware = match semantic_time_to_chrono_datetime(time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let duration = match seconds_to_chrono_duration(seconds) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let result_dt = time_aware + duration;
            OperationResult::Value(Box::new(LiteralValue::time_with_type(
                chrono_datetime_to_semantic_time(result_dt),
                right.lemma_type.clone(),
            )))
        }

        (ValueKind::Time(time), ValueKind::Date(date), ArithmeticComputation::Subtract) => {
            // Time - Date: Create a datetime from the date's date components and the time's time components
            // Then subtract to get the duration
            let time_dt = match semantic_time_to_chrono_datetime(time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };

            // Create a datetime using the date's date components and the time's time components
            let naive_date = match NaiveDate::from_ymd_opt(date.year, date.month, date.day) {
                Some(d) => d,
                None => {
                    return OperationResult::Veto(Some(format!(
                        "Invalid date: {}-{}-{}",
                        date.year, date.month, date.day
                    )))
                }
            };
            let naive_time = match NaiveTime::from_hms_opt(time.hour, time.minute, time.second) {
                Some(t) => t,
                None => {
                    return OperationResult::Veto(Some(format!(
                        "Invalid time: {}:{}:{}",
                        time.hour, time.minute, time.second
                    )))
                }
            };
            let naive_dt = NaiveDateTime::new(naive_date, naive_time);

            // Use the time's timezone, or UTC if not specified
            let offset = match create_semantic_timezone_offset(&time.timezone) {
                Ok(o) => o,
                Err(msg) => return OperationResult::Veto(Some(msg)),
            };
            let date_dt = match offset.from_local_datetime(&naive_dt).single() {
                Some(dt) => dt,
                None => {
                    return OperationResult::Veto(Some(
                        "Ambiguous or invalid datetime for timezone".to_string(),
                    ))
                }
            };

            let duration = time_dt - date_dt;
            let seconds = Decimal::from(duration.num_seconds());
            OperationResult::Value(Box::new(LiteralValue::duration(
                seconds,
                SemanticDurationUnit::Second,
            )))
        }

        _ => OperationResult::Veto(Some(format!(
            "Time arithmetic operation {:?} not supported for these operand types",
            op
        ))),
    }
}

fn semantic_time_to_chrono_datetime(time: &SemanticTime) -> Result<DateTime<FixedOffset>, String> {
    let naive_date =
        NaiveDate::from_ymd_opt(EPOCH_YEAR, EPOCH_MONTH, EPOCH_DAY).ok_or_else(|| {
            format!(
                "Invalid epoch date: {}-{}-{}",
                EPOCH_YEAR, EPOCH_MONTH, EPOCH_DAY
            )
        })?;
    let naive_time =
        NaiveTime::from_hms_opt(time.hour, time.minute, time.second).ok_or_else(|| {
            format!(
                "Invalid time: {}:{}:{}",
                time.hour, time.minute, time.second
            )
        })?;

    let naive_dt = NaiveDateTime::new(naive_date, naive_time);

    let offset = create_semantic_timezone_offset(&time.timezone)?;
    offset
        .from_local_datetime(&naive_dt)
        .single()
        .ok_or_else(|| "Ambiguous or invalid time for timezone".to_string())
}

fn chrono_datetime_to_semantic_time(dt: DateTime<FixedOffset>) -> SemanticTime {
    let offset_seconds = dt.offset().local_minus_utc();
    let offset_hours = (offset_seconds / SECONDS_PER_HOUR) as i8;
    let offset_minutes = ((offset_seconds.abs() % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE) as u8;

    SemanticTime {
        hour: dt.hour(),
        minute: dt.minute(),
        second: dt.second(),
        timezone: Some(SemanticTimezone {
            offset_hours,
            offset_minutes,
        }),
    }
}

// =============================================================================
// Date sugar evaluation helpers
// =============================================================================

use crate::parsing::ast::{CalendarUnit, DateCalendarKind, DateRelativeKind};
use crate::planning::semantics::primitive_boolean;

fn bool_result(b: bool) -> OperationResult {
    OperationResult::Value(Box::new(LiteralValue {
        value: ValueKind::Boolean(b),
        lemma_type: primitive_boolean().clone(),
    }))
}

/// `X in past [tolerance]` / `X in future [tolerance]`
pub fn compute_date_relative(
    kind: &DateRelativeKind,
    date: &SemanticDateTime,
    tolerance_duration: Option<(&Decimal, &SemanticDurationUnit)>,
    now: &SemanticDateTime,
) -> OperationResult {
    let date_chrono = match semantic_datetime_to_chrono(date) {
        Ok(dt) => dt,
        Err(msg) => return OperationResult::Veto(Some(msg)),
    };
    let now_chrono = match semantic_datetime_to_chrono(now) {
        Ok(dt) => dt,
        Err(msg) => return OperationResult::Veto(Some(msg)),
    };

    match kind {
        DateRelativeKind::InPast => match tolerance_duration {
            None => bool_result(date_chrono < now_chrono),
            Some((amount, unit)) => {
                let dur = match duration_to_chrono(amount, unit) {
                    Ok(d) => d,
                    Err(msg) => return OperationResult::Veto(Some(msg)),
                };
                let window_start = now_chrono - dur;
                bool_result(date_chrono >= window_start && date_chrono <= now_chrono)
            }
        },
        DateRelativeKind::InFuture => match tolerance_duration {
            None => bool_result(date_chrono > now_chrono),
            Some((amount, unit)) => {
                let dur = match duration_to_chrono(amount, unit) {
                    Ok(d) => d,
                    Err(msg) => return OperationResult::Veto(Some(msg)),
                };
                let window_end = now_chrono + dur;
                bool_result(date_chrono >= now_chrono && date_chrono <= window_end)
            }
        },
    }
}

fn duration_to_chrono(
    amount: &Decimal,
    unit: &SemanticDurationUnit,
) -> Result<ChronoDuration, String> {
    let val = amount
        .to_i64()
        .ok_or_else(|| format!("Duration value {} cannot be converted to integer", amount))?;
    match unit {
        SemanticDurationUnit::Year => Ok(ChronoDuration::days(val * 365)),
        SemanticDurationUnit::Month => Ok(ChronoDuration::days(val * 30)),
        SemanticDurationUnit::Week => Ok(ChronoDuration::weeks(val)),
        SemanticDurationUnit::Day => Ok(ChronoDuration::days(val)),
        SemanticDurationUnit::Hour => Ok(ChronoDuration::hours(val)),
        SemanticDurationUnit::Minute => Ok(ChronoDuration::minutes(val)),
        SemanticDurationUnit::Second => Ok(ChronoDuration::seconds(val)),
        SemanticDurationUnit::Millisecond => Ok(ChronoDuration::milliseconds(val)),
        SemanticDurationUnit::Microsecond => Ok(ChronoDuration::microseconds(val)),
    }
}

/// Calendar period boundaries
fn calendar_boundaries(
    now: &DateTime<FixedOffset>,
    unit: &CalendarUnit,
    offset: i32,
) -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>), String> {
    let tz = *now.offset();
    match unit {
        CalendarUnit::Year => {
            let target_year = now.year() + offset;
            let start = NaiveDate::from_ymd_opt(target_year, 1, 1)
                .ok_or_else(|| format!("Invalid year: {}", target_year))?
                .and_hms_opt(0, 0, 0)
                .ok_or("Invalid start time")?;
            let end = NaiveDate::from_ymd_opt(target_year, 12, 31)
                .ok_or_else(|| format!("Invalid year end: {}", target_year))?
                .and_hms_micro_opt(23, 59, 59, 999_999)
                .ok_or("Invalid end time")?;
            let start_dt = tz
                .from_local_datetime(&start)
                .single()
                .ok_or("Ambiguous start datetime")?;
            let end_dt = tz
                .from_local_datetime(&end)
                .single()
                .ok_or("Ambiguous end datetime")?;
            Ok((start_dt, end_dt))
        }
        CalendarUnit::Month => {
            let mut target_year = now.year();
            let mut target_month = now.month() as i32 + offset;
            while target_month < 1 {
                target_month += 12;
                target_year -= 1;
            }
            while target_month > 12 {
                target_month -= 12;
                target_year += 1;
            }
            let tm = target_month as u32;
            let start = NaiveDate::from_ymd_opt(target_year, tm, 1)
                .ok_or_else(|| format!("Invalid month start: {}-{}", target_year, tm))?
                .and_hms_opt(0, 0, 0)
                .ok_or("Invalid start time")?;
            let next_month_start = if tm == 12 {
                NaiveDate::from_ymd_opt(target_year + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(target_year, tm + 1, 1)
            }
            .ok_or_else(|| format!("Invalid next month: {}-{}", target_year, tm + 1))?;
            let last_day = next_month_start
                .pred_opt()
                .ok_or("Invalid last day of month")?;
            let end = last_day
                .and_hms_micro_opt(23, 59, 59, 999_999)
                .ok_or("Invalid end time")?;
            let start_dt = tz
                .from_local_datetime(&start)
                .single()
                .ok_or("Ambiguous start datetime")?;
            let end_dt = tz
                .from_local_datetime(&end)
                .single()
                .ok_or("Ambiguous end datetime")?;
            Ok((start_dt, end_dt))
        }
        CalendarUnit::Week => {
            let iso_week = now.iso_week();
            let target_week = iso_week.week() as i32 + offset;
            let target_year = iso_week.year();
            let monday = NaiveDate::from_isoywd_opt(
                target_year,
                target_week.max(1) as u32,
                chrono::Weekday::Mon,
            )
            .ok_or_else(|| {
                format!(
                    "Invalid ISO week: year={}, week={}",
                    target_year, target_week
                )
            })?;
            let sunday = monday + ChronoDuration::days(6);
            let start = monday.and_hms_opt(0, 0, 0).ok_or("Invalid start time")?;
            let end = sunday
                .and_hms_micro_opt(23, 59, 59, 999_999)
                .ok_or("Invalid end time")?;
            let start_dt = tz
                .from_local_datetime(&start)
                .single()
                .ok_or("Ambiguous start datetime")?;
            let end_dt = tz
                .from_local_datetime(&end)
                .single()
                .ok_or("Ambiguous end datetime")?;
            Ok((start_dt, end_dt))
        }
    }
}

/// `X in [past|future] calendar year|month|week` / `X not in calendar year|month|week`
pub fn compute_date_calendar(
    kind: &DateCalendarKind,
    unit: &CalendarUnit,
    date: &SemanticDateTime,
    now: &SemanticDateTime,
) -> OperationResult {
    let date_chrono = match semantic_datetime_to_chrono(date) {
        Ok(dt) => dt,
        Err(msg) => return OperationResult::Veto(Some(msg)),
    };
    let now_chrono = match semantic_datetime_to_chrono(now) {
        Ok(dt) => dt,
        Err(msg) => return OperationResult::Veto(Some(msg)),
    };

    let offset = match kind {
        DateCalendarKind::Current | DateCalendarKind::NotIn => 0,
        DateCalendarKind::Past => -1,
        DateCalendarKind::Future => 1,
    };

    let (start, end) = match calendar_boundaries(&now_chrono, unit, offset) {
        Ok(bounds) => bounds,
        Err(msg) => return OperationResult::Veto(Some(msg)),
    };

    let in_period = date_chrono >= start && date_chrono <= end;
    let result = match kind {
        DateCalendarKind::NotIn => !in_period,
        _ => in_period,
    };
    bool_result(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn utc_datetime(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> SemanticDateTime {
        SemanticDateTime {
            year: y,
            month: m,
            day: d,
            hour: h,
            minute: min,
            second: s,
            microsecond: 0,
            timezone: Some(SemanticTimezone {
                offset_hours: 0,
                offset_minutes: 0,
            }),
        }
    }

    fn tz_datetime(
        (y, m, d, h, min, s, us): (i32, u32, u32, u32, u32, u32, u32),
        (tz_h, tz_m): (i8, u8),
    ) -> SemanticDateTime {
        SemanticDateTime {
            year: y,
            month: m,
            day: d,
            hour: h,
            minute: min,
            second: s,
            microsecond: us,
            timezone: Some(SemanticTimezone {
                offset_hours: tz_h,
                offset_minutes: tz_m,
            }),
        }
    }

    fn assert_true(result: &OperationResult) {
        match result {
            OperationResult::Value(v) => match &v.value {
                ValueKind::Boolean(b) => assert!(*b, "expected true, got false"),
                other => panic!("expected Boolean, got {:?}", other),
            },
            OperationResult::Veto(msg) => panic!("expected Value(true), got Veto({:?})", msg),
        }
    }

    fn assert_false(result: &OperationResult) {
        match result {
            OperationResult::Value(v) => match &v.value {
                ValueKind::Boolean(b) => assert!(!*b, "expected false, got true"),
                other => panic!("expected Boolean, got {:?}", other),
            },
            OperationResult::Veto(msg) => panic!("expected Value(false), got Veto({:?})", msg),
        }
    }

    // ── compute_date_relative ──────────────────────────────────────────

    #[test]
    fn in_past_date_before_now() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 1, 0, 0, 0);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            None,
            &now,
        ));
    }

    #[test]
    fn in_past_date_equal_now_no_tolerance() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        assert_false(&compute_date_relative(
            &DateRelativeKind::InPast,
            &now,
            None,
            &now,
        ));
    }

    #[test]
    fn in_past_date_after_now() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 4, 1, 0, 0, 0);
        assert_false(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            None,
            &now,
        ));
    }

    #[test]
    fn in_past_with_tolerance_inside_window() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 5, 0, 0, 0);
        let amount = Decimal::from(7);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            Some((&amount, &SemanticDurationUnit::Day)),
            &now,
        ));
    }

    #[test]
    fn in_past_with_tolerance_at_boundary_equals_now() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let amount = Decimal::from(7);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InPast,
            &now,
            Some((&amount, &SemanticDurationUnit::Day)),
            &now,
        ));
    }

    #[test]
    fn in_past_with_tolerance_at_window_start() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 2, 28, 12, 0, 0);
        let amount = Decimal::from(7);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            Some((&amount, &SemanticDurationUnit::Day)),
            &now,
        ));
    }

    #[test]
    fn in_past_with_tolerance_outside_window() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 2, 1, 0, 0, 0);
        let amount = Decimal::from(7);
        assert_false(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            Some((&amount, &SemanticDurationUnit::Day)),
            &now,
        ));
    }

    #[test]
    fn in_past_with_zero_tolerance() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let amount = Decimal::from(0);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InPast,
            &now,
            Some((&amount, &SemanticDurationUnit::Day)),
            &now,
        ));
    }

    #[test]
    fn in_future_date_after_now() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 4, 1, 0, 0, 0);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InFuture,
            &date,
            None,
            &now,
        ));
    }

    #[test]
    fn in_future_date_equal_now_no_tolerance() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        assert_false(&compute_date_relative(
            &DateRelativeKind::InFuture,
            &now,
            None,
            &now,
        ));
    }

    #[test]
    fn in_future_date_before_now() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 1, 1, 0, 0, 0);
        assert_false(&compute_date_relative(
            &DateRelativeKind::InFuture,
            &date,
            None,
            &now,
        ));
    }

    #[test]
    fn in_future_with_tolerance_inside_window() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 10, 0, 0, 0);
        let amount = Decimal::from(7);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InFuture,
            &date,
            Some((&amount, &SemanticDurationUnit::Day)),
            &now,
        ));
    }

    #[test]
    fn in_future_with_tolerance_at_boundary_equals_now() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let amount = Decimal::from(7);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InFuture,
            &now,
            Some((&amount, &SemanticDurationUnit::Day)),
            &now,
        ));
    }

    #[test]
    fn in_future_with_tolerance_outside_window() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 6, 1, 0, 0, 0);
        let amount = Decimal::from(7);
        assert_false(&compute_date_relative(
            &DateRelativeKind::InFuture,
            &date,
            Some((&amount, &SemanticDurationUnit::Day)),
            &now,
        ));
    }

    // ── compute_date_calendar ──────────────────────────────────────────

    #[test]
    fn in_calendar_year_same_year() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 6, 15, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_year_different_year() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2025, 6, 15, 0, 0, 0);
        assert_false(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_year_boundary_jan_1() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 1, 1, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_year_boundary_dec_31() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 12, 31, 23, 59, 59);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_past_calendar_year() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2025, 6, 15, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Past,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_past_calendar_year_current_year_excluded() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 1, 15, 0, 0, 0);
        assert_false(&compute_date_calendar(
            &DateCalendarKind::Past,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_future_calendar_year() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2027, 6, 15, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Future,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_future_calendar_year_current_year_excluded() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 12, 31, 0, 0, 0);
        assert_false(&compute_date_calendar(
            &DateCalendarKind::Future,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn not_in_calendar_year_different_year() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2025, 6, 15, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::NotIn,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn not_in_calendar_year_same_year() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 6, 15, 0, 0, 0);
        assert_false(&compute_date_calendar(
            &DateCalendarKind::NotIn,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_month_same_month() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 20, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_month_different_month() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 4, 1, 0, 0, 0);
        assert_false(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_month_boundary_first_day() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 1, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_month_boundary_last_day_march() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 31, 23, 59, 59);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_month_feb_leap_year_boundary() {
        let now = utc_datetime(2024, 2, 15, 12, 0, 0);
        let date = utc_datetime(2024, 2, 29, 23, 59, 59);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_month_feb_non_leap_year_boundary() {
        let now = utc_datetime(2025, 2, 15, 12, 0, 0);
        let date = utc_datetime(2025, 2, 28, 23, 59, 59);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_past_calendar_month() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 2, 15, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Past,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_past_calendar_month_cross_year() {
        let now = utc_datetime(2026, 1, 15, 12, 0, 0);
        let date = utc_datetime(2025, 12, 20, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Past,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_future_calendar_month() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 4, 15, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Future,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_future_calendar_month_cross_year() {
        let now = utc_datetime(2026, 12, 15, 12, 0, 0);
        let date = utc_datetime(2027, 1, 10, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Future,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_week_same_week() {
        // 2026-03-07 is a Saturday (ISO week 10)
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        // Monday of same week: 2026-03-02
        let date = utc_datetime(2026, 3, 2, 10, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Week,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_week_different_week() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 15, 0, 0, 0);
        assert_false(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Week,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_week_sunday_boundary() {
        // 2026-03-07 is Saturday, same ISO week Mon 2026-03-02 through Sun 2026-03-08
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 8, 23, 59, 59);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Week,
            &date,
            &now,
        ));
    }

    #[test]
    fn not_in_calendar_month_different_month() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 5, 1, 0, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::NotIn,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    #[test]
    fn not_in_calendar_month_same_month() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 15, 0, 0, 0);
        assert_false(&compute_date_calendar(
            &DateCalendarKind::NotIn,
            &CalendarUnit::Month,
            &date,
            &now,
        ));
    }

    // ── timezone-aware tests ───────────────────────────────────────────

    #[test]
    fn in_past_respects_timezone_offset() {
        // now is 2026-03-07 01:00 +02:00 = 2026-03-06 23:00 UTC
        let now = tz_datetime((2026, 3, 7, 1, 0, 0, 0), (2, 0));
        // date is 2026-03-07 00:00 UTC = 2026-03-07 00:00 +00:00
        let date = utc_datetime(2026, 3, 7, 0, 0, 0);
        // date (UTC midnight Mar 7) is AFTER now (UTC 23:00 Mar 6)
        assert_false(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            None,
            &now,
        ));
    }

    #[test]
    fn in_calendar_year_timezone_boundary_respects_now_tz() {
        // now is +05:00; calendar year boundary ends at 2026-12-31T23:59:59.999999 +05:00
        // = 2026-12-31T18:59:59.999999 UTC
        // date is 2026-12-31T23:59:59 UTC which is 2027-01-01T04:59:59 +05:00
        // so date is OUTSIDE the calendar year in now's timezone
        let now = tz_datetime((2026, 6, 15, 12, 0, 0, 0), (5, 0));
        let date = utc_datetime(2026, 12, 31, 23, 59, 59);
        assert_false(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_calendar_year_timezone_boundary_inside() {
        // now is +05:00; date is 2026-12-31T18:00 UTC = 2026-12-31T23:00 +05:00 → inside year
        let now = tz_datetime((2026, 6, 15, 12, 0, 0, 0), (5, 0));
        let date = utc_datetime(2026, 12, 31, 18, 0, 0);
        assert_true(&compute_date_calendar(
            &DateCalendarKind::Current,
            &CalendarUnit::Year,
            &date,
            &now,
        ));
    }

    #[test]
    fn in_past_with_tolerance_hours() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 7, 10, 0, 0);
        let amount = Decimal::from(4);
        assert_true(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            Some((&amount, &SemanticDurationUnit::Hour)),
            &now,
        ));
    }

    #[test]
    fn in_past_with_tolerance_hours_outside() {
        let now = utc_datetime(2026, 3, 7, 12, 0, 0);
        let date = utc_datetime(2026, 3, 7, 6, 0, 0);
        let amount = Decimal::from(4);
        assert_false(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            Some((&amount, &SemanticDurationUnit::Hour)),
            &now,
        ));
    }

    // ── microsecond precision ──────────────────────────────────────────

    #[test]
    fn in_past_microsecond_precision_boundary() {
        let now = tz_datetime((2026, 3, 7, 12, 0, 0, 500_000), (0, 0));
        let date = tz_datetime((2026, 3, 7, 12, 0, 0, 499_999), (0, 0));
        // date is 1 microsecond before now
        assert_true(&compute_date_relative(
            &DateRelativeKind::InPast,
            &date,
            None,
            &now,
        ));
    }

    // ── calendar_boundaries direct tests ───────────────────────────────

    #[test]
    fn calendar_boundaries_year_covers_full_year() {
        let now = semantic_datetime_to_chrono(&utc_datetime(2026, 6, 15, 12, 0, 0)).unwrap();
        let (start, end) = calendar_boundaries(&now, &CalendarUnit::Year, 0).unwrap();
        assert_eq!(start.month(), 1);
        assert_eq!(start.day(), 1);
        assert_eq!(start.hour(), 0);
        assert_eq!(end.month(), 12);
        assert_eq!(end.day(), 31);
        assert_eq!(end.hour(), 23);
        assert_eq!(end.minute(), 59);
    }

    #[test]
    fn calendar_boundaries_month_feb_leap() {
        let now = semantic_datetime_to_chrono(&utc_datetime(2024, 2, 15, 0, 0, 0)).unwrap();
        let (start, end) = calendar_boundaries(&now, &CalendarUnit::Month, 0).unwrap();
        assert_eq!(start.day(), 1);
        assert_eq!(end.day(), 29);
    }

    #[test]
    fn calendar_boundaries_month_feb_non_leap() {
        let now = semantic_datetime_to_chrono(&utc_datetime(2025, 2, 15, 0, 0, 0)).unwrap();
        let (start, end) = calendar_boundaries(&now, &CalendarUnit::Month, 0).unwrap();
        assert_eq!(start.day(), 1);
        assert_eq!(end.day(), 28);
    }

    #[test]
    fn calendar_boundaries_week_monday_to_sunday() {
        // 2026-03-07 is a Saturday
        let now = semantic_datetime_to_chrono(&utc_datetime(2026, 3, 7, 12, 0, 0)).unwrap();
        let (start, end) = calendar_boundaries(&now, &CalendarUnit::Week, 0).unwrap();
        assert_eq!(start.weekday(), chrono::Weekday::Mon);
        assert_eq!(end.weekday(), chrono::Weekday::Sun);
    }

    #[test]
    fn calendar_boundaries_past_month_december_from_january() {
        let now = semantic_datetime_to_chrono(&utc_datetime(2026, 1, 15, 12, 0, 0)).unwrap();
        let (start, end) = calendar_boundaries(&now, &CalendarUnit::Month, -1).unwrap();
        assert_eq!(start.year(), 2025);
        assert_eq!(start.month(), 12);
        assert_eq!(start.day(), 1);
        assert_eq!(end.year(), 2025);
        assert_eq!(end.month(), 12);
        assert_eq!(end.day(), 31);
    }

    #[test]
    fn calendar_boundaries_future_month_january_from_december() {
        let now = semantic_datetime_to_chrono(&utc_datetime(2026, 12, 15, 12, 0, 0)).unwrap();
        let (start, end) = calendar_boundaries(&now, &CalendarUnit::Month, 1).unwrap();
        assert_eq!(start.year(), 2027);
        assert_eq!(start.month(), 1);
        assert_eq!(start.day(), 1);
        assert_eq!(end.year(), 2027);
        assert_eq!(end.month(), 1);
        assert_eq!(end.day(), 31);
    }
}
