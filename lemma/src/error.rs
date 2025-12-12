use crate::parsing::ast::Span;
use crate::Source;
use std::fmt;
use std::sync::Arc;

/// Detailed error information with source location
#[derive(Debug, Clone)]
pub struct ErrorDetails {
    pub message: String,
    pub source: Option<Source>,
    pub source_text: Arc<str>,
    pub doc_start_line: usize,
    pub suggestion: Option<String>,
}

/// Error types for the Lemma system with source location tracking
#[derive(Debug, Clone)]
pub enum LemmaError {
    /// Parse error with source location
    Parse(Box<ErrorDetails>),

    /// Semantic validation error with source location
    Semantic(Box<ErrorDetails>),

    /// Runtime error during evaluation with source location
    Runtime(Box<ErrorDetails>),

    /// Engine error without specific source location
    Engine(String),

    /// Missing fact error during evaluation
    MissingFact(crate::FactPath),

    /// Circular dependency error
    CircularDependency(String),

    /// Resource limit exceeded
    ResourceLimitExceeded {
        limit_name: String,
        limit_value: String,
        actual_value: String,
        suggestion: String,
    },

    /// Multiple errors collected together
    MultipleErrors(Vec<LemmaError>),
}

impl LemmaError {
    /// Create a parse error with source information
    pub fn parse(
        message: impl Into<String>,
        span: Span,
        source_id: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
    ) -> Self {
        Self::Parse(Box::new(ErrorDetails {
            message: message.into(),
            source: Some(Source::new(source_id, span, doc_name)),
            source_text,
            doc_start_line,
            suggestion: None,
        }))
    }

    /// Create a parse error with suggestion
    pub fn parse_with_suggestion(
        message: impl Into<String>,
        span: Span,
        source_id: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::Parse(Box::new(ErrorDetails {
            message: message.into(),
            source: Some(Source::new(source_id, span, doc_name)),
            source_text,
            doc_start_line,
            suggestion: Some(suggestion.into()),
        }))
    }

    /// Create a semantic error with source information
    pub fn semantic(
        message: impl Into<String>,
        span: Span,
        source_id: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
    ) -> Self {
        Self::Semantic(Box::new(ErrorDetails {
            message: message.into(),
            source: Some(Source::new(source_id, span, doc_name)),
            source_text,
            doc_start_line,
            suggestion: None,
        }))
    }

    /// Create a semantic error with suggestion
    pub fn semantic_with_suggestion(
        message: impl Into<String>,
        span: Span,
        source_id: impl Into<String>,
        source_text: Arc<str>,
        doc_name: impl Into<String>,
        doc_start_line: usize,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::Semantic(Box::new(ErrorDetails {
            message: message.into(),
            source: Some(Source::new(source_id, span, doc_name)),
            source_text,
            doc_start_line,
            suggestion: Some(suggestion.into()),
        }))
    }
}

