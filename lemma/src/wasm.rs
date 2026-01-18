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
        let json_bytes = if fact_values_json.trim().is_empty() || fact_values_json.trim() == "{}" {
            b"{}"
        } else {
            fact_values_json.as_bytes()
        };

        match self.engine.evaluate_json(doc_name, Vec::new(), json_bytes) {
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

    #[wasm_bindgen(js_name = invert)]
    pub fn invert(
        &self,
        doc_name: &str,
        rule_name: &str,
        target_json: &str,
        provided_values_json: &str,
    ) -> String {
        return to_json_error_string("Inversion not implemented");
    }
}

fn parse_target(_target_json: &str) -> Result<(), String> {
    Err("Inversion not implemented".to_string())
}

fn json_to_literal_value(value: &serde_json::Value) -> Result<crate::LiteralValue, String> {
    use crate::LiteralValue;
    use rust_decimal::Decimal;

    match value {
        serde_json::Value::Bool(b) => Ok(LiteralValue::boolean((*b).into())),
        serde_json::Value::Number(n) => {
            let decimal = Decimal::from_str_exact(&n.to_string())
                .map_err(|e| format!("Invalid number: {}", e))?;
            Ok(LiteralValue::number(decimal))
        }
        serde_json::Value::String(s) => {
            if s.ends_with('%') {
                let num_str = &s[..s.len() - 1];
                let decimal = Decimal::from_str_exact(num_str)
                    .map_err(|e| format!("Invalid percent: {}", e))?;
                // Convert percent (e.g., 50) to ratio (0.50)
                Ok(LiteralValue::ratio(decimal / Decimal::from(100), None))
            } else {
                Ok(LiteralValue::text(s.clone()))
            }
        }
        _ => Err(format!("Unsupported value type: {:?}", value)),
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

fn format_error(error: &LemmaError) -> String {
    match error {
        LemmaError::Parse(details) => format!("Parse Error: {}", details.message),
        LemmaError::Semantic(details) => format!("Semantic Error: {}", details.message),
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
