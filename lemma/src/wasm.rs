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
        let target = match parse_target(target_json) {
            Ok(t) => t,
            Err(e) => return to_json_error_string(&format!("Invalid target: {}", e)),
        };

        let json_bytes =
            if provided_values_json.trim().is_empty() || provided_values_json.trim() == "{}" {
                b"{}"
            } else {
                provided_values_json.as_bytes()
            };

        match self
            .engine
            .invert_json(doc_name, rule_name, target, json_bytes)
        {
            Ok(inversion_response) => {
                let response_json =
                    serde_json::to_value(&inversion_response).unwrap_or_else(|_| json!({}));
                to_json_response(json!({
                    "success": true,
                    "response": response_json
                }))
            }
            Err(e) => to_json_error(&e),
        }
    }
}

fn parse_target(target_json: &str) -> Result<crate::Target, String> {
    use crate::{OperationResult, Target, TargetOp};
    use serde_json::Value;

    let target: Value = serde_json::from_str(target_json)
        .map_err(|e| format!("Failed to parse target JSON: {}", e))?;

    if target.is_null() || target.as_str() == Some("any") {
        return Ok(Target::any_value());
    }

    if target.as_str() == Some("veto") {
        return Ok(Target::any_veto());
    }

    if let Some(obj) = target.as_object() {
        let op_str = obj
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Target object must have 'op' field".to_string())?;

        let value_json = obj
            .get("value")
            .ok_or_else(|| "Target object must have 'value' field".to_string())?;

        let value = json_to_literal_value(value_json)?;

        let op = match op_str {
            "eq" | "=" => TargetOp::Eq,
            "gt" | ">" => TargetOp::Gt,
            "gte" | ">=" => TargetOp::Gte,
            "lt" | "<" => TargetOp::Lt,
            "lte" | "<=" => TargetOp::Lte,
            _ => return Err(format!("Unknown operator: {}", op_str)),
        };

        return Ok(Target::with_op(op, OperationResult::Value(value)));
    }

    let value = json_to_literal_value(&target)?;
    Ok(Target::value(value))
}

fn json_to_literal_value(value: &serde_json::Value) -> Result<crate::LiteralValue, String> {
    use crate::LiteralValue;
    use rust_decimal::Decimal;

    match value {
        serde_json::Value::Bool(b) => Ok(LiteralValue::Boolean((*b).into())),
        serde_json::Value::Number(n) => {
            let decimal = Decimal::from_str_exact(&n.to_string())
                .map_err(|e| format!("Invalid number: {}", e))?;
            Ok(LiteralValue::Number(decimal))
        }
        serde_json::Value::String(s) => {
            if s.ends_with('%') {
                let num_str = &s[..s.len() - 1];
                let decimal = Decimal::from_str_exact(num_str)
                    .map_err(|e| format!("Invalid percentage: {}", e))?;
                Ok(LiteralValue::Percentage(decimal))
            } else {
                Ok(LiteralValue::Text(s.clone()))
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
        LemmaError::Engine(msg) => format!("Engine Error: {msg}"),
        LemmaError::MissingFact(fact_ref) => format!("Missing Fact: {fact_ref}"),
        LemmaError::CircularDependency(msg) => format!("Circular Dependency: {msg}"),
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
