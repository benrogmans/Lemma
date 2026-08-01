//! Fixture coverage and SDK content locks — assert existing content only.
//! No generation, no stamps, no xtask spawn.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn api_schema_path() -> PathBuf {
    workspace_root().join("documentation/schemas/api.v1.json")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api")
}

fn load_schema() -> serde_json::Value {
    let path = api_schema_path();
    assert!(
        path.is_file(),
        "api.v1.json must exist before fixture coverage can run"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("read schema")).expect("schema JSON")
}

/// Collect enum/const string variants from a JSON Schema document.
fn collect_schema_variants(schema: &serde_json::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk_schema(schema, &mut out);
    out
}

fn walk_schema(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(consts)) = map.get("enum") {
                for item in consts {
                    if let Some(s) = item.as_str() {
                        out.insert(s.to_string());
                    }
                }
            }
            if let Some(serde_json::Value::String(c)) = map.get("const") {
                out.insert(c.clone());
            }
            for child in map.values() {
                walk_schema(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                walk_schema(child, out);
            }
        }
        _ => {}
    }
}

fn collect_fixture_strings(dir: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read fixtures: {e}"));
    for entry in entries {
        let entry = entry.expect("dirent");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read fixture");
        let json: serde_json::Value = serde_json::from_str(&text).expect("fixture JSON");
        collect_strings(&json, &mut out);
    }
    out
}

fn collect_strings(value: &serde_json::Value, out: &mut HashSet<String>) {
    match value {
        serde_json::Value::String(s) => {
            out.insert(s.clone());
        }
        serde_json::Value::Object(map) => {
            for child in map.values() {
                collect_strings(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_strings(child, out);
            }
        }
        _ => {}
    }
}

#[test]
fn every_schema_enumerated_variant_appears_in_a_fixture() {
    let schema = load_schema();
    let required = collect_schema_variants(&schema);
    assert!(
        !required.is_empty(),
        "api.v1.json must enumerate variants (enum/const)"
    );
    let present = collect_fixture_strings(&fixtures_dir());
    let missing: Vec<_> = required.iter().filter(|v| !present.contains(*v)).collect();
    assert!(
        missing.is_empty(),
        "schema variants missing from fixtures: {missing:?}"
    );
}

#[test]
fn coverage_check_fails_when_a_variant_has_no_fixture() {
    // Meta-test: the forcing function itself must fail closed.
    // Uses an in-memory required set so this does not collapse into "schema missing".
    let mut required = BTreeSet::new();
    required.insert("__coverage_canary_variant_that_must_not_exist__".to_string());
    let present = collect_fixture_strings(&fixtures_dir());
    let missing: Vec<_> = required
        .iter()
        .filter(|v| !present.contains(v.as_str()))
        .cloned()
        .collect();
    assert!(
        missing
            .iter()
            .any(|m| m == "__coverage_canary_variant_that_must_not_exist__"),
        "coverage check must report absent variants; got missing={missing:?}"
    );
}

#[test]
fn show_minimal_fixture_deserializes_as_lemma_show() {
    let path = fixtures_dir().join("show_minimal.json");
    let text = std::fs::read_to_string(&path).expect("read show_minimal.json");
    let show: lemma::Show = serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("fixture must match lemma::Show API types: {e}\nfixture:\n{text}")
    });
    assert_eq!(show.spec, "sample");
}

/// Same Rust field must not have two TypeScript declarations.
#[test]
fn lemma_d_ts_effective_from_types_agree_on_show_and_listed_spec() {
    let path = workspace_root().join("engine/packages/npm/lemma.d.ts");
    let text = std::fs::read_to_string(&path).expect("read lemma.d.ts");

    let show_type = interface_field_type(&text, "Show", "effective_from")
        .expect("Show.effective_from must be declared in lemma.d.ts");
    let listed_type = interface_field_type(&text, "ListedSpec", "effective_from")
        .expect("ListedSpec.effective_from must be declared in lemma.d.ts");

    assert_eq!(
        show_type, listed_type,
        "Show.effective_from and ListedSpec.effective_from must be the same API type; \
         got Show={show_type:?} ListedSpec={listed_type:?}"
    );
}

fn interface_field_type<'a>(source: &'a str, interface: &str, field: &str) -> Option<&'a str> {
    let marker = format!("export interface {interface} ");
    let start = source.find(&marker)?;
    let after = &source[start..];
    let body_start = after.find('{')?;
    let rest = &after[body_start + 1..];
    let body_end = rest.find("\n}")?;
    let body = &rest[..body_end];
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with(field) {
            continue;
        }
        // `effective_from?: string | null;`
        let after_name = trimmed.strip_prefix(field)?;
        let after_name = after_name.trim_start();
        let after_name = after_name
            .strip_prefix('?')
            .unwrap_or(after_name)
            .trim_start();
        let after_name = after_name.strip_prefix(':')?.trim_start();
        let ty = after_name.trim_end_matches(';').trim();
        return Some(ty);
    }
    None
}
