use crate::error::ErrorKind;
use crate::evaluation::DataValueInput;
use crate::parsing::source::Source;
use crate::{Engine, Error, SourceType};
use serde::Serialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = Engine)]
pub struct WasmEngine {
    engine: Engine,
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl WasmEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        WasmEngine {
            engine: Engine::new(),
        }
    }

    /// Load Lemma source(s).
    ///
    /// - string → one volatile workspace source
    /// - plain object or `[label, code][]` → labeled sources in one planning pass
    ///
    /// Throws with an array of serialized errors on failure. `null` / `undefined` are rejected.
    #[wasm_bindgen(js_name = load)]
    pub fn load_wasm(&mut self, sources: JsValue) -> Result<(), JsValue> {
        let batch = parse_load_sources(sources)?;
        self.engine.load(batch).map_err(serialize_load_errors)
    }

    /// Download Lemma source for a registry identifier via [`crate::registry::LemmaBase`]. Returns `{ source, id }`.
    /// Does not load this [`WasmEngine`]; call [`Self::load`] with `{ [id]: source }`.
    #[wasm_bindgen(js_name = fetch)]
    pub fn fetch_wasm(&self, name: &str) -> js_sys::Promise {
        match crate::spec_set_id::parse_spec_set_id(name) {
            Err(e) => {
                let js_err_array = {
                    let errors = vec![JsError::from(&e)];
                    errors
                        .serialize(&js_error_serializer())
                        .expect("BUG: serialize JsError array")
                };
                wasm_bindgen_futures::future_to_promise(async move { Err(js_err_array) })
            }
            Ok(normalized) => {
                #[cfg(not(feature = "registry"))]
                {
                    let err = Error::request(
                        format!(
                            "fetch of '{normalized}' requires the lemma-engine crate to be built with the `registry` feature (engine has {} loaded repositories)",
                            self.engine.list().len()
                        ),
                        None::<String>,
                    );
                    let js_err_array = {
                        let errors = vec![JsError::from(&err)];
                        errors
                            .serialize(&js_error_serializer())
                            .expect("BUG: serialize JsError array")
                    };
                    wasm_bindgen_futures::future_to_promise(async move { Err(js_err_array) })
                }
                #[cfg(feature = "registry")]
                {
                    wasm_registry_fetch_only_promise(normalized)
                }
            }
        }
    }

    /// Evaluate spec. Returns [`crate::evaluation::Response`] as a JS object. Throws on planning/runtime error.
    #[wasm_bindgen(js_name = run)]
    pub fn run(
        &self,
        repository: Option<String>,
        spec: &str,
        effective: Option<String>,
        data_values: JsValue,
        rule_names: JsValue,
        explain: Option<bool>,
    ) -> Result<JsValue, JsValue> {
        let effective_dt =
            crate::resolve_effective(effective.as_deref()).map_err(|e| error_to_js(&e))?;

        let data = parse_data_values_to_strings(&data_values).map_err(js_err)?;

        let repo = repository
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let response_rules = resolve_wasm_response_rules(&rule_names).map_err(js_err)?;

        let response = self
            .engine
            .run(
                repo,
                spec,
                Some(&effective_dt),
                data,
                response_rules.as_deref(),
                explain.unwrap_or(false),
            )
            .map_err(|e| error_to_js(&e))?;

        serialize_engine_json(&response)
    }

    /// Catalog of loaded repositories and specs (metadata only, no source).
    #[wasm_bindgen(js_name = list)]
    pub fn list_wasm(&self) -> Result<JsValue, JsValue> {
        let repos = self.engine.list();
        serialize_engine_json(&repos)
    }

    /// Spec interface and temporal window at `effective`. Lemma text is [`Self::source`].
    #[wasm_bindgen(js_name = show)]
    pub fn show_wasm(
        &self,
        repository: Option<String>,
        spec: &str,
        effective: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let effective_dt =
            crate::resolve_effective(effective.as_deref()).map_err(|e| error_to_js(&e))?;
        let repo = repository
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let view = self
            .engine
            .show(repo, spec, Some(&effective_dt))
            .map_err(|e| error_to_js(&e))?;
        serialize_engine_json(&view)
    }

    /// Formatted canonical Lemma source. Omit `spec` for whole-repository text.
    #[wasm_bindgen(js_name = source)]
    pub fn source_wasm(
        &self,
        repository: Option<String>,
        spec: Option<String>,
        effective: Option<String>,
    ) -> Result<String, JsValue> {
        let repo = repository
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let spec_name = spec.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let effective_dt = match (spec_name, effective) {
            (Some(_), eff) => {
                Some(crate::resolve_effective(eff.as_deref()).map_err(|e| error_to_js(&e))?)
            }
            _ => None,
        };
        self.engine
            .source(repo, spec_name, effective_dt.as_ref())
            .map_err(|e| error_to_js(&e))
    }

    /// Remove a temporal spec slice. `effective`: ISO datetime string or omit for now.
    #[wasm_bindgen(js_name = remove)]
    pub fn remove_wasm(
        &mut self,
        repository: Option<String>,
        spec: &str,
        effective: Option<String>,
    ) -> Result<(), JsValue> {
        let effective_dt = match effective {
            None => None,
            Some(s) => {
                Some(crate::resolve_effective(Some(s.as_str())).map_err(|e| error_to_js(&e))?)
            }
        };
        let repo = repository
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        self.engine
            .remove(repo, spec, effective_dt.as_ref())
            .map_err(|e| error_to_js(&e))
    }

    /// Resource limits configured for this engine.
    #[wasm_bindgen(js_name = limits)]
    pub fn limits_wasm(&self) -> Result<JsValue, JsValue> {
        serialize_engine_json(self.engine.limits())
    }

    /// Returns formatted source string on success; throws with error message on failure.
    #[wasm_bindgen(js_name = format)]
    pub fn format_wasm(&self, code: &str, attribute: Option<String>) -> Result<JsValue, JsValue> {
        let attr = attribute
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("inline source (no path)");
        match crate::format_source(
            code,
            crate::parsing::source::SourceType::Path(std::sync::Arc::new(
                std::path::PathBuf::from(attr),
            )),
        ) {
            Ok(formatted) => Ok(JsValue::from_str(&formatted)),
            Err(e) => Err(error_to_js(&e)),
        }
    }
}

