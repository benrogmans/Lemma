//! API wire format contract tests (sections A–J).
//!
//! API serialize path: [`Show`] JSON and [`Response`] JSON — same paths
//! [`engine::wasm`] and show export use (`serde_json` on those types).
//! Plan persistence tests live in `execution_plan` unit tests (section I).
//!
//! Section F additionally requires that every show-default per-unit magnitude from
//! section E (`magnitude_in_unit`, API wire `measure` / `ratio` maps) is submittable
//! as convenience input through [`Engine::run`] without computation veto — same path
//! as the CLI interactive trial. See `documentation/learn/precision.md`.

use lemma::{DateTimeValue, Engine, LiteralValue, Show, ValueKind};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

fn decimal_lit(s: &str) -> Decimal {
    Decimal::from_str(s).expect("valid decimal literal in test")
}

fn path_source(file: &str) -> lemma::SourceType {
    lemma::SourceType::Path(Arc::new(PathBuf::from(file)))
}

fn load_engine(code: &str, file: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .load([(path_source(file), code.to_string())])
        .expect("spec must load");
    engine
}

const COST_PRICE_SPEC: &str = r#"
spec cost_price
uses lemma units

data money: measure
  -> unit eur 1.00
  -> unit inr 0.0092
  -> decimals 2

data labor_cost: measure
  -> unit eur_per_hour eur/hour
  -> unit inr_per_hour inr/hour
  -> suggest 25 eur_per_hour

data product_cost: measure
  -> unit eur_per_kg eur/kilogram
  -> unit inr_per_kg inr/kilogram
  -> suggest 4 eur_per_kg

data throughput: measure
  -> unit kg_per_hour kilogram/hour
  -> suggest 12 kg_per_hour

rule cost_price: product_cost + labor_cost / throughput
"#;

const POLICY_RATIO_SPEC: &str = r#"
spec policy
data margin: ratio -> suggest 15%
data bps: ratio
  -> unit basis_points 10000
  -> suggest 500 basis_points
data permille_rate: ratio -> suggest 150 permille
rule m: margin
rule bps_val: bps
rule permille_val: permille_rate
"#;

fn cost_price_engine() -> Engine {
    load_engine(COST_PRICE_SPEC, "cost_price.lemma")
}

fn policy_engine() -> Engine {
    load_engine(POLICY_RATIO_SPEC, "policy.lemma")
}

fn plan_interface_show(engine: &Engine, spec: &str) -> Show {
    let now = DateTimeValue::now();
    engine.show(None, spec, Some(&now)).expect("show")
}

fn show_default_literal(engine: &Engine, spec: &str, data_name: &str) -> LiteralValue {
    plan_interface_show(engine, spec)
        .data
        .get(data_name)
        .unwrap_or_else(|| panic!("{data_name} missing from show"))
        .suggestion
        .clone()
        .unwrap_or_else(|| panic!("{data_name} has no show suggestion"))
}

/// API wire: embed `literal` in a [`Show`] `suggestion` field and serialize (wasm show path).
fn api_wire_json_for_literal(name: &str, literal: &LiteralValue) -> serde_json::Value {
    let mut data = indexmap::IndexMap::new();
    data.insert(
        name.to_string(),
        lemma::DataEntry {
            lemma_type: literal.lemma_type.as_ref().clone(),
            prefilled: None,
            suggestion: Some(literal.clone()),
            needed_by_rules: Vec::new(),
        },
    );
    let show = Show {
        spec: "wire_test".to_string(),
        commentary: None,
        effective_from: None,
        effective_to: None,
        versions: Vec::new(),
        start_line: 1,
        source_type: None,
        data,
        rules: indexmap::IndexMap::new(),
        meta: HashMap::new(),
    };
    serde_json::to_value(&show).expect("Show API JSON must serialize")["data"][name]["suggestion"]
        .clone()
}

fn api_show_default_json(engine: &Engine, spec: &str, data_name: &str) -> serde_json::Value {
    let lit = show_default_literal(engine, spec, data_name);
    api_wire_json_for_literal(data_name, &lit)
}

