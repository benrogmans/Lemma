use lemma::{DateTimeValue, Engine};

#[test]
fn missing_dependency_load_errors_and_get_plan_not_found() {
    let mut engine = Engine::new();
    let code = r#"
spec consumer
uses @missing/dep some_spec
data x: number
rule y: x + some_spec.value
    "#;
    let load_result = engine.load(code, lemma::SourceType::Volatile);
    assert!(
        load_result.is_err(),
        "load must fail for missing dependency"
    );

    let now = DateTimeValue::now();
    let plan_result = engine.get_plan(None, "consumer", Some(&now));
    assert!(
        plan_result.is_err(),
        "get_plan must not serve a plan for a spec whose load failed"
    );
}
