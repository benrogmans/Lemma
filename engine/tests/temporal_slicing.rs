//! Integration tests for temporal slicing of execution plans.
//!
//! When a spec's active range spans dependency version boundaries,
//! planning must produce one ExecutionPlan per temporal slice and each
//! slice must independently validate.
//!
//! These tests define the target behavior. Many will fail until the
//! temporal slicing implementation is complete.

use lemma::planning::semantics::FactData;
use lemma::{DateTimeValue, Engine, Error};
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

fn eval(engine: &Engine, spec_name: &str, effective: &DateTimeValue) -> lemma::Response {
    engine
        .run(spec_name, Some(effective), HashMap::new(), false)
        .unwrap()
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

// ============================================================================
// 1. SINGLE DEPENDENCY — NO VERSIONING
// ============================================================================

#[test]
fn single_unversioned_dependency() {
    let mut engine = Engine::new();

    engine
        .load(
            "spec config\nfact base_rate: 100",
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec pricing 2025-01-01
fact cfg: spec config
rule rate: cfg.base_rate * 2
"#,
            lemma::SourceType::Labeled("pricing.lemma"),
        )
        .unwrap();

    for d in [date(2025, 1, 15), date(2025, 7, 1), date(2025, 12, 15)] {
        assert_rule_value(&eval(&engine, "pricing", &d), "rate", "200");
    }
}

// ============================================================================
// 2. SINGLE DEPENDENCY — ONE VERSION BOUNDARY
// ============================================================================

#[test]
fn one_boundary_produces_two_slices() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec config
fact base_rate: 100

spec config 2025-04-01
fact base_rate: 200
"#,
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec pricing 2025-01-01
fact cfg: spec config
rule rate: cfg.base_rate
"#,
            lemma::SourceType::Labeled("pricing.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "pricing", &date(2025, 2, 1)), "rate", "100");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 3, 31)), "rate", "100");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 4, 1)), "rate", "200");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 6, 15)), "rate", "200");
}

#[test]
fn boundary_exactly_at_spec_effective_from_no_split() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec config
fact rate: 50

spec config 2025-01-01
fact rate: 75
"#,
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec pricing 2025-01-01
fact cfg: spec config
rule rate: cfg.rate
"#,
            lemma::SourceType::Labeled("pricing.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "pricing", &date(2025, 1, 1)), "rate", "75");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 5, 1)), "rate", "75");
}

// ============================================================================
// 3. SINGLE DEPENDENCY — MULTIPLE VERSION BOUNDARIES
// ============================================================================

#[test]
fn three_versions_produce_three_slices() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec rates
fact rate: 10

spec rates 2025-03-01
fact rate: 20

spec rates 2025-07-01
fact rate: 30
"#,
            lemma::SourceType::Labeled("rates.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec pricing 2025-01-01
fact r: spec rates
fact quantity: [number]
rule total: quantity * r.rate
"#,
            lemma::SourceType::Labeled("pricing.lemma"),
        )
        .unwrap();

    let cases = [
        (date(2025, 2, 1), "100"), // v1: 10*10
        (date(2025, 5, 1), "200"), // v2: 10*20
        (date(2025, 9, 1), "300"), // v3: 10*30
    ];
    for (d, expected) in &cases {
        assert_rule_value(
            &eval_with(&engine, "pricing", d, vec![("quantity", "10")]),
            "total",
            expected,
        );
    }
}

