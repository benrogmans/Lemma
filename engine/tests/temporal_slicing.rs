//! Integration tests for temporal slicing of execution plans.
//!
//! When a document's active range spans dependency version boundaries,
//! planning must produce one ExecutionPlan per temporal slice and each
//! slice must independently validate.
//!
//! These tests define the target behavior. Many will fail until the
//! temporal slicing implementation is complete.

mod common;
use common::add_lemma_code_blocking;
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
        timezone: None,
    }
}

fn eval(engine: &Engine, doc: &str, effective: &DateTimeValue) -> lemma::Response {
    engine
        .evaluate(doc, None, effective, vec![], HashMap::new())
        .unwrap()
}

fn eval_with(
    engine: &Engine,
    doc: &str,
    effective: &DateTimeValue,
    facts: Vec<(&str, &str)>,
) -> lemma::Response {
    let map: HashMap<String, String> = facts
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    engine.evaluate(doc, None, effective, vec![], map).unwrap()
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

    add_lemma_code_blocking(
        &mut engine,
        "doc config\nfact base_rate: 100",
        "config.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-01-01
fact cfg: doc config
rule rate: cfg.base_rate * 2
"#,
        "pricing.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc config
fact base_rate: 100

doc config 2025-04-01
fact base_rate: 200
"#,
        "config.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-01-01
fact cfg: doc config
rule rate: cfg.base_rate
"#,
        "pricing.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "pricing", &date(2025, 2, 1)), "rate", "100");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 3, 31)), "rate", "100");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 4, 1)), "rate", "200");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 6, 15)), "rate", "200");
}

#[test]
fn boundary_exactly_at_doc_effective_from_no_split() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc config
fact rate: 50

doc config 2025-01-01
fact rate: 75
"#,
        "config.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-01-01
fact cfg: doc config
rule rate: cfg.rate
"#,
        "pricing.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc rates
fact rate: 10

doc rates 2025-03-01
fact rate: 20

doc rates 2025-07-01
fact rate: 30
"#,
        "rates.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-01-01
fact r: doc rates
fact quantity: [number]
rule total: quantity * r.rate
"#,
        "pricing.lemma",
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
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc rates
fact rate: 10

doc rates 2025-03-01
fact rate: 20

doc rates 2025-06-01
fact rate: 30

doc rates 2025-09-01
fact rate: 40
"#,
        "rates.lemma",
    )
    .unwrap();

    // pricing active [Apr, +∞) → Jun and Sep boundaries are inside
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-04-01
fact r: doc rates
rule rate: r.rate
"#,
        "pricing.lemma",
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
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc tax_rates
fact vat: 19

doc tax_rates 2025-04-01
fact vat: 21
"#,
        "tax_rates.lemma",
    )
    .unwrap();

    // shipping_rates: boundary at July
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc shipping_rates
fact fee: 5

doc shipping_rates 2025-07-01
fact fee: 8
"#,
        "shipping_rates.lemma",
    )
    .unwrap();

    // invoice: depends on both → boundaries at {April, July} → three slices
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc invoice 2025-01-01
fact tax: doc tax_rates
fact shipping: doc shipping_rates
fact price: [number]
rule vat_amount: price * tax.vat / 100
rule shipping_fee: shipping.fee
rule total: price + vat_amount + shipping_fee
"#,
        "invoice.lemma",
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
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc tax_rates
fact vat: 19

doc tax_rates 2025-04-01
fact vat: 21
"#,
        "tax_rates.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc shipping_rates
fact fee: 5

doc shipping_rates 2025-04-01
fact fee: 8
"#,
        "shipping_rates.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc invoice 2025-01-01
fact tax: doc tax_rates
fact shipping: doc shipping_rates
rule combined: tax.vat + shipping.fee
"#,
        "invoice.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc constants
fact pi: 3
"#,
        "constants.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc rates
fact multiplier: 2

doc rates 2025-06-01
fact multiplier: 4
"#,
        "rates.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc calc 2025-01-01
