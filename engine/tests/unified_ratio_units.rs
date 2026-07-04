//! Unified ratio units: multiple `ratio` types per spec, shared builtin/custom units in the
//! unit index, cross-type arithmetic/comparison, per-type `as` scoping, and expression-level
//! (anonymous) ratios. Assertions use canonical `ValueKind` + `RationalInteger`, not display
//! substrings alone.

use lemma::DateTimeValue;
use lemma::{Engine, LiteralValue};
use lemma::{TypeSpecification, ValueKind};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

fn decimal_lit(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn path_source(file: &str) -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from(file)))
}

fn load_ok(engine: &mut Engine, code: &str, file: &str) {
    engine.load(code, path_source(file)).unwrap_or_else(|errs| {
        let joined = errs
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        panic!("expected load to succeed ({file}), got: {joined}");
    });
}

fn expect_load_error(code: &str, file: &str, fragments: &[&str]) {
    let mut engine = Engine::new();
    let result = engine.load(code, path_source(file));
    assert!(result.is_err(), "expected load to fail ({file})");
    let combined = result
        .unwrap_err()
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    for frag in fragments {
        assert!(
            combined.contains(frag),
            "expected error containing '{frag}', got: {combined}"
        );
    }
}

fn run_spec(engine: &Engine, spec: &str, data: HashMap<String, String>) -> lemma::Response {
    let now = DateTimeValue::now();
    engine
        .run(None, spec, Some(&now), data, true, None)
        .unwrap_or_else(|e| panic!("run({spec}) failed: {e}"))
}

fn rule_value<'a>(response: &'a lemma::Response, rule: &str) -> &'a LiteralValue {
    let rr = response
        .results
        .get(rule)
        .unwrap_or_else(|| panic!("rule '{rule}' missing; keys: {:?}", response.results.keys()));
    if rr.vetoed {
        panic!(
            "rule '{rule}' vetoed: {}",
            rr.veto_reason.as_deref().unwrap_or("Vetoed")
        );
    }
    rr.explanation
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value")
}

fn assert_ratio_exact(
    lit: &LiteralValue,
    ctx: &str,
    expected_canonical: &str,
    expected_unit: Option<&str>,
) {
    match &lit.value {
        ValueKind::Ratio(r, u) => {
            assert_eq!(
                lemma::ValueKind::Number(r.clone())
                    .as_decimal_magnitude()
                    .unwrap(),
                decimal_lit(expected_canonical),
                "{ctx}: canonical magnitude"
            );
            assert_eq!(u.as_deref(), expected_unit, "{ctx}: unit tag");
        }
        other => panic!("{ctx}: expected ValueKind::Ratio, got {other:?}"),
    }
}

fn assert_number_exact(lit: &LiteralValue, ctx: &str, expected: &str) {
    match &lit.value {
        ValueKind::Number(n) => {
            assert_eq!(
                lemma::ValueKind::Number(n.clone())
                    .as_decimal_magnitude()
                    .unwrap(),
                decimal_lit(expected),
                "{ctx}"
            );
        }
        other => panic!("{ctx}: expected Number, got {other:?}"),
    }
}

fn assert_bool(lit: &LiteralValue, ctx: &str, expected: bool) {
    match &lit.value {
        ValueKind::Boolean(b) => assert_eq!(*b, expected, "{ctx}"),
        other => panic!("{ctx}: expected Boolean, got {other:?}"),
    }
}

