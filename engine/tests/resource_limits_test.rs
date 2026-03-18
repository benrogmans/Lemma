use lemma::{Engine, Error, ResourceLimits};
mod common;
use common::add_lemma_code_blocking;
use lemma::parsing::ast::DateTimeValue;
use std::time::Instant;

#[test]
fn test_file_size_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 100,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);

    // Create a file larger than 100 bytes
    let large_code = "spec test\nfact x: 1\n".repeat(10); // ~200 bytes

    let result = add_lemma_code_blocking(&mut engine, &large_code, "test.lemma");

    let errs = result.unwrap_err();
    let limit_err =
        find_resource_limit_name(&errs).expect("expected at least one ResourceLimitExceeded");
    assert_eq!(limit_err, "max_file_size_bytes");
}

#[test]
fn test_file_size_just_under_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 1000,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);
    let code = "spec test fact x: 1 rule y: x + 1"; // Small file

    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(result.is_ok(), "Small file should be accepted");
}

#[test]
fn test_expression_depth_limit() {
    let limits = ResourceLimits::default();
    assert_eq!(limits.max_expression_depth, 7);

    let mut engine = Engine::with_limits(limits);
    let code_4 = r#"spec test
fact x: 1
rule r: (((1 + 1) + 1) + 1) + 1"#;
    let result = add_lemma_code_blocking(&mut engine, code_4, "test.lemma");
    assert!(
        result.is_ok(),
        "Depth 4 should be accepted: {:?}",
        result.err()
    );
}

#[test]
fn expression_at_max_depth_is_accepted() {
    let limits = ResourceLimits {
        max_expression_depth: 5,
        ..ResourceLimits::default()
    };
    // 4 nested parens = depth 5 (1 for rule expr + 4 for parens)
    let code = "spec test\nfact x: 1\nrule r: ((((1 + 1) + 1) + 1) + 1) + 1";
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Depth 5 (at limit) should be accepted: {:?}",
        result.err()
    );
}

#[test]
fn expression_exceeding_max_depth_is_rejected() {
    let limits = ResourceLimits {
        max_expression_depth: 5,
        ..ResourceLimits::default()
    };
    // 5 nested parens = depth 6 (1 for rule expr + 5 for parens)
    let code = "spec test\nfact x: 1\nrule r: (((((1 + 1) + 1) + 1) + 1) + 1) + 1";
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err = find_resource_limit_name(&errs)
        .expect("expected ResourceLimitExceeded for expression depth");
    assert_eq!(limit_err, "max_expression_depth");
}

#[test]
fn expression_depth_error_has_source_location() {
    let limits = ResourceLimits {
        max_expression_depth: 3,
        ..ResourceLimits::default()
    };
    let code = "spec test\nfact x: 1\nrule r: (((1 + 1) + 1) + 1) + 1";
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    let errs = result.unwrap_err();
    let err = errs
        .iter()
        .find(|e| matches!(e, Error::ResourceLimitExceeded { .. }))
        .expect("expected ResourceLimitExceeded");
    let source = err
        .location()
        .expect("depth error should have source location");
    assert_eq!(source.attribute, "test.lemma");
    assert!(source.span.line > 0, "source line should be set");
}

#[test]
fn unless_paren_nesting_counts_toward_depth() {
    let limits = ResourceLimits {
        max_expression_depth: 2,
        ..ResourceLimits::default()
    };
    // condition: ((x + 1) + 2) > 3
    // parse_expression depth 1 → ( depth 2 → ( depth 3 → EXCEEDS
    let code = "spec test\nfact x: 1\nrule r: 0 unless ((x + 1) + 2) > 3 then 1";
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    let errs = result.unwrap_err();
    assert!(
        find_resource_limit_name(&errs).is_some(),
        "Double-nested paren in unless should exceed depth 2: {:?}",
        errs
    );
}

#[test]
fn single_paren_in_unless_within_depth_limit() {
    let limits = ResourceLimits {
        max_expression_depth: 2,
        ..ResourceLimits::default()
    };
    // condition: (x + 1) > 2  →  depth 1 → ( depth 2 → ok at limit
    let code = "spec test\nfact x: 1\nrule r: 0 unless (x + 1) > 2 then 1";
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "Single paren in unless at depth 2 should be ok: {:?}",
        result.err()
    );
}

// --- Expression count limits ---

#[test]
fn expression_count_within_limit_is_accepted() {
    let limits = ResourceLimits {
        max_expression_count: 10,
        ..ResourceLimits::default()
    };
    // rule r: x + 1 → 3 nodes (ref x, literal 1, arithmetic)
    let code = "spec test\nfact x: 1\nrule r: x + 1";
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    assert!(
        result.is_ok(),
        "3 nodes should be under limit of 10: {:?}",
        result.err()
    );
}

