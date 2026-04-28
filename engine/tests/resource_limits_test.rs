use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, Error, ResourceLimits};
use std::time::Instant;

#[test]
fn test_file_size_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 100,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);

    // Create a file larger than 100 bytes
    let large_code = "spec test\ndata x: 1\n".repeat(10); // ~200 bytes

    let result = engine.load(&large_code, lemma::SourceType::Labeled("test.lemma"));

    let load_err = result.unwrap_err();
    let limit_err = find_resource_limit_name(&load_err.errors)
        .expect("expected at least one ResourceLimitExceeded");
    assert_eq!(limit_err, "max_file_size_bytes");
}

#[test]
fn expression_exceeding_max_depth_is_rejected() {
    let limits = ResourceLimits {
        max_expression_depth: 5,
        ..ResourceLimits::default()
    };
    // 5 nested parens = depth 6 (1 for rule expr + 5 for parens)
    let code = "spec test\ndata x: 1\nrule r: (((((1 + 1) + 1) + 1) + 1) + 1) + 1";
    let mut engine = Engine::with_limits(limits);
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let limit_err = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for expression depth");
    assert_eq!(limit_err, "max_expression_depth");
}

#[test]
fn expression_depth_error_has_source_location() {
    let limits = ResourceLimits {
        max_expression_depth: 3,
        ..ResourceLimits::default()
    };
    let code = "spec test\ndata x: 1\nrule r: (((1 + 1) + 1) + 1) + 1";
    let mut engine = Engine::with_limits(limits);
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let err = load_err
        .errors
        .iter()
        .find(|e| matches!(e, Error::ResourceLimitExceeded { .. }))
        .expect("expected ResourceLimitExceeded");
    let source = err
        .location()
        .expect("depth error should have source location");
    assert_eq!(source.attribute, "test.lemma");
    assert!(source.span.line > 0, "source line should be set");
}

// --- Expression count limits ---

#[test]
fn expression_count_exceeding_limit_is_rejected() {
    let limits = ResourceLimits {
        max_expression_count: 3,
        ..ResourceLimits::default()
    };
    // a + b + c + d → 7 nodes (4 refs + 3 arithmetic), exceeds 3
    let code = "spec test\ndata a: 1\ndata b: 2\ndata c: 3\ndata d: 4\nrule r: a + b + c + d";
    let mut engine = Engine::with_limits(limits);
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let limit_err = find_resource_limit_name(&load_err.errors)
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
    let code = format!("spec test\ndata x: 1\nrule r: {}", expr);
    let mut engine = Engine::with_limits(limits);
    let result = engine.load(&code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let limit_err = find_resource_limit_name(&load_err.errors)
        .expect("expression count should catch deep sqrt even when depth limit is high");
    assert_eq!(limit_err, "max_expression_count");
}

#[test]
fn expression_count_error_has_source_location() {
    let limits = ResourceLimits {
        max_expression_count: 2,
        ..ResourceLimits::default()
    };
    let code = "spec test\ndata x: 1\nrule r: x + 1 + 2";
    let mut engine = Engine::with_limits(limits);
    let result = engine.load(code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let err = load_err
        .errors
        .iter()
        .find(|e| matches!(e, Error::ResourceLimitExceeded { .. }))
        .expect("expected ResourceLimitExceeded");
    let source = err
        .location()
        .expect("expression count error should have source location");
    assert_eq!(source.attribute, "test.lemma");
}

#[test]
fn test_data_value_size_limit() {
    let limits = ResourceLimits {
        max_data_value_bytes: 50,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);
    engine
        .load(
            "spec test\ndata name: text\nrule result: name",
            lemma::SourceType::Labeled("test.lemma"),
        )
        .unwrap();

    let large_string = "a".repeat(100);
    let mut data = std::collections::HashMap::new();
    data.insert("name".to_string(), large_string);

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), data, false);

    match result {
        Err(Error::ResourceLimitExceeded { ref limit_name, .. }) => {
            assert_eq!(limit_name, "max_data_value_bytes");
        }
        _ => panic!("Expected ResourceLimitExceeded error for large data value"),
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
fn spec_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_SPEC_NAME_LENGTH + 1);
    let code = format!("spec {name}\ndata x: 1");
    let mut engine = Engine::default();
    let result = engine.load(&code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let limit_err = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for spec name");
    assert_eq!(limit_err, "max_spec_name_length");
}

#[test]
fn data_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_DATA_NAME_LENGTH + 1);
    let code = format!("spec test\ndata {name}: 1");
    let mut engine = Engine::default();
    let result = engine.load(&code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let limit_err = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for data name");
    assert_eq!(limit_err, "max_data_name_length");
}

