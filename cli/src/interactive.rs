use anyhow::{Context, Result};
use chrono::NaiveDate;
use inquire::validator::Validation;
use inquire::{DateSelect, MultiSelect, Select, Text};
use lemma::{
    DateTimeValue, Engine, LemmaType, ListedSpec, LiteralValue, Response, TypeSpecification,
    ValueKind, VetoType,
};
use rust_decimal::Decimal;
use std::collections::HashMap;

pub(crate) fn repository_loaded(engine: &Engine, name: &str) -> bool {
    engine
        .list()
        .iter()
        .any(|r| r.repository.as_deref() == Some(name))
}

/// Repository qualifier, spec name, selected rules, merged data (CLI + prompts).
pub type InteractiveResult = (
    Option<String>,
    String,
    Option<Vec<String>>,
    HashMap<String, String>,
);

#[derive(Clone, Debug)]
struct TextConstraints {
    length: Option<usize>,
    help: String,
}

#[derive(Clone, Debug)]
struct NumericConstraints {
    minimum: Option<Decimal>,
    maximum: Option<Decimal>,
    decimals: Option<u8>,
    help: String,
}

fn load_static_show(
    engine: &Engine,
    repo: Option<&str>,
    name: &str,
    now: &DateTimeValue,
) -> Result<lemma::Show> {
    engine
        .show(repo, name, Some(now))
        .map_err(|e| anyhow::anyhow!("{}", e))
}

pub fn run_interactive(
    engine: &Engine,
    spec_name: Option<String>,
    rule_names: Option<Vec<String>>,
    provided_data: &HashMap<String, String>,
    now: &DateTimeValue,
    cli_repository_qualifier: Option<&str>,
) -> Result<InteractiveResult> {
    let (repository_qualifier, specification_name) = match spec_name {
        Some(name) => {
            load_static_show(engine, cli_repository_qualifier, &name, now)?;
            (cli_repository_qualifier.map(String::from), name)
        }
        None => select_spec(engine, now, cli_repository_qualifier)?,
    };

    let rules = match rule_names {
        Some(names) => Some(names),
        None => select_rules(
            engine,
            repository_qualifier.as_deref(),
            &specification_name,
            now,
        )?,
    };

    let data = prompt_data(
        engine,
        repository_qualifier.as_deref(),
        &specification_name,
        &rules,
        provided_data,
        now,
    )?;

    Ok((repository_qualifier, specification_name, rules, data))
}

fn is_active_at(ls: &ListedSpec, now: &DateTimeValue) -> bool {
    let after_start = match &ls.effective_from {
        None => true,
        Some(from) => now >= from,
    };
    let before_end = match &ls.effective_to {
        None => true,
        Some(to) => now < to,
    };
    after_start && before_end
}

fn select_spec(
    engine: &Engine,
    now: &DateTimeValue,
    cli_repository_qualifier: Option<&str>,
) -> Result<(Option<String>, String)> {
    let mut items: Vec<(Option<String>, ListedSpec)> = engine
        .list()
        .into_iter()
        .flat_map(|repo| {
            let repo_name = repo.repository.clone();
            repo.specs
                .into_iter()
                .filter(|ls| is_active_at(ls, now))
                .map(move |ls| (repo_name.clone(), ls))
        })
        .collect();

    if let Some(q) = cli_repository_qualifier {
        if !repository_loaded(engine, q) {
            anyhow::bail!("Repository '{q}' not loaded");
        }
        items.retain(|(repo_name, _)| repo_name.as_deref() == Some(q));
    }

    if items.is_empty() {
        anyhow::bail!("No specs found in workspace. Add .lemma files to get started.");
    }

    let needs_repo_qualifier = {
        let mut names: Vec<&str> = items.iter().map(|(_, ls)| ls.name.as_str()).collect();
        names.sort();
        names.windows(2).any(|w| w[0] == w[1])
    };

    let display_options: Vec<String> = items
        .iter()
        .map(|(repo_name, ls)| {
            let label = repo_name.as_deref().unwrap_or("(workspace)");
            let rq = if needs_repo_qualifier {
                Some(label)
            } else {
                cli_repository_qualifier
            };
            let (data_count, rules_count) = load_static_show(engine, rq, &ls.name, now)
                .ok()
                .map(|show| (show.data.len(), show.rules.len()))
                .unwrap_or((0, 0));
            format!(
                "{} ({}) — {} data, {} rules",
                ls.name, label, data_count, rules_count
            )
        })
        .collect();

    let selected = Select::new("Select a spec:", display_options.clone())
        .with_help_message("Use arrow keys to navigate, Enter to select")
        .prompt()
        .context("Failed to get spec selection")?;

    let spec_index = display_options
        .iter()
        .position(|d| d == &selected)
        .context("Failed to find selected spec index")?;

    let (repo_name, ls) = items
        .into_iter()
        .nth(spec_index)
        .context("Failed to match selected spec")?;

    let rq = if needs_repo_qualifier {
        repo_name.or_else(|| Some("(workspace)".to_string()))
    } else {
        cli_repository_qualifier.map(String::from)
    };
    Ok((rq, ls.name))
}

