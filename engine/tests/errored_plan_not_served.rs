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
    let load_result = engine.load([(lemma::SourceType::Volatile, code.to_string())]);
    assert!(
        load_result.is_err(),
        "load must fail for missing dependency"
    );

    let now = DateTimeValue::now();
    let show_result = engine.show(None, "consumer", Some(&now));
    assert!(
        show_result.is_err(),
        "show must not serve a plan for a spec whose load failed"
    );
}
