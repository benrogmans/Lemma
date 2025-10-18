use lemma::Engine;
use rust_decimal::Decimal;

fn get_rule_value(engine: &Engine, doc_name: &str, rule_name: &str) -> lemma::LiteralValue {
    let response = engine.evaluate(doc_name, vec![]).unwrap();
    response
        .results
        .iter()
        .find(|r| r.rule_name == rule_name)
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .clone()
}

#[test]
fn test_leap_year_feb_29_valid() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact leap_date = 2024-02-29
rule check = leap_date
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");
    let response = engine.evaluate("test", vec![]).expect("Failed to evaluate");
    assert!(!response.results.is_empty());
}

#[test]
fn test_leap_year_century_2000() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact leap_date = 2000-02-29
rule check = leap_date
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");
    let response = engine.evaluate("test", vec![]).expect("Failed to evaluate");
    assert!(!response.results.is_empty());
}

#[test]
fn test_non_leap_year_century_1900() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 1900-02-28
rule next_day = start_date + 1 day
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "next_day") {
        assert_eq!(date.year, 1900);
        assert_eq!(date.month, 3);
        assert_eq!(date.day, 1);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_leap_year_century_2100_not_leap() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2100-02-28
rule next_day = start_date + 1 day
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "next_day") {
        assert_eq!(date.year, 2100);
        assert_eq!(date.month, 3);
        assert_eq!(date.day, 1);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_add_month_with_day_overflow_jan_31_to_feb() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2024-01-31
rule next_month = start_date + 1 month
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "next_month") {
        assert_eq!(date.year, 2024);
        assert_eq!(date.month, 2);
        assert_eq!(date.day, 29);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_add_month_with_day_overflow_jan_31_to_feb_non_leap() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2023-01-31
rule next_month = start_date + 1 month
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "next_month") {
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, 2);
        assert_eq!(date.day, 28);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_add_year_to_feb_29_leap_to_non_leap() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact leap_date = 2024-02-29
rule next_year = leap_date + 1 year
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "next_year") {
        assert_eq!(date.year, 2025);
        assert_eq!(date.month, 2);
        assert_eq!(date.day, 28);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_add_4_years_to_feb_29_leap_to_leap() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact leap_date = 2024-02-29
rule four_years_later = leap_date + 4 years
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "four_years_later") {
        assert_eq!(date.year, 2028);
        assert_eq!(date.month, 2);
        assert_eq!(date.day, 29);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_subtract_months_cross_year_boundary() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2024-02-15
rule three_months_ago = start_date - 3 months
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "three_months_ago") {
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, 11);
        assert_eq!(date.day, 15);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_add_months_cross_multiple_years() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2023-01-15
rule twenty_months_later = start_date + 20 months
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "twenty_months_later")
    {
        assert_eq!(date.year, 2024);
        assert_eq!(date.month, 9);
        assert_eq!(date.day, 15);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_subtract_year_from_year_boundary() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2024-01-01
rule last_year = start_date - 1 year
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "last_year") {
        assert_eq!(date.year, 2023);
        assert_eq!(date.month, 1);
        assert_eq!(date.day, 1);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_date_difference_across_leap_year() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2024-01-01
fact end_date = 2025-01-01
rule days_diff = end_date - start_date
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Unit(lemma::NumericUnit::Duration(seconds, _)) =
        get_rule_value(&engine, "test", "days_diff")
    {
        // 366 days = 31,622,400 seconds
        assert_eq!(seconds, Decimal::from(31622400));
    } else {
        panic!("Expected Duration value");
    }
}

#[test]
fn test_date_difference_non_leap_year() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2023-01-01
fact end_date = 2024-01-01
rule days_diff = end_date - start_date
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Unit(lemma::NumericUnit::Duration(seconds, _)) =
        get_rule_value(&engine, "test", "days_diff")
    {
        // 365 days = 31,536,000 seconds
        assert_eq!(seconds, Decimal::from(31536000));
    } else {
        panic!("Expected Duration value");
    }
}