fn ratio_unit_names(spec: &TypeSpecification) -> Vec<&str> {
    match spec {
        TypeSpecification::Ratio { units, .. } => units.iter().map(|u| u.name.as_str()).collect(),
        other => panic!("expected Ratio spec, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------
// Section 1 — Multi-type spec loads (user regression)
// -----------------------------------------------------------------------------

const TARGETS_SPEC: &str = r#"
spec targets
"""
Corporate financial targets, minimum margins, and standard bonuses.
"""
data standard_margin_pct: ratio
  -> minimum 0%
  -> default 15%

data default_credit_insurance_pct: ratio
  -> default 1.5%

rule margin: standard_margin_pct
rule insurance: default_credit_insurance_pct
"#;

#[test]
fn targets_spec_two_ratio_fields_load_and_run_defaults() {
    let mut engine = Engine::new();
    load_ok(&mut engine, TARGETS_SPEC, "targets.lemma");

    let response = run_spec(&engine, "targets", HashMap::new());
    assert_ratio_exact(
        rule_value(&response, "margin"),
        "margin default",
        "0.15",
        Some("percent"),
    );
    assert_ratio_exact(
        rule_value(&response, "insurance"),
        "insurance default",
        "0.015",
        Some("percent"),
    );
}

#[test]
fn targets_spec_schema_both_ratio_types_have_builtin_units() {
    let mut engine = Engine::new();
    load_ok(&mut engine, TARGETS_SPEC, "targets_schema.lemma");

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "targets", Some(&now)).expect("schema");

    for name in ["standard_margin_pct", "default_credit_insurance_pct"] {
        let entry = schema.data.get(name).expect(name);
        let names = ratio_unit_names(&entry.lemma_type.specifications);
        assert!(
            names.contains(&"percent"),
            "{name}: missing percent unit, got {names:?}"
        );
        assert!(
            names.contains(&"permille"),
            "{name}: missing permille unit, got {names:?}"
        );
    }

    let margin = schema.data.get("standard_margin_pct").expect("margin");
    assert_eq!(
        margin.lemma_type.specifications.minimum_decimal(),
        Some(decimal_lit("0"))
    );
}

#[test]
fn three_ratio_fields_builtin_units_load() {
    let code = r#"
spec rates
data margin_pct: ratio -> default 10%
data fee_pct: ratio -> default 2%
data tax_pct: ratio -> default 1%
rule sum: margin_pct + fee_pct + tax_pct
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "three_ratio.lemma");

    let response = run_spec(&engine, "rates", HashMap::new());
    // 0.10 + 0.02 + 0.01 = 0.13 (left-associative; operands carry percent from defaults)
    assert_ratio_exact(rule_value(&response, "sum"), "sum", "0.13", Some("percent"));
}

// -----------------------------------------------------------------------------
// Section 2 — Cross-type evaluation
// -----------------------------------------------------------------------------

const FINANCE_SPEC: &str = r#"
spec finance
data margin_pct: ratio -> default 15%
data insurance_pct: ratio -> default 1.5%

rule total_rate: margin_pct + insurance_pct
rule margin_higher: margin_pct > insurance_pct
rule product: margin_pct * insurance_pct
rule tier: "low"
    unless margin_pct > 10% then "mid"
    unless margin_pct > 20% then "high"
"#;

#[test]
fn cross_type_add_preserves_canonical_sum() {
    let mut engine = Engine::new();
    load_ok(&mut engine, FINANCE_SPEC, "finance_add.lemma");

    let response = run_spec(&engine, "finance", HashMap::new());
    assert_ratio_exact(
        rule_value(&response, "total_rate"),
        "total_rate",
        "0.165",
        Some("percent"),
    );
}

#[test]
fn cross_type_add_result_carries_left_operand_type() {
    let mut engine = Engine::new();
    load_ok(&mut engine, FINANCE_SPEC, "finance_type.lemma");

    let response = run_spec(&engine, "finance", HashMap::new());
    let lit = rule_value(&response, "total_rate");
    assert_eq!(
        lit.lemma_type.name.as_deref(),
        Some("margin_pct"),
        "ratio + ratio must keep left named type"
    );
}

#[test]
fn cross_type_compare_with_defaults() {
    let mut engine = Engine::new();
    load_ok(&mut engine, FINANCE_SPEC, "finance_cmp.lemma");

    let response = run_spec(&engine, "finance", HashMap::new());
    assert_bool(
        rule_value(&response, "margin_higher"),
        "margin_higher",
        true,
    );
}

#[test]
fn cross_type_multiply_canonical() {
    let mut engine = Engine::new();
    load_ok(&mut engine, FINANCE_SPEC, "finance_mul.lemma");

    let response = run_spec(&engine, "finance", HashMap::new());
    assert_ratio_exact(
        rule_value(&response, "product"),
        "product",
        "0.00225",
        Some("percent"),
    );
}

#[test]
fn cross_type_unless_branches() {
    let mut engine = Engine::new();
    load_ok(&mut engine, FINANCE_SPEC, "finance_tier.lemma");

    let response = run_spec(&engine, "finance", HashMap::new());
    match &rule_value(&response, "tier").value {
        ValueKind::Text(s) => assert_eq!(s, "mid", "15% > 10% but not > 20%"),
        other => panic!("tier must be Text, got {other:?}"),
    }
}

#[test]
fn cross_type_runtime_override_changes_comparison() {
    let code = r#"
spec finance
data margin_pct: ratio -> default 15%
data insurance_pct: ratio -> default 1.5%
rule margin_higher: margin_pct > insurance_pct
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "finance_override.lemma");

    let mut data = HashMap::new();
    data.insert("margin_pct".to_string(), "1%".to_string());
    data.insert("insurance_pct".to_string(), "2%".to_string());

    let response = run_spec(&engine, "finance", data);
    assert_bool(
        rule_value(&response, "margin_higher"),
        "margin_higher",
        false,
    );
}