fn json_ratio_wire(json: &serde_json::Value) -> (String, Option<String>) {
    json_ratio_wire_valuekind(&json["value"])
}

fn json_ratio_wire_valuekind(valuekind: &serde_json::Value) -> (String, Option<String>) {
    let value = valuekind["ratio"]["value"]
        .as_str()
        .expect("ratio.value string in API JSON");
    let unit = valuekind["ratio"]["unit"].as_str().map(String::from);
    let unit = unit.filter(|u| !u.is_empty());
    (value.to_string(), unit)
}

fn json_measure_wire(json: &serde_json::Value) -> String {
    json["value"]["measure"]["value"]
        .as_str()
        .expect("measure.value string in API JSON")
        .to_string()
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
                ValueKind::Number(r.clone())
                    .as_decimal_magnitude()
                    .expect("ratio magnitude"),
                decimal_lit(expected_canonical),
                "{ctx}: canonical magnitude"
            );
            assert_eq!(u.as_deref(), expected_unit, "{ctx}: unit tag");
        }
        other => panic!("{ctx}: expected Ratio, got {other:?}"),
    }
}

fn show_literal_api_json(engine: &Engine, spec: &str, data_name: &str) -> serde_json::Value {
    let show = plan_interface_show(engine, spec);
    let entry = show
        .data
        .get(data_name)
        .unwrap_or_else(|| panic!("{data_name} missing from show.data"));
    let lit = entry
        .prefilled
        .clone()
        .or_else(|| entry.suggestion.clone())
        .unwrap_or_else(|| panic!("{data_name} has no prefilled or suggestion in show.data"));
    api_wire_json_for_literal(data_name, &lit)
}

fn deserialize_api_wire_literal(json: serde_json::Value) -> LiteralValue {
    let lemma_type = json["lemma_type"].clone();
    let entry: lemma::DataEntry = serde_json::from_value(serde_json::json!({
        "type": lemma_type,
        "suggestion": json,
        "needed_by_rules": [],
    }))
    .expect("API wire literal must deserialize via DataEntry");
    entry
        .suggestion
        .expect("default literal present in API wire JSON")
}

// --- A: In-memory unchanged ---

#[test]
fn in_memory_ratio_percent_default_is_canonical() {
    let default = show_default_literal(&policy_engine(), "policy", "margin");
    assert_ratio_exact(&default, "margin default", "0.15", Some("percent"));
}

#[test]
fn in_memory_ratio_basis_points_default_is_canonical() {
    let default = show_default_literal(&policy_engine(), "policy", "bps");
    assert_ratio_exact(&default, "bps default", "0.05", Some("basis_points"));
}

#[test]
fn in_memory_measure_eur_per_hour_default_is_canonical() {
    let default = show_default_literal(&cost_price_engine(), "cost_price", "labor_cost");
    match &default.value {
        ValueKind::Measure(r, sig) => {
            assert_eq!(sig, &vec![("eur_per_hour".to_string(), 1)]);
            let magnitude = ValueKind::Number(r.clone())
                .as_decimal_magnitude()
                .expect("magnitude");
            let expected = decimal_lit("0.0069444444444444444444444444");
            assert_eq!(magnitude, expected, "labor_cost canonical magnitude");
        }
        other => panic!("expected Measure, got {other:?}"),
    }
}

#[test]
fn in_memory_bare_ratio_is_canonical_0_5() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    assert_ratio_exact(&bare, "bare ratio", "0.5", None);
}

// --- B: Ratio API wire serialize ---

#[test]
fn ratio_api_wire_bare_0_5() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    let json = api_wire_json_for_literal("bare", &bare);
    let (value, unit) = json_ratio_wire(&json);
    assert_eq!(value, "0.5");
    assert_eq!(unit, None);
}

#[test]
fn ratio_api_wire_bare_0_15() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.15"), None);
    let json = api_wire_json_for_literal("bare", &bare);
    let (value, unit) = json_ratio_wire(&json);
    assert_eq!(value, "0.15");
    assert_eq!(unit, None);
}

