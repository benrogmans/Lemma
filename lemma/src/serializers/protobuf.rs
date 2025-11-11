use crate::LemmaDoc;
use std::collections::HashMap;

/// Convert Protobuf fact overrides to Lemma syntax strings
///
/// Protobuf provides strongly-typed structured data. The implementation would:
/// - Define .proto schemas for Lemma fact overrides
/// - Deserialize binary Protobuf data
/// - Map fields to Lemma types using document schema
/// - Serialize to Lemma syntax strings
///
/// This is a stub implementation. Full Protobuf support requires:
/// - Add prost dependency
/// - Define .proto message formats
/// - Generate Rust code from .proto files
/// - Implement deserialization and mapping logic
///
/// Convert Protobuf bytes for a document to Lemma `name=value` strings.
///
/// # Errors
/// Always returns an error currently — this is a stub implementation pending
/// proper Protobuf support and mapping to Lemma types.
#[allow(clippy::implicit_hasher)]
pub fn to_lemma_syntax(
    _protobuf: &[u8],
    _doc: &LemmaDoc,
    _all_docs: &HashMap<String, LemmaDoc>,
) -> Result<Vec<String>, crate::LemmaError> {
    Err(crate::LemmaError::Engine(
        "Protobuf serialization not yet implemented".to_string(),
    ))
}
