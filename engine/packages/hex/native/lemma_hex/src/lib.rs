#![recursion_limit = "256"]

mod error_encoding;

use error_encoding::encode_error;
use lemma::inversion::{Target, TargetOp};
use lemma::parsing::ast::{DateTimeValue, Value};
use lemma::planning::semantics::{
    value_to_semantic, LiteralValue, SemanticDateTime, SemanticTimezone,
};
use lemma::{Engine, OperationResult, ResourceLimits, SourceType};
use rustler::types::atom;
use rustler::types::MapIterator;
use rustler::{Encoder, Env, NifResult, OwnedBinary, Resource, ResourceArc, Term};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
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

#[rustler::nif]
fn lemma_load<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    code: String,
    source_label: String,
) -> NifResult<Term<'a>> {
    let source = if source_label.trim().is_empty() {
        SourceType::Inline
    } else {
        SourceType::Labeled(source_label.as_str())
    };
    let mut engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;
    match engine.load(&code, source) {
        Ok(()) => Ok(rustler::Atom::from_str(env, "ok")?.encode(env)),
        Err(load_err) => {
            let list = error_encoding::encode_errors(env, &load_err.errors);
            Ok((rustler::Atom::from_str(env, "error")?, list).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_load_from_paths<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    paths: Vec<String>,
) -> NifResult<Term<'a>> {
    let path_refs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let mut engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;
    match engine.load_from_paths(&path_refs, false) {
        Ok(()) => Ok(rustler::Atom::from_str(env, "ok")?.encode(env)),
        Err(load_err) => {
            let list = error_encoding::encode_errors(env, &load_err.errors);
            Ok((rustler::Atom::from_str(env, "error")?, list).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_list<'a>(env: Env<'a>, resource: ResourceArc<LemmaEngineResource>) -> NifResult<Term<'a>> {
    let engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;

    let datetime_term = |dt: Option<DateTimeValue>| -> Term<'a> {
        match dt {
            Some(d) => d.to_string().encode(env),
            None => atom::nil().encode(env),
        }
    };

    let mut items: Vec<Term<'a>> = Vec::new();
    for (spec, effective_from, effective_to) in engine.list_specs_with_ranges() {
        let plan = match engine.get_plan(&spec.name, effective_from.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                return Err(rustler::Error::RaiseTerm(Box::new(format!(
                    "Failed to get plan for '{}': {}",
                    spec.name, e
                ))))
            }
        };
        let schema_json = serde_json::to_vec(&plan.schema()).map_err(|e| {
            rustler::Error::RaiseTerm(Box::new(format!("Schema serialization failed: {}", e)))
        })?;
        let mut schema_bin = OwnedBinary::new(schema_json.len()).ok_or_else(|| {
            rustler::Error::RaiseTerm(Box::new("Binary allocation failed".to_string()))
        })?;
        schema_bin.as_mut_slice().copy_from_slice(&schema_json);

        let mut map = rustler::types::map::map_new(env);
        map = map
            .map_put(
                rustler::Atom::from_str(env, "name").unwrap().encode(env),
                spec.name.as_str().encode(env),
            )
            .unwrap();
        map = map
            .map_put(
                rustler::Atom::from_str(env, "effective_from")
                    .unwrap()
                    .encode(env),
                datetime_term(effective_from),
            )
            .unwrap();
        map = map
            .map_put(
                rustler::Atom::from_str(env, "effective_to")
                    .unwrap()
                    .encode(env),
                datetime_term(effective_to),
            )
            .unwrap();
        map = map
            .map_put(
                rustler::Atom::from_str(env, "schema").unwrap().encode(env),
                rustler::Binary::from_owned(schema_bin, env).to_term(env),
            )
            .unwrap();
        items.push(map);
    }
    Ok((rustler::Atom::from_str(env, "ok")?, items).encode(env))
}

#[rustler::nif]
fn lemma_schema<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    spec: String,
    effective_opt: Option<String>,
) -> NifResult<Term<'a>> {
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

    match engine.schema(&spec, effective) {
        Ok(schema) => {
            let json = serde_json::to_vec(&schema).map_err(|e| {
                rustler::Error::RaiseTerm(Box::new(format!("Schema serialization failed: {}", e)))
            })?;
            let mut owned = OwnedBinary::new(json.len()).ok_or_else(|| {
                rustler::Error::RaiseTerm(Box::new("Binary allocation failed".to_string()))
            })?;
            owned.as_mut_slice().copy_from_slice(&json);
            let binary = rustler::Binary::from_owned(owned, env);
            Ok((rustler::Atom::from_str(env, "ok")?, binary).encode(env))
        }
        Err(err) => {
            let term = encode_error(env, &err);
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_run<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    spec: String,
    effective_opt: Option<String>,
    data_values: Term<'a>,
) -> NifResult<Term<'a>> {
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
    let values = map_term_to_data_values(data_values)?;
    match engine.run(&spec, effective, values, false) {
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
            let term = encode_error(env, &err);
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_invert<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    spec_name: String,
    effective_opt: Option<String>,
    rule_name: String,
    target_term: Term<'a>,
    values: Term<'a>,
) -> NifResult<Term<'a>> {
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
    let data_values = map_term_to_data_values(values)?;
    let target = decode_target(env, target_term)?;
    match engine.invert(&spec_name, effective, &rule_name, target, data_values) {
        Ok(inversion) => {
            let json = serde_json::to_vec(&inversion).map_err(|e| {
                rustler::Error::RaiseTerm(Box::new(format!(
                    "Inversion serialization failed: {}",
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
        Err(err) => {
            let term = encode_error(env, &err);
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_remove_spec<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    spec_name: String,
    effective: String,
) -> NifResult<Term<'a>> {
    let mut engine = resource
        .0
        .lock()
        .map_err(|_| rustler::Error::RaiseTerm(Box::new("Engine lock poisoned".to_string())))?;
    let effective_dt = effective.parse::<DateTimeValue>().map_err(|e| {
        rustler::Error::RaiseTerm(Box::new(format!("Invalid effective date: {}", e)))
    })?;
    match engine.remove(&spec_name, Some(&effective_dt)) {
        Ok(()) => Ok(rustler::Atom::from_str(env, "ok")?.encode(env)),
        Err(err) => {
            let term = encode_error(env, &err);
            Ok((rustler::Atom::from_str(env, "error")?, term).encode(env))
        }
    }
}

#[rustler::nif]
fn lemma_execution_plan<'a>(
    env: Env<'a>,
    resource: ResourceArc<LemmaEngineResource>,
    spec: String,
    effective_opt: Option<String>,
) -> NifResult<Term<'a>> {
    let plan = {
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
        match engine.get_plan(&spec, effective) {
            Ok(p) => p.clone(),
            Err(err) => {
                let term = encode_error(env, &err);
                return Ok((rustler::Atom::from_str(env, "error")?, term).encode(env));
            }
        }
    };
    let json = serde_json::to_vec(&plan).map_err(|e| {
        rustler::Error::RaiseTerm(Box::new(format!(
            "Execution plan serialization failed: {}",
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

#[rustler::nif]
fn lemma_format<'a>(env: Env<'a>, code: String) -> NifResult<Term<'a>> {
    match lemma::format_source(&code, SourceType::INLINE_KEY) {
        Ok(formatted) => Ok((rustler::Atom::from_str(env, "ok")?, formatted).encode(env)),
        Err(err) => {
            let term = encode_error(env, &err);
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
            "max_files" => limits.max_files = value_usize,
            "max_loaded_bytes" => limits.max_loaded_bytes = value_usize,
            "max_file_size_bytes" => limits.max_file_size_bytes = value_usize,
            "max_total_expression_count" => limits.max_total_expression_count = value_usize,
            "max_expression_depth" => limits.max_expression_depth = value_usize,
            "max_expression_count" => limits.max_expression_count = value_usize,
            "max_data_value_bytes" => limits.max_data_value_bytes = value_usize,
            _ => return Err(format!("unknown limits key: '{}'", key_str)),
        }
    }
    Ok(limits)
}

fn map_term_to_data_values(term: Term) -> Result<HashMap<String, String>, rustler::Error> {
    let iter = MapIterator::new(term).ok_or(rustler::Error::BadArg)?;
    let mut result = HashMap::new();
    for (key, value) in iter {
        let key_str: String = key.decode().map_err(|_| rustler::Error::BadArg)?;
        let value_str = term_to_string(value)?;
        result.insert(key_str, value_str);
    }
    Ok(result)
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
    if let Ok(f) = term.decode::<f64>() {
        return Ok(f.to_string());
    }
    Err(rustler::Error::RaiseTerm(Box::new(
        "data value must be a string, integer, float, or atom".to_string(),
    )))
}

fn get_atom_key<'a>(env: Env<'a>, map: Term<'a>, key: &str) -> Option<Term<'a>> {
    let atom_key = rustler::Atom::from_str(env, key).ok()?;
    map.map_get(atom_key.encode(env)).ok()
}

fn decode_target<'a>(env: Env<'a>, term: Term<'a>) -> Result<Target, rustler::Error> {
    let outcome_term = get_atom_key(env, term, "outcome").ok_or_else(|| {
        rustler::Error::RaiseTerm(Box::new("target map requires :outcome key".to_string()))
    })?;
    let outcome: String = outcome_term
        .atom_to_string()
        .or_else(|_| outcome_term.decode::<String>())
        .map_err(|_| {
            rustler::Error::RaiseTerm(Box::new(
                "target :outcome must be a string or atom".to_string(),
            ))
        })?;

    let op_str: String = get_atom_key(env, term, "op")
        .and_then(|t| t.atom_to_string().or_else(|_| t.decode::<String>()).ok())
        .unwrap_or_else(|| "eq".to_string());

    let op = match op_str.as_str() {
        "eq" => TargetOp::Eq,
        "neq" => TargetOp::Neq,
        "lt" => TargetOp::Lt,
        "lte" => TargetOp::Lte,
        "gt" => TargetOp::Gt,
        "gte" => TargetOp::Gte,
        other => {
            return Err(rustler::Error::RaiseTerm(Box::new(format!(
                "unknown target op: '{}'",
                other
            ))));
        }
    };

    match outcome.as_str() {
        "any_value" => Ok(Target::any_value()),
        "any_veto" => Ok(Target::any_veto()),
        "veto" => {
            let message: Option<String> =
                get_atom_key(env, term, "message").and_then(|t| t.decode().ok());
            Ok(Target::veto(message))
        }
        "value" => {
            let value_term = get_atom_key(env, term, "value").ok_or_else(|| {
                rustler::Error::RaiseTerm(Box::new(
                    "target with outcome 'value' requires a :value field".to_string(),
                ))
            })?;
            let value_str = term_to_string(value_term)?;
            let literal = parse_value_string_to_literal(&value_str)?;
            let result = OperationResult::Value(Box::new(literal));
            Ok(Target::with_op(op, result))
        }
        other => Err(rustler::Error::RaiseTerm(Box::new(format!(
            "unknown target outcome: '{}' (expected: value, veto, any_value, any_veto)",
            other
        )))),
    }
}

fn parse_value_string_to_literal(s: &str) -> Result<LiteralValue, rustler::Error> {
    if let Ok(b) = s.parse::<lemma::parsing::ast::BooleanValue>() {
        let value = Value::Boolean(b);
        let value_kind = value_to_semantic(&value).map_err(|e| {
            rustler::Error::RaiseTerm(Box::new(format!("Value conversion failed: {}", e)))
        })?;
        return Ok(LiteralValue {
            value: value_kind,
            lemma_type: lemma::planning::semantics::primitive_boolean().clone(),
        });
    }
    if let Ok(n) = rust_decimal::Decimal::from_str(s) {
        return Ok(LiteralValue::number(n));
    }
    if let Ok(dt) = s.parse::<DateTimeValue>() {
        let sem_dt = SemanticDateTime {
            year: dt.year,
            month: dt.month,
            day: dt.day,
            hour: dt.hour,
            minute: dt.minute,
            second: dt.second,
            microsecond: dt.microsecond,
            timezone: dt.timezone.as_ref().map(|tz| SemanticTimezone {
                offset_hours: tz.offset_hours,
                offset_minutes: tz.offset_minutes,
            }),
        };
        return Ok(LiteralValue::date(sem_dt));
    }
    Ok(LiteralValue::text(s.to_string()))
}

rustler::init!("Elixir.Lemma.Native", load = load);
