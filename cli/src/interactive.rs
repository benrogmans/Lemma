use anyhow::{Context, Result};
use inquire::validator::Validation;
use inquire::{DateSelect, MultiSelect, Select, Text};
use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, LemmaType, TypeSpecification};
use rust_decimal::Decimal;
use std::collections::HashMap;

pub type InteractiveResult = (
    String,
    Option<Vec<String>>,
    HashMap<String, String>,
    Option<String>,
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

pub fn run_interactive(
    engine: &Engine,
    spec_name: Option<String>,
    rule_names: Option<Vec<String>>,
    provided_data: &HashMap<String, String>,
    target: Option<&String>,
    now: &DateTimeValue,
) -> Result<InteractiveResult> {
    let spec = match spec_name {
        Some(name) => name,
        None => select_spec(engine, now)?,
    };

    let rules = match rule_names {
        Some(names) => Some(names),
        None => select_rules(engine, &spec, now)?,
    };

    let data = prompt_data(engine, &spec, &rules, provided_data, now)?;

    let target = match target {
        Some(t) => Some(t.clone()),
        None => prompt_target(engine, &spec, &rules, now)?,
    };

    Ok((spec, rules, data, target))
}

fn select_spec(engine: &Engine, now: &DateTimeValue) -> Result<String> {
    let specs = engine.list_specs_effective(now);

    if specs.is_empty() {
        anyhow::bail!("No specs found in workspace. Add .lemma files to get started.");
    }

    if specs.len() == 1 {
        return Ok(specs
            .first()
            .ok_or_else(|| anyhow::anyhow!("Expected at least one spec"))?
            .name
            .clone());
    }

    let display_options: Vec<String> = specs
        .iter()
        .map(|spec| {
            let (data_count, rules_count) = engine
                .get_plan(&spec.name, Some(now))
                .ok()
                .map(|p| (p.data.len(), p.rules.len()))
                .unwrap_or((0, 0));
            format!("{} ({} data, {} rules)", spec.name, data_count, rules_count)
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

    Ok(specs[spec_index].name.clone())
}

fn select_rules(
    engine: &Engine,
    spec_name: &str,
    now: &DateTimeValue,
) -> Result<Option<Vec<String>>> {
    let plan = engine
        .get_plan(spec_name, Some(now))
        .map_err(|e| anyhow::anyhow!("{}", e))
        .context(format!("Spec '{}' not found", spec_name))?;
    let rule_names: Vec<String> = plan.schema().rules.keys().cloned().collect();

    if rule_names.is_empty() {
        return Ok(None);
    }

    if rule_names.len() == 1 {
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

fn prompt_target(
    engine: &Engine,
    spec_name: &str,
    rule_names: &Option<Vec<String>>,
    now: &DateTimeValue,
) -> Result<Option<String>> {
    use inquire::Confirm;

    if !Confirm::new("Do you want to invert a rule (find inputs for a target output)?")
        .with_default(false)
        .prompt()
        .context("Failed to get inversion preference")?
    {
        return Ok(None);
    }

    let plan = engine
        .get_plan(spec_name, Some(now))
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let available_rules: Vec<String> = plan
        .rules
        .iter()
        .filter(|r| r.path.segments.is_empty())
        .map(|r| r.name.clone())
        .collect();
    if available_rules.is_empty() {
        return Ok(None);
    }

    let rule_options: Vec<String> = if let Some(selected_rules) = rule_names {
        if selected_rules.len() == 1 {
            vec![selected_rules[0].clone()]
        } else {
            available_rules
        }
    } else {
        available_rules
    };

    if rule_options.is_empty() {
        return Ok(None);
    }

    let selected_rule = if rule_options.len() == 1 {
        rule_options[0].clone()
    } else {
        Select::new("Select rule to invert:", rule_options)
            .prompt()
            .context("Failed to select rule")?
    };

    let target_value = Text::new(&format!(
        "Enter target for {} (e.g., =100, >50, <200, =veto):",
        selected_rule
    ))
    .with_help_message("Format: =value, >value, <value, >=value, <=value, or =veto")
    .prompt()
    .context("Failed to get target value")?;

    if target_value.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(format!("{}={}", selected_rule, target_value.trim())))
}

fn prompt_data(
    engine: &Engine,
    spec_name: &str,
    _rule_names: &Option<Vec<String>>,
    provided_data: &HashMap<String, String>,
    now: &DateTimeValue,
) -> Result<HashMap<String, String>> {
    let plan = engine
        .get_plan(spec_name, Some(now))
        .map_err(|e| anyhow::anyhow!("{}", e))
        .context(format!("Spec '{}' not found", spec_name))?;

    // Full schema: types for any data the evaluator may still need (branch-aware).
    let full_schema = plan.schema();

    let mut collected = HashMap::new();
    let mut header_printed = false;

    loop {
        let mut merged = provided_data.clone();
        merged.extend(collected.clone());

        let response = engine
            .run_plan(plan, Some(now), merged, false)
            .map_err(|e| anyhow::anyhow!("{}", e))
            .context("Evaluation failed while resolving interactive data")?;

        let missing = response.missing_data_ordered();
        if missing.is_empty() {
            return Ok(collected);
        }

        let fp = missing
            .into_iter()
            .next()
            .expect("missing_data_ordered non-empty implies at least one path");
        let data_name = fp.input_key();

        if provided_data.contains_key(&data_name) || collected.contains_key(&data_name) {
            anyhow::bail!(
                "Engine reports missing data '{}' but a value was already provided",
                data_name
            );
        }

        let entry = full_schema
            .data
            .get(&data_name)
            .with_context(|| format!("Missing data '{}' has no entry in spec schema", data_name))?;
        let lemma_type = &entry.lemma_type;

        if entry.default.is_some() {
            anyhow::bail!(
                "BUG: data '{}' has a spec-defined value but evaluation reports it missing",
                data_name
            );
        }

        if !header_printed {
            println!("\nEnter values for data (press Enter to accept defaults):");
            header_printed = true;
        }

        loop {
            let input_value = prompt_value_for_type(&data_name, lemma_type, None)?;

            let mut trial = provided_data.clone();
            trial.extend(collected.clone());
            trial.insert(data_name.clone(), input_value.clone());

            match engine.run_plan(plan, Some(now), trial, false) {
                Ok(_) => {
                    collected.insert(data_name.clone(), input_value);
                    break;
                }
                Err(e) => {
                    eprintln!("  {}\n", e);
                }
            }
        }
    }
}

fn prompt_value_for_type(
    data_name: &str,
    lemma_type: &LemmaType,
    default_value: Option<&str>,
) -> Result<String> {
    let type_str = lemma_type.to_string();

    match &lemma_type.specifications {
        TypeSpecification::Boolean { .. } => prompt_boolean_data(data_name, default_value),
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
                if let Some(default) = default_value {
                    if let Some(idx) = options.iter().position(|o| o == default) {
                        prompt = prompt.with_starting_cursor(idx);
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
                    default_value,
                    &constraints,
                )
            }
        }
        TypeSpecification::Scale {
            minimum,
            maximum,
            decimals,
            units,
            help,
            ..
        } => {
            let constraints = NumericConstraints {
                minimum: *minimum,
                maximum: *maximum,
                decimals: *decimals,
                help: help.clone(),
            };
            prompt_scale_data(data_name, &type_str, None, units, &constraints)
        }
        TypeSpecification::Number {
            minimum,
            maximum,
            decimals,
            help,
            ..
        } => {
            let constraints = NumericConstraints {
                minimum: *minimum,
                maximum: *maximum,
                decimals: *decimals,
                help: help.clone(),
            };
            prompt_number_data(data_name, &type_str, default_value, &constraints)
        }
        TypeSpecification::Ratio {
            minimum,
            maximum,
            units,
            help,
            ..
        } => prompt_ratio_data(
            data_name,
            &type_str,
            default_value,
            units,
            *minimum,
            *maximum,
            help.as_str(),
        ),
        TypeSpecification::Date { .. } => prompt_date_data(data_name, default_value),
        TypeSpecification::Time { help, .. } => prompt_simple_text(
            data_name,
            &type_str,
            default_value,
            help.as_str(),
            "12:34:56",
        ),
        TypeSpecification::Duration { help, .. } => {
            prompt_duration_data(data_name, &type_str, default_value, help.as_str())
        }
        TypeSpecification::Veto { .. } => {
            anyhow::bail!("Data '{}' has veto type which is not promptable", data_name)
        }
        TypeSpecification::Undetermined => unreachable!(
            "BUG: prompt_value_for_type called with Error sentinel type; this type must never reach interactive mode"
        ),
    }
}

fn prompt_date_data(data_name: &str, default_value: Option<&str>) -> Result<String> {
    let help_message = if default_value.is_some() {
        "Use arrow keys to navigate, Enter to select (or accept default)"
    } else {
        "Use arrow keys to navigate, Enter to select"
    };

    let date = DateSelect::new(&format!("{} [date]", data_name))
        .with_help_message(help_message)
        .prompt()
        .context(format!("Failed to get date for {}", data_name))?;

    Ok(format!("{}T00:00:00Z", date.format("%Y-%m-%d")))
}

fn prompt_boolean_data(data_name: &str, default_value: Option<&str>) -> Result<String> {
    let options = vec!["true", "false"];

    let default_index = match default_value {
        Some(default) if default == "true" || default == "yes" || default == "accept" => 0,
        Some(_) => 1,
        None => 0,
    };

    let help_message = if default_value.is_some() {
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
    default_value: Option<&str>,
    constraints: &TextConstraints,
) -> Result<String> {
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

    if let Some(default) = default_value {
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

fn prompt_number_data(
    data_name: &str,
    type_str: &str,
    default_value: Option<&str>,
    constraints: &NumericConstraints,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", data_name, type_str);
    prompt_decimal_input(&prompt_message, default_value, constraints, "10")
}

fn prompt_scale_data(
    data_name: &str,
    type_str: &str,
    default: Option<&(Decimal, String)>,
    units: &lemma::ScaleUnits,
    constraints: &NumericConstraints,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", data_name, type_str);

    if units.is_empty() {
        let default_str = default.map(|(v, _)| v.to_string());
        return prompt_decimal_input(&prompt_message, default_str.as_deref(), constraints, "7.65");
    }

    let unit_names: Vec<String> = units.iter().map(|u| u.name.clone()).collect();
    let unit = if unit_names.len() == 1 {
        unit_names[0].clone()
    } else {
        let prompt_msg = format!("Select unit for {}", data_name);
        let mut select = Select::new(&prompt_msg, unit_names);
        if let Some((_, def_unit)) = default {
            if let Some(idx) = units.iter().position(|u| u.name == *def_unit) {
                select = select.with_starting_cursor(idx);
            }
        }
        select
            .prompt()
            .context(format!("Failed to get unit for {}", data_name))?
    };

    let numeric_default = default.and_then(|(value, def_unit)| {
        let from = units.get(def_unit).ok()?;
        let to = units.get(&unit).ok()?;
        Some((value * from.value / to.value).to_string())
    });

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
    default_value: Option<&str>,
    units: &lemma::RatioUnits,
    minimum: Option<Decimal>,
    maximum: Option<Decimal>,
    help: &str,
) -> Result<String> {
    // Ratio types typically support percent/permille; offer a unit selector.
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

    let value = prompt_decimal_input(
        &prompt_message,
        default_value,
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

fn prompt_duration_data(
    data_name: &str,
    type_str: &str,
    default_value: Option<&str>,
    help: &str,
) -> Result<String> {
    // If there is a default, let the user accept it.
    if let Some(default) = default_value {
        let prompt_message = format!("{} [{}]", data_name, type_str);
        let help_message = if help.is_empty() {
            "Example: 5 days".to_string()
        } else {
            help.to_string()
        };
        return Text::new(&prompt_message)
            .with_help_message(&help_message)
            .with_default(default)
            .prompt()
            .context(format!("Failed to get duration for {}", data_name));
    }

    // Otherwise, guide the user with a unit selector.
    let units = vec![
        "milliseconds",
        "seconds",
        "minutes",
        "hours",
        "days",
        "weeks",
        "months",
        "years",
    ];
    let unit = Select::new(&format!("Select duration unit for {}", data_name), units)
        .prompt()
        .context(format!("Failed to get duration unit for {}", data_name))?;

    let value = prompt_decimal_input(
        &format!("Enter duration value for {}", data_name),
        None,
        &NumericConstraints {
            minimum: None,
            maximum: None,
            decimals: None,
            help: help.to_string(),
        },
        "5",
    )?;

    Ok(format!("{} {}", value, unit))
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