fn select_rules(
    engine: &Engine,
    repo: Option<&str>,
    spec_name: &str,
    now: &DateTimeValue,
) -> Result<Option<Vec<String>>> {
    let show = load_static_show(engine, repo, spec_name, now)
        .context(format!("Spec '{}' not found", spec_name))?;
    let rule_names: Vec<String> = show.rules.keys().cloned().collect();

    if rule_names.is_empty() {
        return Ok(None);
    }

    let selected = MultiSelect::new("Select rules to evaluate:", rule_names.clone())
        .with_default(&(0..rule_names.len()).collect::<Vec<_>>())
        .prompt()
        .context("Failed to get rule selection")?;

    if selected.is_empty() || selected.len() == rule_names.len() {
        Ok(None)
    } else {
        Ok(Some(selected))
    }
}

fn prompt_data(
    engine: &Engine,
    repo: Option<&str>,
    spec_name: &str,
    rule_names: &Option<Vec<String>>,
    provided_data: &HashMap<String, String>,
    now: &DateTimeValue,
) -> Result<HashMap<String, String>> {
    let mut collected: HashMap<String, String> = HashMap::new();
    let mut header_printed = false;
    let rules_for_request = rule_names.as_deref().filter(|rules| !rules.is_empty());
    let show = load_static_show(engine, repo, spec_name, now)?;

    loop {
        let mut trial = provided_data.clone();
        trial.extend(collected.clone());
        let response = engine
            .run(repo, spec_name, Some(now), trial, rules_for_request, false)
            .context(format!("Spec '{}' not found", spec_name))?;

        let next_name = response
            .results
            .values()
            .flat_map(|result| result.missing_data.iter())
            .find(|name| !provided_data.contains_key(*name) && !collected.contains_key(*name))
            .cloned();

        let name = match next_name {
            Some(name) => name,
            None => break,
        };

        let entry = show
            .data
            .get(&name)
            .unwrap_or_else(|| panic!("BUG: missing_data key {name:?} must exist in show.data"));
        let lemma_type = entry.lemma_type.clone();
        // `ShowData.suggestion` is the API-facing per-unit map (`RuleResultValue`); prompts
        // need canonical `LiteralValue` methods (`magnitude_in_unit`, unit signatures, etc.),
        // so reconstruct once here and thread `LiteralValue` through the existing prompt code.
        let suggestion = entry.suggestion.as_ref().map(|v| v.to_literal(&lemma_type));
        let suggestion = suggestion.as_ref();

        if !header_printed {
            println!("\nEnter values for data (press Enter to accept suggestion):");
            header_printed = true;
        }

        loop {
            let input_value = prompt_value_for_type(&name, &lemma_type, suggestion)?;

            let mut validation_trial = provided_data.clone();
            validation_trial.extend(collected.clone());
            validation_trial.insert(name.clone(), input_value.clone());
            match engine.run(
                repo,
                spec_name,
                Some(now),
                validation_trial,
                rules_for_request,
                false,
            ) {
                Ok(response) => {
                    if let Some(reason) = computation_veto_reason_for_trial_input(&response) {
                        eprintln!("  {reason}\n");
                        continue;
                    }
                    collected.insert(name.clone(), input_value);
                    break;
                }
                Err(e) => {
                    eprintln!("  {}\n", e);
                }
            }
        }
    }

    Ok(collected)
}