#[test]
fn four_versions_only_two_boundaries_inside_range() {
    let mut engine = Engine::new();

    // v1: [-∞, Mar), v2: [Mar, Jun), v3: [Jun, Sep), v4: [Sep, +∞)
    engine
        .load(
            r#"
spec rates
fact rate: 10

spec rates 2025-03-01
fact rate: 20

spec rates 2025-06-01
fact rate: 30

spec rates 2025-09-01
fact rate: 40
"#,
            lemma::SourceType::Labeled("rates.lemma"),
        )
        .unwrap();

    // pricing active [Apr, +∞) → Jun and Sep boundaries are inside
    engine
        .load(
            r#"
spec pricing 2025-04-01
fact r: spec rates
rule rate: r.rate
"#,
            lemma::SourceType::Labeled("pricing.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "pricing", &date(2025, 4, 15)), "rate", "20");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 5, 31)), "rate", "20");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 6, 1)), "rate", "30");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 7, 15)), "rate", "30");
}

// ============================================================================
// 4. MULTIPLE DEPENDENCIES — INDEPENDENT BOUNDARIES
// ============================================================================

#[test]
fn two_deps_boundaries_at_different_times() {
    let mut engine = Engine::new();

    // tax_rates: boundary at April
    engine
        .load(
            r#"
spec tax_rates
fact vat: 19

spec tax_rates 2025-04-01
fact vat: 21
"#,
            lemma::SourceType::Labeled("tax_rates.lemma"),
        )
        .unwrap();

    // shipping_rates: boundary at July
    engine
        .load(
            r#"
spec shipping_rates
fact fee: 5

spec shipping_rates 2025-07-01
fact fee: 8
"#,
            lemma::SourceType::Labeled("shipping_rates.lemma"),
        )
        .unwrap();

    // invoice: depends on both → boundaries at {April, July} → three slices
    engine
        .load(
            r#"
spec invoice 2025-01-01
fact tax: spec tax_rates
fact shipping: spec shipping_rates
fact price: [number]
rule vat_amount: price * tax.vat / 100
rule shipping_fee: shipping.fee
rule total: price + vat_amount + shipping_fee
"#,
            lemma::SourceType::Labeled("invoice.lemma"),
        )
        .unwrap();

    // Slice 1: [Jan, Apr) — vat=19, fee=5
    let r = eval_with(
        &engine,
        "invoice",
        &date(2025, 2, 1),
        vec![("price", "100")],
    );
    assert_rule_value(&r, "vat_amount", "19");
    assert_rule_value(&r, "shipping_fee", "5");
    assert_rule_value(&r, "total", "124");

    // Slice 2: [Apr, Jul) — vat=21, fee=5
    let r = eval_with(
        &engine,
        "invoice",
        &date(2025, 5, 1),
        vec![("price", "100")],
    );
    assert_rule_value(&r, "vat_amount", "21");
    assert_rule_value(&r, "shipping_fee", "5");
    assert_rule_value(&r, "total", "126");

    // Slice 3: [Jul, +∞) — vat=21, fee=8
    let r = eval_with(
        &engine,
        "invoice",
        &date(2025, 9, 1),
        vec![("price", "100")],
    );
    assert_rule_value(&r, "vat_amount", "21");
    assert_rule_value(&r, "shipping_fee", "8");
    assert_rule_value(&r, "total", "129");
}

#[test]
fn two_deps_boundaries_at_same_time() {
    let mut engine = Engine::new();

    // Both deps change on April 1
    engine
        .load(
            r#"
spec tax_rates
fact vat: 19

spec tax_rates 2025-04-01
fact vat: 21
"#,
            lemma::SourceType::Labeled("tax_rates.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shipping_rates
fact fee: 5

spec shipping_rates 2025-04-01
fact fee: 8
"#,
            lemma::SourceType::Labeled("shipping_rates.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec invoice 2025-01-01
fact tax: spec tax_rates
fact shipping: spec shipping_rates
rule combined: tax.vat + shipping.fee
"#,
            lemma::SourceType::Labeled("invoice.lemma"),
        )
        .unwrap();

    // Coincident boundary → two slices, not three
    assert_rule_value(
        &eval(&engine, "invoice", &date(2025, 2, 1)),
        "combined",
        "24",
    );
    assert_rule_value(
        &eval(&engine, "invoice", &date(2025, 6, 1)),
        "combined",
        "29",
    );
}

#[test]
fn one_dep_versioned_one_dep_unversioned() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec constants
fact pi: 3
"#,
            lemma::SourceType::Labeled("constants.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec rates
fact multiplier: 2

spec rates 2025-06-01
fact multiplier: 4
"#,
            lemma::SourceType::Labeled("rates.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec calc 2025-01-01
fact c: spec constants
fact r: spec rates
rule result: c.pi * r.multiplier
"#,
            lemma::SourceType::Labeled("calc.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "calc", &date(2025, 3, 1)), "result", "6");
    assert_rule_value(&eval(&engine, "calc", &date(2025, 9, 1)), "result", "12");
}

// ============================================================================
// 5. TRANSITIVE DEPENDENCIES
// ============================================================================

#[test]
fn transitive_two_levels_deep() {
    let mut engine = Engine::new();

    // SpecC: two versions
    engine
        .load(
            r#"
spec base_rates
fact multiplier: 2

spec base_rates 2025-06-01
fact multiplier: 3
"#,
            lemma::SourceType::Labeled("base_rates.lemma"),
        )
        .unwrap();

    // SpecB: unversioned, depends on SpecC
    engine
        .load(
            r#"
spec intermediate
fact base: spec base_rates
fact value: 10
rule adjusted: value * base.multiplier
"#,
            lemma::SourceType::Labeled("intermediate.lemma"),
        )
        .unwrap();

    // SpecA: depends on SpecB → transitively on SpecC
    engine
        .load(
            r#"
spec top 2025-01-01
fact mid: spec intermediate
rule result: mid.adjusted
"#,
            lemma::SourceType::Labeled("top.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "top", &date(2025, 3, 1)), "result", "20");
    assert_rule_value(&eval(&engine, "top", &date(2025, 9, 1)), "result", "30");
}

#[test]
fn transitive_both_levels_versioned() {
    let mut engine = Engine::new();

    // SpecC: boundary at June
    engine
        .load(
            r#"
spec deep
fact factor: 2

spec deep 2025-06-01
fact factor: 5
"#,
            lemma::SourceType::Labeled("deep.lemma"),
        )
        .unwrap();

    // SpecB: boundary at April, depends on SpecC
    engine
        .load(
            r#"
spec middle
fact d: spec deep
fact base: 10
rule value: base * d.factor

spec middle 2025-04-01
fact d: spec deep
fact base: 100
rule value: base * d.factor
"#,
            lemma::SourceType::Labeled("middle.lemma"),
        )
        .unwrap();

    // SpecA: active from Jan → boundaries at {April, June} → three slices
    engine
        .load(
            r#"
spec top 2025-01-01
fact m: spec middle
rule result: m.value
"#,
            lemma::SourceType::Labeled("top.lemma"),
        )
        .unwrap();

    // [Jan, Apr): middle v1 (base=10) * deep v1 (factor=2) = 20
    assert_rule_value(&eval(&engine, "top", &date(2025, 2, 1)), "result", "20");
    // [Apr, Jun): middle v2 (base=100) * deep v1 (factor=2) = 200
    assert_rule_value(&eval(&engine, "top", &date(2025, 5, 1)), "result", "200");
    // [Jun, +∞): middle v2 (base=100) * deep v2 (factor=5) = 500
    assert_rule_value(&eval(&engine, "top", &date(2025, 9, 1)), "result", "500");
}

#[test]
fn diamond_dependency_single_boundary() {
    let mut engine = Engine::new();

    // Shared dep at the bottom
    engine
        .load(
            r#"
spec shared
fact value: 10

spec shared 2025-06-01
fact value: 20
"#,
            lemma::SourceType::Labeled("shared.lemma"),
        )
        .unwrap();

    // Two intermediate specs both depend on shared
    engine
        .load(
            r#"
spec left_branch
fact s: spec shared
rule doubled: s.value * 2
"#,
            lemma::SourceType::Labeled("left.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec right_branch
fact s: spec shared
rule tripled: s.value * 3
"#,
            lemma::SourceType::Labeled("right.lemma"),
        )
        .unwrap();

    // Top depends on both branches (diamond through shared)
    engine
        .load(
            r#"
spec top 2025-01-01
fact l: spec left_branch
fact r: spec right_branch
rule total: l.doubled + r.tripled
"#,
            lemma::SourceType::Labeled("top.lemma"),
        )
        .unwrap();

    // Before June: 10*2 + 10*3 = 50
    assert_rule_value(&eval(&engine, "top", &date(2025, 3, 1)), "total", "50");
    // After June: 20*2 + 20*3 = 100
    assert_rule_value(&eval(&engine, "top", &date(2025, 9, 1)), "total", "100");
}

#[test]
fn diamond_dependency_boundaries_at_different_levels() {
    let mut engine = Engine::new();

    // Shared base: boundary at August
    engine
        .load(
            r#"
spec shared
fact base: 10

spec shared 2025-08-01
fact base: 50
"#,
            lemma::SourceType::Labeled("shared.lemma"),
        )
        .unwrap();

    // Left branch: boundary at April
    engine
        .load(
            r#"
spec left
fact s: spec shared
fact add: 1
rule result: s.base + add

spec left 2025-04-01
fact s: spec shared
fact add: 2
rule result: s.base + add
"#,
            lemma::SourceType::Labeled("left.lemma"),
        )
        .unwrap();

    // Right branch: unversioned
    engine
        .load(
            r#"
spec right
fact s: spec shared
rule result: s.base * 2
"#,
            lemma::SourceType::Labeled("right.lemma"),
        )
        .unwrap();

    // Top: active from Jan → boundaries at {April, August} → three slices
    engine
        .load(
            r#"
spec top 2025-01-01
fact l: spec left
fact r: spec right
rule total: l.result + r.result
"#,
            lemma::SourceType::Labeled("top.lemma"),
        )
        .unwrap();

    // [Jan, Apr): left v1 (10+1=11) + right (10*2=20) = 31
    assert_rule_value(&eval(&engine, "top", &date(2025, 2, 1)), "total", "31");
    // [Apr, Aug): left v2 (10+2=12) + right (10*2=20) = 32
    assert_rule_value(&eval(&engine, "top", &date(2025, 5, 1)), "total", "32");
    // [Aug, +∞): left v2 (50+2=52) + right (50*2=100) = 152
    assert_rule_value(&eval(&engine, "top", &date(2025, 9, 1)), "total", "152");
}

// ============================================================================
// 6. UNRANGED DOC (−∞ to +∞) WITH VERSIONED DEPENDENCY
// ============================================================================

#[test]
fn unranged_spec_sliced_by_versioned_dep() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec rates
fact tax: 19

spec rates 2026-01-01
fact tax: 21
"#,
            lemma::SourceType::Labeled("rates.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec calculator
fact r: spec rates
fact income: [number]
rule tax_amount: income * r.tax / 100
"#,
            lemma::SourceType::Labeled("calculator.lemma"),
        )
        .unwrap();

    assert_rule_value(
        &eval_with(
            &engine,
            "calculator",
            &date(2025, 6, 1),
            vec![("income", "1000")],
        ),
        "tax_amount",
        "190",
    );
    assert_rule_value(
        &eval_with(
            &engine,
            "calculator",
            &date(2026, 6, 1),
            vec![("income", "1000")],
        ),
        "tax_amount",
        "210",
    );
}

// ============================================================================
// 7. DEPENDENCY COVERAGE GAPS — PLANNING ERRORS
// ============================================================================

#[test]
fn dependency_not_yet_active_at_spec_start() {
    let mut engine = Engine::new();

    // config only starts in June
    engine
        .load(
            r#"
spec config 2025-06-01
fact rate: 100
"#,
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec pricing 2025-01-01
fact cfg: spec config
rule rate: cfg.rate
"#,
        lemma::SourceType::Labeled("pricing.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: config not active [Jan, Jun) but pricing needs it from January"
    );
}

#[test]
fn unbounded_spec_depending_on_bounded_dep_rejected() {
    let mut engine = Engine::new();

    // dep only active from June onward
    engine
        .load(
            r#"
spec regulations 2025-06-01
fact max_amount: 500
"#,
            lemma::SourceType::Labeled("regulations.lemma"),
        )
        .unwrap();

    // unbounded spec [-∞, +∞) depends on dep [Jun, +∞) → planning error
    // dep's coverage is narrower than the spec's range
    let result = engine.load(
        r#"
spec contract
fact reg: spec regulations
fact amount: [number]
rule is_valid: amount <= reg.max_amount
"#,
        lemma::SourceType::Labeled("contract.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: unbounded spec can't depend on a dep that doesn't cover [-∞, +∞)"
    );
}

#[test]
fn three_versions_seamlessly_chained() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec policy 2025-01-01
fact limit: 1000

spec policy 2025-04-01
fact limit: 2000

spec policy 2025-08-01
fact limit: 3000
"#,
            lemma::SourceType::Labeled("policy.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec contract 2025-01-01
fact p: spec policy
fact amount: [number]
rule under_limit: amount < p.limit
"#,
            lemma::SourceType::Labeled("contract.lemma"),
        )
        .unwrap();

    // 1500 < 1000 = false, 1500 < 2000 = true, 1500 < 3000 = true
    assert_rule_value(
        &eval_with(
            &engine,
            "contract",
            &date(2025, 2, 1),
            vec![("amount", "1500")],
        ),
        "under_limit",
        "false",
    );
    assert_rule_value(
        &eval_with(
            &engine,
            "contract",
            &date(2025, 5, 1),
            vec![("amount", "1500")],
        ),
        "under_limit",
        "true",
    );
    assert_rule_value(
        &eval_with(
            &engine,
            "contract",
            &date(2025, 9, 1),
            vec![("amount", "1500")],
        ),
        "under_limit",
        "true",
    );
}

// ============================================================================
// 8. PINNED DOC REFS (hash pin) — NO SLICING
// ============================================================================

#[test]
fn hash_pinned_ref_resolves_correct_version() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec config
fact rate: 100

spec config 2025-04-01
fact rate: 999
"#,
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    // Get the hash of the first (unversioned) config spec (active before 2025-04-01)
    let v1_effective = date(2025, 1, 1);
    let v1_hash = engine
        .get_plan_hash("config", &v1_effective)
        .ok()
        .flatten()
        .expect("should have hash for config v1")
        .to_string();

    // Use the hash pin to always resolve config v1
    let pricing_src = format!(
        "spec pricing 2025-01-01\nfact cfg: spec config~{}\nrule rate: cfg.rate",
        v1_hash
    );
    engine
        .load(&pricing_src, lemma::SourceType::Labeled("pricing.lemma"))
        .unwrap();

    assert_rule_value(&eval(&engine, "pricing", &date(2025, 2, 1)), "rate", "100");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 9, 1)), "rate", "100");
}

#[test]
fn hash_pinned_ref_wrong_hash_fails_planning() {
    let mut engine = Engine::new();

    engine
        .load(
            "spec config\nfact rate: 100",
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    let result = engine.load(
        "spec consumer\nfact cfg: spec config~deadbeef\nrule r: cfg.rate",
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    assert!(result.is_err(), "wrong hash pin should fail planning");
    let err_str = result
        .unwrap_err()
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        err_str.contains("config"),
        "error should mention the spec name: {}",
        err_str
    );
}

#[test]
fn hash_pinned_type_import_resolves() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact base_price: 50.00 eur
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    let finance_hash = engine
        .get_plan_hash("finance", &date(2025, 1, 1))
        .ok()
        .flatten()
        .expect("should have finance hash")
        .to_string();

    let consumer_src = format!(
        "spec consumer\ntype money from finance~{}\nfact price: 100.00 eur\nrule double: price * 2",
        finance_hash
    );
    engine
        .load(&consumer_src, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();

    assert_rule_value(
        &eval(&engine, "consumer", &date(2025, 1, 1)),
        "double",
        "200.00 eur",
    );
}

// ============================================================================
// 9. SAME DOC EVALUATED AT DIFFERENT TIMES — CORRECT SLICE SELECTED
// ============================================================================

#[test]
fn evaluate_at_boundary_instant_uses_new_version() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec dep
fact val: 1

spec dep 2025-06-01
fact val: 2
"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec main
fact d: spec dep
rule result: d.val
"#,
            lemma::SourceType::Labeled("main.lemma"),
        )
        .unwrap();

    // Exactly at the boundary: effective_from is inclusive
    assert_rule_value(&eval(&engine, "main", &date(2025, 5, 31)), "result", "1");
    assert_rule_value(&eval(&engine, "main", &date(2025, 6, 1)), "result", "2");
}

// ============================================================================
// 10. DEPENDENT SPEC (SpecC) REFERENCES VERSIONED SPEC (SpecB)
// ============================================================================

#[test]
fn third_level_spec_depends_on_versioned_spec() {
    let mut engine = Engine::new();

    // SpecB: versioned
    engine
        .load(
            r#"
spec rates
fact base: 100

spec rates 2025-05-01
fact base: 200
"#,
            lemma::SourceType::Labeled("rates.lemma"),
        )
        .unwrap();

    // SpecA: unversioned, depends on SpecB
    engine
        .load(
            r#"
spec policy
fact r: spec rates
rule threshold: r.base * 2
"#,
            lemma::SourceType::Labeled("policy.lemma"),
        )
        .unwrap();

    // SpecC: depends on SpecA (which transitively depends on SpecB)
    engine
        .load(
            r#"
spec contract 2025-01-01
fact p: spec policy
fact amount: [number]
rule is_over_threshold: amount > p.threshold
"#,
            lemma::SourceType::Labeled("contract.lemma"),
        )
        .unwrap();

    // Before May: threshold = 100*2 = 200. amount=250 > 200 → true
    assert_rule_value(
        &eval_with(
            &engine,
            "contract",
            &date(2025, 3, 1),
            vec![("amount", "250")],
        ),
        "is_over_threshold",
        "true",
    );
    // After May: threshold = 200*2 = 400. amount=250 > 400 → false
    assert_rule_value(
        &eval_with(
            &engine,
            "contract",
            &date(2025, 9, 1),
            vec![("amount", "250")],
        ),
        "is_over_threshold",
        "false",
    );
}

// ============================================================================
// 11. COMPLEX SCENARIO — REALISTIC MULTI-DOC WITH MULTIPLE BOUNDARIES
// ============================================================================

#[test]
fn realistic_tax_and_labor_law_scenario() {
    let mut engine = Engine::new();

    // Tax rates: change April 1
    engine
        .load(
            r#"
spec tax_law
fact income_tax_rate: 30

spec tax_law 2025-04-01
fact income_tax_rate: 32
"#,
            lemma::SourceType::Labeled("tax.lemma"),
        )
        .unwrap();

    // Labor law: change July 1
    engine
        .load(
            r#"
spec labor_law
fact min_wage_hourly: 12
fact max_weekly_hours: 40

spec labor_law 2025-07-01
fact min_wage_hourly: 15
fact max_weekly_hours: 38
"#,
            lemma::SourceType::Labeled("labor.lemma"),
        )
        .unwrap();

    // Employment contract: depends on both, active from Jan
    engine
        .load(
            r#"
spec employment 2025-01-01
fact tax: spec tax_law
fact labor: spec labor_law
fact hourly_rate: [number]
fact weekly_hours: [number]

rule annual_gross: hourly_rate * weekly_hours * 52
rule annual_tax: annual_gross * tax.income_tax_rate / 100
rule annual_net: annual_gross - annual_tax
rule min_annual_gross: labor.min_wage_hourly * labor.max_weekly_hours * 52
rule meets_minimum: annual_gross >= min_annual_gross
"#,
            lemma::SourceType::Labeled("employment.lemma"),
        )
        .unwrap();

    let facts = vec![("hourly_rate", "20"), ("weekly_hours", "40")];

    // Slice [Jan, Apr): tax=30%, labor v1 (min=12, max_h=40)
    let r = eval_with(&engine, "employment", &date(2025, 2, 1), facts.clone());
    assert_rule_value(&r, "annual_gross", "41600");
    assert_rule_value(&r, "annual_tax", "12480");
    assert_rule_value(&r, "annual_net", "29120");
    assert_rule_value(&r, "min_annual_gross", "24960");
    assert_rule_value(&r, "meets_minimum", "true");

    // Slice [Apr, Jul): tax=32%, labor v1 (min=12, max_h=40)
    let r = eval_with(&engine, "employment", &date(2025, 5, 1), facts.clone());
    assert_rule_value(&r, "annual_tax", "13312");
    assert_rule_value(&r, "annual_net", "28288");
    assert_rule_value(&r, "min_annual_gross", "24960");

    // Slice [Jul, +∞): tax=32%, labor v2 (min=15, max_h=38)
    let r = eval_with(&engine, "employment", &date(2025, 9, 1), facts.clone());
    assert_rule_value(&r, "annual_tax", "13312");
    assert_rule_value(&r, "min_annual_gross", "29640");
    assert_rule_value(&r, "meets_minimum", "true");
}

// ============================================================================
// 12. SELF-VERSIONED DOC REFERENCING VERSIONED DEP
// ============================================================================

#[test]
fn both_spec_and_dep_are_versioned() {
    let mut engine = Engine::new();

    // dep: boundary at June
    engine
        .load(
            r#"
spec dep
fact val: 10

spec dep 2025-06-01
fact val: 20
"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();

    // main v1: [Jan, Apr), main v2: [Apr, +∞)
    // main v1's range [Jan, Apr) has no dep boundary → single slice for v1
    // main v2's range [Apr, +∞) has dep boundary at June → two slices for v2
    engine
        .load(
            r#"
spec main 2025-01-01
fact d: spec dep
fact multiplier: 2
rule result: d.val * multiplier

spec main 2025-04-01
fact d: spec dep
fact multiplier: 3
rule result: d.val * multiplier
"#,
            lemma::SourceType::Labeled("main.lemma"),
        )
        .unwrap();

    // main v1 [Jan, Apr): dep v1 (10) * 2 = 20
    assert_rule_value(&eval(&engine, "main", &date(2025, 2, 1)), "result", "20");
    // main v2 [Apr, Jun): dep v1 (10) * 3 = 30
    assert_rule_value(&eval(&engine, "main", &date(2025, 5, 1)), "result", "30");
    // main v2 [Jun, +∞): dep v2 (20) * 3 = 60
    assert_rule_value(&eval(&engine, "main", &date(2025, 9, 1)), "result", "60");
}

// ============================================================================
// 13. INTERFACE CONSISTENCY — TEMPORAL VALIDATION
// ============================================================================
// Different versions of a dependency CAN have different interfaces.
// Every slice must resolve to a version that satisfies what the
// dependent spec actually references (per-slice interface validation).

#[test]
fn dep_version_removes_referenced_fact_rejected() {
    let mut engine = Engine::new();

    // config v1 has base_rate. config v2 renames it to cost.
    engine
        .load(
            r#"
spec config
fact base_rate: 100

spec config 2025-04-01
fact cost: 200
"#,
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    // pricing references config.base_rate → v2 slice fails (no base_rate in v2)
    let result = engine.load(
        r#"
spec pricing 2025-01-01
fact cfg: spec config
rule rate: cfg.base_rate
"#,
        lemma::SourceType::Labeled("pricing.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: config v2 (April+) removed 'base_rate' that pricing references"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter().any(|e| e.to_string().contains("base_rate")),
        "Error should mention missing 'base_rate'. Got: {}",
        errs.iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
fn dep_version_removes_referenced_rule_rejected() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec policy
fact threshold: 100
rule discount: 10

spec policy 2025-06-01
fact threshold: 200
"#,
            lemma::SourceType::Labeled("policy.lemma"),
        )
        .unwrap();

    // contract references policy.discount → v2 slice fails (no discount rule in v2)
    let result = engine.load(
        r#"
spec contract 2025-01-01
fact p: spec policy
rule applied_discount: p.discount
"#,
        lemma::SourceType::Labeled("contract.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: policy v2 (June+) removed 'discount' rule that contract references"
    );
    let errs = result.unwrap_err();
    assert!(
        !errs.errors.is_empty(),
        "expected at least one planning error (policy v2 missing discount rule)"
    );
}

#[test]
fn dep_version_changes_fact_type_rejected() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec config
fact rate: 100

spec config 2025-04-01
fact rate: "high"
"#,
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    // pricing does arithmetic with config.rate → v2 slice fails (rate is text, not number)
    let result = engine.load(
        r#"
spec pricing 2025-01-01
fact cfg: spec config
rule total: cfg.rate * 2
"#,
        lemma::SourceType::Labeled("pricing.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: config v2 changed 'rate' from number to text, but pricing does arithmetic"
    );
}

#[test]
fn dep_version_changes_rule_type_rejected() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec policy
fact base: 100
rule discount: 10

spec policy 2025-04-01
fact base: 200
rule discount: "fixed"
"#,
            lemma::SourceType::Labeled("policy.lemma"),
        )
        .unwrap();

    // contract does arithmetic with p.discount → v2 slice fails (discount is text, not number)
    let result = engine.load(
        r#"
spec contract 2025-01-01
fact p: spec policy
rule total: p.base - p.discount
"#,
        lemma::SourceType::Labeled("contract.lemma"),
    );

    let err = match result {
        Err(e) => e,
        Ok(()) => panic!(
            "Must reject: policy v2 changed rule 'discount' from number to text, but contract does arithmetic"
        ),
    };
    let planning_msg = err
        .iter()
        .find_map(|e| {
            if let Error::Validation(details) = e {
                Some(details.message.as_str())
            } else {
                None
            }
        })
        .expect("expected at least one Validation error in result");
    assert!(
        planning_msg.contains("number and text")
            || planning_msg.contains("result type")
            || planning_msg.contains("Cannot apply"),
        "Error should report type mismatch. Got: {}",
        planning_msg
    );
}

#[test]
fn dep_versions_with_compatible_interface_accepted() {
    let mut engine = Engine::new();

    // Both versions have base_rate (same name, same kind), just different values
    // v2 also adds a new fact — that's fine, pricing doesn't reference it
    engine
        .load(
            r#"
spec config
fact base_rate: 100

spec config 2025-04-01
fact base_rate: 200
fact extra_field: 999
"#,
            lemma::SourceType::Labeled("config.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec pricing 2025-01-01
fact cfg: spec config
rule rate: cfg.base_rate
"#,
            lemma::SourceType::Labeled("pricing.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "pricing", &date(2025, 2, 1)), "rate", "100");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 5, 1)), "rate", "200");
}

#[test]
fn dep_version_adds_rule_that_other_spec_doesnt_use_accepted() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec policy
fact base: 100
rule discount: 10

spec policy 2025-06-01
fact base: 200
rule discount: 20
rule bonus: 5
"#,
            lemma::SourceType::Labeled("policy.lemma"),
        )
        .unwrap();

    // contract only references policy.discount — bonus is irrelevant
    engine
        .load(
            r#"
spec contract 2025-01-01
fact p: spec policy
rule applied_discount: p.discount
"#,
            lemma::SourceType::Labeled("contract.lemma"),
        )
        .unwrap();

    assert_rule_value(
        &eval(&engine, "contract", &date(2025, 3, 1)),
        "applied_discount",
        "10",
    );
    assert_rule_value(
        &eval(&engine, "contract", &date(2025, 9, 1)),
        "applied_discount",
        "20",
    );
}

#[test]
fn transitive_interface_incompatibility_rejected() {
    let mut engine = Engine::new();

    // deep v1 has factor. deep v2 renames to multiplier.
    engine
        .load(
            r#"
spec deep
fact factor: 2

spec deep 2025-06-01
fact multiplier: 5
"#,
            lemma::SourceType::Labeled("deep.lemma"),
        )
        .unwrap();

    // middle references deep.factor → its v2 slice breaks
    let result = engine.load(
        r#"
spec middle
fact d: spec deep
rule value: d.factor
"#,
        lemma::SourceType::Labeled("middle.lemma"),
    );

    // middle itself should fail because deep v2 lacks 'factor'
    assert!(
        result.is_err(),
        "Must reject: deep v2 removed 'factor' that middle references"
    );
}

// ============================================================================
// SLICE INTERFACE — CATEGORY 1: COMPATIBLE INTERFACES (planning succeeds)
// ============================================================================

#[test]
fn slice_compat_same_fact_type_different_values() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec rates
fact base: 50

spec rates 2025-07-01
fact base: 75
"#,
            lemma::SourceType::Labeled("rates.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec consumer 2025-01-01
fact r: spec rates
rule val: r.base
"#,
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "consumer", &date(2025, 3, 1)), "val", "50");
    assert_rule_value(&eval(&engine, "consumer", &date(2025, 9, 1)), "val", "75");
}

#[test]
fn slice_compat_dep_adds_unreferenced_facts() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec settings
fact limit: 10

spec settings 2025-05-01
fact limit: 20
fact description: "updated settings"
fact extra_number: 999
"#,
            lemma::SourceType::Labeled("settings.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec app 2025-01-01
fact s: spec settings
rule max: s.limit
"#,
            lemma::SourceType::Labeled("app.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "app", &date(2025, 3, 1)), "max", "10");
    assert_rule_value(&eval(&engine, "app", &date(2025, 8, 1)), "max", "20");
}

#[test]
fn slice_compat_dep_adds_unreferenced_rules() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec calc
rule base_fee: 100

spec calc 2025-04-01
rule base_fee: 150
rule surcharge: 25
"#,
            lemma::SourceType::Labeled("calc.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec invoice 2025-01-01
fact c: spec calc
rule fee: c.base_fee
"#,
            lemma::SourceType::Labeled("invoice.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "invoice", &date(2025, 2, 1)), "fee", "100");
    assert_rule_value(&eval(&engine, "invoice", &date(2025, 6, 1)), "fee", "150");
}

#[test]
fn slice_compat_dep_adds_both_facts_and_rules_unreferenced() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec lib
fact a: 1
rule x: a

spec lib 2025-06-01
fact a: 2
fact b: 99
rule x: a
rule y: b
"#,
            lemma::SourceType::Labeled("lib.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec caller 2025-01-01
fact l: spec lib
rule result: l.x
"#,
            lemma::SourceType::Labeled("caller.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "caller", &date(2025, 3, 1)), "result", "1");
    assert_rule_value(&eval(&engine, "caller", &date(2025, 9, 1)), "result", "2");
}

#[test]
fn slice_compat_single_dep_version_no_comparison() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec dep
fact val: 42
"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec main
fact d: spec dep
rule answer: d.val
"#,
            lemma::SourceType::Labeled("main.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "main", &date(2025, 1, 1)), "answer", "42");
}

#[test]
fn slice_compat_hash_pinned_ref_bypasses_interface_check() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec dep
fact x: 10

spec dep 2025-06-01
fact x: "text_now"
"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();

    let hash = engine
        .get_plan_hash("dep", &date(2025, 1, 1))
        .ok()
        .flatten()
        .expect("dep v1 should have hash")
        .to_string();

    let pinned_code = format!(
        r#"
spec pinned_caller
fact d: spec dep~{}
rule val: d.x
"#,
        hash
    );

    engine
        .load(&pinned_code, lemma::SourceType::Labeled("pinned.lemma"))
        .unwrap();

    assert_rule_value(
        &eval(&engine, "pinned_caller", &date(2025, 9, 1)),
        "val",
        "10",
    );
}

#[test]
fn slice_compat_three_dep_versions_all_compatible() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec params
fact rate: 10

spec params 2025-04-01
fact rate: 20

spec params 2025-08-01
fact rate: 30
"#,
            lemma::SourceType::Labeled("params.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec consumer 2025-01-01
fact p: spec params
rule r: p.rate
"#,
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "consumer", &date(2025, 2, 1)), "r", "10");
    assert_rule_value(&eval(&engine, "consumer", &date(2025, 5, 1)), "r", "20");
    assert_rule_value(&eval(&engine, "consumer", &date(2025, 10, 1)), "r", "30");
}

#[test]
fn slice_compat_two_deps_both_stable() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec tax
fact rate: 21

spec tax 2025-06-01
fact rate: 25
"#,
            lemma::SourceType::Labeled("tax.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shipping
fact cost: 5

spec shipping 2025-06-01
fact cost: 8
"#,
            lemma::SourceType::Labeled("shipping.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec order 2025-01-01
fact t: spec tax
fact s: spec shipping
rule total_overhead: t.rate + s.cost
"#,
            lemma::SourceType::Labeled("order.lemma"),
        )
        .unwrap();

    assert_rule_value(
        &eval(&engine, "order", &date(2025, 3, 1)),
        "total_overhead",
        "26",
    );
    assert_rule_value(
        &eval(&engine, "order", &date(2025, 9, 1)),
        "total_overhead",
        "33",
    );
}

#[test]
fn slice_compat_dep_rule_value_changes_but_type_same() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec policy
rule discount: 10

spec policy 2025-05-01
rule discount: 25
"#,
            lemma::SourceType::Labeled("policy.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec shop 2025-01-01
fact p: spec policy
rule d: p.discount
"#,
            lemma::SourceType::Labeled("shop.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "shop", &date(2025, 2, 1)), "d", "10");
    assert_rule_value(&eval(&engine, "shop", &date(2025, 8, 1)), "d", "25");
}

#[test]
fn slice_compat_dep_fact_type_annotation_identical() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec cfg
fact threshold: [number]

spec cfg 2025-04-01
fact threshold: [number]
"#,
            lemma::SourceType::Labeled("cfg.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec consumer 2025-01-01
fact c: spec cfg
fact c.threshold: 50
rule t: c.threshold
"#,
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "consumer", &date(2025, 2, 1)), "t", "50");
    assert_rule_value(&eval(&engine, "consumer", &date(2025, 6, 1)), "t", "50");
}

// ============================================================================
// SLICE INTERFACE — CATEGORY 2: INCOMPATIBLE INTERFACES (planning rejects)
// ============================================================================

#[test]
fn slice_incompat_fact_type_passthrough_changes() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec cfg
fact mode: 1

spec cfg 2025-04-01
fact mode: "turbo"
"#,
            lemma::SourceType::Labeled("cfg.lemma"),
        )
        .unwrap();

    // Caller just passes cfg.mode through — no arithmetic, so per-slice type
    // checking won't catch it. SliceInterface must detect the type change.
    let result = engine.load(
        r#"
spec consumer 2025-01-01
fact c: spec cfg
rule m: c.mode
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: cfg.mode changes from number to text between versions"
    );
    let errs = result.unwrap_err();
    let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
    let joined = msgs.join(" | ");
    assert!(
        joined.contains("interface") || joined.contains("changed"),
        "Error should mention interface change. Got: {}",
        joined
    );
}

#[test]
fn slice_incompat_rule_return_type_changes() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec svc
rule status: true

spec svc 2025-05-01
rule status: "active"
"#,
            lemma::SourceType::Labeled("svc.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec client 2025-01-01
fact s: spec svc
rule is_ok: s.status
"#,
        lemma::SourceType::Labeled("client.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: svc.status changes from boolean to text between versions"
    );
    let errs = result.unwrap_err();
    let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
    let joined = msgs.join(" | ");
    assert!(
        joined.contains("interface") || joined.contains("changed"),
        "Error should mention interface change. Got: {}",
        joined
    );
}

#[test]
fn slice_incompat_spec_ref_fact_changes_target() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec target_a
rule val: 1

spec target_b
rule val: 2
"#,
            lemma::SourceType::Labeled("targets.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec bridge
fact nested: spec target_a

spec bridge 2025-06-01
fact nested: spec target_b
"#,
            lemma::SourceType::Labeled("bridge.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec consumer 2025-01-01
fact b: spec bridge
rule v: b.nested.val
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: bridge.nested changes from spec target_a to spec target_b"
    );
    let errs = result.unwrap_err();
    let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
    let joined = msgs.join(" | ");
    assert!(
        joined.contains("interface") || joined.contains("changed") || joined.contains("target_"),
        "Error should mention interface change. Got: {}",
        joined
    );
}

#[test]
fn slice_incompat_spec_ref_target_incompatible_rule_type() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec target_a
rule val: 1

spec target_b
rule val: "now text"
"#,
            lemma::SourceType::Labeled("targets.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec bridge 2024-01-01
fact nested: spec target_a

spec bridge 2025-06-01
fact nested: spec target_b
"#,
            lemma::SourceType::Labeled("bridge.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec consumer 2024-01-01
fact b: spec bridge
rule v: b.nested.val
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: bridge.nested switches from target_a (val:number) to target_b (val:text)"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("interface") || joined.contains("changed"),
        "Error should mention interface change. Got: {}",
        joined
    );
}

#[test]
fn slice_incompat_type_definition_changes() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec finance
type money: scale
 -> unit eur 1.00
 -> decimals 2
fact price: [money]

spec finance 2025-06-01
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact price: [money]
"#,
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec shop 2025-01-01
fact f: spec finance
rule p: f.price
"#,
        lemma::SourceType::Labeled("shop.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: type 'money' definition changed between finance versions (added unit)"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("interface") || joined.contains("changed"),
        "Error should mention interface change. Got: {}",
        joined
    );
}

#[test]
fn slice_incompat_value_to_spec_ref_caught_by_per_slice() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec other
rule answer: 99
"#,
            lemma::SourceType::Labeled("other.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec provider
fact data: 100

spec provider 2025-06-01
fact data: spec other
"#,
            lemma::SourceType::Labeled("provider.lemma"),
        )
        .unwrap();

    // When a fact changes from value to spec reference (FactData::SpecRef), the access pattern
    // breaks within the affected slice. Per-slice graph building catches this.
    let result = engine.load(
        r#"
spec consumer 2025-01-01
fact p: spec provider
rule val: p.data
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: provider.data changes from value to spec reference"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("spec reference") || joined.contains("data"),
        "Per-slice error should mention the problematic fact. Got: {}",
        joined
    );
}

