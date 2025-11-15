use crate::semantic::*;
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
fn test_literal_value_to_type() {
    let one = Decimal::from_str("1").unwrap();

    assert_eq!(
        LiteralValue::Text("".to_string()).to_type(),
        LemmaType::Text
    );
    assert_eq!(LiteralValue::Number(one).to_type(), LemmaType::Number);
    assert_eq!(LiteralValue::Boolean(true).to_type(), LemmaType::Boolean);

    let dt = DateTimeValue {
        year: 2024,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        timezone: None,
    };
    assert_eq!(LiteralValue::Date(dt).to_type(), LemmaType::Date);
    assert_eq!(
        LiteralValue::Percentage(one).to_type(),
        LemmaType::Percentage
    );
    assert_eq!(
        LiteralValue::Regex("".to_string()).to_type(),
        LemmaType::Regex
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Mass(one, MassUnit::Kilogram)).to_type(),
        LemmaType::Mass
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Length(one, LengthUnit::Meter)).to_type(),
        LemmaType::Length
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Volume(one, VolumeUnit::Liter)).to_type(),
        LemmaType::Volume
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Duration(one, DurationUnit::Second)).to_type(),
        LemmaType::Duration
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Temperature(one, TemperatureUnit::Celsius)).to_type(),
        LemmaType::Temperature
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Power(one, PowerUnit::Watt)).to_type(),
        LemmaType::Power
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Force(one, ForceUnit::Newton)).to_type(),
        LemmaType::Force
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Pressure(one, PressureUnit::Pascal)).to_type(),
        LemmaType::Pressure
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Energy(one, EnergyUnit::Joule)).to_type(),
        LemmaType::Energy
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Frequency(one, FrequencyUnit::Hertz)).to_type(),
        LemmaType::Frequency
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Data(one, DataUnit::Byte)).to_type(),
        LemmaType::Data
    );
    assert_eq!(
        LiteralValue::Unit(NumericUnit::Money(one, MoneyUnit::Usd)).to_type(),
        LemmaType::Money
    );
}

#[test]
fn test_numeric_unit_validate_same_currency() {
    let one = Decimal::from_str("1").unwrap();
    let money_usd = NumericUnit::Money(one, MoneyUnit::Usd);
    let money_eur = NumericUnit::Money(one, MoneyUnit::Eur);
    let mass = NumericUnit::Mass(one, MassUnit::Kilogram);

    assert!(money_usd.validate_same_currency(&money_usd).is_ok());
    assert!(money_usd.validate_same_currency(&mass).is_ok());
    assert!(money_usd.validate_same_currency(&money_eur).is_err());
}

#[test]
fn test_display_implementations() {
    let hundred = Decimal::from_str("100").unwrap();
    assert_eq!(format!("{}", ArithmeticComputation::Add), "+");
    assert_eq!(
        format!("{}", ComparisonComputation::GreaterThanOrEqual),
        ">="
    );
    assert_eq!(
        format!(
            "{}",
            LiteralValue::Unit(NumericUnit::Money(hundred, MoneyUnit::Eur))
        ),
        "100 EUR"
    );
}

#[test]
fn test_numeric_unit_value() {
    let ten = Decimal::from_str("10").unwrap();
    let twenty = Decimal::from_str("20").unwrap();

    assert_eq!(NumericUnit::Mass(ten, MassUnit::Kilogram).value(), ten);
    assert_eq!(
        NumericUnit::Length(twenty, LengthUnit::Meter).value(),
        twenty
    );
    assert_eq!(NumericUnit::Money(ten, MoneyUnit::Usd).value(), ten);
    assert_eq!(
        NumericUnit::Duration(twenty, DurationUnit::Second).value(),
        twenty
    );
}

#[test]
fn test_numeric_unit_same_category() {
    let ten = Decimal::from_str("10").unwrap();
    let twenty = Decimal::from_str("20").unwrap();

    let kg = NumericUnit::Mass(ten, MassUnit::Kilogram);
    let lb = NumericUnit::Mass(twenty, MassUnit::Pound);
    let meter = NumericUnit::Length(ten, LengthUnit::Meter);

    assert!(kg.same_category(&lb), "Same mass units should match");
    assert!(
        !kg.same_category(&meter),
        "Different unit types should not match"
    );
}

#[test]
fn test_numeric_unit_with_value() {
    let ten = Decimal::from_str("10").unwrap();
    let fifty = Decimal::from_str("50").unwrap();

    let original = NumericUnit::Mass(ten, MassUnit::Kilogram);
    let updated = original.with_value(fifty);

    assert_eq!(updated.value(), fifty);
    assert!(original.same_category(&updated));
    assert_eq!(format!("{}", updated), "50 kilogram");
}