// -----------------------------------------------------------------------------
// Section 3 — Per-type `as` scoping and shared custom units
// -----------------------------------------------------------------------------

#[test]
fn own_type_as_percent_ok() {
    let code = r#"
spec s
data margin: ratio -> default 20%
data spread: ratio
  -> unit basis_points 10000
rule margin_as_pct: margin as percent
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "as_percent_ok.lemma");

    let response = run_spec(&engine, "s", HashMap::new());
    assert_ratio_exact(
        rule_value(&response, "margin_as_pct"),
        "margin_as_pct",
        "0.2",
        Some("percent"),
    );
}

#[test]
fn foreign_unit_as_basis_points_fails_plan() {
    let code = r#"
spec s
data margin: ratio -> default 20%
data spread: ratio
  -> unit basis_points 10000
rule bad: margin as basis_points
"#;
    expect_load_error(code, "foreign_bps.lemma", &["basis_points", "margin"]);
}

#[test]
fn shared_custom_unit_same_factor_two_types_number_as_ok() {
    let code = r#"
spec s
data spread: ratio
  -> unit basis_points 10000
data fee: ratio
  -> unit basis_points 10000
rule from_number: 500 basis_points
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "shared_bps.lemma");

    let response = run_spec(&engine, "s", HashMap::new());
    assert_ratio_exact(
        rule_value(&response, "from_number"),
        "from_number",
        "0.05",
        Some("basis_points"),
    );
}

#[test]
fn conflicting_basis_points_factor_errors_at_load() {
    let code = r#"
spec s
data spread_a: ratio
  -> unit basis_points 10000
data spread_b: ratio
  -> unit basis_points 5000
rule out: spread_a
"#;
    expect_load_error(
        code,
        "conflict_bps.lemma",
        &["spread_a", "spread_b", "basis_points"],
    );
}

#[test]
fn redefining_builtin_percent_with_different_factor_errors() {
    let code = r#"
spec s
data margin: ratio
data custom: ratio
  -> unit percent 50
rule out: margin
"#;
    expect_load_error(
        code,
        "redefine_percent.lemma",
        &["percent", "cannot change factor", "inherited"],
    );
}

#[test]
fn mixed_match_and_mismatch_factors_errors_on_first_mismatch() {
    let code = r#"
spec s
data spread_a: ratio
  -> unit basis_points 10000
  -> unit ten_thousandth 10000
data spread_b: ratio
  -> unit basis_points 10000
  -> unit ten_thousandth 5000
rule out: spread_a
"#;
    expect_load_error(
        code,
        "mixed_match_mismatch.lemma",
        &["spread_a", "spread_b", "ten_thousandth"],
    );
}