#[test]
fn data_binding_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_DATA_NAME_LENGTH + 1);
    let code = format!("spec test\ndata other.{name}: 1");
    let mut engine = Engine::default();
    let result = engine.load(&code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let limit_err = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for data binding name");
    assert_eq!(limit_err, "max_data_name_length");
}

#[test]
fn rule_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_RULE_NAME_LENGTH + 1);
    let code = format!("spec test\nrule {name}: 1");
    let mut engine = Engine::default();
    let result = engine.load(&code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let limit_err = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for rule name");
    assert_eq!(limit_err, "max_rule_name_length");
}

#[test]
fn data_type_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_DATA_NAME_LENGTH + 1);
    let code = format!("spec test\ndata {name}: number\ndata x: 1");
    let mut engine = Engine::default();
    let result = engine.load(&code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let rle = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for data name");
    assert_eq!(rle, "max_data_name_length");
}

#[test]
fn data_import_name_exceeding_max_length_is_rejected() {
    let name = "a".repeat(lemma::limits::MAX_DATA_NAME_LENGTH + 1);
    let code = format!("spec test\ndata {name} from other\ndata x: 1");
    let mut engine = Engine::default();
    let result = engine.load(&code, lemma::SourceType::Labeled("test.lemma"));
    let load_err = result.unwrap_err();
    let rle = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for data import name");
    assert_eq!(rle, "max_data_name_length");
}

// --- Engine-wide total expression count ---

#[test]
fn total_expression_count_accumulates_across_files() {
    let limits = ResourceLimits {
        max_total_expression_count: 10,
        ..ResourceLimits::default()
    };
    let mut engine = Engine::with_limits(limits);

    // file 1: rule r: a + b → 3 expression nodes (ref a, ref b, arithmetic)
    engine
        .load(
            "spec s1\ndata a: 1\ndata b: 2\nrule r: a + b",
            lemma::SourceType::Labeled("f1.lemma"),
        )
        .expect("first file should succeed");

    // file 2: another 3+ nodes, pushing past the limit of 10
    engine
        .load(
            "spec s2\ndata c: 1\ndata d: 2\nrule r: c + d + c + d",
            lemma::SourceType::Labeled("f2.lemma"),
        )
        .expect("second file should succeed");

    // file 3: should push past the limit
    let result = engine.load(
        "spec s3\ndata e: 1\ndata f: 2\nrule r: e + f + e + f + e + f",
        lemma::SourceType::Labeled("f3.lemma"),
    );
    let load_err = result.unwrap_err();
    let rle = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for total expression count");
    assert_eq!(rle, "max_total_expression_count");
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
    let result = engine.load(
        "spec test\ndata a: 1\nrule r: a + a + a + a",
        lemma::SourceType::Labeled("test.lemma"),
    );
    let load_err = result.unwrap_err();
    let rle = find_resource_limit_name(&load_err.errors)
        .expect("expected ResourceLimitExceeded for total expression count");
    assert_eq!(rle, "max_total_expression_count");
}

