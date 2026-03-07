//! OpenAPI 3.1 specification generator for Lemma documents.
//!
//! Takes a Lemma `Engine` and produces a complete OpenAPI specification as JSON.
//! Used by both `lemma server` (CLI) and LemmaBase.com for consistent API docs.
//!
//! ## Temporal versioning
//!
//! Lemma documents can have multiple temporal versions (e.g. `doc pricing 2024-01-01`
//! and `doc pricing 2025-01-01`) with potentially different interfaces (facts, rules,
//! types). The OpenAPI spec must reflect the interface active at a specific point in
//! time. Use [`generate_openapi_effective`] with an explicit `DateTimeValue` to get the
//! spec for a given instant. [`generate_openapi`] is a convenience wrapper that uses
//! the current time.
//!
//! For Scalar multi-document rendering, [`temporal_api_sources`] returns the list of
//! temporal version boundaries so the Scalar UI can offer a source selector.

use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, LemmaType, TypeSpecification};
use serde_json::{json, Map, Value};

/// A single Scalar API reference source entry.
///
/// Each temporal version boundary gets its own source so Scalar renders a
/// version switcher in the UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiSource {
    pub title: String,
    pub slug: String,
    pub url: String,
}

/// Compute the list of Scalar multi-source entries for temporal versioning.
///
/// Returns one [`ApiSource`] per distinct temporal version boundary across all
/// loaded documents, plus one "current" source that uses no `effective` (i.e. the
/// latest version). The sources are ordered from oldest to newest, with "current"
/// last.
///
/// If there are no temporal version boundaries (all documents are unversioned),
/// returns a single "current" entry.
pub fn temporal_api_sources(engine: &Engine) -> Vec<ApiSource> {
    let mut all_boundaries: std::collections::BTreeSet<DateTimeValue> =
        std::collections::BTreeSet::new();

    let all_docs = engine.list_documents();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for doc in &all_docs {
        if seen_names.insert(doc.name.clone()) {
            // Collect all version boundaries for this document name.
            // list_documents() returns all temporal versions; we extract effective_from dates.
            for d in all_docs.iter().filter(|d| d.name == doc.name) {
                if let Some(af) = d.effective_from() {
                    all_boundaries.insert(af.clone());
                }
            }
        }
    }

    if all_boundaries.is_empty() {
        return vec![ApiSource {
            title: "Current".to_string(),
            slug: "current".to_string(),
            url: "/openapi.json".to_string(),
        }];
    }

    let mut sources: Vec<ApiSource> = all_boundaries
        .iter()
        .map(|boundary| {
            let label = boundary.to_string();
            ApiSource {
                title: format!("As of {}", label),
                slug: label.clone(),
                url: format!("/openapi.json?effective={}", label),
            }
        })
        .collect();

    sources.push(ApiSource {
        title: "Current".to_string(),
        slug: "current".to_string(),
        url: "/openapi.json".to_string(),
    });

    sources
}

/// Generate a complete OpenAPI 3.1 specification using the current time.
///
/// Convenience wrapper around [`generate_openapi_effective`]. The spec reflects
/// only the documents and interfaces active at `DateTimeValue::now()`.
pub fn generate_openapi(engine: &Engine, proofs_enabled: bool) -> Value {
    generate_openapi_effective(engine, proofs_enabled, &DateTimeValue::now(), false)
}

