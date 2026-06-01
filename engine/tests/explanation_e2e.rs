use lemma::ComputationKind;
use lemma::ConversionTraceRole;
use lemma::DateTimeValue;
use lemma::{Engine, LiteralValue, OperationResult, VetoType};
use std::collections::HashMap;

fn conversion_tree(
    tree: &lemma::TraceNode,
) -> (
    ComputationKind,
    &[lemma::ConversionTraceStep],
    &[lemma::TraceNode],
) {
    let lemma::TraceNode::Computation {
        kind,
        conversion_steps,
        expression,
        operands,
        ..
    } = tree
    else {
        panic!("expected Computation at root, got {tree:?}");
    };
    assert!(
        expression.is_empty(),
        "UnitConversion must not use expression field; expression={expression:?}"
    );
    (
        kind.clone(),
        conversion_steps.as_slice(),
        operands.as_slice(),
    )
}

#[test]
fn test_explanation_generated_during_evaluation() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_explanation

data base_value: 100

rule doubled: base_value * 2
"#;

    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test_explanation", Some(&now), HashMap::new(), true)
        .unwrap();

    let doubled_result = response
        .results
        .values()
        .find(|r| r.rule.name == "doubled")
        .expect("doubled rule should exist");

    // Verify result (literal carries resolved LemmaType; compare rendered value)
    assert_eq!(
        doubled_result.display.clone().expect("display"),
        LiteralValue::number(200.into()).to_string(),
    );

    // Verify explanation was built
    let explanation = doubled_result
        .trace
        .as_ref()
        .expect("Explanation should be generated during evaluation");

    assert_eq!(explanation.rule_path.rule, "doubled");
    assert_eq!(
        doubled_result.display.clone().expect("display"),
        LiteralValue::number(200.into()).to_string(),
    );

    // Verify explanation tree structure exists
    match explanation.tree.as_ref() {
        lemma::TraceNode::Computation { .. } => {
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

    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_explanation_ref",
            Some(&now),
            HashMap::new(),
            true,
        )
        .unwrap();

    let quadruple_result = response
        .results
        .values()
        .find(|r| r.rule.name == "quadruple")
        .expect("quadruple rule should exist");

    assert_eq!(
        quadruple_result.display.clone().expect("display"),
        LiteralValue::number(200.into()).to_string(),
    );

    // Verify explanation exists
    let explanation = quadruple_result
        .trace
        .as_ref()
        .expect("Explanation should be generated");

    // Verify explanation tree contains rule reference
    match explanation.tree.as_ref() {
        lemma::TraceNode::Computation {
            operands, result, ..
        } => {
            assert_eq!(
                result.to_string(),
                LiteralValue::number(200.into()).to_string()
            );

            // First operand should be a rule reference to doubled
            match &operands[0] {
                lemma::TraceNode::RuleReference {
                    rule_path,
                    expansion,
                    ..
                } => {
                    assert_eq!(rule_path.rule, "doubled");

                    // Expansion should contain the explanation for doubled
                    match expansion.as_ref() {
                        lemma::TraceNode::Computation { result, .. } => {
                            assert_eq!(
                                result.to_string(),
                                LiteralValue::number(100.into()).to_string()
                            );
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

    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test_unless", Some(&now), HashMap::new(), true)
        .unwrap();

    let discount_result = response
        .results
        .values()
        .find(|r| r.rule.name == "discount_percentage")
        .expect("discount_percentage rule should exist");

    // Verify result - default should match since no unless clauses match
    // 0% is stored as Ratio(0, Some("percent")) to indicate it's a percentage
    let lit = discount_result
        .trace
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value");
    assert_eq!(
        lit.value,
        lemma::LiteralValue::ratio_from_decimal(
            rust_decimal::Decimal::ZERO,
            Some("percent".to_string()),
        )
        .value
    );

    // Verify explanation exists
    let explanation = discount_result
        .trace
        .as_ref()
        .expect("Explanation should be generated");

    // Verify explanation tree shows branches
    match explanation.tree.as_ref() {
        lemma::TraceNode::Branches {
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

    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test_veto", Some(&now), HashMap::new(), true)
        .unwrap();

    let validation_result = response
        .results
        .values()
        .find(|r| r.rule.name == "age_validation")
        .expect("age_validation rule should exist");

    // Verify veto result
    assert!(validation_result.vetoed);
    assert_eq!(
        validation_result.veto_reason.as_deref(),
        Some("Must be 18 or older")
    );

    // Verify explanation exists even for veto
    let explanation = validation_result
        .trace
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
uses base_ref: base
rule result: base_ref.doubled + 50
"#;

    engine
        .load(
            base_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("base.lemma"))),
        )
        .unwrap();
    engine
        .load(
            main_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("main.lemma"))),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "main", Some(&now), HashMap::new(), true)
        .unwrap();

    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");

    assert_eq!(
        result.display.clone().expect("display"),
        LiteralValue::number(250.into()).to_string(),
    );

    // Verify explanation exists
    let explanation = result
        .trace
        .as_ref()
        .expect("Explanation should be generated");

    // Verify explanation tree contains cross-spec rule reference
    match explanation.tree.as_ref() {
        lemma::TraceNode::Computation { operands, .. } => {
            // First operand should be a rule reference to base_ref.doubled
            match &operands[0] {
                lemma::TraceNode::RuleReference {
                    rule_path,
                    expansion,
                    ..
                } => {
                    assert_eq!(rule_path.rule, "doubled");
                    assert_eq!(rule_path.segments.len(), 1);
                    assert_eq!(rule_path.segments[0].data, "base_ref");

                    // Expansion should exist
                    match expansion.as_ref() {
                        lemma::TraceNode::Computation { .. } => {
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
uses base_ref: base
rule use_cross_spec: base_ref.doubled + 1
"#;

    engine
        .load(
            base_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("base.lemma"))),
        )
        .unwrap();
    engine
        .load(
            main_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("main.lemma"))),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "main", Some(&now), HashMap::new(), true)
        .unwrap();

    let main_rule = response
        .results
        .values()
        .find(|r| r.rule.name == "use_cross_spec")
        .expect("use_cross_spec rule should exist");

    let explanation = main_rule.trace.as_ref().expect("Explanation should exist");

    // The main rule's explanation should have empty segments (it's local)
    assert_eq!(explanation.rule_path.rule, "use_cross_spec");
    assert_eq!(
        explanation.rule_path.segments.len(),
        0,
        "Main spec rule should have no segments"
    );

    // Now check the referenced rule's explanation inside the tree
    match explanation.tree.as_ref() {
        lemma::TraceNode::Computation { operands, .. } => {
            match &operands[0] {
                lemma::TraceNode::RuleReference {
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
uses base_ref: base
rule use_doubled: base_ref.doubled + 10
"#;

    engine
        .load(
            base_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("base.lemma"))),
        )
        .unwrap();
    engine
        .load(
            main_spec,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("main.lemma"))),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "main", Some(&now), HashMap::new(), true)
        .unwrap();

    let json_value = serde_json::to_value(&response).expect("Should serialize");
    let json_str = serde_json::to_string_pretty(&response).unwrap();

    let results_obj = json_value["results"].as_object().unwrap();
    let use_doubled_result = results_obj
        .get("use_doubled")
        .expect("use_doubled result not found");
    let explanation = use_doubled_result["explanation"]
        .as_object()
        .expect("explanation should exist when explain=true");
    assert_eq!(explanation["rule"], "use_doubled");

    let tree = &explanation["tree"];
    assert_eq!(tree["type"], "computation");
    let operands = tree["operands"].as_array().unwrap_or_else(|| {
        panic!(
            "Expected operands array in Computation. JSON:\n{}",
            json_str
        )
    });
    assert!(!operands.is_empty());

    let rule_ref = &operands[0];
    assert_eq!(rule_ref["type"], "rule_reference");
    let rule_name = rule_ref["rule"].as_str().expect("rule name");
    assert!(
        rule_name.contains("doubled"),
        "Rule reference should name doubled, got: {rule_name}"
    );
    assert!(
        rule_name.contains("base_ref"),
        "Cross-spec rule reference must preserve path; got: {rule_name}"
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

    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test", Some(&now), HashMap::new(), true)
        .unwrap();

    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "out")
        .expect("out rule should exist");

    assert!(!result.vetoed);
    assert_eq!(result.boolean, Some(true), "default branch is taken");

    let explanation = result.trace.as_ref().expect("explanation should exist");
    let lemma::TraceNode::Branches { non_matched, .. } = explanation.tree.as_ref() else {
        panic!("expected Branches at root, got {:?}", explanation.tree);
    };
    assert_eq!(non_matched.len(), 1, "one unless branch did not match");

    let condition_node = &non_matched[0].condition;
    let lemma::TraceNode::Computation {
        expression,
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
        expression.contains(">="),
        "negated comparison should show >= not <; got expression: {}",
        expression
    );
    assert_eq!(
        cond_result,
        &LiteralValue::from_bool(true),
        "normalized condition should have result true"
    );
}

#[test]
fn date_range_as_days_conversion_steps() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_conversion_trace
uses lemma units
data age: date range -> default 2024-06-01...2024-06-15
rule a: age as days
"#;

    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_conversion_trace",
            Some(&now),
            HashMap::new(),
            true,
        )
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "a")
        .expect("rule a should exist");

    assert_eq!(
        rule_result
            .quantity
            .as_ref()
            .expect("quantity map")
            .get("days")
            .map(String::as_str),
        Some("14"),
        "quantity map must include 14 days"
    );
    let explanation = rule_result
        .trace
        .as_ref()
        .expect("explanation should exist");

    let (kind, steps, operands) = conversion_tree(explanation.tree.as_ref());
    let ComputationKind::UnitConversion { target } = kind else {
        panic!("expected UnitConversion kind, got {kind:?}");
    };
    assert!(
        format!("{target:?}").contains("days"),
        "expected days target, got {target:?}"
    );
    assert!(
        !steps.is_empty(),
        "UnitConversion must have explanation steps"
    );
    assert!(
        steps[0].text.contains("14")
            || (steps[0].text.contains("2") && steps[0].text.contains("week"))
    );
    assert_eq!(operands.len(), 1);
    assert!(matches!(
        &operands[0],
        lemma::TraceNode::Value {
            source: lemma::TraceValueSource::Data { .. },
            ..
        }
    ));
}

#[test]
fn scalar_quantity_conversion_steps() {
    let mut engine = Engine::new();

    let spec = r#"
spec test_quantity_conversion_trace
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> default 2 kilogram
rule result: mass as gram
"#;

    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_quantity_conversion_trace",
            Some(&now),
            HashMap::new(),
            true,
        )
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");

    let explanation = rule_result
        .trace
        .as_ref()
        .expect("explanation should exist");

    let (kind, steps, operands) = conversion_tree(explanation.tree.as_ref());
    assert!(matches!(kind, ComputationKind::UnitConversion { .. }));
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].text, "2 kilogram");
    assert_eq!(steps[1].text, "1 kilogram is 1000 gram");
    assert_eq!(steps[2].text, "The quantity of mass is 2 kilogram");
    assert_eq!(
        steps[2].data_ref.as_ref().map(|path| path.data.as_str()),
        Some("mass")
    );
    for step in steps {
        assert!(!step.text.contains('×'));
    }
    assert_eq!(operands.len(), 1);
}

