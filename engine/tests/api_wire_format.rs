//! API wire format contract tests (sections A–J).
//!
//! API serialize path: [`SpecSchema`] JSON and [`Response`] JSON — same paths
//! [`engine::wasm`] and schema export use (`serde_json` on those types).
//! Plan persistence uses [`ExecutionPlanSerialized`] separately (section I).

use lemma::{
    BindingDataValue, DataOverlay, DataPath, DataValueInput, DateTimeValue, Engine, ExecutionPlan,
    ExecutionPlanSerialized, LiteralValue, ResourceLimits, SpecSchema, ValueKind,
};
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
        .load(code, path_source(file))
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
  -> default 25 eur_per_hour

data product_cost: measure
  -> unit eur_per_kg eur/kilogram
  -> unit inr_per_kg inr/kilogram
  -> default 4 eur_per_kg

data throughput: measure
  -> unit kg_per_hour kilogram/hour
  -> default 12 kg_per_hour

rule cost_price: product_cost + labor_cost / throughput
"#;

const POLICY_RATIO_SPEC: &str = r#"
spec policy
data margin: ratio -> default 15%
data bps: ratio
  -> unit basis_points 10000
  -> default 500 basis_points
data permille_rate: ratio -> default 150 permille
rule m: margin
"#;

fn cost_price_engine() -> Engine {
    load_engine(COST_PRICE_SPEC, "cost_price.lemma")
}

fn policy_engine() -> Engine {
    load_engine(POLICY_RATIO_SPEC, "policy.lemma")
}

fn plan_interface_schema(engine: &Engine, spec: &str) -> SpecSchema {
    let now = DateTimeValue::now();
    engine
        .get_plan(None, spec, Some(&now))
        .expect("plan")
        .interface_schema(&DataOverlay::default())
}

fn schema_default_literal(engine: &Engine, spec: &str, data_name: &str) -> LiteralValue {
    plan_interface_schema(engine, spec)
        .data
        .get(data_name)
        .unwrap_or_else(|| panic!("{data_name} missing from schema"))
        .default
        .clone()
        .unwrap_or_else(|| panic!("{data_name} has no schema default"))
}

/// API wire: embed `literal` in a [`SpecSchema`] `default` field and serialize (wasm schema path).
fn api_wire_json_for_literal(name: &str, literal: &LiteralValue) -> serde_json::Value {
    let mut data = indexmap::IndexMap::new();
    data.insert(
        name.to_string(),
        lemma::DataEntry {
            lemma_type: literal.lemma_type.as_ref().clone(),
            prefilled: None,
            supplied: None,
            default: Some(literal.clone()),
        },
    );
    let schema = SpecSchema {
        spec: "wire_test".to_string(),
        commentary: None,
        effective: None,
        versions: Vec::new(),
        data,
        rules: indexmap::IndexMap::new(),
        meta: HashMap::new(),
    };
    serde_json::to_value(&schema).expect("SpecSchema API JSON must serialize")["data"][name]
        ["default"]
        .clone()
}

