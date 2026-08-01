#![recursion_limit = "256"]

mod error_encoding;

use error_encoding::encode_error;
use lemma::{DateTimeValue, Engine, ResourceLimits, SourceType};
use rustler::types::atom;
use rustler::types::MapIterator;
use rustler::{Binary, Encoder, Env, NifResult, OwnedBinary, Resource, ResourceArc, Term};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct LemmaEngineResource(pub Mutex<Engine>);

impl Resource for LemmaEngineResource {}

fn load(env: Env, _info: Term) -> bool {
    env.register::<LemmaEngineResource>().is_ok()
}

#[rustler::nif]
fn lemma_new<'a>(env: Env<'a>, limits_term: Option<Term<'a>>) -> NifResult<Term<'a>> {
    let engine = match limits_term {
        None => Engine::new(),
        Some(term) => {
            if term.as_c_arg() == atom::nil().as_c_arg() {
                Engine::new()
            } else {
                let limits = limits_from_term(term)
                    .map_err(|msg| rustler::Error::RaiseTerm(Box::new(msg)))?;
                Engine::with_limits(limits)
            }
        }
    };
    let resource = ResourceArc::new(LemmaEngineResource(Mutex::new(engine)));
    Ok((rustler::Atom::from_str(env, "ok")?, resource).encode(env))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn lemma_load<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    sources_term: Term<'a>,
) -> NifResult<Term<'a>> {
    let batch = match sources_from_load_term(sources_term) {
        Ok(b) => b,
        Err(message) => {
            let err = lemma::Error::request(message, None::<String>);
            let list = error_encoding::encode_errors(env, &[err])?;
            return Ok((rustler::Atom::from_str(env, "error")?, list).encode(env));
        }
    };
    let mut engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;
    match engine.load(batch) {
        Ok(()) => Ok(rustler::Atom::from_str(env, "ok")?.encode(env)),
        Err(load_err) => {
            let list = error_encoding::encode_errors(env, &load_err.errors)?;
            Ok((rustler::Atom::from_str(env, "error")?, list).encode(env))
        }
    }
}

fn sources_from_load_term(term: Term) -> Result<Vec<(SourceType, String)>, String> {
    if let Ok(binary) = term.decode::<Binary>() {
        let code = std::str::from_utf8(&binary)
            .map_err(|_| "load: source text must be valid UTF-8".to_string())?;
        return Ok(vec![(SourceType::Volatile, code.to_string())]);
    }
    if let Some(iter) = MapIterator::new(term) {
        return sources_from_label_map(iter);
    }
    if let Ok(list) = term.decode::<Vec<(String, String)>>() {
        return sources_from_label_pairs(list);
    }
    Err("load: sources must be a binary, a map, or a list of {label, code} tuples".to_string())
}

fn sources_from_label_map(iter: MapIterator) -> Result<Vec<(SourceType, String)>, String> {
    // BEAM maps have no insertion-order contract. Lexicographic label order is the
    // documented map-load contract; list-of-tuples preserves caller order instead.
    let mut pairs = Vec::new();
    for (key, value) in iter {
        let label: String = key
            .decode()
            .map_err(|_| "load: map keys must be strings".to_string())?;
        let code: String = value
            .decode()
            .map_err(|_| "load: map values must be strings (Lemma source text)".to_string())?;
        pairs.push((label, code));
    }
    pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
    pairs
        .into_iter()
        .map(|(label, code)| {
            SourceType::from_binding_label(&label)
                .map(|source_type| (source_type, code))
                .map_err(|e| format!("load: label '{label}': {e}"))
        })
        .collect()
}

fn sources_from_label_pairs(
    pairs: Vec<(String, String)>,
) -> Result<Vec<(SourceType, String)>, String> {
    pairs
        .into_iter()
        .enumerate()
        .map(|(i, (label, code))| {
            SourceType::from_binding_label(&label)
                .map(|source_type| (source_type, code))
                .map_err(|e| format!("load: entry {i}: {e}"))
        })
        .collect()
}

