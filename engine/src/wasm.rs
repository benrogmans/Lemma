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

    /// Add Lemma source (e.g. file contents). Returns a Promise that resolves to a JSON string result.
    #[wasm_bindgen(js_name = addLemmaFile)]
    pub fn add_lemma_file(&self, code: &str, source: &str) -> js_sys::Promise {
        let code = code.to_string();
        let source = source.to_string();
        let engine = self.engine.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let files: HashMap<String, String> = std::iter::once((source, code)).collect();
            let result = engine.borrow_mut().add_lemma_files(files).await;
            match result {
                Ok(()) => Ok(JsValue::from_str(&to_json_response(json!({
                    "success": true,
                    "message": "Document added successfully"
                })))),
                Err(errs) => {
                    let error = match errs.len() {
                        0 => unreachable!("add_lemma_files returned Err with empty error list"),
                        1 => errs.into_iter().next().unwrap(),
                        _ => Error::MultipleErrors(errs),
                    };
                    Ok(JsValue::from_str(&to_json_error(&error)))
                }
            }
        })
    }

    /// Evaluate rules in a document.
    ///
    /// Pass `rule_names_json` as `"[]"` or `""` to evaluate all rules.
    /// Pass a JSON array like `'["total","discount"]'` to evaluate specific rules.
    #[wasm_bindgen(js_name = evaluate)]
    pub fn evaluate(
        &self,
        doc_name: &str,
        rule_names_json: &str,
        fact_values_json: &str,
    ) -> String {
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
            .evaluate_json(doc_name, rule_names, json_bytes)
        {
            Ok(response) => {
                let response_json = serde_json::to_value(&response).unwrap_or_else(|_| json!({}));
                to_json_response(json!({
                    "success": true,
                    "response": response_json
                }))
            }
            Err(e) => to_json_error(&e),
        }
    }

    /// List all loaded documents with their full schemas.
    ///
    /// Returns `{ success: true, documents: [DocumentSchema, ...] }` sorted by
    /// document name, consistent with the HTTP and MCP interfaces.
    #[wasm_bindgen(js_name = listDocuments)]
    pub fn list_documents(&self) -> String {
        let engine = self.engine.borrow();
        let mut names = engine.list_documents();
        names.sort();
        let schemas: Vec<serde_json::Value> = names
            .iter()
            .filter_map(|name| engine.get_execution_plan(name))
            .map(|plan| serde_json::to_value(&plan.schema()).unwrap_or(json!({})))
            .collect();
        to_json_response(json!({
            "success": true,
            "documents": schemas
        }))
    }

    /// Return the full document schema: all facts and rules with their types.
    ///
    /// Returns the `DocumentSchema` used by all Lemma interfaces, serialized as
    /// JSON. Use `getSchema` with specific rule names to get only the facts
    /// required by those rules.
    #[wasm_bindgen(js_name = getSchema)]
    pub fn get_schema(&self, doc_name: &str, rule_names_json: &str) -> String {
        let engine = self.engine.borrow();
        let plan = match engine.get_execution_plan(doc_name) {
            Some(p) => p,
            None => return to_json_error_string(&format!("Document '{}' not found", doc_name)),
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
            "schema": serde_json::to_value(&schema).unwrap_or(json!({}))
        }))
    }

    #[wasm_bindgen(js_name = invert)]
    pub fn invert(
        &self,
        _doc_name: &str,
        _rule_name: &str,
        _target_json: &str,
        _provided_values_json: &str,
    ) -> String {
        to_json_error_string("Inversion not implemented")
    }

    /// Format Lemma source code. Returns a JSON string: `{ "success": true, "formatted": "..." }`
    /// or `{ "success": false, "error": "..." }`. Only formats if the source parses successfully.
    /// Call from JS (e.g. Monaco playground) to implement "Format" without an LSP; there is no on-save in the browser.
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

fn to_json_response(data: serde_json::Value) -> String {
    serde_json::to_string(&data).unwrap_or_else(|_| {
        r#"{"success":false,"error":"Failed to serialize response"}"#.to_string()
    })
}

fn to_json_error(error: &Error) -> String {
    to_json_error_string(&format_error(error))
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
        Error::Planning(details) => format!("Planning Error: {}", details.message),
        Error::Registry {
            details,
            identifier,
            kind,
        } => {
            format!(
                "Registry Error ({}): @{}: {}",
                kind, identifier, details.message
            )
        }
        Error::MissingFact(details) => format!("Missing Fact: {}", details.message),
        Error::CircularDependency { details, .. } => {
            format!("Circular Dependency: {}", details.message)
        }
        Error::ResourceLimitExceeded {
            limit_name,
            limit_value,
            actual_value,
            suggestion,
        } => {
            format!(
                "Resource Limit Exceeded: {limit_name} (limit: {limit_value}, actual: {actual_value}). {suggestion}"
            )
        }
        Error::MultipleErrors(errors) => {
            let error_messages: Vec<String> = errors.iter().map(format_error).collect();
            format!("Multiple Errors:\n{}", error_messages.join("\n"))
        }
    }
}