fn api_schema_default_json(engine: &Engine, spec: &str, data_name: &str) -> serde_json::Value {
    let lit = schema_default_literal(engine, spec, data_name);
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

fn response_data_api_json(response: &lemma::Response, data_name: &str) -> serde_json::Value {
    for group in &response.data {
        for data in &group.data {
            if data.path.input_key() == data_name {
                let BindingDataValue::Definition { value, .. } = &data.value;
                let lit = value
                    .as_ref()
                    .unwrap_or_else(|| panic!("{data_name} has no value in response.data"));
                return api_wire_json_for_literal(data_name, lit);
            }
        }
    }
    panic!("{data_name} not found in response.data");
}

fn deserialize_api_wire_literal(json: serde_json::Value) -> LiteralValue {
    let lemma_type = json["lemma_type"].clone();
    let entry: lemma::DataEntry = serde_json::from_value(serde_json::json!({
        "type": lemma_type,
        "default": json,
    }))
    .expect("API wire literal must deserialize via DataEntry");
    entry
        .default
        .expect("default literal present in API wire JSON")
}

fn first_measure_constant_json(plan: &ExecutionPlan) -> serde_json::Value {
    let plan_json =
        serde_json::to_value(ExecutionPlanSerialized::from(plan)).expect("serialize plan");
    let rules = plan_json["rules"].as_array().expect("plan rules array");
    for rule in rules {
        let constants = rule["instructions"]["constants"]
            .as_array()
            .expect("rule constants array");
        for constant in constants {
            if constant["value"].get("measure").is_some() {
                return constant.clone();
            }
        }
    }
    panic!("no measure constant in plan instructions");
}

// --- A: In-memory unchanged ---

#[test]
fn in_memory_ratio_percent_default_is_canonical() {
    let default = schema_default_literal(&policy_engine(), "policy", "margin");
    assert_ratio_exact(&default, "margin default", "0.15", Some("percent"));
}

#[test]
fn in_memory_ratio_basis_points_default_is_canonical() {
    let default = schema_default_literal(&policy_engine(), "policy", "bps");
    assert_ratio_exact(&default, "bps default", "0.05", Some("basis_points"));
}

#[test]
fn in_memory_measure_eur_per_hour_default_is_canonical() {
    let default = schema_default_literal(&cost_price_engine(), "cost_price", "labor_cost");
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
    let json = api_schema_default_json(&policy_engine(), "policy", "margin");
    let (value, unit) = json_ratio_wire(&json);
    assert_eq!(value, "15");
    assert_eq!(unit.as_deref(), Some("percent"));
}

#[test]
fn ratio_api_wire_permille_150() {
    let json = api_schema_default_json(&policy_engine(), "policy", "permille_rate");
    let (value, unit) = json_ratio_wire(&json);
    assert_eq!(value, "150");
    assert_eq!(unit.as_deref(), Some("permille"));
}

#[test]
fn ratio_api_wire_basis_points_500() {
    let json = api_schema_default_json(&policy_engine(), "policy", "bps");
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
    let original = schema_default_literal(&policy_engine(), "policy", "margin");
    let json = api_wire_json_for_literal("margin", &original);
    let roundtrip: LiteralValue = deserialize_api_wire_literal(json);
    assert_ratio_exact(&roundtrip, "percent roundtrip", "0.15", Some("percent"));
}

#[test]
fn ratio_api_wire_basis_points_roundtrip() {
    let original = schema_default_literal(&policy_engine(), "policy", "bps");
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
        bare.magnitude_default_for_decimal_prompt().as_deref(),
        Some("0.5")
    );
}

#[test]
fn ratio_prompt_percent_15() {
    let default = schema_default_literal(&policy_engine(), "policy", "margin");
    assert_eq!(
        default.magnitude_default_for_decimal_prompt().as_deref(),
        Some("15")
    );
}

#[test]
fn ratio_prompt_basis_points_500() {
    let default = schema_default_literal(&policy_engine(), "policy", "bps");
    assert_eq!(
        default.magnitude_default_for_decimal_prompt().as_deref(),
        Some("500")
    );
}

#[test]
fn measure_prompt_eur_per_hour_25() {
    let default = schema_default_literal(&cost_price_engine(), "cost_price", "labor_cost");
    assert_eq!(
        default.magnitude_default_for_decimal_prompt().as_deref(),
        Some("25")
    );
}

// --- E: Measure API wire ---

#[test]
fn measure_api_wire_eur_per_hour_25() {
    let json = api_schema_default_json(&cost_price_engine(), "cost_price", "labor_cost");
    assert_eq!(json_measure_wire(&json), "25");
}

#[test]
fn measure_api_wire_kg_per_hour_12() {
    let json = api_schema_default_json(&cost_price_engine(), "cost_price", "throughput");
    assert_eq!(json_measure_wire(&json), "12");
}

