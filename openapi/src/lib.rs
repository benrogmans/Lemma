//! OpenAPI 3.1 specification generator for Lemma specs.
//!
//! Takes a Lemma `Engine` and produces a complete OpenAPI specification as JSON.
//! Used by both `lemma server` (CLI) and LemmaBase.com for consistent API docs.
//!
//! ## Temporal versioning
//!
//! Lemma specs can have multiple temporal versions (e.g. `spec pricing 2024-01-01`
//! and `spec pricing 2025-01-01`) with potentially different interfaces (data, rules,
//! types). The OpenAPI spec must reflect the interface active at a specific point in
//! time. Use [`generate_openapi_effective`] with an explicit `DateTimeValue` to get the
//! spec for a given instant. [`generate_openapi`] is a convenience wrapper that uses
//! the current time.
//!
//! For Scalar multi-spec rendering, [`temporal_api_sources`] returns the list of
//! temporal version boundaries so the Scalar UI can offer a source selector.

use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, LemmaType, TypeSpecification};
use serde_json::{json, Map, Value};

/// Query slug for the default temporal view (request-time instant). OpenAPI URLs use no `?effective=`.
pub const NOW_SLUG: &str = "now";

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
/// loaded specs, plus one **now** source (slug [`NOW_SLUG`]) that uses no `effective`
/// query (evaluation instant = request time). That entry is first (Scalar default),
/// then boundaries in descending chronological order (newest first).
///
/// If there are no temporal version boundaries (all specs are unversioned),
/// returns a single **now** entry.
pub fn temporal_api_sources(engine: &Engine) -> Vec<ApiSource> {
    let mut all_boundaries: std::collections::BTreeSet<DateTimeValue> =
        std::collections::BTreeSet::new();

    let all_specs = engine.list_specs();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for spec in &all_specs {
        if seen_names.insert(spec.name.clone()) {
            for s in all_specs.iter().filter(|s| s.name == spec.name) {
                if let Some(af) = s.effective_from() {
                    all_boundaries.insert(af.clone());
                }
            }
        }
    }

    if all_boundaries.is_empty() {
        return vec![ApiSource {
            title: "Now".to_string(),
            slug: NOW_SLUG.to_string(),
            url: "/openapi.json".to_string(),
        }];
    }

    let mut sources: Vec<ApiSource> = Vec::with_capacity(all_boundaries.len() + 1);

    sources.push(ApiSource {
        title: "Now".to_string(),
        slug: NOW_SLUG.to_string(),
        url: "/openapi.json".to_string(),
    });

    for boundary in all_boundaries.iter().rev() {
        let label = boundary.to_string();
        sources.push(ApiSource {
            title: format!("Effective {}", label),
            slug: label.clone(),
            url: format!("/openapi.json?effective={}", label),
        });
    }

    sources
}

/// Generate a complete OpenAPI 3.1 specification using the current time.
///
/// Convenience wrapper around [`generate_openapi_effective`]. The spec reflects
/// only the specs and interfaces active at `DateTimeValue::now()`.
pub fn generate_openapi(engine: &Engine, explanations_enabled: bool) -> Value {
    generate_openapi_effective(engine, explanations_enabled, &DateTimeValue::now())
}