#[test]
fn expression_count_exceeding_limit_is_rejected() {
    let limits = ResourceLimits {
        max_expression_count: 3,
        ..ResourceLimits::default()
    };
    // a + b + c + d → 7 nodes (4 refs + 3 arithmetic), exceeds 3
    let code = "spec test\nfact a: 1\nfact b: 2\nfact c: 3\nfact d: 4\nrule r: a + b + c + d";
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err = find_resource_limit_name(&errs)
        .expect("expected ResourceLimitExceeded for expression count");
    assert_eq!(limit_err, "max_expression_count");
}

#[test]
fn expression_count_catches_deep_sqrt_without_depth_guard() {
    let limits = ResourceLimits {
        max_expression_count: 20,
        max_expression_depth: 1000, // intentionally high — rely on count
        ..ResourceLimits::default()
    };
    let mut expr = String::from("1");
    for _ in 0..50 {
        expr = format!("sqrt {}", expr);
    }
    let code = format!("spec test\nfact x: 1\nrule r: {}", expr);
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err = find_resource_limit_name(&errs)
        .expect("expression count should catch deep sqrt even when depth limit is high");
    assert_eq!(limit_err, "max_expression_count");
}

#[test]
fn expression_count_error_has_source_location() {
    let limits = ResourceLimits {
        max_expression_count: 2,
        ..ResourceLimits::default()
    };
    let code = "spec test\nfact x: 1\nrule r: x + 1 + 2";
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, code, "test.lemma");
    let errs = result.unwrap_err();
    let err = errs
        .iter()
        .find(|e| matches!(e, Error::ResourceLimitExceeded { .. }))
        .expect("expected ResourceLimitExceeded");
    let source = err
        .location()
        .expect("expression count error should have source location");
    assert_eq!(source.attribute, "test.lemma");
}

#[test]
fn bench_deep_nesting_performance() {
    use std::collections::HashMap;

    fn build_nested_parens(depth: usize) -> String {
        let mut expr = String::from("1");
        for _ in 0..depth {
            expr = format!("({} + 1)", expr);
        }
        format!("spec test\nfact x: 1\nrule r: {}", expr)
    }

    // depth 100 overflows parse/plan stack (recursive planner); eval is fine
    for depth in [10, 25, 50, 75] {
        let code = build_nested_parens(depth);
        let limits = ResourceLimits {
            max_expression_depth: depth + 5,
            max_expression_count: depth * 10,
            ..ResourceLimits::default()
        };
        let mut engine = Engine::with_limits(limits);

        let start = Instant::now();
        add_lemma_code_blocking(&mut engine, &code, "test.lemma")
            .unwrap_or_else(|e| panic!("depth {} failed to parse+plan: {:?}", depth, e));
        let parse_plan = start.elapsed();

        let now = DateTimeValue::now();
        eprintln!("depth {:>3}: parse+plan {:>8.2?}", depth, parse_plan);

        let eval_start = Instant::now();
        let resp = engine
            .run("test", Some(&now), HashMap::new())
            .unwrap_or_else(|e| panic!("depth {} failed to evaluate: {}", depth, e));
        let eval = eval_start.elapsed();

        eprintln!(
            "depth {:>3}: eval {:>8.2?}  result={:?}",
            depth, eval, resp.results[0].result
        );
    }
}

#[test]
fn test_overall_execution_time_at_expression_depth_limit() {
    let limits = ResourceLimits::default();
    let code_4 = r#"spec test
fact x: 1
rule r: (((1 + 1) + 1) + 1) + 1"#;
    let mut engine = Engine::with_limits(limits);
    let start = Instant::now();
    add_lemma_code_blocking(&mut engine, code_4, "test.lemma").expect("load");
    let now = DateTimeValue::now();
    let _ = engine
        .run("test", Some(&now), std::collections::HashMap::new())
        .expect("evaluate");
    let elapsed = start.elapsed();
    eprintln!("overall (parse + plan + evaluate, depth 4): {:?}", elapsed);
}

#[test]
fn test_fact_value_size_limit() {
    let limits = ResourceLimits {
        max_fact_value_bytes: 50,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);
    add_lemma_code_blocking(
        &mut engine,
        "spec test\nfact name: [text]\nrule result: name",
        "test.lemma",
    )
    .unwrap();

    let large_string = "a".repeat(100);
    let mut facts = std::collections::HashMap::new();
    facts.insert("name".to_string(), large_string);

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), facts);

    match result {
        Err(Error::ResourceLimitExceeded { ref limit_name, .. }) => {
            assert_eq!(limit_name, "max_fact_value_bytes");
        }
        _ => panic!("Expected ResourceLimitExceeded error for large fact value"),
    }
}

