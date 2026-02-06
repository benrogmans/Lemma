use crate::parsing::ast::Span;

/// Unified source location information
///
/// Combines source file identifier, span, and document name
/// for consistent source location tracking across the codebase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Source {
    /// Source file identifier (e.g., filename or "<input>")
    pub attribute: String,

    /// Span in source code (uses Lemma's existing `Span` type from `crate::ast::Span`)
    pub span: Span,

    /// Document name (the Lemma document containing this code)
    pub doc_name: String,
}

impl Source {
    /// Create a new Source
    #[must_use]
    pub fn new(attribute: impl Into<String>, span: Span, doc_name: impl Into<String>) -> Self {
        Self {
            attribute: attribute.into(),
            span,
            doc_name: doc_name.into(),
        }
    }
}

impl Source {
    /// Extract the source text for this location from the given source string
    ///
    /// Returns `None` if the span is out of bounds for the source.
    pub fn extract_text(&self, source: &str) -> Option<String> {
        let bytes = source.as_bytes();
        if self.span.start < bytes.len() && self.span.end <= bytes.len() {
            Some(String::from_utf8_lossy(&bytes[self.span.start..self.span.end]).to_string())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_valid() {
        let source = "hello world";
        let span = Span {
            start: 0,
            end: 5,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.extract_text(source), Some("hello".to_string()));
    }

    #[test]
    fn test_extract_text_middle() {
        let source = "hello world";
        let span = Span {
            start: 6,
            end: 11,
            line: 1,
            col: 6,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.extract_text(source), Some("world".to_string()));
    }

    #[test]
    fn test_extract_text_full_string() {
        let source = "hello world";
        let span = Span {
            start: 0,
            end: 11,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.extract_text(source), Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_text_empty() {
        let source = "hello world";
        let span = Span {
            start: 5,
            end: 5,
            line: 1,
            col: 5,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.extract_text(source), Some("".to_string()));
    }

    #[test]
    fn test_extract_text_out_of_bounds_start() {
        let source = "hello";
        let span = Span {
            start: 10,
            end: 15,
            line: 1,
            col: 10,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.extract_text(source), None);
    }

    #[test]
    fn test_extract_text_out_of_bounds_end() {
        let source = "hello";
        let span = Span {
            start: 0,
            end: 10,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.extract_text(source), None);
    }

    #[test]
    fn test_extract_text_unicode() {
        let source = "hello 世界";
        let span = Span {
            start: 6,
            end: 12,
            line: 1,
            col: 6,
        };
        let loc = Source::new("test.lemma", span, "test");
        assert_eq!(loc.extract_text(source), Some("世界".to_string()));
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
        assert_eq!(loc.attribute, "test.lemma");
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
        assert_eq!(loc.attribute, "test.lemma");
        assert_eq!(loc.doc_name, "test");
    }
}
