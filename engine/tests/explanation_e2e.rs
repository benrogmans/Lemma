use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, LiteralValue, OperationResult, VetoType};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn test_explanation_generated_during_evaluation() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_explanation

data base_value: 100

rule doubled: base_value * 2
"#;

    engine
        .load(spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run("test_explanation", Some(&now), HashMap::new(), false)
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

    // Verify explanation was built
    let explanation = doubled_result
        .explanation
        .as_ref()
        .expect("Explanation should be generated during evaluation");

    assert_eq!(explanation.rule_path.rule, "doubled");
    assert_eq!(
        explanation.result,
        OperationResult::Value(Box::new(LiteralValue::number(200.into())))
    );

    // Verify explanation tree structure exists
    match explanation.tree.as_ref() {
        lemma::explanation::ExplanationNode::Computation { .. } => {
            // Expected: multiplication computation
        }
        other => panic!("Expected Computation node, got {:?}", other),
    }
}

#[test]
fn test_explanation_with_rule_reference() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_explanation_ref

data base_value: 50

rule doubled: base_value * 2
rule quadruple: doubled * 2
"#;

    engine
        .load(spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run("test_explanation_ref", Some(&now), HashMap::new(), false)
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

    // Verify explanation exists
    let explanation = quadruple_result
        .explanation
        .as_ref()
        .expect("Explanation should be generated");

    // Verify explanation tree contains rule reference
    match explanation.tree.as_ref() {
        lemma::explanation::ExplanationNode::Computation {
            operands, result, ..
        } => {
            assert_eq!(*result, LiteralValue::number(200.into()));

            // First operand should be a rule reference to doubled
            match &operands[0] {
                lemma::explanation::ExplanationNode::RuleReference {
                    rule_path,
                    expansion,
                    ..
                } => {
                    assert_eq!(rule_path.rule, "doubled");

                    // Expansion should contain the explanation for doubled
                    match expansion.as_ref() {
                        lemma::explanation::ExplanationNode::Computation { result, .. } => {
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
fn test_explanation_with_unless_clauses() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_unless

data quantity: 5
data is_premium: false

rule discount_percentage: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 20 then 20%
  unless is_premium then 15%
"#;

    engine
        .load(spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run("test_unless", Some(&now), HashMap::new(), false)
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

    // Verify explanation exists
    let explanation = discount_result
        .explanation
        .as_ref()
        .expect("Explanation should be generated");

    // Verify explanation tree shows branches
    match explanation.tree.as_ref() {
        lemma::explanation::ExplanationNode::Branches {
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
fn test_explanation_with_veto_result() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_veto

data age: 17

rule age_validation: accept
  unless age < 18 then veto "Must be 18 or older"
"#;

    engine
        .load(spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run("test_veto", Some(&now), HashMap::new(), false)
        .unwrap();

    let validation_result = response
        .results
        .values()
        .find(|r| r.rule.name == "age_validation")
        .expect("age_validation rule should exist");

    // Verify veto result
    assert_eq!(
        validation_result.result,
        OperationResult::Veto(VetoType::UserDefined {
            message: Some("Must be 18 or older".to_string()),
        })
    );

    // Verify explanation exists even for veto
    let explanation = validation_result
        .explanation
        .as_ref()
        .expect("Explanation should be generated even for veto results");

    assert_eq!(explanation.rule_path.rule, "age_validation");
    assert_eq!(
        explanation.result,
        OperationResult::Veto(VetoType::UserDefined {
            message: Some("Must be 18 or older".to_string()),
        })
    );
}

#[test]
fn test_explanation_with_cross_spec_rule_reference() {
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
data value: 100
rule doubled: value * 2
"#;

    let main_spec = r#"
spec main
with base_ref: base
rule result: base_ref.doubled + 50
"#;

    engine
        .load(base_spec, lemma::SourceType::Labeled("base.lemma"))
        .unwrap();
    engine
        .load(main_spec, lemma::SourceType::Labeled("main.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("main", Some(&now), HashMap::new(), false)
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

    // Verify explanation exists
    let explanation = result
        .explanation
        .as_ref()
        .expect("Explanation should be generated");

    // Verify explanation tree contains cross-spec rule reference
    match explanation.tree.as_ref() {
        lemma::explanation::ExplanationNode::Computation { operands, .. } => {
            // First operand should be a rule reference to base_ref.doubled
            match &operands[0] {
                lemma::explanation::ExplanationNode::RuleReference {
                    rule_path,
                    expansion,
                    ..
                } => {
                    assert_eq!(rule_path.rule, "doubled");
                    assert_eq!(rule_path.segments.len(), 1);
                    assert_eq!(rule_path.segments[0].data, "base_ref");

                    // Expansion should exist
                    match expansion.as_ref() {
                        lemma::explanation::ExplanationNode::Computation { .. } => {
                            // Good - cross-spec rule explanation is included
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
fn test_cross_spec_explanation_has_correct_path() {
    // This test specifically validates that explanations stored in context
    // have the correct rule_path including segments
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
data value: 100
rule doubled: value * 2
"#;

    let main_spec = r#"
spec main
with base_ref: base
rule use_cross_spec: base_ref.doubled + 1
"#;

    engine
        .load(base_spec, lemma::SourceType::Labeled("base.lemma"))
        .unwrap();
    engine
        .load(main_spec, lemma::SourceType::Labeled("main.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("main", Some(&now), HashMap::new(), false)
        .unwrap();

    let main_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "use_cross_spec")
        .expect("use_cross_spec rule should exist");

    let explanation = main_rule
        .explanation
        .as_ref()
        .expect("Explanation should exist");

    // The main rule's explanation should have empty segments (it's local)
    assert_eq!(explanation.rule_path.rule, "use_cross_spec");
    assert_eq!(
        explanation.rule_path.segments.len(),
        0,
        "Main spec rule should have no segments"
    );

    // Now check the referenced rule's explanation inside the tree
    match explanation.tree.as_ref() {
        lemma::explanation::ExplanationNode::Computation { operands, .. } => {
            match &operands[0] {
                lemma::explanation::ExplanationNode::RuleReference {
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
                    assert_eq!(ref_path.segments[0].data, "base_ref");
                    assert_eq!(ref_path.segments[0].spec, "base");
                }
                other => panic!("Expected RuleReference, got {:?}", other),
            }
        }
        other => panic!("Expected Computation, got {:?}", other),
    }
}

#[test]
fn test_explanation_serialization_preserves_cross_spec_paths() {
    // CRITICAL TEST: This catches the bug where Explanation.rule_path had empty segments
    // even for cross-spec rules. The buggy code would pass all other tests
    // because they only checked the tree structure, not the top-level Explanation metadata.
    let mut engine = Engine::new();

    let base_spec = r#"
spec base
data value: 50
rule doubled: value * 2
"#;

    let main_spec = r#"
spec main
with base_ref: base
rule use_doubled: base_ref.doubled + 10
"#;

    engine
        .load(base_spec, lemma::SourceType::Labeled("base.lemma"))
        .unwrap();
    engine
        .load(main_spec, lemma::SourceType::Labeled("main.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run("main", Some(&now), HashMap::new(), false)
        .unwrap();

    let main_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "use_doubled")
        .expect("use_doubled rule should exist");

    let explanation = main_rule
        .explanation
        .as_ref()
        .expect("Explanation should exist");

    // Check that the main rule's explanation has correct structure
    assert_eq!(explanation.rule_path.rule, "use_doubled");
    assert_eq!(explanation.rule_path.segments.len(), 0);

    // Now serialize and check the RuleReference path in the JSON
    let json_value = serde_json::to_value(&response).expect("Should serialize");

    // Serialize to JSON for validation
    let json_str = serde_json::to_string_pretty(&response).unwrap();

    // Navigate to the explanation for use_doubled -> tree -> operands[0] (the RuleReference)
    // results is now an IndexMap (object), so we need to find the use_doubled rule by key
    let results_obj = json_value["results"].as_object().unwrap();
    let use_doubled_result = results_obj
        .get("use_doubled")
        .expect("use_doubled result not found");
    let explanation_tree = &use_doubled_result["explanation"]["tree"];

    // The tree should be a `computation` node with operands
    let computation = explanation_tree["computation"]
        .as_object()
        .unwrap_or_else(|| {
            panic!(
                "Expected computation node in explanation tree. JSON:\n{}",
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

    // The ExplanationNode is serialized as a tagged enum, so it's {"rule_reference": {...}}
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
         Empty segments means we lost the path information during explanation construction."
    );

    assert_eq!(
        segments[0]["data"].as_str().unwrap(),
        "base_ref",
        "Segment should reference base_ref data"
    );
    assert_eq!(
        segments[0]["spec"].as_str().unwrap(),
        "base",
        "Segment should reference base spec"
    );
}

#[test]
fn test_comparison_false_normalized_to_positive_in_explanation() {
    let mut engine = Engine::new();

    let spec = r#"
spec test
rule out: true
 unless 5 < 3 then false
"#;

    engine
        .load(spec, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run("test", Some(&now), HashMap::new(), false)
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

    let explanation = result
        .explanation
        .as_ref()
        .expect("explanation should exist");
    let lemma::explanation::ExplanationNode::Branches { non_matched, .. } =
        explanation.tree.as_ref()
    else {
        panic!("expected Branches at root, got {:?}", explanation.tree);
    };
    assert_eq!(non_matched.len(), 1, "one unless branch did not match");

    let condition_node = &non_matched[0].condition;
    let lemma::explanation::ExplanationNode::Computation {
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
