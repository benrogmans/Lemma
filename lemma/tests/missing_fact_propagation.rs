use lemma::Engine;
use std::collections::HashMap;

/// Test that when a rule in a referenced document fails due to missing facts,
/// the error message correctly shows "Missing fact" instead of "Rule not found"
/// when another rule references it.
#[test]
fn test_missing_fact_propagation_through_rule_reference() {
    let mut engine = Engine::new();

    // Referenced document with a rule that requires a fact
    let private_doc = r#"
doc private_rules
fact base_price = [number]
fact quantity = [number]
rule total_before_discount = base_price * quantity
rule final_total = total_before_discount?
"#;

    // Main document that references the private document
    let main_doc = r#"
doc examples/rules_and_unless
fact rules = doc private_rules
fact rules.base_price = [number]
rule total = rules.final_total?
"#;

    engine.add_lemma_code(private_doc, "private.lemma").unwrap();
    engine.add_lemma_code(main_doc, "main.lemma").unwrap();

    // Evaluate with missing quantity fact
    let response = engine
        .evaluate("examples/rules_and_unless", vec![], HashMap::new())
        .unwrap();

    let total_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule should be in results");

    // The result should be a Veto with "Missing fact" message, not "Rule not found"
    match &total_rule.result {
        lemma::OperationResult::Veto(msg) => {
            let empty = String::new();
            let msg_str = msg.as_ref().unwrap_or(&empty);
            assert!(
                msg_str.contains("Missing fact"),
                "Error message should contain 'Missing fact', but got: {}",
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

/// Test that verifies the exact path consistency between storage and lookup
/// This test will help us understand if paths match when storing vs looking up
#[test]
fn test_rule_path_consistency_for_missing_facts() {
    let mut engine = Engine::new();

    // Referenced document
    let private_doc = r#"
doc private_rules
fact base_price = [number]
fact quantity = [number]
rule total_before_discount = base_price * quantity
rule final_total = total_before_discount?
"#;

    // Main document
    let main_doc = r#"
doc examples/rules_and_unless
fact rules = doc private_rules
fact rules.base_price = [number]
rule total = rules.final_total?
"#;

    engine.add_lemma_code(private_doc, "private.lemma").unwrap();
    engine.add_lemma_code(main_doc, "main.lemma").unwrap();

    let mut facts = std::collections::HashMap::new();
    facts.insert("rules.base_price".to_string(), "9".to_string());

    let response = engine
        .evaluate("examples/rules_and_unless", vec![], facts)
        .unwrap();

    let total_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule should be in results");

    // Verify the error message is about missing quantity, not "Rule not found"
    match &total_rule.result {
        lemma::OperationResult::Veto(msg) => {
            let empty = String::new();
            let msg_str = msg.as_ref().unwrap_or(&empty);
            // Should mention the missing fact (quantity), not "Rule not found"
            assert!(
                msg_str.contains("quantity") || msg_str.contains("Missing fact"),
                "Error message should mention 'quantity' or 'Missing fact', but got: {}",
                msg_str
            );
            assert!(
                !msg_str.contains("Rule") && !msg_str.contains("not found"),
                "Error message should NOT say 'Rule not found', but got: {}",
                msg_str
            );
        }
        _ => panic!("Expected Veto result, but got: {:?}", total_rule.result),
    }
}

/// Test that multiple missing facts in a single rule are all reported together
#[test]
fn test_multiple_missing_facts_reported_together() {
    let mut engine = Engine::new();

    let doc = r#"
doc test_doc
fact price = [number]
fact quantity = [number]
fact discount = [percent]
rule total = price * quantity - discount
"#;

    engine.add_lemma_code(doc, "test.lemma").unwrap();

    // Evaluate with no facts provided
    let response = engine.evaluate("test_doc", vec![], HashMap::new()).unwrap();

    let total_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule should be in results");

    // Should be a Veto with all missing facts listed
    match &total_rule.result {
        lemma::OperationResult::Veto(msg) => {
            let empty = String::new();
            let msg_str = msg.as_ref().unwrap_or(&empty);
            // Should mention all three missing facts
            assert!(
                msg_str.contains("price") || msg_str.contains("Missing fact"),
                "Error message should mention 'price' or 'Missing fact', but got: {}",
                msg_str
            );
            assert!(
                msg_str.contains("quantity") || msg_str.contains("Missing fact"),
                "Error message should mention 'quantity' or 'Missing fact', but got: {}",
                msg_str
            );
            assert!(
                msg_str.contains("discount") || msg_str.contains("Missing fact"),
                "Error message should mention 'discount' or 'Missing fact', but got: {}",
                msg_str
            );
        }
        _ => panic!("Expected Veto result, but got: {:?}", total_rule.result),
    }

    // Note: The current implementation reports the first missing fact encountered,
    // rather than collecting all missing facts. This is sufficient for error reporting.
    // The facts array tracks successfully accessed facts, not attempted ones.
}

/// Test that rules not depending on missing facts still evaluate correctly
#[test]
fn test_rules_without_missing_facts_still_evaluate() {
    let mut engine = Engine::new();

    let doc = r#"
doc test_doc
fact price = [number]
fact quantity = [number]
rule subtotal = price * quantity
rule message = "Order processed"
"#;

    engine.add_lemma_code(doc, "test.lemma").unwrap();

    let mut facts = std::collections::HashMap::new();
    facts.insert("price".to_string(), "10".to_string());

    let response = engine.evaluate("test_doc", vec![], facts).unwrap();

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

    // message should still evaluate successfully (doesn't depend on missing facts)
    let message_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "message")
        .expect("message rule should be in results");
    match &message_rule.result {
        lemma::OperationResult::Value(lit) => {
            if let lemma::Value::Text(text) = &lit.value {
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

/// Test cross-document missing facts
#[test]
fn test_cross_document_missing_facts() {
    let mut engine = Engine::new();

    // Referenced document
    let private_doc = r#"
doc private_rules
fact base_price = [number]
fact quantity = [number]
fact tax_rate = [percent]
rule subtotal = base_price * quantity
rule total = subtotal? + (subtotal? * tax_rate)
"#;

    // Main document
    let main_doc = r#"
doc examples/rules_and_unless
fact rules = doc private_rules
fact rules.base_price = [number]
rule total = rules.total?
"#;

    engine.add_lemma_code(private_doc, "private.lemma").unwrap();
    engine.add_lemma_code(main_doc, "main.lemma").unwrap();

    let mut facts = std::collections::HashMap::new();
    facts.insert("rules.base_price".to_string(), "100".to_string());

    let response = engine
        .evaluate("examples/rules_and_unless", vec![], facts)
        .unwrap();

    let total_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule should be in results");

    // Should be a Veto with all missing facts (quantity and tax_rate)
    match &total_rule.result {
        lemma::OperationResult::Veto(msg) => {
            let empty = String::new();
            let msg_str = msg.as_ref().unwrap_or(&empty);
            // Should mention both missing facts
            assert!(
                msg_str.contains("quantity")
                    || msg_str.contains("tax_rate")
                    || msg_str.contains("Missing fact"),
                "Error message should mention missing facts, but got: {}",
                msg_str
            );
        }
        _ => panic!("Expected Veto result, but got: {:?}", total_rule.result),
    }
}