#[test]
fn ratio_api_wire_percent_15() {
    let json = api_show_default_json(&policy_engine(), "policy", "margin");
    let (value, unit) = json_ratio_wire(&json);
    assert_eq!(value, "15");
    assert_eq!(unit.as_deref(), Some("percent"));
}

#[test]
fn ratio_api_wire_permille_150() {
    let json = api_show_default_json(&policy_engine(), "policy", "permille_rate");
    let (value, unit) = json_ratio_wire(&json);
    assert_eq!(value, "150");
    assert_eq!(unit.as_deref(), Some("permille"));
}

#[test]
fn ratio_api_wire_basis_points_500() {
    let json = api_show_default_json(&policy_engine(), "policy", "bps");
    let (value, unit) = json_ratio_wire(&json);
    assert_eq!(value, "500");
    assert_eq!(unit.as_deref(), Some("basis_points"));
}

// --- C: Ratio API wire deserialize / accept ---

#[test]
fn ratio_api_wire_bare_0_5_accepted() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    let json = api_wire_json_for_literal("bare", &bare);
    let roundtrip: LiteralValue = deserialize_api_wire_literal(json);
    assert_ratio_exact(&roundtrip, "bare deserialize", "0.5", None);
}

#[test]
fn ratio_api_wire_bare_0_5_roundtrip() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    let json = api_wire_json_for_literal("bare", &bare);
    let roundtrip: LiteralValue = deserialize_api_wire_literal(json);
    assert_eq!(roundtrip.value, bare.value);
}

#[test]
fn ratio_api_wire_percent_roundtrip() {
    let original = show_default_literal(&policy_engine(), "policy", "margin");
    let json = api_wire_json_for_literal("margin", &original);
    let roundtrip: LiteralValue = deserialize_api_wire_literal(json);
    assert_ratio_exact(&roundtrip, "percent roundtrip", "0.15", Some("percent"));
}

#[test]
fn ratio_api_wire_basis_points_roundtrip() {
    let original = show_default_literal(&policy_engine(), "policy", "bps");
    let json = api_wire_json_for_literal("bps", &original);
    let roundtrip: LiteralValue = deserialize_api_wire_literal(json);
    assert_ratio_exact(&roundtrip, "bps roundtrip", "0.05", Some("basis_points"));
}

#[test]
fn ratio_api_wire_unit_tag_controls_scaling() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    let mut with_unit = api_wire_json_for_literal("scaled", &bare);
    with_unit["value"]["ratio"]["unit"] = serde_json::Value::String("percent".into());
    let roundtrip: LiteralValue = deserialize_api_wire_literal(with_unit);
    assert_ratio_exact(&roundtrip, "0.5 as percent wire", "0.005", Some("percent"));

    let without_unit = api_wire_json_for_literal("bare", &bare);
    let roundtrip_bare: LiteralValue = deserialize_api_wire_literal(without_unit);
    assert_ratio_exact(&roundtrip_bare, "0.5 bare wire", "0.5", None);
}

// --- D: Prompt matches wire ---

#[test]
fn ratio_prompt_bare_0_5() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    assert_eq!(
        bare.magnitude_suggestion_for_decimal_prompt().as_deref(),
        Some("0.5")
    );
}

#[test]
fn ratio_prompt_percent_15() {
    let default = show_default_literal(&policy_engine(), "policy", "margin");
    assert_eq!(
        default.magnitude_suggestion_for_decimal_prompt().as_deref(),
        Some("15")
    );
}

#[test]
fn ratio_prompt_basis_points_500() {
    let default = show_default_literal(&policy_engine(), "policy", "bps");
    assert_eq!(
        default.magnitude_suggestion_for_decimal_prompt().as_deref(),
        Some("500")
    );
}

#[test]
fn measure_prompt_eur_per_hour_25() {
    let default = show_default_literal(&cost_price_engine(), "cost_price", "labor_cost");
    assert_eq!(
        default.magnitude_suggestion_for_decimal_prompt().as_deref(),
        Some("25")
    );
}

// --- E: Measure API wire ---

