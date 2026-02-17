//! OpenAPI 3.1 specification generator for Lemma documents.
//!
//! Takes a Lemma `Engine` and produces a complete OpenAPI specification as JSON.
//! Used by both `lemma server` (CLI) and LemmaBase.com for consistent API docs.

use lemma::{Engine, LemmaType, TypeSpecification};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Generate a complete OpenAPI 3.1 specification from a Lemma engine.
///
/// The specification includes:
/// - Document endpoints (`/{doc_name}/{rules}` where `rules` is optional)
/// - GET operations with query parameters
/// - POST operations with JSON request bodies
/// - Response schemas with rule result shapes
/// - Meta routes (`/`, `/health`, `/openapi.json`, `/docs`)
///
/// When `proofs_enabled` is true, the spec adds the `x-proofs` header parameter
/// to evaluation endpoints and documents the optional `proof` object in responses.
pub fn generate_openapi(engine: &Engine, proofs_enabled: bool) -> Value {
    let mut paths = Map::new();
    let mut components_schemas = Map::new();

    let document_names = {
        let mut names = engine.list_documents();
        names.sort();
        names
    };

    // Document routes (rendered first so they appear above Meta in the sidebar)
    for doc_name in &document_names {
        if let Some(plan) = engine.get_execution_plan(doc_name) {
            let schema = plan.schema();
            let facts = collect_input_facts_from_schema(&schema);
            let rule_names: Vec<String> = schema.rules.keys().cloned().collect();

            // Response schema for this document
            let response_schema_name = format!("{}_response", doc_name);
            components_schemas.insert(
                response_schema_name.clone(),
                build_response_schema(&schema, &rule_names, proofs_enabled),
            );

            // POST body schema
            let post_body_schema_name = format!("{}_request", doc_name);
            components_schemas.insert(
                post_body_schema_name.clone(),
                build_post_request_schema(&facts),
            );

            // /{doc_name}/{rules} path (rules is optional)
            let path = format!("/{}/{{rules}}", doc_name);
            paths.insert(
                path,
                build_document_path_item(
                    doc_name,
                    &facts,
                    &response_schema_name,
                    &post_body_schema_name,
                    &rule_names,
                    proofs_enabled,
                ),
            );

            // Per-rule sub-endpoints: /{doc_name}/{rule_name}
            for rule_name in &rule_names {
                let rule_path = format!("/{}/{}", doc_name, rule_name);
                paths.insert(
                    rule_path,
                    build_rule_path_item(
                        doc_name,
                        rule_name,
                        &facts,
                        &response_schema_name,
                        &post_body_schema_name,
                        proofs_enabled,
                    ),
                );
            }
        }
    }

    // Documents index (top-level, separate from Meta)
    paths.insert("/".to_string(), index_path_item(&document_names, engine));
    // Meta routes
    paths.insert("/health".to_string(), health_path_item());
    paths.insert("/openapi.json".to_string(), openapi_json_path_item());
    // Note: /docs is deliberately excluded from the spec — it serves the
    // documentation UI itself and showing it inside that UI is circular.

    // Tags
    let mut tags = vec![json!({
        "name": "Documents",
        "description": "Simple API to retrieve the list of Lemma documents"
    })];
    for doc_name in &document_names {
        tags.push(json!({
            "name": doc_name,
            "x-displayName": "All rules",
            "description": format!("Evaluate all rules in '{}', or filter specific rules by name", doc_name)
        }));
        if !collect_rule_names_for_doc(engine, doc_name).is_empty() {
            let rules_tag = format!("{} rules", doc_name);
            tags.push(json!({
                "name": rules_tag,
                "x-displayName": "Specific rules",
                "description": format!("Individual rule endpoints for '{}'", doc_name)
            }));
        }
    }
    tags.push(json!({
        "name": "Meta",
        "description": "Server metadata and introspection endpoints"
    }));

    // x-tagGroups for hierarchical sidebar in Scalar.
    let mut tag_groups = Vec::new();
    for doc_name in &document_names {
        let mut doc_tags = vec![json!(doc_name)];
        if !collect_rule_names_for_doc(engine, doc_name).is_empty() {
            doc_tags.push(json!(format!("{} rules", doc_name)));
        }
        tag_groups.push(json!({
            "name": doc_name,
            "tags": doc_tags
        }));
    }
    tag_groups.push(json!({
        "name": "General",
        "tags": ["Documents", "Meta"]
    }));

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Lemma API",
            "description": "Lemma is a declarative language for expressing business logic — pricing rules, tax calculations, eligibility criteria, contracts, and policies. Learn more at [LemmaBase.com](https://lemmabase.com).",
            "version": env!("CARGO_PKG_VERSION")
        },
        "tags": tags,
        "x-tagGroups": tag_groups,
        "paths": Value::Object(paths),
        "components": {
            "schemas": Value::Object(components_schemas)
        }
    })
}

/// Information about a single input fact for OpenAPI generation.
struct InputFact {
    /// The fact name as it appears in the API (e.g. "quantity", "is_member").
    name: String,
    /// The resolved Lemma type for this fact.
    lemma_type: LemmaType,
    /// Whether this fact has a default value (making it optional in the API).
    has_default: bool,
}

