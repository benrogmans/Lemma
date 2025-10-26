use crate::{Engine, LemmaError};
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
            Ok(_) => r#"{"success":true,"message":"Document added successfully","error":null}"#
                .to_string(),
            Err(e) => format!(
                r#"{{"success":false,"message":null,"error":"{}"}}"#,
                format_error(&e).replace('"', "\\\"")
            ),
        }
    }

    #[wasm_bindgen(js_name = evaluate)]
    pub fn evaluate(&mut self, doc_name: &str, fact_values_json: &str) -> String {
        // Convert JSON object to Lemma syntax strings using serializers
        let fact_values: Vec<String> = if fact_values_json.is_empty() || fact_values_json == "{}" {
            Vec::new()
        } else {
            // Get the document and all documents for schema-aware conversion
            let doc = match self.engine.get_document(doc_name) {
                Some(d) => d,
                None => {
                    return format!(
                        r#"{{"success":false,"document":null,"rules":null,"warnings":null,"error":"Document '{}' not found"}}"#,
                        doc_name
                    );
                }
            };
            let all_docs = self.engine.get_all_documents();

            // Use JSON serializer to convert to Lemma syntax
            match crate::serializers::from_json(fact_values_json.as_bytes(), doc, all_docs) {
                Ok(lemma_strings) => lemma_strings,
                Err(e) => {
                    return format!(
                        r#"{{"success":false,"document":null,"rules":null,"warnings":null,"error":"{}"}}"#,
                        format_error(&e).replace('"', "\\\"")
                    );
                }
            }
        };

        let fact_refs: Vec<&str> = fact_values.iter().map(|s| s.as_str()).collect();
        let facts = if !fact_refs.is_empty() {
            match crate::parser::parse_facts(&fact_refs) {
                Ok(f) => Some(f),
                Err(e) => return format!("{{\"success\":false,\"error\":\"{}\"}}", e),
            }
        } else {
            None
        };

        match self.engine.evaluate(doc_name, None, facts) {
            Ok(response) => {
                // Transform results array into an object with rule names as keys
                let mut results_map = serde_json::Map::new();
                for result in response.results {
                    let mut rule_obj = serde_json::Map::new();

                    // Transform the result to clean type/value format
                    if let Some(ref lit_val) = result.result {
                        rule_obj.insert(
                            "result".to_string(),
                            serde_json::json!({
                                "type": lit_val.to_type().to_string(),
                                "value": lit_val.display_value()
                            }),
                        );
                    } else {
                        rule_obj.insert("result".to_string(), serde_json::Value::Null);
                    }

                    // Include veto message if present
                    if let Some(veto_msg) = result.veto_message {
                        rule_obj.insert("veto".to_string(), serde_json::Value::String(veto_msg));
                    }

                    // Include missing facts if present
                    if let Some(missing) = result.missing_facts {
                        if !missing.is_empty() {
                            rule_obj.insert(
                                "missing_facts".to_string(),
                                serde_json::to_value(&missing).unwrap_or(serde_json::Value::Null),
                            );
                        }
                    }

                    // Include operations if present
                    if !result.operations.is_empty() {
                        rule_obj.insert(
                            "operations".to_string(),
                            serde_json::json!(result.operations.clone()),
                        );
                    }

                    results_map.insert(result.rule_name, serde_json::Value::Object(rule_obj));
                }

                // Build the flat response with consistent structure
                serde_json::to_string(&serde_json::json!({
                    "success": true,
                    "document": response.doc_name,
                    "rules": results_map,
                    "warnings": if response.warnings.is_empty() { serde_json::Value::Null } else { serde_json::json!(response.warnings) },
                    "error": serde_json::Value::Null
                })).unwrap_or_else(|_| r#"{"success":false,"document":null,"rules":null,"warnings":null,"error":"Failed to serialize response"}"#.to_string())
            }
            Err(e) => format!(
                r#"{{"success":false,"document":null,"rules":null,"warnings":null,"error":"{}"}}"#,
                format_error(&e).replace('"', "\\\"")
            ),
        }
    }

    #[wasm_bindgen(js_name = listDocuments)]
    pub fn list_documents(&self) -> String {
        let docs = self.engine.list_documents();
        match serde_json::to_string(&serde_json::json!({
            "success": true,
            "documents": docs,
            "error": serde_json::Value::Null
        })) {
            Ok(json) => json,
            Err(_) => {
                r#"{"success":false,"documents":null,"error":"Failed to serialize documents"}"#
                    .to_string()
            }
        }
    }
}

fn format_error(error: &LemmaError) -> String {
    match error {
        LemmaError::Parse(details) => format!("Parse Error: {}", details.message),
        LemmaError::Semantic(details) => format!("Semantic Error: {}", details.message),
        LemmaError::Runtime(details) => format!("Runtime Error: {}", details.message),
        LemmaError::Engine(msg) => format!("Engine Error: {}", msg),
        LemmaError::CircularDependency(msg) => format!("Circular Dependency: {}", msg),
        LemmaError::ResourceLimitExceeded {
            limit_name,
            limit_value,
            actual_value,
            suggestion,
        } => {
            format!(
                "Resource Limit Exceeded: {} (limit: {}, actual: {}). {}",
                limit_name, limit_value, actual_value, suggestion
            )
        }
        LemmaError::MultipleErrors(errors) => {
            let error_messages: Vec<String> = errors.iter().map(format_error).collect();
            format!("Multiple Errors:\n{}", error_messages.join("\n"))
        }
    }
}
