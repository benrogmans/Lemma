use lemma::{Engine, TraceStep};

#[test]
fn test_trace_simple_rule() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact price = 100
        rule total = price * 2
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    assert_eq!(response.results.len(), 1);
    let result = &response.results[0];

    assert!(!result.trace.is_empty(), "Trace should not be empty");

    let has_fact_used = result
        .trace
        .iter()
        .any(|step| matches!(step, TraceStep::FactUsed { name, .. } if name == "price"));
    assert!(has_fact_used, "Trace should contain fact_used for price");

    let has_default_value = result
        .trace
        .iter()
        .any(|step| matches!(step, TraceStep::DefaultValue { .. }));
    assert!(has_default_value, "Trace should contain default_value");

    let has_final_result = result
        .trace
        .iter()
        .any(|step| matches!(step, TraceStep::FinalResult { .. }));
    assert!(has_final_result, "Trace should contain final_result");
}

#[test]
fn test_trace_unless_clauses() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact quantity = 5
        fact is_premium = true

        rule discount = 0%
          unless quantity >= 10 then 10%
          unless quantity >= 20 then 15%
          unless is_premium then 20%
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    assert_eq!(response.results.len(), 1);
    let result = &response.results[0];

    let unless_steps: Vec<_> = result
        .trace
        .iter()
        .filter_map(|step| {
            if let TraceStep::UnlessClauseEvaluated {
                index,
                matched,
                result_if_matched,
            } = step
            {
                Some((*index, *matched, result_if_matched.clone()))
            } else {
                None
            }
        })
        .collect();

    // With reversed evaluation, we only trace what we actually evaluate
    // We evaluate in reverse: clause 2 (is_premium) matches, so we stop there
    assert_eq!(
        unless_steps.len(),
        1,
        "Should have 1 unless clause evaluation (stopped at first match)"
    );

    assert_eq!(
        unless_steps[0].0, 2,
        "Last unless clause index (is_premium)"
    );
    assert_eq!(unless_steps[0].1, true, "is_premium should be true");
    assert!(
        unless_steps[0].2.is_some(),
        "Matching unless should have result value"
    );
}

#[test]
fn test_trace_with_rule_reference() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact base = 100
        rule double = base * 2
        rule quadruple = double? * 2
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    let quadruple_result = response
        .results
        .iter()
        .find(|r| r.rule_name == "quadruple")
        .expect("Should have quadruple result");

    let has_rule_used = quadruple_result
        .trace
        .iter()
        .any(|step| matches!(step, TraceStep::RuleUsed { name, .. } if name == "double"));
    assert!(has_rule_used, "Trace should contain rule_used for double");
}

#[test]
fn test_trace_last_matching_unless_wins() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact quantity = 15

        rule discount = 0%
          unless quantity >= 10 then 10%
          unless quantity >= 20 then 20%
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    let result = &response.results[0];

    let unless_steps: Vec<_> = result
        .trace
        .iter()
        .filter_map(|step| {
            if let TraceStep::UnlessClauseEvaluated { index, matched, .. } = step {
                Some((*index, *matched))
            } else {
                None
            }
        })
        .collect();

    // With reversed evaluation, we evaluate from the end
    // Clause 1 (quantity >= 20) doesn't match (15 >= 20 is false)
    // Clause 0 (quantity >= 10) matches (15 >= 10 is true), so we return
    assert_eq!(unless_steps.len(), 2, "Should evaluate 2 clauses");
    assert_eq!(
        unless_steps[0],
        (1, false),
        "Last clause evaluated first (quantity >= 20 should be false)"
    );
    assert_eq!(
        unless_steps[1],
        (0, true),
        "Second-to-last clause evaluated second (quantity >= 10 should match)"
    );

    if let Some(TraceStep::FinalResult { value }) = result.trace.last() {
        assert_eq!(
            format!("{}", value),
            "10%",
            "Final result should be 10% (last matching unless wins)"
        );
    } else {
        panic!("Expected final_result in trace");
    }
}