fn computation_veto_reason_for_trial_input(response: &Response) -> Option<String> {
    for rule_result in response.results.values() {
        if !rule_result.vetoed {
            continue;
        }
        if matches!(
            rule_result.veto_detail.as_ref(),
            Some(VetoType::Computation { .. })
        ) {
            return rule_result.veto_reason.clone();
        }
    }
    None
}

fn prompt_value_for_type(
    data_name: &str,
    lemma_type: &LemmaType,
    suggestion: Option<&LiteralValue>,
) -> Result<String> {
    let type_str = lemma_type.to_string();

    match &lemma_type.specifications {
        TypeSpecification::Boolean { .. } => prompt_boolean_data(data_name, suggestion),
        TypeSpecification::Text {
            options,
            length,
            help,
            ..
        } => {
            if !options.is_empty() {
                if options.len() == 1 {
                    return Ok(options[0].clone());
                }
                let prompt_message = format!("{} [{}]", data_name, type_str);
                let mut prompt =
                    Select::new(&prompt_message, options.clone()).with_help_message(help.as_str());
                if let Some(lit) = suggestion {
                    if let ValueKind::Text(s) = &lit.value {
                        if let Some(idx) = options.iter().position(|o| o == s) {
                            prompt = prompt.with_starting_cursor(idx);
                        }
                    }
                }
                prompt
                    .prompt()
                    .context(format!("Failed to get option for {}", data_name))
            } else {
                let constraints = TextConstraints {
                    length: *length,
                    help: help.clone(),
                };
                prompt_text_data_with_constraints(
                    data_name,
                    &type_str,
                    lemma_type,
                    suggestion,
                    &constraints,
                )
            }
        }
        TypeSpecification::Measure {
            minimum,
            maximum,
            decimals,
            units,
            help,
            traits,
            decomposition,
            ..
        } => {
            let measure_spec = TypeSpecification::Measure {
                minimum: minimum.clone(),
                maximum: maximum.clone(),
                decimals: *decimals,
                units: units.clone(),
                traits: traits.clone(),
                decomposition: decomposition.clone(),
                help: help.clone(),
            };
            let constraints = NumericConstraints {
                minimum: measure_spec.minimum_decimal(),
                maximum: measure_spec.maximum_decimal(),
                decimals: *decimals,
                help: help.clone(),
            };
            prompt_measure_data(data_name, &type_str, suggestion, units, &constraints)
        }
        TypeSpecification::Number {
            minimum,
            maximum,
            decimals,
            help,
            ..
        } => {
            let number_spec = TypeSpecification::Number {
                minimum: minimum.clone(),
                maximum: maximum.clone(),
                decimals: *decimals,
                help: help.clone(),
            };
            let constraints = NumericConstraints {
                minimum: number_spec.minimum_decimal(),
                maximum: number_spec.maximum_decimal(),
                decimals: *decimals,
                help: help.clone(),
            };
            prompt_number_data(data_name, &type_str, suggestion, &constraints)
        }
        TypeSpecification::Ratio {
            minimum,
            maximum,
            decimals,
            units,
            help,
            ..
        } => {
            let ratio_spec = TypeSpecification::Ratio {
                minimum: minimum.clone(),
                maximum: maximum.clone(),
                decimals: *decimals,
                units: units.clone(),
                help: help.clone(),
            };
            prompt_ratio_data(
                data_name,
                &type_str,
                suggestion,
                units,
                ratio_spec.minimum_decimal(),
                ratio_spec.maximum_decimal(),
                help.as_str(),
            )
        }
        TypeSpecification::Date { .. } => prompt_date_data(data_name, suggestion),
        TypeSpecification::Time { help, .. } => {
            let def = suggestion
                .filter(|l| matches!(l.value, ValueKind::Time(_)))
                .map(|l| l.to_string());
            prompt_simple_text(data_name, &type_str, def.as_deref(), help.as_str(), "12:34:56")
        }
        TypeSpecification::NumberRange { help, .. }
        | TypeSpecification::DateRange { help, .. }
        | TypeSpecification::TimeRange { help, .. }
        | TypeSpecification::MeasureRange { help, .. }
        | TypeSpecification::RatioRange { help, .. } => {
            prompt_range_data(data_name, &type_str, lemma_type, suggestion, help.as_str())
        }
        TypeSpecification::Veto { .. } => {
            anyhow::bail!("Data '{}' has veto type which is not promptable", data_name)
        }
        TypeSpecification::Undetermined => unreachable!(
            "BUG: prompt_value_for_type called with Error sentinel type; this type must never reach interactive mode"
        ),
    }
}

