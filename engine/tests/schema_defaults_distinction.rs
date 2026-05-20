//! Defaults on the stored plan are suggestions until [`lemma::planning::ExecutionPlan::with_defaults`] runs.

use lemma::parsing::ast::DateTimeValue;
use lemma::planning::semantics::{DataDefinition, DataPath};
use lemma::Engine;
use std::collections::HashMap;

#[test]
fn typedecl_default_stays_typedecl_until_with_defaults() {
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

    let matured = plan.clone().with_defaults();
    match matured.data.get(&path).expect("n") {
        DataDefinition::Value { .. } => {}
        other => panic!("expected Value after with_defaults, got {other:?}"),
    }
}

#[test]
fn run_plan_without_defaults_surfaces_missing_data_for_typedecl_default() {
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
        .run_plan_without_defaults(
            plan,
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("response");

    assert!(
        response
            .missing_data_ordered()
            .iter()
            .any(|p| p.input_key() == "n"),
        "expected missing n without with_defaults, got {:?}",
        response.missing_data_ordered()
    );
}
