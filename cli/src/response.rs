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

/// Convert a LiteralValue to (json_value, optional_unit).
fn literal_to_json(v: &lemma::LiteralValue) -> (serde_json::Value, Option<String>) {
    match &v.value {
        lemma::ValueKind::Boolean(b) => (serde_json::Value::Bool(*b), None),
        lemma::ValueKind::Number(n) => (decimal_to_json(n), None),
        lemma::ValueKind::Scale(n, unit) => (decimal_to_json(n), Some(unit.clone())),
        lemma::ValueKind::Ratio(r, _) => (decimal_to_json(r), None),
        lemma::ValueKind::Duration(n, unit) => (decimal_to_json(n), Some(unit.to_string())),
        _ => (serde_json::Value::String(v.display_value()), None),
    }
}

fn decimal_to_json(d: &rust_decimal::Decimal) -> serde_json::Value {
    if d.fract().is_zero() {
        serde_json::Value::Number(
            i64::try_from(d.trunc())
                .expect("BUG: integer decimal out of i64 range")
                .into(),
        )
    } else {
        serde_json::Value::Number(
            serde_json::Number::from_f64(
                d.to_string()
                    .parse::<f64>()
                    .expect("BUG: Decimal::to_string produced non-numeric output"),
            )
            .expect("BUG: decimal produced NaN or Infinity"),
        )
    }
}

/// Convert an engine `Response` into a JSON-ready map of rule results,
/// ordered by definition line in the source document.
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
                    let (val, unit) = literal_to_json(v);
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
