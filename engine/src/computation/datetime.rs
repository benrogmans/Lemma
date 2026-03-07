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
        NaiveTime::from_hms_opt(date.hour, date.minute, date.second).ok_or_else(|| {
            format!(
                "Invalid time: {}:{}:{}",
                date.hour, date.minute, date.second
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