fact c: doc constants
fact r: doc rates
rule result: c.pi * r.multiplier
"#,
        "calc.lemma",
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

    // DocC: two versions
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc base_rates
fact multiplier: 2

doc base_rates 2025-06-01
fact multiplier: 3
"#,
        "base_rates.lemma",
    )
    .unwrap();

    // DocB: unversioned, depends on DocC
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc intermediate
fact base: doc base_rates
fact value: 10
rule adjusted: value * base.multiplier
"#,
        "intermediate.lemma",
    )
    .unwrap();

    // DocA: depends on DocB → transitively on DocC
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc top 2025-01-01
fact mid: doc intermediate
rule result: mid.adjusted
"#,
        "top.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "top", &date(2025, 3, 1)), "result", "20");
    assert_rule_value(&eval(&engine, "top", &date(2025, 9, 1)), "result", "30");
}

#[test]
fn transitive_both_levels_versioned() {
    let mut engine = Engine::new();

    // DocC: boundary at June
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc deep
fact factor: 2

doc deep 2025-06-01
fact factor: 5
"#,
        "deep.lemma",
    )
    .unwrap();

    // DocB: boundary at April, depends on DocC
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc middle
fact d: doc deep
fact base: 10
rule value: base * d.factor

doc middle 2025-04-01
fact d: doc deep
fact base: 100
rule value: base * d.factor
"#,
        "middle.lemma",
    )
    .unwrap();

    // DocA: active from Jan → boundaries at {April, June} → three slices
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc top 2025-01-01
fact m: doc middle
rule result: m.value
"#,
        "top.lemma",
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
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc shared
fact value: 10

doc shared 2025-06-01
fact value: 20
"#,
        "shared.lemma",
    )
    .unwrap();

    // Two intermediate docs both depend on shared
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc left_branch
fact s: doc shared
rule doubled: s.value * 2
"#,
        "left.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc right_branch
fact s: doc shared
rule tripled: s.value * 3
"#,
        "right.lemma",
    )
    .unwrap();

    // Top depends on both branches (diamond through shared)
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc top 2025-01-01
fact l: doc left_branch
fact r: doc right_branch
rule total: l.doubled + r.tripled
"#,
        "top.lemma",
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
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc shared
fact base: 10

doc shared 2025-08-01
fact base: 50
"#,
        "shared.lemma",
    )
    .unwrap();

    // Left branch: boundary at April
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc left
fact s: doc shared
fact add: 1
rule result: s.base + add

doc left 2025-04-01
fact s: doc shared
fact add: 2
rule result: s.base + add
"#,
        "left.lemma",
    )
    .unwrap();

    // Right branch: unversioned
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc right
fact s: doc shared
rule result: s.base * 2
"#,
        "right.lemma",
    )
    .unwrap();

    // Top: active from Jan → boundaries at {April, August} → three slices
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc top 2025-01-01
fact l: doc left
fact r: doc right
rule total: l.result + r.result
"#,
        "top.lemma",
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
fn unranged_doc_sliced_by_versioned_dep() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc rates
fact tax: 19

doc rates 2026-01-01
fact tax: 21
"#,
        "rates.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc calculator
fact r: doc rates
fact income: [number]
rule tax_amount: income * r.tax / 100
"#,
        "calculator.lemma",
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
fn dependency_not_yet_active_at_doc_start() {
    let mut engine = Engine::new();

    // config only starts in June
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc config 2025-06-01
fact rate: 100
"#,
        "config.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-01-01
fact cfg: doc config
rule rate: cfg.rate
"#,
        "pricing.lemma",
    );

    assert!(
        result.is_err(),
        "Must reject: config not active [Jan, Jun) but pricing needs it from January"
    );
}

