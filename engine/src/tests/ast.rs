use crate::parsing::ast::*;
use crate::planning::semantics::{
    date_time_to_semantic, duration_unit_to_semantic, primitive_time, time_to_semantic, LemmaType,
    LiteralValue, TypeExtends, TypeSpecification,
};
use rust_decimal::Decimal;
use std::str::FromStr;

#[test]
fn test_literal_value_to_primitive_type() {
    let one = Decimal::from_str("1").unwrap();

    assert_eq!(LiteralValue::text("".to_string()).lemma_type.name(), "text");
    assert_eq!(LiteralValue::number(one).lemma_type.name(), "number");
    assert_eq!(
        LiteralValue::from_bool(bool::from(BooleanValue::True))
            .lemma_type
            .name(),
        "boolean"
    );

    let dt = DateTimeValue {
        year: 2024,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
    };
    assert_eq!(
        LiteralValue::date(date_time_to_semantic(&dt))
            .lemma_type
            .name(),
        "date"
    );
    assert_eq!(
        LiteralValue::ratio(one / Decimal::from(100), None)
            .lemma_type
            .name(),
        "ratio"
    );
    assert_eq!(
        LiteralValue::duration(one, duration_unit_to_semantic(&DurationUnit::Second))
            .lemma_type
            .name(),
        "duration"
    );
}

#[test]
fn test_arithmetic_operation_display() {
    assert_eq!(format!("{}", ArithmeticComputation::Add), "+");
    assert_eq!(format!("{}", ArithmeticComputation::Subtract), "-");
    assert_eq!(format!("{}", ArithmeticComputation::Multiply), "*");
    assert_eq!(format!("{}", ArithmeticComputation::Divide), "/");
    assert_eq!(format!("{}", ArithmeticComputation::Modulo), "%");
    assert_eq!(format!("{}", ArithmeticComputation::Power), "^");
}

#[test]
fn test_comparison_operator_display() {
    assert_eq!(format!("{}", ComparisonComputation::GreaterThan), ">");
    assert_eq!(format!("{}", ComparisonComputation::LessThan), "<");
    assert_eq!(
        format!("{}", ComparisonComputation::GreaterThanOrEqual),
        ">="
    );
    assert_eq!(format!("{}", ComparisonComputation::LessThanOrEqual), "<=");
    assert_eq!(format!("{}", ComparisonComputation::Is), "is");
    assert_eq!(format!("{}", ComparisonComputation::IsNot), "is not");
}

#[test]
fn test_duration_unit_display() {
    assert_eq!(format!("{}", DurationUnit::Second), "seconds");
    assert_eq!(format!("{}", DurationUnit::Minute), "minutes");
    assert_eq!(format!("{}", DurationUnit::Hour), "hours");
    assert_eq!(format!("{}", DurationUnit::Day), "days");
    assert_eq!(format!("{}", DurationUnit::Week), "weeks");
    assert_eq!(format!("{}", DurationUnit::Millisecond), "milliseconds");
    assert_eq!(format!("{}", DurationUnit::Microsecond), "microseconds");
    assert_eq!(format!("{}", DurationUnit::Year), "years");
    assert_eq!(format!("{}", DurationUnit::Month), "months");
}

#[test]
fn test_conversion_target_display() {
    assert_eq!(
        format!("{}", ConversionTarget::Duration(DurationUnit::Hour)),
        "hours"
    );
    assert_eq!(
        format!("{}", ConversionTarget::Unit("eur".to_string())),
        "eur"
    );
}

#[test]
fn test_spec_type_display() {
    assert_eq!(
        format!("{}", crate::planning::semantics::primitive_text()),
        "text"
    );
    assert_eq!(
        format!("{}", crate::planning::semantics::primitive_number()),
        "number"
    );
    assert_eq!(
        format!("{}", crate::planning::semantics::primitive_date()),
        "date"
    );
    assert_eq!(
        format!("{}", crate::planning::semantics::primitive_boolean()),
        "boolean"
    );
    assert_eq!(
        format!("{}", crate::planning::semantics::primitive_ratio()),
        "ratio"
    );
    assert_eq!(
        format!("{}", crate::planning::semantics::primitive_duration()),
        "duration"
    );
    assert_eq!(format!("{}", primitive_time()), "time");
}

#[test]
fn test_type_constructor() {
    let specs = TypeSpecification::number();
    let lemma_type = LemmaType::new("dice".to_string(), specs, TypeExtends::Primitive);
    assert_eq!(lemma_type.name(), "dice");
}

#[test]
fn test_type_display() {
    let specs = TypeSpecification::text();
    let lemma_type = LemmaType::new("name".to_string(), specs, TypeExtends::Primitive);
    assert_eq!(format!("{}", lemma_type), "name");
}

#[test]
fn test_type_equality() {
    let specs1 = TypeSpecification::number();
    let specs2 = TypeSpecification::number();
    let lemma_type1 = LemmaType::new("dice".to_string(), specs1, TypeExtends::Primitive);
    let lemma_type2 = LemmaType::new("dice".to_string(), specs2, TypeExtends::Primitive);
    assert_eq!(lemma_type1, lemma_type2);

    let specs3 = TypeSpecification::number();
    let lemma_type3 = LemmaType::new("other_dice".to_string(), specs3, TypeExtends::Primitive);
    assert_ne!(
        lemma_type1, lemma_type3,
        "Types with different names must not be equal"
    );
}

