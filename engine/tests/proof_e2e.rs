use lemma::{Engine, LiteralValue, OperationResult};
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn test_proof_generated_during_evaluation() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_proof

fact base_value: 100

rule doubled: base_value * 2
"#;

    add_lemma_code_blocking(&mut engine, spec, "test.lemma").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test_proof", None, &now, vec![], HashMap::new())
        .unwrap();

    let doubled_result = response
        .results
        .values()
        .find(|r| r.rule.name == "doubled")
        .expect("doubled rule should exist");

    // Verify result
    assert_eq!(
        doubled_result.result,
        OperationResult::Value(Box::new(LiteralValue::number(200.into())))
    );

    // Verify proof was built
    let proof = doubled_result
        .proof
        .as_ref()
        .expect("Proof should be generated during evaluation");

    assert_eq!(proof.rule_path.rule, "doubled");
    assert_eq!(
        proof.result,
        OperationResult::Value(Box::new(LiteralValue::number(200.into())))
    );

    // Verify proof tree structure exists
    match &proof.tree {
        lemma::proof::ProofNode::Computation { .. } => {
            // Expected: multiplication computation
        }
        other => panic!("Expected Computation node, got {:?}", other),
    }
}

#[test]
fn test_proof_with_rule_reference() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_proof_ref

fact base_value: 50

rule doubled: base_value * 2
rule quadruple: doubled * 2
"#;

    add_lemma_code_blocking(&mut engine, spec, "test.lemma").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test_proof_ref", None, &now, vec![], HashMap::new())
        .unwrap();

    let quadruple_result = response
        .results
        .values()
        .find(|r| r.rule.name == "quadruple")
        .expect("quadruple rule should exist");

    // Verify result
    assert_eq!(
        quadruple_result.result,
        OperationResult::Value(Box::new(LiteralValue::number(200.into())))
    );

    // Verify proof exists
    let proof = quadruple_result
        .proof
        .as_ref()
        .expect("Proof should be generated");

    // Verify proof tree contains rule reference
    match &proof.tree {
        lemma::proof::ProofNode::Computation {
            operands, result, ..
        } => {
            assert_eq!(*result, LiteralValue::number(200.into()));

            // First operand should be a rule reference to doubled
            match &operands[0] {
                lemma::proof::ProofNode::RuleReference {
                    rule_path,
                    expansion,
                    ..
                } => {
                    assert_eq!(rule_path.rule, "doubled");

                    // Expansion should contain the proof for doubled
                    match &**expansion {
                        lemma::proof::ProofNode::Computation { result, .. } => {
                            assert_eq!(*result, LiteralValue::number(100.into()));
                        }
                        other => panic!("Expected Computation in expansion, got {:?}", other),
                    }
                }
                other => panic!("Expected RuleReference for doubled?, got {:?}", other),
            }
        }
        other => panic!("Expected Computation at root, got {:?}", other),
    }
}

#[test]
fn test_proof_with_unless_clauses() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_unless

fact quantity: 5
fact is_premium: false

rule discount_percentage: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 20 then 20%
  unless is_premium then 15%
"#;

    add_lemma_code_blocking(&mut engine, spec, "test.lemma").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test_unless", None, &now, vec![], HashMap::new())
        .unwrap();

    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount_percentage")
        .expect("discount_percentage rule should exist");

    // Verify result - default should match since no unless clauses match
    // 0% is stored as Ratio(0, Some("percent")) to indicate it's a percentage
    assert_eq!(
        discount_result.result,
        OperationResult::Value(Box::new(LiteralValue::ratio(
            Decimal::from(0),
            Some("percent".to_string())
        )))
    );

    // Verify proof exists
    let proof = discount_result
        .proof
        .as_ref()
        .expect("Proof should be generated");

    // Verify proof tree shows branches
    match &proof.tree {
        lemma::proof::ProofNode::Branches {
            matched,
            non_matched,
            ..
        } => {
            // Matched branch should be the default (no condition)
            assert!(
                matched.condition.is_none(),
                "Default branch should have no condition"
            );

            // Should have 3 non-matched unless clauses
            assert_eq!(
                non_matched.len(),
                3,
                "Should have 3 non-matched unless clauses"
            );
        }
        other => panic!(
            "Expected Branches node for rule with unless clauses, got {:?}",
            other
        ),
    }
}

#[test]
fn test_proof_with_veto_result() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_veto

fact age: 17

rule age_validation: accept
  unless age < 18 then veto "Must be 18 or older"
