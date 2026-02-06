use lemma::LemmaError;
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

    // Compute column as UTF-16 code units from line start to the offset.
    // LSP specifies columns as UTF-16 code unit offsets.
    let line_slice = &text[line_start_byte..clamped_offset];
    let utf16_column: u32 = line_slice.encode_utf16().count() as u32;

    Position {
        line,
        character: utf16_column,
    }
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

/// A safe default range anchored at the first character of the document.
///
/// Used for errors that have no specific source span (e.g. ResourceLimitExceeded).
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

/// Flatten a LemmaError into a list of individual errors.
///
/// MultipleErrors is recursively flattened. All other variants yield a single-element list.
fn flatten_errors(error: &LemmaError) -> Vec<&LemmaError> {
    match error {
        LemmaError::MultipleErrors(errors) => errors.iter().flat_map(flatten_errors).collect(),
        other => vec![other],
    }
}

/// Convert a single (non-MultipleErrors) LemmaError into an LSP Diagnostic.
///
/// The `text` parameter is the current editor buffer content, used to convert
/// byte offsets to LSP positions.
/// The `file_attribute` is the source identifier for the file being diagnosed,
/// used to filter errors that belong to this file.
fn single_error_to_diagnostic(error: &LemmaError, text: &str) -> Diagnostic {
    let range = match error {
        LemmaError::ResourceLimitExceeded { .. } => default_range(),
        other => {
            if let Some(source) = other.location() {
                let start = source.span.start;
                let end = source.span.end;
                // If both start and end are 0 (e.g. from pest parse errors where byte
                // offsets aren't available), fall back to default range.
                if start == 0 && end == 0 {
                    default_range()
                } else {
                    span_to_range(text, start, end)
                }
            } else {
                default_range()
            }
        }
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

/// Convert all LemmaErrors into LSP Diagnostics for a given file.
///
/// - `errors`: the errors to convert (may include MultipleErrors, which are flattened).
/// - `text`: the current editor buffer content for the file being diagnosed.
/// - `file_attribute`: the source identifier (filename) for the file being diagnosed.
///   Only errors whose source location `attribute` matches this value are included.
///   Errors without a source location (e.g. ResourceLimitExceeded) are always included.
pub fn errors_to_diagnostics(
    errors: &[LemmaError],
    text: &str,
    file_attribute: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for error in errors {
        let flat = flatten_errors(error);
        for single_error in flat {
            // Filter: only include errors that belong to this file,
            // or errors without a source location (they apply everywhere).
            let belongs_to_file = match single_error {
                LemmaError::ResourceLimitExceeded { .. } => true,
                other => match other.location() {
                    Some(source) => source.attribute == file_attribute,
                    None => true,
                },
            };

            if belongs_to_file {
                diagnostics.push(single_error_to_diagnostic(single_error, text));
            }
        }
    }

    diagnostics
}

/// Convert a single parse error into diagnostics.
///
/// This is used for the fast path: immediately publishing parse errors
/// for the active file without waiting for the debounced workspace re-plan.
pub fn parse_error_to_diagnostics(error: &LemmaError, text: &str) -> Vec<Diagnostic> {
    let flat = flatten_errors(error);
    flat.into_iter()
        .map(|single_error| single_error_to_diagnostic(single_error, text))
        .collect()
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
        let text = "doc test\nfact x = 10";
        let range = span_to_range(text, 9, 20);
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 1);
        assert_eq!(range.end.character, 11);
    }

    #[test]
    fn span_to_range_multiline() {
        let text = "doc test\nfact x = 10\nrule y = 20";
        let range = span_to_range(text, 9, 32);
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 2);
        assert_eq!(range.end.character, 11);
    }

    #[test]
    fn flatten_single_error() {
        let error = LemmaError::ResourceLimitExceeded {
            limit_name: "test".to_string(),
            limit_value: "100".to_string(),
            actual_value: "200".to_string(),
            suggestion: "reduce size".to_string(),
        };
        let flat = flatten_errors(&error);
        assert_eq!(flat.len(), 1);
    }

    #[test]
    fn flatten_multiple_errors_recursively() {
        let error1 = LemmaError::ResourceLimitExceeded {
            limit_name: "limit_a".to_string(),
            limit_value: "100".to_string(),
            actual_value: "200".to_string(),
            suggestion: "fix a".to_string(),
        };
        let error2 = LemmaError::ResourceLimitExceeded {
            limit_name: "limit_b".to_string(),
            limit_value: "50".to_string(),
            actual_value: "75".to_string(),
            suggestion: "fix b".to_string(),
        };
        let inner_multiple = LemmaError::MultipleErrors(vec![error1]);
        let outer_multiple = LemmaError::MultipleErrors(vec![inner_multiple, error2]);
        let flat = flatten_errors(&outer_multiple);
        assert_eq!(flat.len(), 2);
    }

    #[test]
    fn resource_limit_exceeded_uses_default_range() {
        let error = LemmaError::ResourceLimitExceeded {
            limit_name: "max_file_size_bytes".to_string(),
            limit_value: "5MB".to_string(),
            actual_value: "10MB".to_string(),
            suggestion: "reduce file size".to_string(),
        };
        let diagnostics = parse_error_to_diagnostics(&error, "doc test\nfact x = 10");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start.line, 0);
        assert_eq!(diagnostics[0].range.start.character, 0);
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostics[0].source, Some("lemma".to_string()));
    }

    #[test]
    fn errors_to_diagnostics_filters_by_file_attribute() {
        use lemma::Span;
        use std::sync::Arc;

        let error_in_file = LemmaError::parse(
            "bad syntax",
            Span {
                start: 0,
                end: 8,
                line: 1,
                col: 1,
            },
            "file_a.lemma",
            Arc::from("doc test"),
            "test",
            1,
            None::<String>,
        );
        let error_in_other_file = LemmaError::parse(
            "also bad",
            Span {
                start: 0,
                end: 5,
                line: 1,
                col: 1,
            },
            "file_b.lemma",
            Arc::from("doc other"),
            "other",
            1,
            None::<String>,
        );

        let text = "doc test";
        let diagnostics =
            errors_to_diagnostics(&[error_in_file, error_in_other_file], text, "file_a.lemma");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("bad syntax"));
    }
}
