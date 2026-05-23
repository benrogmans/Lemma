//! Evaluation request options (rule-result unit conversion).

use crate::error::Error;
use crate::planning::execution_plan::ExecutionPlan;
use crate::planning::semantics::SemanticConversionTarget;
use std::collections::HashMap;

/// Optional per-rule unit conversion for evaluation APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationRequest {
    /// Rule name → validated conversion target (quantity or ratio unit).
    pub rule_result_units: HashMap<String, SemanticConversionTarget>,
}

impl EvaluationRequest {
    /// Parse `rule:unit` strings and validate against the execution plan.
    pub fn from_rule_conversion_strings(
        strings: HashMap<String, String>,
        plan: &ExecutionPlan,
    ) -> Result<Self, Error> {
        let mut rule_result_units = HashMap::new();
        for (rule_name, unit_raw) in strings {
            let rule_name =
                crate::parsing::ast::ascii_lowercase_logical_name(rule_name.trim().to_string());
            if rule_name.is_empty() {
                return Err(Error::request(
                    "Rule name in conversion map cannot be empty",
                    None::<String>,
                ));
            }
            if rule_result_units.contains_key(&rule_name) {
                return Err(Error::request(
                    format!("Duplicate conversion for rule '{rule_name}'"),
                    None::<String>,
                ));
            }
            let unit = normalize_unit_name(&unit_raw)?;
            if is_reserved_api_conversion_target(&unit) {
                return Err(Error::request(
                    format!(
                        "API rule conversion supports quantity or ratio unit names only; '{}' is reserved. \
                         Use in-rule `as` for other conversion targets.",
                        unit
                    ),
                    None::<String>,
                ));
            }
            let exec_rule = plan.get_rule(&rule_name).ok_or_else(|| {
                Error::request(
                    format!(
                        "Rule '{}' not found in spec '{}'",
                        rule_name, plan.spec_name
                    ),
                    None::<String>,
                )
            })?;
            if !exec_rule.path.segments.is_empty() {
                return Err(Error::request(
                    format!(
                        "Rule '{}' is not a top-level rule; API conversion applies only to top-level rules",
                        rule_name
                    ),
                    None::<String>,
                ));
            }
            let rule_type = &exec_rule.rule_type;
            if rule_type.is_anonymous_quantity()
                || (!rule_type.is_quantity() && !rule_type.is_ratio())
            {
                return Err(Error::request(
                    format!(
                        "Rule '{}' has result type '{}'; API conversion requires a quantity or ratio result type",
                        rule_name,
                        rule_type.name()
                    ),
                    None::<String>,
                ));
            }
            let target = rule_type
                .validate_rule_result_unit_conversion(&unit, &plan.unit_index, &plan.spec_name)
                .map_err(|message| {
                    Error::request(format!("Rule '{rule_name}': {message}"), None::<String>)
                })?;
            rule_result_units.insert(rule_name, target);
        }
        Ok(Self { rule_result_units })
    }
}

/// Parse comma-separated or repeated `rule:unit` entries into a map of raw strings.
pub fn parse_rule_result_conversion_strings(
    comma_separated: &str,
) -> Result<HashMap<String, String>, Error> {
    let mut out = HashMap::new();
    for segment in comma_separated.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let (rule_name, unit) = segment.split_once(':').ok_or_else(|| {
            Error::request(
                format!(
                    "Invalid conversion '{}'; expected 'rule:unit' (for example 'price:usd')",
                    segment
                ),
                None::<String>,
            )
        })?;
        let rule_name =
            crate::parsing::ast::ascii_lowercase_logical_name(rule_name.trim().to_string());
        let unit = unit.trim();
        if rule_name.is_empty() || unit.is_empty() {
            return Err(Error::request(
                format!(
                    "Invalid conversion '{}'; rule name and unit cannot be empty",
                    segment
                ),
                None::<String>,
            ));
        }
        if out.contains_key(&rule_name) {
            return Err(Error::request(
                format!("Duplicate conversion for rule '{rule_name}'"),
                None::<String>,
            ));
        }
        out.insert(rule_name.to_string(), unit.to_string());
    }
    Ok(out)
}

fn normalize_unit_name(raw: &str) -> Result<String, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::request("Unit name cannot be empty", None::<String>));
    }
    Ok(trimmed.to_lowercase())
}

fn is_reserved_api_conversion_target(unit: &str) -> bool {
    unit == "number"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::DateTimeValue;
    use crate::parsing::source::SourceType;
    use crate::Engine;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn parse_rule_result_conversion_strings_splits_pairs() {
        let map = parse_rule_result_conversion_strings("price:usd,total:eur").unwrap();
        assert_eq!(map.get("price").map(String::as_str), Some("usd"));
        assert_eq!(map.get("total").map(String::as_str), Some("eur"));
    }

    #[test]
    fn reserved_number_target_rejected() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"spec money
data price: quantity -> unit eur 1 -> unit usd 0.91 -> default 100 eur
rule total: price"#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("t.lemma"))),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "money", Some(&now)).unwrap().clone();
        let err = EvaluationRequest::from_rule_conversion_strings(
            HashMap::from([("total".to_string(), "number".to_string())]),
            &plan,
        )
        .expect_err("number reserved");
        assert!(err
            .to_string()
            .contains("quantity or ratio unit names only"));
    }
}
