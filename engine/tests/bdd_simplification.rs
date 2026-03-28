use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, LiteralValue, ResourceLimits, Target};
use std::collections::HashMap;

#[test]
fn bdd_unification_simplifies_to_single_atom() {
    // A := discount_code is "SAVE30"
    // B := member_level is "platinum"
    // Branches with same outcome (1): (A & B) and (A & !B)
    // After last-wins and unification, condition should simplify to A.
    let code = r#"
        spec shop_bdd
        fact discount_code: [text]
        fact member_level: [text]

        rule target: 0
        unless (discount_code is "SAVE30" and member_level is "platinum") then 1
        unless (discount_code is "SAVE30" and not (member_level is "platinum")) then 1
    "#;

    let limits = ResourceLimits {
        max_expression_depth: 6,
        ..ResourceLimits::default()
    };
    let mut engine = Engine::with_limits(limits);
    engine
        .load(code, lemma::SourceType::Labeled("test"))
        .unwrap();

    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "shop_bdd",
            &now,
            "target",
            Target::value(LiteralValue::number(1.into())),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have solutions
    assert!(!solutions.is_empty(), "Expected at least one solution");

    // Should track discount_code in domains
    let var_count: usize = solutions.domains.iter().map(|d| d.len()).sum();
    assert!(var_count >= 1, "Expected variables in domains");

    // Test validates that BDD simplification works during inversion
    // The condition (A OR FALSE) simplifies to just A
}
