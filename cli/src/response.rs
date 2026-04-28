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
    /// Structured explanation tree (JSON). Included only when the caller opts in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<serde_json::Value>,
}

/// Evaluation response envelope with spec identity and effective datetime.
#[derive(Debug, Serialize)]
pub struct EvaluationEnvelope {
    pub spec: String,
    pub effective: String,
    pub result: IndexMap<String, RuleResultJson>,
}

/// Convert an engine `Response` into an envelope with traceability fields.
pub fn convert_response_envelope(
    response: &lemma::Response,
    include_explanations: bool,
    spec_name: &str,
    effective: &lemma::DateTimeValue,
) -> EvaluationEnvelope {
    let result = convert_response(response, include_explanations);
    EvaluationEnvelope {
        spec: spec_name.to_string(),
        effective: effective.to_string(),
        result,
    }
}

/// Convert an engine `Response` into a JSON-ready map of rule results,
/// ordered by definition line in the source spec.
pub fn convert_response(
    response: &lemma::Response,
    include_explanations: bool,
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
                lemma::OperationResult::Veto(reason) => {
                    (None, None, None, true, Some(reason.to_string()))
                }
            };
            let explanation = if include_explanations {
                rule_result
                    .explanation
                    .as_ref()
                    .and_then(|e| serde_json::to_value(e).ok())
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
                    explanation,
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