#[derive(Serialize)]
struct RegistryFetchPayload {
    source: String,
    id: String,
}

#[cfg(feature = "registry")]
fn wasm_registry_fetch_only_promise(name: String) -> js_sys::Promise {
    wasm_bindgen_futures::future_to_promise(async move {
        use crate::registry::{LemmaBase, Registry, RegistryErrorKind};

        let registry = LemmaBase::new();
        let bundle = match registry.get(&name).await {
            Ok(b) => b,
            Err(registry_error) => {
                let suggestion = match &registry_error.kind {
                    RegistryErrorKind::NotFound => Some(
                        "Check that the repository qualifier is spelled correctly and that the repository exists on the registry.".to_string(),
                    ),
                    RegistryErrorKind::Unauthorized => Some(
                        "Check your authentication credentials or permissions for this registry."
                            .to_string(),
                    ),
                    RegistryErrorKind::NetworkError => Some(
                        "Check your network connection.".to_string(),
                    ),
                    RegistryErrorKind::ServerError => Some(
                        "The registry server returned an internal error. Try again later.".to_string(),
                    ),
                    RegistryErrorKind::Other => None,
                };
                let source = Source::new(
                    SourceType::Volatile,
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 1,
                    },
                );
                let err = Error::registry(
                    registry_error.message,
                    source,
                    name.clone(),
                    registry_error.kind,
                    suggestion,
                    None,
                    None,
                );
                let errors = vec![JsError::from(&err)];
                return Err(errors
                    .serialize(&js_error_serializer())
                    .expect("BUG: serialize JsError array"));
            }
        };

        let payload = RegistryFetchPayload {
            source: bundle.source,
            id: name,
        };
        serialize_engine_json(&payload)
    })
}

/// Same JSON as CLI/HTTP.
/// `IndexMap` entries (e.g. `Response.results` → `{}`); `JSON.parse` matches browser semantics.
fn serialize_engine_json<T: Serialize>(v: &T) -> Result<JsValue, JsValue> {
    let s = serde_json::to_string(v)
        .map_err(|e| js_err(format!("BUG: serde_json::to_string failed: {}", e)))?;
    js_sys::JSON::parse(&s).map_err(|e| {
        let detail = e
            .as_string()
            .unwrap_or_else(|| "(non-string error from JSON.parse)".to_string());
        js_err(format!("BUG: JSON.parse failed: {}", detail))
    })
}

fn js_err(msg: impl Into<String>) -> JsValue {
    JsValue::from_str(&msg.into())
}

/// Source slice serialized for JS (`EngineError.source` in TS).
#[derive(Serialize)]
struct JsSource {
    attribute: String,
    line: usize,
    column: usize,
    length: usize,
}

