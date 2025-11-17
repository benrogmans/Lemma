use lemma::Engine;

/// Test that when a rule in a referenced document fails due to missing facts,
/// the error message correctly shows "Missing fact" instead of "Rule not found"
/// when another rule references it.
#[test]
fn test_missing_fact_propagation_through_rule_reference() {
    let mut engine = Engine::new();

    // Referenced document with a rule that requires a fact
    let private_doc = r#"
doc private_rules
fact base_price = [money]
fact quantity = [number]
rule total_before_discount = base_price * quantity
rule final_total = total_before_discount?
"#;

    // Main document that references the private document
    let main_doc = r#"
doc examples/rules_and_unless
fact rules = doc private_rules
fact rules.base_price = [money]
rule total = rules.final_total?
"#;

    engine.add_lemma_code(private_doc, "private.lemma").unwrap();
    engine.add_lemma_code(main_doc, "main.lemma").unwrap();

    // Evaluate with missing quantity fact
    let response = engine
        .evaluate("examples/rules_and_unless", None, None)
        .unwrap();

    let total_rule = response
        .results
        .iter()
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
fact base_price = [money]
fact quantity = [number]
rule total_before_discount = base_price * quantity
rule final_total = total_before_discount?
"#;

    // Main document
    let main_doc = r#"
doc examples/rules_and_unless
fact rules = doc private_rules
fact rules.base_price = [money]
rule total = rules.final_total?
"#;

    engine.add_lemma_code(private_doc, "private.lemma").unwrap();
    engine.add_lemma_code(main_doc, "main.lemma").unwrap();

    // Evaluate with only base_price, missing quantity
    let fact_strings = vec!["rules.base_price=9 USD"];
    let facts = lemma::parse_facts(&fact_strings).unwrap();
    let response = engine
        .evaluate("examples/rules_and_unless", None, Some(facts))
        .unwrap();

    let total_rule = response
        .results
        .iter()
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
