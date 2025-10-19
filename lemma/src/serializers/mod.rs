mod json;
mod msgpack;
mod protobuf;

pub use json::to_lemma_syntax as from_json;
pub use msgpack::to_lemma_syntax as from_msgpack;
pub use protobuf::to_lemma_syntax as from_protobuf;

use crate::{FactValue, LemmaDoc, LemmaError, LemmaType, TypeAnnotation};
use std::collections::HashMap;

/// Find the type of a fact in a document
pub(crate) fn find_fact_type(
    name: &str,
    doc: &LemmaDoc,
    all_docs: &HashMap<String, LemmaDoc>,
) -> Result<LemmaType, LemmaError> {
    for fact in &doc.facts {
        let fact_name = crate::analysis::fact_display_name(fact);
        if fact_name == name {
            return match &fact.value {
                FactValue::Literal(lit) => Ok(lit.to_type()),
                FactValue::TypeAnnotation(TypeAnnotation::LemmaType(t)) => Ok(t.clone()),
                FactValue::DocumentReference(ref_doc) => {
                    if name.contains('.') {
                        let parts: Vec<&str> = name.splitn(2, '.').collect();
                        if parts.len() == 2 {
                            if let Some(referenced) = all_docs.get(ref_doc) {
                                return find_fact_type(parts[1], referenced, all_docs);
                            }
                        }
                    }
                    Err(LemmaError::Engine(format!(
                        "Cannot override document reference '{}'",
                        name
                    )))
                }
            };
        }
    }
    Err(LemmaError::Engine(format!(
        "Fact '{}' not found in document",
        name
    )))
}
