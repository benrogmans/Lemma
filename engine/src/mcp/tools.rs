use std::collections::HashMap;

use serde_json::Value;

use crate::documentation::{GuideTopic, EVALUATE_GUIDE};
use crate::engine::{resolve_effective as resolve_effective_datetime, Engine};
use crate::format_explanation;
use crate::mcp::error::{map_engine_error, ToolError};
use crate::parsing::ast::DateTimeValue;
use crate::parsing::source::SourceType;
use crate::spec_set_id::parse_spec_set_id;

pub fn evaluate(engine: &Engine, args: &Value) -> Result<String, ToolError> {
    let spec_set_id = required_string(args, "spec")?;
    if spec_set_id.is_empty() {
        return Err(ToolError::invalid_arguments("Spec set id cannot be empty"));
    }
    let spec_name = parse_spec_set_id(spec_set_id).map_err(map_engine_error)?;
    let rule_names = optional_rule_names(args)?;
    let data_values = parse_data(args)?;
    let now = resolve_effective(args)?;
    let rules = if rule_names.is_empty() {
        None
    } else {
        Some(rule_names.as_slice())
    };
    let response = engine
        .run(None, &spec_name, Some(&now), data_values, rules, true)
        .map_err(map_engine_error)?;

    let show_for_missing = if response
        .results
        .values()
        .any(|result| result.awaits_missing_data())
    {
        Some(
            engine
                .show(None, &spec_name, Some(&now))
                .unwrap_or_else(|error| {
                    panic!("BUG: show must succeed after evaluate for '{spec_set_id}': {error}")
                }),
        )
    } else {
        None
    };

    let mut output = String::new();
    output.push_str(&format!("spec: {spec_set_id}\n"));
    output.push_str(&format!("effective: {now}\n"));
    output.push('\n');

    for result in response.results.values() {
        output.push_str(&format!("{}: ", result.rule.name));
        if result.vetoed {
            if let Some(reason) = result.veto_reason.as_deref() {
                output.push_str(reason);
            }
        } else {
            let display = result.display().unwrap_or_else(|| {
                panic!(
                    "BUG: rule '{}' evaluated without display after evaluation",
                    result.rule.name
                )
            });
            output.push_str(display);
            if let Some(value) = &result.value {
                if let Some(measure) = &value.measure {
                    append_unit_map(&mut output, measure);
                } else if let Some(ratio) = &value.ratio {
                    append_unit_map(&mut output, ratio);
                }
            }
        }
        output.push('\n');

        if result.awaits_missing_data() {
            let show = show_for_missing
                .as_ref()
                .expect("BUG: any awaiting rule requires show_for_missing after evaluate");
            output.push_str("missing_data:\n");
            for name in result.missing_data() {
                let entry = show.data.get(name).unwrap_or_else(|| {
                    panic!("BUG: missing_data key {name:?} must exist in show.data after evaluate")
                });
                let type_name = entry.lemma_type.specifications.to_string();
                let help = entry.lemma_type.specifications.help();
                if help.is_empty() {
                    output.push_str(&format!("  {name}: {type_name}\n"));
                } else {
                    output.push_str(&format!("  {name}: {type_name} — {help}\n"));
                }
            }
        }

        if let Some(explanation) = &result.explanation {
            let steps = format_explanation(explanation);
            if !steps.is_empty() {
                output.push_str("\nReasoning:\n");
                output.push_str(&steps);
                output.push('\n');
            }
        }
    }

    Ok(output)
}

pub fn list(engine: &Engine, args: &Value) -> Result<String, ToolError> {
    require_object(args)?;
    let list = engine.list();
    Ok(serde_json::to_string_pretty(&list)
        .unwrap_or_else(|error| panic!("BUG: engine list must serialize: {error}")))
}

pub fn show(engine: &Engine, args: &Value) -> Result<String, ToolError> {
    let spec_set_id = required_string(args, "spec")?;
    if spec_set_id.is_empty() {
        return Err(ToolError::invalid_arguments("Spec set id cannot be empty"));
    }
    let spec_name = parse_spec_set_id(spec_set_id).map_err(map_engine_error)?;
    let now = resolve_effective(args)?;
    let show = engine
        .show(None, &spec_name, Some(&now))
        .map_err(map_engine_error)?;
    Ok(serde_json::to_string_pretty(&show)
        .unwrap_or_else(|error| panic!("BUG: show response must serialize: {error}")))
}

pub fn source(engine: &Engine, args: &Value) -> Result<String, ToolError> {
    require_object(args)?;
    if let Some(repository) = optional_nonempty_string(args, "repository")? {
        return engine
            .source(Some(repository), None, None)
            .map_err(map_engine_error);
    }
    let spec_set_id = required_string(args, "spec")
        .map_err(|_| ToolError::invalid_arguments("Missing 'spec' or 'repository' field"))?;
    let spec_name = parse_spec_set_id(spec_set_id).map_err(map_engine_error)?;
    let now = resolve_effective(args)?;
    engine
        .source(None, Some(&spec_name), Some(&now))
        .map_err(map_engine_error)
}