#[test]
fn test_type_serialization() {
    let specs = TypeSpecification::number();
    let lemma_type = LemmaType::new("dice".to_string(), specs, TypeExtends::Primitive);
    let serialized = serde_json::to_string(&lemma_type).unwrap();
    let deserialized: LemmaType = serde_json::from_str(&serialized).unwrap();
    assert_eq!(lemma_type, deserialized);
}

#[test]
fn test_literal_value_display_value() {
    let ten = Decimal::from_str("10").unwrap();

    assert_eq!(
        LiteralValue::text("hello".to_string()).display_value(),
        "hello"
    );
    assert_eq!(LiteralValue::number(ten).display_value(), "10");
    assert_eq!(LiteralValue::from_bool(true).display_value(), "true");
    assert_eq!(LiteralValue::from_bool(false).display_value(), "false");

    let ten_percent_ratio = LiteralValue::ratio(Decimal::from_str("0.10").unwrap(), None);
    assert_eq!(ten_percent_ratio.display_value(), "0.1");

    let date = DateTimeValue {
        year: 2024,
        month: 6,
        day: 15,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
    };
    assert_eq!(
        LiteralValue::date(date_time_to_semantic(&date)).display_value(),
        "2024-06-15"
    );

    let datetime = DateTimeValue {
        year: 2024,
        month: 12,
        day: 25,
        hour: 14,
        minute: 30,
        second: 45,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: 1,
            offset_minutes: 0,
        }),
    };
    assert_eq!(
        LiteralValue::date(date_time_to_semantic(&datetime)).display_value(),
        "2024-12-25T14:30:45+01:00"
    );

    let time = TimeValue {
        hour: 14,
        minute: 30,
        second: 0,
        timezone: None,
    };
    assert_eq!(
        LiteralValue::time(time_to_semantic(&time)).display_value(),
        "14:30:00"
    );

    assert_eq!(
        LiteralValue::duration(ten, duration_unit_to_semantic(&DurationUnit::Hour)).display_value(),
        "10 hours"
    );
}

#[test]
fn test_literal_value_time_type() {
    let time = TimeValue {
        hour: 14,
        minute: 30,
        second: 0,
        timezone: None,
    };
    assert_eq!(
        LiteralValue::time(time_to_semantic(&time))
            .lemma_type
            .name(),
        "time"
    );
}

#[test]
fn test_datetime_value_display() {
    let dt = DateTimeValue {
        year: 2024,
        month: 12,
        day: 25,
        hour: 14,
        minute: 30,
        second: 45,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: 1,
            offset_minutes: 0,
        }),
    };
    let display = format!("{}", dt);
    assert_eq!(display, "2024-12-25T14:30:45+01:00");
}

#[test]
fn test_time_value_display() {
    let time = TimeValue {
        hour: 14,
        minute: 30,
        second: 45,
        timezone: Some(TimezoneValue {
            offset_hours: -5,
            offset_minutes: 30,
        }),
    };
    let display = format!("{}", time);
    assert_eq!(display, "14:30:45");
}

#[test]
fn test_timezone_value() {
    let tz_positive = TimezoneValue {
        offset_hours: 5,
        offset_minutes: 30,
    };
    assert_eq!(format!("{}", tz_positive), "+05:30");

    let tz_negative = TimezoneValue {
        offset_hours: -8,
        offset_minutes: 0,
    };
    assert_eq!(format!("{}", tz_negative), "-08:00");

    let tz_utc = TimezoneValue {
        offset_hours: 0,
        offset_minutes: 0,
    };
    assert_eq!(format!("{}", tz_utc), "Z");
}

#[test]
fn test_negation_types() {
    let json = serde_json::to_string(&NegationType::Not).expect("serialize NegationType");
    let decoded: NegationType = serde_json::from_str(&json).expect("deserialize NegationType");
    assert_eq!(decoded, NegationType::Not);
}

#[test]
fn test_veto_expression() {
    let veto_with_message = VetoExpression {
        message: Some("Must be over 18".to_string()),
    };
    assert_eq!(
        veto_with_message.message,
        Some("Must be over 18".to_string())
    );

    let veto_without_message = VetoExpression { message: None };
    assert!(veto_without_message.message.is_none());
}

#[test]
fn test_datetime_value_parse_year_and_year_month_equal() {
    let from_year: DateTimeValue = "2026".parse().expect("2026 should parse");
    let from_year_month: DateTimeValue = "2026-01".parse().expect("2026-01 should parse");
    assert_eq!(
        from_year, from_year_month,
        "2026 and 2026-01 should normalize to same value"
    );
    assert_eq!(from_year.year, 2026);
    assert_eq!(from_year.month, 1);
    assert_eq!(from_year.day, 1);
    assert_eq!(from_year.hour, 0);
    assert_eq!(from_year.minute, 0);
    assert_eq!(from_year.second, 0);
}
