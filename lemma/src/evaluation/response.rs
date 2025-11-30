use crate::evaluation::operations::{OperationRecord, OperationResult};
use crate::serialization::serialize_operation_result;
use indexmap::IndexMap;
use serde::Serialize;

/// Facts from a specific document
#[derive(Debug, Clone, Serialize)]
pub struct Facts {
    pub fact_path: String,
    pub referencing_fact_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_reference: Option<String>,
    pub facts: Vec<crate::LemmaFact>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub referenced_docs: Vec<Facts>,
}

/// Response from evaluating a Lemma document
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub doc_name: String,
    pub facts: Vec<Facts>,
    pub results: IndexMap<String, RuleResult>,
}

/// Result of evaluating a single rule
#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    #[serde(skip_serializing)]
    pub rule: crate::LemmaRule,
    #[serde(serialize_with = "serialize_operation_result")]
    pub result: OperationResult,
    pub facts: Vec<crate::LemmaFact>,
    #[serde(skip_serializing)]
    pub operations: Vec<OperationRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<crate::evaluation::proof::Proof>,
}

impl Response {
    pub fn add_result(&mut self, result: RuleResult) {
        self.results.insert(result.rule.name.clone(), result);
    }

    pub fn filter_rules(&mut self, rule_names: &[String]) {
        self.results.retain(|name, _| rule_names.contains(name));
    }
}
