use lemma::{DateTimeValue, Engine, Error, EvaluationRequest};
use std::collections::HashMap;

pub fn build_evaluation_request(
    engine: &Engine,
    repository: Option<&str>,
    spec_set_identifier: &str,
    effective: &DateTimeValue,
    rule_result_units: &[String],
    rule_filter: &[String],
) -> Result<EvaluationRequest, Error> {
    let conversion_strings = if rule_result_units.is_empty() {
        HashMap::new()
    } else {
        let joined = rule_result_units.join(",");
        lemma::parse_rule_result_conversion_strings(&joined)?
    };
    if !rule_filter.is_empty() {
        for rule_name in conversion_strings.keys() {
            if !rule_filter.contains(rule_name) {
                return Err(Error::request(
                    format!(
                        "Conversion for rule '{rule_name}' was requested but that rule is not listed in --rules"
                    ),
                    None::<String>,
                ));
            }
        }
    }
    if conversion_strings.is_empty() {
        return Ok(EvaluationRequest::default());
    }
    let plan = engine.get_plan(repository, spec_set_identifier, Some(effective))?;
    EvaluationRequest::from_rule_conversion_strings(conversion_strings, plan)
}

pub fn build_evaluation_request_from_query(
    engine: &Engine,
    repository: Option<&str>,
    spec_set_identifier: &str,
    effective: &DateTimeValue,
    as_units_query: Option<&str>,
    rule_filter: &[String],
) -> Result<EvaluationRequest, Error> {
    let rule_result_units: Vec<String> = match as_units_query {
        Some(text) if !text.trim().is_empty() => vec![text.to_string()],
        _ => Vec::new(),
    };
    build_evaluation_request(
        engine,
        repository,
        spec_set_identifier,
        effective,
        &rule_result_units,
        rule_filter,
    )
}