/// Generate a complete OpenAPI 3.1 specification for a specific point in time.
///
/// The specification includes:
/// - `GET /` — list loaded specs (name, data/rule counts)
/// - Spec endpoints (`/{spec_name}`) with `?rules=` query parameter
/// - GET (schema: `spec_set_id`, `effective_from`, `data`, `rules`, `meta`, `versions`) and
///   POST (evaluate: envelope `spec`, `effective`, `result`) with `Accept-Datetime` header
/// - `x-effective-from` / `x-effective-to` vendor extensions on each spec PathItem
///   exposing the half-open `[effective_from, effective_to)` range of the version
///   resolved at the document's effective instant (both `null` when unbounded)
///
/// CLI `lemma server` also exposes shell routes (`/openapi.json`, `/health`, `/docs`) and
/// legacy schema routes (`/schema/{spec_name}`, `/schema/{spec_name}/{rules}`); both are
/// intentionally omitted from the generated document. The legacy `/schema/*` routes
/// predate the spec envelope returned by `GET /{spec_name}` and are kept for backward
/// compatibility only; use `GET /{spec_name}` instead. `GET /` (list loaded specs) is
/// included alongside `GET|POST /{spec_name}`.
///
/// When `explanations_enabled` is true, the spec adds the `x-explanations` header parameter
/// to evaluation endpoints and describes the optional `explanation` field on rule results.
pub fn generate_openapi_effective(
    engine: &Engine,
    explanations_enabled: bool,
    effective: &DateTimeValue,
) -> Value {
    let mut paths = Map::new();
    let mut components_schemas = Map::new();

    components_schemas.insert(
        "LemmaRuleResult".to_string(),
        build_rule_result_schema(explanations_enabled),
    );

    let active_specs = engine.list_specs_effective(effective);
    let unique_spec_names: Vec<String> = active_specs.iter().map(|s| s.name.clone()).collect();

    paths.insert(
        "/".to_string(),
        index_path_item(&unique_spec_names, engine, effective),
    );

    for spec_name in &unique_spec_names {
        if let Ok(plan) = engine.get_plan(spec_name, Some(effective)) {
            let schema = plan.schema();
            let data = collect_input_data_from_schema(&schema);
            let rule_names: Vec<String> = schema.rules.keys().cloned().collect();

            let spec_set = engine
                .get_spec_set(spec_name)
                .expect("BUG: spec in list_specs_effective but spec set missing from engine");
            let active_spec = active_specs
                .iter()
                .find(|s| s.name == *spec_name)
                .expect("BUG: active_specs was produced by this engine for this name");
            let (spec_effective_from, spec_effective_to) = spec_set.effective_range(active_spec);

            let safe_name = spec_name.replace('/', "_");
            let get_response_schema_name = format!("{}_get_response", safe_name);
            components_schemas.insert(
                get_response_schema_name.clone(),
                build_get_schema_response(),
            );

            let evaluate_response_schema_name = format!("{}_evaluate_response", safe_name);
            components_schemas.insert(
                evaluate_response_schema_name.clone(),
                build_evaluate_response_schema(&schema, &rule_names),
            );

            let post_body_schema_name = format!("{}_request", safe_name);
            components_schemas.insert(
                post_body_schema_name.clone(),
                build_post_request_schema(&data),
            );

            let path = format!("/{}", spec_name);
            paths.insert(
                path,
                build_spec_path_item(
                    spec_name,
                    &get_response_schema_name,
                    &evaluate_response_schema_name,
                    &post_body_schema_name,
                    &rule_names,
                    explanations_enabled,
                    (spec_effective_from.as_ref(), spec_effective_to.as_ref()),
                ),
            );
        }
    }

    let mut tags = vec![json!({
        "name": "Specs",
        "description": "Simple API to retrieve the list of Lemma specs"
    })];
    for spec_name in &unique_spec_names {
        let safe_tag = spec_name.replace('/', "_");
        tags.push(json!({
            "name": safe_tag,
            "x-displayName": spec_name,
            "description": format!("GET schema or POST evaluate for spec '{}'. Use ?rules= to scope.", spec_name)
        }));
    }

    let spec_tags: Vec<Value> = unique_spec_names
        .iter()
        .map(|n| Value::String(n.replace('/', "_")))
        .collect();

    let tag_groups = vec![
        json!({ "name": "Overview", "tags": ["Specs"] }),
        json!({ "name": "Specs", "tags": spec_tags }),
    ];

    let version_label = format!("{} (effective {})", env!("CARGO_PKG_VERSION"), effective);

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Lemma API",
            "description": "Lemma is a declarative language for expressing business logic — pricing rules, tax calculations, eligibility criteria, contracts, and policies. Learn more at [LemmaBase.com](https://lemmabase.com).\n\n**Temporal resolution.** `GET /{spec}` describes **version boundaries**: each entry in `versions` carries the half-open `[effective_from, effective_to)` validity range of a temporal version. `POST /{spec}` treats the request's effective instant (from the `Accept-Datetime` header, or the evaluation envelope's `effective` field) as the **evaluation instant** used to pick the active version and compute the result.",
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

/// Information about a single input data for OpenAPI generation.
struct InputData {
    /// The data name as it appears in the API (e.g. "quantity", "is_member").
    name: String,
    /// The resolved Lemma type for this data.
    lemma_type: LemmaType,
    /// The data's literal value if defined in the spec (e.g. `data quantity: 10`).
    /// None for type-only data (e.g. `data quantity: number`).
    default_value: Option<lemma::LiteralValue>,
}

/// Collect all local input data from a pre-built schema.
///
/// Only includes data local to the spec (no dot-separated cross-spec
/// paths like `calc.price`). Already sorted alphabetically by `schema()`.
fn collect_input_data_from_schema(schema: &lemma::SpecSchema) -> Vec<InputData> {
    schema
        .data
        .iter()
        .filter(|(name, _)| !name.contains('.'))
        .map(|(name, entry)| InputData {
            name: name.clone(),
            lemma_type: entry.lemma_type.clone(),
            default_value: entry.default.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Index path (list specs)
// ---------------------------------------------------------------------------

fn index_path_item(spec_names: &[String], engine: &Engine, effective: &DateTimeValue) -> Value {
    let spec_items: Vec<Value> = spec_names
        .iter()
        .map(|name| match engine.schema(name, Some(effective)) {
            Ok(s) => {
                let data_count = s.data.keys().filter(|n| !n.contains('.')).count();
                let rules_count = s.rules.len();
                json!({
                    "name": name,
                    "data": data_count,
                    "rules": rules_count
                })
            }
            Err(e) => json!({
                "name": name,
                "schema_error": true,
                "message": e.to_string()
            }),
        })
        .collect();

    json!({
        "get": {
            "operationId": "list",
            "summary": "List all available specs",
            "tags": ["Specs"],
            "responses": {
                "200": {
                    "description": "List of loaded Lemma specs",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "name": { "type": "string" },
                                        "data": { "type": "integer" },
                                        "rules": { "type": "integer" },
                                        "schema_error": { "type": "boolean" },
                                        "message": { "type": "string" }
                                    },
                                    "required": ["name"]
                                }
                            },
                            "example": spec_items
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
        "description": "Spec not found",
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

fn memento_spec_response_headers() -> Value {
    json!({
        "Memento-Datetime": {
            "description": "RFC 7089: datetime of the resolved spec version (absent for unversioned specs)",
            "schema": { "type": "string" }
        },
        "Vary": {
            "description": "Indicates negotiation on Accept-Datetime",
            "schema": { "type": "string", "example": "Accept-Datetime" }
        }
    })
}

/// GET `/{spec}` body: matches [cli::server::GetSpecResponse].
fn build_get_schema_response() -> Value {
    json!({
        "type": "object",
        "required": ["spec_set_id", "data", "rules", "meta", "versions"],
        "properties": {
            "spec_set_id": {
                "type": "string",
                "description": "Spec set identifier (path segments, e.g. org/product/pricing)"
            },
            "effective_from": {
                "type": ["string", "null"],
                "description": "Effective-from of the resolved temporal version, if any"
            },
            "data": {
                "type": "object",
                "description": "Input data names mapped to type metadata and optional defaults",
                "additionalProperties": true
            },
            "rules": {
                "type": "object",
                "description": "Rule names mapped to result types (scoped by ?rules= when provided)",
                "additionalProperties": true
            },
            "meta": {
                "type": "object",
                "description": "Spec metadata key/value pairs",
                "additionalProperties": true
            },
            "versions": {
                "type": "array",
                "description": "All loaded temporal versions for this spec name, each with a half-open [effective_from, effective_to) range",
                "items": {
                    "type": "object",
                    "required": ["effective_from", "effective_to"],
                    "properties": {
                        "effective_from": {
                            "type": ["string", "null"],
                            "description": "Start of validity for this version; null when unbounded (no earlier version exists)"
                        },
                        "effective_to": {
                            "type": ["string", "null"],
                            "description": "Exclusive end of validity (same instant as the next version's effective_from); null when this is the latest version and has no successor"
                        }
                    }
                }
            }
        }
    })
}

/// Single rule output: matches [cli::response::RuleResultJson].
fn build_rule_result_schema(explanations_enabled: bool) -> Value {
    let mut explanation = json!({
        "type": "object",
        "description": "Structured explanation tree when explanations are enabled"
    });
    if explanations_enabled {
        explanation["description"] = Value::String(
            "Structured explanation tree (present when x-explanations is sent and server uses --explanations)"
                .to_string(),
        );
    }

    json!({
        "type": "object",
        "required": ["vetoed", "rule_type"],
        "properties": {
            "value": {
                "description": "Native JSON value when not vetoed (boolean, number, string, array, object)"
            },
            "unit": {
                "type": "string",
                "description": "Unit for scale/duration results (e.g. currency code, hours)"
            },
            "display": {
                "type": "string",
                "description": "Human-readable formatted value"
            },
            "vetoed": { "type": "boolean" },
            "veto_reason": { "type": "string" },
            "rule_type": {
                "type": "string",
                "description": "Result type name (e.g. number, boolean, money)"
            },
            "explanation": explanation
        }
    })
}

/// POST evaluate body: matches [cli::response::EvaluationEnvelope].
fn build_evaluate_response_schema(schema: &lemma::SpecSchema, rule_names: &[String]) -> Value {
    let mut result_props = Map::new();
    for rule_name in rule_names {
        if schema.rules.contains_key(rule_name) {
            result_props.insert(
                rule_name.clone(),
                json!({
                    "$ref": "#/components/schemas/LemmaRuleResult"
                }),
            );
        }
    }

    json!({
        "type": "object",
        "required": ["spec", "effective", "result"],
        "properties": {
            "spec": {
                "type": "string",
                "description": "Spec set id that was evaluated"
            },
            "effective": {
                "type": "string",
                "description": "Evaluation instant used for temporal resolution (matches request instant unless overridden)"
            },
            "result": {
                "type": "object",
                "description": "Rule names to evaluation results (definition order in response; keys match ?rules= filter when set)",
                "properties": Value::Object(result_props)
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Spec path items
// ---------------------------------------------------------------------------

fn x_explanations_header_parameter() -> Value {
    json!({
        "name": "x-explanations",
        "in": "header",
        "required": false,
        "description": "Set to request explanation objects in the response (server must be started with --explanations)",
        "schema": { "type": "string", "default": "true" }
    })
}

fn accept_datetime_header_parameter() -> Value {
    json!({
        "name": "Accept-Datetime",
        "in": "header",
        "required": false,
        "description": "RFC 7089 (Memento): resolve the spec version active at this datetime. Omit to evaluate at the request instant (now).",
        "schema": { "type": "string", "format": "date-time" },
        "example": "Sat, 01 Jan 2025 00:00:00 GMT"
    })
}

/// Build the PathItem for `/{spec_name}` (GET schema + POST evaluate).
///
/// `effective_range` is the half-open `[effective_from, effective_to)`
/// validity range of the temporal version resolved at the OpenAPI document's
/// effective instant. Both bounds are emitted as the `x-effective-from` /
/// `x-effective-to` vendor extensions on the PathItem so tooling can render
/// the active version's window without having to inspect the `versions`
/// array. `None` in either position (unbounded start for the first row,
/// unbounded end for the latest row) is serialised as JSON `null`.
fn build_spec_path_item(
    spec_name: &str,
    get_response_schema_name: &str,
    evaluate_response_schema_name: &str,
    post_body_schema_name: &str,
    rule_names: &[String],
    explanations_enabled: bool,
    effective_range: (Option<&DateTimeValue>, Option<&DateTimeValue>),
) -> Value {
    let (effective_from, effective_to) = effective_range;

    let get_schema_ref = json!({
        "$ref": format!("#/components/schemas/{}", get_response_schema_name)
    });
    let evaluate_schema_ref = json!({
        "$ref": format!("#/components/schemas/{}", evaluate_response_schema_name)
    });
    let body_ref = json!({
        "$ref": format!("#/components/schemas/{}", post_body_schema_name)
    });

    let tag = spec_name.replace('/', "_");

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
    if explanations_enabled {
        get_parameters.push(x_explanations_header_parameter());
    }

    let get_summary = "Schema of resolved version (spec, data, rules, meta, versions)".to_string();
    let post_summary = "Evaluate".to_string();
    let get_operation_id = format!("get_{}", spec_name);
    let post_operation_id = format!("post_{}", spec_name);

    let mut post_parameters: Vec<Value> = vec![rules_param];
    post_parameters.push(accept_datetime_header_parameter());
    if explanations_enabled {
        post_parameters.push(x_explanations_header_parameter());
    }

    let datetime_or_null = |dt: Option<&DateTimeValue>| -> Value {
        match dt {
            Some(d) => Value::String(d.to_string()),
            None => Value::Null,
        }
    };

    json!({
        "x-effective-from": datetime_or_null(effective_from),
        "x-effective-to": datetime_or_null(effective_to),
        "get": {
            "operationId": get_operation_id,
            "summary": get_summary,
            "tags": [tag],
            "parameters": get_parameters,
            "responses": {
                "200": {
                    "description": "Schema of resolved version (spec_set_id, effective_from, data, rules, meta, versions).",
                    "headers": memento_spec_response_headers(),
                    "content": {
                        "application/json": {
                            "schema": get_schema_ref
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
                    "description": "Evaluation envelope: spec, effective, result (per-rule RuleResultJson).",
                    "headers": memento_spec_response_headers(),
                    "content": {
                        "application/json": {
                            "schema": evaluate_schema_ref
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
        TypeSpecification::Undetermined => unreachable!(
            "BUG: type_help called with Undetermined sentinel type; this type must never reach OpenAPI generation"
        ),
    }
}

// ---------------------------------------------------------------------------
// POST request body schema generation (form-encoded — all string values)
// ---------------------------------------------------------------------------

fn build_post_request_schema(data: &[InputData]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();

    for data in data {
        properties.insert(
            data.name.clone(),
            build_post_property_schema(&data.lemma_type, data.default_value.as_ref()),
        );
        if data.default_value.is_none() {
            required.push(Value::String(data.name.clone()));
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
    data_value: Option<&lemma::LiteralValue>,
) -> Value {
    let mut schema = build_post_type_schema(lemma_type);

    let help = type_help(lemma_type);
    if !help.is_empty() {
        schema["description"] = Value::String(help);
    }

    if let Some(v) = data_value {
        schema["default"] = Value::String(v.display_value());
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
// Helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use lemma::parsing::ast::DateTimeValue;
    use lemma::SourceType;

    fn create_engine_with_code(code: &str) -> Engine {
        let mut engine = Engine::new();
        engine
            .load(code, SourceType::Labeled("test.lemma"))
            .expect("failed to parse lemma code");
        engine
    }

    fn create_engine_with_files(files: Vec<(&str, &str)>) -> Engine {
        let mut engine = Engine::new();
        for (name, code) in files {
            engine
                .load(code, SourceType::Labeled(name))
                .expect("failed to parse lemma code");
        }
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

    // =======================================================================
    // Basic spec structure (pre-existing, adapted)
    // =======================================================================

    #[test]
    fn test_generate_openapi_x_tag_groups() {
        let engine = create_engine_with_code(
            "spec pricing
            data quantity: 10
            rule total: quantity * 2",
        );
        let spec = generate_openapi(&engine, false);

        let groups = spec["x-tagGroups"]
            .as_array()
            .expect("x-tagGroups should be array");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["name"], "Overview");
        assert_eq!(groups[0]["tags"], json!(["Specs"]));
        assert_eq!(groups[1]["name"], "Specs");
        assert_eq!(groups[1]["tags"], json!(["pricing"]));
    }

    #[test]
    fn test_spec_path_has_get_and_post() {
        let engine = create_engine_with_code(
            "spec pricing
            data quantity: 10
            rule total: quantity * 2",
        );
        let spec = generate_openapi(&engine, false);

        assert!(
            spec["paths"]["/pricing"].is_object(),
            "single spec path /pricing"
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

        let get_ref = spec["paths"]["/pricing"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]["$ref"]
            .as_str()
            .unwrap();
        let post_ref = spec["paths"]["/pricing"]["post"]["responses"]["200"]["content"]
            ["application/json"]["schema"]["$ref"]
            .as_str()
            .unwrap();
        assert_eq!(get_ref, "#/components/schemas/pricing_get_response");
        assert_eq!(post_ref, "#/components/schemas/pricing_evaluate_response");
        assert_ne!(get_ref, post_ref);

        let get_schema = &spec["components"]["schemas"]["pricing_get_response"];
        assert!(get_schema["properties"]["spec_set_id"]["type"] == "string");
        assert!(get_schema["properties"]["versions"].is_object());

        let h200 = &spec["paths"]["/pricing"]["get"]["responses"]["200"];
        assert!(h200["headers"]["Memento-Datetime"].is_object());
        assert!(h200["headers"]["Vary"].is_object());
    }

    /// The generated OpenAPI document describes the public spec surface only.
    /// Server shell routes (`/openapi.json`, `/health`, `/docs`) and legacy
    /// schema routes (`/schema/{spec_name}` and `/schema/{spec_name}/{rules}`)
    /// are intentionally omitted; consumers must not rely on them for code
    /// generation or contract inspection.
    #[test]
    fn test_openapi_omits_shell_and_legacy_schema_routes() {
        let engine = create_engine_with_code(
            "spec pricing
            data quantity: 10
            rule total: quantity * 2",
        );
        let spec = generate_openapi(&engine, false);

        let paths = spec["paths"].as_object().expect("paths object");
        assert!(paths.contains_key("/"));
        assert_eq!(paths["/"]["get"]["operationId"], "list");
        assert!(!paths.contains_key("/openapi.json"));
        assert!(!paths.contains_key("/health"));
        assert!(!paths.contains_key("/docs"));
        assert!(!paths.contains_key("/schema/pricing"));
        assert!(!paths.contains_key("/schema/pricing/{rules}"));
        assert!(!paths.keys().any(|key| key.starts_with("/schema/")));
    }

    #[test]
    fn test_generate_openapi_explanations_enabled_adds_x_explanations_and_explanation_schema() {
        let engine = create_engine_with_code(
            "spec pricing
            data quantity: 10
            rule total: quantity * 2",
        );
        let spec = generate_openapi(&engine, true);

        let get_params = &spec["paths"]["/pricing"]["get"]["parameters"];
        assert!(has_param(get_params, "x-explanations"));

        let rule_result = &spec["components"]["schemas"]["LemmaRuleResult"];
        assert!(rule_result["properties"]["explanation"].is_object());
        assert!(rule_result["properties"]["vetoed"]["type"] == "boolean");
        assert!(rule_result["properties"]["rule_type"]["type"] == "string");

        let evaluate = &spec["components"]["schemas"]["pricing_evaluate_response"];
        assert!(evaluate["required"]
            .as_array()
            .unwrap()
            .contains(&json!("spec")));
        assert!(evaluate["required"]
            .as_array()
            .unwrap()
            .contains(&json!("effective")));
        assert!(evaluate["required"]
            .as_array()
            .unwrap()
            .contains(&json!("result")));
        let total_ref = evaluate["properties"]["result"]["properties"]["total"]["$ref"]
            .as_str()
            .unwrap();
        assert_eq!(total_ref, "#/components/schemas/LemmaRuleResult");
    }

    #[test]
    fn test_generate_openapi_multiple_specs() {
        let engine = create_engine_with_files(vec![
            (
                "pricing.lemma",
                "spec pricing
                data quantity: 10
                rule total: quantity * 2",
            ),
            (
                "shipping.lemma",
                "spec shipping
                data weight: 5
                rule cost: weight * 3",
            ),
        ]);
        let spec = generate_openapi(&engine, false);

        assert!(spec["paths"]["/pricing"].is_object());
        assert!(spec["paths"]["/shipping"].is_object());
    }

    #[test]
    fn test_nested_spec_path_schema_refs_are_valid() {
        let engine = create_engine_with_code(
            "spec a/b/c
        data x: number
        rule result: x",
        );
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

    // =======================================================================
    // generate_openapi_effective with explicit timestamp
    // =======================================================================

    #[test]
    fn test_generate_openapi_effective_reflects_specific_time() {
        let engine = create_engine_with_code(
            "spec pricing
            data quantity: 10
            rule total: quantity * 2",
        );
        let effective = date(2025, 6, 15);
        let spec = generate_openapi_effective(&engine, false, &effective);

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
spec policy
data base: 100
rule discount: 10

spec policy 2025-06-01
data base: 200
data premium: boolean
rule discount: 20
rule surcharge:
  5
  unless premium then 10
"#,
        )]);

        let before = date(2025, 3, 1);
        let spec_v1 = generate_openapi_effective(&engine, false, &before);

        assert!(spec_v1["paths"]["/policy"].is_object());
        let v1_evaluate = &spec_v1["components"]["schemas"]["policy_evaluate_response"];
        let v1_result = &v1_evaluate["properties"]["result"]["properties"];
        assert_eq!(
            v1_result["discount"]["$ref"].as_str(),
            Some("#/components/schemas/LemmaRuleResult"),
            "v1 should have discount rule"
        );
        assert!(
            v1_result["surcharge"].is_null(),
            "v1 must NOT have surcharge rule"
        );
        let v1_request = &spec_v1["components"]["schemas"]["policy_request"];
        assert!(
            v1_request["properties"]["premium"].is_null(),
            "v1 must NOT have premium data"
        );

        let after = date(2025, 8, 1);
        let spec_v2 = generate_openapi_effective(&engine, false, &after);

        let v2_evaluate = &spec_v2["components"]["schemas"]["policy_evaluate_response"];
        let v2_result = &v2_evaluate["properties"]["result"]["properties"];
        assert!(
            v2_result["discount"]["$ref"].is_string(),
            "v2 should have discount rule"
        );
        assert!(
            v2_result["surcharge"]["$ref"].is_string(),
            "v2 should have surcharge rule"
        );
        let v2_request = &spec_v2["components"]["schemas"]["policy_request"];
        assert!(
            v2_request["properties"]["premium"].is_object(),
            "v2 should have premium data"
        );
    }

    /// Each spec PathItem carries `x-effective-from` and `x-effective-to`
    /// describing the half-open `[effective_from, effective_to)` validity
    /// range of the version resolved at the document's effective instant.
    ///
    /// - Earlier row: `x-effective-to` = next row's `effective_from`.
    /// - Latest row: `x-effective-to` = `null` (no successor).
    /// - Unversioned spec (no declared `effective_from`): both extensions are
    ///   `null`.
    #[test]
    fn test_spec_path_item_exposes_half_open_effective_range_as_vendor_extensions() {
        let engine = create_engine_with_files(vec![(
            "policy.lemma",
            r#"
spec policy 2025-01-01
data base: 10
rule total: base

spec policy 2026-01-01
data base: 99
rule total: base
"#,
        )]);

        let at_earlier = date(2025, 6, 1);
        let earlier_doc = generate_openapi_effective(&engine, false, &at_earlier);
        let earlier_path = &earlier_doc["paths"]["/policy"];
        assert_eq!(
            earlier_path["x-effective-from"].as_str(),
            Some("2025-01-01"),
            "earlier version effective_from on PathItem"
        );
        assert_eq!(
            earlier_path["x-effective-to"].as_str(),
            Some("2026-01-01"),
            "earlier version effective_to equals next version's effective_from"
        );

        let at_latest = date(2026, 6, 1);
        let latest_doc = generate_openapi_effective(&engine, false, &at_latest);
        let latest_path = &latest_doc["paths"]["/policy"];
        assert_eq!(
            latest_path["x-effective-from"].as_str(),
            Some("2026-01-01"),
            "latest version effective_from on PathItem"
        );
        assert!(
            latest_path["x-effective-to"].is_null(),
            "latest version has no successor; x-effective-to must be null: {latest_path}"
        );
    }

    /// Unversioned specs (no declared `effective_from`) have both extensions
    /// serialised as JSON `null`, not omitted.
    #[test]
    fn test_spec_path_item_effective_extensions_null_for_unversioned_spec() {
        let engine = create_engine_with_code(
            "spec pricing
            data quantity: 10
            rule total: quantity * 2",
        );
        let document = generate_openapi(&engine, false);
        let path_item = &document["paths"]["/pricing"];
        assert!(
            path_item["x-effective-from"].is_null(),
            "unversioned spec: x-effective-from must be null: {path_item}"
        );
        assert!(
            path_item["x-effective-to"].is_null(),
            "unversioned spec: x-effective-to must be null: {path_item}"
        );
    }

    // =======================================================================
    // temporal_api_sources
    // =======================================================================

    #[test]
    fn test_temporal_sources_versioned_returns_boundaries_plus_now() {
        let engine = create_engine_with_files(vec![(
            "policy.lemma",
            r#"
spec policy
data base: 100
rule discount: 10

spec policy 2025-06-01
data base: 200
rule discount: 20
"#,
        )]);

        let sources = temporal_api_sources(&engine);

        assert_eq!(sources.len(), 2, "should have 1 now + 1 boundary");

        assert_eq!(sources[0].title, "Now");
        assert_eq!(sources[0].slug, NOW_SLUG);
        assert_eq!(sources[0].url, "/openapi.json");

        assert_eq!(sources[1].title, "Effective 2025-06-01");
        assert_eq!(sources[1].slug, "2025-06-01");
        assert_eq!(sources[1].url, "/openapi.json?effective=2025-06-01");
    }

    #[test]
    fn test_temporal_sources_multiple_specs_merged_boundaries() {
        let engine = create_engine_with_files(vec![
            (
                "policy.lemma",
                r#"
spec policy
data base: 100
rule discount: 10

spec policy 2025-06-01
data base: 200
rule discount: 20
"#,
            ),
            (
                "rates.lemma",
                r#"
spec rates
data rate: 5
rule total: rate * 2

spec rates 2025-03-01
data rate: 7
rule total: rate * 2

spec rates 2025-06-01
data rate: 9
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
        assert!(slugs.contains(&NOW_SLUG), "should contain now");
        assert_eq!(slugs.len(), 3, "2 unique boundaries + now");
    }

    #[test]
    fn test_temporal_sources_ordered_chronologically() {
        let engine = create_engine_with_files(vec![(
            "policy.lemma",
            r#"
spec policy
data base: 100
rule discount: 10

spec policy 2024-01-01
data base: 50
rule discount: 5

spec policy 2025-06-01
data base: 200
rule discount: 20
"#,
        )]);

        let sources = temporal_api_sources(&engine);
        let slugs: Vec<&str> = sources.iter().map(|s| s.slug.as_str()).collect();
        assert_eq!(slugs, vec![NOW_SLUG, "2025-06-01", "2024-01-01"]);
    }

    // =======================================================================
    // Type-specific parameter tests
    // =======================================================================

    #[test]
    fn test_post_schema_text_with_options_has_enum() {
        let engine = create_engine_with_code(
            "spec test
            data product: text -> option \"A\" -> option \"B\"
            rule result: product",
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
        let engine = create_engine_with_code(
            "spec test
            data is_active: boolean
            rule result: is_active",
        );
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        let is_active = &schema["properties"]["is_active"];
        assert_eq!(is_active["type"], "string");
        assert_eq!(is_active["enum"], json!(["true", "false"]));
    }

    #[test]
    fn test_post_schema_number_is_string() {
        let engine = create_engine_with_code(
            "spec test
            data quantity: number
            rule result: quantity",
        );
        let spec = generate_openapi(&engine, false);

        let schema = &spec["components"]["schemas"]["test_request"];
        assert_eq!(schema["properties"]["quantity"]["type"], "string");
    }

    #[test]
    fn test_data_with_default_is_not_required() {
        let engine = create_engine_with_code(
            "spec test
            data quantity: 10
            data name: text
            rule result: quantity
            rule label: name",
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
            r#"spec test
data quantity: number -> help "Number of items to order" -> default 10
data active: boolean -> help "Whether the feature is enabled" -> default true
rule result:
  quantity
  unless active then 0
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
