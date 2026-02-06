use crate::parsing::ast::*;
use rust_decimal::Decimal;
use std::str::FromStr;

#[test]
fn test_arithmetic_operation_name() {
    assert_eq!(ArithmeticComputation::Add.name(), "addition");
    assert_eq!(ArithmeticComputation::Subtract.name(), "subtraction");
    assert_eq!(ArithmeticComputation::Multiply.name(), "multiplication");
    assert_eq!(ArithmeticComputation::Divide.name(), "division");
    assert_eq!(ArithmeticComputation::Modulo.name(), "modulo");
    assert_eq!(ArithmeticComputation::Power.name(), "exponentiation");
}

#[test]
fn test_comparison_operator_name() {
    assert_eq!(ComparisonComputation::GreaterThan.name(), "greater than");
    assert_eq!(ComparisonComputation::LessThan.name(), "less than");
    assert_eq!(
        ComparisonComputation::GreaterThanOrEqual.name(),
        "greater than or equal"
    );
    assert_eq!(
        ComparisonComputation::LessThanOrEqual.name(),
        "less than or equal"
    );
    assert_eq!(ComparisonComputation::Equal.name(), "equal");
    assert_eq!(ComparisonComputation::NotEqual.name(), "not equal");
    assert_eq!(ComparisonComputation::Is.name(), "is");
    assert_eq!(ComparisonComputation::IsNot.name(), "is not");
}

#[test]
fn test_literal_value_to_primitive_type() {
    let one = Decimal::from_str("1").unwrap();

    assert_eq!(LiteralValue::text("".to_string()).lemma_type.name(), "text");
    assert_eq!(LiteralValue::number(one).lemma_type.name(), "number");
    assert_eq!(
        LiteralValue::boolean(crate::BooleanValue::True)
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
        timezone: None,
    };
    assert_eq!(LiteralValue::date(dt).lemma_type.name(), "date");
    assert_eq!(
        LiteralValue::ratio(one / rust_decimal::Decimal::from(100), None).lemma_type.name(),
        "ratio"
    );
    assert_eq!(
        LiteralValue::duration(one, DurationUnit::Second)
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
    assert_eq!(format!("{}", ComparisonComputation::Equal), "==");
    assert_eq!(format!("{}", ComparisonComputation::NotEqual), "!=");
    assert_eq!(format!("{}", ComparisonComputation::Is), "is");
    assert_eq!(format!("{}", ComparisonComputation::IsNot), "is not");
}

#[test]
fn test_duration_unit_display() {
    assert_eq!(format!("{}", DurationUnit::Second), "second");
    assert_eq!(format!("{}", DurationUnit::Minute), "minute");
    assert_eq!(format!("{}", DurationUnit::Hour), "hour");
    assert_eq!(format!("{}", DurationUnit::Day), "day");
    assert_eq!(format!("{}", DurationUnit::Week), "week");
    assert_eq!(format!("{}", DurationUnit::Millisecond), "millisecond");
    assert_eq!(format!("{}", DurationUnit::Microsecond), "microsecond");
}

#[test]
fn test_conversion_target_display() {
        assert_eq!(
        format!("{}", ConversionTarget::Duration(DurationUnit::Hour)),
        "hour"
    );
}

#[test]
fn test_doc_type_display() {
    assert_eq!(format!("{}", crate::planning::semantics::primitive_text()), "text");
    assert_eq!(format!("{}", crate::planning::semantics::primitive_number()), "number");
    assert_eq!(format!("{}", crate::planning::semantics::primitive_date()), "date");
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
}

#[test]
fn test_type_constructor() {
    let specs = TypeSpecification::number();
    let lemma_type = LemmaType::new("dice".to_string(), specs);
    assert_eq!(lemma_type.name(), "dice");
}

#[test]
fn test_type_display() {
    let specs = TypeSpecification::text();
    let lemma_type = LemmaType::new("name".to_string(), specs);
    assert_eq!(format!("{}", lemma_type), "name");
}

#[test]
fn test_type_equality() {
    let specs1 = TypeSpecification::number();
    let specs2 = TypeSpecification::number();
    let lemma_type1 = LemmaType::new("dice".to_string(), specs1);
    let lemma_type2 = LemmaType::new("dice".to_string(), specs2);
    assert_eq!(lemma_type1, lemma_type2);
}

#[test]
fn test_type_serialization() {
    let specs = TypeSpecification::number();
    let lemma_type = LemmaType::new("dice".to_string(), specs);
    let serialized = serde_json::to_string(&lemma_type).unwrap();
    let deserialized: LemmaType = serde_json::from_str(&serialized).unwrap();
    assert_eq!(lemma_type, deserialized);
}

#[test]
fn test_literal_value_display_value() {
    let ten = Decimal::from_str("10").unwrap();

    assert_eq!(
        LiteralValue::text("hello".to_string()).display_value(),
        "\"hello\""
    );
    assert_eq!(LiteralValue::number(ten).display_value(), "10");
    assert_eq!(
        LiteralValue::boolean(crate::BooleanValue::True).display_value(),
        "true"
    );
    assert_eq!(
        LiteralValue::boolean(crate::BooleanValue::False).display_value(),
        "false"
    );
    // 10% stored as 0.10 ratio
    let ten_percent_ratio = LiteralValue::ratio(rust_decimal::Decimal::from_str("0.10").unwrap(), None);
    assert_eq!(ten_percent_ratio.display_value(), "0.1");

    let time = TimeValue {
        hour: 14,
        minute: 30,
        second: 0,
        timezone: None,
    };
    let time_display = LiteralValue::time(time).display_value();
    assert!(time_display.contains("14"));
    assert!(time_display.contains("30"));
}

#[test]
fn test_literal_value_time_type() {
    let time = TimeValue {
        hour: 14,
        minute: 30,
        second: 0,
        timezone: None,
    };
    assert_eq!(LiteralValue::time(time).lemma_type.name(), "time");
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
        timezone: Some(TimezoneValue {
            offset_hours: 1,
            offset_minutes: 0,
        }),
    };
    let display = format!("{}", dt);
    assert!(display.contains("2024"));
    assert!(display.contains("12"));
    assert!(display.contains("25"));
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
    assert!(display.contains("14"));
    assert!(display.contains("30"));
    assert!(display.contains("45"));
}

