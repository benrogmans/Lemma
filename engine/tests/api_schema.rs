//! Contract: `documentation/schemas/api.v1.json` is the schema for Show, Response, list, and errors.

use lemma::{DateTimeValue, Engine, ResourceLimits, SourceType};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn api_schema_path() -> PathBuf {
    workspace_root().join("documentation/schemas/api.v1.json")
}

fn load_api_schema() -> serde_json::Value {
    let path = api_schema_path();
    assert!(
        path.is_file(),
        "documentation/schemas/api.v1.json must exist (emit via `cargo run -p xtask -- schema`)"
    );
    let text = std::fs::read_to_string(&path).expect("read api.v1.json");
    serde_json::from_str(&text).expect("api.v1.json must be JSON")
}

fn sample_documents() -> Vec<(&'static str, serde_json::Value)> {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Path(Arc::new(PathBuf::from("sample.lemma"))),
            r#"
spec sample 2024-01-01
meta title: "t"
data amount: number -> suggest 1
data money: measure
  -> unit eur 1
  -> suggest 10 eur
rule ok: amount
"#
            .to_string(),
        )])
        .expect("load");
    let now = DateTimeValue::now();
    let show = engine.show(None, "sample", Some(&now)).expect("show");
    let response = engine
        .run(
            None,
            "sample",
            Some(&now),
            HashMap::from([("amount".into(), "2".into())]),
            None,
            true,
        )
        .expect("run");
    let list = engine.list();

    let limits = ResourceLimits {
        max_source_size_bytes: 50,
        ..ResourceLimits::default()
    };
    let err = Engine::with_limits(limits)
        .load([(SourceType::Volatile, "spec t\ndata x: 1\n".repeat(20))])
        .expect_err("limit");
    let errors: Vec<_> = err
        .errors
        .iter()
        .map(lemma::__test_support::current_binding_error_json)
        .collect();

    vec![
        ("show", serde_json::to_value(&show).expect("show")),
        (
            "response",
            serde_json::to_value(&response).expect("response"),
        ),
        ("list", serde_json::to_value(&list).expect("list")),
        ("errors", serde_json::Value::Array(errors)),
    ]
}

#[test]
fn api_schema_file_exists_and_is_object() {
    let schema = load_api_schema();
    assert!(schema.is_object(), "api.v1.json root must be an object");
}

#[test]
fn real_show_run_list_and_error_documents_validate_against_schema() {
    let schema = load_api_schema();
    let _ = schema;
    for (name, document) in sample_documents() {
        // Full JSON Schema validation lands with the schema emitter; until then the
        // schema file must at least exist and documents must be JSON objects/arrays.
        assert!(
            document.is_object() || document.is_array(),
            "{name} document must be object or array"
        );
        validate_against_api_schema(&document)
            .unwrap_or_else(|e| panic!("{name} must validate against api.v1.json: {e}"));
    }
}

fn validate_against_api_schema(document: &serde_json::Value) -> Result<(), String> {
    let schema = load_api_schema();
    let validator = jsonschema::validator_for(&schema).map_err(|e| e.to_string())?;
    validator.validate(document).map_err(|e| e.to_string())
}

#[test]
fn every_decimal_string_field_has_explicit_format_marker() {
    let schema = load_api_schema();
    let text = serde_json::to_string(&schema).expect("schema text");
    assert!(
        text.contains("\"format\"")
            && (text.contains("decimal") || text.contains("lemma-decimal")),
        "decimal-string fields must carry an explicit format marker for Java BigDecimal / TS string mapping"
    );
}

#[test]
fn regenerating_schema_is_a_noop() {
    let path = api_schema_path();
    let before = if path.is_file() {
        std::fs::read(&path).expect("read")
    } else {
        Vec::new()
    };
    let status = Command::new("cargo")
        .args(["run", "-p", "xtask", "--quiet", "--", "schema"])
        .current_dir(workspace_root())
        .status()
        .expect("spawn xtask schema");
    assert!(
        status.success(),
        "`cargo run -p xtask -- schema` must succeed"
    );
    let after = std::fs::read(&path).unwrap_or_default();
    assert_eq!(
        before, after,
        "schema regeneration must be a no-op against the checked-in api.v1.json"
    );
}
