use lemma::parsing::ast::{DateTimeValue, TimezoneValue};
use lemma::{Engine, ValueKind};
mod common;
use common::add_lemma_code_blocking;
use std::collections::HashMap;

fn make_effective(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTimeValue {
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
    }
}

fn make_effective_tz(
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
    }
}

fn eval_rule_bool(
    engine: &Engine,
    spec_name: &str,
    rule: &str,
    effective: &DateTimeValue,
    facts: HashMap<String, String>,
) -> bool {
    let response = engine
        .run(spec_name, Some(effective), facts, false)
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == rule)
        .unwrap_or_else(|| panic!("rule '{}' not found in response", rule));
    match &rule_result.result.value().unwrap().value {
        ValueKind::Boolean(b) => *b,
        other => panic!("expected Boolean, got {:?}", other),
    }
}

fn eval_rule_date(
    engine: &Engine,
    spec_name: &str,
    rule: &str,
    effective: &DateTimeValue,
    facts: HashMap<String, String>,
) -> lemma::LiteralValue {
    let response = engine
        .run(spec_name, Some(effective), facts, false)
        .unwrap();
    response
        .results
        .values()
        .find(|r| r.rule.name == rule)
        .unwrap_or_else(|| panic!("rule '{}' not found in response", rule))
        .result
        .value()
        .unwrap()
        .clone()
}

// =============================================================================
// now keyword
// =============================================================================

#[test]
fn now_resolves_to_effective_datetime() {
    let mut engine = Engine::new();
    let code = r#"
spec test
rule current: now
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 14, 30, 0);
    let lit = eval_rule_date(&engine, "test", "current", &effective, HashMap::new());
    if let ValueKind::Date(dt) = &lit.value {
        assert_eq!(dt.year, 2026);
        assert_eq!(dt.month, 3);
        assert_eq!(dt.day, 7);
        assert_eq!(dt.hour, 14);
        assert_eq!(dt.minute, 30);
    } else {
        panic!("expected Date, got {:?}", lit.value);
    }
}

#[test]
fn now_in_arithmetic_subtraction() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact birth_date: 2000-01-01
rule age_duration: now - birth_date
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 1, 1, 0, 0, 0);
    let lit = eval_rule_date(&engine, "test", "age_duration", &effective, HashMap::new());
    if let ValueKind::Duration(seconds, _) = &lit.value {
        let days = *seconds / rust_decimal::Decimal::from(86400);
        assert_eq!(days, rust_decimal::Decimal::from(9497));
    } else {
        panic!("expected Duration, got {:?}", lit.value);
    }
}

#[test]
fn now_in_comparison() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact deadline: 2026-04-01
rule is_before_deadline: now < deadline
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "is_before_deadline",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn now_in_comparison_after_deadline() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact deadline: 2026-04-01
rule is_before_deadline: now < deadline
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 5, 1, 0, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "is_before_deadline",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn now_different_effective_gives_different_result() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact threshold: 2026-06-01
rule is_past_threshold: now > threshold
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();

    let before = make_effective(2026, 3, 1, 0, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "is_past_threshold",
        &before,
        HashMap::new()
    ));

    let after = make_effective(2026, 7, 1, 0, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "is_past_threshold",
        &after,
        HashMap::new()
    ));
}

// =============================================================================
// in past / in future (no tolerance)
// =============================================================================

#[test]
fn in_past_with_literal_date() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-01-15
rule was_in_past: event_date in past
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "was_in_past",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_future_date_is_false() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-12-25
rule was_in_past: event_date in past
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "was_in_past",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_future_with_literal_date() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact launch_date: 2026-12-01
rule is_upcoming: launch_date in future
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "is_upcoming",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_future_past_date_is_false() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact launch_date: 2025-06-01
rule is_upcoming: launch_date in future
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "is_upcoming",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_date_equal_now_is_false() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-07T12:00:00Z
rule check: event_date in past
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_future_date_equal_now_is_false() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-07T12:00:00Z
rule check: event_date in future
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

// =============================================================================
// in past / in future (with tolerance)
// =============================================================================

#[test]
fn in_past_7_days_inside_window() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact delivered: 2026-03-03
rule recent_delivery: delivered in past 7 days
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "recent_delivery",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_7_days_outside_window() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact delivered: 2026-02-15
rule recent_delivery: delivered in past 7 days
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "recent_delivery",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_future_30_days_inside_window() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact renewal_date: 2026-03-20
rule upcoming_renewal: renewal_date in future 30 days
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "upcoming_renewal",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_future_30_days_outside_window() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact renewal_date: 2026-06-15
rule upcoming_renewal: renewal_date in future 30 days
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "upcoming_renewal",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_tolerance_at_exact_boundary() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event: 2026-02-28T12:00:00Z
rule check: event in past 7 days
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    // now - 7 days = 2026-02-28T12:00:00Z, event == window_start → inclusive
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_tolerance_with_hours() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event: 2026-03-07T10:00:00Z
rule check: event in past 4 hours
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_tolerance_with_hours_outside() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event: 2026-03-07T06:00:00Z
rule check: event in past 4 hours
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

