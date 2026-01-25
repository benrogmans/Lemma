use crate::{Engine, LemmaError};
use serde_json::json;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
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

    #[wasm_bindgen(js_name = addLemmaCode)]
    pub fn add_lemma_code(&mut self, code: &str, source: &str) -> String {
        match self.engine.add_lemma_code(code, source) {
            Ok(_) => to_json_response(json!({
                "success": true,
                "message": "Document added successfully"
            })),
            Err(e) => to_json_error(&e),
        }
    }

    #[wasm_bindgen(js_name = evaluate)]
    pub fn evaluate(&mut self, doc_name: &str, fact_values_json: &str) -> String {
        self.evaluate_rules(doc_name, "[]", fact_values_json)
    }

    #[wasm_bindgen(js_name = evaluateRules)]
    pub fn evaluate_rules(
        &mut self,
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

        match self.engine.evaluate_json(doc_name, rule_names, json_bytes) {
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
            "documents": self.engine.list_documents()
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

        let necessary_facts = match self.engine.get_facts(doc_name, &rule_names) {
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