#[test]
fn unbounded_doc_depending_on_bounded_dep_rejected() {
    let mut engine = Engine::new();

    // dep only active from June onward
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc regulations 2025-06-01
fact max_amount: 500
"#,
        "regulations.lemma",
    )
    .unwrap();

    // unbounded doc [-∞, +∞) depends on dep [Jun, +∞) → planning error
    // dep's coverage is narrower than the doc's range
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc contract
fact reg: doc regulations
fact amount: [number]
rule is_valid: amount <= reg.max_amount
"#,
        "contract.lemma",
    );

    assert!(
        result.is_err(),
        "Must reject: unbounded doc can't depend on a dep that doesn't cover [-∞, +∞)"
    );
}

#[test]
fn three_versions_seamlessly_chained() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc policy 2025-01-01
fact limit: 1000

doc policy 2025-04-01
fact limit: 2000

doc policy 2025-08-01
fact limit: 3000
"#,
        "policy.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc contract 2025-01-01
fact p: doc policy
fact amount: [number]
rule under_limit: amount < p.limit
"#,
        "contract.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc config
fact rate: 100

doc config 2025-04-01
fact rate: 999
"#,
        "config.lemma",
    )
    .unwrap();

    // Get the hash of the first (unversioned) config doc
    let v1_hash = engine
        .all_hash_pins()
        .iter()
        .find(|(name, af, _)| *name == "config" && af.is_none())
        .map(|(_, _, h)| h.to_string())
        .expect("should have hash for config v1");

    // Use the hash pin to always resolve config v1
    let pricing_src = format!(
        "doc pricing 2025-01-01\nfact cfg: doc config~{}\nrule rate: cfg.rate",
        v1_hash
    );
    add_lemma_code_blocking(&mut engine, &pricing_src, "pricing.lemma").unwrap();

    assert_rule_value(&eval(&engine, "pricing", &date(2025, 2, 1)), "rate", "100");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 9, 1)), "rate", "100");
}

#[test]
fn hash_pinned_ref_wrong_hash_fails_planning() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(&mut engine, "doc config\nfact rate: 100", "config.lemma").unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        "doc consumer\nfact cfg: doc config~deadbeef\nrule r: cfg.rate",
        "consumer.lemma",
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
        "error should mention the doc name: {}",
        err_str
    );
}

#[test]
fn hash_pinned_type_import_resolves() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc finance
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact base_price: 50.00 eur
"#,
        "finance.lemma",
    )
    .unwrap();

    let finance_hash = engine
        .hash_pin("finance", &date(2025, 1, 1))
        .expect("should have finance hash")
        .to_string();

    let consumer_src = format!(
        "doc consumer\ntype money from finance {}\nfact price: 100.00 eur\nrule double: price * 2",
        finance_hash
    );
    add_lemma_code_blocking(&mut engine, &consumer_src, "consumer.lemma").unwrap();

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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc dep
fact val: 1

doc dep 2025-06-01
fact val: 2
"#,
        "dep.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc main
fact d: doc dep
rule result: d.val
"#,
        "main.lemma",
    )
    .unwrap();

    // Exactly at the boundary: effective_from is inclusive
    assert_rule_value(&eval(&engine, "main", &date(2025, 5, 31)), "result", "1");
    assert_rule_value(&eval(&engine, "main", &date(2025, 6, 1)), "result", "2");
}

// ============================================================================
// 10. DEPENDENT DOC (DocC) REFERENCES VERSIONED DOC (DocB)
// ============================================================================

#[test]
fn third_level_doc_depends_on_versioned_doc() {
    let mut engine = Engine::new();

    // DocB: versioned
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc rates
fact base: 100

doc rates 2025-05-01
fact base: 200
"#,
        "rates.lemma",
    )
    .unwrap();

    // DocA: unversioned, depends on DocB
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc policy
fact r: doc rates
rule threshold: r.base * 2
"#,
        "policy.lemma",
    )
    .unwrap();

    // DocC: depends on DocA (which transitively depends on DocB)
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc contract 2025-01-01
fact p: doc policy
fact amount: [number]
rule is_over_threshold: amount > p.threshold
"#,
        "contract.lemma",
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
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc tax_law
fact income_tax_rate: 30

