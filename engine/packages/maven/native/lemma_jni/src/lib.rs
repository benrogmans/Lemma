//! JNI bridge for `com.lemmabase.lemma.Native`.

mod error_json;

use error_json::engine_errors_json;
use jni::objects::{JClass, JObject, JObjectArray, JString, JThrowable, JValue};
use jni::sys::{jboolean, jlong, jstring, JNI_FALSE};
use jni::{jni_sig, jni_str, Env, EnvUnowned};
use lemma::{DateTimeValue, Engine, ResourceLimits, SourceType};
use std::collections::HashMap;
use std::sync::Mutex;

type EngineHandle = Mutex<Engine>;

#[derive(Debug)]
enum BridgeError {
    Bug(String),
    Jni(jni::errors::Error),
}

impl From<jni::errors::Error> for BridgeError {
    fn from(error: jni::errors::Error) -> Self {
        Self::Jni(error)
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bug(message) => write!(f, "{message}"),
            Self::Jni(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BridgeError {}

struct ThrowLemmaBugAndDefault;

impl<T: Default> jni::errors::ErrorPolicy<T, BridgeError> for ThrowLemmaBugAndDefault {
    type Captures<'unowned_env_local: 'native_method, 'native_method> = ();

    fn on_error<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _captures: &mut Self::Captures<'unowned_env_local, 'native_method>,
        err: BridgeError,
    ) -> jni::errors::Result<T> {
        if env.exception_check() {
            return Ok(T::default());
        }
        let message = match err {
            BridgeError::Bug(message) => message,
            BridgeError::Jni(error) => format!("BUG: JNI error: {error}"),
        };
        throw_bug(env, &message);
        Ok(T::default())
    }

    fn on_panic<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _captures: &mut Self::Captures<'unowned_env_local, 'native_method>,
        _payload: Box<dyn std::any::Any + Send + 'static>,
    ) -> jni::errors::Result<T> {
        if env.exception_check() {
            return Ok(T::default());
        }
        throw_bug(env, "BUG: Rust panic crossed JNI boundary");
        Ok(T::default())
    }
}

fn throw_lemma_exception(env: &mut Env, message: &str, errors_json: &str) {
    let exception = env
        .find_class(jni_str!("com/lemmabase/lemma/LemmaException"))
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
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)V"),
            &[JValue::Object(&msg), JValue::Object(&errors)],
        )
        .expect("BUG: failed to construct LemmaException");
    let throwable = env
        .cast_local::<JThrowable>(obj)
        .expect("BUG: LemmaException must be throwable");
    // jni 0.22: throw returns Err(JavaException) after a successful throw.
    let _ = env.throw(throwable);
}

fn throw_bug(env: &mut Env, message: &str) {
    let exception = env
        .find_class(jni_str!("com/lemmabase/lemma/LemmaBugError"))
        .expect("BUG: LemmaBugError class must exist");
    let msg = env
        .new_string(message)
        .expect("BUG: failed to allocate bug message");
    let obj = env
        .new_object(
            exception,
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&msg)],
        )
        .expect("BUG: failed to construct LemmaBugError");
    let throwable = env
        .cast_local::<JThrowable>(obj)
        .expect("BUG: LemmaBugError must be throwable");
    let _ = env.throw(throwable);
}

fn with_catch<'local, R, F>(unowned: &mut EnvUnowned<'local>, f: F) -> R
where
    R: Default,
    F: FnOnce(&mut Env<'local>) -> Result<R, String>,
{
    unowned
        .with_env(|env| f(env).map_err(BridgeError::Bug))
        .resolve::<ThrowLemmaBugAndDefault>()
}

fn handle_from_jlong(handle: jlong) -> Result<&'static EngineHandle, String> {
    if handle == 0 {
        return Err("BUG: engine handle is null (use-after-close or never created)".to_string());
    }
    Ok(unsafe { &*(handle as *const EngineHandle) })
}

fn jstring_required(env: &Env, value: &JString) -> Result<String, String> {
    value
        .try_to_string(env)
        .map_err(|e| format!("BUG: failed to read Java string: {e}"))
}

fn jstring_optional(env: &Env, value: &JString) -> Result<Option<String>, String> {
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

fn jstring_from_object<'local>(env: &Env, obj: JObject<'local>) -> Result<JString<'local>, String> {
    env.cast_local::<JString>(obj)
        .map_err(|e| format!("BUG: expected java.lang.String: {e}"))
}

fn string_pairs_from_java(
    env: &mut Env,
    keys: &JObjectArray,
    values: &JObjectArray,
) -> Result<Vec<(String, String)>, String> {
    let len = keys
        .len(env)
        .map_err(|e| format!("BUG: get_array_length keys: {e}"))?;
    let values_len = values
        .len(env)
        .map_err(|e| format!("BUG: get_array_length values: {e}"))?;
    if len != values_len {
        return Err("BUG: data key/value array length mismatch".to_string());
    }
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let key_obj = keys
            .get_element(env, i)
            .map_err(|e| format!("BUG: get key[{i}]: {e}"))?;
        let val_obj = values
            .get_element(env, i)
            .map_err(|e| format!("BUG: get value[{i}]: {e}"))?;
        let key = jstring_required(env, &jstring_from_object(env, key_obj)?)?;
        let value = jstring_required(env, &jstring_from_object(env, val_obj)?)?;
        out.push((key, value));
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
        limits
            .apply(key, n)
            .map_err(|e| lemma::Error::request(e, None::<String>))?;
    }
    Ok(limits)
}

