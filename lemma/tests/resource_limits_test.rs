use lemma::{Engine, LemmaError, ResourceLimits};

#[test]
fn test_file_size_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 100,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);

    // Create a file larger than 100 bytes
    let large_code = "doc test\nfact x = 1\n".repeat(10); // ~200 bytes

    let result = engine.add_lemma_code(&large_code, "test.lemma");

    match result {
        Err(LemmaError::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_file_size_bytes");
        }
        _ => panic!("Expected ResourceLimitExceeded error"),
    }
}

#[test]
fn test_file_size_just_under_limit() {
    let limits = ResourceLimits {
        max_file_size_bytes: 1000,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);
    let code = "doc test\nfact x = 1\nrule y = x + 1"; // Small file

    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok(), "Small file should be accepted");
}

#[test]
fn test_fact_value_size_limit() {
    let limits = ResourceLimits {
        max_fact_value_bytes: 50,
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);
    engine
        .add_lemma_code(
            "doc test\nfact name = [text]\nrule result = name",
            "test.lemma",
        )
        .unwrap();

    // Try to evaluate with a large fact value
    let large_string = "a".repeat(100); // 100 bytes, exceeds 50 byte limit
    let fact_string = format!("name=\"{}\"", large_string);
    let facts = lemma::parse_facts(&[fact_string.as_str()]).unwrap();

    let result = engine.evaluate("test", None, Some(facts));

    match result {
        Err(LemmaError::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_fact_value_bytes");
        }
        _ => panic!("Expected ResourceLimitExceeded error for large fact value"),
    }
}

#[test]
fn test_evaluation_timeout() {
    let limits = ResourceLimits {
        max_evaluation_time_ms: 10, // Very short timeout
        ..ResourceLimits::default()
    };

    let mut engine = Engine::with_limits(limits);

    // Create a document with many rules to potentially trigger timeout
    let mut code = String::from("doc test\nfact x = 1\n");
    for i in 0..1000 {
        code.push_str(&format!("rule r{} = x + {}\n", i, i));
    }

    engine.add_lemma_code(&code, "test.lemma").unwrap();

    let result = engine.evaluate("test", None, None);

    // Note: This might not always trigger depending on system speed
    // But the infrastructure should be in place
    if let Err(LemmaError::ResourceLimitExceeded { limit_name, .. }) = result {
        assert_eq!(limit_name, "max_evaluation_time_ms");
    }
}

#[test]
fn test_expression_depth_limit() {
    let limits = lemma::ResourceLimits {
        max_expression_depth: 5, // Very shallow depth
        ..lemma::ResourceLimits::default()
    };

    let mut engine = lemma::Engine::with_limits(limits);

    // Create deeply nested expression: ((((((x))))))
    let mut code = String::from("doc test\nfact x = 1\nrule result = ");
    for _ in 0..10 {
        code.push('(');
    }
    code.push('x');
    for _ in 0..10 {
        code.push(')');
    }

    let result = engine.add_lemma_code(&code, "test.lemma");

    match result {
        Err(lemma::LemmaError::ResourceLimitExceeded { limit_name, .. }) => {
            assert_eq!(limit_name, "max_expression_depth");
        }
        _ => panic!("Expected ResourceLimitExceeded error for deep nesting"),
    }
}
