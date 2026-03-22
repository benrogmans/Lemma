use crate::parsing::ast::DateTimeValue;
use crate::serialization::fact_values_from_map;
use crate::{Engine, SourceType};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = Engine)]
pub struct WasmEngine {
    engine: Rc<RefCell<Engine>>,
}

#[wasm_bindgen]
impl WasmEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        WasmEngine {
            engine: Rc::new(RefCell::new(Engine::new())),
        }
    }

    /// Load Lemma source. Resolves with `undefined` on success; rejects with an array of error strings.
    #[wasm_bindgen(js_name = load)]
    pub fn load_wasm(&self, code: &str, attribute: &str) -> js_sys::Promise {
        let code = code.to_string();
        let label = if attribute.trim().is_empty() {
            None
        } else {
            Some(attribute.to_string())
        };
        let engine = self.engine.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let source = match &label {
                None => SourceType::Inline,
                Some(s) => SourceType::Labeled(s.as_str()),
            };
            let result = engine.borrow_mut().load(&code, source);
            match result {
                Ok(()) => Ok(JsValue::UNDEFINED),
                Err(load_err) => {
                    let messages: Vec<String> =
                        load_err.errors.iter().map(|e| e.to_string()).collect();
                    Err(to_js(&messages).expect("BUG: serialize error messages"))
                }
            }
        })
    }

    /// Evaluate spec. Returns [`crate::evaluation::Response`] as a JS object. Throws on planning/runtime error.
    #[wasm_bindgen(js_name = run)]
    pub fn run(
        &self,
        spec: &str,
        rule_names: JsValue,
        fact_values: JsValue,
        effective: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let effective_dt = effective
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.parse::<DateTimeValue>().ok())
            .unwrap_or_else(DateTimeValue::now);

        let rule_names = parse_rule_names(&rule_names).map_err(js_err)?;
        let facts = parse_fact_values(&fact_values).map_err(js_err)?;

        let engine = self.engine.borrow();
        let mut response = engine
            .run(spec, Some(&effective_dt), facts, false)
            .map_err(|e| js_err(e.to_string()))?;

        if !rule_names.is_empty() {
            response.filter_rules(&rule_names);
        }

        serialize_engine_json(&response)
    }

    /// Spec names from the engine (same order as [`Engine::list_specs`]).
    #[wasm_bindgen(js_name = list)]
    pub fn list(&self) -> Result<JsValue, JsValue> {
        let specs = self.engine.borrow().list_specs();
        to_js(&specs).map_err(|e| js_err(e.to_string()))
    }

    /// Planning schema for the spec ([`crate::planning::execution_plan::SpecSchema`]). Throws on error.
    #[wasm_bindgen(js_name = schema)]
    pub fn schema(&self, spec: &str, effective: Option<String>) -> Result<JsValue, JsValue> {
        let effective_dt = effective
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.parse::<DateTimeValue>().ok())
            .unwrap_or_else(DateTimeValue::now);

        let engine = self.engine.borrow();
        let plan = engine
            .get_plan(spec, Some(&effective_dt))
            .map_err(|e| js_err(e.to_string()))?;
        let schema = plan.schema();

        serialize_engine_json(&schema)
    }

    #[wasm_bindgen(js_name = invert)]
    pub fn invert(
        &self,
        _spec_name: &str,
        _rule_name: &str,
        _target_json: &str,
        _provided_values_json: &str,
    ) -> Result<JsValue, JsValue> {
        Err(js_err("Inversion not implemented"))
    }

    /// Returns formatted source string on success; throws with error message on failure.
    #[wasm_bindgen(js_name = format)]
    pub fn format_wasm(&self, code: &str, attribute: Option<String>) -> Result<JsValue, JsValue> {
        let attr = match attribute
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => s,
            None => SourceType::INLINE_KEY,
        };
        match crate::format_source(code, attr) {
            Ok(formatted) => Ok(JsValue::from_str(&formatted)),
            Err(e) => Err(js_err(e.to_string())),
        }
    }
}

fn to_js<T: Serialize>(v: &T) -> Result<JsValue, serde_wasm_bindgen::Error> {
    serde_wasm_bindgen::to_value(v)
}

/// Same JSON as CLI/HTTP. `serde_wasm_bindgen::to_value(serde_json::Value)` drops
/// `IndexMap` entries (e.g. `Response.results` → `{}`); `JSON.parse` matches browser semantics.
fn serialize_engine_json<T: Serialize>(v: &T) -> Result<JsValue, JsValue> {
    let s = serde_json::to_string(v)
        .map_err(|e| js_err(format!("BUG: serde_json::to_string failed: {}", e)))?;
    js_sys::JSON::parse(&s).map_err(|e| {
        let detail = e
            .as_string()
            .unwrap_or_else(|| format!("(non-string error from JSON.parse)"));
        js_err(format!("BUG: JSON.parse failed: {}", detail))
    })
}

fn js_err(msg: impl Into<String>) -> JsValue {
    JsValue::from_str(&msg.into())
}

fn parse_rule_names(v: &JsValue) -> Result<Vec<String>, String> {
    if v.is_undefined() || v.is_null() {
        return Ok(Vec::new());
    }
    if js_sys::Array::is_array(v) {
        return serde_wasm_bindgen::from_value(v.clone())
            .map_err(|e| format!("rule_names must be an array of strings: {}", e));
    }
    if let Some(s) = v.as_string() {
        let trimmed = s.trim();
        if trimmed.is_empty() || trimmed == "[]" {
            return Ok(Vec::new());
        }
        return serde_json::from_str::<Vec<String>>(trimmed).map_err(|e| {
            format!(
                "rule_names must be an array of strings (or JSON array string): {}",
                e
            )
        });
    }
    Err("rule_names must be an array of strings".into())
}

fn parse_fact_values(v: &JsValue) -> Result<HashMap<String, String>, String> {
    if v.is_undefined() || v.is_null() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, serde_json::Value> = serde_wasm_bindgen::from_value(v.clone())
        .map_err(|e| format!("fact_values must be a plain object: {}", e))?;
    Ok(fact_values_from_map(map))
}
