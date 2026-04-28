//! Regression test: type-only dependencies must respect temporal versioning.
//!
//! When spec B depends on spec A *only* via `data money from A` (no data-level
//! spec ref), and A has multiple temporal versions with different type
//! definitions, B must produce separate temporal slices — one per version of A
//! that falls within B's effective range.

use lemma::{DateTimeValue, Engine};
use std::collections::HashMap;

fn date(year: i32, month: u32, day: u32) -> DateTimeValue {
    DateTimeValue {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
    }
}

fn eval_with(
    engine: &Engine,
    spec_name: &str,
    effective: &DateTimeValue,
    data: Vec<(&str, &str)>,
) -> lemma::Response {
    let map: HashMap<String, String> = data
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    engine.run(spec_name, Some(effective), map, false).unwrap()
}

fn assert_rule_value(response: &lemma::Response, rule: &str, expected: &str) {
    let result = response
        .results
        .get(rule)
        .unwrap_or_else(|| panic!("rule '{}' not in results", rule));
    let val = result
        .result
        .value()
        .unwrap_or_else(|| panic!("rule '{}' is Veto, expected Value", rule));
    assert_eq!(
        val.to_string(),
        expected,
        "rule '{}': expected {}, got {}",
        rule,
        expected,
        val
    );
}

/// Qualified type import: `from finance 2025-02-01` pins to finance v1 (eur only)
/// regardless of evaluation datetime. The pin freezes the type at that instant.
#[test]
fn qualified_type_import_pins_to_referenced_version() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
data money: scale
 -> unit eur 1.00
 -> decimals 2
data base_price: 50.00 eur

spec finance 2025-07-01
data money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
data base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shop 2025-01-01
data money from finance 2025-02-01
data price: money
rule doubled: price * 2
"#,
            lemma::SourceType::Labeled("shop.lemma"),
        )
        .unwrap();

    // Pin resolves finance v1 (eur only), works at any eval datetime.
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 3, 1),
            vec![("price", "10.00 eur")],
        ),
        "doubled",
        "20.00 eur",
    );

    // Even after the boundary, the pin keeps us on v1 — still eur only.
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 9, 1),
            vec![("price", "10.00 eur")],
        ),
        "doubled",
        "20.00 eur",
    );
}

/// Qualified type import `from finance 2025-02-01` pins to finance v1 (eur only).
/// Using a unit from v2 (usd) must produce a validation error even after the
/// v2 boundary, because the pin freezes the type at the qualified instant.
#[test]
fn qualified_type_import_rejects_unit_from_later_version() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
data money: scale
 -> unit eur 1.00
 -> decimals 2

spec finance 2025-07-01
data money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shop 2025-01-01
data money from finance 2025-02-01
data price: money
rule doubled: price * 2
"#,
            lemma::SourceType::Labeled("shop.lemma"),
        )
        .unwrap();

    // eur works: finance v1 has eur
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 3, 1),
            vec![("price", "10.00 eur")],
        ),
        "doubled",
        "20.00 eur",
    );

    // usd must fail: pin locks to finance v1 which only has eur
    let result = engine.run(
        "shop",
        Some(&date(2025, 9, 1)),
        vec![("price".to_string(), "10.00 usd".to_string())]
            .into_iter()
            .collect(),
        false,
    );
    assert!(result.is_err(), "usd should be rejected by pinned v1 type");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Unknown unit") && err.contains("usd"),
        "error should mention unknown unit 'usd', got: {err}"
    );
}

/// Unranged consumer with a type-only dep whose interface changes between
/// temporal slices must be rejected when the reference is not pinned.
#[test]
fn unranged_spec_with_type_only_dep_rejects_incompatible_interface() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec units
data weight: scale
 -> unit kg 1.00
 -> decimals 1

spec units 2025-06-01
data weight: scale
 -> unit kg 1.00
 -> unit lb 2.205
 -> decimals 1
"#,
            lemma::SourceType::Labeled("units.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec warehouse
data weight from units
data item_weight: weight
rule heavy: item_weight > 100.0 kg
"#,
        lemma::SourceType::Labeled("warehouse.lemma"),
    );

    assert!(
        result.is_err(),
        "Unpinned type-only dep with incompatible interfaces must be rejected"
    );
    let errs = result.unwrap_err();
    let combined: String = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        combined.contains("changed its interface"),
        "Error should mention interface change, got: {combined}"
    );
}

