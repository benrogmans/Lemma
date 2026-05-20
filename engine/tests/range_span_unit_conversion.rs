//! Range span projection: `(lo...hi) as <unit>` = **width** of the interval in `<unit>`,
//! not a range value and not `lo unit...hi unit` unless that is already a quantity range literal.
//!
//! | Range type | Correct `as` targets (when implemented) | Must reject |
//! |------------|----------------------------------------|-------------|
//! | **DateRange** | duration units (`days`, `seconds`), calendar (`years`, `months`), `number` | mass, ratio, … |
//! | **NumberRange** | `number`, duration units | mass, ratio, … |
//! | **QuantityRange** (duration) | duration units (`days`, `seconds`, `hours`, …) | — |
//! | **QuantityRange** (mass/money) | — (no duration span) | duration + same-family unit |
//! | **RatioRange** | ratio units (`percent`, …) when implemented | duration, mass, … |
//!
//! `CalendarRange` span `as` is not supported (calendar interval width uses month arithmetic, not quantity units).

use lemma::parsing::ast::{DateTimeValue, TimezoneValue};
use lemma::{Engine, LiteralValue};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn source() -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from("range_span_unit_conversion.lemma")))
}

fn default_effective() -> DateTimeValue {
    DateTimeValue {
        year: 2026,
        month: 3,
        day: 7,
        hour: 12,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: 0,
            offset_minutes: 0,
        }),
    }
}

fn eval_literal(code: &str, spec_name: &str, rule_name: &str) -> LiteralValue {
    let mut engine = Engine::new();
    engine.load(code, source()).expect("Should parse and plan");
    let response = engine
        .run(
            None,
            spec_name,
            Some(&default_effective()),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("Should evaluate");
    response
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("Rule '{}' not found", rule_name))
        .result
        .value()
        .unwrap_or_else(|| panic!("Rule '{}' returned non-value", rule_name))
        .clone()
}

fn eval_rule(code: &str, spec_name: &str, rule_name: &str) -> String {
    eval_literal(code, spec_name, rule_name).to_string()
}

fn expect_plan_error(code: &str, expected_fragment: &str) {
    let mut engine = Engine::new();
    let result = engine.load(code, source());
    assert!(result.is_err(), "Expected planning error");
    let combined = result
        .unwrap_err()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        combined
            .to_lowercase()
            .contains(&expected_fragment.to_lowercase()),
        "Expected error containing '{}', got: {}",
        expected_fragment,
        combined
    );
}

fn assert_no_range_syntax(actual: &str) {
    assert!(
        !actual.contains("..."),
        "Expected scalar output, got range-like '{}'",
        actual
    );
}

fn assert_contains_parts(actual: &str, parts: &[&str]) {
    assert_no_range_syntax(actual);
    let lower = actual.to_lowercase();
    for part in parts {
        assert!(
            lower.contains(&part.to_lowercase()),
            "Expected '{}' to contain '{}'",
            actual,
            part
        );
    }
}

const USES_SI: &str = "uses lemma si";

const MONEY: &str = r#"data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91"#;

const WEIGHT: &str = r#"data weight: quantity
  -> unit stone 1
  -> unit pound 14"#;

// =============================================================================
// DateRange
// =============================================================================

mod date_range {
    use super::*;

    #[test]
    fn span_as_days() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: 2024-01-01...2024-01-11 as days"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["10", "day"]);
    }

    #[test]
    fn span_as_seconds() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: 2024-01-01...2024-01-02 as seconds"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["86400", "second"]);
    }

    #[test]
    fn span_as_years_calendar_unit() {
        let code = r#"spec test
rule span: 2024-01-15...2025-01-15 as years"#;
        assert_contains_parts(&eval_rule(code, "test", "span"), &["1", "year"]);
    }

    #[test]
    fn span_as_months_calendar_unit() {
        let code = r#"spec test
rule span: 2024-01-16...2024-02-16 as months"#;
        assert_contains_parts(&eval_rule(code, "test", "span"), &["1", "month"]);
    }

    #[test]
    fn span_as_number() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: 2024-01-01...2024-01-11 as days as number"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["10"]);
    }

    #[test]
    fn rejects_span_as_mass_unit() {
        let code = format!(
            r#"spec test
{USES_SI}
{WEIGHT}
rule bad: 2024-01-01...2024-01-11 as stone"#
        );
        expect_plan_error(&code, "convert");
    }

    #[test]
    fn rejects_span_as_ratio_unit() {
        let code = r#"spec test
rule bad: 2024-01-01...2024-01-11 as percent"#;
        expect_plan_error(code, "convert");
    }
}

// =============================================================================
// NumberRange
// =============================================================================

mod number_range {
    use super::*;

