use lemma::{DateGranularity, DateTimeValue, TimezoneValue};

pub fn make_effective(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTimeValue {
    DateTimeValue {
        year: y,
        month: m,
        day: d,
        hour: h,
        minute: min,
        second: s,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: 0,
            offset_minutes: 0,
        }),
        granularity: DateGranularity::DateTime,
    }
}

pub fn make_effective_tz(
    (y, m, d, h, min, s): (i32, u32, u32, u32, u32, u32),
    (tz_h, tz_m): (i8, u8),
) -> DateTimeValue {
    DateTimeValue {
        year: y,
        month: m,
        day: d,
        hour: h,
        minute: min,
        second: s,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: tz_h,
            offset_minutes: tz_m,
        }),
        granularity: DateGranularity::DateTime,
    }
}