#[test]
fn test_timezone_value() {
    let tz_positive = TimezoneValue {
        offset_hours: 5,
        offset_minutes: 30,
    };
    assert_eq!(tz_positive.offset_hours, 5);
    assert_eq!(tz_positive.offset_minutes, 30);

    let tz_negative = TimezoneValue {
        offset_hours: -8,
        offset_minutes: 0,
    };
    assert_eq!(tz_negative.offset_hours, -8);
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
fn test_expression_get_source_text_with_location() {
    use crate::{Expression, ExpressionKind, LiteralValue, Source, Span};
    use std::collections::HashMap;

    let source = "fact value = 42";
    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), source.to_string());

    let span = Span {
        start: 13,
        end: 15,
        line: 1,
        col: 13,
    };
    let source_location = Source::new("test.lemma", span, "test");
    let expr = Expression::new(
        ExpressionKind::Literal(LiteralValue::number(rust_decimal::Decimal::new(42, 0))),
        source_location,
    );

    assert_eq!(expr.get_source_text(&sources), Some("42".to_string()));
}

#[test]
fn test_expression_get_source_text_no_location() {
    use crate::{Expression, ExpressionKind, LiteralValue};
    use std::collections::HashMap;

    let mut sources = HashMap::new();
    sources.insert("test.lemma".to_string(), "fact value = 42".to_string());

    let expr = Expression::new(
        ExpressionKind::Literal(LiteralValue::number(rust_decimal::Decimal::new(42, 0))),
        None,
    );

    assert_eq!(expr.get_source_text(&sources), None);
}

#[test]
fn test_expression_get_source_text_source_not_found() {
    use crate::{Expression, ExpressionKind, LiteralValue, Source, Span};
    use std::collections::HashMap;

    let sources = HashMap::new();
    let span = Span {
        start: 0,
        end: 5,
        line: 1,
        col: 0,
    };
    let source_location = Source::new("missing.lemma", span, "test");
    let expr = Expression::new(
        ExpressionKind::Literal(LiteralValue::number(rust_decimal::Decimal::new(42, 0))),
        source_location,
    );

    assert_eq!(expr.get_source_text(&sources), None);
}
