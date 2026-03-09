use crate::parsing::ast::DateTimeValue;
use crate::{Engine, Error};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
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

    #[wasm_bindgen(js_name = addLemmaFile)]
    pub fn add_lemma_file(&self, code: &str, source: &str) -> js_sys::Promise {
        let code = code.to_string();
        let source = source.to_string();
        let engine = self.engine.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let files: HashMap<String, String> = std::iter::once((source, code)).collect();
            let result = engine.borrow_mut().add_lemma_files(files);
            match result {
                Ok(()) => Ok(JsValue::from_str(&to_json_response(json!({
                    "success": true,
                    "message": "Spec added successfully"
                })))),
                Err(errs) => {
                    let messages: Vec<String> = errs.iter().map(format_error).collect();
                    Ok(JsValue::from_str(&to_json_errors(&messages)))
                }
            }
        })
    }

    /// Evaluate at current time.
    #[wasm_bindgen(js_name = evaluate)]
    pub fn evaluate(
        &self,
        spec_name: &str,
        hash: &str,
        rule_names_json: &str,
        fact_values_json: &str,
    ) -> String {
        self.evaluate_inner(
            spec_name,
            &DateTimeValue::now(),
            hash,
            rule_names_json,
            fact_values_json,
        )
    }

    /// Evaluate at a specific datetime.
    #[wasm_bindgen(js_name = evaluateEffective)]
    pub fn evaluate_effective(
        &self,
        spec_name: &str,
        effective: &str,
        hash: &str,
        rule_names_json: &str,
        fact_values_json: &str,
    ) -> String {
        let effective_dt = match DateTimeValue::parse(effective) {
            Some(dt) => dt,
            None => return to_json_error_string(&format!("Invalid effective: '{}'", effective)),
        };
        self.evaluate_inner(
            spec_name,
            &effective_dt,
            hash,
            rule_names_json,
            fact_values_json,
        )
    }

    #[wasm_bindgen(js_name = listSpecs)]
    pub fn list_specs(&self) -> String {
        let engine = self.engine.borrow();
        let specs = engine.list_specs();
        to_json_response(json!({
            "success": true,
            "specs": specs
        }))
    }

    /// Schema at current time.
    #[wasm_bindgen(js_name = getSchema)]
    pub fn get_schema(&self, spec_name: &str, rule_names_json: &str) -> String {
        self.get_schema_inner(spec_name, &DateTimeValue::now(), rule_names_json)
    }

    /// Schema at a specific datetime.
    #[wasm_bindgen(js_name = getSchemaEffective)]
    pub fn get_schema_effective(
        &self,
        spec_name: &str,
        effective: &str,
        rule_names_json: &str,
    ) -> String {
        let effective_dt = match DateTimeValue::parse(effective) {
            Some(dt) => dt,
            None => return to_json_error_string(&format!("Invalid effective: '{}'", effective)),
        };
        self.get_schema_inner(spec_name, &effective_dt, rule_names_json)
    }

    #[wasm_bindgen(js_name = invert)]
    pub fn invert(
        &self,
        _spec_name: &str,
        _rule_name: &str,
        _target_json: &str,
        _provided_values_json: &str,
    ) -> String {
        to_json_error_string("Inversion not implemented")
    }

    #[wasm_bindgen(js_name = formatSource)]
    pub fn format_source(&self, code: &str, source_attribute: &str) -> String {
        match crate::format_source(code, source_attribute) {
            Ok(formatted) => to_json_response(json!({
                "success": true,
                "formatted": formatted
            })),
            Err(e) => to_json_error(&e),
        }
    }
}

impl WasmEngine {
    fn evaluate_inner(
        &self,
        spec_name: &str,
        effective: &DateTimeValue,
        hash: &str,
        rule_names_json: &str,
        fact_values_json: &str,
    ) -> String {
        let hash_pin = if hash.trim().is_empty() {
            None
        } else {
            Some(hash.trim())
        };

        let rule_names: Vec<String> = match parse_rule_names(rule_names_json) {
            Ok(v) => v,
            Err(msg) => return to_json_error_string(&msg),
        };

        let json_bytes = if fact_values_json.trim().is_empty() || fact_values_json.trim() == "{}" {
            b"{}"
        } else {
            fact_values_json.as_bytes()
        };

        match self
            .engine
            .borrow()
            .evaluate_json(spec_name, hash_pin, effective, rule_names, json_bytes)
        {
            Ok(response) => {
                let response_json = match serde_json::to_value(&response) {
                    Ok(v) => v,
                    Err(e) => {
                        return to_json_error_string(&format!(
                            "BUG: failed to serialize response: {}",
                            e
                        ))
                    }
                };
                to_json_response(json!({
                    "success": true,
                    "response": response_json
                }))
            }
            Err(e) => to_json_error(&e),
        }
    }

    fn get_schema_inner(
        &self,
        spec_name: &str,
        effective: &DateTimeValue,
        rule_names_json: &str,
    ) -> String {
        let engine = self.engine.borrow();
        let plan = match engine.get_execution_plan(spec_name, None, effective) {
            Some(p) => p,
            None => return to_json_error_string(&format!("Spec '{}' not found", spec_name)),
        };

        let rule_names: Vec<String> = match parse_rule_names(rule_names_json) {
            Ok(v) => v,
            Err(msg) => return to_json_error_string(&msg),
        };

        let schema = if rule_names.is_empty() {
            plan.schema()
        } else {
            match plan.schema_for_rules(&rule_names) {
                Ok(s) => s,
                Err(e) => return to_json_error_string(&e.to_string()),
            }
        };

        to_json_response(json!({
            "success": true,
            "schema": match serde_json::to_value(&schema) {
                Ok(v) => v,
                Err(e) => return to_json_error_string(&format!("BUG: failed to serialize schema: {}", e)),
            }
        }))
    }
}

fn to_json_response(data: serde_json::Value) -> String {
    serde_json::to_string(&data).expect(
        "BUG: serde_json::to_string failed on a serde_json::Value — this should never happen",
    )
}

fn to_json_error(error: &Error) -> String {
    to_json_error_string(&format_error(error))
}

fn to_json_errors(messages: &[String]) -> String {
    to_json_response(json!({
        "success": false,
        "errors": messages
    }))
}

fn to_json_error_string(error_msg: &str) -> String {
    to_json_response(json!({
        "success": false,
        "error": error_msg
    }))
}

fn parse_rule_names(rule_names_json: &str) -> Result<Vec<String>, String> {
    let trimmed = rule_names_json.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<String>>(trimmed)
        .map_err(|e| format!("Invalid rule_names JSON (expected array of strings): {}", e))
}

fn format_error(error: &Error) -> String {
    match error {
        Error::Parsing(details) => format!("Parse Error: {}", details.message),
        Error::Inversion(details) => format!("Inversion Error: {}", details.message),
        Error::Validation(details) => format!("Validation Error: {}", details.message),
        Error::Registry {
            details,
            identifier,
            kind,
        } => {
            format!(
                "Registry Error ({}): {}: {}",
                kind, identifier, details.message
            )
        }
        Error::ResourceLimitExceeded {
            details,
            limit_name,
            limit_value,
            actual_value,
        } => {
            let mut msg = format!(
                "Resource Limit Exceeded: {limit_name} (limit: {limit_value}, actual: {actual_value})"
            );
            if let Some(suggestion) = &details.suggestion {
                msg.push_str(&format!(". {suggestion}"));
            }
            msg
        }
        Error::Request(details) => format!("Request Error: {}", details.message),
    }
}