fn parse_effective(env: &mut Env, effective: &JString) -> Result<Option<DateTimeValue>, ()> {
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

fn return_string(env: &mut Env, value: String) -> jstring {
    env.new_string(value)
        .expect("BUG: failed to allocate return string")
        .into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_create(
    mut unowned: EnvUnowned,
    _class: JClass,
) -> jlong {
    with_catch(&mut unowned, |_env| {
        let engine = Box::new(Mutex::new(Engine::new()));
        Ok(Box::into_raw(engine) as jlong)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_createWithLimits(
    mut unowned: EnvUnowned,
    _class: JClass,
    limits_json: JString,
) -> jlong {
    with_catch(&mut unowned, |env| {
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
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
) {
    with_catch(&mut unowned, |_env| {
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
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
    code: JString,
) {
    with_catch(&mut unowned, |env| {
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
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
    labels: JObjectArray,
    codes: JObjectArray,
) {
    with_catch(&mut unowned, |env| {
        let engine = handle_from_jlong(handle)?;
        let pairs = string_pairs_from_java(env, &labels, &codes)?;
        let mut batch = Vec::with_capacity(pairs.len());
        for (label, code) in pairs {
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
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
) -> jstring {
    with_catch(&mut unowned, |env| {
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
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
    repository: JString,
    spec: JString,
    effective: JString,
) -> jstring {
    with_catch(&mut unowned, |env| {
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
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
    repository: JString,
    spec: JString,
    effective: JString,
) -> jstring {
    with_catch(&mut unowned, |env| {
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
    mut unowned: EnvUnowned,
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
    with_catch(&mut unowned, |env| {
        let engine = handle_from_jlong(handle)?;
        let repo = jstring_optional(env, &repository)?;
        let spec = jstring_required(env, &spec)?;
        let effective = match parse_effective(env, &effective) {
            Ok(v) => v,
            Err(()) => return Ok(std::ptr::null_mut()),
        };
        let data: HashMap<String, String> = string_pairs_from_java(env, &data_keys, &data_values)?
            .into_iter()
            .collect();
        let rules = if rules.is_null() {
            None
        } else {
            let len = rules
                .len(env)
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
            let mut names = Vec::with_capacity(len);
            for i in 0..len {
                let obj = rules
                    .get_element(env, i)
                    .map_err(|e| format!("BUG: get rules[{i}]: {e}"))?;
                names.push(jstring_required(env, &jstring_from_object(env, obj)?)?);
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
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
    repository: JString,
    spec: JString,
    effective: JString,
) {
    with_catch(&mut unowned, |env| {
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
pub extern "system" fn Java_com_lemmabase_lemma_Native_update(
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
    repository: JString,
    spec: JString,
    effective: JString,
    code: JString,
    attribute: JString,
) {
    with_catch(&mut unowned, |env| {
        let engine = handle_from_jlong(handle)?;
        let repo = jstring_optional(env, &repository)?;
        let spec = jstring_required(env, &spec)?;
        let effective = match parse_effective(env, &effective) {
            Ok(v) => v,
            Err(()) => return Ok(()),
        };
        let code = jstring_required(env, &code)?;
        let attribute = jstring_optional(env, &attribute)?;
        let source_type = match attribute
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => SourceType::Volatile,
            Some(label) => match SourceType::from_binding_label(label) {
                Ok(st) => st,
                Err(message) => {
                    throw_lemma_exception(
                        env,
                        "update failed",
                        &engine_errors_json(&[lemma::Error::request(
                            format!("update: label '{label}': {message}"),
                            None::<String>,
                        )]),
                    );
                    return Ok(());
                }
            },
        };
        let mut guard = engine
            .lock()
            .map_err(|_| "BUG: Engine lock poisoned".to_string())?;
        if let Err(load_err) = guard.update(
            repo.as_deref(),
            &spec,
            effective.as_ref(),
            source_type,
            code,
        ) {
            throw_lemma_exception(env, "update failed", &engine_errors_json(&load_err.errors));
        }
        Ok(())
    });
}

#[no_mangle]
pub extern "system" fn Java_com_lemmabase_lemma_Native_format(
    mut unowned: EnvUnowned,
    _class: JClass,
    code: JString,
) -> jstring {
    with_catch(&mut unowned, |env| {
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
    mut unowned: EnvUnowned,
    _class: JClass,
    handle: jlong,
) -> jstring {
    with_catch(&mut unowned, |env| {
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