"#;

    add_lemma_code_blocking(&mut engine, spec, "test.lemma").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test_veto", None, &now, vec![], HashMap::new())
        .unwrap();

    let validation_result = response
        .results
        .values()
        .find(|r| r.rule.name == "age_validation")
        .expect("age_validation rule should exist");

    // Verify veto result
    assert_eq!(
        validation_result.result,
        OperationResult::Veto(Some("Must be 18 or older".to_string()))
    );

    // Verify proof exists even for veto
    let proof = validation_result
        .proof
        .as_ref()
        .expect("Proof should be generated even for veto results");

    assert_eq!(proof.rule_path.rule, "age_validation");
    assert_eq!(
        proof.result,
        OperationResult::Veto(Some("Must be 18 or older".to_string()))
    );
}

#[test]
fn test_proof_with_cross_spec_rule_reference() {
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
fact value: 100
rule doubled: value * 2
"#;

    let main_spec = r#"
spec main
fact base_ref: spec base
rule result: base_ref.doubled + 50
"#;

    add_lemma_code_blocking(&mut engine, base_spec, "base.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, main_spec, "main.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("main", None, &now, vec![], HashMap::new())
        .unwrap();

    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");

    // Verify result
    assert_eq!(
        result.result,
        OperationResult::Value(Box::new(LiteralValue::number(250.into())))
    );

    // Verify proof exists
    let proof = result.proof.as_ref().expect("Proof should be generated");

    // Verify proof tree contains cross-spec rule reference
    match &proof.tree {
        lemma::proof::ProofNode::Computation { operands, .. } => {
            // First operand should be a rule reference to base_ref.doubled
            match &operands[0] {
                lemma::proof::ProofNode::RuleReference {
                    rule_path,
                    expansion,
                    ..
                } => {
                    assert_eq!(rule_path.rule, "doubled");
                    assert_eq!(rule_path.segments.len(), 1);
                    assert_eq!(rule_path.segments[0].fact, "base_ref");

                    // Expansion should exist
                    match &**expansion {
                        lemma::proof::ProofNode::Computation { .. } => {
                            // Good - cross-spec rule proof is included
                        }
                        other => panic!(
                            "Expected Computation in cross-spec expansion, got {:?}",
                            other
                        ),
                    }
                }
                other => panic!(
                    "Expected RuleReference for base_ref.doubled?, got {:?}",
                    other
                ),
            }
        }
        other => panic!("Expected Computation at root, got {:?}", other),
    }
}

#[test]
fn test_cross_spec_proof_has_correct_path() {
    // This test specifically validates that proofs stored in context
    // have the correct rule_path including segments
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
fact value: 100
rule doubled: value * 2
"#;

    let main_spec = r#"
spec main
fact base_ref: spec base
rule use_cross_spec: base_ref.doubled + 1
"#;

    add_lemma_code_blocking(&mut engine, base_spec, "base.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, main_spec, "main.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("main", None, &now, vec![], HashMap::new())
        .unwrap();

    let main_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "use_cross_spec")
        .expect("use_cross_spec rule should exist");

    let proof = main_rule.proof.as_ref().expect("Proof should exist");

    // The main rule's proof should have empty segments (it's local)
    assert_eq!(proof.rule_path.rule, "use_cross_spec");
    assert_eq!(
        proof.rule_path.segments.len(),
        0,
        "Main spec rule should have no segments"
    );

    // Now check the referenced rule's proof inside the tree
    match &proof.tree {
        lemma::proof::ProofNode::Computation { operands, .. } => {
            match &operands[0] {
                lemma::proof::ProofNode::RuleReference {
                    rule_path: ref_path,
                    ..
                } => {
                    // CRITICAL: The rule_path in the RuleReference node should have segments
                    assert_eq!(ref_path.rule, "doubled");
                    assert_eq!(
                        ref_path.segments.len(),
                        1,
                        "Cross-spec rule reference MUST have segments showing the path"
                    );
                    assert_eq!(ref_path.segments[0].fact, "base_ref");
                    assert_eq!(ref_path.segments[0].spec, "base");
                }
                other => panic!("Expected RuleReference, got {:?}", other),
            }
        }
        other => panic!("Expected Computation, got {:?}", other),
    }
}

#[test]
fn test_proof_serialization_preserves_cross_spec_paths() {
    // CRITICAL TEST: This catches the bug where Proof.rule_path had empty segments
    // even for cross-spec rules. The buggy code would pass all other tests
    // because they only checked the tree structure, not the top-level Proof metadata.
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
fact value: 50
rule doubled: value * 2
"#;

    let main_spec = r#"
spec main
fact base_ref: spec base
rule use_doubled: base_ref.doubled + 10
"#;

    add_lemma_code_blocking(&mut engine, base_spec, "base.lemma").unwrap();
    add_lemma_code_blocking(&mut engine, main_spec, "main.lemma").unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .evaluate("main", None, &now, vec![], HashMap::new())
        .unwrap();

    let main_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "use_doubled")
        .expect("use_doubled rule should exist");

    let proof = main_rule.proof.as_ref().expect("Proof should exist");

    // Check that the main rule's proof has correct structure
    assert_eq!(proof.rule_path.rule, "use_doubled");
    assert_eq!(proof.rule_path.segments.len(), 0);

    // Now serialize and check the RuleReference path in the JSON
    let json_value = serde_json::to_value(&response).expect("Should serialize");

    // Serialize to JSON for validation
    let json_str = serde_json::to_string_pretty(&response).unwrap();

    // Navigate to the proof for use_doubled -> tree -> operands[0] (the RuleReference)
    // results is now an IndexMap (object), so we need to find the use_doubled rule by key
    let results_obj = json_value["results"].as_object().unwrap();
    let use_doubled_result = results_obj
        .get("use_doubled")
        .expect("use_doubled result not found");
    let proof_tree = &use_doubled_result["proof"]["tree"];

    // The tree should be a `computation` node with operands
    let computation = proof_tree["computation"].as_object().unwrap_or_else(|| {
        panic!(
            "Expected computation node in proof tree. JSON:\n{}",
            json_str
        )
    });

    let operands = computation["operands"].as_array().unwrap_or_else(|| {
        panic!(
            "Expected operands array in Computation. JSON:\n{}",
            json_str
        )
    });

    assert!(
        !operands.is_empty(),
        "Should have at least one operand (the rule reference)"
    );

    let rule_ref_node = &operands[0];

    // The ProofNode is serialized as a tagged enum, so it's {"rule_reference": {...}}
    let rule_ref = rule_ref_node["rule_reference"]
        .as_object()
        .unwrap_or_else(|| {
            panic!(
                "Expected rule_reference variant. Got:\n{}",
                serde_json::to_string_pretty(rule_ref_node).unwrap()
            )
        });

    // This should be the RuleReference to base_ref.doubled
    let rule_ref_path = &rule_ref["rule_path"];
    assert_eq!(
        rule_ref_path["rule"].as_str().unwrap(),
        "doubled",
        "Rule reference should point to 'doubled'"
    );

    // THIS IS THE CRITICAL ASSERTION that would have caught the bug:
    // The segments array should NOT be empty for a cross-spec reference
    let segments = rule_ref_path["segments"].as_array().unwrap_or_else(|| {
        panic!(
            "Expected segments array. Rule ref path JSON:\n{}",
            serde_json::to_string_pretty(rule_ref_path).unwrap()
        )
    });

    assert_eq!(
        segments.len(),
        1,
        "BUG: Cross-spec rule reference MUST have segments! \
         Empty segments means we lost the path information during proof construction."
    );

    assert_eq!(
        segments[0]["fact"].as_str().unwrap(),
        "base_ref",
        "Segment should reference base_ref fact"
    );
    assert_eq!(
        segments[0]["spec"].as_str().unwrap(),
        "base",
        "Segment should reference base spec"
    );
}