#[test]
fn three_types_third_introduces_factor_conflict() {
    let code = r#"
spec s
data a: ratio
  -> unit thirds 3
data b: ratio
  -> unit thirds 3
data c: ratio
  -> unit thirds 6
rule out: a
"#;
    expect_load_error(code, "three_types_conflict.lemma", &["thirds"]);
}

#[test]
fn three_types_third_introduces_factor_conflict_reordered() {
    let code = r#"
spec s
data c: ratio
  -> unit thirds 6
data a: ratio
  -> unit thirds 3
data b: ratio
  -> unit thirds 3
rule out: a
"#;
    expect_load_error(code, "three_types_conflict_reordered.lemma", &["thirds"]);
}

// -----------------------------------------------------------------------------
// Section 4 — Anonymous / expression ratios
// -----------------------------------------------------------------------------

#[test]
fn division_as_percent_is_ratio_not_number() {
    let code = r#"
spec calc
data part: 75
data whole: 300
rule savings_ratio: (part / whole) as percent
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "anon_div.lemma");

    let response = run_spec(&engine, "calc", HashMap::new());
    assert_ratio_exact(
        rule_value(&response, "savings_ratio"),
        "savings_ratio",
        "0.25",
        Some("percent"),
    );
}

#[test]
fn literal_percent_in_rule_is_ratio() {
    let code = r#"
spec s
rule r: 15%
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "literal_pct.lemma");

    let response = run_spec(&engine, "s", HashMap::new());
    assert_ratio_exact(rule_value(&response, "r"), "r", "0.15", Some("percent"));
}

#[test]
fn anonymous_ratio_arithmetic_in_multi_ratio_spec() {
    let code = r#"
spec finance
data margin_pct: ratio -> default 15%
data insurance_pct: ratio -> default 1.5%
data part: 10
data whole: 40

rule pct: (part / whole) as percent
rule plus_five: pct + 5%
rule compared: plus_five > 25%
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "anon_arith.lemma");

    let response = run_spec(&engine, "finance", HashMap::new());
    assert_ratio_exact(rule_value(&response, "pct"), "pct", "0.25", Some("percent"));
    assert_ratio_exact(
        rule_value(&response, "plus_five"),
        "plus_five",
        "0.30",
        Some("percent"),
    );
    assert_bool(rule_value(&response, "compared"), "compared", true);
}

#[test]
fn number_as_percent_with_two_named_ratio_fields_present() {
    let code = r#"
spec finance
data margin_pct: ratio -> default 15%
data insurance_pct: ratio -> default 1.5%
rule anon: 0.25 as percent
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "number_as_pct.lemma");

    let response = run_spec(&engine, "finance", HashMap::new());
    let lit = rule_value(&response, "anon");
    assert_ratio_exact(lit, "anon", "0.25", Some("percent"));
    assert!(
        lit.lemma_type.is_ratio(),
        "result must remain ratio-typed, got {:?}",
        lit.lemma_type.specifications
    );
}

#[test]
fn ratio_display_none_vs_percent_unit() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    let display = bare.display_value();
    assert!(
        !display.contains("percent") && display.contains("0.5"),
        "ratio without unit must not show percent label, got: {display}"
    );

    let tagged = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), Some("percent".to_string()));
    let display_tagged = tagged.display_value();
    assert!(
        display_tagged.contains('%'),
        "ratio with percent unit must show %, got: {display_tagged}"
    );
}

// -----------------------------------------------------------------------------
// Section 5 — Ratio ranges with multiple ratio data fields
// -----------------------------------------------------------------------------

fn assert_range_endpoints_canonical_ratio(
    lit: &LiteralValue,
    ctx: &str,
    expected_left_canonical: &str,
    expected_left_unit: Option<&str>,
    expected_right_canonical: &str,
    expected_right_unit: Option<&str>,
) {
    let (left, right) = match &lit.value {
        ValueKind::Range(left, right) => (left.as_ref(), right.as_ref()),
        other => panic!("{ctx}: expected Range, got {other:?}"),
    };
    assert!(
        left.lemma_type.is_ratio(),
        "{ctx}: left endpoint lemma_type must be ratio, got {} (specs={:?})",
        left.lemma_type.name(),
        left.lemma_type.specifications,
    );
    assert!(
        right.lemma_type.is_ratio(),
        "{ctx}: right endpoint lemma_type must be ratio, got {} (specs={:?})",
        right.lemma_type.name(),
        right.lemma_type.specifications,
    );
    assert_ratio_exact(
        left,
        &format!("{ctx} left"),
        expected_left_canonical,
        expected_left_unit,
    );
    assert_ratio_exact(
        right,
        &format!("{ctx} right"),
        expected_right_canonical,
        expected_right_unit,
    );
}