#[test]
fn measure_api_wire_eur_per_hour_25() {
    let json = api_show_default_json(&cost_price_engine(), "cost_price", "labor_cost");
    assert_eq!(json_measure_wire(&json), "25");
}

#[test]
fn measure_api_wire_kg_per_hour_12() {
    let json = api_show_default_json(&cost_price_engine(), "cost_price", "throughput");
    assert_eq!(json_measure_wire(&json), "12");
}

#[test]
fn measure_show_default_includes_all_declared_units() {
    let default = show_default_literal(&cost_price_engine(), "cost_price", "labor_cost");
    let json = api_wire_json_for_literal("labor_cost", &default);
    let measure = json["measure"]
        .as_object()
        .expect("labor_cost default must include measure unit map");
    assert_eq!(
        measure["eur_per_hour"].as_str(),
        Some("25"),
        "eur_per_hour magnitude"
    );
    let inr = measure["inr_per_hour"]
        .as_str()
        .expect("inr_per_hour must be materialized");
    assert_ne!(inr, "25", "inr_per_hour must differ from eur magnitude");
    assert_eq!(
        default.magnitude_in_unit("inr_per_hour").as_deref(),
        Some(inr),
        "magnitude_in_unit must match wire map"
    );
}

#[test]
fn ratio_show_default_includes_all_declared_units() {
    let default = show_default_literal(&policy_engine(), "policy", "bps");
    let json = api_wire_json_for_literal("bps", &default);
    let ratio = json["ratio"]
        .as_object()
        .expect("bps default must include ratio unit map");
    assert_eq!(
        ratio["basis_points"].as_str(),
        Some("500"),
        "basis_points magnitude"
    );
    assert_eq!(
        default.magnitude_in_unit("basis_points").as_deref(),
        Some("500"),
        "magnitude_in_unit must match wire map"
    );
}

#[test]
fn measure_api_wire_roundtrip_canonical() {
    let original = show_default_literal(&cost_price_engine(), "cost_price", "labor_cost");
    let json = api_wire_json_for_literal("labor_cost", &original);
    let roundtrip: LiteralValue = deserialize_api_wire_literal(json);
    assert_eq!(roundtrip.value, original.value);
}

#[test]
fn measure_prompt_matches_api_wire() {
    let show = plan_interface_show(&cost_price_engine(), "cost_price");
    for (name, wire) in [
        ("labor_cost", "25"),
        ("throughput", "12"),
        ("product_cost", "4"),
    ] {
        let default = show
            .data
            .get(name)
            .unwrap_or_else(|| panic!("{name}"))
            .suggestion
            .as_ref()
            .unwrap_or_else(|| panic!("{name} default"));
        assert_eq!(
            default.magnitude_suggestion_for_decimal_prompt().as_deref(),
            Some(wire),
            "{name} prompt must match API wire"
        );
        assert_eq!(
            json_measure_wire(&api_wire_json_for_literal(name, default)),
            wire,
            "{name} API wire must match"
        );
    }
}

// --- F: End-to-end consumer ---

fn cost_price_run_inputs() -> HashMap<String, String> {
    HashMap::from([
        ("product_cost".into(), "4 eur_per_kg".into()),
        ("labor_cost".into(), "25 eur_per_hour".into()),
        ("throughput".into(), "12 kg_per_hour".into()),
    ])
}

fn run_cost_price(engine: &Engine, now: &DateTimeValue) -> lemma::Response {
    engine
        .run(
            None,
            "cost_price",
            Some(now),
            cost_price_run_inputs(),
            None,
            true,
        )
        .expect("evaluation")
}

#[test]
fn measure_eval_per_unit_inputs_ok() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    run_cost_price(&engine, &now);
}

fn run_cost_price_with_single_override(
    engine: &Engine,
    now: &DateTimeValue,
    data_name: &str,
    convenience: &str,
    rule_names: Option<&[&str]>,
    explain: bool,
) -> lemma::Response {
    let mut data = cost_price_run_inputs();
    data.insert(data_name.to_string(), convenience.to_string());
    let rules: Option<Vec<String>> =
        rule_names.map(|names| names.iter().map(|name| (*name).to_string()).collect());
    engine
        .run(
            None,
            "cost_price",
            Some(now),
            data,
            rules.as_deref(),
            explain,
        )
        .expect("evaluation must complete")
}