fn prompt_date_data(data_name: &str, suggestion: Option<&LiteralValue>) -> Result<String> {
    let help_message = if suggestion.is_some() {
        "Use arrow keys to navigate, Enter to select (or accept suggestion)"
    } else {
        "Use arrow keys to navigate, Enter to select"
    };

    let prompt_title = format!("{} [date]", data_name);
    let mut ds = DateSelect::new(&prompt_title).with_help_message(help_message);

    if let Some(lit) = suggestion {
        if let ValueKind::Date(d) = &lit.value {
            if let Some(naive) = NaiveDate::from_ymd_opt(d.year, d.month, d.day) {
                ds = ds.with_default(naive);
            }
        }
    }

    let date = ds
        .prompt()
        .context(format!("Failed to get date for {}", data_name))?;

    Ok(format!("{}T00:00:00Z", date.format("%Y-%m-%d")))
}

fn prompt_boolean_data(data_name: &str, suggestion: Option<&LiteralValue>) -> Result<String> {
    let options = vec!["true", "false"];

    let default_index = match suggestion.and_then(|lit| match &lit.value {
        ValueKind::Boolean(b) => Some(*b),
        _ => None,
    }) {
        Some(true) => 0,
        Some(false) => 1,
        None => 0,
    };

    let help_message = if suggestion.is_some() {
        format!(
            "Default: {} - Use arrow keys to change, Enter to confirm",
            options[default_index]
        )
    } else {
        "Use arrow keys to select, Enter to confirm".to_string()
    };

    let selected = Select::new(&format!("{} [boolean]", data_name), options)
        .with_help_message(&help_message)
        .with_starting_cursor(default_index)
        .prompt()
        .context(format!("Failed to get boolean value for {}", data_name))?;

    Ok(selected.to_string())
}

fn prompt_text_data_with_constraints(
    data_name: &str,
    type_str: &str,
    lemma_type: &LemmaType,
    suggestion: Option<&LiteralValue>,
    constraints: &TextConstraints,
) -> Result<String> {
    let default_str = suggestion.map(|l| l.to_string());
    let prompt_message = format!("{} [{}]", data_name, type_str);
    let example = lemma_type.example_value();

    let TextConstraints { length, help } = constraints.clone();

    let validator = move |input: &str| {
        let s = input.trim();
        if s.is_empty() {
            return Ok(Validation::Invalid("Value is required".into()));
        }
        if let Some(len) = length {
            if s.chars().count() != len {
                return Ok(Validation::Invalid(
                    format!("Must be exactly {} characters", len).into(),
                ));
            }
        }
        Ok(Validation::Valid)
    };

    let mut prompt = Text::new(&prompt_message).with_validator(validator);
    let help_message = if help.is_empty() {
        format!("Example: {}", example)
    } else {
        help.clone()
    };
    prompt = prompt.with_help_message(&help_message);

    if let Some(default) = default_str.as_deref() {
        prompt = prompt.with_default(default);
    }

    prompt
        .prompt()
        .context(format!("Failed to get value for {}", data_name))
}

