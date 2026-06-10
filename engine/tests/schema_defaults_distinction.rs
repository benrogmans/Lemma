//! Spec-declared defaults stay on [`DataDefinition::TypeDeclaration`] in the plan;
//! the evaluator applies them when the caller does not supply a value.

use lemma::DateTimeValue;
use lemma::Engine;
use lemma::{DataDefinition, DataOverlay, DataPath};
use std::collections::HashMap;

#[test]
fn typedecl_default_stays_typedecl_on_immutable_plan() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
        spec s
        data n: number -> default 42
        rule r: n
    "#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("s.lemma"))),
        )
        .expect("load");

    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "s", Some(&now)).expect("plan");
    let path = DataPath::local("n".into());
    match plan.data.get(&path).expect("n") {
        DataDefinition::TypeDeclaration {
            declared_default: Some(_),
            ..
        } => {}
        other => panic!("expected TypeDeclaration with default, got {other:?}"),
    }
}

#[test]
fn run_plan_applies_typedecl_default_when_not_supplied() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
        spec s
        data n: number -> default 42
        rule r: n
    "#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("raw.lemma"))),
        )
        .expect("load");

    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "s", Some(&now)).expect("plan");
    let response = engine
        .run_plan(plan, Some(&now), HashMap::new(), false, None)
        .expect("response");

    assert!(
        !response
            .missing_data_ordered()
            .iter()
            .any(|p| p.input_key() == "n"),
        "evaluator must apply default for n, got missing {:?}",
        response.missing_data_ordered()
    );

    let rule = response.results.get("r").expect("rule r");
    assert!(!rule.vetoed, "rule r must succeed with default n=42");
    assert_eq!(rule.display.as_deref(), Some("42"));
}

#[test]
fn schema_shows_default_not_bound_without_overlay() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
        spec s
        data n: number -> default 42
        rule r: n
    "#,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("s.lemma"))),
        )
        .expect("load");

    let now = DateTimeValue::now();
    let plan = engine.get_plan(None, "s", Some(&now)).expect("plan");
    let entry = plan
        .schema(&DataOverlay::default())
        .data
        .get("n")
        .expect("n in schema")
        .clone();

    assert!(
        entry.bound_value.is_none(),
        "default is not bound until caller supplies"
    );
    assert!(
        entry.default.is_some(),
        "schema must expose default suggestion"
    );
}