#[test]
fn measure_api_wire_roundtrip_canonical() {
    let original = schema_default_literal(&cost_price_engine(), "cost_price", "labor_cost");
    let json = api_wire_json_for_literal("labor_cost", &original);
    let roundtrip: LiteralValue = deserialize_api_wire_literal(json);
    assert_eq!(roundtrip.value, original.value);
}

#[test]
fn measure_prompt_matches_api_wire() {
    let schema = plan_interface_schema(&cost_price_engine(), "cost_price");
    for (name, wire) in [
        ("labor_cost", "25"),
        ("throughput", "12"),
        ("product_cost", "4"),
    ] {
        let default = schema
            .data
            .get(name)
            .unwrap_or_else(|| panic!("{name}"))
            .default
            .as_ref()
            .unwrap_or_else(|| panic!("{name} default"));
        assert_eq!(
            default.magnitude_default_for_decimal_prompt().as_deref(),
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

fn cost_price_inputs() -> HashMap<String, DataValueInput> {
    let mut data = HashMap::new();
    data.insert(
        "product_cost".into(),
        DataValueInput::convenience("4 eur_per_kg"),
    );
    data.insert(
        "labor_cost".into(),
        DataValueInput::convenience("25 eur_per_hour"),
    );
    data.insert(
        "throughput".into(),
        DataValueInput::convenience("12 kg_per_hour"),
    );
    data
}

#[test]
fn measure_eval_per_unit_inputs_ok() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "cost_price", Some(&now))
        .expect("plan");
    engine
        .run_plan(plan, Some(&now), cost_price_inputs(), true, None)
        .expect("evaluation");
}

#[test]
fn measure_response_data_api_wire() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "cost_price", Some(&now))
        .expect("plan");
    let response = engine
        .run_plan(plan, Some(&now), cost_price_inputs(), true, None)
        .expect("evaluation");
    assert_eq!(
        json_measure_wire(&response_data_api_json(&response, "labor_cost")),
        "25"
    );
    assert_eq!(
        json_measure_wire(&response_data_api_json(&response, "throughput")),
        "12"
    );
}

#[test]
fn measure_response_json_serializes() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "cost_price", Some(&now))
        .expect("plan");
    let response = engine
        .run_plan(plan, Some(&now), cost_price_inputs(), true, None)
        .expect("evaluation");
    serde_json::to_string(&response).expect("response API JSON must serialize");
}

#[test]
fn ratio_eval_15_percent_ok() {
    let engine = policy_engine();
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "policy", Some(&now)).expect("plan");
    let mut data = HashMap::new();
    data.insert("margin".into(), DataValueInput::convenience("15%"));
    let response = engine
        .run_plan(plan, Some(&now), data, true, None)
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
fn ratio_response_data_api_wire_percent() {
    let engine = policy_engine();
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "policy", Some(&now)).expect("plan");
    let mut data = HashMap::new();
    data.insert("margin".into(), DataValueInput::convenience("15%"));
    let response = engine
        .run_plan(plan, Some(&now), data, true, None)
        .expect("evaluation");
    let (value, _) = json_ratio_wire(&response_data_api_json(&response, "margin"));
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

// --- G: Overlay ---

#[test]
fn overlay_rejects_double_canonical_measure() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "cost_price", Some(&now))
        .expect("plan");
    let mut data = HashMap::new();
    data.insert(
        "product_cost".into(),
        DataValueInput::convenience("4 eur_per_kg"),
    );
    data.insert(
        "labor_cost".into(),
        DataValueInput::convenience("0.0069444444444444444444444444 eur_per_hour"),
    );
    data.insert(
        "throughput".into(),
        DataValueInput::convenience("0.0033333333333333333333333333 kg_per_hour"),
    );
    let overlay = DataOverlay::resolve(plan, data, &ResourceLimits::default()).expect("overlay");
    assert!(overlay
        .violated
        .contains_key(&DataPath::local("labor_cost".into())));
    assert!(overlay
        .violated
        .contains_key(&DataPath::local("throughput".into())));
}

#[test]
fn overlay_accepts_per_unit_measure() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "cost_price", Some(&now))
        .expect("plan");
    let overlay = DataOverlay::resolve(plan, cost_price_inputs(), &ResourceLimits::default())
        .expect("overlay");
    assert!(overlay.violated.is_empty());
}

