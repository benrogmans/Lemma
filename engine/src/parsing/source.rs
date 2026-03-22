use crate::parsing::ast::Span;
use std::collections::HashMap;

/// Positional source location: file and span.
///
/// Text is resolved via `text_from(sources)` when needed. No embedded source text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Source {
    /// Source file identifier (e.g., filename)
    pub attribute: String,

    /// Span in source code
    pub span: Span,
}

impl Source {
    #[must_use]
    pub fn new(attribute: impl Into<String>, span: Span) -> Self {
        Self {
            attribute: attribute.into(),
            span,
        }
    }

    /// Resolve source text from the sources map.
    #[must_use]
    pub fn text_from<'a>(
        &self,
        sources: &'a HashMap<String, String>,
    ) -> Option<std::borrow::Cow<'a, str>> {
        let s = sources.get(&self.attribute)?;
        s.get(self.span.start..self.span.end)
            .map(std::borrow::Cow::Borrowed)
    }

    /// Extract the source text from the given source string.
    ///
    /// Returns `None` if the span is out of bounds.
    #[must_use]
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
    use std::collections::HashMap;

    #[test]
    fn test_extract_text_valid() {
        let source = "hello world";
        let span = Span {
            start: 0,
            end: 5,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span);
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
        let loc = Source::new("test.lemma", span);
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
        let loc = Source::new("test.lemma", span);
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
        let loc = Source::new("test.lemma", span);
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
        let loc = Source::new("test.lemma", span);
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
        let loc = Source::new("test.lemma", span);
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
        let loc = Source::new("test.lemma", span);
        assert_eq!(loc.extract_text(source), Some("世界".to_string()));
    }

    #[test]
    fn test_new() {
        let span = Span {
            start: 0,
            end: 5,
            line: 1,
            col: 0,
        };
        let loc = Source::new("test.lemma", span);
        assert_eq!(loc.attribute, "test.lemma");
    }

    #[test]
    fn test_text_from() {
        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), "hello world".to_string());
        let loc = Source::new(
            "test.lemma",
            Span {
                start: 0,
                end: 5,
                line: 1,
                col: 0,
            },
        );
        assert_eq!(loc.text_from(&sources).as_deref(), Some("hello"));
    }
}