doc tax_law 2025-04-01
fact income_tax_rate: 32
"#,
        "tax.lemma",
    )
    .unwrap();

    // Labor law: change July 1
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc labor_law
fact min_wage_hourly: 12
fact max_weekly_hours: 40

doc labor_law 2025-07-01
fact min_wage_hourly: 15
fact max_weekly_hours: 38
"#,
        "labor.lemma",
    )
    .unwrap();

    // Employment contract: depends on both, active from Jan
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc employment 2025-01-01
fact tax: doc tax_law
fact labor: doc labor_law
fact hourly_rate: [number]
fact weekly_hours: [number]

rule annual_gross: hourly_rate * weekly_hours * 52
rule annual_tax: annual_gross * tax.income_tax_rate / 100
rule annual_net: annual_gross - annual_tax
rule min_annual_gross: labor.min_wage_hourly * labor.max_weekly_hours * 52
rule meets_minimum: annual_gross >= min_annual_gross
"#,
        "employment.lemma",
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
fn both_doc_and_dep_are_versioned() {
    let mut engine = Engine::new();

    // dep: boundary at June
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc dep
fact val: 10

doc dep 2025-06-01
fact val: 20
"#,
        "dep.lemma",
    )
    .unwrap();

    // main v1: [Jan, Apr), main v2: [Apr, +∞)
    // main v1's range [Jan, Apr) has no dep boundary → single slice for v1
    // main v2's range [Apr, +∞) has dep boundary at June → two slices for v2
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc main 2025-01-01
fact d: doc dep
fact multiplier: 2
rule result: d.val * multiplier

doc main 2025-04-01
fact d: doc dep
fact multiplier: 3
rule result: d.val * multiplier
"#,
        "main.lemma",
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
// dependent doc actually references (per-slice interface validation).

#[test]
fn dep_version_removes_referenced_fact_rejected() {
    let mut engine = Engine::new();

    // config v1 has base_rate. config v2 renames it to cost.
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc config
fact base_rate: 100

doc config 2025-04-01
fact cost: 200
"#,
        "config.lemma",
    )
    .unwrap();

    // pricing references config.base_rate → v2 slice fails (no base_rate in v2)
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-01-01
fact cfg: doc config
rule rate: cfg.base_rate
"#,
        "pricing.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc policy
fact threshold: 100
rule discount: 10

doc policy 2025-06-01
fact threshold: 200
"#,
        "policy.lemma",
    )
    .unwrap();

    // contract references policy.discount → v2 slice fails (no discount rule in v2)
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc contract 2025-01-01
fact p: doc policy
rule applied_discount: p.discount
"#,
        "contract.lemma",
    );

    assert!(
        result.is_err(),
        "Must reject: policy v2 (June+) removed 'discount' rule that contract references"
    );
    let errs = result.unwrap_err();
    assert!(
        !errs.is_empty(),
        "expected at least one planning error (policy v2 missing discount rule)"
    );
}

#[test]
fn dep_version_changes_fact_type_rejected() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc config
fact rate: 100

doc config 2025-04-01
fact rate: "high"
"#,
        "config.lemma",
    )
    .unwrap();

    // pricing does arithmetic with config.rate → v2 slice fails (rate is text, not number)
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-01-01
fact cfg: doc config
rule total: cfg.rate * 2
"#,
        "pricing.lemma",
    );

    assert!(
        result.is_err(),
        "Must reject: config v2 changed 'rate' from number to text, but pricing does arithmetic"
    );
}

#[test]
fn dep_version_changes_rule_type_rejected() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc policy
fact base: 100
rule discount: 10

doc policy 2025-04-01
fact base: 200
rule discount: "fixed"
"#,
        "policy.lemma",
    )
    .unwrap();

    // contract does arithmetic with p.discount → v2 slice fails (discount is text, not number)
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc contract 2025-01-01
fact p: doc policy
rule total: p.base - p.discount
"#,
        "contract.lemma",
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
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc config
fact base_rate: 100