#[rustler::nif]
fn lemma_list<'a>(env: Env<'a>, resource: ResourceArc<LemmaEngineResource>) -> NifResult<Term<'a>> {
    let engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;

    let repos = engine.list();
    let json = serde_json::to_vec(&repos).map_err(|e| {
        rustler::Error::RaiseTerm(Box::new(format!("List serialization failed: {e}")))
    })?;
    let mut owned = OwnedBinary::new(json.len()).ok_or_else(|| {
        rustler::Error::RaiseTerm(Box::new("Binary allocation failed".to_string()))
    })?;
    owned.as_mut_slice().copy_from_slice(&json);
    let binary = Binary::from_owned(owned, env);
    Ok((rustler::Atom::from_str(env, "ok")?, binary).encode(env))
}

#[rustler::nif]
fn lemma_show<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    repository: Option<String>,
    spec: String,
    effective_opt: Option<String>,
) -> NifResult<Term<'a>> {
    let engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;
    let effective = match effective_opt {
        Some(s) => Some(s.parse::<DateTimeValue>().map_err(|e| {
            rustler::Error::RaiseTerm(Box::new(format!("Invalid effective date: {}", e)))
        })?),
        None => None,
    };
    let repo = repository
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match engine.show(repo, &spec, effective.as_ref()) {
        Ok(view) => {
            let json = serde_json::to_vec(&view).map_err(|e| {
                rustler::Error::RaiseTerm(Box::new(format!("Show serialization failed: {}", e)))
            })?;
            let mut owned = OwnedBinary::new(json.len()).ok_or_else(|| {
                rustler::Error::RaiseTerm(Box::new("Binary allocation failed".to_string()))
            })?;
            owned.as_mut_slice().copy_from_slice(&json);
            let binary = rustler::Binary::from_owned(owned, env);
            Ok((rustler::Atom::from_str(env, "ok")?, binary).encode(env))
        }
        Err(err) => {
            let term = encode_error(env, &err)?;
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_source<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    repository: Option<String>,
    spec: Option<String>,
    effective_opt: Option<String>,
) -> NifResult<Term<'a>> {
    let engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;
    let repo = repository
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let spec_name = spec.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let effective = match (spec_name, effective_opt) {
        (Some(_), Some(s)) => Some(s.parse::<DateTimeValue>().map_err(|e| {
            rustler::Error::RaiseTerm(Box::new(format!("Invalid effective date: {}", e)))
        })?),
        (Some(_), None) => None,
        _ => None,
    };
    match engine.source(repo, spec_name, effective.as_ref()) {
        Ok(text) => Ok((rustler::Atom::from_str(env, "ok")?, text).encode(env)),
        Err(err) => {
            let term = encode_error(env, &err)?;
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn lemma_run<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    target: Term<'a>,
    options: Term<'a>,
) -> NifResult<Term<'a>> {
    let (repository, spec, effective_opt) = decode_run_target(target)?;
    let RunOptions {
        data: data_values,
        rules,
        explain,
    } = decode_run_options(options)?;
    let engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;
    let effective = match effective_opt {
        Some(s) => Some(&s.parse::<DateTimeValue>().map_err(|e| {
            rustler::Error::RaiseTerm(Box::new(format!("Invalid effective date: {}", e)))
        })?),
        None => None,
    };
    let repo = repository
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let rules = match rules {
        None => None,
        Some(names) if names.is_empty() => {
            return Err(rustler::Error::RaiseTerm(Box::new(
                "rules must not be empty".to_string(),
            )));
        }
        Some(names) => Some(names),
    };
    match engine.run(
        repo,
        &spec,
        effective,
        data_values,
        rules.as_deref(),
        explain,
    ) {
        Ok(response) => {
            let json = serde_json::to_vec(&response).map_err(|e| {
                rustler::Error::RaiseTerm(Box::new(format!("Response serialization failed: {}", e)))
            })?;
            let mut owned = OwnedBinary::new(json.len()).ok_or_else(|| {
                rustler::Error::RaiseTerm(Box::new("Binary allocation failed".to_string()))
            })?;
            owned.as_mut_slice().copy_from_slice(&json);
            let binary = rustler::Binary::from_owned(owned, env);
            Ok((rustler::Atom::from_str(env, "ok")?, binary).encode(env))
        }
        Err(err) => {
            let term = encode_error(env, &err)?;
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_remove<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    repository: Option<String>,
    spec_name: String,
    effective: Option<String>,
) -> NifResult<Term<'a>> {
    let mut engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;
    let effective_dt = match effective {
        None => None,
        Some(s) => Some(s.parse::<DateTimeValue>().map_err(|e| {
            rustler::Error::RaiseTerm(Box::new(format!("Invalid effective date: {}", e)))
        })?),
    };
    let repo = repository
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match engine.remove(repo, &spec_name, effective_dt.as_ref()) {
        Ok(()) => Ok(rustler::Atom::from_str(env, "ok")?.encode(env)),
        Err(err) => {
            let term = encode_error(env, &err)?;
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_format<'a>(env: Env<'a>, code: String) -> NifResult<Term<'a>> {
    match lemma::format_source(&code, SourceType::Volatile) {
        Ok(formatted) => Ok((rustler::Atom::from_str(env, "ok")?, formatted).encode(env)),
        Err(err) => {
            let term = encode_error(env, &err)?;
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_generate_openapi<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    explanations_enabled: bool,
    effective_opt: Option<String>,
) -> NifResult<Term<'a>> {
    let engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;

    let effective = match effective_opt {
        None => DateTimeValue::now(),
        Some(s) => s.parse::<DateTimeValue>().map_err(|e| {
            rustler::Error::RaiseTerm(Box::new(format!("Invalid effective date: {}", e)))
        })?,
    };

    let spec = lemma_openapi::generate_openapi_effective(&engine, explanations_enabled, &effective);
    let json = serde_json::to_vec(&spec).map_err(|e| {
        rustler::Error::RaiseTerm(Box::new(format!(
            "OpenAPI JSON serialization failed: {}",
            e
        )))
    })?;
    let mut owned = OwnedBinary::new(json.len()).ok_or_else(|| {
        rustler::Error::RaiseTerm(Box::new("Binary allocation failed".to_string()))
    })?;
    owned.as_mut_slice().copy_from_slice(&json);
    let binary = rustler::Binary::from_owned(owned, env);
    Ok((rustler::Atom::from_str(env, "ok")?, binary).encode(env))
}

/// Temporal version choices (title + slug) for API docs, aligned with `lemma_openapi::temporal_api_sources`.
#[rustler::nif]
fn lemma_temporal_api_sources<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
) -> NifResult<Term<'a>> {
    let engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;

    let sources = lemma_openapi::temporal_api_sources(&engine);
    let rows: Vec<_> = sources
        .into_iter()
        .map(|s| {
            json!({
                "title": s.title,
                "slug": s.slug,
            })
        })
        .collect();

    let json = serde_json::to_vec(&rows).map_err(|e| {
        rustler::Error::RaiseTerm(Box::new(format!("temporal API sources JSON failed: {}", e)))
    })?;
    let mut owned = OwnedBinary::new(json.len()).ok_or_else(|| {
        rustler::Error::RaiseTerm(Box::new("Binary allocation failed".to_string()))
    })?;
    owned.as_mut_slice().copy_from_slice(&json);
    let binary = rustler::Binary::from_owned(owned, env);
    Ok((rustler::Atom::from_str(env, "ok")?, binary).encode(env))
}

fn limits_from_term(term: Term) -> Result<ResourceLimits, String> {
    let iter = MapIterator::new(term).ok_or_else(|| "limits must be a map".to_string())?;
    let mut limits = ResourceLimits::default();
    for (key, value) in iter {
        let key_str: String = key
            .decode()
            .map_err(|_| "limits map keys must be strings".to_string())?;
        let value_int: i64 = value
            .decode()
            .map_err(|_| format!("limits value for '{}' must be an integer", key_str))?;
        if value_int < 0 {
            return Err(format!(
                "limits value for '{}' must be non-negative",
                key_str
            ));
        }
        let value_usize = value_int as usize;
        match key_str.as_str() {
            "max_sources" => limits.max_sources = value_usize,
            "max_loaded_bytes" => limits.max_loaded_bytes = value_usize,
            "max_source_size_bytes" => limits.max_source_size_bytes = value_usize,
            "max_expression_depth" => limits.max_expression_depth = value_usize,
            "max_expression_count" => limits.max_expression_count = value_usize,
            "max_data_value_bytes" => limits.max_data_value_bytes = value_usize,
            "max_spec_dependency_depth" => limits.max_spec_dependency_depth = value_usize,
            "max_dag_specs" => limits.max_dag_specs = value_usize,
            "max_normalized_expression_nodes" => {
                limits.max_normalized_expression_nodes = value_usize
            }
            _ => return Err(format!("unknown limits key: '{}'", key_str)),
        }
    }
    Ok(limits)
}

fn map_key_string(term: Term) -> Result<String, rustler::Error> {
    if let Ok(s) = term.atom_to_string() {
        return Ok(s);
    }
    term.decode::<String>().map_err(|_| rustler::Error::BadArg)
}

fn map_term_to_data_values(term: Term) -> Result<HashMap<String, String>, rustler::Error> {
    let iter = MapIterator::new(term).ok_or(rustler::Error::BadArg)?;
    let mut result = HashMap::new();
    for (key, value) in iter {
        let key_str = map_key_string(key)?;
        let value_str = term_to_string(value)?;
        result.insert(key_str, value_str);
    }
    Ok(result)
}

fn optional_string(term: Term) -> Result<Option<String>, rustler::Error> {
    if term.as_c_arg() == atom::nil().as_c_arg() {
        return Ok(None);
    }
    Ok(Some(
        term.decode::<String>()
            .map_err(|_| rustler::Error::BadArg)?,
    ))
}

fn decode_run_target(
    term: Term,
) -> Result<(Option<String>, String, Option<String>), rustler::Error> {
    let iter = MapIterator::new(term).ok_or(rustler::Error::BadArg)?;
    let mut repository = None;
    let mut spec = None;
    let mut effective = None;
    for (key, value) in iter {
        let key_str = map_key_string(key)?;
        match key_str.as_str() {
            "repo" | "repository" => repository = optional_string(value)?,
            "spec" => {
                spec = Some(
                    value
                        .decode::<String>()
                        .map_err(|_| rustler::Error::BadArg)?,
                )
            }
            "effective" => effective = optional_string(value)?,
            other => {
                return Err(rustler::Error::RaiseTerm(Box::new(format!(
                    "unknown run target key: '{other}'"
                ))));
            }
        }
    }
    let spec = spec.ok_or(rustler::Error::RaiseTerm(Box::new(
        "run target must include :spec".to_string(),
    )))?;
    Ok((repository, spec, effective))
}

struct RunOptions {
    data: HashMap<String, String>,
    rules: Option<Vec<String>>,
    explain: bool,
}

fn decode_run_options(term: Term) -> Result<RunOptions, rustler::Error> {
    if term.as_c_arg() == atom::nil().as_c_arg() {
        return Ok(RunOptions {
            data: HashMap::new(),
            rules: None,
            explain: false,
        });
    }
    let iter = MapIterator::new(term).ok_or(rustler::Error::BadArg)?;
    let mut data = HashMap::new();
    let mut rules = None;
    let mut explain = false;
    for (key, value) in iter {
        let key_str = map_key_string(key)?;
        match key_str.as_str() {
            "data" => data = map_term_to_data_values(value)?,
            "rules" => {
                if value.as_c_arg() == atom::nil().as_c_arg() {
                    rules = None;
                } else {
                    rules = Some(
                        value
                            .decode::<Vec<String>>()
                            .map_err(|_| rustler::Error::BadArg)?,
                    );
                }
            }
            "explain" => explain = value.decode::<bool>().map_err(|_| rustler::Error::BadArg)?,
            other => {
                return Err(rustler::Error::RaiseTerm(Box::new(format!(
                    "unknown run options key: '{other}'"
                ))));
            }
        }
    }
    Ok(RunOptions {
        data,
        rules,
        explain,
    })
}

fn term_to_string(term: Term) -> Result<String, rustler::Error> {
    if let Ok(s) = term.atom_to_string() {
        return Ok(s);
    }
    if let Ok(s) = term.decode::<String>() {
        return Ok(s);
    }
    if let Ok(i) = term.decode::<i64>() {
        return Ok(i.to_string());
    }
    if term.decode::<f64>().is_ok() {
        return Err(rustler::Error::RaiseTerm(Box::new(
            "decimal values must be passed as strings to preserve exactness".to_string(),
        )));
    }
    Err(rustler::Error::RaiseTerm(Box::new(
        "data value must be a string, integer, or atom".to_string(),
    )))
}

rustler::init!("Elixir.Lemma.Native", load = load);
