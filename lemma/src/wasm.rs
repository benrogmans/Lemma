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

    #[wasm_bindgen(js_name = invert)]
    pub fn invert(
        &self,
        doc_name: &str,
        rule_name: &str,
        target_json: &str,
        given_facts_json: &str,
    ) -> String {
        // Parse target
        let target = match parse_target_from_json(target_json) {
            Ok(t) => t,
            Err(e) => {
                return format!(
                    r#"{{"success":false,"solutions":null,"error":"Invalid target: {}"}}"#,
                    e.replace('"', "\\\"")
                );
            }
        };

        // Parse given facts
        let given_facts = if given_facts_json.is_empty() || given_facts_json == "{}" {
            std::collections::HashMap::new()
        } else {
            match parse_given_facts_from_json(given_facts_json, doc_name, &self.engine) {
                Ok(facts) => facts,
                Err(e) => {
                    return format!(
                        r#"{{"success":false,"solutions":null,"error":"Invalid given facts: {}"}}"#,
                        e.replace('"', "\\\"")
                    );
                }
            }
        };

        // Perform inversion
        match self.engine.invert(doc_name, rule_name, target, given_facts) {
            Ok(solutions) => {
                // Convert solutions to JSON
                let mut solutions_array = Vec::new();
                for solution in solutions {
                    let mut solution_obj = serde_json::Map::new();
                    for (fact_path, domain) in solution {
                        solution_obj.insert(fact_path.to_string(), domain_to_json(&domain));
                    }
                    solutions_array.push(serde_json::Value::Object(solution_obj));
                }

                serde_json::to_string(&serde_json::json!({
                    "success": true,
                    "solutions": solutions_array,
                    "error": serde_json::Value::Null
                }))
                .unwrap_or_else(|_| {
                    r#"{"success":false,"solutions":null,"error":"Failed to serialize response"}"#
                        .to_string()
                })
            }
            Err(e) => format!(
                r#"{{"success":false,"solutions":null,"error":"{}"}}"#,
                format_error(&e).replace('"', "\\\"")
            ),
        }
    }
}

fn parse_target_from_json(target_json: &str) -> Result<crate::Target, String> {
    use crate::{OperationResult, Target, TargetOp};

    let target: serde_json::Value = serde_json::from_str(target_json)
        .map_err(|e| format!("Failed to parse target JSON: {}", e))?;

    if target.is_null() || target == "any" {
        return Ok(Target::any_value());
    }

    if target == "veto" {
        return Ok(Target::any_veto());
    }

    // Check if it's an object with op and value
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

    // Otherwise treat as a specific value
    let value = json_to_literal_value(&target)?;
    Ok(Target::value(value))
}

fn parse_given_facts_from_json(
    json: &str,
    doc_name: &str,
    engine: &Engine,
) -> Result<std::collections::HashMap<String, crate::LiteralValue>, String> {
    let doc = engine
        .get_document(doc_name)
        .ok_or_else(|| format!("Document '{}' not found", doc_name))?;
    let all_docs = engine.get_all_documents();

    let lemma_strings = crate::serializers::from_json(json.as_bytes(), doc, all_docs)
        .map_err(|e| format!("Failed to parse given facts: {}", e))?;

    let fact_refs: Vec<&str> = lemma_strings.iter().map(|s| s.as_str()).collect();
    let parsed_facts = crate::parser::parse_facts(&fact_refs)
        .map_err(|e| format!("Failed to parse facts: {}", e))?;

    let mut fact_map = std::collections::HashMap::new();
    for fact in parsed_facts {
        if let crate::FactValue::Literal(value) = fact.value {
            let fact_name = match &fact.fact_type {
                crate::FactType::Local(name) => format!("{}.{}", doc_name, name),
                crate::FactType::Foreign(foreign) => foreign.reference.join("."),
            };
            fact_map.insert(fact_name, value);
        }
    }

    Ok(fact_map)
}

fn json_to_literal_value(value: &serde_json::Value) -> Result<crate::LiteralValue, String> {
    use crate::{LiteralValue, MoneyUnit, NumericUnit};
    use rust_decimal::Decimal;

    match value {
        serde_json::Value::Bool(b) => Ok(LiteralValue::Boolean(*b)),
        serde_json::Value::Number(n) => {
            let decimal = Decimal::from_str_exact(&n.to_string())
                .map_err(|e| format!("Invalid number: {}", e))?;
            Ok(LiteralValue::Number(decimal))
        }
        serde_json::Value::String(s) => {
            // Try to parse as percentage
            if s.ends_with('%') {
                let num_str = &s[..s.len() - 1];
                let decimal = Decimal::from_str_exact(num_str)
                    .map_err(|e| format!("Invalid percentage: {}", e))?;
                Ok(LiteralValue::Percentage(decimal))
            } else if s.ends_with("EUR") || s.ends_with("USD") {
                // Parse money
                let parts: Vec<&str> = s.split_whitespace().collect();
                if parts.len() != 2 {
                    return Err(format!("Invalid money format: {}", s));
                }
                let amount = Decimal::from_str_exact(parts[0])
                    .map_err(|e| format!("Invalid money amount: {}", e))?;
                let unit = match parts[1] {
                    "EUR" => MoneyUnit::Eur,
                    "USD" => MoneyUnit::Usd,
                    _ => return Err(format!("Unknown currency: {}", parts[1])),
                };
                Ok(LiteralValue::Unit(NumericUnit::Money(amount, unit)))
            } else {
                Ok(LiteralValue::Text(s.clone()))
            }
        }
        _ => Err(format!("Unsupported value type: {:?}", value)),
    }
}

fn domain_to_json(domain: &crate::Domain) -> serde_json::Value {
    use crate::Domain;

    match domain {
        Domain::Range { min, max } => {
            serde_json::json!({
                "type": "range",
                "min": bound_to_json(min),
                "max": bound_to_json(max)
            })
        }
        Domain::Enumeration(values) => {
            let vals: Vec<serde_json::Value> = values
                .iter()
                .map(|v| serde_json::Value::String(v.to_string()))
                .collect();
            serde_json::json!({
                "type": "enumeration",
                "values": vals
            })
        }
        Domain::Union(domains) => {
            let doms: Vec<serde_json::Value> = domains.iter().map(domain_to_json).collect();
            serde_json::json!({
                "type": "union",
                "domains": doms
            })
        }
        Domain::Complement(inner) => {
            serde_json::json!({
                "type": "complement",
                "domain": domain_to_json(inner)
            })
        }
        Domain::Unconstrained => {
            serde_json::json!({
                "type": "unconstrained"
            })
        }
    }
}

fn bound_to_json(bound: &crate::Bound) -> serde_json::Value {
    use crate::Bound;

    match bound {
        Bound::Unbounded => serde_json::json!({"type": "unbounded"}),
        Bound::Inclusive(v) => serde_json::json!({
            "type": "inclusive",
            "value": v.to_string()
        }),
        Bound::Exclusive(v) => serde_json::json!({
            "type": "exclusive",
            "value": v.to_string()
        }),
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