doc config 2025-04-01
fact base_rate: 200
fact extra_field: 999
"#,
        "config.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc pricing 2025-01-01
fact cfg: doc config
rule rate: cfg.base_rate
"#,
        "pricing.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "pricing", &date(2025, 2, 1)), "rate", "100");
    assert_rule_value(&eval(&engine, "pricing", &date(2025, 5, 1)), "rate", "200");
}

#[test]
fn dep_version_adds_rule_that_other_doc_doesnt_use_accepted() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc policy
fact base: 100
rule discount: 10

doc policy 2025-06-01
fact base: 200
rule discount: 20
rule bonus: 5
"#,
        "policy.lemma",
    )
    .unwrap();

    // contract only references policy.discount — bonus is irrelevant
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc contract 2025-01-01
fact p: doc policy
rule applied_discount: p.discount
"#,
        "contract.lemma",
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
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc deep
fact factor: 2

doc deep 2025-06-01
fact multiplier: 5
"#,
        "deep.lemma",
    )
    .unwrap();

    // middle references deep.factor → its v2 slice breaks
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc middle
fact d: doc deep
rule value: d.factor
"#,
        "middle.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc rates
fact base: 50

doc rates 2025-07-01
fact base: 75
"#,
        "rates.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2025-01-01
fact r: doc rates
rule val: r.base
"#,
        "consumer.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "consumer", &date(2025, 3, 1)), "val", "50");
    assert_rule_value(&eval(&engine, "consumer", &date(2025, 9, 1)), "val", "75");
}

#[test]
fn slice_compat_dep_adds_unreferenced_facts() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc settings
fact limit: 10

doc settings 2025-05-01
fact limit: 20
fact description: "updated settings"
fact extra_number: 999
"#,
        "settings.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc app 2025-01-01
fact s: doc settings
rule max: s.limit
"#,
        "app.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "app", &date(2025, 3, 1)), "max", "10");
    assert_rule_value(&eval(&engine, "app", &date(2025, 8, 1)), "max", "20");
}

#[test]
fn slice_compat_dep_adds_unreferenced_rules() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc calc
rule base_fee: 100

doc calc 2025-04-01
rule base_fee: 150
rule surcharge: 25
"#,
        "calc.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc invoice 2025-01-01
fact c: doc calc
rule fee: c.base_fee
"#,
        "invoice.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "invoice", &date(2025, 2, 1)), "fee", "100");
    assert_rule_value(&eval(&engine, "invoice", &date(2025, 6, 1)), "fee", "150");
}

#[test]
fn slice_compat_dep_adds_both_facts_and_rules_unreferenced() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc lib
fact a: 1
rule x: a

doc lib 2025-06-01
fact a: 2
fact b: 99
rule x: a
rule y: b
"#,
        "lib.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc caller 2025-01-01
fact l: doc lib
rule result: l.x
"#,
        "caller.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "caller", &date(2025, 3, 1)), "result", "1");
    assert_rule_value(&eval(&engine, "caller", &date(2025, 9, 1)), "result", "2");
}

#[test]
fn slice_compat_single_dep_version_no_comparison() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc dep
fact val: 42
"#,
        "dep.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc main
fact d: doc dep
rule answer: d.val
"#,
        "main.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "main", &date(2025, 1, 1)), "answer", "42");
}

#[test]
fn slice_compat_hash_pinned_ref_bypasses_interface_check() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc dep
fact x: 10

doc dep 2025-06-01
fact x: "text_now"
"#,
        "dep.lemma",
    )
    .unwrap();

    let hash = engine
        .hash_pin("dep", &date(2025, 1, 1))
        .expect("dep v1 should have hash")
        .to_string();

    let pinned_code = format!(
        r#"
doc pinned_caller
fact d: doc dep~{}
rule val: d.x
"#,
        hash
    );

    add_lemma_code_blocking(&mut engine, &pinned_code, "pinned.lemma").unwrap();

    assert_rule_value(
        &eval(&engine, "pinned_caller", &date(2025, 9, 1)),
        "val",
        "10",
    );
}