fn assert_cost_price_rule_not_vetoed(response: &lemma::Response, context: &str) {
    let rule = response
        .results
        .get("cost_price")
        .unwrap_or_else(|| panic!("{context}: cost_price rule must be present"));
    assert!(
        !rule.vetoed,
        "{context}: cost_price must not veto, got {:?}",
        rule.veto_reason
    );
    assert_ne!(
        rule.veto_reason.as_deref(),
        Some("Calculated result exceeds decimal value limit"),
        "{context}: must not veto for decimal limit"
    );
    assert!(
        rule.display.is_some(),
        "{context}: cost_price must produce a committable display value"
    );
}

#[test]
fn measure_show_default_inr_per_hour_convenience_input_evaluates() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let default = show_default_literal(&engine, "cost_price", "labor_cost");
    let magnitude = default
        .magnitude_in_unit("inr_per_hour")
        .expect("section E guarantees inr_per_hour materialization");
    let input = format!("{magnitude} inr_per_hour");
    let response = run_cost_price_with_single_override(
        &engine,
        &now,
        "labor_cost",
        &input,
        Some(&["cost_price"]),
        false,
    );
    assert_cost_price_rule_not_vetoed(
        &response,
        "show default inr_per_hour magnitude as convenience input",
    );
}

#[test]
fn measure_show_default_each_declared_unit_convenience_input_evaluates() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let cases = [
        ("labor_cost", "eur_per_hour"),
        ("labor_cost", "inr_per_hour"),
        ("product_cost", "eur_per_kg"),
        ("product_cost", "inr_per_kg"),
        ("throughput", "kg_per_hour"),
    ];
    for (data_name, unit) in cases {
        let default = show_default_literal(&engine, "cost_price", data_name);
        let magnitude = default
            .magnitude_in_unit(unit)
            .unwrap_or_else(|| panic!("{data_name} must materialize for unit {unit}"));
        let input = format!("{magnitude} {unit}");
        let response = run_cost_price_with_single_override(
            &engine,
            &now,
            data_name,
            &input,
            Some(&["cost_price"]),
            false,
        );
        assert_cost_price_rule_not_vetoed(
            &response,
            &format!("{data_name} show default in {unit} as convenience input"),
        );
    }
}

#[test]
fn measure_show_default_inr_per_hour_not_overprecision_string() {
    let default = show_default_literal(&cost_price_engine(), "cost_price", "labor_cost");
    let materialized = default
        .magnitude_in_unit("inr_per_hour")
        .expect("inr_per_hour must be materialized");
    let overprecision = "2717.3913043478260869565217391";
    assert_ne!(
        materialized, overprecision,
        "show default must not emit unbounded output precision as convenience input"
    );
}

#[test]
fn ratio_show_default_basis_points_convenience_input_evaluates() {
    let engine = policy_engine();
    let now = DateTimeValue::now();
    let default = show_default_literal(&engine, "policy", "bps");
    let magnitude = default
        .magnitude_in_unit("basis_points")
        .expect("section E guarantees basis_points materialization");
    assert_eq!(magnitude, "500");
    let mut data = HashMap::new();
    data.insert("bps".into(), format!("{magnitude} basis_points"));
    let response = engine
        .run(
            None,
            "policy",
            Some(&now),
            data,
            Some(&["bps_val".to_string()]),
            false,
        )
        .expect("evaluation must complete");
    let rule = response.results.get("bps_val").expect("bps_val rule");
    assert!(
        !rule.vetoed,
        "bps_val must not veto, got {:?}",
        rule.veto_reason
    );
}

#[test]
fn measure_show_literal_api_wire_after_run() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    run_cost_price(&engine, &now);
    assert_eq!(
        json_measure_wire(&show_literal_api_json(&engine, "cost_price", "labor_cost")),
        "25"
    );
    assert_eq!(
        json_measure_wire(&show_literal_api_json(&engine, "cost_price", "throughput")),
        "12"
    );
}