impl<'a> From<&'a Source> for JsSource {
    fn from(s: &'a Source) -> Self {
        JsSource {
            attribute: s.source_type.to_string(),
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
    source: Option<JsSource>,
    suggestion: Option<&'a str>,
    /// Set for [`Error::MissingRepository`] and [`Error::Registry`] (registry `@` id).
    repository: Option<&'a str>,
}

impl<'a> From<&'a Error> for JsError<'a> {
    fn from(e: &'a Error) -> Self {
        JsError {
            kind: e.kind(),
            message: e.message(),
            related_data: e.related_data(),
            spec: e.spec_context_name(),
            related_spec: e.related_spec(),
            source: e.source_location().map(JsSource::from),
            suggestion: e.suggestion(),
            repository: e.repository(),
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

fn resolve_wasm_response_rules(rule_names: &JsValue) -> Result<Option<Vec<String>>, String> {
    if rule_names.is_undefined() || rule_names.is_null() {
        return Ok(None);
    }
    let parsed = parse_rule_names(rule_names)?;
    if parsed.is_empty() {
        return Err("rule_names must not be empty".to_string());
    }
    Ok(Some(parsed))
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

fn data_input_from_json_value(value: serde_json::Value) -> Result<DataValueInput, String> {
    use std::collections::BTreeMap;
    match value {
        serde_json::Value::String(s) => Ok(DataValueInput::Convenience(s)),
        serde_json::Value::Bool(b) => Ok(DataValueInput::Boolean(b)),
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                Ok(DataValueInput::Convenience(n.to_string()))
            } else {
                Err("decimal values must be passed as strings to preserve exactness".to_string())
            }
        }
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
                return Ok(DataValueInput::MeasureMap(map));
            }
            Err("data value object must be a unit map with string magnitudes".to_string())
        }
        serde_json::Value::Null => Err("data value must not be null".to_string()),
        serde_json::Value::Array(_) => Err("data value must not be an array".to_string()),
    }
}

fn serialize_load_errors(load_err: crate::Errors) -> JsValue {
    let errors: Vec<JsError> = load_err.errors.iter().map(JsError::from).collect();
    errors
        .serialize(&js_error_serializer())
        .expect("BUG: serialize JsError array")
}

fn request_error_js(message: impl Into<String>) -> JsValue {
    let err = Error::request(message, None::<String>);
    let errors = vec![JsError::from(&err)];
    errors
        .serialize(&js_error_serializer())
        .expect("BUG: serialize JsError array")
}

fn parse_load_sources(sources: JsValue) -> Result<Vec<(SourceType, String)>, JsValue> {
    if sources.is_undefined() || sources.is_null() {
        return Err(request_error_js(
            "load: sources must be a string, plain object, or array of [label, code] pairs"
                .to_string(),
        ));
    }
    if let Some(code) = sources.as_string() {
        return Ok(vec![(SourceType::Volatile, code)]);
    }
    if sources.is_array() {
        let arr = js_sys::Array::from(&sources);
        let mut batch = Vec::with_capacity(arr.length() as usize);
        for (i, item) in arr.iter().enumerate() {
            if !item.is_array() {
                return Err(request_error_js(format!(
                    "load: entry {i} must be a [label, code] pair"
                )));
            }
            let pair = js_sys::Array::from(&item);
            if pair.length() != 2 {
                return Err(request_error_js(format!(
                    "load: entry {i} must be a [label, code] pair"
                )));
            }
            let label = pair.get(0).as_string().ok_or_else(|| {
                request_error_js(format!("load: entry {i} label must be a string"))
            })?;
            let code = pair.get(1).as_string().ok_or_else(|| {
                request_error_js(format!("load: entry {i} code must be a string"))
            })?;
            let source_type = SourceType::from_binding_label(&label)
                .map_err(|e| request_error_js(format!("load: entry {i}: {e}")))?;
            batch.push((source_type, code));
        }
        return Ok(batch);
    }
    let map: HashMap<String, String> = serde_wasm_bindgen::from_value(sources).map_err(|e| {
        request_error_js(format!(
            "load: sources must be a string, plain object, or array of [label, code] pairs: {e}"
        ))
    })?;
    map.into_iter()
        .map(|(label, code)| {
            SourceType::from_binding_label(&label)
                .map(|source_type| (source_type, code))
                .map_err(|e| request_error_js(format!("load: label '{label}': {e}")))
        })
        .collect()
}

fn parse_data_values_to_strings(v: &JsValue) -> Result<HashMap<String, String>, String> {
    if v.is_undefined() || v.is_null() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, serde_json::Value> = serde_wasm_bindgen::from_value(v.clone())
        .map_err(|e| format!("data_values must be a plain object: {}", e))?;
    map.into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| {
            let input = data_input_from_json_value(v)?;
            match input {
                DataValueInput::Convenience(s) => Ok((k, s)),
                DataValueInput::Boolean(b) => Ok((k, b.to_string())),
                DataValueInput::MeasureMap(m) => {
                    if m.len() == 1 {
                        let (unit, mag) = m.into_iter().next().expect("BUG: single entry map");
                        Ok((k, format!("{mag} {unit}")))
                    } else {
                        Err(format!(
                            "data value '{k}' must be a convenience string for WASM run"
                        ))
                    }
                }
                DataValueInput::RatioMap(m) => {
                    if m.len() == 1 {
                        let (unit, mag) = m.into_iter().next().expect("BUG: single entry map");
                        Ok((k, format!("{mag} {unit}")))
                    } else {
                        Err(format!(
                            "data value '{k}' must be a convenience string for WASM run"
                        ))
                    }
                }
            }
        })
        .collect()
}