    #[test]
    fn span_as_days() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: (0...100) as days
rule scalar: 100 as days"#
        );
        let span = eval_rule(&code, "test", "span");
        let scalar = eval_rule(&code, "test", "scalar");
        assert_contains_parts(&span, &["100", "day"]);
        assert_eq!(span, scalar, "span and scalar cast should match");
    }

    #[test]
    fn span_as_hours() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: (0...48) as hours"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["48", "hour"]);
    }

    #[test]
    fn reversed_endpoints_span_as_days() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: (100...0) as days"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["100", "day"]);
    }

    #[test]
    fn span_as_number() {
        let code = r#"spec test
rule span: (0...100) as number"#;
        assert_contains_parts(&eval_rule(code, "test", "span"), &["100"]);
    }

    #[test]
    fn rejects_span_as_mass_unit() {
        let code = format!(
            r#"spec test
{USES_SI}
{WEIGHT}
rule bad: (0...100) as stone"#
        );
        expect_plan_error(&code, "convert");
    }

    #[test]
    fn rejects_span_as_ratio_unit() {
        let code = r#"spec test
rule bad: (0...100) as percent"#;
        expect_plan_error(code, "convert");
    }
}

// =============================================================================
// QuantityRange (trait duration / si.duration)
// =============================================================================

mod duration_quantity_range {
    use super::*;

    #[test]
    fn span_as_days() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: (7 days...14 days) as days"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["7", "day"]);
    }

    #[test]
    fn span_as_hours() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: (2 hours...5 hours) as hours"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["3", "hour"]);
    }

    #[test]
    fn span_as_seconds() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: (7 days...14 days) as seconds"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["604800", "second"]);
    }

    #[test]
    fn span_as_weeks_mixed_day_endpoint() {
        let code = format!(
            r#"spec test
{USES_SI}
rule span: (7 days...2 weeks) as days"#
        );
        assert_contains_parts(&eval_rule(&code, "test", "span"), &["7", "day"]);
    }

    #[test]
    fn rejects_span_as_mass_unit() {
        let code = format!(
            r#"spec test
{USES_SI}
{WEIGHT}
rule bad: (7 days...14 days) as stone"#
        );
        expect_plan_error(&code, "convert");
    }

    #[test]
    fn rejects_span_as_ratio_unit() {
        let code = format!(
            r#"spec test
{USES_SI}
rule bad: (7 days...14 days) as percent"#
        );
        expect_plan_error(&code, "convert");
    }
}

// =============================================================================
// QuantityRange (mass / money — not a duration)
// =============================================================================

mod mass_quantity_range {
    use super::*;

    #[test]
    fn rejects_span_as_duration_with_mass_endpoints() {
        let code = format!(
            r#"spec test
{USES_SI}
{WEIGHT}
rule bad: (3 stone...5 stone) as days"#
        );
        expect_plan_error(&code, "convert");
    }

    #[test]
    fn rejects_span_as_same_mass_unit() {
        let code = format!(
            r#"spec test
{WEIGHT}
rule bad: (3 stone...5 stone) as stone"#
        );
        expect_plan_error(&code, "convert");
    }

    #[test]
    fn rejects_span_as_duration_nonsensical_unit_pairing() {
        let code = format!(
            r#"spec test
{USES_SI}
data cargo: quantity -> unit crate 1
rule bad: (3 crate...5 crate) as days"#
        );
        expect_plan_error(&code, "convert");
    }
}

mod money_quantity_range {
    use super::*;

    #[test]
    fn rejects_span_as_duration_with_money_endpoints() {
        let code = format!(
            r#"spec test
{USES_SI}
{MONEY}
rule bad: (10 eur...50 eur) as days"#
        );
        expect_plan_error(&code, "convert");
    }

    #[test]
    fn rejects_span_as_same_money_unit() {
        let code = format!(
            r#"spec test
{MONEY}
rule bad: (10 eur...50 eur) as eur"#
        );
        expect_plan_error(&code, "convert");
    }

    #[test]
    fn rejects_span_as_other_money_unit() {
        let code = format!(
            r#"spec test
{MONEY}
rule bad: (10 eur...50 eur) as usd"#
        );
        expect_plan_error(&code, "convert");
    }
}

// =============================================================================
// RatioRange
// =============================================================================

mod ratio_range {
    use super::*;

    #[test]
    fn span_as_percent() {
        let code = r#"spec test
rule span: (10%...50%) as percent"#;
        assert_contains_parts(&eval_rule(code, "test", "span"), &["40", "%"]);
    }

    #[test]
    fn span_as_permille() {
        let code = r#"spec test
rule span: (100 permille...500 permille) as permille"#;
        assert_contains_parts(&eval_rule(code, "test", "span"), &["400", "%%"]);
    }

    #[test]
    fn rejects_span_as_duration_unit() {
        let code = format!(
            r#"spec test
{USES_SI}
rule bad: (10%...50%) as days"#
        );
        expect_plan_error(&code, "convert");
    }

    #[test]
    fn rejects_span_as_mass_unit() {
        let code = format!(
            r#"spec test
{USES_SI}
{WEIGHT}
rule bad: (10%...50%) as stone"#
        );
        expect_plan_error(&code, "convert");
    }
}

// =============================================================================
// CalendarRange — span `as` not supported
// =============================================================================

mod calendar_range_span {
    use super::*;

    #[test]
    fn rejects_span_as_duration_unit() {
        let code = format!(
            r#"spec test
{USES_SI}
rule bad: (18 years...67 years) as days"#
        );
        expect_plan_error(&code, "convert");
    }
}