#[test]
fn slice_incompat_three_versions_middle_breaks() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec cfg
fact flag: true

spec cfg 2025-04-01
fact flag: "yes"

spec cfg 2025-08-01
fact flag: true
"#,
            lemma::SourceType::Labeled("cfg.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec consumer 2025-01-01
fact c: spec cfg
rule f: c.flag
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: cfg.flag is boolean in v1, text in v2, boolean in v3 — interface inconsistent"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("interface") || joined.contains("changed"),
        "Error should mention interface change. Got: {}",
        joined
    );
}

#[test]
fn slice_incompat_multiple_deps_one_unstable() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec stable_dep
fact x: 10

spec stable_dep 2025-06-01
fact x: 20
"#,
            lemma::SourceType::Labeled("stable.lemma"),
        )
        .unwrap();

    engine
        .load(
            r#"
spec unstable_dep
fact y: 5

spec unstable_dep 2025-06-01
fact y: "five"
"#,
            lemma::SourceType::Labeled("unstable.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec consumer 2025-01-01
fact a: spec stable_dep
fact b: spec unstable_dep
rule sx: a.x
rule sy: b.y
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: unstable_dep.y changes from number to text"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("changed its interface between temporal slices"),
        "Error must come from SliceInterface validation. Got: {}",
        joined
    );
    assert!(
        joined.contains("unstable_dep"),
        "Error must identify unstable_dep as the changed dependency. Got: {}",
        joined
    );
}

