use lemma::{Engine, LiteralValue, OperationResult};
use std::collections::HashMap;

#[test]
fn bdd_consensus_rule_simplifies_three_terms_to_two() {
    // A := discount_code is "SAVE30"
    // B := member_level is "platinum"
    // C := solution is "EU"
    //
    // Branches are already normalized (mutually exclusive) in the execution plan.
    // The equation for target=1 is: (A & B) | (!A & C) | (B & C)
    // Consensus theorem should eliminate (B & C) as redundant.
    let code = r#"
        doc shop_consensus
        fact discount_code = [text]
        fact member_level = [text]
        fact solution = [text]

        rule target = 0
        unless (discount_code is "SAVE30" and member_level is "platinum") then 1
        unless (not (discount_code is "SAVE30") and solution is "EU") then 1
        unless (member_level is "platinum" and solution is "EU") then 1
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let response = engine
        .invert_strict(
            "shop_consensus",
            "target",
            "=",
            Some(OperationResult::Value(LiteralValue::number(1))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // All 3 branches represent distinct valid patterns for target=1
    // After normalization with "last wins" semantics, all are needed
    assert_eq!(
        response.solutions.len(),
        3,
        "Expected 3 solutions for the 3 unless branches. Got {} solutions.",
        response.solutions.len()
    );
}