impl fmt::Display for LemmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LemmaError::Parse(details) => {
                write!(f, "Parse error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                if let Some(source) = &details.source {
                    write!(
                        f,
                        " at {}:{}:{}",
                        source.source_id, source.span.line, source.span.col
                    )?;
                }
                Ok(())
            }
            LemmaError::Semantic(details) => {
                write!(f, "Semantic error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                if let Some(source) = &details.source {
                    write!(
                        f,
                        " at {}:{}:{}",
                        source.source_id, source.span.line, source.span.col
                    )?;
                }
                Ok(())
            }
            LemmaError::Runtime(details) => {
                write!(f, "Runtime error: {}", details.message)?;
                if let Some(suggestion) = &details.suggestion {
                    write!(f, " (suggestion: {suggestion})")?;
                }
                if let Some(source) = &details.source {
                    write!(
                        f,
                        " at {}:{}:{}",
                        source.source_id, source.span.line, source.span.col
                    )?;
                }
                Ok(())
            }
            LemmaError::Engine(msg) => write!(f, "Engine error: {msg}"),
            LemmaError::MissingFact(fact_ref) => write!(f, "Missing fact: {fact_ref}"),
            LemmaError::CircularDependency(msg) => write!(f, "Circular dependency: {msg}"),
            LemmaError::ResourceLimitExceeded {
                limit_name,
                limit_value,
                actual_value,
                suggestion,
            } => {
                write!(
                    f,
                    "Resource limit exceeded: {limit_name} (limit: {limit_value}, actual: {actual_value}). {suggestion}"
                )
            }
            LemmaError::MultipleErrors(errors) => {
                writeln!(f, "Multiple errors:")?;
                for (i, error) in errors.iter().enumerate() {
                    write!(f, "  {}. {error}", i + 1)?;
                    if i < errors.len() - 1 {
                        writeln!(f)?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LemmaError {}

impl From<std::fmt::Error> for LemmaError {
    fn from(err: std::fmt::Error) -> Self {
        LemmaError::Engine(format!("Format error: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::Span;
    use std::sync::Arc;

    fn create_test_error(
        variant: fn(String, Span, String, Arc<str>, String, usize) -> LemmaError,
    ) -> LemmaError {
        let source_text = "fact amount = 100";
        let span = Span {
            start: 14,
            end: 21,
            line: 1,
            col: 15,
        };
        variant(
            "Invalid currency".to_string(),
            span,
            "test.lemma".to_string(),
            Arc::from(source_text),
            "test_doc".to_string(),
            1,
        )
    }

    #[test]
    fn test_error_creation_and_display() {
        let parse_error = create_test_error(LemmaError::parse);
        let parse_error_display = format!("{parse_error}");
        assert!(parse_error_display.contains("Parse error: Invalid currency"));
        assert!(parse_error_display.contains("test.lemma:1:15"));

        let semantic_error = create_test_error(LemmaError::semantic);
        let semantic_error_display = format!("{semantic_error}");
        assert!(semantic_error_display.contains("Semantic error: Invalid currency"));
        assert!(semantic_error_display.contains("test.lemma:1:15"));

        let source_text = "fact amont = 100";
        let span = Span {
            start: 5,
            end: 10,
            line: 1,
            col: 6,
        };
        let parse_error_with_suggestion = LemmaError::parse_with_suggestion(
            "Typo in fact name",
            span.clone(),
            "suggestion.lemma",
            Arc::from(source_text),
            "suggestion_doc",
            1,
            "Did you mean 'amount'?",
        );
        let parse_error_with_suggestion_display = format!("{parse_error_with_suggestion}");
        assert!(parse_error_with_suggestion_display.contains("Typo in fact name"));
        assert!(parse_error_with_suggestion_display.contains("Did you mean 'amount'?"));

        let semantic_error_with_suggestion = LemmaError::semantic_with_suggestion(
            "Incompatible types",
            span.clone(),
            "suggestion.lemma",
            Arc::from(source_text),
            "suggestion_doc",
            1,
            "Try converting one of the types.",
        );
        let semantic_error_with_suggestion_display = format!("{semantic_error_with_suggestion}");
        assert!(semantic_error_with_suggestion_display.contains("Incompatible types"));
        assert!(semantic_error_with_suggestion_display.contains("Try converting one of the types."));

        let engine_error = LemmaError::Engine("Something went wrong".to_string());
        assert_eq!(
            format!("{engine_error}"),
            "Engine error: Something went wrong"
        );

        let circular_dependency_error = LemmaError::CircularDependency("a -> b -> a".to_string());
        assert_eq!(
            format!("{circular_dependency_error}"),
            "Circular dependency: a -> b -> a"
        );

        let multiple_errors =
            LemmaError::MultipleErrors(vec![parse_error, semantic_error, engine_error]);
        let multiple_errors_display = format!("{multiple_errors}");
        assert!(multiple_errors_display.contains("Multiple errors:"));
        assert!(multiple_errors_display.contains("Parse error: Invalid currency"));
        assert!(multiple_errors_display.contains("Semantic error: Invalid currency"));
        assert!(multiple_errors_display.contains("Engine error: Something went wrong"));
    }

    #[test]
    fn test_duplicate_fact_definition_error() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc test
        fact salary = 50000
        fact salary = 60000
    "#,
            "test.lemma",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(
                    msg.to_lowercase().contains("duplicate") && msg.to_lowercase().contains("fact"),
                    "Error should mention duplicate fact, got: {}",
                    msg
                );
                assert!(
                    msg.contains("salary"),
                    "Error should mention fact name, got: {}",
                    msg
                );
            }
            Err(e) => panic!("Expected Engine error for duplicate fact, got: {e:?}"),
            Ok(_) => panic!("Expected error for duplicate fact"),
        }
    }

    #[test]
    fn test_duplicate_rule_definition_error() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc test
        fact x = 10
        rule total = x * 2
        rule total = x * 3
    "#,
            "test.lemma",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(
                    msg.to_lowercase().contains("duplicate") && msg.to_lowercase().contains("rule"),
                    "Error should mention duplicate rule, got: {}",
                    msg
                );
                assert!(
                    msg.contains("total"),
                    "Error should mention rule name, got: {}",
                    msg
                );
            }
            Err(e) => panic!("Expected Engine error for duplicate rule, got: {e:?}"),
            Ok(_) => panic!("Expected error for duplicate rule"),
        }
    }

    #[test]
    fn test_duplicate_fact_shows_name() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc test
        fact name = "Alice"
        fact age = 30
        fact name = "Bob"
    "#,
            "test.lemma",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(
                    msg.contains("Duplicate"),
                    "Error should mention duplicate, got: {}",
                    msg
                );
                assert!(
                    msg.contains("name"),
                    "Error should mention fact name, got: {}",
                    msg
                );
            }
            Err(e) => panic!("Expected Engine error for duplicate fact, got: {e:?}"),
            Ok(_) => panic!("Expected error for duplicate fact"),
        }
    }

    #[test]
    fn test_parse_error_with_span() {
        let result = crate::parse(
            r#"
        doc test
        fact name = "Unclosed string
        fact age = 25
    "#,
            Some("test.lemma".to_string()),
            &crate::ResourceLimits::default(),
        );

        match result {
            Err(LemmaError::Parse(details)) => {
                let source = details.source.as_ref().expect("should have source");
                assert_eq!(source.source_id, "test.lemma");
                assert_eq!(source.doc_name, "<parse-error>");
            }
            Err(e) => panic!("Expected Parse error, got: {e:?}"),
            Ok(_) => panic!("Expected parse error for unclosed string"),
        }
    }

    #[test]
    fn test_parse_error_malformed_input() {
        let result = crate::parse(
            r#"
        doc test
        this is not valid lemma syntax @#$%
    "#,
            Some("test.lemma".to_string()),
            &crate::ResourceLimits::default(),
        );

        assert!(result.is_err(), "Should fail on malformed input");

        match result {
            Err(LemmaError::Parse { .. }) => {
                // Expected
            }
            Err(e) => panic!("Expected Parse error, got: {e:?}"),
            Ok(_) => panic!("Expected parse error"),
        }
    }

    #[test]
    fn test_circular_dependency_has_helpful_suggestion() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc test
        rule x = y?
        rule y = x?
    "#,
            "test.lemma",
        );

        match result {
            Err(LemmaError::CircularDependency(msg)) => {
                assert!(
                    msg.to_lowercase().contains("circular") || msg.to_lowercase().contains("cycle")
                );
                assert!(msg.contains("x") && msg.contains("y"));
            }
            Err(e) => panic!("Expected CircularDependency error, got: {e:?}"),
            Ok(_) => panic!("Expected error for circular dependency"),
        }
    }

    #[test]
    fn test_duplicate_error_contains_fact_name() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc my_document
        fact price = 100
        fact price = 200
    "#,
            "my_file.lemma",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(msg.contains("Duplicate"), "Error should mention duplicate");
                assert!(msg.contains("price"), "Error should mention fact name");
            }
            Err(e) => panic!("Expected Engine error, got: {e:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_duplicate_error_is_reported() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc test
        fact x = 10
        fact x = 20
    "#,
            "test.lemma",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(msg.contains("Duplicate"), "Error should mention duplicate");
                assert!(msg.contains("x"), "Error should mention fact name");
            }
            Err(e) => panic!("Expected Engine error, got: {e:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_duplicate_in_second_doc_is_caught() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc first_doc
        fact a = 1

        doc second_doc
        fact b = 2
        fact b = 3
    "#,
            "multi.lemma",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(msg.contains("Duplicate"), "Error should mention duplicate");
                assert!(msg.contains("b"), "Error should mention fact name");
            }
            Err(e) => panic!("Expected Engine error, got: {e:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_error_display_contains_duplicate_info() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc test
        fact value = 100
        fact value = 200
    "#,
            "test.lemma",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(msg.contains("Duplicate"), "Error should mention duplicate");
                assert!(msg.contains("value"), "Error should mention fact name");
            }
            Err(e) => panic!("Expected Engine error, got: {e:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_duplicate_fact_is_detected() {
        let mut engine = crate::Engine::new();

        let lemma_code = r#"doc test
fact line2 = 1
fact line3 = 2
fact line4 = 3
fact line4 = 4"#;

        let result = engine.add_lemma_code(lemma_code, "test.lemma");

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(msg.contains("Duplicate"), "Error should mention duplicate");
                assert!(
                    msg.contains("line4"),
                    "Error should mention the duplicated fact name"
                );
            }
            Err(e) => panic!("Expected Engine error, got: {e:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_duplicate_detected_from_database_source() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc contract
        fact amount = 1000
        fact amount = 2000
    "#,
            "db://contracts/123",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(msg.contains("Duplicate"), "Error should mention duplicate");
                assert!(msg.contains("amount"), "Error should mention fact name");
            }
            Err(e) => panic!("Expected Engine error, got: {e:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_duplicate_detected_from_api_source() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc policy
        rule rate = 1.5
        rule rate = 2.0
    "#,
            "api://policies/endpoint",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(msg.contains("Duplicate"), "Error should mention duplicate");
                assert!(msg.contains("rate"), "Error should mention rule name");
            }
            Err(e) => panic!("Expected Engine error, got: {e:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_duplicate_detected_from_runtime_source() {
        let mut engine = crate::Engine::new();

        let result = engine.add_lemma_code(
            r#"
        doc runtime_doc
        fact x = 5
        fact x = 10
    "#,
            "<runtime>",
        );

        match result {
            Err(LemmaError::Engine(msg)) => {
                assert!(msg.contains("Duplicate"), "Error should mention duplicate");
                assert!(msg.contains("x"), "Error should mention fact name");
            }
            Err(e) => panic!("Expected Engine error, got: {e:?}"),
            Ok(_) => panic!("Expected error"),
        }
    }
}