#[test]
fn slice_compat_three_dep_versions_all_compatible() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc params
fact rate: 10

doc params 2025-04-01
fact rate: 20

doc params 2025-08-01
fact rate: 30
"#,
        "params.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2025-01-01
fact p: doc params
rule r: p.rate
"#,
        "consumer.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "consumer", &date(2025, 2, 1)), "r", "10");
    assert_rule_value(&eval(&engine, "consumer", &date(2025, 5, 1)), "r", "20");
    assert_rule_value(&eval(&engine, "consumer", &date(2025, 10, 1)), "r", "30");
}

#[test]
fn slice_compat_two_deps_both_stable() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc tax
fact rate: 21

doc tax 2025-06-01
fact rate: 25
"#,
        "tax.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc shipping
fact cost: 5

doc shipping 2025-06-01
fact cost: 8
"#,
        "shipping.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc order 2025-01-01
fact t: doc tax
fact s: doc shipping
rule total_overhead: t.rate + s.cost
"#,
        "order.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc policy
rule discount: 10

doc policy 2025-05-01
rule discount: 25
"#,
        "policy.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc shop 2025-01-01
fact p: doc policy
rule d: p.discount
"#,
        "shop.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "shop", &date(2025, 2, 1)), "d", "10");
    assert_rule_value(&eval(&engine, "shop", &date(2025, 8, 1)), "d", "25");
}

#[test]
fn slice_compat_dep_fact_type_annotation_identical() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc cfg
fact threshold: [number]

doc cfg 2025-04-01
fact threshold: [number]
"#,
        "cfg.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2025-01-01
fact c: doc cfg
fact c.threshold: 50
rule t: c.threshold
"#,
        "consumer.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc cfg
fact mode: 1

doc cfg 2025-04-01
fact mode: "turbo"
"#,
        "cfg.lemma",
    )
    .unwrap();

    // Caller just passes cfg.mode through — no arithmetic, so per-slice type
    // checking won't catch it. SliceInterface must detect the type change.
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2025-01-01
fact c: doc cfg
rule m: c.mode
"#,
        "consumer.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc svc
rule status: true

doc svc 2025-05-01
rule status: "active"
"#,
        "svc.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc client 2025-01-01
fact s: doc svc
rule is_ok: s.status
"#,
        "client.lemma",
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
fn slice_incompat_document_ref_fact_changes_target() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc target_a
rule val: 1

doc target_b
rule val: 2
"#,
        "targets.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc bridge
fact nested: doc target_a

doc bridge 2025-06-01
fact nested: doc target_b
"#,
        "bridge.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2025-01-01
fact b: doc bridge
rule v: b.nested.val
"#,
        "consumer.lemma",
    );

    assert!(
        result.is_err(),
        "Must reject: bridge.nested changes from doc target_a to doc target_b"
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
fn slice_incompat_document_ref_target_incompatible_rule_type() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc target_a
rule val: 1

doc target_b
rule val: "now text"
"#,
        "targets.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc bridge 2024-01-01
fact nested: doc target_a

doc bridge 2025-06-01
fact nested: doc target_b
"#,
        "bridge.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2024-01-01
fact b: doc bridge
rule v: b.nested.val
"#,
        "consumer.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc finance
type money: scale
 -> unit eur 1.00
 -> decimals 2
fact price: [money]

doc finance 2025-06-01
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2
fact price: [money]
"#,
        "finance.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc shop 2025-01-01
fact f: doc finance
rule p: f.price
"#,
        "shop.lemma",
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
fn slice_incompat_value_to_docref_caught_by_per_slice() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc other
rule answer: 99
"#,
        "other.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc provider
fact data: 100

doc provider 2025-06-01
fact data: doc other
"#,
        "provider.lemma",
    )
    .unwrap();

    // When a fact changes from value to DocumentRef, the access pattern
    // breaks within the affected slice. Per-slice graph building catches this.
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2025-01-01
fact p: doc provider
rule val: p.data
"#,
        "consumer.lemma",
    );

    assert!(
        result.is_err(),
        "Must reject: provider.data changes from value to document reference"
    );
    let errs = result.unwrap_err();
    let joined = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("document reference") || joined.contains("data"),
        "Per-slice error should mention the problematic fact. Got: {}",
        joined
    );
}