#[test]
fn ratio_range_default_with_percent_endpoints_canonical() {
    let code = r#"
spec policy
data allowed_band: ratio range -> default 10%...50%
rule band: allowed_band
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "ratio_range_default_pct.lemma");

    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "policy", Some(&now))
        .expect("plan must build with ratio range default");
    let path = lemma::DataPath::local("allowed_band".into());
    let def = plan.data.get(&path).expect("allowed_band in plan.data");
    let suggestion = def
        .default_suggestion()
        .expect("ratio range typedef must surface declared default");
    assert!(
        matches!(
            &suggestion.lemma_type.specifications,
            TypeSpecification::RatioRange { .. }
        ),
        "default suggestion lemma_type must be RatioRange, got {:?}",
        suggestion.lemma_type.specifications
    );
    assert_range_endpoints_canonical_ratio(
        &suggestion,
        "ratio_range_default percent",
        "0.10",
        Some("percent"),
        "0.50",
        Some("percent"),
    );
}

#[test]
fn ratio_range_default_with_basis_points_endpoints_canonical() {
    let code = r#"
spec policy
data allowed_band: ratio range
  -> unit basis_points 10000
  -> default 200 basis_points...3500 basis_points
rule band: allowed_band
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "ratio_range_default_bps.lemma");

    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "policy", Some(&now))
        .expect("plan must build with custom-unit ratio range default");
    let path = lemma::DataPath::local("allowed_band".into());
    let def = plan.data.get(&path).expect("allowed_band in plan.data");
    let suggestion = def
        .default_suggestion()
        .expect("ratio range typedef must surface declared default");
    assert_range_endpoints_canonical_ratio(
        &suggestion,
        "ratio_range_default basis_points",
        "0.02",
        Some("basis_points"),
        "0.35",
        Some("basis_points"),
    );
}

#[test]
fn ratio_range_default_runtime_uses_canonical_endpoints() {
    let code = r#"
spec policy
data allowed_band: ratio range -> default 10%...50%
data candidate: ratio -> default 25%
rule in_default_band: candidate in allowed_band
rule out_of_band: 5% in allowed_band
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "ratio_range_default_runtime.lemma");

    let response = run_spec(&engine, "policy", HashMap::new());
    assert_bool(
        rule_value(&response, "in_default_band"),
        "in_default_band",
        true,
    );
    assert_bool(rule_value(&response, "out_of_band"), "out_of_band", false);
}

#[test]
fn ratio_range_default_endpoints_must_be_ratio_not_measure() {
    let code = r#"
spec policy
data allowed_band: ratio range -> default 10%...50%
rule band: allowed_band
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "ratio_range_endpoint_typing.lemma");

    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "policy", Some(&now)).expect("plan");
    let path = lemma::DataPath::local("allowed_band".into());
    let def = plan.data.get(&path).expect("allowed_band in plan.data");
    let suggestion = def
        .default_suggestion()
        .expect("declared default must exist");

    let (left, right) = match &suggestion.value {
        ValueKind::Range(l, r) => (l.as_ref(), r.as_ref()),
        other => panic!("expected Range, got {other:?}"),
    };
    for (label, endpoint) in [("left", left), ("right", right)] {
        assert!(
            !matches!(
                &endpoint.lemma_type.specifications,
                TypeSpecification::Measure { .. }
            ),
            "{label} endpoint must not be lifted as Measure for a percent literal in a ratio range default",
        );
        assert!(
            matches!(&endpoint.value, ValueKind::Ratio(_, _)),
            "{label} endpoint ValueKind must be Ratio (got {:?})",
            endpoint.value
        );
    }
}