/// Generate a complete OpenAPI 3.1 specification for a specific point in time.
///
/// The specification includes:
/// - Document endpoints (`/{doc_name}/{rules}` where `rules` is optional)
/// - GET operations with query parameters (including `effective` and `hash_pin`)
/// - POST operations with JSON request bodies (including `effective` and `hash_pin`)
/// - Response schemas with rule result shapes
/// - Meta routes (`/`, `/health`, `/openapi.json`, `/docs`)
///
/// When `proofs_enabled` is true, the spec adds the `x-proofs` header parameter
/// to evaluation endpoints and documents the optional `proof` object in responses.
///
/// The `effective` parameter determines which temporal version of each document is
/// visible. When `use_permalink_paths` is true (e.g. for a specific temporal version),
/// paths use ~hash (e.g. /pricing~abc1234); otherwise bare paths (e.g. /pricing).
pub fn generate_openapi_effective(
    engine: &Engine,
    proofs_enabled: bool,
    effective: &DateTimeValue,
    use_permalink_paths: bool,
) -> Value {
    let mut paths = Map::new();
    let mut components_schemas = Map::new();

    let active_docs = engine.list_documents_effective(effective);
    let unique_doc_names: Vec<String> = active_docs.iter().map(|d| d.name.clone()).collect();

    for doc_name in &unique_doc_names {
        if let Some(plan) = engine.get_execution_plan(doc_name, None, effective) {
            let schema = plan.schema();
            let facts = collect_input_facts_from_schema(&schema);
            let rule_names: Vec<String> = schema.rules.keys().cloned().collect();

            let doc_path = if use_permalink_paths {
                engine
                    .hash_pin(doc_name, effective)
                    .map(|h| format!("{}~{}", doc_name, h))
                    .unwrap_or_else(|| doc_name.clone())
            } else {
                doc_name.clone()
            };

            let safe_name = doc_name.replace('/', "_");
            let response_schema_name = format!("{}_response", safe_name);
            components_schemas.insert(
                response_schema_name.clone(),
                build_response_schema(&schema, &rule_names, proofs_enabled),
            );

            let post_body_schema_name = format!("{}_request", safe_name);
            components_schemas.insert(
                post_body_schema_name.clone(),
                build_post_request_schema(&facts),
            );

            let path = format!("/{}", doc_path);
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
        }
    }

    paths.insert(
        "/".to_string(),
        index_path_item(&unique_doc_names, engine, effective),
    );
    paths.insert("/health".to_string(), health_path_item());
    paths.insert("/openapi.json".to_string(), openapi_json_path_item());

    let mut tags = vec![json!({
        "name": "Documents",
        "description": "Simple API to retrieve the list of Lemma documents"
    })];
    for doc_name in &unique_doc_names {
        let safe_tag = doc_name.replace('/', "_");
        tags.push(json!({
            "name": safe_tag,
            "x-displayName": doc_name,
            "description": format!("GET schema or POST evaluate for document '{}'. Use ?rules= to scope.", doc_name)
        }));
    }
    tags.push(json!({
        "name": "Meta",
        "description": "Server metadata and introspection endpoints"
    }));

    let doc_tags: Vec<Value> = unique_doc_names
        .iter()
        .map(|n| Value::String(n.replace('/', "_")))
        .collect();

    let tag_groups = vec![
        json!({ "name": "Overview", "tags": ["Documents"] }),
        json!({ "name": "Documents", "tags": doc_tags }),
        json!({ "name": "Meta", "tags": ["Meta"] }),
    ];

    let version_label = format!("{} (effective {})", env!("CARGO_PKG_VERSION"), effective);

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Lemma API",
            "description": "Lemma is a declarative language for expressing business logic — pricing rules, tax calculations, eligibility criteria, contracts, and policies. Learn more at [LemmaBase.com](https://lemmabase.com).",
            "version": version_label
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
    /// The fact's literal value if defined in the document (e.g. `fact quantity: 10`).
    /// None for type-only facts (e.g. `fact quantity: [number]`).
    default_value: Option<lemma::LiteralValue>,
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
            default_value: default.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Meta route path items
// ---------------------------------------------------------------------------

fn index_path_item(document_names: &[String], engine: &Engine, effective: &DateTimeValue) -> Value {
    let doc_items: Vec<Value> = document_names
        .iter()
        .map(|name| {
            let (facts_count, rules_count) = engine
                .get_execution_plan(name, None, effective)
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

fn accept_datetime_header_parameter() -> Value {
    json!({
        "name": "Accept-Datetime",
        "in": "header",
        "required": false,
        "description": "RFC 7089 (Memento): resolve the document version active at this datetime. Omit for current. Use path with ~hash for a permalink to a specific version.",
        "schema": { "type": "string", "format": "date-time" },
        "example": "Sat, 01 Jan 2025 00:00:00 GMT"
    })
}

fn build_document_path_item(
    doc_name: &str,
    _facts: &[InputFact],
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

    let tag = doc_name.replace('/', "_");

    let rules_example = if rule_names.is_empty() {
        String::new()
    } else {
        rule_names.join(",")
    };

    let rules_param = json!({
        "name": "rules",
        "in": "query",
        "required": false,
        "description": "Comma-separated list of rule names (GET: scope schema; POST: evaluate only these). Omit for all.",
        "schema": { "type": "string" },
        "example": rules_example
    });

    let mut get_parameters: Vec<Value> = vec![rules_param.clone()];
    get_parameters.push(accept_datetime_header_parameter());
    if proofs_enabled {
        get_parameters.push(x_proofs_header_parameter());
    }

    let get_summary = "Schema of resolved version (doc, facts, rules, meta, versions)".to_string();
    let post_summary = "Evaluate".to_string();
    let get_operation_id = format!("get_{}", doc_name);
    let post_operation_id = format!("post_{}", doc_name);

    let mut post_parameters: Vec<Value> = vec![rules_param];
    post_parameters.push(accept_datetime_header_parameter());
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
                    "description": "Schema of resolved version (doc, effective_from, facts, rules, meta, versions). Headers: ETag, Memento-Datetime, Vary.",
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
                    "application/x-www-form-urlencoded": {
                        "schema": body_ref
                    }
                }
            },
            "responses": {
                "200": {
                    "description": "Evaluation results. Headers: ETag, Memento-Datetime, Vary.",
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
        TypeSpecification::Undetermined => unreachable!(
            "BUG: type_help called with Undetermined sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

/// Default value as a string for form-encoded POST body schema.
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
            default.as_ref().map(|(v, u)| format!("{} {}", v, u))
        }
        TypeSpecification::Veto { .. } => None,
        TypeSpecification::Undetermined => unreachable!(
            "BUG: type_default_as_string called with Undetermined sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

// ---------------------------------------------------------------------------
// POST request body schema generation (form-encoded — all string values)
// ---------------------------------------------------------------------------

fn build_post_request_schema(facts: &[InputFact]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();

    for fact in facts {
        properties.insert(
            fact.name.clone(),
            build_post_property_schema(&fact.lemma_type, fact.default_value.as_ref()),
        );
        if fact.default_value.is_none() {
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

fn build_post_property_schema(
    lemma_type: &LemmaType,
    fact_value: Option<&lemma::LiteralValue>,
) -> Value {
    let mut schema = build_post_type_schema(lemma_type);

    let help = type_help(lemma_type);
    if !help.is_empty() {
        schema["description"] = Value::String(help);
    }

    // Priority: fact's actual value > type's default > nothing
    let default_str = fact_value
        .map(|v| v.display_value())
        .or_else(|| type_default_as_string(lemma_type));
    if let Some(d) = default_str {
        schema["default"] = Value::String(d);
    }

    schema
}

fn build_post_type_schema(lemma_type: &LemmaType) -> Value {
    match &lemma_type.specifications {
        TypeSpecification::Text { options, .. } => {
            let mut schema = json!({ "type": "string" });
            if !options.is_empty() {
                schema["enum"] =
                    Value::Array(options.iter().map(|o| Value::String(o.clone())).collect());
            }
            schema
        }
        TypeSpecification::Boolean { .. } => {
            json!({ "type": "string", "enum": ["true", "false"] })
        }
        _ => json!({ "type": "string" }),
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
        TypeSpecification::Undetermined => unreachable!(
            "BUG: type_base_name called with Undetermined sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lemma::parsing::ast::DateTimeValue;

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

    fn create_engine_with_files(files: Vec<(&str, &str)>) -> Engine {
        let mut engine = Engine::new();
        let file_map: std::collections::HashMap<String, String> = files
            .into_iter()
            .map(|(name, code)| (name.to_string(), code.to_string()))
            .collect();
        tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(engine.add_lemma_files(file_map))
            .expect("failed to parse lemma code");
        engine
    }

    fn date(year: i32, month: u32, day: u32) -> DateTimeValue {
        DateTimeValue {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        }
    }

    fn has_param(params: &Value, name: &str) -> bool {
        params
            .as_array()
            .map(|a| a.iter().any(|p| p["name"] == name))
            .unwrap_or(false)
    }

    fn find_param<'a>(params: &'a Value, name: &str) -> &'a Value {
        params
            .as_array()
            .expect("parameters should be array")
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("parameter '{}' not found", name))
    }

    // =======================================================================
    // Basic spec structure (pre-existing, adapted)
    // =======================================================================

    #[test]
    fn test_generate_openapi_has_required_fields() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
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
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        let tags = spec["tags"].as_array().expect("tags should be array");
        let tag_names: Vec<&str> = tags.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(tag_names, vec!["Documents", "pricing", "Meta"]);
    }

    #[test]
    fn test_generate_openapi_x_tag_groups() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        let groups = spec["x-tagGroups"]
            .as_array()
            .expect("x-tagGroups should be array");
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0]["name"], "Overview");
        assert_eq!(groups[0]["tags"], json!(["Documents"]));
        assert_eq!(groups[1]["name"], "Documents");
        assert_eq!(groups[1]["tags"], json!(["pricing"]));
        assert_eq!(groups[2]["name"], "Meta");
        assert_eq!(groups[2]["tags"], json!(["Meta"]));
    }

    #[test]
    fn test_index_endpoint_uses_documents_tag() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        let index_tag = &spec["paths"]["/"]["get"]["tags"][0];
        assert_eq!(index_tag, "Documents");
    }

    #[test]
    fn test_doc_path_has_get_and_post() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        assert!(
            spec["paths"]["/pricing"].is_object(),
            "single doc path /pricing"
        );
        assert!(spec["paths"]["/pricing"]["get"].is_object());
        assert!(spec["paths"]["/pricing"]["post"].is_object());

        assert_eq!(
            spec["paths"]["/pricing"]["get"]["operationId"],
            "get_pricing"
        );
        assert_eq!(
            spec["paths"]["/pricing"]["post"]["operationId"],
            "post_pricing"
        );
        assert_eq!(spec["paths"]["/pricing"]["get"]["tags"][0], "pricing");

        let get_params = spec["paths"]["/pricing"]["get"]["parameters"]
            .as_array()
            .expect("parameters array");
        let param_names: Vec<&str> = get_params
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert!(
            param_names.contains(&"rules"),
            "GET must have rules query param"
        );
        assert!(
            param_names.contains(&"Accept-Datetime"),
            "GET must have Accept-Datetime header"
        );
    }

    #[test]
    fn test_doc_endpoint_has_accept_datetime_and_rules() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        let get_params = &spec["paths"]["/pricing"]["get"]["parameters"];
        assert!(has_param(get_params, "Accept-Datetime"));
        assert!(has_param(get_params, "rules"));

        let post_params = &spec["paths"]["/pricing"]["post"]["parameters"];
        assert!(has_param(post_params, "Accept-Datetime"));
    }

    #[test]
    fn test_generate_openapi_meta_routes() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        assert!(spec["paths"]["/"].is_object());
        assert!(spec["paths"]["/health"].is_object());
        assert!(spec["paths"]["/openapi.json"].is_object());
        assert!(spec["paths"]["/docs"].is_null());
    }

    #[test]
    fn test_generate_openapi_document_routes() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        assert!(spec["paths"]["/pricing"].is_object());
        assert!(spec["paths"]["/pricing"]["get"].is_object());
        assert!(spec["paths"]["/pricing"]["post"].is_object());
    }

    #[test]
    fn test_generate_openapi_schemas() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        assert!(spec["components"]["schemas"]["pricing_response"].is_object());
        assert!(spec["components"]["schemas"]["pricing_request"].is_object());
    }

    #[test]
    fn test_generate_openapi_proofs_enabled_adds_x_proofs_and_proof_schema() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, true);

        let get_params = &spec["paths"]["/pricing"]["get"]["parameters"];
        assert!(has_param(get_params, "x-proofs"));

        let response_schema = &spec["components"]["schemas"]["pricing_response"];
        let total_props = &response_schema["properties"]["total"]["oneOf"];
        let first_branch = &total_props[0]["properties"];
        assert!(first_branch["proof"].is_object());
    }

    #[test]
    fn test_generate_openapi_multiple_documents() {
        let engine = create_engine_with_files(vec![
            (
                "pricing.lemma",
                "doc pricing\nfact quantity: 10\nrule total: quantity * 2",
            ),
            (
                "shipping.lemma",
                "doc shipping\nfact weight: 5\nrule cost: weight * 3",
            ),
        ]);
        let spec = generate_openapi(&engine, false);

        assert!(spec["paths"]["/pricing"].is_object());
        assert!(spec["paths"]["/shipping"].is_object());
    }

    #[test]
    fn test_nested_doc_path_schema_refs_are_valid() {
        let engine = create_engine_with_code("doc a/b/c\nfact x: [number]\nrule result: x");
        let spec = generate_openapi(&engine, false);

        assert!(spec["paths"]["/a/b/c"]["post"].is_object());
        let body_ref = spec["paths"]["/a/b/c"]["post"]["requestBody"]["content"]
            ["application/x-www-form-urlencoded"]["schema"]["$ref"]
            .as_str()
            .unwrap();
        assert_eq!(body_ref, "#/components/schemas/a_b_c_request");
        assert!(spec["components"]["schemas"]["a_b_c_request"].is_object());
        assert!(spec["components"]["schemas"]["a_b_c_request"]["properties"]["x"].is_object());
    }

    #[test]
    fn test_document_endpoint_has_accept_datetime_header() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let spec = generate_openapi(&engine, false);

        let get_params = &spec["paths"]["/pricing"]["get"]["parameters"];
        assert!(
            has_param(get_params, "Accept-Datetime"),
            "GET must have Accept-Datetime header"
        );
        let accept_dt = find_param(get_params, "Accept-Datetime");
        assert_eq!(accept_dt["in"], "header");
        assert_eq!(accept_dt["required"], false);

        let post_params = &spec["paths"]["/pricing"]["post"]["parameters"];
        assert!(
            has_param(post_params, "Accept-Datetime"),
            "POST must have Accept-Datetime header"
        );
    }

    // =======================================================================
    // generate_openapi_effective with explicit timestamp
    // =======================================================================

    #[test]
    fn test_generate_openapi_effective_reflects_specific_time() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let effective = date(2025, 6, 15);
        let spec = generate_openapi_effective(&engine, false, &effective, true);

        assert_eq!(spec["openapi"], "3.1.0");
        let version = spec["info"]["version"].as_str().unwrap();
        assert!(
            version.contains("2025-06-15"),
            "version string should contain the effective date, got: {}",
            version
        );
    }

    #[test]
    fn test_effective_shows_correct_temporal_version_interface() {
        let engine = create_engine_with_files(vec![(
            "policy.lemma",
            r#"
doc policy
fact base: 100
rule discount: 10

doc policy 2025-06-01
fact base: 200
fact premium: [boolean]
rule discount: 20
rule surcharge: 5
"#,
        )]);

        let before = date(2025, 3, 1);
        let spec_v1 = generate_openapi_effective(&engine, false, &before, true);

        let v1_paths = spec_v1["paths"].as_object().unwrap();
        let policy_path_v1 = v1_paths
            .keys()
            .find(|k| k.starts_with("/policy"))
            .expect("policy path in v1 spec");
        assert!(spec_v1["paths"][policy_path_v1].is_object());
        let v1_response = &spec_v1["components"]["schemas"]["policy_response"];
        assert!(
            v1_response["properties"]["discount"].is_object(),
            "v1 should have discount rule"
        );
        assert!(
            v1_response["properties"]["surcharge"].is_null(),
            "v1 must NOT have surcharge rule"
        );
        let v1_request = &spec_v1["components"]["schemas"]["policy_request"];
        assert!(
            v1_request["properties"]["premium"].is_null(),
            "v1 must NOT have premium fact"
        );

        let after = date(2025, 8, 1);
        let spec_v2 = generate_openapi_effective(&engine, false, &after, true);

        let v2_response = &spec_v2["components"]["schemas"]["policy_response"];
        assert!(
            v2_response["properties"]["discount"].is_object(),
            "v2 should have discount rule"
        );
        assert!(
            v2_response["properties"]["surcharge"].is_object(),
            "v2 should have surcharge rule"
        );
        let v2_request = &spec_v2["components"]["schemas"]["policy_request"];
        assert!(
            v2_request["properties"]["premium"].is_object(),
            "v2 should have premium fact"
        );
    }

    #[test]
    fn test_effective_per_rule_endpoints_match_temporal_version() {
        let engine = create_engine_with_files(vec![(
            "policy.lemma",
            r#"
doc policy
fact base: 100
rule discount: 10

doc policy 2025-06-01
fact base: 200
rule discount: 20
rule surcharge: 5
"#,
        )]);

        let before = date(2025, 3, 1);
        let spec_v1 = generate_openapi_effective(&engine, false, &before, true);
        let v1_response = &spec_v1["components"]["schemas"]["policy_response"];
        assert!(
            v1_response["properties"]["discount"].is_object(),
            "v1 should have discount rule"
        );
        assert!(
            v1_response["properties"]["surcharge"].is_null(),
            "v1 must NOT have surcharge rule"
        );

        let after = date(2025, 8, 1);
        let spec_v2 = generate_openapi_effective(&engine, false, &after, true);
        let v2_response = &spec_v2["components"]["schemas"]["policy_response"];
        assert!(
            v2_response["properties"]["discount"].is_object(),
            "v2 should have discount rule"
        );
        assert!(
            v2_response["properties"]["surcharge"].is_object(),
            "v2 should have surcharge rule"
        );
    }

    #[test]
    fn test_effective_tags_reflect_temporal_version() {
        let engine = create_engine_with_files(vec![(
            "policy.lemma",
            r#"
doc policy
fact base: 100
rule discount: 10

doc policy 2025-06-01
fact base: 200
rule discount: 20
rule surcharge: 5
"#,
        )]);

        let before = date(2025, 3, 1);
        let spec_v1 = generate_openapi_effective(&engine, false, &before, true);
        let v1_tags: Vec<&str> = spec_v1["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(v1_tags.contains(&"policy"));

        let after = date(2025, 8, 1);
        let spec_v2 = generate_openapi_effective(&engine, false, &after, true);
        let v2_tags: Vec<&str> = spec_v2["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(v2_tags.contains(&"policy"));
    }

    // =======================================================================
    // temporal_api_sources
    // =======================================================================

    #[test]
    fn test_temporal_sources_unversioned_returns_single_current() {
        let engine =
            create_engine_with_code("doc pricing\nfact quantity: 10\nrule total: quantity * 2");
        let sources = temporal_api_sources(&engine);

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].title, "Current");
        assert_eq!(sources[0].slug, "current");
        assert_eq!(sources[0].url, "/openapi.json");
    }

    #[test]
    fn test_temporal_sources_versioned_returns_boundaries_plus_current() {
        let engine = create_engine_with_files(vec![(
            "policy.lemma",
            r#"
doc policy
fact base: 100
rule discount: 10

doc policy 2025-06-01
fact base: 200
rule discount: 20
"#,
        )]);

        let sources = temporal_api_sources(&engine);

        assert_eq!(sources.len(), 2, "should have 1 boundary + 1 current");

        assert_eq!(sources[0].title, "As of 2025-06-01");
        assert_eq!(sources[0].slug, "2025-06-01");
        assert_eq!(sources[0].url, "/openapi.json?effective=2025-06-01");

        assert_eq!(sources[1].title, "Current");
        assert_eq!(sources[1].slug, "current");
        assert_eq!(sources[1].url, "/openapi.json");
    }

    #[test]
    fn test_temporal_sources_multiple_documents_merged_boundaries() {
        let engine = create_engine_with_files(vec![
            (
                "policy.lemma",
                r#"
doc policy
fact base: 100
rule discount: 10

doc policy 2025-06-01
fact base: 200
rule discount: 20
"#,
            ),
            (
                "rates.lemma",
                r#"
doc rates
fact rate: 5
rule total: rate * 2

doc rates 2025-03-01
fact rate: 7
rule total: rate * 2

doc rates 2025-06-01
fact rate: 9
rule total: rate * 2
"#,
            ),
        ]);

        let sources = temporal_api_sources(&engine);

        let slugs: Vec<&str> = sources.iter().map(|s| s.slug.as_str()).collect();
        assert!(
            slugs.contains(&"2025-03-01"),
            "should contain rates boundary"
        );
        assert!(
            slugs.contains(&"2025-06-01"),
            "should contain shared boundary"
        );
        assert!(slugs.contains(&"current"), "should contain current");
        assert_eq!(slugs.len(), 3, "2 unique boundaries + current");
    }

    #[test]
    fn test_temporal_sources_ordered_chronologically() {
        let engine = create_engine_with_files(vec![(
            "policy.lemma",
            r#"
doc policy
fact base: 100
rule discount: 10

doc policy 2024-01-01
fact base: 50
rule discount: 5

doc policy 2025-06-01
fact base: 200
rule discount: 20
"#,
        )]);

        let sources = temporal_api_sources(&engine);
        let slugs: Vec<&str> = sources.iter().map(|s| s.slug.as_str()).collect();
        assert_eq!(slugs, vec!["2024-01-01", "2025-06-01", "current"]);
    }

    // =======================================================================
    // Type-specific parameter tests
    // =======================================================================

    #[test]
    fn test_post_schema_text_with_options_has_enum() {
        let engine = create_engine_with_code(
            "doc test\nfact product: [text -> option \"A\" -> option \"B\"]\nrule result: product",
        );
        let spec = generate_openapi(&engine, false);

        let product_prop = &spec["components"]["schemas"]["test_request"]["properties"]["product"];
        assert!(product_prop["enum"].is_array());
        let enums = product_prop["enum"].as_array().unwrap();
        assert_eq!(enums.len(), 2);
        assert_eq!(enums[0], "A");
        assert_eq!(enums[1], "B");
    }

    #[test]
    fn test_post_schema_boolean_is_string_with_enum() {
        let engine =
            create_engine_with_code("doc test\nfact is_active: [boolean]\nrule result: is_active");
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        let is_active = &schema["properties"]["is_active"];
        assert_eq!(is_active["type"], "string");
        assert_eq!(is_active["enum"], json!(["true", "false"]));
    }

    #[test]
    fn test_post_schema_number_is_string() {
        let engine =
            create_engine_with_code("doc test\nfact quantity: [number]\nrule result: quantity");
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        assert_eq!(schema["properties"]["quantity"]["type"], "string");
    }

    #[test]
    fn test_post_schema_date_is_string() {
        let engine =
            create_engine_with_code("doc test\nfact deadline: [date]\nrule result: deadline");
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        assert_eq!(schema["properties"]["deadline"]["type"], "string");
    }

    #[test]
    fn test_fact_with_default_is_not_required() {
        let engine = create_engine_with_code(
            "doc test\nfact quantity: 10\nfact name: [text]\nrule result: quantity",
        );
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        let required = schema["required"]
            .as_array()
            .expect("required should be array");

        assert!(required.contains(&Value::String("name".to_string())));
        assert!(!required.contains(&Value::String("quantity".to_string())));
    }

    #[test]
    fn test_help_and_default_in_openapi() {
        let engine = create_engine_with_code(
            r#"doc test
fact quantity: [number -> help "Number of items to order" -> default 10]
fact active: [boolean -> help "Whether the feature is enabled" -> default true]
rule result: quantity
"#,
        );
        let spec = generate_openapi(&engine, false);

        let req_schema = &spec["components"]["schemas"]["test_request"];
        assert!(req_schema["properties"]["quantity"]["description"]
            .as_str()
            .unwrap()
            .contains("Number of items to order"));
        assert_eq!(
            req_schema["properties"]["quantity"]["default"]
                .as_str()
                .unwrap(),
            "10"
        );
        assert!(req_schema["properties"]["active"]["description"]
            .as_str()
            .unwrap()
            .contains("Whether the feature is enabled"));
        assert_eq!(
            req_schema["properties"]["active"]["default"]
                .as_str()
                .unwrap(),
            "true"
        );
    }
}
