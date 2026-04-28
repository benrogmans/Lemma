//! Literal value types and string parsing. No dependency on parsing/ast.
//! AST and planning re-export these types where needed.

use chrono::{Datelike, Timelike};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

// -----------------------------------------------------------------------------
// Unit tables for Scale and Ratio types
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScaleUnit {
    pub name: String,
    pub value: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScaleUnits(pub Vec<ScaleUnit>);

impl ScaleUnits {
    pub fn new() -> Self {
        ScaleUnits(Vec::new())
    }
    pub fn get(&self, name: &str) -> Result<&ScaleUnit, String> {
        self.0.iter().find(|u| u.name == name).ok_or_else(|| {
            let valid: Vec<&str> = self.0.iter().map(|u| u.name.as_str()).collect();
            format!(
                "Unknown unit '{}' for this scale type. Valid units: {}",
                name,
                valid.join(", ")
            )
        })
    }
    pub fn iter(&self) -> std::slice::Iter<'_, ScaleUnit> {
        self.0.iter()
    }
    pub fn push(&mut self, u: ScaleUnit) {
        self.0.push(u);
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Default for ScaleUnits {
    fn default() -> Self {
        ScaleUnits::new()
    }
}

impl From<Vec<ScaleUnit>> for ScaleUnits {
    fn from(v: Vec<ScaleUnit>) -> Self {
        ScaleUnits(v)
    }
}

impl<'a> IntoIterator for &'a ScaleUnits {
    type Item = &'a ScaleUnit;
    type IntoIter = std::slice::Iter<'a, ScaleUnit>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RatioUnit {
    pub name: String,
    pub value: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RatioUnits(pub Vec<RatioUnit>);

impl RatioUnits {
    pub fn new() -> Self {
        RatioUnits(Vec::new())
    }
    pub fn get(&self, name: &str) -> Result<&RatioUnit, String> {
        self.0.iter().find(|u| u.name == name).ok_or_else(|| {
            let valid: Vec<&str> = self.0.iter().map(|u| u.name.as_str()).collect();
            format!(
                "Unknown unit '{}' for this ratio type. Valid units: {}",
                name,
                valid.join(", ")
            )
        })
    }
    pub fn iter(&self) -> std::slice::Iter<'_, RatioUnit> {
        self.0.iter()
    }
    pub fn push(&mut self, u: RatioUnit) {
        self.0.push(u);
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Default for RatioUnits {
    fn default() -> Self {
        RatioUnits::new()
    }
}

impl From<Vec<RatioUnit>> for RatioUnits {
    fn from(v: Vec<RatioUnit>) -> Self {
        RatioUnits(v)
    }
}

impl<'a> IntoIterator for &'a RatioUnits {
    type Item = &'a RatioUnit;
    type IntoIter = std::slice::Iter<'a, RatioUnit>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

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

/// Strict scale literal: `<number> <unit-name>` separated by any whitespace run.
///
/// Does NOT accept ratio sigils (`%`, `%%`) — those are a `Ratio` concern. See
/// [`RatioLiteral`] for runtime ratio input parsing. Trailing tokens after the
/// unit are rejected (no silent truncation).
pub(crate) struct NumberWithUnit(pub Decimal, pub String);

impl std::str::FromStr for NumberWithUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(
                "Scale value cannot be empty. Use a number followed by a unit (e.g. '10 eur')."
                    .to_string(),
            );
        }

        let mut parts = trimmed.split_whitespace();
        let number_part = parts
            .next()
            .expect("split_whitespace yields >=1 token after non-empty guard");
        let unit_part = parts.next().ok_or_else(|| {
            format!(
                "Scale value must include a unit (e.g. '{} eur').",
                number_part
            )
        })?;
        if parts.next().is_some() {
            return Err(format!(
                "Invalid scale value: '{}'. Expected exactly '<number> <unit>', got extra tokens.",
                s
            ));
        }
        let n = number_part
            .parse::<NumberLiteral>()
            .map_err(|_| format!("Invalid scale: '{}'", s))?
            .0;
        Ok(NumberWithUnit(n, unit_part.to_string()))
    }
}

/// Strict ratio runtime literal.
///
/// Grammar (all inputs trimmed first):
/// - `<number>`                      → `Bare(n)`
/// - `<number>%`  (glued, no inner whitespace) → `Percent(n / 100)`
/// - `<number>%%` (glued, no inner whitespace) → `Permille(n / 1000)`
/// - `<number> <unit-name>`          → `Named { value: n, unit: <unit-name> }`
///
/// `<number>` is parsed by [`NumberLiteral`] (signed, allows `_`/`,` separators).
/// Whitespace between the number and a keyword unit may be any non-empty run
/// (`"50 percent"`, `"50    percent"`, `"50\tpercent"` are all accepted).
///
/// The sigils `%` / `%%` are language-level constants meaning "divide by 100 / 1000"
/// and unconditionally produce the canonical unit names `"percent"` / `"permille"`.
/// They are NOT accepted as standalone unit-position tokens (i.e. `"5 %"` is rejected).
///
/// Signedness is intentionally not constrained at this layer: bounds are the
/// type-system's job (`-> minimum 0%`), and the evaluator can produce signed
/// ratios from non-negative inputs (e.g. `this_year - last_year` on `percent`).
/// The parser must accept everything the evaluator can emit (round-trip symmetry).
///
/// `Named` carries the raw unit name; the caller in `parse_number_unit::Ratio`
/// resolves it against the type's [`RatioUnits`] table (covering built-in
/// `percent`/`permille` and any user-defined units like `basis_points`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RatioLiteral {
    Bare(Decimal),
    Percent(Decimal),
    Permille(Decimal),
    Named { value: Decimal, unit: String },
}

impl std::str::FromStr for RatioLiteral {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(
                "Ratio value cannot be empty. Use a number, optionally followed by '%', '%%', or a unit name (e.g. '0.5', '50%', '25%%', '50 percent')."
                    .to_string(),
            );
        }

        let mut parts = trimmed.split_whitespace();
        let first = parts
            .next()
            .expect("split_whitespace yields >=1 token after non-empty guard");
        let second = parts.next();
        if parts.next().is_some() {
            return Err(format!(
                "Invalid ratio value: '{}'. Expected '<number>', '<number>%', '<number>%%', or '<number> <unit>'.",
                s
            ));
        }

        match second {
            // 1-token forms: bare number, or sigil-suffixed number.
            None => {
                if let Some(rest) = first.strip_suffix("%%") {
                    if rest.is_empty() {
                        return Err(format!(
                            "Invalid ratio value: '{}'. '%%' must follow a number (e.g. '25%%').",
                            s
                        ));
                    }
                    let n = rest
                        .parse::<NumberLiteral>()
                        .map_err(|_| {
                            format!(
                            "Invalid ratio value: '{}'. '{}' is not a valid number before '%%'.",
                            s, rest
                        )
                        })?
                        .0;
                    return Ok(RatioLiteral::Permille(n / Decimal::from(1000)));
                }
                if let Some(rest) = first.strip_suffix('%') {
                    if rest.is_empty() {
                        return Err(format!(
                            "Invalid ratio value: '{}'. '%' must follow a number (e.g. '50%').",
                            s
                        ));
                    }
                    let n = rest
                        .parse::<NumberLiteral>()
                        .map_err(|_| {
                            format!(
                                "Invalid ratio value: '{}'. '{}' is not a valid number before '%'.",
                                s, rest
                            )
                        })?
                        .0;
                    return Ok(RatioLiteral::Percent(n / Decimal::from(100)));
                }
                let n = first.parse::<NumberLiteral>().map_err(|_| {
                    format!(
                        "Invalid ratio value: '{}'. Must be a number, '<n>%', '<n>%%', '<n> percent', '<n> permille', or '<n> <unit>'.",
                        s
                    )
                })?.0;
                Ok(RatioLiteral::Bare(n))
            }
            // 2-token form: <number> <unit-name>. Sigils are not accepted as unit-position tokens.
            Some(unit) => {
                if unit == "%" || unit == "%%" {
                    return Err(format!(
                        "Invalid ratio value: '{}'. '{}' must be glued to the number (e.g. '{}{}'), not separated by whitespace.",
                        s, unit, first, unit
                    ));
                }
                let n = first
                    .parse::<NumberLiteral>()
                    .map_err(|_| {
                        format!(
                            "Invalid ratio value: '{}'. '{}' is not a valid number.",
                            s, first
                        )
                    })?
                    .0;
                Ok(RatioLiteral::Named {
                    value: n,
                    unit: unit.to_string(),
                })
            }
        }
    }
}