// ============================================================================
// SLICE INTERFACE — CATEGORY 3: EDGE CASES & BOUNDARY CONDITIONS
// ============================================================================

#[test]
fn slice_edge_fact_referenced_only_in_unless_branch() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec dep
fact main_val: 10
fact alt_val: 20

spec dep 2025-06-01
fact main_val: "ten"
fact alt_val: "twenty"
"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();

    // alt_val only appears in the unless then-branch, not the default.
    // Both slices are individually valid (branches have matching types within
    // each slice), but the interface changes across slices.
    let result = engine.load(
        r#"
spec caller 2025-01-01
fact d: spec dep
fact use_alt: [boolean]
rule result: d.main_val
 unless use_alt then d.alt_val
"#,
        lemma::SourceType::Labeled("caller.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: dep facts change from number to text across slices"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("changed its interface between temporal slices"),
        "Error must come from SliceInterface validation. Got: {}",
        joined
    );
}

#[test]
fn slice_edge_fact_in_both_condition_and_expression() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec data
fact amount: 100

spec data 2025-05-01
fact amount: 200
"#,
            lemma::SourceType::Labeled("data.lemma"),
        )
        .unwrap();

    // amount appears in both the unless condition and the then expression
    engine
        .load(
            r#"
spec calc 2025-01-01
fact d: spec data
rule result: 0
 unless d.amount > 50 then d.amount * 2
"#,
            lemma::SourceType::Labeled("calc.lemma"),
        )
        .unwrap();

    assert_rule_value(&eval(&engine, "calc", &date(2025, 3, 1)), "result", "200");
    assert_rule_value(&eval(&engine, "calc", &date(2025, 8, 1)), "result", "400");
}

