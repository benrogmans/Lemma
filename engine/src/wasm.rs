use crate::planning::plan;
use crate::{parse, Engine, LemmaError, ResourceLimits};
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

    /// Add Lemma source code. Returns a Promise that resolves to a JSON string result.
    #[wasm_bindgen(js_name = addLemmaCode)]
    pub fn add_lemma_code(&self, code: &str, source: &str) -> js_sys::Promise {
        let code = code.to_string();
        let source = source.to_string();
        let engine = self.engine.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let result = engine.borrow_mut().add_lemma_code(&code, &source).await;
            match result {
                Ok(()) => Ok(JsValue::from_str(&to_json_response(json!({
                    "success": true,
                    "message": "Document added successfully"
                })))),
                Err(e) => Ok(JsValue::from_str(&to_json_error(&e))),
            }
        })
    }

    #[wasm_bindgen(js_name = evaluate)]
    pub fn evaluate(&self, doc_name: &str, fact_values_json: &str) -> String {
        self.evaluate_rules(doc_name, "[]", fact_values_json)
    }

    #[wasm_bindgen(js_name = evaluateRules)]
    pub fn evaluate_rules(
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

    #[wasm_bindgen(js_name = listDocuments)]
    pub fn list_documents(&self) -> String {
        to_json_response(json!({
            "success": true,
            "documents": self.engine.borrow().list_documents()
        }))
    }

    /// Return a UI-friendly schema for a document: facts + resolved types (from execution plan).
    ///
    /// This is intended for frontends to build fact input forms without having to parse Lemma code.
    #[wasm_bindgen(js_name = getDocumentSchema)]
    pub fn get_document_schema(&self, doc_name: &str) -> String {
        self.get_required_facts(doc_name, "[]")
    }

    #[wasm_bindgen(js_name = getRequiredFacts)]
    pub fn get_required_facts(&self, doc_name: &str, rule_names_json: &str) -> String {
        let rule_names: Vec<String> = match parse_rule_names(rule_names_json) {
            Ok(v) => v,
            Err(msg) => return to_json_error_string(&msg),
        };

        let necessary_facts = match self.engine.borrow().get_facts(doc_name, &rule_names) {
            Ok(facts) => facts,
            Err(e) => return to_json_error_string(&e.to_string()),
        };

        let mut fact_entries: Vec<_> = necessary_facts.into_iter().collect();
        fact_entries.sort_by(|a, b| a.0.to_string().cmp(&b.0.to_string()));

        let facts: Vec<_> = fact_entries
            .into_iter()
            .map(|(path, schema_type)| {
                let schema_type_json =
                    serde_json::to_value(&schema_type).unwrap_or(serde_json::Value::Null);
                json!({
                    "name": path.to_string(),
                    "required": true,
                    "valueKind": "type_declaration",
                    "schemaType": schema_type_json,
                    "defaultValue": serde_json::Value::Null
                })
            })
            .collect();

        to_json_response(json!({
            "success": true,
            "doc": {
                "name": doc_name,
                "rules": rule_names,
                "facts": facts
            }
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

    /// Return LSP-style diagnostics for the given Lemma source (parse + plan errors).
    /// Used by the WASM playground to show inline errors in the editor.
    /// Returns a JSON array of { message, severity, startLine, startColumn, endLine, endColumn }
    /// (Monaco uses 1-based line and column).
    #[wasm_bindgen(js_name = getDiagnostics)]
    pub fn get_diagnostics(&self, code: &str, source_attribute: &str) -> String {
        let diagnostics = collect_diagnostics(code, source_attribute);
        to_json_response(serde_json::to_value(&diagnostics).unwrap_or(json!([])))
    }
}

fn to_json_response(data: serde_json::Value) -> String {
    serde_json::to_string(&data).unwrap_or_else(|_| {
        r#"{"success":false,"error":"Failed to serialize response"}"#.to_string()
    })
}

fn to_json_error(error: &LemmaError) -> String {
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

fn format_error(error: &LemmaError) -> String {
    match error {
        LemmaError::Parse(details) => format!("Parse Error: {}", details.message),
        LemmaError::Semantic(details) => format!("Semantic Error: {}", details.message),
        LemmaError::Inversion(details) => format!("Inversion Error: {}", details.message),
        LemmaError::Runtime(details) => format!("Runtime Error: {}", details.message),
        LemmaError::Engine(details) => format!("Engine Error: {}", details.message),
        LemmaError::Registry {
            details,
            identifier,
            kind,
        } => {
            format!(
                "Registry Error ({}): @{}: {}",
                kind, identifier, details.message
            )
        }
        LemmaError::MissingFact(details) => format!("Missing Fact: {}", details.message),
        LemmaError::CircularDependency { details, .. } => {
            format!("Circular Dependency: {}", details.message)
        }
        LemmaError::ResourceLimitExceeded {
            limit_name,
            limit_value,
            actual_value,
            suggestion,
        } => {
            format!(
                "Resource Limit Exceeded: {limit_name} (limit: {limit_value}, actual: {actual_value}). {suggestion}"
            )
        }
        LemmaError::MultipleErrors(errors) => {
            let error_messages: Vec<String> = errors.iter().map(format_error).collect();
            format!("Multiple Errors:\n{}", error_messages.join("\n"))
        }
    }
}

/// Convert byte offset in source text to (line, column) 1-based for Monaco.
fn byte_offset_to_line_col(text: &str, byte_offset: usize) -> (u32, u32) {
    let clamped = byte_offset.min(text.len());
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if i >= clamped {
            break;
        }
        if b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn flatten_errors(error: &LemmaError) -> Vec<&LemmaError> {
    match error {
        LemmaError::MultipleErrors(errors) => errors.iter().flat_map(flatten_errors).collect(),
        other => vec![other],
    }
}

fn collect_diagnostics(code: &str, source_attribute: &str) -> Vec<serde_json::Value> {
    let limits = ResourceLimits::default();
    let mut result = Vec::new();

    let docs = match parse(code, source_attribute, &limits) {
        Ok(d) => d,
        Err(e) => {
            for err in flatten_errors(&e) {
                result.push(lemma_error_to_diagnostic(err, code, source_attribute));
            }
            return result;
        }
    };

    let sources: HashMap<String, String> =
        std::iter::once((source_attribute.to_string(), code.to_string())).collect();

    for doc in &docs {
        if let Err(plan_errors) = plan(doc, &docs, sources.clone()) {
            for err in plan_errors {
                let err_attribute = err
                    .location()
                    .map(|s| s.attribute.as_str())
                    .unwrap_or(source_attribute);
                if err_attribute == source_attribute {
                    result.push(lemma_error_to_diagnostic(&err, code, source_attribute));
                }
            }
        }
    }

    result
}

fn lemma_error_to_diagnostic(
    error: &LemmaError,
    text: &str,
    file_attribute: &str,
) -> serde_json::Value {
    let message = format_error(error);
    let (start_line, start_col, end_line, end_col) = match error {
        LemmaError::ResourceLimitExceeded { .. } => (1u32, 1u32, 1u32, 1u32),
        other => {
            if let Some(source) = other.location() {
                if source.attribute != file_attribute {
                    return json!({
                        "message": message,
                        "severity": "error",
                        "startLine": 1,
                        "startColumn": 1,
                        "endLine": 1,
                        "endColumn": 1
                    });
                }
                let (sl, sc) = byte_offset_to_line_col(text, source.span.start);
                let (el, ec) = byte_offset_to_line_col(text, source.span.end);
                (sl, sc, el, ec)
            } else {
                (1, 1, 1, 1)
            }
        }
    };
    json!({
        "message": message,
        "severity": "error",
        "startLine": start_line,
        "startColumn": start_col,
        "endLine": end_line,
        "endColumn": end_col
    })
}
