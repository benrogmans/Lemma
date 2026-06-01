use lemma::SourceType;
use lemma::{DataValueInput, DateTimeValue, Engine};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub struct Fixture {
    pub spec_name: &'static str,
    pub label: &'static str,
    pub source: &'static str,
    /// Raw bytes of the `*.inputs.json` sidecar. Benches that time
    /// JSON parsing must `serde_json::from_slice` this inside the
    /// measured closure so the work matches the Python boundary.
    pub data_json: &'static str,
    pub effective: DateTimeValue,
}

fn effective() -> DateTimeValue {
    DateTimeValue {
        year: 2026,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
    }
}

pub fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            spec_name: "bench_shipping",
            label: "engine/benches/specs/shipping.lemma",
            source: include_str!("../specs/shipping.lemma"),
            data_json: include_str!("../specs/shipping.inputs.json"),
            effective: effective(),
        },
        Fixture {
            spec_name: "bench_pricing",
            label: "engine/benches/specs/pricing.lemma",
            source: include_str!("../specs/pricing.lemma"),
            data_json: include_str!("../specs/pricing.inputs.json"),
            effective: effective(),
        },
        Fixture {
            spec_name: "bench_order_pipeline",
            label: "engine/benches/specs/order_pipeline.lemma",
            source: include_str!("../specs/order_pipeline.lemma"),
            data_json: include_str!("../specs/order_pipeline.inputs.json"),
            effective: effective(),
        },
    ]
}

pub fn build_engine(fixture: &Fixture) -> Engine {
    let mut engine = Engine::new();
    let source_type = SourceType::Path(Arc::new(PathBuf::from(fixture.label)));
    engine
        .load(fixture.source, source_type)
        .expect("BUG: bench fixture spec must load");
    engine
}

fn data_input_from_json_value(value: serde_json::Value) -> Result<DataValueInput, String> {
    use std::collections::BTreeMap;
    match value {
        serde_json::Value::String(s) => Ok(DataValueInput::Convenience(s)),
        serde_json::Value::Bool(b) => Ok(DataValueInput::Boolean(b)),
        serde_json::Value::Number(n) => Ok(DataValueInput::Convenience(n.to_string())),
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                return Err("data value object must not be empty".to_string());
            }
            if obj.len() == 2 && obj.contains_key("value") && obj.contains_key("unit") {
                return Err(
                    "the {value, unit} object shape is not supported; use a unit map like {\"eur\": \"84\"}"
                        .to_string(),
                );
            }
            if obj.values().all(|v| v.is_string()) {
                let map: BTreeMap<String, String> = obj
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            v.as_str()
                                .expect("BUG: object values checked as strings")
                                .to_string(),
                        )
                    })
                    .collect();
                return Ok(DataValueInput::QuantityMap(map));
            }
            Err("data value object must be a unit map with string magnitudes".to_string())
        }
        serde_json::Value::Null => Err("data value must not be null".to_string()),
        serde_json::Value::Array(_) => Err("data value must not be an array".to_string()),
    }
}

/// Parse the fixture's pinned `*.inputs.json` into [`DataValueInput`] values.
///
/// Used inside timed closures so JSON parsing is counted in the latency
/// and allocation numbers, matching the Python harness which calls
/// `json.loads` per iteration before invoking `compute`.
pub fn parse_data_values(raw_bytes: &[u8]) -> HashMap<String, DataValueInput> {
    let map: HashMap<String, serde_json::Value> = serde_json::from_slice(raw_bytes)
        .expect("BUG: bench fixture inputs JSON must parse as HashMap<String, serde_json::Value>");
    map.into_iter()
        .map(|(k, v)| {
            (
                k,
                data_input_from_json_value(v)
                    .expect("BUG: bench fixture data value must be supported"),
            )
        })
        .collect()
}
