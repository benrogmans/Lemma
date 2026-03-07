use lemma::{Engine, LiteralValue, Target};
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use std::collections::HashMap;

#[test]
fn bdd_partial_simplification_on_large_expression() {
    // Build a spec with many unless branches (discount_code and tag_i) to stress BDD/solver.
    let mut code = String::from("spec shop_partial\n\nfact discount_code: [text]\n");

    let n_extra = 20;
    for i in 1..=n_extra {
        code.push_str(&format!("fact tag{}: [text]\n", i));
    }

    code.push_str("\nrule target: 0\n");
    for i in 1..=n_extra {
        code.push_str(&format!(
            "  unless discount_code is \"SAVE30\" and tag{} is \"yes\" then 1\n",
            i
        ));
    }

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, &code, "gen").unwrap();

    let now = DateTimeValue::now();
    let solutions = engine
        .invert(
            "shop_partial",
            &now,
            "target",
            Target::value(LiteralValue::number(1.into())),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert!(!solutions.is_empty(), "Expected at least one solution");
}
