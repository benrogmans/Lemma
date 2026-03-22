//! Literal value types and string parsing. No dependency on parsing/ast.
//! AST and planning re-export these types where needed.

use chrono::{Datelike, Timelike};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

// -----------------------------------------------------------------------------
// Literal value types
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BooleanValue {
    True,
    False,
    Yes,
    No,
    Accept,
    Reject,
}

impl From<BooleanValue> for bool {
    fn from(value: BooleanValue) -> bool {
        matches!(
            value,
            BooleanValue::True | BooleanValue::Yes | BooleanValue::Accept
        )
    }
}

impl From<&BooleanValue> for bool {
    fn from(value: &BooleanValue) -> bool {
        (*value).into() // Copy makes this ok
    }
}

impl From<bool> for BooleanValue {
    fn from(value: bool) -> BooleanValue {
        if value {
            BooleanValue::True
        } else {
            BooleanValue::False
        }
    }
}

impl std::ops::Not for BooleanValue {
    type Output = BooleanValue;

    fn not(self) -> Self::Output {
        if self.into() {
            BooleanValue::False
        } else {
            BooleanValue::True
        }
    }
}

impl std::ops::Not for &BooleanValue {
    type Output = BooleanValue;

    fn not(self) -> Self::Output {
        if (*self).into() {
            BooleanValue::False
        } else {
            BooleanValue::True
        }
    }
}

impl std::str::FromStr for BooleanValue {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "true" => Ok(BooleanValue::True),
            "false" => Ok(BooleanValue::False),
            "yes" => Ok(BooleanValue::Yes),
            "no" => Ok(BooleanValue::No),
            "accept" => Ok(BooleanValue::Accept),
            "reject" => Ok(BooleanValue::Reject),
            _ => Err(format!("Invalid boolean: '{}'", s)),
        }
    }
}

impl BooleanValue {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            BooleanValue::True => "true",
            BooleanValue::False => "false",
            BooleanValue::Yes => "yes",
            BooleanValue::No => "no",
            BooleanValue::Accept => "accept",
            BooleanValue::Reject => "reject",
        }
    }
}

impl fmt::Display for BooleanValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
}