/// Mixed scenario: consumer has both a data-level spec ref and a type import
/// to the same dependency. Consumer needs separate temporal versions to satisfy
/// the cross-spec interface contract (dep's interface changes between slices).
#[test]
fn mixed_spec_ref_and_type_import_to_same_dep() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
data money: scale
 -> unit eur 1.00
 -> decimals 2
data base_price: 50.00 eur

spec finance 2025-07-01
data money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
data base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shop 2025-01-01
data money from finance
with ref: finance
data price: money
rule total: ref.base_price + price

spec shop 2025-07-01
data money from finance
with ref: finance
data price: money
rule total: ref.base_price + price
"#,
            lemma::SourceType::Labeled("shop.lemma"),
        )
        .unwrap();

    // Before boundary: finance v1, base_price=50, only eur
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 3, 1),
            vec![("price", "10.00 eur")],
        ),
        "total",
        "60.00 eur",
    );

    // After boundary: finance v2, base_price=75, eur+usd available
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 9, 1),
            vec![("price", "10.00 eur")],
        ),
        "total",
        "85.00 eur",
    );

    // After boundary: usd must be available from the type import
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 9, 1),
            vec![("price", "10.00 usd")],
        ),
        "total",
        "84.09 eur",
    );
}

/// Inline type import (`data price: money from finance`) without pinning
/// must be rejected when the source spec's interface changes across versions.
#[test]
fn inline_type_import_rejects_incompatible_unpinned_dep() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
data money: scale
 -> unit eur 1.00
 -> decimals 2
data base_price: 50.00 eur

spec finance 2025-07-01
data money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
data base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec shop 2025-01-01
data price: money from finance
rule doubled: price * 2
"#,
        lemma::SourceType::Labeled("shop.lemma"),
    );

    assert!(
        result.is_err(),
        "Unpinned inline type import with incompatible dep interfaces must be rejected"
    );
    let errs = result.unwrap_err();
    let combined: String = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        combined.contains("changed its interface"),
        "Error should mention interface change, got: {combined}"
    );
}

/// Type import with explicit effective datetime pins resolution to that version.
#[test]
fn type_import_with_effective_datetime_pins_version() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
data money: scale
 -> unit eur 1.00
 -> decimals 2
data base_price: 50.00 eur

spec finance 2025-07-01
data money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
data base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    // Pin the type import to a date before the v2 boundary;
    // even when evaluated after 2025-07-01, only eur should be available
    engine
        .load(
            r#"
spec shop 2025-01-01
data money from finance 2025-03-01
data price: money
rule doubled: price * 2
"#,
            lemma::SourceType::Labeled("shop.lemma"),
        )
        .unwrap();

    // Evaluate after the finance boundary — but the type import is pinned
    // to 2025-03-01 which resolves finance v1 (eur only).
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 9, 1),
            vec![("price", "10.00 eur")],
        ),
        "doubled",
        "20.00 eur",
    );
}

/// Regression: qualified pin to early dep version must NOT silently bind to a
/// later body. Two finance versions with incompatible types (v1=eur only,
/// v2=eur+usd). Consumer pins to v1 via `from finance 2025-02-01`.
/// Evaluating with `usd` in a later slice must fail (v1 has no usd).
#[test]
fn qualified_pin_must_not_leak_later_version_types() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
data money: scale
 -> unit eur 1.00
 -> decimals 2

spec finance 2025-07-01
data money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shop 2025-01-01
data money from finance 2025-02-01
data price: money
rule doubled: price * 2
"#,
            lemma::SourceType::Labeled("shop.lemma"),
        )
        .unwrap();

    // Evaluate after boundary with usd — must error because pin locks to v1 (no usd).
    let result = engine.run(
        "shop",
        Some(&date(2025, 9, 1)),
        [("price".to_string(), "10.00 usd".to_string())]
            .into_iter()
            .collect(),
        false,
    );
    match &result {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("usd") || msg.contains("unit") || msg.contains("unknown"),
                "error should mention the rejected unit, got: {msg}"
            );
        }
        Ok(resp) => {
            // If the engine returns Ok, every rule result must NOT have a successful
            // value using usd — that would mean the pin leaked v2's types.
            for (rule, r) in &resp.results {
                if let Some(val) = r.result.value() {
                    let s = val.to_string();
                    assert!(
                        !s.contains("usd"),
                        "rule '{rule}' produced {s} — usd must not be accepted when pinned to finance v1"
                    );
                }
            }
        }
    }
}