#[test]
fn slice_edge_error_message_contains_diff_info() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec dep
fact score: 100
rule grade: 1

spec dep 2025-06-01
fact score: "A+"
rule grade: "first"
"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec caller 2025-01-01
fact d: spec dep
rule s: d.score
rule g: d.grade
"#,
        lemma::SourceType::Labeled("caller.lemma"),
    );

    assert!(result.is_err(), "Must reject: dep interface changed");
    let errs = result.unwrap_err();
    let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
    let joined = msgs.join(" | ");
    assert!(
        joined.contains("dep"),
        "Error should name the referenced spec 'dep'. Got: {}",
        joined
    );
    assert!(
        joined.contains("caller"),
        "Error should name the calling spec 'caller'. Got: {}",
        joined
    );
}

#[test]
fn slice_edge_dep_removes_bound_fact_caught_by_per_slice() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec dep
fact x: [number]
rule doubled: x * 2

spec dep 2025-06-01
rule doubled: 0
"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();

    // Caller binds dep.x — but dep v2 doesn't have fact x anymore.
    // Per-slice graph building should catch this.
    let result = engine.load(
        r#"
spec caller 2025-01-01
fact d: spec dep
fact d.x: 5
rule val: d.doubled
"#,
        lemma::SourceType::Labeled("caller.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: dep v2 no longer has fact 'x' that caller binds"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("does not exist") || joined.contains("x"),
        "Error should mention the missing fact. Got: {}",
        joined
    );
}