// =============================================================================
// in calendar year/month/week
// =============================================================================

#[test]
fn in_calendar_year_same_year() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact invoice_date: 2026-06-15
rule current_year_invoice: invoice_date in calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "current_year_invoice",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_year_different_year() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact invoice_date: 2025-06-15
rule current_year_invoice: invoice_date in calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "current_year_invoice",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_calendar_year() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact invoice_date: 2025-06-15
rule last_year_invoice: invoice_date in past calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "last_year_invoice",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_calendar_year_two_years_ago_excluded() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact invoice_date: 2024-06-15
rule last_year_invoice: invoice_date in past calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "last_year_invoice",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_future_calendar_year() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact target_date: 2027-06-15
rule next_year: target_date in future calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "next_year",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn not_in_calendar_year() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact old_date: 2024-01-01
rule is_not_this_year: old_date not in calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "is_not_this_year",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn not_in_calendar_year_current_year_is_false() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact today_date: 2026-03-07
rule is_not_this_year: today_date not in calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "is_not_this_year",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_month_same_month() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact payment_date: 2026-03-15
rule this_month_payment: payment_date in calendar month
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "this_month_payment",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_month_different_month() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact payment_date: 2026-04-01
rule this_month_payment: payment_date in calendar month
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "this_month_payment",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_calendar_month() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact payment_date: 2026-02-15
rule last_month_payment: payment_date in past calendar month
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "last_month_payment",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_calendar_month_cross_year_boundary() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact payment_date: 2025-12-15
rule last_month_payment: payment_date in past calendar month
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 1, 15, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "last_month_payment",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_future_calendar_month() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact due_date: 2026-04-15
rule next_month_due: due_date in future calendar month
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "next_month_due",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_week_same_week() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact meeting_date: 2026-03-02
rule this_week_meeting: meeting_date in calendar week
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    // 2026-03-07 is Saturday, ISO week 10 (Mon Mar 2 - Sun Mar 8)
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "this_week_meeting",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_week_different_week() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact meeting_date: 2026-03-15
rule this_week_meeting: meeting_date in calendar week
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "this_week_meeting",
        &effective,
        HashMap::new()
    ));
}

// =============================================================================
// unless clauses with date sugar
// =============================================================================

#[test]
fn unless_with_in_past() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact order_date: 2026-03-05
rule shipping_fee: 15
  unless order_date in past 3 days then 0
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    let lit = eval_rule_date(&engine, "test", "shipping_fee", &effective, HashMap::new());
    if let ValueKind::Number(n) = &lit.value {
        assert_eq!(*n, rust_decimal::Decimal::from(0));
    } else {
        panic!("expected Number, got {:?}", lit.value);
    }
}

#[test]
fn unless_with_in_past_not_matching() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact order_date: 2026-02-01
rule shipping_fee: 15
  unless order_date in past 3 days then 0
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    let lit = eval_rule_date(&engine, "test", "shipping_fee", &effective, HashMap::new());
    if let ValueKind::Number(n) = &lit.value {
        assert_eq!(*n, rust_decimal::Decimal::from(15));
    } else {
        panic!("expected Number, got {:?}", lit.value);
    }
}

#[test]
fn unless_with_in_calendar_year() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact hire_date: 2026-01-15
rule is_new_hire: false
  unless hire_date in calendar year then true
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "is_new_hire",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn unless_with_not_in_calendar_year() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact hire_date: 2024-06-01
rule needs_recertification: false
  unless hire_date not in calendar year then true
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "needs_recertification",
        &effective,
        HashMap::new()
    ));
}

// =============================================================================
// combined conditions
// =============================================================================

#[test]
fn date_sugar_combined_with_and() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact order_date: 2026-03-05
fact is_premium: true
rule qualifies: order_date in past 7 days and is_premium
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "qualifies",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn date_sugar_combined_with_and_false() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact order_date: 2026-03-05
fact is_premium: false
rule qualifies: order_date in past 7 days and is_premium
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "qualifies",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn multiple_date_sugar_in_unless_chain() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-05
rule status: "unknown"
  unless event_date in past then "completed"
  unless event_date in future then "upcoming"
  unless event_date in past 3 days then "recently_completed"
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    let lit = eval_rule_date(&engine, "test", "status", &effective, HashMap::new());
    if let ValueKind::Text(s) = &lit.value {
        assert_eq!(s, "recently_completed");
    } else {
        panic!("expected Text, got {:?}", lit.value);
    }
}

