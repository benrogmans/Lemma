//! JNI bridge for `com.lemmabase.lemma.Native`.

mod error_json;

use error_json::engine_errors_json;
use jni::objects::{JClass, JObjectArray, JString, JThrowable, JValue};
use jni::sys::{jboolean, jlong, jstring, JNI_FALSE};
use jni::JNIEnv;
use lemma::{DateTimeValue, Engine, ResourceLimits, SourceType};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

type EngineHandle = Mutex<Engine>;

fn throw_lemma_exception(env: &mut JNIEnv, message: &str, errors_json: &str) {
    let exception = env
        .find_class("com/lemmabase/lemma/LemmaException")
        .expect("BUG: LemmaException class must exist");
    let msg = env
        .new_string(message)
        .expect("BUG: failed to allocate exception message");
    let errors = env
        .new_string(errors_json)
        .expect("BUG: failed to allocate errors JSON");
    let obj = env
        .new_object(
            exception,
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&msg), JValue::Object(&errors)],
        )
        .expect("BUG: failed to construct LemmaException");
    env.throw(JThrowable::from(obj))
        .expect("BUG: failed to throw LemmaException");
}

fn throw_bug(env: &mut JNIEnv, message: &str) {
    let exception = env
        .find_class("com/lemmabase/lemma/LemmaBugError")
        .expect("BUG: LemmaBugError class must exist");
    let msg = env
        .new_string(message)
        .expect("BUG: failed to allocate bug message");
    let obj = env
        .new_object(exception, "(Ljava/lang/String;)V", &[JValue::Object(&msg)])
        .expect("BUG: failed to construct LemmaBugError");
    env.throw(JThrowable::from(obj))
        .expect("BUG: failed to throw LemmaBugError");
}

fn with_catch<R, F>(env: &mut JNIEnv, default: R, f: F) -> R
where
    F: FnOnce(&mut JNIEnv) -> Result<R, String> + std::panic::UnwindSafe,
{
    match catch_unwind(AssertUnwindSafe(|| f(env))) {
        Ok(Ok(value)) => value,
        Ok(Err(message)) => {
            throw_bug(env, &message);
            default
        }
        Err(_) => {
            throw_bug(env, "BUG: Rust panic crossed JNI boundary");
            default
        }
    }
}

fn handle_from_jlong(handle: jlong) -> Result<&'static EngineHandle, String> {
    if handle == 0 {
        return Err("BUG: engine handle is null (use-after-close or never created)".to_string());
    }
    Ok(unsafe { &*(handle as *const EngineHandle) })
}

fn jstring_required(env: &mut JNIEnv, value: &JString) -> Result<String, String> {
    env.get_string(value)
        .map(|s| s.into())
        .map_err(|e| format!("BUG: failed to read Java string: {e}"))
}

fn jstring_optional(env: &mut JNIEnv, value: &JString) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let s = jstring_required(env, value)?;
    if s.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

fn string_map_from_java(
    env: &mut JNIEnv,
    keys: &JObjectArray,
    values: &JObjectArray,
) -> Result<HashMap<String, String>, String> {
    let len = env
        .get_array_length(keys)
        .map_err(|e| format!("BUG: get_array_length keys: {e}"))?;
    let values_len = env
        .get_array_length(values)
        .map_err(|e| format!("BUG: get_array_length values: {e}"))?;
    if len != values_len {
        return Err("BUG: data key/value array length mismatch".to_string());
    }
    let mut out = HashMap::new();
    for i in 0..len {
        let key_obj = env
            .get_object_array_element(keys, i)
            .map_err(|e| format!("BUG: get key[{i}]: {e}"))?;
        let val_obj = env
            .get_object_array_element(values, i)
            .map_err(|e| format!("BUG: get value[{i}]: {e}"))?;
        let key = jstring_required(env, &JString::from(key_obj))?;
        let value = jstring_required(env, &JString::from(val_obj))?;
        out.insert(key, value);
    }
    Ok(out)
}

fn limits_from_json(raw: &str) -> Result<ResourceLimits, lemma::Error> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| lemma::Error::request(format!("limits JSON: {e}"), None::<String>))?;
    let obj = value
        .as_object()
        .ok_or_else(|| lemma::Error::request("limits must be a JSON object", None::<String>))?;
    let mut limits = ResourceLimits::default();
    for (key, v) in obj {
        let n = v.as_u64().ok_or_else(|| {
            lemma::Error::request(
                format!("limits value for '{key}' must be a non-negative integer"),
                None::<String>,
            )
        })? as usize;
        match key.as_str() {
            "max_sources" => limits.max_sources = n,
            "max_loaded_bytes" => limits.max_loaded_bytes = n,
            "max_source_size_bytes" => limits.max_source_size_bytes = n,
            "max_expression_depth" => limits.max_expression_depth = n,
            "max_expression_count" => limits.max_expression_count = n,
            "max_data_value_bytes" => limits.max_data_value_bytes = n,
            "max_spec_dependency_depth" => limits.max_spec_dependency_depth = n,
            "max_dag_specs" => limits.max_dag_specs = n,
            "max_normalized_expression_nodes" => limits.max_normalized_expression_nodes = n,
            "max_normal_form_depth" => limits.max_normal_form_depth = n,
            other => {
                return Err(lemma::Error::request(
                    format!("unknown limits key: '{other}'"),
                    None::<String>,
                ));
            }
        }
    }
    Ok(limits)
}

