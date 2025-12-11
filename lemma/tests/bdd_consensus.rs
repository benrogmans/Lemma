use lemma::{FactRuleConstraint, Engine, FactPath, LiteralValue, Target};
use std::collections::HashMap;

#[test]
fn bdd_consensus_rule_simplifies_three_terms_to_two() {
    // A := discount_code is "SAVE30"
    // B := member_level is "platinum"
    // C := solution is "EU"
    // (A & B) | (!A & C) | (B & C) => (A & B) | (!A & C)
    // The third branch (B & C) is redundant because:
    // - If A is true, then (A & B) covers (B & C) when A is true
    // - If A is false, then (!A & C) covers (B & C) when A is false
    // So we should get exactly 2 solutions, not 3
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
            Target::value(LiteralValue::number(1)),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // BDD consensus theorem should simplify the three branches to two
    // (A & B) | (!A & C) | (B & C) => (A & B) | (!A & C)
    // The third branch (B & C) is redundant and should be eliminated
    assert_eq!(
        response.solutions.len(),
        2,
        "BDD consensus simplification should reduce 3 branches to 2 solutions. Got {} solutions. This indicates the simplification is not working correctly.",
        response.solutions.len()
    );

    // Verify each solution has the correct domain constraints
    let discount_code_path = FactPath::local("discount_code".to_string());
    let member_level_path = FactPath::local("member_level".to_string());
    let solution_path = FactPath::local("solution".to_string());

    // Find solution 1: (A & B) = discount_code == "SAVE30" AND member_level == "platinum"
    let solution1 = response
        .solutions
        .iter()
        .enumerate()
        .find(|(idx, _)| {
            let domains = &response.domains[*idx];
            let discount_domain = domains.get(&discount_code_path);
            let member_domain = domains.get(&member_level_path);
            matches!(
                (discount_domain, member_domain),
                (
                    Some(FactRuleConstraint::Enumeration(discount_vals)),
                    Some(FactRuleConstraint::Enumeration(member_vals))
                ) if discount_vals.contains(&LiteralValue::Text("SAVE30".to_string()))
                    && member_vals.contains(&LiteralValue::Text("platinum".to_string()))
            )
        })
        .expect("Should have solution with discount_code='SAVE30' and member_level='platinum'");

    // Verify solution 1 domains
    let solution1_domains = &response.domains[solution1.0];
    let solution1_discount = solution1_domains
        .get(&discount_code_path)
        .expect("Solution 1 should have discount_code domain");
    let solution1_member = solution1_domains
        .get(&member_level_path)
        .expect("Solution 1 should have member_level domain");

    match solution1_discount {
        FactRuleConstraint::Enumeration(values) => {
            assert!(
                values.contains(&LiteralValue::Text("SAVE30".to_string())),
                "Solution 1: discount_code should be 'SAVE30', got {:?}",
                values
            );
        }
        other => panic!(
            "Solution 1: discount_code should be Enumeration, got {:?}",
            other
        ),
    }

    match solution1_member {
        FactRuleConstraint::Enumeration(values) => {
            assert!(
                values.contains(&LiteralValue::Text("platinum".to_string())),
                "Solution 1: member_level should be 'platinum', got {:?}",
                values
            );
        }
        other => panic!(
            "Solution 1: member_level should be Enumeration, got {:?}",
            other
        ),
    }

    // Find solution 2: (!A & C) = discount_code != "SAVE30" AND solution == "EU"
    // This should be a solution with solution="EU" and discount_code != "SAVE30"
    // It should NOT be solution 1 (A & B) which has discount_code="SAVE30"
    let solution2 = response
        .solutions
        .iter()
        .enumerate()
        .find(|(idx, _)| {
            let domains = &response.domains[*idx];
            let discount_domain = domains.get(&discount_code_path);
            let solution_domain = domains.get(&solution_path);
            
            // Must have solution == "EU"
            let has_solution_eu = matches!(
                solution_domain,
                Some(FactRuleConstraint::Enumeration(solution_vals)) if solution_vals.contains(&LiteralValue::Text("EU".to_string()))
            );
            
            if !has_solution_eu {
                return false;
            }
            
            // Must have discount_code != "SAVE30"
            // This can be: Complement(Enumeration(["SAVE30"])), Unconstrained, or Enumeration without "SAVE30"
            let discount_not_save30 = match discount_domain {
                Some(FactRuleConstraint::Enumeration(discount_vals)) => {
                    !discount_vals.contains(&LiteralValue::Text("SAVE30".to_string()))
                }
                Some(FactRuleConstraint::Complement(inner)) => {
                    // Complement(Enumeration(["SAVE30"])) means != "SAVE30"
                    match inner.as_ref() {
                        FactRuleConstraint::Enumeration(vals) => vals.contains(&LiteralValue::Text("SAVE30".to_string())),
                        _ => true,
                    }
                }
                Some(FactRuleConstraint::Unconstrained) => true,
                _ => false,
            };
            
            if !discount_not_save30 {
                return false;
            }
            
            // Exclude solution 1: (A & B) which has discount_code == "SAVE30"
            let is_not_solution1 = !matches!(
                discount_domain,
                Some(FactRuleConstraint::Enumeration(discount_vals)) if discount_vals.contains(&LiteralValue::Text("SAVE30".to_string()))
            );
            
            is_not_solution1
        });
    
    let solution2 = solution2.expect("Should have solution with solution='EU' and discount_code != 'SAVE30'");

    // Verify solution 2 domains
    let solution2_domains = &response.domains[solution2.0];
    let solution2_discount = solution2_domains
        .get(&discount_code_path)
        .expect("Solution 2 should have discount_code domain");
    let solution2_solution = solution2_domains
        .get(&solution_path)
        .expect("Solution 2 should have solution domain");

    match solution2_discount {
        FactRuleConstraint::Enumeration(values) => {
            assert!(
                !values.contains(&LiteralValue::Text("SAVE30".to_string())),
                "Solution 2: discount_code should NOT be 'SAVE30', got {:?}",
                values
            );
        }
        FactRuleConstraint::Complement(inner) => {
            // Complement means NOT the inner domain - verify it excludes "SAVE30"
            match inner.as_ref() {
                FactRuleConstraint::Enumeration(vals) => {
                    // If the complement is Enumeration(["SAVE30"]), that means discount_code != "SAVE30" ✓
                    // If it contains other values, we need to check
                    if vals.contains(&LiteralValue::Text("SAVE30".to_string())) {
                        // Good - complement of ["SAVE30"] means != "SAVE30"
                    } else {
                        // Complement of other values - this is still acceptable
                    }
                }
                _ => {
                    // Other complement types are acceptable
                }
            }
        }
        FactRuleConstraint::Unconstrained => {
            // Unconstrained is acceptable - it means any value, which includes values != "SAVE30"
        }
        other => panic!(
            "Solution 2: discount_code should be Enumeration, Complement, or Unconstrained, got {:?}",
            other
        ),
    }

    match solution2_solution {
        FactRuleConstraint::Enumeration(values) => {
            assert!(
                values.contains(&LiteralValue::Text("EU".to_string())),
                "Solution 2: solution should be 'EU', got {:?}",
                values
            );
        }
        other => panic!(
            "Solution 2: solution should be Enumeration, got {:?}",
            other
        ),
    }

    // Check if branch 3 (member_level == "platinum" AND solution == "EU") exists as a separate solution
    // Branch 3: (B & C) = member_level="platinum" AND solution="EU"
    // This is distinct from:
    // - Solution 1 (A & B): discount_code="SAVE30" AND member_level="platinum"
    // - Solution 2 (!A & C): solution="EU" AND discount_code != "SAVE30"
    // If simplification occurred, branch 3 should NOT exist as a separate solution
    let branch3_solution = response
        .solutions
        .iter()
        .enumerate()
        .find(|(idx, _)| {
            let domains = &response.domains[*idx];
            let member_domain = domains.get(&member_level_path);
            let solution_domain = domains.get(&solution_path);
            let discount_domain = domains.get(&discount_code_path);
            
            // Branch 3: member_level="platinum" AND solution="EU"
            // Must NOT be solution 1 (has discount_code="SAVE30")
            // Must NOT be solution 2 (has discount_code != "SAVE30" explicitly)
            let has_member_platinum = matches!(
                member_domain,
                Some(FactRuleConstraint::Enumeration(member_vals)) if member_vals.contains(&LiteralValue::Text("platinum".to_string()))
            );
            
            let has_solution_eu = matches!(
                solution_domain,
                Some(FactRuleConstraint::Enumeration(solution_vals)) if solution_vals.contains(&LiteralValue::Text("EU".to_string()))
            );
            
            // Exclude solution 1: has discount_code="SAVE30"
            let is_not_solution1 = !matches!(
                discount_domain,
                Some(FactRuleConstraint::Enumeration(discount_vals)) if discount_vals.contains(&LiteralValue::Text("SAVE30".to_string()))
            );
            
            // Exclude solution 2: has discount_code != "SAVE30" (Complement or explicit Enumeration without "SAVE30")
            let is_not_solution2 = match discount_domain {
                Some(FactRuleConstraint::Complement(_)) => false, // Complement means != "SAVE30", so this is solution 2
                Some(FactRuleConstraint::Enumeration(discount_vals)) => {
                    discount_vals.contains(&LiteralValue::Text("SAVE30".to_string())) // If contains "SAVE30", it's solution 1, not solution 2
                }
                _ => true, // Unconstrained or other - not solution 2
            };
            
            has_member_platinum && has_solution_eu && is_not_solution1 && is_not_solution2
        });

    if let Some((idx, _)) = branch3_solution {
        // Branch 3 exists - verify it's correct
        let branch3_domains = &response.domains[idx];
        let branch3_member = branch3_domains
            .get(&member_level_path)
            .expect("Branch 3 should have member_level domain");
        let branch3_solution_domain = branch3_domains
            .get(&solution_path)
            .expect("Branch 3 should have solution domain");
        // discount_code might be unconstrained (not in domains map) or explicitly set
        let branch3_discount = branch3_domains.get(&discount_code_path);

        match branch3_member {
            FactRuleConstraint::Enumeration(values) => {
                assert!(
                    values.contains(&LiteralValue::Text("platinum".to_string())),
                    "Branch 3: member_level should be 'platinum', got {:?}",
                    values
                );
            }
            other => panic!("Branch 3: member_level should be Enumeration, got {:?}", other),
        }

        match branch3_solution_domain {
            FactRuleConstraint::Enumeration(values) => {
                assert!(
                    values.contains(&LiteralValue::Text("EU".to_string())),
                    "Branch 3: solution should be 'EU', got {:?}",
                    values
                );
            }
            other => panic!("Branch 3: solution should be Enumeration, got {:?}", other),
        }

        match branch3_discount {
            Some(FactRuleConstraint::Enumeration(values)) => {
                // discount_code can be anything for branch 3, but shouldn't be "SAVE30" (that would be solution 1)
                if values.contains(&LiteralValue::Text("SAVE30".to_string())) {
                    panic!("Branch 3: discount_code should NOT be 'SAVE30' (that would be solution 1), got {:?}", values);
                }
            }
            Some(FactRuleConstraint::Unconstrained) | None => {
                // Unconstrained (or not in domains) is acceptable for branch 3
                // This means discount_code can be any value
            }
            Some(_other) => {
                // Other domain types are acceptable
            }
        }

        // Branch 3 should NOT exist - if it does, simplification failed
        panic!(
            "BDD consensus simplification failed: Branch 3 (member_level='platinum' AND solution='EU') exists as a separate solution at index {}. This redundant branch should have been eliminated by simplification. FactRuleConstraints: {:?}",
            idx, branch3_domains
        );
    }
    
    // If we reach here, simplification worked correctly (exactly 2 solutions, branch 3 eliminated)
}