// --- Name length limits ---

/// Helper to extract the `limit_name` from the first `ResourceLimitExceeded` in a list of errors.
fn find_resource_limit_name(errors: &[Error]) -> Option<String> {
    errors.iter().find_map(|e| match e {
        Error::ResourceLimitExceeded { limit_name, .. } => Some(limit_name.clone()),
        _ => None,
    })
}

#[test]
fn spec_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_SPEC_NAME_LENGTH);
    let code = format!("spec {name}\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Spec name at max length should be accepted: {result:?}"
    );
}

#[test]
fn spec_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_SPEC_NAME_LENGTH + 1);
    let code = format!("spec {name}\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err =
        find_resource_limit_name(&errs).expect("expected ResourceLimitExceeded for spec name");
    assert_eq!(limit_err, "max_spec_name_length");
}

#[test]
fn fact_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH);
    let code = format!("spec test\nfact {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Fact name at max length should be accepted: {result:?}"
    );
}

#[test]
fn fact_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH + 1);
    let code = format!("spec test\nfact {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err =
        find_resource_limit_name(&errs).expect("expected ResourceLimitExceeded for fact name");
    assert_eq!(limit_err, "max_fact_name_length");
}

#[test]
fn fact_binding_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_FACT_NAME_LENGTH + 1);
    let code = format!("spec test\nfact other.{name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err = find_resource_limit_name(&errs)
        .expect("expected ResourceLimitExceeded for fact binding name");
    assert_eq!(limit_err, "max_fact_name_length");
}

#[test]
fn rule_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_RULE_NAME_LENGTH);
    let code = format!("spec test\nrule {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Rule name at max length should be accepted: {result:?}"
    );
}

#[test]
fn rule_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_RULE_NAME_LENGTH + 1);
    let code = format!("spec test\nrule {name}: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let limit_err =
        find_resource_limit_name(&errs).expect("expected ResourceLimitExceeded for rule name");
    assert_eq!(limit_err, "max_rule_name_length");
}

#[test]
fn type_name_at_max_length_is_accepted() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH);
    let code = format!("spec test\ntype {name}: number\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_ok(),
        "Type name at max length should be accepted: {result:?}"
    );
}

#[test]
fn type_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH + 1);
    let code = format!("spec test\ntype {name}: number\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let rle =
        find_resource_limit_name(&errs).expect("expected ResourceLimitExceeded for type name");
    assert_eq!(rle, "max_type_name_length");
}

#[test]
fn deeply_nested_math_functions_are_bounded() {
    let limits = ResourceLimits {
        max_expression_depth: 5,
        ..ResourceLimits::default()
    };
    // sqrt sqrt sqrt ... 1 recursion bypasses parse_expression depth tracking
    let mut expr = String::from("1");
    for _ in 0..200 {
        expr = format!("sqrt {}", expr);
    }
    let code = format!("spec test\nfact x: 1\nrule r: {}", expr);
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(result.is_err(), "200 nested sqrt should be rejected");
}

#[test]
fn deeply_nested_power_operators_are_bounded() {
    let limits = ResourceLimits {
        max_expression_depth: 5,
        ..ResourceLimits::default()
    };
    // 1 ^ 1 ^ 1 ^ ... ^ 1 — right-associative recursion in parse_power
    let parts: Vec<&str> = std::iter::repeat_n("1", 200).collect();
    let expr = parts.join(" ^ ");
    let code = format!("spec test\nfact x: 1\nrule r: {}", expr);
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(result.is_err(), "200 chained ^ should be rejected");
}

#[test]
fn deeply_nested_not_operators_are_bounded() {
    let limits = ResourceLimits {
        max_expression_depth: 5,
        ..ResourceLimits::default()
    };
    let mut expr = String::from("true");
    for _ in 0..200 {
        expr = format!("not {}", expr);
    }
    let code = format!("spec test\nfact x: 1\nrule r: {}", expr);
    let mut engine = Engine::with_limits(limits);
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    assert!(
        result.is_err(),
        "200 nested not should be rejected, not cause stack overflow"
    );
}

#[test]
fn type_import_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_TYPE_NAME_LENGTH + 1);
    let code = format!("spec test\ntype {name} from other\nfact x: 1");
    let mut engine = Engine::default();
    let result = add_lemma_code_blocking(&mut engine, &code, "test.lemma");
    let errs = result.unwrap_err();
    let rle = find_resource_limit_name(&errs)
        .expect("expected ResourceLimitExceeded for type import name");
    assert_eq!(rle, "max_type_name_length");
}

