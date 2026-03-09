use indexmap::IndexMap;
use serde::Serialize;

/// Unified JSON representation of a single rule's evaluation result.
///
/// Used by the CLI (`run --output json`), the HTTP server, and any future
/// JSON-producing surface. Ensures consistent field names and veto handling.
#[derive(Debug, Serialize)]
pub struct RuleResultJson {
    /// The computed value using native JSON types where possible:
    /// boolean -> JSON bool, number/scale/ratio/duration -> JSON number, text/date/time -> string.
    /// `None` when the rule was vetoed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// Unit of the value (e.g. "eur", "hours"). Present for scale and duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Human-readable formatted value (e.g. "345.00 eur", "true", "21%"). Always a string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// `true` when the rule produced a Veto (no value), `false` otherwise.
    pub vetoed: bool,
    /// Human-readable veto reason, if the rule was vetoed with a message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub veto_reason: Option<String>,
    /// The rule's result type (e.g. "number", "boolean", "money").
    pub rule_type: String,
    /// Structured proof tree (JSON). Included only when the caller opts in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<serde_json::Value>,
}

/// Evaluation response envelope with spec identity, effective datetime, and content hash.
#[derive(Debug, Serialize)]
pub struct EvaluationEnvelope {
    pub spec: String,
    pub effective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    pub result: IndexMap<String, RuleResultJson>,
}

/// Convert an engine `Response` into an envelope with traceability fields.
pub fn convert_response_with_hash(
    response: &lemma::Response,
    include_proofs: bool,
    spec_name: &str,
    effective: &lemma::DateTimeValue,
    hash: Option<&str>,
) -> EvaluationEnvelope {
    let result = convert_response(response, include_proofs);
    EvaluationEnvelope {
        spec: spec_name.to_string(),
        effective: effective.to_string(),
        hash: hash.map(|h| h.to_string()),
        result,
    }
}

/// Convert an engine `Response` into a JSON-ready map of rule results,
/// ordered by definition line in the source spec.
pub fn convert_response(
    response: &lemma::Response,
    include_proofs: bool,
) -> IndexMap<String, RuleResultJson> {
    let mut entries: Vec<_> = response
        .results
        .iter()
        .map(|(name, rule_result)| {
            let line = rule_result.rule.source_location.span.line;
            let (value, unit, display, vetoed, veto_reason) = match &rule_result.result {
                lemma::OperationResult::Value(v) => {
                    let (val, unit) = lemma::serialization::literal_value_to_json(v);
                    (Some(val), unit, Some(v.display_value()), false, None)
                }
                lemma::OperationResult::Veto(msg) => (None, None, None, true, msg.clone()),
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
                line,
                name.clone(),
                RuleResultJson {
                    value,
                    unit,
                    display,
                    vetoed,
                    veto_reason,
                    rule_type: rule_result.rule_type.name(),
                    proof,
                },
            )
        })
        .collect();

    entries.sort_by_key(|(line, _, _)| *line);
    entries
        .into_iter()
        .map(|(_, name, result)| (name, result))
        .collect()
}
