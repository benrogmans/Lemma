use crate::{Engine, LemmaError};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmEngine {
    engine: Engine,
}

#[derive(Serialize, Deserialize)]
struct WasmResponse {
    success: bool,
    data: Option<String>,
    error: Option<String>,
    warnings: Option<Vec<String>>,
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
            Ok(_) => serde_json::to_string(&WasmResponse {
                success: true,
                data: Some("Document added successfully".to_string()),
                error: None,
                warnings: None,
            }).unwrap_or_else(|_| r#"{"success":true,"data":"Document added successfully","error":null,"warnings":null}"#.to_string()),
            Err(e) => serde_json::to_string(&WasmResponse {
                success: false,
                data: None,
                error: Some(format_error(&e)),
                warnings: None,
            }).unwrap_or_else(|_| format!(r#"{{"success":false,"data":null,"error":"{}","warnings":null}}"#, format_error(&e).replace('"', "\\\""))),
        }
    }

    #[wasm_bindgen(js_name = evaluate)]
    pub fn evaluate(&mut self, doc_name: &str, fact_values_json: &str) -> String {
        let fact_values: Vec<String> = if fact_values_json.is_empty() {
            Vec::new()
        } else {
            match serde_json::from_str(fact_values_json) {
                Ok(v) => v,
                Err(e) => {
                    return serde_json::to_string(&WasmResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Invalid fact values JSON: {}", e)),
                        warnings: None,
                    }).unwrap_or_else(|_| format!(r#"{{"success":false,"data":null,"error":"Invalid fact values JSON","warnings":null}}"#));
                }
            }
        };

        let fact_refs: Vec<&str> = fact_values.iter().map(|s| s.as_str()).collect();

        match self.engine.evaluate(doc_name, fact_refs) {
            Ok(response) => {
                match serde_json::to_string(&response) {
                    Ok(json) => serde_json::to_string(&WasmResponse {
                        success: true,
                        data: Some(json),
                        error: None,
                        warnings: if response.warnings.is_empty() { None } else { Some(response.warnings.clone()) },
                    }).unwrap_or_else(|_| r#"{"success":false,"data":null,"error":"Failed to serialize response","warnings":null}"#.to_string()),
                    Err(e) => serde_json::to_string(&WasmResponse {
                        success: false,
                        data: None,
                        error: Some(format!("Serialization error: {}", e)),
                        warnings: None,
                    }).unwrap_or_else(|_| r#"{"success":false,"data":null,"error":"Serialization error","warnings":null}"#.to_string()),
                }
            }
            Err(e) => serde_json::to_string(&WasmResponse {
                success: false,
                data: None,
                error: Some(format_error(&e)),
                warnings: None,
            }).unwrap_or_else(|_| format!(r#"{{"success":false,"data":null,"error":"{}","warnings":null}}"#, format_error(&e).replace('"', "\\\""))),
        }
    }

    #[wasm_bindgen(js_name = listDocuments)]
    pub fn list_documents(&self) -> String {
        let docs = self.engine.list_documents();
        let docs_json = serde_json::to_string(&docs).unwrap_or_else(|_| "[]".to_string());
        serde_json::to_string(&WasmResponse {
            success: true,
            data: Some(docs_json),
            error: None,
            warnings: None,
        })
        .unwrap_or_else(|_| {
            r#"{"success":true,"data":"[]","error":null,"warnings":null}"#.to_string()
        })
    }
}

fn format_error(error: &LemmaError) -> String {
    match error {
        LemmaError::Parse(details) => format!("Parse Error: {}", details.message),
        LemmaError::Semantic(details) => format!("Semantic Error: {}", details.message),
        LemmaError::Runtime(details) => format!("Runtime Error: {}", details.message),
        LemmaError::Engine(msg) => format!("Engine Error: {}", msg),
        LemmaError::CircularDependency(msg) => format!("Circular Dependency: {}", msg),
        LemmaError::MultipleErrors(errors) => {
            let error_messages: Vec<String> = errors.iter().map(format_error).collect();
            format!("Multiple Errors:\n{}", error_messages.join("\n"))
        }
        LemmaError::Veto(msg) => match msg {
            Some(m) => format!("Veto: {}", m),
            None => "Veto".to_string(),
        },
    }
}
