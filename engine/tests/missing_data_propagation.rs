use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

/// Test that when a rule in a referenced spec fails due to missing data,
/// the error message correctly shows "Missing data" instead of "Rule not found"
/// when another rule references it.
#[test]
fn test_missing_data_propagation_through_rule_reference() {
    let mut engine = Engine::new();

    // Referenced spec with a rule that requires a data
    let private_spec = r#"
spec private_rules
data base_price: number
data quantity: number
rule total_before_discount: base_price * quantity
rule final_total: total_before_discount
"#;

    // Main spec that references the other spec
    let main_spec = r#"
spec examples/rules_and_unless
with rules: private_rules
data rules.base_price: 500
rule total: rules.final_total
"#;

    engine
        .load(private_spec, lemma::SourceType::Labeled("private.lemma"))
        .unwrap();
    engine
        .load(main_spec, lemma::SourceType::Labeled("main.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    // Evaluate with missing quantity data
    let response = engine
        .run(
            "examples/rules_and_unless",
            Some(&now),
            HashMap::new(),
            false,
        )
        .unwrap();

    let total_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule should be in results");

    // The result should be a Veto with "Missing data" message, not "Rule not found"
    match &total_rule.result {
        lemma::OperationResult::Veto(reason) => {
            let msg_str = reason.to_string();
            assert!(
                msg_str.contains("Missing data"),
                "Error message should contain 'Missing data', but got: {}",
                msg_str
            );
            assert!(
                !msg_str.contains("not found"),
                "Error message should NOT contain 'not found', but got: {}",
                msg_str
            );
        }
        _ => panic!("Expected Veto result, but got: {:?}", total_rule.result),
    }
}

/// Test that rules not depending on missing data still evaluate correctly
#[test]
fn test_rules_without_missing_data_still_evaluate() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_spec
data price: number
data quantity: number
rule subtotal: price * quantity
rule message: "Order processed"
"#;

    engine
        .load(spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut data = std::collections::HashMap::new();
    data.insert("price".to_string(), "10".to_string());

    let now = DateTimeValue::now();
    let response = engine.run("test_spec", Some(&now), data, false).unwrap();

    // subtotal should fail due to missing quantity
    let subtotal_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "subtotal")
        .expect("subtotal rule should be in results");
    assert!(
        matches!(subtotal_rule.result, lemma::OperationResult::Veto(_)),
        "subtotal should be Veto due to missing quantity"
    );

    // message should still evaluate successfully (doesn't depend on missing data, None)
    let message_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "message")
        .expect("message rule should be in results");
    match &message_rule.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::ValueKind::Text(text) = &lit.value {
                assert_eq!(text, "Order processed");
            } else {
                panic!("Expected text result");
            }
        }
        _ => panic!(
            "message rule should evaluate successfully, but got: {:?}",
            message_rule.result
        ),
    }
}

/// A reference whose target has no value at eval time must surface as a
/// MissingData veto at any rule consuming the reference, naming the
/// reference path (not the target path, which is an implementation detail).
#[test]
fn reference_with_missing_target_vetoes_as_missing_data() {
    let code = r#"
spec inner
data slot: number

spec outer
with i: inner
data here: i.slot
rule r: here
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("missing.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let resp = engine
        .run("outer", Some(&now), HashMap::new(), false)
        .expect("evaluates");

    let rr = resp.results.get("r").expect("rule 'r'");
    match &rr.result {
        lemma::OperationResult::Veto(lemma::VetoType::MissingData { data }) => {
            // The reference's own name is what the rule referenced. It
            // should be the one reported, not the target path in the inner
            // spec.
            let shown = data.to_string();
            assert!(
                shown.contains("here"),
                "missing-data veto from reference consumption should name 'here' (the reference path); \
                 got: {shown}"
            );
        }
        other => panic!("expected MissingData veto, got: {:?}", other),
    }
}

/// Rule-target reference whose target rule returns a Veto must propagate
/// that veto to any consumer.
#[test]
fn rule_target_reference_veto_propagates_to_consumer() {
    let code = r#"
spec inner
data denom: number -> default 0
rule divided: 10 / denom

spec top
with i: inner
data x: i.divided
rule out: x
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("missing.lemma"))
        .expect("rule-target reference must be accepted at plan time");

    let now = DateTimeValue::now();
    let resp = engine
        .run("top", Some(&now), HashMap::new(), false)
        .expect("evaluator must run; veto is a domain result, not an error");

    let rr = resp.results.get("out").expect("rule 'out'");
    match &rr.result {
        lemma::OperationResult::Veto(v) => {
            let s = v.to_string();
            assert!(
                s.contains("Division by zero"),
                "rule-target reference must propagate the exact veto reason of the target rule, \
                 got: {s}"
            );
        }
        other => panic!("expected propagated veto, got: {:?}", other),
    }
}
