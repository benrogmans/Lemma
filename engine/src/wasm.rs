use crate::error::ErrorKind;
use crate::parsing::ast::DateTimeValue;
use crate::parsing::source::Source;
use crate::serialization::data_values_from_map;
use crate::{Engine, Error, SourceType};
use serde::Serialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = Engine)]
pub struct WasmEngine {
    engine: Engine,
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

    /// Load Lemma source. Throws with an array of serialized errors
    /// (same shape as `EngineError` in `engine/packages/npm/lemma.d.ts`).
    #[wasm_bindgen(js_name = load)]
    pub fn load_wasm(&mut self, code: &str, attribute: &str) -> Result<(), JsValue> {
        let source = if attribute.trim().is_empty() {
            SourceType::Volatile
        } else {
            SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(attribute)))
        };
        self.engine.load(code, source).map_err(|load_err| {
            let errors: Vec<JsError> = load_err.errors.iter().map(JsError::from).collect();
            errors
                .serialize(&js_error_serializer())
                .expect("BUG: serialize JsError array")
        })
    }

    /// Load multiple Lemma sources in one planning pass (same as [`Engine::load_batch`]).
    ///
    /// `sources` is a plain object mapping path labels to source text. Labels become
    /// [`SourceType::Path`]; use `""` as a key for [`SourceType::Volatile`].
    ///
    /// `dependency`: when non-empty after trim, sources are tagged as that dependency id.
    ///
    /// Throws with an array of `JsError` on failure.
    #[wasm_bindgen(js_name = load_batch)]
    pub fn load_batch_wasm(
        &mut self,
        sources: JsValue,
        dependency: Option<String>,
    ) -> Result<(), JsValue> {
        let map: HashMap<String, String> = if sources.is_undefined() || sources.is_null() {
            HashMap::new()
        } else {
            serde_wasm_bindgen::from_value(sources).map_err(|e| {
                let err = Error::request(
                    format!(
                        "load_batch: sources must be a plain object with string keys and string values: {e}"
                    ),
                    None::<String>,
                );
                let errors = vec![JsError::from(&err)];
                errors
                    .serialize(&js_error_serializer())
                    .expect("BUG: serialize JsError array")
            })?
        };
        let mut batch: HashMap<SourceType, String> = HashMap::with_capacity(map.len());
        for (key, code) in map {
            let source = if key.trim().is_empty() {
                SourceType::Volatile
            } else {
                SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(key)))
            };
            batch.insert(source, code);
        }
        let owned_dep = dependency.and_then(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        });
        self.engine
            .load_batch(batch, owned_dep.as_deref())
            .map_err(|load_err| {
                let errors: Vec<JsError> = load_err.errors.iter().map(JsError::from).collect();
                errors
                    .serialize(&js_error_serializer())
                    .expect("BUG: serialize JsError array")
            })
    }

    /// Download Lemma source for a registry identifier via [`crate::registry::LemmaBase`]. Returns `{ source, id }`.
    /// Does not load this [`WasmEngine`]; call [`Self::load_batch`], etc., yourself.
    #[wasm_bindgen(js_name = fetch)]
    pub fn fetch_wasm(&self, name: &str) -> js_sys::Promise {
        let _ = self;
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
                    let _: String = normalized;
                    let err = Error::request(
                        "fetch requires the lemma-engine crate to be built with the `registry` feature",
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
    ///
    /// `repository`: repository qualifier (`@org/pkg`), or `null`/empty for workspace (same as
    /// [`Engine::run`] `repo: None`).
    #[wasm_bindgen(js_name = run)]
    pub fn run(
        &self,
        repository: Option<String>,
        spec: &str,
        rule_names: JsValue,
        data_values: JsValue,
        rule_result_units: JsValue,
        effective: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let effective_dt = effective
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.parse::<DateTimeValue>().ok())
            .unwrap_or_else(DateTimeValue::now);

        let rule_names = parse_rule_names(&rule_names).map_err(js_err)?;
        let data = parse_data_values(&data_values).map_err(js_err)?;

        let repo = repository
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let plan = self
            .engine
            .get_plan(repo, spec, Some(&effective_dt))
            .map_err(|e| error_to_js(&e))?;

        let conversion_map = parse_rule_result_units(&rule_result_units).map_err(js_err)?;
        let rule_result_units_list: Vec<String> = conversion_map
            .into_iter()
            .map(|(rule, unit)| format!("{rule}:{unit}"))
            .collect();
        let evaluation_request = if rule_result_units_list.is_empty() {
            crate::evaluation::EvaluationRequest::default()
        } else {
            let joined = rule_result_units_list.join(",");
            let strings = crate::evaluation::parse_rule_result_conversion_strings(&joined)
                .map_err(|e| error_to_js(&e))?;
            if !rule_names.is_empty() {
                for rule_name in strings.keys() {
                    if !rule_names.contains(rule_name) {
                        return Err(js_err(format!(
                            "Conversion for rule '{rule_name}' was requested but that rule is not in rule_names"
                        )));
                    }
                }
            }
            crate::evaluation::EvaluationRequest::from_rule_conversion_strings(strings, plan)
                .map_err(|e| error_to_js(&e))?
        };

        let mut response = self
            .engine
            .run_plan(plan, Some(&effective_dt), data, false, evaluation_request)
            .map_err(|e| error_to_js(&e))?;

        if !rule_names.is_empty() {
            response.filter_rules(&rule_names);
        }

        serialize_engine_json(&response)
    }

    /// Same data as [`Engine::list`]: grouped [`ResolvedRepository`] JSON without planning.
    #[wasm_bindgen(js_name = list)]
    pub fn list_wasm(&self) -> Result<JsValue, JsValue> {
        serialize_engine_json(&self.engine.list())
    }

    /// Canonical Lemma source for `repository`, formatted from the in-engine AST (e.g. `"lemma"`).
    #[wasm_bindgen(js_name = format_repository)]
    pub fn format_repository_wasm(&self, repository: &str) -> Result<String, JsValue> {
        self.engine
            .format_repository(repository)
            .map_err(|e| error_to_js(&e))
    }

    /// Planning schema for the spec ([`crate::planning::execution_plan::SpecSchema`]). Throws on error.
    ///
    /// `repository`: qualifier string or `null`/empty for workspace ([`Engine::schema`]).
    #[wasm_bindgen(js_name = schema)]
    pub fn schema(
        &self,
        repository: Option<String>,
        spec: &str,
        effective: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let effective_dt = effective
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.parse::<DateTimeValue>().ok())
            .unwrap_or_else(DateTimeValue::now);

        let repo = repository
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let plan = self
            .engine
            .get_plan(repo, spec, Some(&effective_dt))
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
            None => "inline source (no path)",
        };
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

    /// Loaded repositories (workspace and dependencies): `{ name, dependency }`.
    #[wasm_bindgen(js_name = repositories)]
    pub fn repositories(&self) -> Result<JsValue, JsValue> {
        let rows: Vec<serde_json::Value> = self
            .engine
            .list()
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.repository.name,
                    "dependency": r.repository.dependency,
                })
            })
            .collect();
        serialize_engine_json(&rows)
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
            source: bundle.lemma_source,
            id: name,
        };
        serialize_engine_json(&payload)
    })
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

fn parse_rule_result_units(v: &JsValue) -> Result<HashMap<String, String>, String> {
    if v.is_undefined() || v.is_null() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, String> = serde_wasm_bindgen::from_value(v.clone()).map_err(|e| {
        format!("rule_result_units must be a plain object of rule name to unit: {e}")
    })?;
    Ok(map)
}

fn parse_data_values(v: &JsValue) -> Result<HashMap<String, serde_json::Value>, String> {
    if v.is_undefined() || v.is_null() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, serde_json::Value> = serde_wasm_bindgen::from_value(v.clone())
        .map_err(|e| format!("data_values must be a plain object: {}", e))?;
    Ok(data_values_from_map(map))
}
