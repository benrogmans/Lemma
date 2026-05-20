//! Repro for missing resolved type during graph build (TypeResolver / `ResolvedSpecTypes.resolved`).
//! Observed in WASM (browser). Mirrors the exact load sequence from the playground.

use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;
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
        .load_batch(
            HashMap::from([(path_source("deps/test.lemma"), dep_source.to_string())]),
            Some("@benrogmans/test"),
        )
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

    let result = engine.load(workspace_source, path_source("workspace.lemma"));
    // `uses f` is a dangling import — should produce a planning error, not a panic.
    match result {
        Ok(()) => {
            // If it loads, try planning both specs
            let now = DateTimeValue::now();
            let _ = engine.get_plan(None, "x", Some(&now));
            let _ = engine.get_plan(None, "d23d23", Some(&now));
        }
        Err(e) => {
            // Error is acceptable (dangling `uses f`), panic is not.
            eprintln!("Expected error (not panic): {:?}", e);
        }
    }
}

/// Same structure but without the dangling `uses f` — should succeed cleanly.
#[test]
fn wasm_repro_dep_then_workspace_without_dangling_uses() {
    let mut engine = Engine::new();

    let dep_source = r#"spec constants

data pi: 3.14
"#;
    engine
        .load_batch(
            HashMap::from([(path_source("deps/test.lemma"), dep_source.to_string())]),
            Some("@benrogmans/test"),
        )
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
        .load(workspace_source, path_source("workspace.lemma"))
        .expect("workspace without dangling uses should load");

    let now = DateTimeValue::now();
    engine
        .get_plan(None, "d23d23", Some(&now))
        .expect("d23d23 should plan");
}
