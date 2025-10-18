use lemma::Engine;
use rust_decimal::Decimal;

fn get_rule_value(
    engine: &Engine,
    doc_name: &str,
    rule_name: &str,
) -> lemma::LiteralValue {
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
fn test_timezone_comparison_same_instant() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact time_nyc = 2024-03-15T10:00:00-05:00
fact time_london = 2024-03-15T15:00:00+00:00
rule are_equal = time_nyc == time_london
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Boolean(value) = get_rule_value(&engine, "test", "are_equal")
    {
        assert!(value, "Same instant in different timezones should be equal");
    } else {
        panic!("Expected Boolean value");
    }
}

#[test]
fn test_timezone_comparison_different_instants() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact time_nyc = 2024-03-15T10:00:00-05:00
fact time_tokyo = 2024-03-15T10:00:00+09:00
rule nyc_is_later = time_nyc > time_tokyo
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Boolean(value) =
        get_rule_value(&engine, "test", "nyc_is_later")
    {
        assert!(value, "NYC 10am is later than Tokyo 10am (same local time)");
    } else {
        panic!("Expected Boolean value");
    }
}

#[test]
fn test_timezone_arithmetic_preserved() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact start_time = 2024-03-15T10:00:00+01:00
rule later = start_time + 2 hours
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "later") {
        assert_eq!(date.hour, 12);
        assert_eq!(date.minute, 0);
        assert_eq!(date.second, 0);
        if let Some(tz) = date.timezone {
            assert_eq!(tz.offset_hours, 1);
            assert_eq!(tz.offset_minutes, 0);
        } else {
            panic!("Expected timezone to be preserved");
        }
    } else {
        panic!("Expected Date value");
    }
}

// Note: Z notation parsing has issues in current grammar
// Using +00:00 is more reliable

#[test]
fn test_negative_timezone_offset() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact west_coast = 2024-03-15T09:00:00-08:00
rule later = west_coast + 3 hours
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "later") {
        assert_eq!(date.hour, 12);
        if let Some(tz) = date.timezone {
            assert_eq!(tz.offset_hours, -8);
            assert_eq!(tz.offset_minutes, 0);
        } else {
            panic!("Expected timezone to be preserved");
        }
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_timezone_crossing_midnight() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact evening = 2024-03-15T23:00:00+05:30
rule next_day = evening + 2 hours
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "next_day") {
        assert_eq!(date.year, 2024);
        assert_eq!(date.month, 3);
        assert_eq!(date.day, 16);
        assert_eq!(date.hour, 1);
        assert_eq!(date.minute, 0);
        if let Some(tz) = date.timezone {
            assert_eq!(tz.offset_hours, 5);
            assert_eq!(tz.offset_minutes, 30);
        } else {
            panic!("Expected timezone to be preserved");
        }
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_timezone_date_difference() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact time1 = 2024-03-15T10:00:00-05:00
fact time2 = 2024-03-15T16:00:00+01:00
rule hours_diff = time2 - time1
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Unit(lemma::NumericUnit::Duration(seconds, _)) =
        get_rule_value(&engine, "test", "hours_diff")
    {
        // time1: 10:00 -05:00 = 15:00 UTC
        // time2: 16:00 +01:00 = 15:00 UTC
        // Difference should be 0
        assert_eq!(seconds, Decimal::from(0));
    } else {
        panic!("Expected Duration value");
    }
}

#[test]
fn test_timezone_30_minute_offset() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact india_time = 2024-03-15T14:30:00+05:30
rule utc_equivalent = india_time
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) =
        get_rule_value(&engine, "test", "utc_equivalent")
    {
        if let Some(tz) = date.timezone {
            assert_eq!(tz.offset_hours, 5);
            assert_eq!(tz.offset_minutes, 30);
        } else {
            panic!("Expected timezone to be present");
        }
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_timezone_45_minute_offset() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact nepal_time = 2024-03-15T14:30:00+05:45
rule preserved = nepal_time
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "preserved") {
        if let Some(tz) = date.timezone {
            assert_eq!(tz.offset_hours, 5);
            assert_eq!(tz.offset_minutes, 45);
        } else {
            panic!("Expected timezone to be present");
        }
    } else {
        panic!("Expected Date value");
    }
}

// Note: Time literals with timezones are not supported by chrono parser
// (time-only strings can't be parsed with timezone offsets)
// Use full datetime if timezone is needed

// Note: Datetime equality comparison requires using comparison operators like < or >
// == operator on datetime values has parser limitations

#[test]
fn test_extreme_western_timezone() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact hawaii = 2024-03-15T12:00:00-10:00
rule later = hawaii + 1 hour
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "later") {
        assert_eq!(date.hour, 13);
        if let Some(tz) = date.timezone {
            assert_eq!(tz.offset_hours, -10);
        } else {
            panic!("Expected timezone to be preserved");
        }
    } else {
        panic!("Expected Date value");
    }
}

#[test]
fn test_extreme_eastern_timezone() {
    let mut engine = Engine::new();
    let code = r#"
doc test
fact kiribati = 2024-03-15T12:00:00+14:00
rule earlier = kiribati - 1 hour
    "#;

    engine
        .add_lemma_code(code, "test.lemma")
        .expect("Failed to parse");

    if let lemma::LiteralValue::Date(date) = get_rule_value(&engine, "test", "earlier") {
        assert_eq!(date.hour, 11);
        if let Some(tz) = date.timezone {
            assert_eq!(tz.offset_hours, 14);
        } else {
            panic!("Expected timezone to be preserved");
        }
    } else {
        panic!("Expected Date value");
    }
}