fn prompt_simple_text(
    data_name: &str,
    type_str: &str,
    default_value: Option<&str>,
    help: &str,
    example: &str,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", data_name, type_str);
    let validator = |input: &str| {
        if input.trim().is_empty() {
            Ok(Validation::Invalid("Value is required".into()))
        } else {
            Ok(Validation::Valid)
        }
    };
    let mut prompt = Text::new(&prompt_message).with_validator(validator);
    let help_message = if help.is_empty() {
        format!("Example: {}", example)
    } else {
        help.to_string()
    };
    prompt = prompt.with_help_message(&help_message);
    if let Some(default) = default_value {
        prompt = prompt.with_default(default);
    }
    prompt
        .prompt()
        .context(format!("Failed to get value for {}", data_name))
}

fn prompt_range_data(
    data_name: &str,
    type_str: &str,
    lemma_type: &LemmaType,
    suggestion: Option<&LiteralValue>,
    help: &str,
) -> Result<String> {
    let (left_default, right_default) = match suggestion {
        Some(LiteralValue {
            value: ValueKind::Range(left, right),
            ..
        }) => (Some(left.display_value()), Some(right.display_value())),
        _ => (None, None),
    };

    let endpoint_example = match &lemma_type.specifications {
        TypeSpecification::DateRange { .. } => "2024-01-01",
        TypeSpecification::TimeRange { .. } => "09:00",
        TypeSpecification::NumberRange { .. } => "0",
        TypeSpecification::MeasureRange { .. } => "30 kilogram",
        TypeSpecification::RatioRange { .. } => "10%",
        _ => unreachable!("BUG: prompt_range_data called with non-range type"),
    };

    let left_value = prompt_simple_text(
        &format!("{}.start", data_name),
        type_str,
        left_default.as_deref(),
        help,
        endpoint_example,
    )?;
    let right_value = prompt_simple_text(
        &format!("{}.end", data_name),
        type_str,
        right_default.as_deref(),
        help,
        endpoint_example,
    )?;

    Ok(format!("{}...{}", left_value.trim(), right_value.trim()))
}

fn prompt_number_data(
    data_name: &str,
    type_str: &str,
    suggestion: Option<&LiteralValue>,
    constraints: &NumericConstraints,
) -> Result<String> {
    let default_str = suggestion.map(|l| l.to_string());
    let prompt_message = format!("{} [{}]", data_name, type_str);
    prompt_decimal_input(&prompt_message, default_str.as_deref(), constraints, "10")
}

fn prompt_measure_data(
    data_name: &str,
    type_str: &str,
    suggestion: Option<&LiteralValue>,
    units: &lemma::MeasureUnits,
    constraints: &NumericConstraints,
) -> Result<String> {
    let parsed = suggestion.and_then(|lit| match &lit.value {
        ValueKind::Measure(n, signature) => Some((
            n.clone(),
            signature.first().map(|(n, _)| n.as_str()).unwrap_or(""),
        )),
        _ => None,
    });

    let prompt_message = format!("{} [{}]", data_name, type_str);

    if units.is_empty() {
        let default_str = suggestion.and_then(|lit| lit.magnitude_suggestion_for_decimal_prompt());
        return prompt_decimal_input(&prompt_message, default_str.as_deref(), constraints, "7.65");
    }

    let unit_names: Vec<String> = units.iter().map(|u| u.name.clone()).collect();
    let unit = if unit_names.len() == 1 {
        unit_names[0].clone()
    } else {
        let prompt_msg = format!("Select unit for {}", data_name);
        let mut select = Select::new(&prompt_msg, unit_names);
        if let Some((_, def_unit)) = parsed {
            if let Some(idx) = units.iter().position(|u| u.name == def_unit) {
                select = select.with_starting_cursor(idx);
            }
        }
        select
            .prompt()
            .context(format!("Failed to get unit for {}", data_name))?
    };

    let numeric_default = suggestion.and_then(|lit| lit.magnitude_in_unit(&unit));

    let value_constraints = NumericConstraints {
        help: if constraints.help.is_empty() {
            format!("Enter numeric value (unit: {})", unit)
        } else {
            constraints.help.clone()
        },
        ..constraints.clone()
    };
    let value = prompt_decimal_input(
        &format!("Enter value for {} ({})", data_name, unit),
        numeric_default.as_deref(),
        &value_constraints,
        "7.65",
    )?;

    Ok(format!("{} {}", value, unit))
}