#[test]
fn overlay_rejects_uncommittable_canonical() {
    let engine = cost_price_engine();
    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "cost_price", Some(&now))
        .expect("plan");
    let mut data = cost_price_inputs();
    data.insert(
        "labor_cost".into(),
        DataValueInput::convenience(
            "1000000000000000000000000000000000000000000000000000000000000 eur_per_hour",
        ),
    );
    let overlay = DataOverlay::resolve(plan, data, &ResourceLimits::default()).expect("overlay");
    assert!(overlay
        .violated
        .contains_key(&DataPath::local("labor_cost".into())));
}

// --- H: Range API wire ---

const RATIO_RANGE_PERCENT_SPEC: &str = r#"
spec policy
data allowed_band: ratio range -> default 10%...50%
rule band: allowed_band
"#;

const RATIO_RANGE_BPS_SPEC: &str = r#"
spec policy
data allowed_band: ratio range
  -> unit basis_points 10000
  -> default 200 basis_points...3500 basis_points
rule band: allowed_band
"#;

fn range_endpoint_ratio_wire(json: &serde_json::Value, side: &str) -> (String, Option<String>) {
    json_ratio_wire_valuekind(&json["value"]["range"][side])
}

#[test]
fn ratio_range_api_wire_percent_endpoints() {
    let engine = load_engine(RATIO_RANGE_PERCENT_SPEC, "ratio_range_pct.lemma");
    let default = schema_default_literal(&engine, "policy", "allowed_band");
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
    let default = schema_default_literal(&engine, "policy", "allowed_band");
    let json = api_wire_json_for_literal("allowed_band", &default);
    let (left, _) = range_endpoint_ratio_wire(&json, "from");
    let (right, _) = range_endpoint_ratio_wire(&json, "to");
    assert_eq!(left, "200");
    assert_eq!(right, "3500");
}

#[test]
fn ratio_range_api_wire_roundtrip_canonical() {
    let engine = load_engine(RATIO_RANGE_PERCENT_SPEC, "ratio_range_pct.lemma");
    let original = schema_default_literal(&engine, "policy", "allowed_band");
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

// --- I: Plan persistence ---

const INLINE_MEASURE_CONSTANT_SPEC: &str = r#"
spec t
uses lemma units
data money: measure
  -> unit eur 1
  -> decimals 2
data labor: measure
  -> unit eur_per_hour eur/hour
rule r: 25 eur_per_hour
"#;

#[test]
fn execution_plan_constants_serialize_canonical_not_api_wire() {
    let engine = load_engine(INLINE_MEASURE_CONSTANT_SPEC, "inline_measure.lemma");
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "t", Some(&now)).expect("plan");
    let constant_json = first_measure_constant_json(plan);
    let wire = constant_json["value"]["measure"]["value"]
        .as_str()
        .expect("measure constant value");
    assert_ne!(
        wire, "25",
        "plan persistence must not use API per-unit wire; got {wire}"
    );
    assert!(
        wire.starts_with("0.006944") || wire.contains("944"),
        "plan constant must stay canonical, got {wire}"
    );
}

#[test]
fn execution_plan_constants_roundtrip_preserves_canonical() {
    let engine = load_engine(INLINE_MEASURE_CONSTANT_SPEC, "inline_measure.lemma");
    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "t", Some(&now)).expect("plan");
    let json = serde_json::to_value(ExecutionPlanSerialized::from(plan)).expect("serialize plan");
    let serialized: ExecutionPlanSerialized =
        serde_json::from_value(json).expect("deserialize plan");
    let reconstructed = ExecutionPlan::try_from(serialized).expect("reconstruct plan");
    let response = engine
        .run_plan(&reconstructed, Some(&now), HashMap::new(), false, None)
        .expect("evaluate reconstructed plan");
    assert!(
        response.results.contains_key("r"),
        "rule r must produce a result"
    );
}
