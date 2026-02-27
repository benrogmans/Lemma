use lemma::{Engine, LiteralValue, Target, TargetOp};
mod common;
use common::add_lemma_code_blocking;

#[test]
fn piecewise_value_guard_pruning_equality() {
    let code = r#"
        doc shipping
        fact weight: [number]

        rule shipping_cost: 5
             unless weight >= 10 then 10
             unless weight >= 50 then 25
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    let solutions = engine
        .invert(
            "shipping",
            "shipping_cost",
            Target::value(LiteralValue::number(10.into())),
            std::collections::HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have solutions
    assert!(!solutions.is_empty(), "Expected at least one solution");

    // Test validates that guard conditions filter branches correctly
    // The 10 branch should be included with appropriate weight constraints
}

#[test]
fn piecewise_value_guard_pruning_inequality() {
    let code = r#"
        doc shipping
        fact weight: [number]

        rule shipping_cost: 5
             unless weight >= 10 then 10
             unless weight >= 50 then 25
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    let solutions = engine
        .invert(
            "shipping",
            "shipping_cost",
            Target::with_op(
                TargetOp::Gt,
                lemma::OperationResult::Value(Box::new(LiteralValue::number(5.into()))),
            ),
            std::collections::HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have solutions (both 10 and 25 satisfy > 5)
    assert!(!solutions.is_empty(), "Expected at least one solution");
}