impl Serialize for DurationUnit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DurationUnit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for DurationUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            DurationUnit::Year => "years",
            DurationUnit::Month => "months",
            DurationUnit::Week => "weeks",
            DurationUnit::Day => "days",
            DurationUnit::Hour => "hours",
            DurationUnit::Minute => "minutes",
            DurationUnit::Second => "seconds",
            DurationUnit::Millisecond => "milliseconds",
            DurationUnit::Microsecond => "microseconds",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for DurationUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "year" | "years" => Ok(DurationUnit::Year),
            "month" | "months" => Ok(DurationUnit::Month),
            "week" | "weeks" => Ok(DurationUnit::Week),
            "day" | "days" => Ok(DurationUnit::Day),
            "hour" | "hours" => Ok(DurationUnit::Hour),
            "minute" | "minutes" => Ok(DurationUnit::Minute),
            "second" | "seconds" => Ok(DurationUnit::Second),
            "millisecond" | "milliseconds" => Ok(DurationUnit::Millisecond),
            "microsecond" | "microseconds" => Ok(DurationUnit::Microsecond),
            _ => Err(format!("Unknown duration unit: '{}'", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimezoneValue {
    pub offset_hours: i8,
    pub offset_minutes: u8,
}

impl fmt::Display for TimezoneValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.offset_hours == 0 && self.offset_minutes == 0 {
            write!(f, "Z")
        } else {
            let sign = if self.offset_hours >= 0 { "+" } else { "-" };
            let hours = self.offset_hours.abs();
            write!(f, "{}{:02}:{:02}", sign, hours, self.offset_minutes)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct TimeValue {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub timezone: Option<TimezoneValue>,
}

impl fmt::Display for TimeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DateTimeValue {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    #[serde(default)]
    pub microsecond: u32,
    pub timezone: Option<TimezoneValue>,
}

impl DateTimeValue {
    pub fn now() -> Self {
        let now = chrono::Local::now();
        let offset_secs = now.offset().local_minus_utc();
        Self {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            hour: now.time().hour(),
            minute: now.time().minute(),
            second: now.time().second(),
            microsecond: now.time().nanosecond() / 1000 % 1_000_000,
            timezone: Some(TimezoneValue {
                offset_hours: (offset_secs / 3600) as i8,
                offset_minutes: ((offset_secs.abs() % 3600) / 60) as u8,
            }),
        }
    }

    fn parse_iso_week(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split("-W").collect();
        if parts.len() != 2 {
            return None;
        }
        let year: i32 = parts[0].parse().ok()?;
        let week: u32 = parts[1].parse().ok()?;
        if week == 0 || week > 53 {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)?;
        Some(Self {
            year: date.year(),
            month: date.month(),
            day: date.day(),
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        })
    }
}

impl fmt::Display for DateTimeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let has_time = self.hour != 0
            || self.minute != 0
            || self.second != 0
            || self.microsecond != 0
            || self.timezone.is_some();
        if !has_time {
            write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
        } else {
            write!(
                f,
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                self.year, self.month, self.day, self.hour, self.minute, self.second
            )?;
            if self.microsecond != 0 {
                write!(f, ".{:06}", self.microsecond)?;
            }
            if let Some(tz) = &self.timezone {
                write!(f, "{}", tz)?;
            }
            Ok(())
        }
    }
}

/// Literal value data (no type information). Single source of truth in literals.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Number(Decimal),
    Scale(Decimal, String),
    Text(String),
    Date(DateTimeValue),
    Time(TimeValue),
    Boolean(BooleanValue),
    Duration(Decimal, DurationUnit),
    Ratio(Decimal, Option<String>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{}", n),
            Value::Text(s) => write!(f, "{}", s),
            Value::Date(dt) => write!(f, "{}", dt),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Time(time) => write!(f, "{}", time),
            Value::Scale(n, u) => write!(f, "{} {}", n, u),
            Value::Duration(n, u) => write!(f, "{} {}", n, u),
            Value::Ratio(n, u) => match u.as_deref() {
                Some("percent") => {
                    let display_value = *n * Decimal::from(100);
                    let norm = display_value.normalize();
                    let s = if norm.fract().is_zero() {
                        norm.trunc().to_string()
                    } else {
                        norm.to_string()
                    };
                    write!(f, "{}%", s)
                }
                Some("permille") => {
                    let display_value = *n * Decimal::from(1000);
                    let norm = display_value.normalize();
                    let s = if norm.fract().is_zero() {
                        norm.trunc().to_string()
                    } else {
                        norm.to_string()
                    };
                    write!(f, "{}%%", s)
                }
                Some(unit) => {
                    let norm = n.normalize();
                    let s = if norm.fract().is_zero() {
                        norm.trunc().to_string()
                    } else {
                        norm.to_string()
                    };
                    write!(f, "{} {}", s, unit)
                }
                None => {
                    let norm = n.normalize();
                    let s = if norm.fract().is_zero() {
                        norm.trunc().to_string()
                    } else {
                        norm.to_string()
                    };
                    write!(f, "{}", s)
                }
            },
        }
    }
}

// -----------------------------------------------------------------------------
// FromStr (single source of truth per type)
// -----------------------------------------------------------------------------