// --- Engine-wide total expression count ---

#[test]
fn default_total_expression_count_is_pi() {
    let limits = ResourceLimits::default();
    assert_eq!(limits.max_total_expression_count, 3_141_592);
}

#[test]
fn total_expression_count_accumulates_across_files() {
    let limits = ResourceLimits {
        max_total_expression_count: 10,
        ..ResourceLimits::default()
    };
    let mut engine = Engine::with_limits(limits);

    // file 1: rule r: a + b → 3 expression nodes (ref a, ref b, arithmetic)
    add_lemma_code_blocking(
        &mut engine,
        "spec s1\nfact a: 1\nfact b: 2\nrule r: a + b",
        "f1.lemma",
    )
    .expect("first file should succeed");

    // file 2: another 3+ nodes, pushing past the limit of 10
    add_lemma_code_blocking(
        &mut engine,
        "spec s2\nfact c: 1\nfact d: 2\nrule r: c + d + c + d",
        "f2.lemma",
    )
    .expect("second file should succeed");

    // file 3: should push past the limit
    let result = add_lemma_code_blocking(
        &mut engine,
        "spec s3\nfact e: 1\nfact f: 2\nrule r: e + f + e + f + e + f",
        "f3.lemma",
    );
    let errs = result.unwrap_err();
    let rle = find_resource_limit_name(&errs)
        .expect("expected ResourceLimitExceeded for total expression count");
    assert_eq!(rle, "max_total_expression_count");
}

#[test]
fn total_expression_count_within_limit_succeeds() {
    let limits = ResourceLimits {
        max_total_expression_count: 100,
        ..ResourceLimits::default()
    };
    let mut engine = Engine::with_limits(limits);

    add_lemma_code_blocking(&mut engine, "spec s1\nfact x: 1\nrule r: x + 1", "f1.lemma")
        .expect("first file should succeed");
    add_lemma_code_blocking(&mut engine, "spec s2\nfact y: 2\nrule r: y * 3", "f2.lemma")
        .expect("second file should succeed");
}

#[test]
fn single_file_exceeding_total_expression_count_is_rejected() {
    let limits = ResourceLimits {
        max_total_expression_count: 3,
        max_expression_count: 4096,
        ..ResourceLimits::default()
    };
    let mut engine = Engine::with_limits(limits);

    // a + b + c + d → 7 nodes, exceeds total limit of 3
    let result = add_lemma_code_blocking(
        &mut engine,
        "spec test\nfact a: 1\nrule r: a + a + a + a",
        "test.lemma",
    );
    let errs = result.unwrap_err();
    let rle = find_resource_limit_name(&errs)
        .expect("expected ResourceLimitExceeded for total expression count");
    assert_eq!(rle, "max_total_expression_count");
}

/// Scaling test: incremental rule counts to find performance cliffs.
/// Run with: cargo nextest run bench_1m --run-ignored ignored-only --workspace
#[test]
#[ignore]
fn bench_1m_expressions() {
    use std::collections::HashMap;
    use std::fmt::Write;

    const NODES_PER_RULE: usize = 19;

    fn build_wide_spec(spec_name: &str, num_rules: usize) -> String {
        let mut code = String::with_capacity(num_rules * 60);
        write!(code, "spec {spec_name}\nfact x: 1\n").unwrap();
        for i in 0..num_rules {
            writeln!(code, "rule r_{i}: x + x + x + x + x + x + x + x + x + x").unwrap();
        }
        code
    }

    for num_rules in [100, 1_000, 5_000, 10_000, 25_000, 52_631] {
        let nodes = num_rules * NODES_PER_RULE;
        let code = build_wide_spec("test", num_rules);
        let bytes = code.len();
        let limits = ResourceLimits {
            max_file_size_bytes: 100 * 1024 * 1024,
            max_expression_count: nodes + 1000,
            max_total_expression_count: nodes + 1000,
            ..ResourceLimits::default()
        };
        let mut engine = Engine::with_limits(limits);

        let start = Instant::now();
        engine
            .load(&code, lemma::LoadSource::Labeled("test.lemma"))
            .unwrap_or_else(|errs| panic!("{num_rules} rules failed: {:?}", errs));
        let elapsed = start.elapsed();

        let now = DateTimeValue::now();
        let eval_start = Instant::now();
        let resp = engine.run("test", Some(&now), HashMap::new()).unwrap();
        let eval_time = eval_start.elapsed();

        eprintln!(
            "{num_rules:>6} rules ({nodes:>7} nodes, {bytes:>8} bytes): parse+plan {elapsed:>8.2?}  eval {eval_time:>8.2?}  result={:?}",
            resp.results[0].result
        );
    }
}