#[test]
fn slice_edge_dep_removes_referenced_rule_caught_by_per_slice() {
    let mut engine = Engine::new();

    engine
        .load(
            r#"
spec svc
rule compute: 42

spec svc 2025-06-01
rule other_compute: 99
"#,
            lemma::SourceType::Labeled("svc.lemma"),
        )
        .unwrap();

    let result = engine.load(
        r#"
spec caller 2025-01-01
fact s: spec svc
rule val: s.compute
"#,
        lemma::SourceType::Labeled("caller.lemma"),
    );

    assert!(
        result.is_err(),
        "Must reject: svc v2 no longer has rule 'compute' that caller references"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("compute"),
        "Error should mention the missing rule 'compute'. Got: {}",
        joined
    );
}

// ============================================================================
// 14. LOGIC LOCK TESTS
// ============================================================================

#[test]
fn unpinned_ref_stores_resolved_plan_hash() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec dep\nfact rate: 100\nrule r: rate",
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();
    engine
        .load(
            "spec consumer\nfact d: spec dep\nrule val: d.r",
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();

    let plan = engine
        .get_plan("consumer", Some(&date(2025, 1, 1)))
        .unwrap();
    let d_fact = plan
        .facts
        .values()
        .find(|fd| matches!(fd, FactData::SpecRef { .. }));
    let d_fact = d_fact.expect("consumer should have a SpecRef fact");
    assert!(
        d_fact.resolved_plan_hash().is_some(),
        "unpinned SpecRef must store resolved_plan_hash, got None"
    );

    let dep_hash = engine
        .get_plan_hash("dep", &date(2025, 1, 1))
        .ok()
        .flatten()
        .expect("dep should have hash");
    assert_eq!(
        d_fact.resolved_plan_hash().unwrap(),
        dep_hash.to_ascii_lowercase(),
        "resolved_plan_hash should equal dep's plan hash"
    );
}

