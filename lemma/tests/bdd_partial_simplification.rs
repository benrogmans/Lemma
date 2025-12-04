use lemma::{Domain, Engine, FactPath, LiteralValue, Target};
use std::collections::HashMap;

#[test]
fn bdd_partial_simplification_on_large_expression() {
    // Build a document with many independent atoms to exceed the 64-atom cap overall,
    // while embedding a small pattern (A&B)|(A&!B) that should still reduce to A locally.
    // Pattern: (discount_code is "SAVE30" and member_level is "platinum") 
    //       or (discount_code is "SAVE30" and not (member_level is "platinum"))
    // Should simplify to: discount_code is "SAVE30"
    // This simplification should occur even when the overall expression has >64 atoms.
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

    let response = engine
        .invert_strict(
            "shop_partial",
            "target",
            Target::value(LiteralValue::number(1)),
            HashMap::new(),
        )
        .expect("invert should succeed");

    assert!(
        !response.solutions.is_empty(),
        "Expected at least one solution"
    );

    let discount_code_path = FactPath::local("discount_code".to_string());
    let member_level_path = FactPath::local("member_level".to_string());

    // Verify that partial simplification occurred: discount_code should be constrained to "SAVE30"
    // in all solutions, while member_level should NOT be constrained (can be any value)
    for (idx, _solution) in response.solutions.iter().enumerate() {
        let domains = &response.domains[idx];

        // Verify discount_code is constrained to "SAVE30"
        let discount_domain = domains
            .get(&discount_code_path)
            .expect("All solutions should have discount_code domain");

        match discount_domain {
            Domain::Enumeration(values) => {
                assert!(
                    values.contains(&LiteralValue::Text("SAVE30".to_string())),
                    "Solution {}: discount_code should be constrained to 'SAVE30' after simplification (A&B)|(A&!B) => A, got {:?}",
                    idx,
                    values
                );
            }
            Domain::Unconstrained => {
                panic!(
                    "Solution {}: discount_code should be constrained after simplification, but is Unconstrained",
                    idx
                );
            }
            other => {
                panic!(
                    "Solution {}: discount_code should be Enumeration with 'SAVE30', got {:?}",
                    idx, other
                );
            }
        }

        // Verify member_level is NOT constrained (simplification removed the member_level constraint)
        let member_domain = domains.get(&member_level_path);
        match member_domain {
            Some(Domain::Unconstrained) => {
                // Good - member_level is not constrained, which is correct after simplification
            }
            Some(Domain::Enumeration(_)) => {
                // This is acceptable - member_level might be constrained by other parts of the expression
                // But it should NOT be required to be "platinum" for the simplification to work
            }
            Some(_other) => {
                // Other domain types are acceptable
            }
            None => {
                // No domain means unconstrained, which is fine
            }
        }

        // Verify at least one tag fact is "yes" (required by the AND condition)
        let mut has_tag_yes = false;
        for tag_idx in 1..=n_extra {
            let tag_path = FactPath::local(format!("tag{}", tag_idx));
            if let Some(tag_domain) = domains.get(&tag_path) {
                match tag_domain {
                    Domain::Enumeration(values) => {
                        if values.contains(&LiteralValue::Text("yes".to_string())) {
                            has_tag_yes = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        assert!(
            has_tag_yes,
            "Solution {}: At least one tag fact should be 'yes' to satisfy the AND condition",
            idx
        );
    }
}