fn prompt_ratio_data(
    data_name: &str,
    type_str: &str,
    suggestion: Option<&LiteralValue>,
    units: &lemma::RatioUnits,
    minimum: Option<Decimal>,
    maximum: Option<Decimal>,
    help: &str,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", data_name, type_str);
    let selected_unit = if units.len() == 1 {
        units
            .iter()
            .next()
            .map(|u| u.name.clone())
            .unwrap_or_else(|| "(none)".to_string())
    } else {
        let mut unit_choices: Vec<String> = vec!["(none)".to_string()];
        unit_choices.extend(units.iter().map(|u| u.name.clone()));
        Select::new(
            &format!("Select ratio unit for {}", data_name),
            unit_choices,
        )
        .prompt()
        .context(format!("Failed to get ratio unit for {}", data_name))?
    };
    let default_decimal = if selected_unit == "(none)" {
        suggestion.and_then(|lit| lit.magnitude_suggestion_for_decimal_prompt())
    } else {
        suggestion.and_then(|lit| lit.magnitude_in_unit(&selected_unit))
    };

    let value = prompt_decimal_input(
        &prompt_message,
        default_decimal.as_deref(),
        &NumericConstraints {
            minimum,
            maximum,
            decimals: None,
            help: help.to_string(),
        },
        "0.10",
    )?;

    match selected_unit.as_str() {
        "(none)" => Ok(value),
        "percent" => Ok(format!("{}%", value)),
        "permille" => Ok(format!("{}%%", value)),
        other => Ok(format!("{} {}", value, other)),
    }
}

fn prompt_decimal_input(
    prompt_message: &str,
    default_value: Option<&str>,
    constraints: &NumericConstraints,
    example: &str,
) -> Result<String> {
    let NumericConstraints {
        minimum: min,
        maximum: max,
        decimals: decs,
        help,
    } = constraints.clone();

    let validator = move |input: &str| {
        let raw = input.trim();
        if raw.is_empty() {
            return Ok(Validation::Invalid("Value is required".into()));
        }
        let clean = raw.replace(['_', ','], "");
        let provided_decimals = clean
            .split_once('.')
            .map(|(_, frac)| frac.len())
            .unwrap_or(0);
        if let Some(d) = decs {
            if provided_decimals > d as usize {
                return Ok(Validation::Invalid(
                    format!("Too many decimals (max {})", d).into(),
                ));
            }
        }
        let value = match Decimal::from_str_exact(&clean) {
            Ok(v) => v,
            Err(_) => {
                return Ok(Validation::Invalid(
                    format!("Invalid number: '{}'", raw).into(),
                ))
            }
        };
        if let Some(min) = min {
            if value < min {
                return Ok(Validation::Invalid(format!("Must be >= {}", min).into()));
            }
        }
        if let Some(max) = max {
            if value > max {
                return Ok(Validation::Invalid(format!("Must be <= {}", max).into()));
            }
        }
        Ok(Validation::Valid)
    };

    let mut prompt = Text::new(prompt_message).with_validator(validator);
    let help_message = if help.is_empty() {
        format!("Example: {}", example)
    } else {
        help.clone()
    };
    prompt = prompt.with_help_message(&help_message);

    if let Some(default) = default_value {
        prompt = prompt.with_default(default);
    }

    let raw = prompt.prompt().context(format!(
        "Failed to get numeric value for {}",
        prompt_message
    ))?;
    Ok(raw.trim().replace(['_', ','], ""))
}