#[test]
fn test_add_hours_crossing_midnight() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_datetime = 2024-03-15T22:00:00
rule next_day = start_datetime + 5 hours
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "next_day") {
        assert_eq!(date.year, 2024);
        assert_eq!(date.month, 3);
        assert_eq!(date.day, 16);
        assert_eq!(date.hour, 3);
        assert_eq!(date.minute, 0);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_subtract_hours_crossing_midnight_backward() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_datetime = 2024-03-16T02:00:00
rule prev_day = start_datetime - 5 hours
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "prev_day") {
        assert_eq!(date.year, 2024);
        assert_eq!(date.month, 3);
        assert_eq!(date.day, 15);
        assert_eq!(date.hour, 21);
        assert_eq!(date.minute, 0);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_add_minutes_precise() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_time = 2024-03-15T10:30:45
rule later = start_time + 90 minutes
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "later") {
        assert_eq!(date.hour, 12);
        assert_eq!(date.minute, 0);
        assert_eq!(date.second, 45);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_add_seconds_overflow_to_minutes() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_time = 2024-03-15T10:30:30
rule later = start_time + 90 seconds
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "later") {
        assert_eq!(date.hour, 10);
        assert_eq!(date.minute, 32);
        assert_eq!(date.second, 0);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_time_arithmetic_crossing_midnight() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact evening_time = 23:30:00
rule after_midnight = evening_time + 90 minutes
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Time(time) = get_rule_value(&engine, "test", "after_midnight") {
        assert_eq!(time.hour, 1);
        assert_eq!(time.minute, 0);
        assert_eq!(time.second, 0);
    } else {
        panic!("Expected Time value");
    }
}

#[test]
fn test_time_difference() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_time = 10:00:00
fact end_time = 15:30:00
rule duration = end_time - start_time
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Unit(lemma::NumericUnit::Duration(seconds, _)) =
        get_rule_value(&engine, "test", "duration")
    {
        // 5.5 hours = 19800 seconds
        assert_eq!(seconds, Decimal::new(19800, 0));
    } else {
        panic!("Expected Duration value");
    }
}

#[test]
fn test_negative_time_difference() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_time = 15:30:00
fact end_time = 10:00:00
rule duration = end_time - start_time
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Unit(lemma::NumericUnit::Duration(seconds, _)) =
        get_rule_value(&engine, "test", "duration")
    {
        // -5.5 hours = -19800 seconds
        assert_eq!(seconds, Decimal::new(-19800, 0));
    } else {
        panic!("Expected Duration value");
    }
}

#[test]
fn test_add_large_duration_days() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2024-01-01
rule future = start_date + 1000 days
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "future") {
        assert_eq!(date.year, 2026);
        assert_eq!(date.month, 9);
        assert_eq!(date.day, 27);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_fractional_hours() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_time = 2024-03-15T10:00:00
rule later = start_time + 2.5 hours
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "later") {
        assert_eq!(date.hour, 12);
        assert_eq!(date.minute, 30);
        assert_eq!(date.second, 0);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_datetime_comparison_across_years() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact date1 = 2023-12-31T23:59:59
fact date2 = 2024-01-01T00:00:00
rule is_before = date1 < date2
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Boolean(value) = get_rule_value(&engine, "test", "is_before") {
        assert!(value);
    } else {
        panic!("Expected Boolean value");
    }
}

#[test]
fn test_month_31_to_30_day_month() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2024-03-31
rule april = start_date + 1 month
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "april") {
        assert_eq!(date.year, 2024);
        assert_eq!(date.month, 4);
        assert_eq!(date.day, 30);
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_dec_31_plus_1_month() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_date = 2023-12-31
rule january = start_date + 1 month
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "january") {
        assert_eq!(date.year, 2024);
        assert_eq!(date.month, 1);
        assert_eq!(date.day, 31);
    } else {
        panic!("Expected Date value");
    }
}