// =============================================================================
// fact binding with date sugar
// =============================================================================

#[test]
fn in_past_with_fact_binding() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: [date]
rule was_recent: event_date in past 7 days
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    let mut facts = HashMap::new();
    facts.insert("event_date".to_string(), "2026-03-05".to_string());
    assert!(eval_rule_bool(
        &engine,
        "test",
        "was_recent",
        &effective,
        facts
    ));
}

#[test]
fn in_past_with_fact_binding_outside_window() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: [date]
rule was_recent: event_date in past 7 days
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    let mut facts = HashMap::new();
    facts.insert("event_date".to_string(), "2026-01-01".to_string());
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "was_recent",
        &effective,
        facts
    ));
}

// =============================================================================
// timezone-aware evaluation
// =============================================================================

#[test]
fn in_past_timezone_aware() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-07T00:00:00Z
rule check: event_date in past
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    // effective is 2026-03-07T01:00:00+02:00 = 2026-03-06T23:00:00Z
    // event (UTC midnight Mar 7) is AFTER effective (UTC 23:00 Mar 6)
    let effective = make_effective_tz((2026, 3, 7, 1, 0, 0), (2, 0));
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_year_with_timezone_boundary() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-12-31T23:00:00Z
rule check: event_date in calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    // effective timezone is +05:00; year boundary ends at 2026-12-31T23:59:59.999999+05:00
    // = 2026-12-31T18:59:59.999999 UTC
    // event is 2026-12-31T23:00:00 UTC which is past the boundary
    let effective = make_effective_tz((2026, 6, 15, 12, 0, 0), (5, 0));
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

// =============================================================================
// type checking errors
// =============================================================================

#[test]
fn in_past_with_non_date_type_produces_error() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact quantity: 42
rule check: quantity in past
    "#;
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "should reject non-date operand for 'in past'"
    );
}

#[test]
fn in_calendar_year_with_non_date_type_produces_error() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact name: "Alice"
rule check: name in calendar year
    "#;
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "should reject non-date operand for 'in calendar year'"
    );
}

#[test]
fn in_past_with_non_duration_tolerance_produces_error() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-01
rule check: event_date in past 7
    "#;
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_err(),
        "should reject non-duration tolerance for 'in past'"
    );
}

// =============================================================================
// calendar month edge cases
// =============================================================================

#[test]
fn in_calendar_month_february_leap_year() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact payment_date: 2024-02-29
rule this_month: payment_date in calendar month
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2024, 2, 15, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "this_month",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_month_last_day_of_31_day_month() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-31T23:59:59Z
rule check: event_date in calendar month
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 1, 0, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_month_first_of_next_month_excluded() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-04-01T00:00:00Z
rule check: event_date in calendar month
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 15, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

// =============================================================================
// calendar week edge cases
// =============================================================================

#[test]
fn in_calendar_week_monday_boundary() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-02T00:00:00Z
rule check: event_date in calendar week
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    // 2026-03-07 is Saturday, week Mon Mar 2 - Sun Mar 8
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_week_sunday_boundary() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-08T23:59:59Z
rule check: event_date in calendar week
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_week_next_monday_excluded() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-03-09T00:00:00Z
rule check: event_date in calendar week
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 3, 7, 12, 0, 0);
    assert!(!eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

// =============================================================================
// year boundary edge cases
// =============================================================================

#[test]
fn in_calendar_year_jan_1_boundary() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-01-01T00:00:00Z
rule check: event_date in calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 6, 15, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_calendar_year_dec_31_boundary() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2026-12-31T23:59:59Z
rule check: event_date in calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 6, 15, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_calendar_year_boundary_last_day() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2025-12-31T23:59:59Z
rule check: event_date in past calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 1, 1, 0, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}

#[test]
fn in_past_calendar_year_boundary_first_day() {
    let mut engine = Engine::new();
    let code = r#"
spec test
fact event_date: 2025-01-01T00:00:00Z
rule check: event_date in past calendar year
    "#;
    add_lemma_code_blocking(&mut engine, code, "test.lemma").unwrap();
    let effective = make_effective(2026, 6, 15, 12, 0, 0);
    assert!(eval_rule_bool(
        &engine,
        "test",
        "check",
        &effective,
        HashMap::new()
    ));
}
