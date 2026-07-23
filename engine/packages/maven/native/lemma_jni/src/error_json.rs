//! WASM-shaped `EngineError` JSON for the Java package.

use lemma::{Error, ErrorKind, Source};
use serde::Serialize;

#[derive(Serialize)]
struct EngineErrorSource {
    attribute: String,
    line: usize,
    column: usize,
    length: usize,
}

impl From<&Source> for EngineErrorSource {
    fn from(s: &Source) -> Self {
        EngineErrorSource {
            attribute: s.source_type.to_string(),
            line: s.span.line,
            column: s.span.col,
            length: s.span.end.saturating_sub(s.span.start),
        }
    }
}

#[derive(Serialize)]
struct EngineErrorJson<'a> {
    kind: ErrorKind,
    message: &'a str,
    related_data: Option<&'a str>,
    spec: Option<&'a str>,
    related_spec: Option<&'a str>,
    source: Option<EngineErrorSource>,
    suggestion: Option<&'a str>,
    repository: Option<&'a str>,
}

impl<'a> From<&'a Error> for EngineErrorJson<'a> {
    fn from(e: &'a Error) -> Self {
        EngineErrorJson {
            kind: e.kind(),
            message: e.message(),
            related_data: e.related_data(),
            spec: e.spec_context_name(),
            related_spec: e.related_spec(),
            source: e.source_location().map(EngineErrorSource::from),
            suggestion: e.suggestion(),
            repository: e.repository(),
        }
    }
}

pub fn engine_errors_json(errors: &[Error]) -> String {
    let rows: Vec<EngineErrorJson<'_>> = errors.iter().map(EngineErrorJson::from).collect();
    serde_json::to_string(&rows).expect("BUG: EngineError array JSON serialization failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_error_serializes_kind() {
        let err = Error::request("decimal values must be passed as strings", None::<String>);
        let json = engine_errors_json(std::slice::from_ref(&err));
        assert!(json.contains("\"kind\":\"request\""));
        assert!(json.contains("decimal values must be passed as strings"));
    }
}