fn parse_effective(env: &mut JNIEnv, effective: &JString) -> Result<Option<DateTimeValue>, ()> {
    let raw = match jstring_optional(env, effective) {
        Ok(v) => v,
        Err(message) => {
            throw_bug(env, &message);
            return Err(());
        }
    };
    let Some(raw) = raw else {
        return Ok(None);
    };
    match raw.parse::<DateTimeValue>() {
        Ok(dt) => Ok(Some(dt)),
        Err(e) => {
            let err = lemma::Error::request(format!("Invalid effective date: {e}"), None::<String>);
            throw_lemma_exception(
                env,
                "invalid effective date",
                &engine_errors_json(std::slice::from_ref(&err)),
            );
            Err(())
        }
    }
}

fn return_string(env: &mut JNIEnv, value: String) -> jstring {
    env.new_string(value)
        .expect("BUG: failed to allocate return string")
        .into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_create(
    mut env: JNIEnv,
    _class: JClass,
) -> jlong {
    with_catch(&mut env, 0, |_env| {
        let engine = Box::new(Mutex::new(Engine::new()));
        Ok(Box::into_raw(engine) as jlong)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_createWithLimits(
    mut env: JNIEnv,
    _class: JClass,
    limits_json: JString,
) -> jlong {
    with_catch(&mut env, 0, |env| {
        let raw = jstring_required(env, &limits_json)?;
        let limits = match limits_from_json(&raw) {
            Ok(l) => l,
            Err(err) => {
                throw_lemma_exception(
                    env,
                    "invalid resource limits",
                    &engine_errors_json(std::slice::from_ref(&err)),
                );
                return Ok(0);
            }
        };
        let engine = Box::new(Mutex::new(Engine::with_limits(limits)));
        Ok(Box::into_raw(engine) as jlong)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_destroy(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    with_catch(&mut env, (), |_env| {
        if handle != 0 {
            unsafe {
                drop(Box::from_raw(handle as *mut EngineHandle));
            }
        }
        Ok(())
    });
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_load(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    code: JString,
) {
    with_catch(&mut env, (), |env| {
        let engine = handle_from_jlong(handle)?;
        let code = jstring_required(env, &code)?;
        let mut guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        if let Err(load_err) = guard.load([(SourceType::Volatile, code)]) {
            throw_lemma_exception(env, "load failed", &engine_errors_json(&load_err.errors));
        }
        Ok(())
    });
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_loadLabeled(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    labels: JObjectArray,
    codes: JObjectArray,
) {
    with_catch(&mut env, (), |env| {
        let engine = handle_from_jlong(handle)?;
        let map = string_map_from_java(env, &labels, &codes)?;
        let mut batch = Vec::with_capacity(map.len());
        for (label, code) in map {
            match SourceType::from_binding_label(&label) {
                Ok(source_type) => batch.push((source_type, code)),
                Err(e) => {
                    let err = lemma::Error::request(
                        format!("load: label '{label}': {e}"),
                        None::<String>,
                    );
                    throw_lemma_exception(
                        env,
                        "load failed",
                        &engine_errors_json(std::slice::from_ref(&err)),
                    );
                    return Ok(());
                }
            }
        }
        let mut guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        if let Err(load_err) = guard.load(batch) {
            throw_lemma_exception(env, "load failed", &engine_errors_json(&load_err.errors));
        }
        Ok(())
    });
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_list(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    with_catch(&mut env, std::ptr::null_mut(), |env| {
        let engine = handle_from_jlong(handle)?;
        let guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        let json = serde_json::to_string(&guard.list())
            .map_err(|e| format!("BUG: list serialization failed: {e}"))?;
        Ok(return_string(env, json))
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_show(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    repository: JString,
    spec: JString,
    effective: JString,
) -> jstring {
    with_catch(&mut env, std::ptr::null_mut(), |env| {
        let engine = handle_from_jlong(handle)?;
        let repo = jstring_optional(env, &repository)?;
        let spec = jstring_required(env, &spec)?;
        let effective = match parse_effective(env, &effective) {
            Ok(v) => v,
            Err(()) => return Ok(std::ptr::null_mut()),
        };
        let guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        match guard.show(repo.as_deref(), &spec, effective.as_ref()) {
            Ok(view) => {
                let json = serde_json::to_string(&view)
                    .map_err(|e| format!("BUG: show serialization failed: {e}"))?;
                Ok(return_string(env, json))
            }
            Err(err) => {
                throw_lemma_exception(
                    env,
                    "show failed",
                    &engine_errors_json(std::slice::from_ref(&err)),
                );
                Ok(std::ptr::null_mut())
            }
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_source(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    repository: JString,
    spec: JString,
    effective: JString,
) -> jstring {
    with_catch(&mut env, std::ptr::null_mut(), |env| {
        let engine = handle_from_jlong(handle)?;
        let repo = jstring_optional(env, &repository)?;
        let spec_name = jstring_optional(env, &spec)?;
        let effective = match (&spec_name, parse_effective(env, &effective)) {
            (_, Err(())) => return Ok(std::ptr::null_mut()),
            (Some(_), Ok(v)) => v,
            (None, Ok(_)) => None,
        };
        let guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        match guard.source(repo.as_deref(), spec_name.as_deref(), effective.as_ref()) {
            Ok(text) => Ok(return_string(env, text)),
            Err(err) => {
                throw_lemma_exception(
                    env,
                    "source failed",
                    &engine_errors_json(std::slice::from_ref(&err)),
                );
                Ok(std::ptr::null_mut())
            }
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_run(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    repository: JString,
    spec: JString,
    effective: JString,
    data_keys: JObjectArray,
    data_values: JObjectArray,
    rules: JObjectArray,
    explain: jboolean,
) -> jstring {
    with_catch(&mut env, std::ptr::null_mut(), |env| {
        let engine = handle_from_jlong(handle)?;
        let repo = jstring_optional(env, &repository)?;
        let spec = jstring_required(env, &spec)?;
        let effective = match parse_effective(env, &effective) {
            Ok(v) => v,
            Err(()) => return Ok(std::ptr::null_mut()),
        };
        let data = string_map_from_java(env, &data_keys, &data_values)?;
        let rules = if rules.is_null() {
            None
        } else {
            let len = env
                .get_array_length(&rules)
                .map_err(|e| format!("BUG: get_array_length rules: {e}"))?;
            if len == 0 {
                let err =
                    lemma::Error::request("rules must not be empty".to_string(), None::<String>);
                throw_lemma_exception(
                    env,
                    "run failed",
                    &engine_errors_json(std::slice::from_ref(&err)),
                );
                return Ok(std::ptr::null_mut());
            }
            let mut names = Vec::with_capacity(len as usize);
            for i in 0..len {
                let obj = env
                    .get_object_array_element(&rules, i)
                    .map_err(|e| format!("BUG: get rules[{i}]: {e}"))?;
                names.push(jstring_required(env, &JString::from(obj))?);
            }
            Some(names)
        };
        let explain = explain != JNI_FALSE;
        let guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        match guard.run(
            repo.as_deref(),
            &spec,
            effective.as_ref(),
            data,
            rules.as_deref(),
            explain,
        ) {
            Ok(response) => {
                let json = serde_json::to_string(&response)
                    .map_err(|e| format!("BUG: response serialization failed: {e}"))?;
                Ok(return_string(env, json))
            }
            Err(err) => {
                throw_lemma_exception(
                    env,
                    "run failed",
                    &engine_errors_json(std::slice::from_ref(&err)),
                );
                Ok(std::ptr::null_mut())
            }
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_remove(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    repository: JString,
    spec: JString,
    effective: JString,
) {
    with_catch(&mut env, (), |env| {
        let engine = handle_from_jlong(handle)?;
        let repo = jstring_optional(env, &repository)?;
        let spec = jstring_required(env, &spec)?;
        let effective = match parse_effective(env, &effective) {
            Ok(v) => v,
            Err(()) => return Ok(()),
        };
        let mut guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        if let Err(err) = guard.remove(repo.as_deref(), &spec, effective.as_ref()) {
            throw_lemma_exception(
                env,
                "remove failed",
                &engine_errors_json(std::slice::from_ref(&err)),
            );
        }
        Ok(())
    });
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_format(
    mut env: JNIEnv,
    _class: JClass,
    code: JString,
) -> jstring {
    with_catch(&mut env, std::ptr::null_mut(), |env| {
        let code = jstring_required(env, &code)?;
        match lemma::format_source(&code, SourceType::Volatile) {
            Ok(formatted) => Ok(return_string(env, formatted)),
            Err(err) => {
                throw_lemma_exception(
                    env,
                    "format failed",
                    &engine_errors_json(std::slice::from_ref(&err)),
                );
                Ok(std::ptr::null_mut())
            }
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_limits(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    with_catch(&mut env, std::ptr::null_mut(), |env| {
        let engine = handle_from_jlong(handle)?;
        let guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        let json = serde_json::to_string(guard.limits())
            .map_err(|e| format!("BUG: limits serialization failed: {e}"))?;
        Ok(return_string(env, json))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_from_json_applies_known_keys() {
        let limits = limits_from_json(r#"{"max_sources": 7, "max_dag_specs": 3}"#)
            .expect("valid limits JSON");
        assert_eq!(limits.max_sources, 7);
        assert_eq!(limits.max_dag_specs, 3);
    }

    #[test]
    fn limits_from_json_rejects_unknown_key() {
        let err = limits_from_json(r#"{"not_a_limit": 1}"#).expect_err("unknown key");
        assert!(err.message().contains("unknown limits key"));
    }

    #[test]
    fn limits_from_json_rejects_non_object() {
        let err = limits_from_json("[]").expect_err("array");
        assert!(err.message().contains("limits must be a JSON object"));
    }
}