pub fn check(args: &Value) -> Result<String, ToolError> {
    let sources_value = args
        .get("sources")
        .ok_or_else(|| ToolError::invalid_arguments("Missing 'sources' array field"))?;
    let sources_arr = sources_value
        .as_array()
        .ok_or_else(|| ToolError::invalid_arguments("Missing 'sources' array field"))?;
    if sources_arr.is_empty() {
        return Err(ToolError::invalid_arguments(
            "'sources' must be a non-empty array of [label, code] pairs",
        ));
    }

    let mut sources: Vec<(SourceType, String)> = Vec::with_capacity(sources_arr.len());
    for (i, entry) in sources_arr.iter().enumerate() {
        let pair = entry.as_array().ok_or_else(|| {
            ToolError::invalid_arguments(format!("sources[{i}] must be a [label, code] array"))
        })?;
        if pair.len() != 2 {
            return Err(ToolError::invalid_arguments(format!(
                "sources[{i}] must have exactly 2 elements [label, code]"
            )));
        }
        let label = pair[0].as_str().ok_or_else(|| {
            ToolError::invalid_arguments(format!("sources[{i}][0] (label) must be a string"))
        })?;
        let code = pair[1].as_str().ok_or_else(|| {
            ToolError::invalid_arguments(format!("sources[{i}][1] (code) must be a string"))
        })?;
        let source_type =
            SourceType::from_binding_label(label).map_err(ToolError::invalid_arguments)?;
        sources.push((source_type, code.to_string()));
    }

    let mut engine = Engine::new();
    if let Err(load_err) = engine.load(sources) {
        return Err(ToolError::diagnostics(&load_err.errors));
    }

    let recommendations = engine.quality();
    let mut text = String::from(
        "Parsed and planned. Syntax is valid; this does not verify the policy is correct.",
    );
    const MAX_RECOMMENDATIONS: usize = 20;
    if !recommendations.is_empty() {
        text.push_str("\n\nRecommendations:");
        for (i, rec) in recommendations.iter().take(MAX_RECOMMENDATIONS).enumerate() {
            text.push_str(&format!("\n{}. {}", i + 1, rec));
        }
        let omitted = recommendations.len().saturating_sub(MAX_RECOMMENDATIONS);
        if omitted > 0 {
            text.push_str(&format!("\n… and {omitted} more"));
        }
    }
    Ok(text)
}

pub fn guide(args: &Value) -> Result<String, ToolError> {
    require_object(args)?;
    match args.get("topic") {
        None => Ok(EVALUATE_GUIDE.to_string()),
        Some(value) => {
            let topic_name = value
                .as_str()
                .ok_or_else(|| ToolError::invalid_arguments("topic must be a string"))?;
            let topic = GuideTopic::parse(topic_name).ok_or_else(|| {
                ToolError::invalid_arguments(format!(
                    "Unknown guide topic '{topic_name}'. Valid: {}",
                    GuideTopic::VALID_LIST
                ))
            })?;
            Ok(topic.section_text().to_string())
        }
    }
}

fn require_object(args: &Value) -> Result<(), ToolError> {
    if args.is_object() || args.is_null() {
        Ok(())
    } else {
        Err(ToolError::invalid_arguments("arguments must be an object"))
    }
}

fn required_string<'a>(args: &'a Value, field: &str) -> Result<&'a str, ToolError> {
    match args.get(field) {
        Some(Value::String(value)) => Ok(value.trim()),
        Some(_) => Err(ToolError::invalid_arguments(format!(
            "'{field}' must be a string"
        ))),
        None => Err(ToolError::invalid_arguments(format!(
            "Missing '{field}' field"
        ))),
    }
}

fn optional_nonempty_string<'a>(
    args: &'a Value,
    field: &str,
) -> Result<Option<&'a str>, ToolError> {
    match args.get(field) {
        None => Ok(None),
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        }
        Some(_) => Err(ToolError::invalid_arguments(format!(
            "'{field}' must be a string"
        ))),
    }
}

fn optional_rule_names(args: &Value) -> Result<Vec<String>, ToolError> {
    match args.get("rule") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(rule)) => {
            let trimmed = rule.trim();
            if trimmed.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![trimmed.to_string()])
            }
        }
        Some(_) => Err(ToolError::invalid_arguments("'rule' must be a string")),
    }
}

fn parse_data(args: &Value) -> Result<HashMap<String, String>, ToolError> {
    match args.get("data") {
        None | Some(Value::Null) => Ok(HashMap::new()),
        Some(Value::Array(entries)) => {
            let mut data = HashMap::new();
            for (i, entry) in entries.iter().enumerate() {
                let Some(raw) = entry.as_str() else {
                    return Err(ToolError::invalid_arguments(format!(
                        "data[{i}] must be a string 'name=value'"
                    )));
                };
                let Some((name, value)) = raw.split_once('=') else {
                    return Err(ToolError::invalid_arguments(format!(
                        "data[{i}] must be 'name=value', got '{raw}'"
                    )));
                };
                data.insert(name.to_string(), value.to_string());
            }
            Ok(data)
        }
        Some(_) => Err(ToolError::invalid_arguments(
            "'data' must be an array of 'name=value' strings",
        )),
    }
}

fn resolve_effective(args: &Value) -> Result<DateTimeValue, ToolError> {
    match args.get("effective") {
        None | Some(Value::Null) => resolve_effective_datetime(None).map_err(map_engine_error),
        Some(Value::String(raw)) => resolve_effective_datetime(Some(raw)).map_err(map_engine_error),
        Some(_) => Err(ToolError::invalid_arguments("'effective' must be a string")),
    }
}

fn append_unit_map(output: &mut String, map: &std::collections::BTreeMap<String, String>) {
    let parts: Vec<String> = map
        .iter()
        .map(|(unit, magnitude)| format!("{unit} {magnitude}"))
        .collect();
    output.push_str(" (");
    output.push_str(&parts.join(", "));
    output.push(')');
}