impl std::str::FromStr for DateTimeValue {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(dt) = s.parse::<chrono::DateTime<chrono::FixedOffset>>() {
            let offset = dt.offset().local_minus_utc();
            let microsecond = dt.nanosecond() / 1000 % 1_000_000;
            return Ok(DateTimeValue {
                year: dt.year(),
                month: dt.month(),
                day: dt.day(),
                hour: dt.hour(),
                minute: dt.minute(),
                second: dt.second(),
                microsecond,
                timezone: Some(TimezoneValue {
                    offset_hours: (offset / 3600) as i8,
                    offset_minutes: ((offset.abs() % 3600) / 60) as u8,
                }),
            });
        }
        if let Ok(dt) = s.parse::<chrono::NaiveDateTime>() {
            let microsecond = dt.nanosecond() / 1000 % 1_000_000;
            return Ok(DateTimeValue {
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
            return Ok(DateTimeValue {
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
        if let Some(week_val) = Self::parse_iso_week(s) {
            return Ok(week_val);
        }
        if let Ok(ym) = chrono::NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d") {
            return Ok(Self {
                year: ym.year(),
                month: ym.month(),
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                microsecond: 0,
                timezone: None,
            });
        }
        if let Ok(year) = s.parse::<i32>() {
            if (1..=9999).contains(&year) {
                return Ok(Self {
                    year,
                    month: 1,
                    day: 1,
                    hour: 0,
                    minute: 0,
                    second: 0,
                    microsecond: 0,
                    timezone: None,
                });
            }
        }
        Err(format!("Invalid date format: '{}'", s))
    }
}

impl std::str::FromStr for TimeValue {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(t) = s.parse::<chrono::DateTime<chrono::FixedOffset>>() {
            let offset = t.offset().local_minus_utc();
            return Ok(TimeValue {
                hour: t.hour() as u8,
                minute: t.minute() as u8,
                second: t.second() as u8,
                timezone: Some(TimezoneValue {
                    offset_hours: (offset / 3600) as i8,
                    offset_minutes: ((offset.abs() % 3600) / 60) as u8,
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
}

/// Number literal with Lemma rules (strip _ and ,; MAX_NUMBER_DIGITS).
pub(crate) struct NumberLiteral(pub Decimal);

impl std::str::FromStr for NumberLiteral {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let clean = s.trim().replace(['_', ','], "");
        let digit_count = clean.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count > crate::limits::MAX_NUMBER_DIGITS {
            return Err(format!(
                "Number has too many digits (max {})",
                crate::limits::MAX_NUMBER_DIGITS
            ));
        }
        Decimal::from_str(&clean)
            .map_err(|_| format!("Invalid number: '{}'", s))
            .map(NumberLiteral)
    }
}

/// Text literal with length limit.
pub(crate) struct TextLiteral(pub String);

impl std::str::FromStr for TextLiteral {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() > crate::limits::MAX_TEXT_VALUE_LENGTH {
            return Err(format!(
                "Text value exceeds maximum length (max {} characters)",
                crate::limits::MAX_TEXT_VALUE_LENGTH
            ));
        }
        Ok(TextLiteral(s.to_string()))
    }
}

/// Duration magnitude: number + unit (e.g. "10 hours").
pub(crate) struct DurationLiteral(pub Decimal, pub DurationUnit);

impl std::str::FromStr for DurationLiteral {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let mut parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(format!(
                "Invalid duration: '{}'. Expected format: <number> <unit> (e.g. 10 hours, 2 weeks)",
                s
            ));
        }
        let unit_str = parts.pop().unwrap();
        let number_str = parts.join(" ");
        let n = number_str
            .parse::<NumberLiteral>()
            .map_err(|_| format!("Invalid duration number: '{}'", number_str))?
            .0;
        let unit = unit_str.parse()?;
        Ok(DurationLiteral(n, unit))
    }
}

/// Number with unit name (e.g. "1 eur", "50 percent"). Unit not validated against a type.
pub(crate) struct NumberWithUnit(pub Decimal, pub String);

impl std::str::FromStr for NumberWithUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
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
        let n = number_part
            .parse::<NumberLiteral>()
            .map_err(|_| format!("Invalid scale: '{}'", s))?
            .0;
        Ok(NumberWithUnit(n, unit_part.to_string()))
    }
}
