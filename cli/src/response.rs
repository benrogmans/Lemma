use serde::Serialize;
use std::collections::HashMap;

/// Unified JSON representation of a single rule's evaluation result.
///
/// Used by the CLI (`run --output json`), the HTTP server, and any future
/// JSON-producing surface. Ensures consistent field names and veto handling.
#[derive(Debug, Serialize)]
pub struct RuleResultJson {
    /// The computed value as a display string, or `None` when the rule was vetoed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// `true` when the rule produced a Veto (no value), `false` otherwise.
    pub is_veto: bool,
    /// Human-readable veto reason, if the rule was vetoed with a message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub veto_reason: Option<String>,
    /// The rule's result type (e.g. "number", "boolean", "money").
    pub rule_type: String,
    /// Structured proof tree (JSON). Included only when the caller opts in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<serde_json::Value>,
}

/// Convert an engine `Response` into a JSON-ready map of rule results.
pub fn convert_response(
    response: &lemma::Response,
    include_proofs: bool,
) -> HashMap<String, RuleResultJson> {
    response
        .results
        .iter()
        .map(|(name, rule_result)| {
            let (value, is_veto, veto_reason) = match &rule_result.result {
                lemma::OperationResult::Value(v) => (Some(v.to_string()), false, None),
                lemma::OperationResult::Veto(msg) => (None, true, msg.clone()),
            };
            let proof = if include_proofs {
                rule_result
                    .proof
                    .as_ref()
                    .and_then(|p| serde_json::to_value(p).ok())
            } else {
                None
            };
            (
                name.clone(),
                RuleResultJson {
                    value,
                    is_veto,
                    veto_reason,
                    rule_type: rule_result.rule_type.name(),
                    proof,
                },
            )
        })
        .collect()
}
