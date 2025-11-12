use lemma::{Engine, LiteralValue, Target};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn bdd_partial_simplification_on_large_expression() {
    // Build a document with many independent atoms to exceed the 64-atom cap overall,
    // while embedding a small pattern (A&B)|(A&!B) that should still reduce to A locally.
    let mut code = String::from(
        "doc shop_partial\n\nfact discount_code = [text]\nfact member_level = [text]\n",
    );

    // Add 70 extra text facts and use them in a big OR to push atom count > 64
    let n_extra = 70;
    for i in 1..=n_extra {
        code.push_str(&format!("fact tag{} = [text]\n", i));
    }

    code.push_str("\nrule target = 0\n  unless ((discount_code is \"SAVE30\" and member_level is \"platinum\") or (discount_code is \"SAVE30\" and not (member_level is \"platinum\"))) and (" );
    for i in 1..=n_extra {
        if i > 1 {
            code.push_str(" or ");
        }
        code.push_str(&format!("tag{} is \"yes\"", i));
    }
    code.push_str(") then 1\n");

    let mut engine = Engine::new();
    engine.add_lemma_code(&code, "gen").unwrap();

    let solutions = engine
        .invert(
            "shop_partial",
            "target",
            Target::value(LiteralValue::Number(Decimal::from(1))),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert!(!solutions.is_empty(), "Expected at least one solution");
}
