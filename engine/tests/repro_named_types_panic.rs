//! Repro for missing resolved type during graph build (TypeResolver / `ResolvedSpecTypes.resolved`).
//! Observed in WASM (browser). Mirrors the exact load sequence from the playground.

use lemma::{DateTimeValue, Engine, SourceType};
use std::sync::Arc;

fn path_source(path: &str) -> SourceType {
    SourceType::Path(Arc::new(std::path::PathBuf::from(path)))
}

/// Exact repro: dependency batch first, then workspace with two specs,
/// one referencing the dep and a dangling `uses f`.
#[test]
fn wasm_repro_dep_then_workspace_with_dangling_uses() {
    let mut engine = Engine::new();

    let dep_source = r#"spec constants

data pi: 3.14
"#;
    engine
        .load([(
            SourceType::Dependency("@benrogmans/test".to_string()),
            dep_source.to_string(),
        )])
        .expect("dependency batch loads");

    let workspace_source = r#"spec x

data x: 5


spec d23d23

data x: 6
data y: number
  -> minimum 5

uses b: @benrogmans/test constants

uses f
"#;

    let err = engine
        .load([(path_source("workspace.lemma"), workspace_source.to_string())])
        .expect_err("dangling `uses f` must be a planning error, not Ok or panic");
    let joined = err
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains('f')
            || joined.to_lowercase().contains("uses")
            || joined.to_lowercase().contains("import")
            || joined.to_lowercase().contains("missing"),
        "planning error must mention dangling import f, got: {joined}"
    );
}

/// Same structure but without the dangling `uses f` — should succeed cleanly.
#[test]
fn wasm_repro_dep_then_workspace_without_dangling_uses() {
    let mut engine = Engine::new();

    let dep_source = r#"spec constants

data pi: 3.14
"#;
    engine
        .load([(
            SourceType::Dependency("@benrogmans/test".to_string()),
            dep_source.to_string(),
        )])
        .expect("dependency batch loads");

    let workspace_source = r#"spec x

data x: 5


spec d23d23

data x: 6
data y: number
  -> minimum 5

uses b: @benrogmans/test constants
"#;

    engine
        .load([(path_source("workspace.lemma"), workspace_source.to_string())])
        .expect("workspace without dangling uses should load");

    let now = DateTimeValue::now();
    engine
        .show(None, "d23d23", Some(&now))
        .expect("d23d23 should plan");
}
