//! Parity targets for WasmEngine-style loading: [`Engine::load_batch`] with a dependency id
//! tags every repository parsed from that batch as that dependency (see `Engine::add_sources_inner`).
//! Browser stacks often load sources that way; plain CLI `load` keeps workspace `name: None`.
//!
//! These tests exercise `get_plan` + `schema` at several effective instants (mirrors
//! `WasmEngine::schema` defaulting effective to [`DateTimeValue::now`]) and cross-load patterns
//! that stress type resolution after a dependency batch.

use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;
use std::sync::Arc;

fn path_source(path: &str) -> SourceType {
    SourceType::Path(Arc::new(std::path::PathBuf::from(path)))
}

fn wasm_style_instants() -> [DateTimeValue; 4] {
    [
        DateTimeValue::now(),
        DateTimeValue {
            year: 2026,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        },
        DateTimeValue {
            year: 2024,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        },
        DateTimeValue {
            year: 2020,
            month: 6,
            day: 15,
            hour: 12,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        },
    ]
}

#[test]
fn dependency_batch_get_plan_and_schema_via_repo_qualifier() {
    let mut engine = Engine::new();
    let bundle = r#"
spec finance
data money: ratio -> decimals 2 -> minimum 0

spec cashier
uses F: finance
data till: F.money
rule total: till
"#;
    engine
        .load_batch(
            HashMap::from([(SourceType::Volatile, bundle.to_string())]),
            Some("@lemma/std"),
        )
        .expect("dependency-tag load_batch should parse and plan");

    for instant in wasm_style_instants() {
        let plan = engine
            .get_plan(Some("@lemma/std"), "cashier", Some(&instant))
            .expect("get_plan(Some(dep), cashier) mirrors wasm.schema with repository set");
        let _ = plan.schema();

        engine
            .schema(Some("@lemma/std"), "cashier", Some(&instant))
            .expect("Engine::schema same path as WasmEngine.schema");
        engine
            .schema(Some("@lemma/std"), "finance", Some(&instant))
            .expect("finance exposes schema inside dependency-qualified repository");
    }
}

#[test]
fn workspace_consumer_after_dependency_batch_resolves_money_type() {
    let mut engine = Engine::new();
    engine
        .load_batch(
            HashMap::from([(
                path_source("deps/stdlib.lemma"),
                r#"spec finance
data money: ratio -> decimals 2 -> minimum 0
"#
                .to_string(),
            )]),
            Some("@lemma/stdlib"),
        )
        .expect("anonymous dependency bundle (repo name `@lemma/stdlib`) loads");

    engine
        .load(
            r#"spec kiosk
uses @lemma/stdlib finance
data drawer: money from finance -> minimum 0
rule tally: drawer
"#,
            path_source("kiosk.lemma"),
        )
        .expect("workspace kiosk uses registry-style qualifier line like examples/12_registry_references");

    for instant in wasm_style_instants() {
        let plan = engine.get_plan(None, "kiosk", Some(&instant));
        match plan {
            Ok(p) => {
                let _ = p.schema();
                let _ = engine.schema(None, "kiosk", Some(&instant));
            }
            Err(e) => {
                panic!(
                    "kiosk should plan across instants once dependency types resolve — error: {}",
                    e
                );
            }
        }
    }
}

#[test]
fn duplicate_named_finance_across_named_repositories_plan_without_panic() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo tier_a

spec finance
data x: number
rule gross: x
"#,
            path_source("a.lemma"),
        )
        .expect("tier_a repository loads");

    engine
        .load_batch(
            HashMap::from([(
                path_source("tier_b/bundle.lemma"),
                r#"repo tier_b

spec finance
data y: number
rule gross: y
"#
                .to_string(),
            )]),
            Some("tier_b_dep"),
        )
        .expect("tier_b dependency bundle loads with duplicate bare spec name `finance`");

    let counts: Vec<(Option<String>, usize)> = engine
        .list()
        .iter()
        .map(|entry| (entry.repository.name.clone(), entry.specs.len()))
        .collect();

    assert!(
        counts
            .iter()
            .any(|(name, _)| name.as_deref() == Some("tier_a")),
        "expected tier_a repository in {:?}",
        counts
    );
    assert!(
        counts
            .iter()
            .any(|(name, _)| name.as_deref() == Some("tier_b")),
        "expected tier_b repository in {:?}",
        counts
    );

    let now = DateTimeValue::now();
    engine
        .get_plan(Some("tier_a"), "finance", Some(&now))
        .expect("finance in tier_a");
    engine
        .get_plan(Some("tier_b"), "finance", Some(&now))
        .expect("finance in tier_b");

    engine
        .schema(Some("tier_a"), "finance", Some(&now))
        .expect("schema tier_a finance");
    engine
        .schema(Some("tier_b"), "finance", Some(&now))
        .expect("schema tier_b finance");
}
