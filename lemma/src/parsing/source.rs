use crate::parsing::ast::Span;

/// Unified source location information
///
/// Combines source file identifier, span, and document name
/// for consistent source location tracking across the codebase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Source {
    /// Source file identifier (e.g., filename or database identifier)
    pub source_id: String,

    /// Span in source code (uses Lemma's existing `Span` type from `crate::ast::Span`)
    pub span: Span,

    /// Document name (the Lemma document containing this code)
    pub doc_name: String,
}

impl Source {
    /// Create a new Source
    #[must_use]
    pub fn new(source_id: impl Into<String>, span: Span, doc_name: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            span,
            doc_name: doc_name.into(),
        }
    }

    /// Get the source text for this location from the given source_text map
    ///
    /// Returns an error if the source is not found in the map or the span is out of bounds.
    pub fn get_text(
        &self,
        source_text: &std::collections::HashMap<String, (String, String)>,
    ) -> crate::LemmaResult<String> {
        let (stored_source_id, text) = source_text
            .get(&self.doc_name)
            .ok_or_else(|| {
                crate::LemmaError::Engine(format!(
                    "bug: document '{}' not found in source_text map - evaluation context is missing source",
                    self.doc_name
                ))
            })?;

        if stored_source_id != &self.source_id {
            return Err(crate::LemmaError::Engine(format!(
                "bug: source_id mismatch for document '{}' - expected '{}', found '{}'",
                self.doc_name, self.source_id, stored_source_id
            )));
        }

        let bytes = text.as_bytes();
        if self.span.start < bytes.len() && self.span.end <= bytes.len() {
            Ok(String::from_utf8_lossy(&bytes[self.span.start..self.span.end]).to_string())
        } else {
            Err(crate::LemmaError::Engine(format!(
                "bug: span {:?} is out of bounds for source (length: {}) - invalid span calculation",
                self.span, bytes.len()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_text_valid() {
        let mut source_map = std::collections::HashMap::new();
        source_map.insert("test".to_string(), ("test.lemma".to_string(), "hello world".to_string()));
        let span = Span {
            start: 0,
            end: 5,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.get_text(&source_map).unwrap(), "hello".to_string());
    }

    #[test]
    fn test_get_text_middle() {
        let mut source_map = std::collections::HashMap::new();
        source_map.insert("test".to_string(), ("test.lemma".to_string(), "hello world".to_string()));
        let span = Span {
            start: 6,
            end: 11,
            line: 1,
            col: 6,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.get_text(&source_map).unwrap(), "world".to_string());
    }

    #[test]
    fn test_get_text_full_string() {
        let mut source_map = std::collections::HashMap::new();
        source_map.insert("test".to_string(), ("test.lemma".to_string(), "hello world".to_string()));
        let span = Span {
            start: 0,
            end: 11,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.get_text(&source_map).unwrap(), "hello world".to_string());
    }

    #[test]
    fn test_get_text_empty() {
        let mut source_map = std::collections::HashMap::new();
        source_map.insert("test".to_string(), ("test.lemma".to_string(), "hello world".to_string()));
        let span = Span {
            start: 5,
            end: 5,
            line: 1,
            col: 5,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.get_text(&source_map).unwrap(), "".to_string());
    }

    #[test]
    fn test_get_text_out_of_bounds_start() {
        let mut source_map = std::collections::HashMap::new();
        source_map.insert("test".to_string(), ("test.lemma".to_string(), "hello".to_string()));
        let span = Span {
            start: 10,
            end: 15,
            line: 1,
            col: 10,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert!(loc.get_text(&source_map).is_err());
    }

    #[test]
    fn test_get_text_out_of_bounds_end() {
        let mut source_map = std::collections::HashMap::new();
        source_map.insert("test".to_string(), ("test.lemma".to_string(), "hello".to_string()));
        let span = Span {
            start: 0,
            end: 10,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert!(loc.get_text(&source_map).is_err());
    }

    #[test]
    fn test_get_text_unicode() {
        let mut source_map = std::collections::HashMap::new();
        source_map.insert("test".to_string(), ("test.lemma".to_string(), "hello 世界".to_string()));
        let span = Span {
            start: 6,
            end: 12,
            line: 1,
            col: 6,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.get_text(&source_map).unwrap(), "世界".to_string());
    }

    #[test]
    fn test_new_with_string() {
        let span = Span {
            start: 0,
            end: 5,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.source_id, "test.lemma");
        assert_eq!(loc.doc_name, "test");
    }

    #[test]
    fn test_new_with_str() {
        let span = Span {
            start: 0,
            end: 5,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.source_id, "test.lemma");
        assert_eq!(loc.doc_name, "test");
    }

    #[test]
    fn test_source_get_text_with_location() {
        use crate::parsing::ast::Span;
        use crate::parsing::source::Source;
        use std::collections::HashMap;

        let source = "fact value = 42";
        let mut sources = HashMap::new();
        sources.insert("test".to_string(), ("test.lemma".to_string(), source.to_string()));

        let span = Span {
            start: 13,
            end: 15,
            line: 1,
            col: 13,
        };
        let source = Source::new("test.lemma", span, "test");

        assert_eq!(
            source.get_text(&sources).unwrap(),
            "42".to_string()
        );
    }

    #[test]
    fn test_source_get_text_source_not_found() {
        use crate::parsing::ast::Span;
        use crate::parsing::source::Source;
        use std::collections::HashMap;

        let sources = HashMap::new();
        let span = Span {
            start: 0,
            end: 5,
            line: 1,
            col: 0,
        };
        let source = Source::new("missing.lemma", span, "test");

        assert!(source.get_text(&sources).is_err());
    }
}