#[test]
fn test_trace_captures_actual_values() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact price = 42.50 USD
        rule display = price
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    let result = &response.results[0];

    let has_fact_used = result
        .trace
        .iter()
        .any(|step| matches!(step, TraceStep::FactUsed { name, .. } if name == "price"));
    assert!(has_fact_used, "Trace should contain fact_used for price");

    let has_default_value = result
        .trace
        .iter()
        .any(|step| matches!(step, TraceStep::DefaultValue { .. }));
    assert!(has_default_value, "Trace should contain default_value");

    let has_final_result = result
        .trace
        .iter()
        .any(|step| matches!(step, TraceStep::FinalResult { .. }));
    assert!(has_final_result, "Trace should contain final_result");
}

#[test]
fn test_trace_arithmetic_operation() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact x = 10
        fact y = 20
        rule sum = x + y
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    let result = &response.results[0];

    let operation_steps: Vec<_> = result
        .trace
        .iter()
        .filter_map(|step| {
            if let TraceStep::OperationExecuted {
                operation,
                inputs,
                result,
                ..
            } = step
            {
                Some((operation.clone(), inputs.clone(), result.clone()))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(operation_steps.len(), 1, "Should have 1 operation");
    assert_eq!(operation_steps[0].0, "add");
    assert_eq!(operation_steps[0].1.len(), 2);
    assert_eq!(format!("{:?}", operation_steps[0].2), "Number(30)");
}

#[test]
fn test_trace_comparison_operation() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact value = 42
        rule is_positive = value > 0
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    let result = &response.results[0];

    let operation_steps: Vec<_> = result
        .trace
        .iter()
        .filter_map(|step| {
            if let TraceStep::OperationExecuted {
                operation,
                inputs,
                result,
                ..
            } = step
            {
                Some((operation.clone(), inputs.clone(), result.clone()))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        operation_steps.len(),
        1,
        "Should have 1 comparison operation"
    );
    assert_eq!(operation_steps[0].0, "greater_than");
    assert_eq!(format!("{:?}", operation_steps[0].2), "Boolean(true)");
}

#[test]
fn test_trace_multiple_operations() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact a = 10
        fact b = 5
        fact c = 2
        rule result = (a + b) * c
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    let result = &response.results[0];

    let operation_steps: Vec<_> = result
        .trace
        .iter()
        .filter_map(|step| {
            if let TraceStep::OperationExecuted { operation, .. } = step {
                Some(operation.clone())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        operation_steps.len(),
        2,
        "Should have 2 operations (add and multiply)"
    );
    assert_eq!(operation_steps[0], "add");
    assert_eq!(operation_steps[1], "multiply");
}

#[test]
fn test_trace_operations_with_unless() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact x = 10
        rule result = 0
          unless x > 5 then x * 2
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    let result = &response.results[0];

    let operation_steps: Vec<_> = result
        .trace
        .iter()
        .filter_map(|step| {
            if let TraceStep::OperationExecuted { operation, .. } = step {
                Some(operation.clone())
            } else {
                None
            }
        })
        .collect();

    assert!(
        operation_steps.len() >= 2,
        "Should have at least 2 operations (comparison and multiply)"
    );
    assert!(operation_steps.contains(&"greater_than".to_string()));
    assert!(operation_steps.contains(&"multiply".to_string()));
}

#[test]
fn test_trace_logical_operations() {
    let mut engine = Engine::new();

    let lemma_code = r#"
        doc test
        fact x = 10
        fact y = 20
        rule both_positive = (x > 0) and (y > 0)
    "#;

    engine.add_lemma_code(lemma_code, "test.lemma").unwrap();
    let response = engine.evaluate("test", vec![]).unwrap();

    let result = &response.results[0];

    let operation_steps: Vec<_> = result
        .trace
        .iter()
        .filter_map(|step| {
            if let TraceStep::OperationExecuted { operation, .. } = step {
                Some(operation.clone())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        operation_steps.len(),
        2,
        "Should have 2 comparison operations"
    );
    assert_eq!(operation_steps[0], "greater_than");
    assert_eq!(operation_steps[1], "greater_than");
}