/// Collect all local input facts from a pre-built schema.
///
/// Only includes facts local to the document (no dot-separated cross-document
/// paths like `calc.price`). Already sorted alphabetically by `schema()`.
fn collect_input_facts_from_schema(schema: &lemma::DocumentSchema) -> Vec<InputFact> {
    schema
        .facts
        .iter()
        .filter(|(name, _)| !name.contains('.'))
        .map(|(name, (lemma_type, default))| InputFact {
            name: name.clone(),
            lemma_type: lemma_type.clone(),
            has_default: default.is_some(),
        })
        .collect()
}

/// Convenience wrapper: get rule names for a document by name.
fn collect_rule_names_for_doc(engine: &Engine, doc_name: &str) -> Vec<String> {
    engine
        .get_execution_plan(doc_name)
        .map(|plan| plan.schema().rules.into_keys().collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Meta route path items
// ---------------------------------------------------------------------------

fn index_path_item(document_names: &[String], engine: &Engine) -> Value {
    let doc_items: Vec<Value> = document_names
        .iter()
        .map(|name| {
            let (facts_count, rules_count) = engine
                .get_execution_plan(name)
                .map(|p| {
                    let schema = p.schema();
                    let facts_count = schema.facts.keys().filter(|n| !n.contains('.')).count();
                    let rules_count = schema.rules.len();
                    (facts_count, rules_count)
                })
                .unwrap_or((0, 0));
            json!({
                "name": name,
                "facts": facts_count,
                "rules": rules_count
            })
        })
        .collect();

    json!({
        "get": {
            "operationId": "listDocuments",
            "summary": "List all available documents",
            "tags": ["Documents"],
            "responses": {
                "200": {
                    "description": "List of loaded Lemma documents",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "name": { "type": "string" },
                                        "facts": { "type": "integer" },
                                        "rules": { "type": "integer" }
                                    },
                                    "required": ["name", "facts", "rules"]
                                }
                            },
                            "example": doc_items
                        }
                    }
                }
            }
        }
    })
}

fn health_path_item() -> Value {
    json!({
        "get": {
            "operationId": "healthCheck",
            "summary": "Health check",
            "tags": ["Meta"],
            "responses": {
                "200": {
                    "description": "Server is healthy",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "status": { "type": "string" },
                                    "service": { "type": "string" },
                                    "version": { "type": "string" }
                                },
                                "required": ["status", "service", "version"]
                            }
                        }
                    }
                }
            }
        }
    })
}