/// Scaling test: incremental rule counts to find performance cliffs.
/// Run with: cargo nextest run bench_1m --run-ignored ignored-only --workspace
#[test]
#[ignore]
fn performance_test_10k_rules() {
    use std::collections::HashMap;
    use std::fmt::Write;

    const NODES_PER_RULE: usize = 19;

    fn build_wide_spec(spec_name: &str, num_rules: usize) -> String {
        let mut code = String::with_capacity(num_rules * 60);
        write!(code, "spec {spec_name}\ndata x: 1\n").unwrap();
        for i in 0..num_rules {
            writeln!(code, "rule r_{i}: x + x + x + x + x + x + x + x + x + x").unwrap();
        }
        code
    }

    let num_rules = 10000;
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
        .load(&code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap_or_else(|errs| panic!("{num_rules} rules failed: {:?}", errs));
    let elapsed = start.elapsed();

    let now = DateTimeValue::now();
    let eval_start = Instant::now();
    let resp = engine
        .run("test", Some(&now), HashMap::new(), false)
        .unwrap();
    let eval_time = eval_start.elapsed();

    eprintln!(
        "{num_rules:>6} rules ({nodes:>7} nodes, {bytes:>8} bytes): parse+plan {elapsed:>8.2?}  eval {eval_time:>8.2?}  result={:?}",
        resp.results[0].result
    );

    // Assert that the test takes less than 10 seconds
    assert!(elapsed.as_secs() < 10, "test took too long: {elapsed:?}");
}

/// Scaling test: deep rule dependency chains (linear + binary tree).
/// Run with: cargo nextest run bench_deep_chains --run-ignored only -p lemma-engine
#[test]
#[ignore]
fn bench_deep_chains() {
    const STACK_SIZE: usize = 32 * 1024 * 1024;

    let handle = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(bench_deep_chains_body)
        .expect("spawn bench thread");
    handle.join().expect("bench thread panicked");
}

fn bench_deep_chains_body() {
    use std::collections::HashMap;
    use std::fmt::Write;

    fn build_linear_chain(num_rules: usize) -> String {
        let mut code = String::with_capacity(num_rules * 30);
        write!(code, "spec chain\ndata x: 1\nrule r_0: x\n").unwrap();
        for i in 1..num_rules {
            writeln!(code, "rule r_{i}: r_{} + 1", i - 1).unwrap();
        }
        code
    }

    fn build_binary_tree(depth: u32) -> String {
        let leaves = 1_usize << depth;
        let total_rules = (1 << (depth + 1)) - 1;
        let mut code = String::with_capacity(total_rules * 45);
        write!(code, "spec tree\ndata x: 1\n").unwrap();
        for i in 0..leaves {
            writeln!(code, "rule r_0_{i}: x").unwrap();
        }
        for level in 1..=depth {
            let level_size = 1 << (depth - level);
            for j in 0..level_size {
                let left = 2 * j;
                let right = 2 * j + 1;
                writeln!(
                    code,
                    "rule r_{level}_{j}: r_{}_{left} + r_{}_{right}",
                    level - 1,
                    level - 1
                )
                .unwrap();
            }
        }
        code
    }

    const LINEAR_NODES_PER_RULE: usize = 5;
    const TREE_LEAF_NODES: usize = 2;
    const TREE_INTERNAL_NODES: usize = 5;

    eprintln!("--- Linear chain ---");
    for num_rules in [100, 500, 1_000] {
        let code = build_linear_chain(num_rules);
        let est_nodes = num_rules * LINEAR_NODES_PER_RULE;
        let limits = ResourceLimits {
            max_file_size_bytes: 100 * 1024 * 1024,
            max_expression_count: est_nodes + 1000,
            max_total_expression_count: est_nodes + 1000,
            ..ResourceLimits::default()
        };
        let mut engine = Engine::with_limits(limits);

        let start = Instant::now();
        engine
            .load(&code, lemma::SourceType::Labeled("chain.lemma"))
            .unwrap_or_else(|errs| panic!("linear {num_rules} rules failed: {:?}", errs));
        let elapsed = start.elapsed();

        let now = DateTimeValue::now();
        let eval_start = Instant::now();
        let resp = engine
            .run("chain", Some(&now), HashMap::new(), false)
            .unwrap();
        let eval_time = eval_start.elapsed();

        eprintln!(
            "chain {num_rules:>6} rules (~{est_nodes:>6} nodes): parse+plan {elapsed:>8.2?}  eval {eval_time:>8.2?}  result={:?}",
            resp.results[0].result
        );
    }

    eprintln!("--- Binary tree ---");
    for depth in [6, 8, 10] {
        let leaves = 1_usize << depth;
        let total_rules = (1 << (depth + 1)) - 1;
        let est_nodes = leaves * TREE_LEAF_NODES + (total_rules - leaves) * TREE_INTERNAL_NODES;
        let code = build_binary_tree(depth);
        let limits = ResourceLimits {
            max_file_size_bytes: 100 * 1024 * 1024,
            max_expression_count: est_nodes + 1000,
            max_total_expression_count: est_nodes + 1000,
            ..ResourceLimits::default()
        };
        let mut engine = Engine::with_limits(limits);

        let start = Instant::now();
        engine
            .load(&code, lemma::SourceType::Labeled("tree.lemma"))
            .unwrap_or_else(|errs| panic!("tree depth {depth} failed: {:?}", errs));
        let elapsed = start.elapsed();

        let now = DateTimeValue::now();
        let eval_start = Instant::now();
        let resp = engine
            .run("tree", Some(&now), HashMap::new(), false)
            .unwrap();
        let eval_time = eval_start.elapsed();

        eprintln!(
            "tree  {total_rules:>6} rules (depth {depth:>2}, ~{est_nodes:>6} nodes): parse+plan {elapsed:>8.2?}  eval {eval_time:>8.2?}  result={:?}",
            resp.results[0].result
        );
    }
}