#[test]
fn test_numeric_unit_validate_currency_mismatch() {
    let ten = Decimal::from_str("10").unwrap();
    let usd = NumericUnit::Money(ten, MoneyUnit::Usd);
    let eur = NumericUnit::Money(ten, MoneyUnit::Eur);
    let kg = NumericUnit::Mass(ten, MassUnit::Kilogram);

    // Same currency should pass
    assert!(usd.validate_same_currency(&usd).is_ok());

    // Different currencies should fail
    assert!(usd.validate_same_currency(&eur).is_err());

    // Non-money units should pass
    assert!(kg.validate_same_currency(&usd).is_ok());
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
fn test_unit_display_formats() {
    let one = Decimal::from_str("1").unwrap();

    // Mass units
    assert_eq!(format!("{}", MassUnit::Kilogram), "kilogram");
    assert_eq!(format!("{}", MassUnit::Pound), "pound");
    assert_eq!(
        format!("{}", NumericUnit::Mass(one, MassUnit::Gram)),
        "1 gram"
    );

    // Length units
    assert_eq!(format!("{}", LengthUnit::Meter), "meter");
    assert_eq!(format!("{}", LengthUnit::Mile), "mile");

    // Volume units
    assert_eq!(format!("{}", VolumeUnit::Liter), "liter");
    assert_eq!(format!("{}", VolumeUnit::Gallon), "gallon");

    // Duration units
    assert_eq!(format!("{}", DurationUnit::Second), "second");
    assert_eq!(format!("{}", DurationUnit::Hour), "hour");

    // Temperature units
    assert_eq!(format!("{}", TemperatureUnit::Celsius), "celsius");
    assert_eq!(format!("{}", TemperatureUnit::Fahrenheit), "fahrenheit");

    // Power units
    assert_eq!(format!("{}", PowerUnit::Watt), "watt");
    assert_eq!(format!("{}", PowerUnit::Kilowatt), "kilowatt");

    // Other units
    assert_eq!(format!("{}", ForceUnit::Newton), "newton");
    assert_eq!(format!("{}", PressureUnit::Pascal), "pascal");
    assert_eq!(format!("{}", EnergyUnit::Joule), "joule");
    assert_eq!(format!("{}", FrequencyUnit::Hertz), "hertz");
    assert_eq!(format!("{}", DataUnit::Byte), "byte");
    assert_eq!(format!("{}", DataUnit::Gigabyte), "gigabyte");
}

#[test]
fn test_money_unit_display() {
    assert_eq!(format!("{}", MoneyUnit::Usd), "USD");
    assert_eq!(format!("{}", MoneyUnit::Eur), "EUR");
    assert_eq!(format!("{}", MoneyUnit::Gbp), "GBP");
    assert_eq!(format!("{}", MoneyUnit::Jpy), "JPY");
    assert_eq!(format!("{}", MoneyUnit::Cny), "CNY");
}

#[test]
fn test_conversion_target_display() {
    assert_eq!(
        format!("{}", ConversionTarget::Mass(MassUnit::Kilogram)),
        "kilogram"
    );
    assert_eq!(
        format!("{}", ConversionTarget::Length(LengthUnit::Meter)),
        "meter"
    );
    assert_eq!(
        format!("{}", ConversionTarget::Money(MoneyUnit::Usd)),
        "USD"
    );
    assert_eq!(format!("{}", ConversionTarget::Percentage), "percentage");
}

#[test]
fn test_lemma_type_display() {
    assert_eq!(format!("{}", LemmaType::Text), "text");
    assert_eq!(format!("{}", LemmaType::Number), "number");
    assert_eq!(format!("{}", LemmaType::Date), "date");
    assert_eq!(format!("{}", LemmaType::Boolean), "boolean");
    assert_eq!(format!("{}", LemmaType::Percentage), "percentage");
    assert_eq!(format!("{}", LemmaType::Mass), "mass");
    assert_eq!(format!("{}", LemmaType::Money), "money");
}

#[test]
fn test_literal_value_display_value() {
    let ten = Decimal::from_str("10").unwrap();

    assert_eq!(
        LiteralValue::Text("hello".to_string()).display_value(),
        "\"hello\""
    );
    assert_eq!(LiteralValue::Number(ten).display_value(), "10");
    assert_eq!(LiteralValue::Boolean(true).display_value(), "true");
    assert_eq!(LiteralValue::Boolean(false).display_value(), "false");
    assert_eq!(LiteralValue::Percentage(ten).display_value(), "10%");

    let money = LiteralValue::Unit(NumericUnit::Money(ten, MoneyUnit::Usd));
    assert_eq!(money.display_value(), "10 USD");

    let time = TimeValue {
        hour: 14,
        minute: 30,
        second: 0,
        timezone: None,
    };
    let time_display = LiteralValue::Time(time).display_value();
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
    assert_eq!(LiteralValue::Time(time).to_type(), LemmaType::Date);
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
fn test_all_unit_categories() {
    let v = Decimal::from_str("1").unwrap();

    // Test that all unit types can be created
    let _ = NumericUnit::Mass(v, MassUnit::Kilogram);
    let _ = NumericUnit::Length(v, LengthUnit::Meter);
    let _ = NumericUnit::Volume(v, VolumeUnit::Liter);
    let _ = NumericUnit::Duration(v, DurationUnit::Second);
    let _ = NumericUnit::Temperature(v, TemperatureUnit::Celsius);
    let _ = NumericUnit::Power(v, PowerUnit::Watt);
    let _ = NumericUnit::Force(v, ForceUnit::Newton);
    let _ = NumericUnit::Pressure(v, PressureUnit::Pascal);
    let _ = NumericUnit::Energy(v, EnergyUnit::Joule);
    let _ = NumericUnit::Frequency(v, FrequencyUnit::Hertz);
    let _ = NumericUnit::Data(v, DataUnit::Byte);
    let _ = NumericUnit::Money(v, MoneyUnit::Usd);
}

#[test]
fn test_negation_types() {
    // Ensure all negation types exist
    let _ = NegationType::Not;
    let _ = NegationType::HaveNot;
    let _ = NegationType::NotHave;
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
