use crate::error::EngineError;
use crate::evaluation::RunDataValue;
use crate::{Engine, Error, Source, SourceType};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
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

#[wasm_bindgen(js_class = "Engine")]
impl WasmEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        WasmEngine {
            engine: Engine::new(),
        }
    }

    /// Create an engine with custom [`crate::ResourceLimits`].
    ///
    /// Pass a plain object of limit keys (same names as Rust / Java). Unknown keys throw.
    #[wasm_bindgen(js_name = withLimits)]
    pub fn with_limits(limits: JsValue) -> Result<WasmEngine, JsValue> {
        console_error_panic_hook::set_once();
        let map: HashMap<String, f64> = serde_wasm_bindgen::from_value(limits)
            .map_err(|e| js_err(format!("invalid limits: {e}")))?;
        let mut resource_limits = crate::ResourceLimits::default();
        for (key, value) in map {
            let n = crate::limits::usize_limit_from_f64(&key, value).map_err(js_err)?;
            resource_limits.apply(&key, n).map_err(js_err)?;
        }
        Ok(WasmEngine {
            engine: Engine::with_limits(resource_limits),
        })
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
                    let errors = vec![EngineError::from(&e)];
                    errors
                        .serialize(&js_error_serializer())
                        .expect("BUG: serialize EngineError array")
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
                        let errors = vec![EngineError::from(&err)];
                        errors
                            .serialize(&js_error_serializer())
                            .expect("BUG: serialize EngineError array")
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
    ///
    /// Accepts an options object: `{ spec, repository?, effective?, data?, rules?, explain? }`.
    #[wasm_bindgen(js_name = run)]
    pub fn run(&self, options: JsValue) -> Result<JsValue, JsValue> {
        let opts: RunOptions = serde_wasm_bindgen::from_value(options)
            .map_err(|e| js_err(format!("invalid run options: {e}")))?;

        let effective_dt =
            crate::resolve_effective(opts.effective.as_deref()).map_err(|e| error_to_js(&e))?;

        let data = parse_run_data(&opts.data).map_err(js_err)?;

        let repo = opts
            .repository
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let response_rules = resolve_run_rules(&opts.rules).map_err(js_err)?;

        let response = self
            .engine
            .run(
                repo,
                &opts.spec,
                Some(&effective_dt),
                data,
                response_rules.as_deref(),
                opts.explain.unwrap_or(false),
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
        let (repo, effective_dt) = parse_repo_and_effective(repository.as_ref(), effective)?;
        self.engine
            .remove(repo, spec, effective_dt.as_ref())
            .map_err(|e| error_to_js(&e))
    }

    /// Replace a temporal spec slice with new source (atomic remove + load).
    ///
    /// `attribute` is the source label (path or `@owner/repo`). Omit for a volatile source.
    #[wasm_bindgen(js_name = update)]
    pub fn update_wasm(
        &mut self,
        repository: Option<String>,
        spec: &str,
        effective: Option<String>,
        code: &str,
        attribute: Option<String>,
    ) -> Result<(), JsValue> {
        let (repo, effective_dt) = parse_repo_and_effective(repository.as_ref(), effective)?;
        let source_type = match attribute
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => SourceType::Volatile,
            Some(label) => SourceType::from_binding_label(label).map_err(js_err)?,
        };
        self.engine
            .update(
                repo,
                spec,
                effective_dt.as_ref(),
                source_type,
                code.to_string(),
            )
            .map_err(serialize_load_errors)
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

    /// Structural quality recommendations across loaded specs. Advisory only.
    #[wasm_bindgen(js_name = quality)]
    pub fn quality_wasm(&self) -> Result<JsValue, JsValue> {
        serialize_engine_json(&self.engine.quality())
    }
}

fn parse_repo_and_effective(
    repository: Option<&String>,
    effective: Option<String>,
) -> Result<(Option<&str>, Option<crate::DateTimeValue>), JsValue> {
    let effective_dt = match effective {
        None => None,
        Some(s) => Some(crate::resolve_effective(Some(s.as_str())).map_err(|e| error_to_js(&e))?),
    };
    let repo = repository
        .map(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    Ok((repo, effective_dt))
}

#[derive(Deserialize)]
struct RunOptions {
    spec: String,
    repository: Option<String>,
    effective: Option<String>,
    data: Option<serde_json::Value>,
    rules: Option<serde_json::Value>,
    explain: Option<bool>,
}

fn parse_run_data(data: &Option<serde_json::Value>) -> Result<HashMap<String, String>, String> {
    let Some(value) = data else {
        return Ok(HashMap::new());
    };
    if value.is_null() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, serde_json::Value> = serde_json::from_value(value.clone())
        .map_err(|e| format!("data must be a plain object: {e}"))?;
    map.into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| {
            let input = run_data_value_from_json_value(v)?;
            match input {
                RunDataValue::String(s) => Ok((k, s)),
                RunDataValue::Boolean(b) => Ok((k, b.to_string())),
                RunDataValue::MeasureMap(m) => {
                    if m.len() == 1 {
                        let (unit, mag) = m.into_iter().next().expect("BUG: single entry map");
                        Ok((k, format!("{mag} {unit}")))
                    } else {
                        Err(format!(
                            "data value '{k}' must be a convenience string for WASM run"
                        ))
                    }
                }
                RunDataValue::RatioMap(m) => {
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

fn resolve_run_rules(rules: &Option<serde_json::Value>) -> Result<Option<Vec<String>>, String> {
    let Some(value) = rules else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    if let Some(s) = value.as_str() {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err("rules must not be empty".to_string());
        }
        return Ok(Some(vec![trimmed.to_string()]));
    }
    if let Some(arr) = value.as_array() {
        if arr.is_empty() {
            return Err("rules must not be empty".to_string());
        }
        let names: Vec<String> = arr
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| "rules must be an array of strings".to_string())
            })
            .collect::<Result<_, _>>()?;
        return Ok(Some(names));
    }
    Err("rules must be a string or array of strings".to_string())
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
                let errors = vec![EngineError::from(&err)];
                return Err(errors
                    .serialize(&js_error_serializer())
                    .expect("BUG: serialize EngineError array"));
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

/// Serializer that emits `null` (not `undefined`) for missing optionals so the object
/// matches the published `EngineError` TypeScript type.
fn js_error_serializer() -> serde_wasm_bindgen::Serializer {
    serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true)
}

/// Convert an engine [`Error`] into a plain JS object thrown from WASM.
fn error_to_js(e: &Error) -> JsValue {
    let err = EngineError::from(e);
    err.serialize(&js_error_serializer())
        .expect("BUG: serialize EngineError")
}

fn serialize_load_errors(load_err: crate::Errors) -> JsValue {
    let errors: Vec<EngineError> = load_err.errors.iter().map(EngineError::from).collect();
    errors
        .serialize(&js_error_serializer())
        .expect("BUG: serialize EngineError array")
}

fn request_error_js(message: impl Into<String>) -> JsValue {
    let err = Error::request(message, None::<String>);
    let errors = vec![EngineError::from(&err)];
    errors
        .serialize(&js_error_serializer())
        .expect("BUG: serialize EngineError array")
}

fn run_data_value_from_json_value(value: serde_json::Value) -> Result<RunDataValue, String> {
    use std::collections::BTreeMap;
    match value {
        serde_json::Value::String(s) => Ok(RunDataValue::String(s)),
        serde_json::Value::Bool(b) => Ok(RunDataValue::Boolean(b)),
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                Ok(RunDataValue::String(n.to_string()))
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
                return Ok(RunDataValue::MeasureMap(map));
            }
            Err("data value object must be a unit map with string magnitudes".to_string())
        }
        serde_json::Value::Null => Err("data value must not be null".to_string()),
        serde_json::Value::Array(_) => Err("data value must not be an array".to_string()),
    }
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
    let map: IndexMap<String, String> = serde_wasm_bindgen::from_value(sources).map_err(|e| {
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
