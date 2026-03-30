//! Regression test: type-only dependencies must respect temporal versioning.
//!
//! When spec B depends on spec A *only* via `type money from A` (no fact-level
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
    facts: Vec<(&str, &str)>,
) -> lemma::Response {
    let map: HashMap<String, String> = facts
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

/// Type-only dependency where the source spec's type definition changes between
/// temporal versions. The consumer has NO fact-level spec ref — only
/// `type money from finance`.
///
/// finance v1 (before 2025-07-01): money has only `eur`
/// finance v2 (from 2025-07-01):  money has `eur` + `usd`
///
/// consumer is active from 2025-01-01 and uses `eur` literals.
/// Both slices should plan successfully with the correct version of the type.
/// Crucially, evaluating *after* the boundary must see finance v2's type (which
/// includes `usd`), not finance v1's.
#[test]
fn type_only_dep_must_produce_temporal_slices() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
type money: scale
 -> unit eur 1.00
 -> decimals 2
fact base_price: 50.00 eur

spec finance 2025-07-01
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shop 2025-01-01
type money from finance
fact price: [money]
rule doubled: price * 2
"#,
            lemma::SourceType::Labeled("shop.lemma"),
        )
        .unwrap();

    // Before boundary: finance v1 type (eur only). 10 eur * 2 = 20.00 eur.
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

    // After boundary: finance v2 type (eur + usd). 10 eur * 2 = 20.00 eur.
    // This must use v2's type — if temporal slicing worked, this slice would
    // resolve finance at 2025-07-01 and get the eur+usd version.
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

    // The definitive check: after the boundary, `usd` must be a valid unit
    // because finance v2 defines it. If temporal slicing is broken, this will
    // fail with "unknown unit 'usd'" because the single slice resolved
    // finance v1 (which only has eur).
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 9, 1),
            vec![("price", "10.00 usd")],
        ),
        "doubled",
        "20.00 usd",
    );
}

/// Same scenario but the consumer spec is unranged (no effective_from).
/// The type-only dependency should still produce slices based on finance's
/// version boundary.
#[test]
fn unranged_spec_with_type_only_dep_must_slice() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec units
type weight: scale
 -> unit kg 1.00
 -> decimals 1

spec units 2025-06-01
type weight: scale
 -> unit kg 1.00
 -> unit lb 2.205
 -> decimals 1
"#,
            lemma::SourceType::Labeled("units.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec warehouse
type weight from units
fact item_weight: [weight]
rule heavy: item_weight > 100.0 kg
"#,
            lemma::SourceType::Labeled("warehouse.lemma"),
        )
        .unwrap();

    // Before boundary: only kg is available
    assert_rule_value(
        &eval_with(
            &engine,
            "warehouse",
            &date(2025, 3, 1),
            vec![("item_weight", "150.0 kg")],
        ),
        "heavy",
        "true",
    );

    // After boundary: lb must also be available (units v2 adds it).
    // If slicing is broken, this fails with "unknown unit 'lb'".
    assert_rule_value(
        &eval_with(
            &engine,
            "warehouse",
            &date(2025, 9, 1),
            vec![("item_weight", "250.0 lb")],
        ),
        "heavy",
        "true",
    );
}

/// The plan hashes for the consumer spec must differ across slices when the
/// type-only dependency changes, proving that separate plans were built.
#[test]
fn type_only_dep_produces_distinct_plan_hashes_per_slice() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec types_lib
type score: number -> minimum 0 -> maximum 100

spec types_lib 2025-06-01
type score: number -> minimum 0 -> maximum 1000
"#,
            lemma::SourceType::Labeled("types_lib.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec grader 2025-01-01
type score from types_lib
fact student_score: [score]
rule passed: student_score >= 50
"#,
            lemma::SourceType::Labeled("grader.lemma"),
        )
        .unwrap();

    let hash_before = engine
        .get_plan_hash("grader", &date(2025, 3, 1))
        .ok()
        .flatten()
        .expect("grader should have a plan hash before boundary");

    let hash_after = engine
        .get_plan_hash("grader", &date(2025, 9, 1))
        .ok()
        .flatten()
        .expect("grader should have a plan hash after boundary");

    assert_ne!(
        hash_before, hash_after,
        "plan hashes must differ across slices when the type-only dependency changes \
         (maximum changed from 100 to 1000). Got same hash: {}",
        hash_before
    );
}

/// Hash-pinned type import must NOT create slice boundaries.
/// The consumer should get exactly one plan hash regardless of dep versions.
#[test]
fn hash_pinned_type_import_does_not_create_slices() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
type money: scale
 -> unit eur 1.00
 -> decimals 2
fact base_price: 50.00 eur

spec finance 2025-07-01
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    let finance_hash = engine
        .get_plan_hash("finance", &date(2025, 3, 1))
        .ok()
        .flatten()
        .expect("finance should have a plan hash")
        .to_string();

    let consumer_src = format!(
        "spec shop 2025-01-01\ntype money from finance~{}\nfact price: [money]\nrule doubled: price * 2",
        finance_hash
    );
    engine
        .load(&consumer_src, lemma::SourceType::Labeled("shop.lemma"))
        .unwrap();

    let hash_before = engine
        .get_plan_hash("shop", &date(2025, 3, 1))
        .ok()
        .flatten()
        .expect("shop should have a plan hash before boundary");

    let hash_after = engine
        .get_plan_hash("shop", &date(2025, 9, 1))
        .ok()
        .flatten()
        .expect("shop should have a plan hash after boundary");

    assert_eq!(
        hash_before, hash_after,
        "hash-pinned type import must NOT create slice boundaries; hashes should be identical"
    );
}

