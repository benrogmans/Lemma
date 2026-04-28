use crate::error::ErrorKind;
use crate::parsing::ast::DateTimeValue;
use crate::parsing::source::Source;
use crate::serialization::data_values_from_map;
use crate::{Engine, Error, SourceType};
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

    /// Load Lemma source. Resolves with `undefined` on success; rejects with an array of
    /// serialized errors (same shape as `EngineError` in `engine/packages/npm/lemma.d.ts`).
    ///
    /// Breaking: previously rejected with an array of strings.
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
                    let errors: Vec<JsError> = load_err.errors.iter().map(JsError::from).collect();
                    Err(errors
                        .serialize(&js_error_serializer())
                        .expect("BUG: serialize JsError array"))
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
        data_values: JsValue,
        effective: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let effective_dt = effective
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.parse::<DateTimeValue>().ok())
            .unwrap_or_else(DateTimeValue::now);

        let rule_names = parse_rule_names(&rule_names).map_err(js_err)?;
        let data = parse_data_values(&data_values).map_err(js_err)?;

        let engine = self.engine.borrow();
        let mut response = engine
            .run(spec, Some(&effective_dt), data, false)
            .map_err(|e| error_to_js(&e))?;

        if !rule_names.is_empty() {
            response.filter_rules(&rule_names);
        }

        serialize_engine_json(&response)
    }

    /// Loaded specs, each paired with its planning schema.
    ///
    /// Each entry has `{ name, effective_from, effective_to, schema }`. The
    /// pair describes a half-open `[effective_from, effective_to)` validity
    /// range; `effective_from` is `null` when the first version has no
    /// declared start, and `effective_to` is `null` for the latest version of
    /// a name (no successor). Order matches [`Engine::list_specs_with_ranges`].
    ///
    /// `schema` is the same envelope returned by [`WasmEngine::schema`] for
    /// `(name, effective_from)`; shipping it inline saves the N+1 round-trip
    /// every consumer (playground, dashboards, docs) was doing.
    #[wasm_bindgen(js_name = list)]
    pub fn list(&self) -> Result<JsValue, JsValue> {
        let engine = self.engine.borrow();
        let mut entries: Vec<SpecListEntry> = Vec::new();
        for (spec, effective_from, effective_to) in engine.list_specs_with_ranges() {
            let plan = engine
                .get_plan(&spec.name, effective_from.as_ref())
                .map_err(|e| error_to_js(&e))?;
            entries.push(SpecListEntry {
                name: spec.name.clone(),
                effective_from: effective_from.map(|d| d.to_string()),
                effective_to: effective_to.map(|d| d.to_string()),
                schema: plan.schema(),
            });
        }
        serialize_engine_json(&entries)
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
            .map_err(|e| error_to_js(&e))?;
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
            Err(e) => Err(error_to_js(&e)),
        }
    }
}

/// Per-version record exposed to JS by [`WasmEngine::list`].
///
/// The half-open range is `[effective_from, effective_to)`; both bounds are
/// `null` in JS when their corresponding bound is unbounded (`None`). The
/// planning `schema` is included inline so consumers never need an N+1
/// `engine.schema(name, effective_from)` call.
#[derive(Serialize)]
struct SpecListEntry {
    name: String,
    effective_from: Option<String>,
    effective_to: Option<String>,
    schema: crate::planning::execution_plan::SpecSchema,
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

/// Source slice serialized for JS (`EngineError.source` in TS).
#[derive(Serialize)]
struct JsSource<'a> {
    attribute: &'a str,
    line: usize,
    column: usize,
    length: usize,
}

impl<'a> From<&'a Source> for JsSource<'a> {
    fn from(s: &'a Source) -> Self {
        JsSource {
            attribute: &s.attribute,
            line: s.span.line,
            column: s.span.col,
            length: s.span.end.saturating_sub(s.span.start),
        }
    }
}

/// Flat view of [`Error`] for `serde_wasm_bindgen` — matches `EngineError` in
/// `engine/packages/npm/lemma.d.ts`.
#[derive(Serialize)]
struct JsError<'a> {
    kind: ErrorKind,
    message: &'a str,
    related_data: Option<&'a str>,
    spec: Option<&'a str>,
    related_spec: Option<&'a str>,
    source: Option<JsSource<'a>>,
    suggestion: Option<&'a str>,
}

impl<'a> From<&'a Error> for JsError<'a> {
    fn from(e: &'a Error) -> Self {
        JsError {
            kind: e.kind(),
            message: e.message(),
            related_data: e.related_data(),
            spec: e.spec(),
            related_spec: e.related_spec(),
            source: e.source_location().map(JsSource::from),
            suggestion: e.suggestion(),
        }
    }
}

/// Serializer that emits `null` (not `undefined`) for missing optionals so the object
/// matches the published `EngineError` TypeScript type.
fn js_error_serializer() -> serde_wasm_bindgen::Serializer {
    serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true)
}

/// Convert an engine [`Error`] into a plain JS object thrown from WASM.
fn error_to_js(e: &Error) -> JsValue {
    let err = JsError::from(e);
    err.serialize(&js_error_serializer())
        .expect("BUG: serialize JsError")
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

fn parse_data_values(v: &JsValue) -> Result<HashMap<String, String>, String> {
    if v.is_undefined() || v.is_null() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, serde_json::Value> = serde_wasm_bindgen::from_value(v.clone())
        .map_err(|e| format!("data_values must be a plain object: {}", e))?;
    Ok(data_values_from_map(map))
}