#[test]
fn measure_response_json_serializes() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let response = run_cost_price(&engine, &now);
    serde_json::to_string(&response).expect("response API JSON must serialize");
}

#[test]
fn ratio_eval_15_percent_ok() {
    let engine = policy_engine();
    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("margin".into(), "15%".to_string());
    let response = engine
        .run(None, "policy", Some(&now), data, None, true)
        .expect("evaluation");
    let rr = response.results.get("m").expect("rule m");
    assert!(!rr.vetoed, "rule must not veto");
    let lit = rr
        .explanation
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("result value");
    assert_ratio_exact(lit, "rule m", "0.15", Some("percent"));
}

#[test]
fn ratio_show_suggestion_api_wire_percent() {
    let engine = policy_engine();
    let (value, _) = json_ratio_wire(&show_literal_api_json(&engine, "policy", "margin"));
    assert_eq!(value, "15");
}

#[test]
fn ratio_rule_result_bare_0_5_api_wire() {
    let bare = LiteralValue::ratio_from_decimal(decimal_lit("0.5"), None);
    let json = api_wire_json_for_literal("half", &bare);
    let (value, unit) = json_ratio_wire(&json);
    assert_eq!(value, "0.5");
    assert_eq!(unit, None);
    let roundtrip = deserialize_api_wire_literal(json);
    assert_ratio_exact(&roundtrip, "bare API roundtrip", "0.5", None);
}

// --- H: Range API wire ---

const RATIO_RANGE_PERCENT_SPEC: &str = r#"
spec policy
data allowed_band: ratio range -> suggest 10%...50%
rule band: allowed_band
"#;

const RATIO_RANGE_BPS_SPEC: &str = r#"
spec policy
data allowed_band: ratio range
  -> unit basis_points 10000
  -> suggest 200 basis_points...3500 basis_points
rule band: allowed_band
"#;

fn range_endpoint_ratio_wire(json: &serde_json::Value, side: &str) -> (String, Option<String>) {
    json_ratio_wire_valuekind(&json["value"]["range"][side])
}

#[test]
fn ratio_range_api_wire_percent_endpoints() {
    let engine = load_engine(RATIO_RANGE_PERCENT_SPEC, "ratio_range_pct.lemma");
    let default = show_default_literal(&engine, "policy", "allowed_band");
    let json = api_wire_json_for_literal("allowed_band", &default);
    let (left, left_unit) = range_endpoint_ratio_wire(&json, "from");
    let (right, right_unit) = range_endpoint_ratio_wire(&json, "to");
    assert_eq!(left, "10");
    assert_eq!(left_unit.as_deref(), Some("percent"));
    assert_eq!(right, "50");
    assert_eq!(right_unit.as_deref(), Some("percent"));
}

#[test]
fn ratio_range_api_wire_basis_points_endpoints() {
    let engine = load_engine(RATIO_RANGE_BPS_SPEC, "ratio_range_bps.lemma");
    let default = show_default_literal(&engine, "policy", "allowed_band");
    let json = api_wire_json_for_literal("allowed_band", &default);
    let (left, _) = range_endpoint_ratio_wire(&json, "from");
    let (right, _) = range_endpoint_ratio_wire(&json, "to");
    assert_eq!(left, "200");
    assert_eq!(right, "3500");
}

#[test]
fn ratio_range_api_wire_roundtrip_canonical() {
    let engine = load_engine(RATIO_RANGE_PERCENT_SPEC, "ratio_range_pct.lemma");
    let original = show_default_literal(&engine, "policy", "allowed_band");
    let json = api_wire_json_for_literal("allowed_band", &original);
    let roundtrip: LiteralValue = deserialize_api_wire_literal(json);
    match (&original.value, &roundtrip.value) {
        (ValueKind::Range(l0, r0), ValueKind::Range(l1, r1)) => {
            assert_eq!(l0.value, l1.value);
            assert_eq!(r0.value, r1.value);
        }
        _ => panic!("expected range values"),
    }
}
