use lemma::Error;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

/// Convert a byte offset to an LSP Position (0-based line and UTF-16 code unit column).
///
/// Walks the text to compute the line and column at the given byte offset.
/// If the offset is beyond the end of the text, returns the position at the end of the text.
fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let clamped_offset = byte_offset.min(text.len());
    let mut line: u32 = 0;
    let mut line_start_byte: usize = 0;

    for (index, byte) in text.bytes().enumerate() {
        if index == clamped_offset {
            break;
        }
        if byte == b'\n' {
            line += 1;
            line_start_byte = index + 1;
        }
    }

    // If offset points at a newline, treat as start of next line (LSP convention for spans that start at newline).
    let (line, character) =
        if clamped_offset < text.len() && text.as_bytes()[clamped_offset] == b'\n' {
            (line + 1, 0)
        } else {
            let line_slice = &text[line_start_byte..clamped_offset];
            (line, line_slice.encode_utf16().count() as u32)
        };

    Position { line, character }
}

/// Convert a Lemma Span (byte offsets) to an LSP Range using the editor buffer text.
///
/// This is the reliable approach: building the Range from byte offsets against the
/// current editor buffer text so offsets match what the user is seeing.
fn span_to_range(text: &str, start_byte: usize, end_byte: usize) -> Range {
    let start_position = byte_offset_to_position(text, start_byte);
    let end_position = byte_offset_to_position(text, end_byte);
    Range {
        start: start_position,
        end: end_position,
    }
}

/// A safe default range anchored at the first character of the spec.
///
/// Used for errors that have no specific source span.
fn default_range() -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 0,
        },
    }
}

/// Convert a single Error into an LSP Diagnostic.
///
/// The `text` parameter is the current editor buffer content, used to convert
/// byte offsets to LSP positions.
/// The `file_attribute` is the source identifier for the file being diagnosed,
/// used to filter errors that belong to this file.
pub fn single_error_to_diagnostic(error: &Error, text: &str) -> Diagnostic {
    let range = if let Some(source) = error.location() {
        let start = source.span.start;
        let end = source.span.end;
        if start == 0 && end == 0 {
            default_range()
        } else {
            span_to_range(text, start, end)
        }
    } else {
        default_range()
    };

    let message = format!("{}", error);

    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: Some("lemma".to_string()),
        message,
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Convert all Errors into LSP Diagnostics for a given file.
///
/// - `errors`: the errors to convert.
/// - `text`: the current editor buffer content for the file being diagnosed.
/// - `file_attribute`: the source identifier (filename) for the file being diagnosed.
///   Only errors whose source location `attribute` matches this value are included.
///   Errors without a source location are always included.
pub fn errors_to_diagnostics(
    errors: &[Error],
    text: &str,
    file_attribute: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for error in errors {
        let belongs_to_file = match error.location() {
            Some(source) => source.attribute == file_attribute,
            None => true,
        };

        if belongs_to_file {
            diagnostics.push(single_error_to_diagnostic(error, text));
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_offset_to_position_first_line() {
        let text = "hello world";
        let position = byte_offset_to_position(text, 6);
        assert_eq!(position.line, 0);
        assert_eq!(position.character, 6);
    }

    #[test]
    fn byte_offset_to_position_second_line() {
        let text = "hello\nworld";
        let position = byte_offset_to_position(text, 6);
        assert_eq!(position.line, 1);
        assert_eq!(position.character, 0);
    }

    #[test]
    fn byte_offset_to_position_middle_of_second_line() {
        let text = "hello\nworld";
        let position = byte_offset_to_position(text, 9);
        assert_eq!(position.line, 1);
        assert_eq!(position.character, 3);
    }

    #[test]
    fn byte_offset_to_position_at_end_of_text() {
        let text = "hello\nworld";
        let position = byte_offset_to_position(text, 11);
        assert_eq!(position.line, 1);
        assert_eq!(position.character, 5);
    }

    #[test]
    fn byte_offset_to_position_beyond_text_clamps() {
        let text = "hello";
        let position = byte_offset_to_position(text, 100);
        assert_eq!(position.line, 0);
        assert_eq!(position.character, 5);
    }

    #[test]
    fn byte_offset_to_position_empty_text() {
        let text = "";
        let position = byte_offset_to_position(text, 0);
        assert_eq!(position.line, 0);
        assert_eq!(position.character, 0);
    }

    #[test]
    fn byte_offset_to_position_unicode() {
        // "a" (1 byte) + "日" (3 bytes, 1 UTF-16 code unit) + "b" (1 byte)
        let text = "a日b";
        // Offset 1 is start of "日" (3 bytes)
        let position = byte_offset_to_position(text, 1);
        assert_eq!(position.line, 0);
        assert_eq!(position.character, 1); // 'a' = 1 UTF-16 code unit
                                           // Offset 4 is start of "b"
        let position = byte_offset_to_position(text, 4);
        assert_eq!(position.line, 0);
        assert_eq!(position.character, 2); // 'a' + '日' = 2 UTF-16 code units
    }

    #[test]
    fn span_to_range_single_line() {
        let text = "spec test\nfact x: 10";
        let range = span_to_range(text, 9, 20);
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 1);
        assert_eq!(range.end.character, 10);
    }

    #[test]
    fn span_to_range_multiline() {
        let text = "spec test\nfact x: 10\nrule y: 20";
        let range = span_to_range(text, 9, 32);
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 2);
        assert_eq!(range.end.character, 10);
    }

    #[test]
    fn errors_to_diagnostics_with_multiple_errors() {
        let error1 = Error::resource_limit_exceeded("limit_a", "100", "200", "fix a", None);
        let error2 = Error::resource_limit_exceeded("limit_b", "50", "75", "fix b", None);
        let diagnostics =
            errors_to_diagnostics(&[error1, error2], "spec test\nfact x: 10", "test.lemma");
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn errors_to_diagnostics_filters_by_file_attribute() {
        use lemma::Span;
        use std::sync::Arc;

        let error_in_file = Error::parsing(
            "bad syntax",
            Some(lemma::Source::new(
                "file_a.lemma",
                Span {
                    start: 0,
                    end: 8,
                    line: 1,
                    col: 1,
                },
                "test",
                Arc::from("spec test"),
            )),
            None::<String>,
        );
        let error_in_other_file = Error::parsing(
            "also bad",
            Some(lemma::Source::new(
                "file_b.lemma",
                Span {
                    start: 0,
                    end: 5,
                    line: 1,
                    col: 1,
                },
                "other",
                Arc::from("spec other"),
            )),
            None::<String>,
        );

        let text = "spec test";
        let diagnostics =
            errors_to_diagnostics(&[error_in_file, error_in_other_file], text, "file_a.lemma");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("bad syntax"));
    }
}