#[test]
fn duration_scalar_conversion_steps() {
    let mut engine = Engine::new();
    let spec = r#"
spec test_duration_conversion_trace
uses lemma units
data age: units.duration
rule hours: age as hours"#;
    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("age".to_string(), "90 minutes".to_string());
    let response = engine
        .run(
            None,
            "test_duration_conversion_trace",
            Some(&now),
            data,
            true,
        )
        .unwrap();
    let rule_result = response
        .results
        .get("hours")
        .expect("hours rule should exist");
    let explanation = rule_result
        .trace
        .as_ref()
        .expect("explanation should exist");
    let (kind, steps, _) = conversion_tree(explanation.tree.as_ref());
    assert!(matches!(kind, ComputationKind::UnitConversion { .. }));
    assert_eq!(steps.len(), 3);
    assert!(matches!(steps[1].role, ConversionTraceRole::Rule));
    assert!(steps[1].text.contains(" is "));
    assert!(!steps[1].text.contains('×'));
    assert!(
        steps[2].text == "The quantity of age is 90 minutes"
            || steps[2].text == "The quantity of age is 90 minute"
    );
}

#[test]
fn quantity_range_as_hours_conversion_steps() {
    let mut engine = Engine::new();
    let spec = r#"
spec test_quantity_range_conversion_trace
uses lemma units
rule result: (7 days...14 days) as hours"#;
    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_quantity_range_conversion_trace",
            Some(&now),
            HashMap::new(),
            true,
        )
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");
    let explanation = rule_result
        .trace
        .as_ref()
        .expect("explanation should exist");
    let (_, steps, _) = conversion_tree(explanation.tree.as_ref());
    assert_eq!(steps.len(), 3);
    assert!(
        rule_result
            .quantity
            .as_ref()
            .and_then(|m| m.get("hours"))
            .map(String::as_str)
            == Some("168")
    );
    assert!(
        steps[0].text.contains("168")
            || (steps[0].text.contains("1") && steps[0].text.contains("week"))
    );
    assert!(
        steps[1].text.contains("168")
            || steps[1].text.contains("1 week")
            || (steps[1].text.contains("14") && steps[1].text.contains("7"))
    );
    assert!(!steps[1].text.contains(';'));
    assert!(!steps[1].text.contains('×'));
}