/// Mixed scenario: consumer has both a fact-level spec ref and a type import
/// to the same dependency. Consumer needs separate temporal versions to satisfy
/// the cross-spec interface contract (dep's interface changes between slices).
#[test]
fn mixed_spec_ref_and_type_import_to_same_dep() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
type money: scale
 -> unit eur 1.00
 -> decimals 2
fact base_price: 50.00 eur

spec finance 2025-07-01
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shop 2025-01-01
type money from finance
fact ref: spec finance
fact price: [money]
rule total: ref.base_price + price

spec shop 2025-07-01
type money from finance
fact ref: spec finance
fact price: [money]
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

/// Inline type import (`fact price: [money from finance]`) must also produce
/// temporal slices when the source spec has multiple versions.
#[test]
fn inline_type_import_creates_temporal_slices() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
type money: scale
 -> unit eur 1.00
 -> decimals 2
fact base_price: 50.00 eur

spec finance 2025-07-01
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shop 2025-01-01
fact price: [money from finance]
rule doubled: price * 2
"#,
            lemma::SourceType::Labeled("shop.lemma"),
        )
        .unwrap();

    // Before boundary: finance v1 (eur only)
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

    // After boundary: finance v2, usd must be available
    assert_rule_value(
        &eval_with(
            &engine,
            "shop",
            &date(2025, 9, 1),
            vec![("price", "10.00 usd")],
        ),
        "doubled",
        "20.00 usd",
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
type money: scale
 -> unit eur 1.00
 -> decimals 2
fact base_price: 50.00 eur

spec finance 2025-07-01
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact base_price: 75.00 eur
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
type money from finance 2025-03-01
fact price: [money]
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

// ============================================================================
// Hash pin + effective datetime validation
// ============================================================================

fn load_finance(engine: &mut Engine) -> String {
    engine
        .load(
            r#"
spec finance
type money: scale
 -> unit eur 1.00
 -> decimals 2
fact base_price: 50.00 eur

spec finance 2025-07-01
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact base_price: 75.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    engine
        .get_plan_hash("finance", &date(2025, 3, 1))
        .ok()
        .flatten()
        .expect("finance v1 should have a plan hash")
        .to_string()
}

/// Hash-pinned type import with effective IN the pinned version's range succeeds.
#[test]
fn type_import_hash_pin_with_effective_in_range_succeeds() {
    let mut engine = Engine::new();
    let hash = load_finance(&mut engine);

    let consumer_src = format!(
        "spec shop 2025-01-01\ntype money from finance~{} 2025-03-01\nfact price: [money]\nrule doubled: price * 2",
        hash
    );
    engine
        .load(&consumer_src, lemma::SourceType::Labeled("shop.lemma"))
        .unwrap();

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
}

fn assert_load_fails_with(engine: &mut Engine, src: &str, expected_substr: &str) {
    let result = engine.load(src, lemma::SourceType::Labeled("shop.lemma"));
    assert!(
        result.is_err(),
        "should fail with error containing '{}'",
        expected_substr
    );
    let msgs: Vec<String> = result
        .unwrap_err()
        .errors
        .iter()
        .map(|e| format!("{:?}", e))
        .collect();
    let combined = msgs.join(" ");
    assert!(
        combined.contains(expected_substr),
        "expected error containing '{}', got: {}",
        expected_substr,
        combined
    );
}

/// Hash-pinned type import with effective OUTSIDE the pinned version's range is a hard error.
#[test]
fn type_import_hash_pin_with_effective_out_of_range_fails() {
    let mut engine = Engine::new();
    let hash = load_finance(&mut engine);

    let consumer_src = format!(
        "spec shop 2025-01-01\ntype money from finance~{} 2099-01-01\nfact price: [money]\nrule doubled: price * 2",
        hash
    );
    assert_load_fails_with(&mut engine, &consumer_src, "outside the temporal range");
}

/// Fact-level spec ref with hash pin + out-of-range effective is a hard error.
#[test]
fn fact_spec_ref_hash_pin_with_effective_out_of_range_fails() {
    let mut engine = Engine::new();
    let hash = load_finance(&mut engine);

    let consumer_src = format!(
        "spec shop 2025-01-01\nfact ref: spec finance~{} 2099-01-01\nrule x: ref.base_price",
        hash
    );
    assert_load_fails_with(&mut engine, &consumer_src, "outside the temporal range");
}

/// Inline type import with hash pin + out-of-range effective is a hard error.
#[test]
fn inline_type_import_hash_pin_with_effective_out_of_range_fails() {
    let mut engine = Engine::new();
    let hash = load_finance(&mut engine);

    let consumer_src = format!(
        "spec shop 2025-01-01\nfact price: [money from finance~{} 2099-01-01]\nrule doubled: price * 2",
        hash
    );
    assert_load_fails_with(&mut engine, &consumer_src, "outside the temporal range");
}