fn openapi_json_path_item() -> Value {
    json!({
        "get": {
            "operationId": "getOpenApiSpec",
            "summary": "OpenAPI 3.1 specification",
            "tags": ["Meta"],
            "responses": {
                "200": {
                    "description": "OpenAPI specification as JSON",
                    "content": {
                        "application/json": {
                            "schema": { "type": "object" }
                        }
                    }
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Shared response schemas
// ---------------------------------------------------------------------------

fn error_response_schema() -> Value {
    json!({
        "description": "Evaluation error",
        "content": {
            "application/json": {
                "schema": {
                    "type": "object",
                    "properties": {
                        "error": { "type": "string" }
                    },
                    "required": ["error"]
                }
            }
        }
    })
}

fn not_found_response_schema() -> Value {
    json!({
        "description": "Document not found",
        "content": {
            "application/json": {
                "schema": {
                    "type": "object",
                    "properties": {
                        "error": { "type": "string" }
                    },
                    "required": ["error"]
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Document path items
// ---------------------------------------------------------------------------

fn x_proofs_header_parameter() -> Value {
    json!({
        "name": "x-proofs",
        "in": "header",
        "required": false,
        "description": "Set to request proof objects in the response (server must be started with --proofs)",
        "schema": { "type": "string", "default": "true" }
    })
}

fn build_document_path_item(
    doc_name: &str,
    facts: &[InputFact],
    response_schema_name: &str,
    post_body_schema_name: &str,
    rule_names: &[String],
    proofs_enabled: bool,
) -> Value {
    let response_ref = json!({
        "$ref": format!("#/components/schemas/{}", response_schema_name)
    });
    let body_ref = json!({
        "$ref": format!("#/components/schemas/{}", post_body_schema_name)
    });

    let tag = doc_name.to_string();

    let rules_example = if rule_names.is_empty() {
        String::new()
    } else {
        rule_names.join(",")
    };

    let rules_param = json!({
        "name": "rules",
        "in": "path",
        "required": false,
        "description": "Comma-separated list of rule names to evaluate (omit to evaluate all rules)",
        "schema": { "type": "string" },
        "example": rules_example
    });

    // GET query parameters: rules path param first, then fact params
    let mut get_parameters: Vec<Value> = vec![rules_param.clone()];
    get_parameters.extend(facts.iter().map(build_query_parameter));
    if proofs_enabled {
        get_parameters.push(x_proofs_header_parameter());
    }

    let get_summary = "Evaluate".to_string();
    let post_summary = "Evaluate (JSON)".to_string();
    let get_operation_id = format!("get_{}", doc_name);
    let post_operation_id = format!("post_{}", doc_name);

    let mut post_parameters: Vec<Value> = vec![rules_param];
    if proofs_enabled {
        post_parameters.push(x_proofs_header_parameter());
    }

    let path_item = json!({
        "get": {
            "operationId": get_operation_id,
            "summary": get_summary,
            "tags": [tag],
            "parameters": get_parameters,
            "responses": {
                "200": {
                    "description": "Evaluation results",
                    "content": {
                        "application/json": {
                            "schema": response_ref
                        }
                    }
                },
                "400": error_response_schema(),
                "404": not_found_response_schema()
            }
        },
        "post": {
            "operationId": post_operation_id,
            "summary": post_summary,
            "tags": [tag],
            "parameters": post_parameters,
            "requestBody": {
                "required": true,
                "content": {
                    "application/json": {
                        "schema": body_ref
                    }
                }
            },
            "responses": {
                "200": {
                    "description": "Evaluation results",
                    "content": {
                        "application/json": {
                            "schema": response_ref
                        }
                    }
                },
                "400": error_response_schema(),
                "404": not_found_response_schema()
            }
        }
    });

    path_item
}

/// Build a path item for a single rule endpoint: `/{doc_name}/{rule_name}`.
///
/// Uses the same response/request schemas as the parent document but with a
/// fixed rule rather than a path parameter.
fn build_rule_path_item(
    doc_name: &str,
    rule_name: &str,
    facts: &[InputFact],
    response_schema_name: &str,
    post_body_schema_name: &str,
    proofs_enabled: bool,
) -> Value {
    let response_ref = json!({
        "$ref": format!("#/components/schemas/{}", response_schema_name)
    });
    let body_ref = json!({
        "$ref": format!("#/components/schemas/{}", post_body_schema_name)
    });

    let rules_tag = format!("{} rules", doc_name);

    let mut get_parameters: Vec<Value> = facts.iter().map(build_query_parameter).collect();
    if proofs_enabled {
        get_parameters.push(x_proofs_header_parameter());
    }

    let mut post_parameters: Vec<Value> = vec![];
    if proofs_enabled {
        post_parameters.push(x_proofs_header_parameter());
    }

    json!({
        "get": {
            "operationId": format!("get_{}_{}", doc_name, rule_name),
            "summary": format!("{}", rule_name),
            "tags": [rules_tag],
            "parameters": get_parameters,
            "responses": {
                "200": {
                    "description": format!("Result of rule '{}'", rule_name),
                    "content": {
                        "application/json": {
                            "schema": response_ref
                        }
                    }
                },
                "400": error_response_schema(),
                "404": not_found_response_schema()
            }
        },
        "post": {
            "operationId": format!("post_{}_{}", doc_name, rule_name),
            "summary": format!("{} (JSON)", rule_name),
            "tags": [rules_tag],
            "parameters": post_parameters,
            "requestBody": {
                "required": true,
                "content": {
                    "application/json": {
                        "schema": body_ref
                    }
                }
            },
            "responses": {
                "200": {
                    "description": format!("Result of rule '{}'", rule_name),
                    "content": {
                        "application/json": {
                            "schema": response_ref
                        }
                    }
                },
                "400": error_response_schema(),
                "404": not_found_response_schema()
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Help and default from Lemma types
// ---------------------------------------------------------------------------

/// Extract the type's help text for use as description. Always has a value for non-Veto types.
fn type_help(lemma_type: &LemmaType) -> String {
    match &lemma_type.specifications {
        TypeSpecification::Boolean { help, .. } => help.clone(),
        TypeSpecification::Scale { help, .. } => help.clone(),
        TypeSpecification::Number { help, .. } => help.clone(),
        TypeSpecification::Ratio { help, .. } => help.clone(),
        TypeSpecification::Text { help, .. } => help.clone(),
        TypeSpecification::Date { help, .. } => help.clone(),
        TypeSpecification::Time { help, .. } => help.clone(),
        TypeSpecification::Duration { help, .. } => help.clone(),
        TypeSpecification::Veto { .. } => String::new(),
        TypeSpecification::Error => unreachable!(
            "BUG: type_help called with Error sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

/// Default value as string (for GET query params).
fn type_default_as_string(lemma_type: &LemmaType) -> Option<String> {
    match &lemma_type.specifications {
        TypeSpecification::Boolean { default, .. } => default.map(|b| b.to_string()),
        TypeSpecification::Scale { default, .. } => {
            default.as_ref().map(|(d, u)| format!("{} {}", d, u))
        }
        TypeSpecification::Number { default, .. } => default.as_ref().map(|d| d.to_string()),
        TypeSpecification::Ratio { default, .. } => default.as_ref().map(|d| d.to_string()),
        TypeSpecification::Text { default, .. } => default.clone(),
        TypeSpecification::Date { default, .. } => default.as_ref().map(|dt| format!("{}", dt)),
        TypeSpecification::Time { default, .. } => default.as_ref().map(|t| format!("{}", t)),
        TypeSpecification::Duration { default, .. } => {
            default.as_ref().map(|(v, u)| format!("{}+{}", v, u))
        }
        TypeSpecification::Veto { .. } => None,
        TypeSpecification::Error => unreachable!(
            "BUG: type_default_as_string called with Error sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

/// Default value as JSON (for POST body schema).
fn type_default_as_json(lemma_type: &LemmaType) -> Option<Value> {
    match &lemma_type.specifications {
        TypeSpecification::Boolean { default, .. } => default.map(Value::Bool),
        TypeSpecification::Scale { default, .. } => default.as_ref().map(
            |(d, u)| json!({ "value": d.to_string().parse::<f64>().unwrap_or(0.0), "unit": u }),
        ),
        TypeSpecification::Number { default, .. } => default
            .as_ref()
            .and_then(|d| d.to_string().parse::<f64>().ok())
            .map(Value::from),
        TypeSpecification::Ratio { default, .. } => default
            .as_ref()
            .and_then(|d| d.to_string().parse::<f64>().ok())
            .map(|n| json!({ "value": n })),
        TypeSpecification::Text { default, .. } => default.clone().map(Value::String),
        TypeSpecification::Date { default, .. } => {
            default.as_ref().map(|dt| Value::String(format!("{}", dt)))
        }
        TypeSpecification::Time { default, .. } => {
            default.as_ref().map(|t| Value::String(format!("{}", t)))
        }
        TypeSpecification::Duration { default, .. } => default.as_ref().map(|(v, u)| {
            json!({
                "value": v.to_string().parse::<f64>().unwrap_or(0.0),
                "unit": format!("{}", u)
            })
        }),
        TypeSpecification::Veto { .. } => None,
        TypeSpecification::Error => unreachable!(
            "BUG: type_default_as_json called with Error sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

// ---------------------------------------------------------------------------
// Query parameter generation (GET — all string typed)
// ---------------------------------------------------------------------------

fn build_query_parameter(fact: &InputFact) -> Value {
    let type_description = build_get_parameter_description(&fact.lemma_type);
    let help = type_help(&fact.lemma_type);
    let description = if help.is_empty() {
        type_description
    } else {
        format!("{}. {}", help, type_description)
    };
    let default_str = type_default_as_string(&fact.lemma_type);
    let mut schema = build_get_parameter_schema(&fact.lemma_type);
    if let Some(ref d) = default_str {
        schema["default"] = Value::String(d.clone());
    }
    let example = default_str.or_else(|| build_get_example(&fact.lemma_type));
    let mut param = json!({
        "name": fact.name,
        "in": "query",
        "required": !fact.has_default,
        "description": description,
        "schema": schema
    });
    if let Some(ex) = example {
        param["example"] = Value::String(ex);
    }
    param
}

/// Schema for a GET query parameter. Query params are always strings on the wire;
/// we use enum where applicable so UIs (e.g. Scalar) show dropdowns and the right semantic type.
fn build_get_parameter_schema(lemma_type: &LemmaType) -> Value {
    let mut schema = json!({ "type": "string" });
    match &lemma_type.specifications {
        TypeSpecification::Text { options, .. } => {
            if !options.is_empty() {
                schema["enum"] =
                    Value::Array(options.iter().map(|o| Value::String(o.clone())).collect());
            }
        }
        TypeSpecification::Boolean { .. } => {
            schema["enum"] = json!(["true", "false"]);
        }
        _ => {}
    }
    schema
}

fn build_get_parameter_description(lemma_type: &LemmaType) -> String {
    let mut parts = Vec::new();

    let type_name = type_base_name(lemma_type);
    parts.push(format!("Type: {}", type_name));

    match &lemma_type.specifications {
        TypeSpecification::Number {
            minimum, maximum, ..
        } => {
            if let Some(min) = minimum {
                parts.push(format!("Minimum: {}", min));
            }
            if let Some(max) = maximum {
                parts.push(format!("Maximum: {}", max));
            }
        }
        TypeSpecification::Scale {
            minimum,
            maximum,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                parts.push(format!("Units: {}", unit_names.join(", ")));
                parts.push(format!("Format: value+unit (e.g. 100+{})", unit_names[0]));
            }
            if let Some(min) = minimum {
                parts.push(format!("Minimum: {}", min));
            }
            if let Some(max) = maximum {
                parts.push(format!("Maximum: {}", max));
            }
        }
        TypeSpecification::Ratio {
            minimum,
            maximum,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                parts.push(format!("Units: {}", unit_names.join(", ")));
                parts.push("Format: value+unit (e.g. 21+percent)".to_string());
            }
            if let Some(min) = minimum {
                parts.push(format!("Minimum: {}", min));
            }
            if let Some(max) = maximum {
                parts.push(format!("Maximum: {}", max));
            }
        }
        TypeSpecification::Text { options, .. } => {
            if !options.is_empty() {
                parts.push(format!("Options: {}", options.join(", ")));
            }
        }
        TypeSpecification::Boolean { .. } => {
            parts.push("Values: true, false".to_string());
        }
        TypeSpecification::Date { .. } => {
            parts.push("Format: YYYY-MM-DD (e.g. 2024-01-15)".to_string());
        }
        TypeSpecification::Time { .. } => {
            parts.push("Format: HH:MM:SS (e.g. 14:30:00)".to_string());
        }
        TypeSpecification::Duration { .. } => {
            parts.push("Format: value+unit (e.g. 40+hours)".to_string());
            parts.push("Units: years, months, weeks, days, hours, minutes, seconds".to_string());
        }
        TypeSpecification::Veto { .. } => {}
        TypeSpecification::Error => unreachable!(
            "BUG: build_get_parameter_description called with Error sentinel type; this type must never reach OpenAPI generation"
        ),
    }

    parts.join(". ")
}

fn build_get_example(lemma_type: &LemmaType) -> Option<String> {
    match &lemma_type.specifications {
        TypeSpecification::Number { .. } => Some("10".to_string()),
        TypeSpecification::Scale { units, .. } => {
            if let Some(first_unit) = units.iter().next() {
                Some(format!("100+{}", first_unit.name))
            } else {
                Some("100".to_string())
            }
        }
        TypeSpecification::Ratio { units, .. } => {
            if let Some(first_unit) = units.iter().next() {
                Some(format!("21+{}", first_unit.name))
            } else {
                Some("0.21".to_string())
            }
        }
        TypeSpecification::Text { options, .. } => {
            if let Some(first) = options.first() {
                Some(first.clone())
            } else {
                Some("example".to_string())
            }
        }
        TypeSpecification::Boolean { .. } => Some("true".to_string()),
        TypeSpecification::Date { .. } => Some("2024-01-15".to_string()),
        TypeSpecification::Time { .. } => Some("14:30:00".to_string()),
        TypeSpecification::Duration { .. } => Some("40+hours".to_string()),
        TypeSpecification::Veto { .. } => None,
        TypeSpecification::Error => unreachable!(
            "BUG: build_get_example called with Error sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

// ---------------------------------------------------------------------------
// POST request body schema generation (JSON — native types)
// ---------------------------------------------------------------------------

fn build_post_request_schema(facts: &[InputFact]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();

    for fact in facts {
        properties.insert(
            fact.name.clone(),
            build_post_property_schema(&fact.lemma_type),
        );
        if !fact.has_default {
            required.push(Value::String(fact.name.clone()));
        }
    }

    let mut schema = json!({
        "type": "object",
        "properties": Value::Object(properties)
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

fn build_post_property_schema(lemma_type: &LemmaType) -> Value {
    let mut schema = build_post_property_schema_inner(lemma_type);
    let help = type_help(lemma_type);
    if !help.is_empty() {
        schema["description"] = Value::String(help);
    }
    if let Some(default) = type_default_as_json(lemma_type) {
        schema["default"] = default;
    }
    schema
}

fn build_post_property_schema_inner(lemma_type: &LemmaType) -> Value {
    match &lemma_type.specifications {
        TypeSpecification::Number {
            minimum, maximum, ..
        } => {
            let mut schema = json!({ "type": "number" });
            if let Some(min) = minimum {
                schema["minimum"] = json!(min.to_string().parse::<f64>().unwrap_or(0.0));
            }
            if let Some(max) = maximum {
                schema["maximum"] = json!(max.to_string().parse::<f64>().unwrap_or(0.0));
            }
            schema
        }
        TypeSpecification::Scale {
            minimum,
            maximum,
            units,
            ..
        } => {
            let unit_names: Vec<Value> = units
                .iter()
                .map(|u| Value::String(u.name.clone()))
                .collect();

            let mut value_schema = json!({ "type": "number" });
            if let Some(min) = minimum {
                value_schema["minimum"] = json!(min.to_string().parse::<f64>().unwrap_or(0.0));
            }
            if let Some(max) = maximum {
                value_schema["maximum"] = json!(max.to_string().parse::<f64>().unwrap_or(0.0));
            }

            if unit_names.is_empty() {
                value_schema
            } else {
                json!({
                    "type": "object",
                    "properties": {
                        "value": value_schema,
                        "unit": {
                            "type": "string",
                            "enum": unit_names
                        }
                    },
                    "required": ["value", "unit"]
                })
            }
        }
        TypeSpecification::Ratio {
            minimum,
            maximum,
            units,
            ..
        } => {
            let unit_names: Vec<Value> = units
                .iter()
                .map(|u| Value::String(u.name.clone()))
                .collect();

            let mut value_schema = json!({ "type": "number" });
            if let Some(min) = minimum {
                value_schema["minimum"] = json!(min.to_string().parse::<f64>().unwrap_or(0.0));
            }
            if let Some(max) = maximum {
                value_schema["maximum"] = json!(max.to_string().parse::<f64>().unwrap_or(0.0));
            }

            if unit_names.is_empty() {
                value_schema
            } else {
                json!({
                    "type": "object",
                    "properties": {
                        "value": value_schema,
                        "unit": {
                            "type": "string",
                            "enum": unit_names
                        }
                    },
                    "required": ["value"]
                })
            }
        }
        TypeSpecification::Text { options, .. } => {
            let mut schema = json!({ "type": "string" });
            if !options.is_empty() {
                schema["enum"] =
                    Value::Array(options.iter().map(|o| Value::String(o.clone())).collect());
            }
            schema
        }
        TypeSpecification::Boolean { .. } => {
            json!({ "type": "boolean", "example": true })
        }
        TypeSpecification::Date { .. } => {
            json!({ "type": "string", "format": "date" })
        }
        TypeSpecification::Time { .. } => {
            json!({ "type": "string", "format": "time" })
        }
        TypeSpecification::Duration { .. } => {
            json!({
                "type": "object",
                "properties": {
                    "value": { "type": "number" },
                    "unit": {
                        "type": "string",
                        "enum": [
                            "years", "months", "weeks", "days",
                            "hours", "minutes", "seconds"
                        ]
                    }
                },
                "required": ["value", "unit"]
            })
        }
        TypeSpecification::Veto { .. } => {
            json!({ "type": "string" })
        }
        TypeSpecification::Error => unreachable!(
            "BUG: build_post_property_schema_inner called with Error sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

// ---------------------------------------------------------------------------
// Response schema generation
// ---------------------------------------------------------------------------

fn build_response_schema(
    schema: &lemma::DocumentSchema,
    rule_names: &[String],
    proofs_enabled: bool,
) -> Value {
    let mut properties = Map::new();

    let proof_prop = proofs_enabled.then(|| {
        json!({
            "type": "object",
            "description": "Proof tree (included when x-proofs header is sent and server started with --proofs)"
        })
    });

    for rule_name in rule_names {
        if let Some(rule_type) = schema.rules.get(rule_name) {
            let result_type_name = type_base_name(rule_type);
            let mut value_props = Map::new();
            value_props.insert(
                "value".to_string(),
                json!({
                    "type": "string",
                    "description": format!("Computed value (type: {})", result_type_name)
                }),
            );
            if let Some(ref p) = proof_prop {
                value_props.insert("proof".to_string(), p.clone());
            }
            let mut veto_props = Map::new();
            veto_props.insert(
                "veto_reason".to_string(),
                json!({
                    "type": "string",
                    "description": "Reason the rule was vetoed (no value produced)"
                }),
            );
            if let Some(ref p) = proof_prop {
                veto_props.insert("proof".to_string(), p.clone());
            }
            let value_branch = json!({
                "type": "object",
                "properties": Value::Object(value_props),
                "required": ["value"]
            });
            let veto_branch = json!({
                "type": "object",
                "properties": Value::Object(veto_props)
            });
            properties.insert(
                rule_name.clone(),
                json!({
                    "oneOf": [ value_branch, veto_branch ]
                }),
            );
        }
    }

    json!({
        "type": "object",
        "properties": Value::Object(properties)
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get a human-readable base type name for display purposes.
fn type_base_name(lemma_type: &LemmaType) -> String {
    if let Some(ref name) = lemma_type.name {
        return name.clone();
    }
    match &lemma_type.specifications {
        TypeSpecification::Boolean { .. } => "boolean".to_string(),
        TypeSpecification::Number { .. } => "number".to_string(),
        TypeSpecification::Scale { .. } => "scale".to_string(),
        TypeSpecification::Text { .. } => "text".to_string(),
        TypeSpecification::Date { .. } => "date".to_string(),
        TypeSpecification::Time { .. } => "time".to_string(),
        TypeSpecification::Duration { .. } => "duration".to_string(),
        TypeSpecification::Ratio { .. } => "ratio".to_string(),
        TypeSpecification::Veto { .. } => "veto".to_string(),
        TypeSpecification::Error => unreachable!(
            "BUG: type_base_name called with Error sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

/// Convert structured POST JSON input into the flat `HashMap<String, String>` format
/// that the engine expects.
///
/// For example:
/// - `{"quantity": 10}` → `("quantity", "10")`
/// - `{"price": {"value": 100, "unit": "eur"}}` → `("price", "100 eur")`
/// - `{"is_member": true}` → `("is_member", "true")`
/// - `{"deadline": "2024-01-15"}` → `("deadline", "2024-01-15")`
pub fn json_body_to_fact_values(body: &Value) -> HashMap<String, String> {
    let mut result = HashMap::new();

    if let Value::Object(map) = body {
        for (key, value) in map {
            if value.is_null() {
                continue;
            }
            let string_value = structured_value_to_string(value);
            result.insert(key.clone(), string_value);
        }
    }

    result
}

/// Convert a single JSON value (potentially a structured object with value+unit)
/// into the string format the engine expects.
fn structured_value_to_string(value: &Value) -> String {
    match value {
        Value::Object(obj) => {
            // Structured format: {"value": N, "unit": "u"} → "N u"
            if let (Some(val), Some(unit)) = (obj.get("value"), obj.get("unit")) {
                let val_str = match val {
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let unit_str = match unit {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("{} {}", val_str, unit_str)
            } else if let Some(val) = obj.get("value") {
                // Object with only "value" (no unit), e.g. ratio without unit
                match val {
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                }
            } else {
                serde_json::to_string(value).unwrap_or_default()
            }
        }
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        Value::Array(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_engine_with_code(code: &str) -> Engine {
        let mut engine = Engine::new();
        let files: std::collections::HashMap<String, String> =
            std::iter::once(("test.lemma".to_string(), code.to_string())).collect();
        tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(engine.add_lemma_files(files))
            .expect("failed to parse lemma code");
        engine
    }

    #[test]
    fn test_generate_openapi_has_required_fields() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, false);

        assert_eq!(spec["openapi"], "3.1.0");
        assert!(spec["info"]["title"].is_string());
        assert!(spec["tags"].is_array());
        assert!(spec["paths"].is_object());
        assert!(spec["components"]["schemas"].is_object());
    }

    #[test]
    fn test_generate_openapi_tags_order() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, false);

        let tags = spec["tags"].as_array().expect("tags should be array");
        let tag_names: Vec<&str> = tags.iter().map(|t| t["name"].as_str().unwrap()).collect();
        // Documents first, then doc tag, doc rules tag, Meta last
        assert_eq!(
            tag_names,
            vec!["Documents", "pricing", "pricing rules", "Meta"]
        );
    }

    #[test]
    fn test_generate_openapi_x_tag_groups() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, false);

        let groups = spec["x-tagGroups"]
            .as_array()
            .expect("x-tagGroups should be array");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["name"], "pricing");
        assert_eq!(groups[0]["tags"], json!(["pricing", "pricing rules"]));
        assert_eq!(groups[1]["name"], "General");
        assert_eq!(groups[1]["tags"], json!(["Documents", "Meta"]));
    }

    #[test]
    fn test_index_endpoint_uses_documents_tag() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, false);

        let index_tag = &spec["paths"]["/"]["get"]["tags"][0];
        assert_eq!(index_tag, "Documents");
    }

    #[test]
    fn test_per_rule_endpoints() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, false);

        // Per-rule endpoint exists
        assert!(spec["paths"]["/pricing/total"].is_object());
        assert!(spec["paths"]["/pricing/total"]["get"].is_object());
        assert!(spec["paths"]["/pricing/total"]["post"].is_object());

        // Correct operation IDs
        assert_eq!(
            spec["paths"]["/pricing/total"]["get"]["operationId"],
            "get_pricing_total"
        );
        assert_eq!(
            spec["paths"]["/pricing/total"]["post"]["operationId"],
            "post_pricing_total"
        );

        // Tagged under the document's Rules sub-group
        assert_eq!(
            spec["paths"]["/pricing/total"]["get"]["tags"][0],
            "pricing rules"
        );

        // GET has fact parameters but no rules path parameter
        let get_params = spec["paths"]["/pricing/total"]["get"]["parameters"]
            .as_array()
            .expect("parameters array");
        assert!(get_params.iter().all(|p| p["name"] != "rules"));
    }

    #[test]
    fn test_generate_openapi_meta_routes() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, false);

        assert!(spec["paths"]["/"].is_object());
        assert!(spec["paths"]["/health"].is_object());
        assert!(spec["paths"]["/openapi.json"].is_object());
        // /docs is deliberately excluded from the spec (it serves the UI itself)
        assert!(spec["paths"]["/docs"].is_null());
    }

    #[test]
    fn test_generate_openapi_document_routes() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, false);

        assert!(spec["paths"]["/pricing/{rules}"].is_object());

        // GET and POST operations present
        assert!(spec["paths"]["/pricing/{rules}"]["get"].is_object());
        assert!(spec["paths"]["/pricing/{rules}"]["post"].is_object());
    }

    #[test]
    fn test_generate_openapi_schemas() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, false);

        assert!(spec["components"]["schemas"]["pricing_response"].is_object());
        assert!(spec["components"]["schemas"]["pricing_request"].is_object());
    }

    #[test]
    fn test_generate_openapi_proofs_enabled_adds_x_proofs_and_proof_schema() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity = 10\nrule total = quantity * 2");
        let spec = generate_openapi(&engine, true);

        let get_params = &spec["paths"]["/pricing/{rules}"]["get"]["parameters"];
        let has_x_proofs = get_params
            .as_array()
            .map(|a| a.iter().any(|p| p["name"] == "x-proofs"))
            .unwrap_or(false);
        assert!(
            has_x_proofs,
            "GET should have x-proofs header when proofs enabled"
        );

        let response_schema = &spec["components"]["schemas"]["pricing_response"];
        let total_props = &response_schema["properties"]["total"]["oneOf"];
        let first_branch = &total_props[0]["properties"];
        assert!(
            first_branch["proof"].is_object(),
            "response schema should include optional proof when proofs enabled"
        );
    }

    #[test]
    fn test_json_body_to_fact_values_simple_types() {
        let body = json!({
            "quantity": 10,
            "name": "Alice",
            "is_member": true
        });
        let facts = json_body_to_fact_values(&body);

        assert_eq!(facts.get("quantity").unwrap(), "10");
        assert_eq!(facts.get("name").unwrap(), "Alice");
        assert_eq!(facts.get("is_member").unwrap(), "true");
    }

    #[test]
    fn test_json_body_to_fact_values_structured_with_unit() {
        let body = json!({
            "price": { "value": 100, "unit": "eur" }
        });
        let facts = json_body_to_fact_values(&body);

        assert_eq!(facts.get("price").unwrap(), "100 eur");
    }

    #[test]
    fn test_json_body_to_fact_values_structured_without_unit() {
        let body = json!({
            "rate": { "value": 0.21 }
        });
        let facts = json_body_to_fact_values(&body);

        assert_eq!(facts.get("rate").unwrap(), "0.21");
    }

    #[test]
    fn test_json_body_to_fact_values_skips_null() {
        let body = json!({
            "quantity": 10,
            "optional": null
        });
        let facts = json_body_to_fact_values(&body);

        assert_eq!(facts.len(), 1);
        assert!(facts.contains_key("quantity"));
        assert!(!facts.contains_key("optional"));
    }

    #[test]
    fn test_json_body_to_fact_values_duration() {
        let body = json!({
            "workweek": { "value": 40, "unit": "hours" }
        });
        let facts = json_body_to_fact_values(&body);

        assert_eq!(facts.get("workweek").unwrap(), "40 hours");
    }

    #[test]
    fn test_generate_openapi_multiple_documents() {
        let mut engine = Engine::new();
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let mut files = std::collections::HashMap::new();
        files.insert(
            "pricing.lemma".to_string(),
            "doc pricing\nfact quantity = 10\nrule total = quantity * 2".to_string(),
        );
        files.insert(
            "shipping.lemma".to_string(),
            "doc shipping\nfact weight = 5\nrule cost = weight * 3".to_string(),
        );
        runtime
            .block_on(engine.add_lemma_files(files))
            .expect("failed to parse");

        let spec = generate_openapi(&engine, false);

        assert!(spec["paths"]["/pricing/{rules}"].is_object());
        assert!(spec["paths"]["/shipping/{rules}"].is_object());
    }

    #[test]
    fn test_query_parameter_for_text_with_options() {
        let engine = create_engine_with_code(
            "doc test\nfact product = [text -> option \"A\" -> option \"B\"]\nrule result = product",
        );
        let spec = generate_openapi(&engine, false);

        let params = &spec["paths"]["/test/{rules}"]["get"]["parameters"];
        let product_param = params
            .as_array()
            .expect("parameters should be array")
            .iter()
            .find(|p| p["name"] == "product")
            .expect("should have product parameter");

        assert!(product_param["schema"]["enum"].is_array());
        let enums = product_param["schema"]["enum"].as_array().unwrap();
        assert_eq!(enums.len(), 2);
        assert_eq!(enums[0], "A");
        assert_eq!(enums[1], "B");
    }

    #[test]
    fn test_post_schema_boolean_uses_native_type() {
        let engine = create_engine_with_code(
            "doc test\nfact is_active = [boolean]\nrule result = is_active",
        );
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        assert_eq!(schema["properties"]["is_active"]["type"], "boolean");
    }

    #[test]
    fn test_get_parameter_boolean_has_enum() {
        let engine = create_engine_with_code(
            "doc test\nfact is_active = [boolean]\nrule result = is_active",
        );
        let spec = generate_openapi(&engine, false);

        let params = &spec["paths"]["/test/{rules}"]["get"]["parameters"];
        let is_active_param = params
            .as_array()
            .expect("parameters should be array")
            .iter()
            .find(|p| p["name"] == "is_active")
            .expect("should have is_active parameter");
        assert_eq!(is_active_param["schema"]["type"], "string");
        assert_eq!(is_active_param["schema"]["enum"], json!(["true", "false"]));
    }

    #[test]
    fn test_post_schema_number_uses_native_type() {
        let engine =
            create_engine_with_code("doc test\nfact quantity = [number]\nrule result = quantity");
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        assert_eq!(schema["properties"]["quantity"]["type"], "number");
    }

    #[test]
    fn test_post_schema_date_uses_string_format() {
        let engine =
            create_engine_with_code("doc test\nfact deadline = [date]\nrule result = deadline");
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        assert_eq!(schema["properties"]["deadline"]["type"], "string");
        assert_eq!(schema["properties"]["deadline"]["format"], "date");
    }

    #[test]
    fn test_fact_with_default_is_not_required() {
        let engine = create_engine_with_code(
            "doc test\nfact quantity = 10\nfact name = [text]\nrule result = quantity",
        );
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        let required = schema["required"]
            .as_array()
            .expect("required should be array");

        // "name" should be required (type-only, no default)
        assert!(required.contains(&Value::String("name".to_string())));
        // "quantity" should NOT be required (has default value of 10)
        assert!(!required.contains(&Value::String("quantity".to_string())));
    }

    #[test]
    fn test_help_and_default_in_openapi() {
        let engine = create_engine_with_code(
            r#"doc test
fact quantity = [number -> help "Number of items to order" -> default 10]
fact active = [boolean -> help "Whether the feature is enabled" -> default true]
rule result = quantity
"#,
        );
        let spec = generate_openapi(&engine, false);

        let get_params = spec["paths"]["/test/{rules}"]["get"]["parameters"]
            .as_array()
            .expect("parameters array");
        let quantity_param = get_params
            .iter()
            .find(|p| p["name"] == "quantity")
            .expect("quantity param");
        assert!(quantity_param["description"]
            .as_str()
            .unwrap()
            .contains("Number of items to order"));
        assert_eq!(quantity_param["schema"]["default"], "10");
        assert_eq!(quantity_param["example"], "10");

        let active_param = get_params
            .iter()
            .find(|p| p["name"] == "active")
            .expect("active param");
        assert!(active_param["description"]
            .as_str()
            .unwrap()
            .contains("Whether the feature is enabled"));
        assert_eq!(active_param["schema"]["default"], "true");

        let req_schema = &spec["components"]["schemas"]["test_request"];
        assert_eq!(
            req_schema["properties"]["quantity"]["description"]
                .as_str()
                .unwrap(),
            "Number of items to order"
        );
        assert_eq!(
            req_schema["properties"]["quantity"]["default"]
                .as_f64()
                .unwrap(),
            10.0
        );
        assert_eq!(
            req_schema["properties"]["active"]["description"]
                .as_str()
                .unwrap(),
            "Whether the feature is enabled"
        );
        assert!(req_schema["properties"]["active"]["default"]
            .as_bool()
            .unwrap());
    }
}
