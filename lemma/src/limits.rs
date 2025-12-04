/// Limits to prevent abuse and enable predictable resource usage
///
/// These limits protect against malicious inputs while being generous enough
/// for all legitimate use cases.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum file size in bytes
    /// Real usage: ~5KB, Limit: 5MB (1000x)
    pub max_file_size_bytes: usize,

    /// Maximum expression nesting depth
    /// Real usage: ~3 levels, Limit: 100 (30x+)
    pub max_expression_depth: usize,

    /// Maximum size of a single fact value in bytes
    /// Real usage: ~100 bytes, Limit: 1KB (10x)
    /// Enables server pre-allocation for zero-allocation evaluation
    pub max_fact_value_bytes: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_file_size_bytes: 5 * 1024 * 1024, // 5 MB
            max_expression_depth: 100,
            max_fact_value_bytes: 1024, // 1 KB
        }
    }
}

impl ResourceLimits {
    /// Create a new ResourceLimits with default values
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, LemmaError};

    #[test]
    fn test_file_size_limit() {
        let limits = ResourceLimits {
            max_file_size_bytes: 100,
            ..ResourceLimits::default()
        };

        let mut engine = Engine::with_limits(limits);

        let large_code = "doc test\nfact x = 1\n".repeat(10);

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
        let code = "doc test\nfact x = 1\nrule y = x + 1";

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

        let large_string = "a".repeat(100);
        let mut facts = std::collections::HashMap::new();
        facts.insert("name".to_string(), large_string);

        let result = engine.evaluate("test", vec![], facts);

        match result {
            Err(LemmaError::ResourceLimitExceeded { limit_name, .. }) => {
                assert_eq!(limit_name, "max_fact_value_bytes");
            }
            _ => panic!("Expected ResourceLimitExceeded error for large fact value"),
        }
    }

    #[test]
    fn test_expression_depth_limit() {
        let limits = ResourceLimits {
            max_expression_depth: 5,
            ..ResourceLimits::default()
        };

        let mut engine = Engine::with_limits(limits);

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
            Err(LemmaError::ResourceLimitExceeded { limit_name, .. }) => {
                assert_eq!(limit_name, "max_expression_depth");
            }
            _ => panic!("Expected ResourceLimitExceeded error for deep nesting"),
        }
    }
}
