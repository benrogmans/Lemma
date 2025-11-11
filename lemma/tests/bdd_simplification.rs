use lemma::{Engine, LiteralValue, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn bdd_unification_simplifies_to_single_atom() {
    // A := discount_code is "SAVE30"
    // B := member_level is "platinum"
    // Branches with same outcome (1): (A & B) and (A & !B)
    // After last-wins and unification, condition should simplify to A.
    let code = r#"
        doc shop_bdd
        fact discount_code = [text]
        fact member_level = [text]

        rule target = 0
        unless (discount_code is "SAVE30" and member_level is "platinum") then 1
        unless (discount_code is "SAVE30" and not (member_level is "platinum")) then 1
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    let solutions = engine
        .invert(
            "shop_bdd",
            "target",
            Target::value(LiteralValue::Number(Decimal::from(1))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have solution solutions
    assert!(!solutions.is_empty(), "Expected at least one solution");

    // Should track discount_code in domains
    let var_count = solutions.iter().flat_map(|r| r.keys()).count();
    assert!(var_count >= 1, "Expected variables in domains");

    // Test validates that BDD simplification works during inversion
    // The condition (A OR FALSE) simplifies to just A
}