#[test]
fn test_comparison_false_normalized_to_positive_in_proof() {
    let mut engine = Engine::new();

    let spec = r#"
spec test
rule out: true
 unless 5 < 3 then false
"#;

    add_lemma_code_blocking(&mut engine, spec, "test.lemma").unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .evaluate("test", None, &now, vec![], HashMap::new())
        .unwrap();

    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "out")
        .expect("out rule should exist");

    assert_eq!(
        result.result,
        OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
        "default branch is taken"
    );

    let proof = result.proof.as_ref().expect("proof should exist");
    let lemma::proof::ProofNode::Branches { non_matched, .. } = &proof.tree else {
        panic!("expected Branches at root, got {:?}", proof.tree);
    };
    assert_eq!(non_matched.len(), 1, "one unless branch did not match");

    let condition_node = &non_matched[0].condition;
    let lemma::proof::ProofNode::Computation {
        original_expression,
        result: cond_result,
        ..
    } = condition_node.as_ref()
    else {
        panic!(
            "expected Computation for condition, got {:?}",
            condition_node
        );
    };

    assert!(
        original_expression.contains(">="),
        "negated comparison should show >= not <; got original_expression: {}",
        original_expression
    );
    assert_eq!(
        cond_result,
        &LiteralValue::from_bool(true),
        "normalized condition should have result true"
    );
}
