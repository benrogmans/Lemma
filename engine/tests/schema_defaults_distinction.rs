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
fn schema_shows_default_not_prefilled_without_overlay() {
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
        entry.prefilled.is_none(),
        "default is not prefilled until caller supplies"
    );
    assert!(
        entry.default.is_some(),
        "schema must expose default suggestion"
    );
}

#[test]
fn template_literal_with_prefills_nested_slot_not_default() {
    let code = r#"
spec a
data x: number -> minimum 0 -> default 1
rule r: x

spec a/template
uses a
with a.x: 2
rule r: a.r
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("load");

    let now = DateTimeValue::now();
    let base = engine
        .get_plan(None, "a", Some(&now))
        .expect("base plan")
        .schema(&DataOverlay::default());
    let base_x = base.data.get("x").expect("x in base schema");
    assert!(base_x.default.is_some(), "base spec exposes default on x");
    assert!(base_x.prefilled.is_none(), "base spec has no prefilled x");

    let template = engine
        .get_plan(None, "a/template", Some(&now))
        .expect("template plan")
        .schema(&DataOverlay::default());
    let template_x = template.data.get("a.x").expect("a.x in template schema");
    assert!(
        template_x.prefilled.is_some(),
        "literal with must surface as prefilled"
    );
    assert!(
        template_x.default.is_none(),
        "template must not inherit typedef default once slot is prefilled"
    );
}

#[test]
fn schema_overlay_sets_supplied_not_prefilled() {
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
    let overlay = DataOverlay::resolve(
        plan,
        [(
            "n".to_string(),
            lemma::DataValueInput::convenience("7".to_string()),
        )]
        .into(),
        engine.limits(),
    )
    .expect("overlay");

    let entry = plan
        .schema(&overlay)
        .data
        .get("n")
        .expect("n in schema")
        .clone();

    assert!(
        entry.prefilled.is_none(),
        "overlay must not appear as prefilled"
    );
    assert!(entry.supplied.is_some(), "overlay must appear as supplied");
    assert!(
        entry.default.is_some(),
        "typedef default remains for documentation"
    );
}