#[test]
fn nested_operand_arithmetic_conversion_steps() {
    let mut engine = Engine::new();
    let spec = r#"
spec test_nested_conversion_trace
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> default 2 kilogram
rule result: (mass * 2) as gram"#;
    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_nested_conversion_trace",
            Some(&now),
            HashMap::new(),
            true,
        )
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .expect("result rule should exist");
    let explanation = rule_result
        .trace
        .as_ref()
        .expect("explanation should exist");
    let (_, steps, operands) = conversion_tree(explanation.tree.as_ref());
    assert_eq!(steps.len(), 3);
    assert!(matches!(
        &operands[0],
        lemma::TraceNode::Computation {
            kind: ComputationKind::Arithmetic(_),
            ..
        }
    ));
}

#[test]
fn unless_wraps_unit_conversion_steps() {
    let mut engine = Engine::new();
    let spec = r#"
spec test_unless_conversion_trace
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> default 2 kilogram
data flag: false
rule total: mass as gram as number
  unless flag then 0"#;
    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_unless_conversion_trace",
            Some(&now),
            HashMap::new(),
            true,
        )
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule should exist");
    let explanation = rule_result
        .trace
        .as_ref()
        .expect("explanation should exist");
    let lemma::TraceNode::Branches { matched, .. } = explanation.tree.as_ref() else {
        panic!("expected Branches at root, got {:?}", explanation.tree);
    };
    let (kind, steps, _) = conversion_tree(matched.result.as_ref());
    assert!(
        matches!(kind, ComputationKind::UnitConversion { .. }),
        "expected UnitConversion, got {kind:?}"
    );
    assert!(
        !steps.is_empty(),
        "UnitConversion must have explanation steps"
    );
    assert_eq!(rule_result.display.clone().expect("display"), "2000");
}

#[test]
fn conversion_steps_in_response_json() {
    let mut engine = Engine::new();
    let spec = r#"
spec test_conversion_json
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> default 2 kilogram
rule result: mass as gram"#;
    engine.load(spec, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "test_conversion_json",
            Some(&now),
            HashMap::new(),
            true,
        )
        .unwrap();
    let json_value = serde_json::to_value(&response).expect("serialize response");
    let result = json_value["results"]["result"]
        .as_object()
        .expect("result object");
    let tree = &result["explanation"]["tree"];
    assert_eq!(tree["type"], "conversion");
    let steps = tree["steps"].as_array().expect("conversion steps");
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0]["role"], "outcome");
    assert_eq!(steps[1]["role"], "rule");
    assert_eq!(steps[2]["role"], "source");
}
