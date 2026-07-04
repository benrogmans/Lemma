//! Emit Lemma's per-rule output values for each benchmark fixture.
//!
//! No timing, no warmup, no iteration loop. Runs each fixture once and
//! prints a JSON document the xtask report can diff against the Python
//! benchmark's output dump.
//!
//! The shape is one record per fixture:
//!
//! ```json
//! {
//!   "fixtures": [
//!     {
//!       "spec_name": "bench_shipping",
//!       "outputs": {
//!         "base_rate": { "kind": "number", "value": "5", "unit": null },
//!         "member_discount": { "kind": "ratio", "value": "0.1", "unit": "percent" }
//!       }
//!     }
//!   ]
//! }
//! ```
//!
//! Normalisation strategy: serialise each [`lemma::ValueKind`] through its
//! existing serde impl (defined in `engine/src/planning/semantics.rs` around
//! line 2186), then unpack the single-key object into a flat `{kind, value,
//! unit}` triple. Veto results are surfaced with `kind = "veto"` and the
//! veto's `Display` reason in `value`.

use lemma::{OperationResult, VetoType};
use serde::Serialize;
use serde_json::Value;

mod common;

#[derive(Serialize)]
struct Document {
    fixtures: Vec<FixtureOutputs>,
}

#[derive(Serialize)]
struct FixtureOutputs {
    spec_name: &'static str,
    outputs: serde_json::Map<String, Value>,
}

#[derive(Serialize)]
struct Output {
    kind: &'static str,
    value: String,
    unit: Option<String>,
}

fn normalize_value_kind(value: &lemma::ValueKind) -> Output {
    let raw = serde_json::to_value(value).expect("BUG: ValueKind serialization is infallible here");
    let Value::Object(mut map) = raw else {
        panic!("BUG: ValueKind serialized to non-object: {raw:?}");
    };
    if map.len() != 1 {
        panic!(
            "BUG: ValueKind serialized with {} keys, expected 1",
            map.len()
        );
    }
    let (tag, payload) = map
        .iter_mut()
        .next()
        .map(|(k, v)| (k.clone(), v.take()))
        .expect("BUG: len-1 map has one entry");

    match tag.as_str() {
        "number" => Output {
            kind: "number",
            value: payload
                .as_str()
                .expect("BUG: number payload must be a string")
                .to_string(),
            unit: None,
        },
        "ratio" => {
            let (value, unit) = take_value_unit(&payload, "ratio");
            Output {
                kind: "ratio",
                value,
                unit: Some(unit),
            }
        }
        "measure" => {
            let (value, unit) = take_value_unit(&payload, "measure");
            Output {
                kind: "measure",
                value,
                unit: Some(unit),
            }
        }
        "calendar" => {
            let (value, unit) = take_value_unit(&payload, "calendar");
            Output {
                kind: "calendar",
                value,
                unit: Some(unit),
            }
        }
        "boolean" => Output {
            kind: "boolean",
            value: match payload
                .as_bool()
                .expect("BUG: boolean payload must be a JSON bool")
            {
                true => "true".to_string(),
                false => "false".to_string(),
            },
            unit: None,
        },
        "text" => Output {
            kind: "text",
            value: payload
                .as_str()
                .expect("BUG: text payload must be a string")
                .to_string(),
            unit: None,
        },
        "date" => Output {
            kind: "date",
            value: serde_json::to_string(&payload).expect("BUG: date payload is JSON-serializable"),
            unit: None,
        },
        "time" => Output {
            kind: "time",
            value: serde_json::to_string(&payload).expect("BUG: time payload is JSON-serializable"),
            unit: None,
        },
        "range" => {
            todo!(
                "range output not yet expected in benchmark fixtures; comparison semantics undefined"
            )
        }
        other => panic!("BUG: unknown ValueKind tag '{other}' in fixture output"),
    }
}

fn take_value_unit(payload: &Value, label: &str) -> (String, String) {
    let object = payload
        .as_object()
        .unwrap_or_else(|| panic!("BUG: {label} payload must be an object"));
    let value = object
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("BUG: {label} payload missing 'value' string"))
        .to_string();
    let unit = object
        .get("unit")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("BUG: {label} payload missing 'unit' string"))
        .to_string();
    (value, unit)
}

fn normalize_veto(veto: &VetoType) -> Output {
    Output {
        kind: "veto",
        value: veto.to_string(),
        unit: None,
    }
}

fn main() {
    let mut fixtures = Vec::new();
    for fixture in common::fixtures() {
        let engine = common::build_engine(&fixture);
        let plan = engine
            .get_plan(None, fixture.spec_name, Some(&fixture.effective))
            .expect("BUG: bench fixture must produce execution plan");
        let data = fixture.data.clone();
        let response = engine
            .run_plan(plan, Some(&fixture.effective), data, true, None)
            .expect("BUG: outputs bench fixture must evaluate");

        let mut outputs = serde_json::Map::new();
        for (rule_name, result) in &response.results {
            let operation_result = &result
                .explanation
                .as_ref()
                .expect("BUG: outputs bench requires explain: true")
                .result;
            let normalized = match operation_result {
                OperationResult::Value(literal) => normalize_value_kind(&literal.value),
                OperationResult::Veto(veto) => normalize_veto(veto),
            };
            outputs.insert(
                rule_name.clone(),
                serde_json::to_value(&normalized)
                    .expect("BUG: Output is trivially JSON-serializable"),
            );
        }
        fixtures.push(FixtureOutputs {
            spec_name: fixture.spec_name,
            outputs,
        });
    }

    let document = Document { fixtures };
    let rendered = serde_json::to_string(&document).expect("BUG: Document is JSON-serializable");
    println!("{rendered}");
}