#[test]
fn slice_incompat_three_versions_middle_breaks() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc cfg
fact flag: true

doc cfg 2025-04-01
fact flag: "yes"

doc cfg 2025-08-01
fact flag: true
"#,
        "cfg.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2025-01-01
fact c: doc cfg
rule f: c.flag
"#,
        "consumer.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc stable_dep
fact x: 10

doc stable_dep 2025-06-01
fact x: 20
"#,
        "stable.lemma",
    )
    .unwrap();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc unstable_dep
fact y: 5

doc unstable_dep 2025-06-01
fact y: "five"
"#,
        "unstable.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc consumer 2025-01-01
fact a: doc stable_dep
fact b: doc unstable_dep
rule sx: a.x
rule sy: b.y
"#,
        "consumer.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc dep
fact main_val: 10
fact alt_val: 20

doc dep 2025-06-01
fact main_val: "ten"
fact alt_val: "twenty"
"#,
        "dep.lemma",
    )
    .unwrap();

    // alt_val only appears in the unless then-branch, not the default.
    // Both slices are individually valid (branches have matching types within
    // each slice), but the interface changes across slices.
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc caller 2025-01-01
fact d: doc dep
fact use_alt: [boolean]
rule result: d.main_val
 unless use_alt then d.alt_val
"#,
        "caller.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc data
fact amount: 100

doc data 2025-05-01
fact amount: 200
"#,
        "data.lemma",
    )
    .unwrap();

    // amount appears in both the unless condition and the then expression
    add_lemma_code_blocking(
        &mut engine,
        r#"
doc calc 2025-01-01
fact d: doc data
rule result: 0
 unless d.amount > 50 then d.amount * 2
"#,
        "calc.lemma",
    )
    .unwrap();

    assert_rule_value(&eval(&engine, "calc", &date(2025, 3, 1)), "result", "200");
    assert_rule_value(&eval(&engine, "calc", &date(2025, 8, 1)), "result", "400");
}

#[test]
fn slice_edge_error_message_contains_diff_info() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc dep
fact score: 100
rule grade: 1

doc dep 2025-06-01
fact score: "A+"
rule grade: "first"
"#,
        "dep.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc caller 2025-01-01
fact d: doc dep
rule s: d.score
rule g: d.grade
"#,
        "caller.lemma",
    );

    assert!(result.is_err(), "Must reject: dep interface changed");
    let errs = result.unwrap_err();
    let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
    let joined = msgs.join(" | ");
    assert!(
        joined.contains("dep"),
        "Error should name the referenced document 'dep'. Got: {}",
        joined
    );
    assert!(
        joined.contains("caller"),
        "Error should name the calling document 'caller'. Got: {}",
        joined
    );
}

#[test]
fn slice_edge_dep_removes_bound_fact_caught_by_per_slice() {
    let mut engine = Engine::new();

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc dep
fact x: [number]
rule doubled: x * 2

doc dep 2025-06-01
rule doubled: 0
"#,
        "dep.lemma",
    )
    .unwrap();

    // Caller binds dep.x — but dep v2 doesn't have fact x anymore.
    // Per-slice graph building should catch this.
    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc caller 2025-01-01
fact d: doc dep
fact d.x: 5
rule val: d.doubled
"#,
        "caller.lemma",
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

    add_lemma_code_blocking(
        &mut engine,
        r#"
doc svc
rule compute: 42

doc svc 2025-06-01
rule other_compute: 99
"#,
        "svc.lemma",
    )
    .unwrap();

    let result = add_lemma_code_blocking(
        &mut engine,
        r#"
doc caller 2025-01-01
fact s: doc svc
rule val: s.compute
"#,
        "caller.lemma",
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