#[test]
fn parent_hash_changes_when_dependency_changes() {
    let mut engine1 = Engine::new();
    engine1
        .load(
            "spec dep\nfact rate: 100\nrule r: rate",
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();
    engine1
        .load(
            "spec consumer\nfact d: spec dep\nrule val: d.r",
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();
    let h1 = engine1
        .get_plan_hash("consumer", &date(2025, 1, 1))
        .ok()
        .flatten()
        .expect("consumer hash v1");

    let mut engine2 = Engine::new();
    engine2
        .load(
            "spec dep\nfact rate: 200\nrule r: rate",
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();
    engine2
        .load(
            "spec consumer\nfact d: spec dep\nrule val: d.r",
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();
    let h2 = engine2
        .get_plan_hash("consumer", &date(2025, 1, 1))
        .ok()
        .flatten()
        .expect("consumer hash v2");

    assert_ne!(h1, h2, "consumer hash must change when dep content changes");
}

#[test]
fn type_import_pin_mismatch_fails_planning() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec finance\ntype money: scale\n -> unit eur 1.00\n -> decimals 2\nfact p: 10.00 eur",
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    let result = engine.load(
        "spec consumer\ntype money from finance~deadbeef\nfact price: 10.00 eur\nrule r: price",
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    assert!(
        result.is_err(),
        "type import with wrong hash pin should fail planning"
    );
    let err_str = result
        .unwrap_err()
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        err_str.to_lowercase().contains("finance"),
        "error should mention the spec name: {}",
        err_str
    );
}

#[test]
fn type_import_pin_match_succeeds_with_correct_hash() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec finance\ntype money: scale\n -> unit eur 1.00\n -> decimals 2\nfact p: 10.00 eur",
            lemma::SourceType::Labeled("finance.lemma"),
        )
        .unwrap();

    let finance_hash = engine
        .get_plan_hash("finance", &date(2025, 1, 1))
        .ok()
        .flatten()
        .expect("finance hash")
        .to_string();

    let consumer_src = format!(
        "spec consumer\ntype money from finance~{}\nfact price: 10.00 eur\nrule r: price * 2",
        finance_hash
    );
    engine
        .load(&consumer_src, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();

    assert_rule_value(
        &eval(&engine, "consumer", &date(2025, 1, 1)),
        "r",
        "20.00 eur",
    );
}

#[test]
fn missing_dependency_hash_when_dep_fails_planning() {
    let mut engine = Engine::new();
    // dep has circular rules -> planning will fail for dep
    let result = engine.load(
        "spec dep\nrule a: b\nrule b: a\n\nspec consumer\nfact d: spec dep\nrule val: d.a",
        lemma::SourceType::Labeled("all.lemma"),
    );

    assert!(result.is_err(), "should fail when dep has circular rules");
    let err_str = result
        .unwrap_err()
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        err_str.to_lowercase().contains("circular") || err_str.to_lowercase().contains("cycle"),
        "error should mention circularity: {}",
        err_str
    );
}

#[test]
fn serde_round_trip_resolved_plan_hash() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec dep\nfact rate: 100\nrule r: rate",
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();
    engine
        .load(
            "spec consumer\nfact d: spec dep\nrule val: d.r",
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();

    let plan = engine
        .get_plan("consumer", Some(&date(2025, 1, 1)))
        .unwrap();
    let json = serde_json::to_string(plan).expect("serialize plan");
    assert!(
        json.contains("resolved_plan_hash"),
        "serialized plan should contain resolved_plan_hash key"
    );

    let deserialized: lemma::ExecutionPlan = serde_json::from_str(&json).expect("deserialize plan");
    let d_fact = deserialized
        .facts
        .values()
        .find(|fd| matches!(fd, FactData::SpecRef { .. }));
    let d_fact = d_fact.expect("deserialized plan should have SpecRef");
    assert!(
        d_fact.resolved_plan_hash().is_some(),
        "resolved_plan_hash must survive round-trip"
    );
}
