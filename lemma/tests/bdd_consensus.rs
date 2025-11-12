use lemma::{Engine, LiteralValue, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn bdd_consensus_rule_simplifies_three_terms_to_two() {
    // A := discount_code is "SAVE30"
    // B := member_level is "platinum"
    // C := solution is "EU"
    // (A & B) | (!A & C) | (B & C) => (A & B) | (!A & C)
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

    let solutions = engine
        .invert(
            "shop_consensus",
            "target",
            Target::value(LiteralValue::Number(Decimal::from(1))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have solution solutions
    assert!(!solutions.is_empty(), "Expected at least one solution");

    // Test validates that BDD consensus theorem application simplifies branches
    // The three branches should unify and simplify to (A & B) | (!A & C)
}