#[test]
fn ratio_range_typedef_with_second_ratio_field_loads() {
    let code = r#"
spec policy
data margin_pct: ratio -> default 15%
data allowed_band: ratio range
rule margin: margin_pct
rule band_slot: allowed_band
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "ratio_range_load.lemma");

    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "policy", Some(&now)).expect("plan");
    let path = lemma::DataPath::local("allowed_band".into());
    let def = plan.data.get(&path).expect("allowed_band in plan.data");
    let lemma_type = def
        .schema_type()
        .expect("allowed_band must be a typed data slot");
    match &lemma_type.specifications {
        TypeSpecification::RatioRange { units, .. } => {
            let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            assert!(
                names.contains(&"percent"),
                "ratio range must inherit builtin percent, got {names:?}"
            );
        }
        other => panic!("allowed_band must be RatioRange, got {other:?}"),
    }
}

#[test]
fn margin_in_ratio_range_literal() {
    let code = r#"
spec policy
data margin_pct: ratio -> default 15%
data insurance_pct: ratio -> default 1.5%
rule in_band: margin_pct in 10%...50%
rule below: margin_pct in 0%...10%
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "in_band.lemma");

    let response = run_spec(&engine, "policy", HashMap::new());
    assert_bool(rule_value(&response, "in_band"), "in_band", true);
    assert_bool(rule_value(&response, "below"), "below", false);
}

// -----------------------------------------------------------------------------
// Section 6 — Schema and runtime input
// -----------------------------------------------------------------------------

#[test]
fn runtime_input_each_ratio_field_independent() {
    let mut engine = Engine::new();
    load_ok(&mut engine, TARGETS_SPEC, "targets_runtime.lemma");

    let mut data = HashMap::new();
    data.insert("standard_margin_pct".to_string(), "20%".to_string());
    data.insert("default_credit_insurance_pct".to_string(), "2%".to_string());

    let response = run_spec(&engine, "targets", data);
    assert_ratio_exact(
        rule_value(&response, "margin"),
        "margin runtime",
        "0.2",
        Some("percent"),
    );
    assert_ratio_exact(
        rule_value(&response, "insurance"),
        "insurance runtime",
        "0.02",
        Some("percent"),
    );
}

#[test]
fn schema_declared_defaults_canonical_for_targets() {
    let mut engine = Engine::new();
    load_ok(&mut engine, TARGETS_SPEC, "targets_defaults.lemma");

    let now = DateTimeValue::now();
    let schema = engine.schema(None, "targets", Some(&now)).expect("schema");

    let margin = schema.data.get("standard_margin_pct").expect("margin");
    let default = margin.default.as_ref().expect("margin declared default");
    assert_ratio_exact(default, "margin schema default", "0.15", Some("percent"));

    let insurance = schema
        .data
        .get("default_credit_insurance_pct")
        .expect("insurance");
    let ins_default = insurance
        .default
        .as_ref()
        .expect("insurance declared default");
    assert_ratio_exact(
        ins_default,
        "insurance schema default",
        "0.015",
        Some("percent"),
    );
}

// -----------------------------------------------------------------------------
// Section 7 — Discount arithmetic across named ratio types
// -----------------------------------------------------------------------------

#[test]
fn number_minus_other_ratio_field_canonical() {
    let code = r#"
spec pricing
data discount: ratio -> default 10%
data surcharge: ratio -> default 5%
data base: 100

rule after_discount: base * (100% - discount)
rule after_surcharge: base * (100% + surcharge)
"#;
    let mut engine = Engine::new();
    load_ok(&mut engine, code, "pricing_mix.lemma");

    let response = run_spec(&engine, "pricing", HashMap::new());
    assert_number_exact(
        rule_value(&response, "after_discount"),
        "after_discount",
        "90",
    );
    assert_number_exact(
        rule_value(&response, "after_surcharge"),
        "after_surcharge",
        "105",
    );
}
