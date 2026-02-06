use anyhow::{Context, Result};
use inquire::validator::Validation;
use inquire::{DateSelect, MultiSelect, Select, Text};
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
    minimum: Option<usize>,
    maximum: Option<usize>,
    length: Option<usize>,
    help: Option<String>,
}

#[derive(Clone, Debug)]
struct NumericConstraints {
    minimum: Option<Decimal>,
    maximum: Option<Decimal>,
    decimals: Option<u8>,
    help: Option<String>,
}

pub fn run_interactive(
    engine: &Engine,
    doc_name: Option<String>,
    rule_names: Option<Vec<String>>,
    provided_facts: &HashMap<String, String>,
) -> Result<InteractiveResult> {
    let doc = match doc_name {
        Some(name) => name,
        None => select_document(engine)?,
    };

    let rules = match rule_names {
        Some(names) => Some(names),
        None => select_rules(engine, &doc)?,
    };

    // let target = prompt_target(engine, &doc, &rules)?;
    let target = None;
    let facts = prompt_facts(engine, &doc, &rules, provided_facts)?;

    Ok((doc, rules, facts, target))
}

fn select_document(engine: &Engine) -> Result<String> {
    let documents = engine.list_documents();

    if documents.is_empty() {
        anyhow::bail!("No documents found in workspace. Add .lemma files to get started.");
    }

    if documents.len() == 1 {
        return Ok(documents
            .first()
            .ok_or_else(|| anyhow::anyhow!("Expected at least one document"))?
            .clone());
    }

    let display_options: Vec<String> = documents
        .iter()
        .map(|doc_name| {
            let (facts_count, rules_count) = engine
                .get_execution_plan(doc_name)
                .map(|p| (p.facts.len(), p.rules.len()))
                .unwrap_or((0, 0));
            format!(
                "{} ({} facts, {} rules)",
                doc_name, facts_count, rules_count
            )
        })
        .collect();

    let selected = Select::new("Select a document:", display_options.clone())
        .with_help_message("Use arrow keys to navigate, Enter to select")
        .prompt()
        .context("Failed to get document selection")?;

    let doc_index = display_options
        .iter()
        .position(|d| d == &selected)
        .context("Failed to find selected document index")?;

    Ok(documents[doc_index].clone())
}

fn select_rules(engine: &Engine, doc_name: &str) -> Result<Option<Vec<String>>> {
    let rule_names = engine.get_document_rule_names(doc_name);

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

fn _prompt_target(
    engine: &Engine,
    doc_name: &str,
    rule_names: &Option<Vec<String>>,
) -> Result<Option<String>> {
    use inquire::Confirm;

    let wants_inversion =
        Confirm::new("Do you want to invert a rule (find inputs for a target output)?")
            .with_default(false)
            .prompt()
            .context("Failed to get inversion preference")?;

    if !wants_inversion {
        return Ok(None);
    }

    let available_rules = engine.get_document_rules(doc_name);
    if available_rules.is_empty() {
        return Ok(None);
    }

    let rule_options: Vec<String> = if let Some(selected_rules) = rule_names {
        if selected_rules.len() == 1 {
            vec![selected_rules[0].clone()]
        } else {
            available_rules.iter().map(|r| r.name.clone()).collect()
        }
    } else {
        available_rules.iter().map(|r| r.name.clone()).collect()
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

fn prompt_facts(
    engine: &Engine,
    doc_name: &str,
    rule_names: &Option<Vec<String>>,
    provided_facts: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let selected_rules: Vec<String> = rule_names.clone().unwrap_or_default();
    let necessary_facts = engine
        .get_facts(doc_name, &selected_rules)
        .context("Failed to get facts")?;

    let promptable_facts: Vec<_> = necessary_facts
        .into_iter()
        .filter(|(path, _)| !provided_facts.contains_key(&path.to_string()))
        .collect();

    if promptable_facts.is_empty() {
        return Ok(HashMap::new());
    }

    let mut facts = HashMap::new();

    println!("\nEnter values for facts (press Enter to accept defaults):");

    for (fact_path, lemma_type) in promptable_facts {
        let fact_name = fact_path.to_string();
        let default_value = default_value_from_type(&lemma_type);

        let input_value = prompt_value_for_type(&fact_name, &lemma_type, default_value.as_deref())?;
        facts.insert(fact_name, input_value);
    }

    Ok(facts)
}

fn default_value_from_type(lemma_type: &LemmaType) -> Option<String> {
    match &lemma_type.specifications {
        TypeSpecification::Boolean { default, .. } => default.map(|b| b.to_string()),
        TypeSpecification::Scale { default, .. } => default
            .as_ref()
            .map(|(value, unit)| format!("{} {}", value, unit)),
        TypeSpecification::Number { default, .. } => default.map(|d| d.to_string()),
        TypeSpecification::Ratio { default, .. } => default.map(|d| d.to_string()),
        TypeSpecification::Text { default, .. } => default.clone(),
        TypeSpecification::Date { default, .. } => default.as_ref().map(|d| d.to_string()),
        TypeSpecification::Time { default, .. } => default.as_ref().map(|t| t.to_string()),
        TypeSpecification::Duration { default, .. } => {
            default.as_ref().map(|(v, u)| format!("{} {}", v, u))
        }
        TypeSpecification::Veto { .. } => None,
    }
}

fn prompt_value_for_type(
    fact_name: &str,
    lemma_type: &LemmaType,
    default_value: Option<&str>,
) -> Result<String> {
    let type_str = lemma_type.to_string();

    match &lemma_type.specifications {
        TypeSpecification::Boolean { default, .. } => {
            let default_str = default_value
                .map(|s| s.to_string())
                .or_else(|| default.map(|b| b.to_string()));
            prompt_boolean_fact(fact_name, default_str.as_deref())
        }
        TypeSpecification::Text {
            options,
            minimum,
            maximum,
            length,
            help,
            ..
        } => {
            if !options.is_empty() {
                let prompt_message = format!("{} [{}]", fact_name, type_str);
                let mut prompt = Select::new(&prompt_message, options.clone());
                if let Some(help) = help.as_ref() {
                    prompt = prompt.with_help_message(help);
                }
                if let Some(default) = default_value {
                    if let Some(idx) = options.iter().position(|o| o == default) {
                        prompt = prompt.with_starting_cursor(idx);
                    }
                }
                prompt
                    .prompt()
                    .context(format!("Failed to get option for {}", fact_name))
            } else {
                let constraints = TextConstraints {
                    minimum: *minimum,
                    maximum: *maximum,
                    length: *length,
                    help: help.clone(),
                };
                prompt_text_fact_with_constraints(
                    fact_name,
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
            prompt_scale_fact(fact_name, &type_str, default_value, units, &constraints)
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
            prompt_number_fact(fact_name, &type_str, default_value, &constraints)
        }
        TypeSpecification::Ratio {
            minimum,
            maximum,
            units,
            help,
            ..
        } => prompt_ratio_fact(
            fact_name,
            &type_str,
            default_value,
            units,
            *minimum,
            *maximum,
            help.as_deref(),
        ),
        TypeSpecification::Date { .. } => prompt_date_fact(fact_name, default_value),
        TypeSpecification::Time { help, .. } => prompt_simple_text(
            fact_name,
            &type_str,
            default_value,
            help.as_deref(),
            "12:34:56",
        ),
        TypeSpecification::Duration { help, .. } => {
            prompt_duration_fact(fact_name, &type_str, default_value, help.as_deref())
        }
        TypeSpecification::Veto { .. } => {
            anyhow::bail!("Fact '{}' has veto type which is not promptable", fact_name)
        }
    }
}

fn prompt_date_fact(fact_name: &str, default_value: Option<&str>) -> Result<String> {
    let help_message = if default_value.is_some() {
        "Use arrow keys to navigate, Enter to select (or accept default)"
    } else {
        "Use arrow keys to navigate, Enter to select"
    };

    let date = DateSelect::new(&format!("{} [date]", fact_name))
        .with_help_message(help_message)
        .prompt()
        .context(format!("Failed to get date for {}", fact_name))?;

    Ok(format!("{}T00:00:00Z", date.format("%Y-%m-%d")))
}

fn prompt_boolean_fact(fact_name: &str, default_value: Option<&str>) -> Result<String> {
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

    let selected = Select::new(&format!("{} [boolean]", fact_name), options)
        .with_help_message(&help_message)
        .with_starting_cursor(default_index)
        .prompt()
        .context(format!("Failed to get boolean value for {}", fact_name))?;

    Ok(selected.to_string())
}

fn prompt_text_fact_with_constraints(
    fact_name: &str,
    type_str: &str,
    lemma_type: &LemmaType,
    default_value: Option<&str>,
    constraints: &TextConstraints,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", fact_name, type_str);
    let example = lemma_type.example_value();

    let TextConstraints {
        minimum,
        maximum,
        length,
        help,
    } = constraints.clone();

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
        if let Some(min) = minimum {
            if s.chars().count() < min {
                return Ok(Validation::Invalid(
                    format!("Must be at least {} characters", min).into(),
                ));
            }
        }
        if let Some(max) = maximum {
            if s.chars().count() > max {
                return Ok(Validation::Invalid(
                    format!("Must be at most {} characters", max).into(),
                ));
            }
        }
        Ok(Validation::Valid)
    };

    let mut prompt = Text::new(&prompt_message).with_validator(validator);
    let help_message = help.unwrap_or_else(|| format!("Example: {}", example));
    prompt = prompt.with_help_message(&help_message);

    if let Some(default) = default_value {
        prompt = prompt.with_default(default);
    }

    prompt
        .prompt()
        .context(format!("Failed to get value for {}", fact_name))
}

fn prompt_simple_text(
    fact_name: &str,
    type_str: &str,
    default_value: Option<&str>,
    help: Option<&str>,
    example: &str,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", fact_name, type_str);
    let validator = |input: &str| {
        if input.trim().is_empty() {
            Ok(Validation::Invalid("Value is required".into()))
        } else {
            Ok(Validation::Valid)
        }
    };
    let mut prompt = Text::new(&prompt_message).with_validator(validator);
    let help_message = help
        .map(|h| h.to_string())
        .unwrap_or_else(|| format!("Example: {}", example));
    prompt = prompt.with_help_message(&help_message);
    if let Some(default) = default_value {
        prompt = prompt.with_default(default);
    }
    prompt
        .prompt()
        .context(format!("Failed to get value for {}", fact_name))
}

fn prompt_number_fact(
    fact_name: &str,
    type_str: &str,
    default_value: Option<&str>,
    constraints: &NumericConstraints,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", fact_name, type_str);
    prompt_decimal_input(&prompt_message, default_value, constraints, "10")
}

fn prompt_scale_fact(
    fact_name: &str,
    type_str: &str,
    default_value: Option<&str>,
    units: &lemma::ScaleUnits,
    constraints: &NumericConstraints,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", fact_name, type_str);

    if units.is_empty() {
        return prompt_decimal_input(&prompt_message, default_value, constraints, "7.65");
    }

    let unit_names: Vec<String> = units.iter().map(|u| u.name.clone()).collect();
    let unit = Select::new(&format!("Select unit for {}", fact_name), unit_names)
        .prompt()
        .context(format!("Failed to get unit for {}", fact_name))?;

    let value = prompt_decimal_input(
        &format!("Enter value for {} ({})", fact_name, unit),
        None,
        constraints,
        "7.65",
    )?;

    Ok(format!("{} {}", value, unit))
}

fn prompt_ratio_fact(
    fact_name: &str,
    type_str: &str,
    default_value: Option<&str>,
    units: &lemma::RatioUnits,
    minimum: Option<Decimal>,
    maximum: Option<Decimal>,
    help: Option<&str>,
) -> Result<String> {
    // Ratio types typically support percent/permille; offer a unit selector.
    let prompt_message = format!("{} [{}]", fact_name, type_str);
    let mut unit_choices: Vec<String> = vec!["(none)".to_string()];
    unit_choices.extend(units.iter().map(|u| u.name.clone()));

    let selected_unit = Select::new(
        &format!("Select ratio unit for {}", fact_name),
        unit_choices,
    )
    .prompt()
    .context(format!("Failed to get ratio unit for {}", fact_name))?;

    let value = prompt_decimal_input(
        &prompt_message,
        default_value,
        &NumericConstraints {
            minimum,
            maximum,
            decimals: None,
            help: help.map(|h| h.to_string()),
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

fn prompt_duration_fact(
    fact_name: &str,
    type_str: &str,
    default_value: Option<&str>,
    help: Option<&str>,
) -> Result<String> {
    // If there is a default, let the user accept it.
    if let Some(default) = default_value {
        let prompt_message = format!("{} [{}]", fact_name, type_str);
        let mut prompt = Text::new(&prompt_message);
        if let Some(help) = help {
            prompt = prompt.with_help_message(help);
        } else {
            prompt = prompt.with_help_message("Example: 5 days");
        }
        return prompt
            .with_default(default)
            .prompt()
            .context(format!("Failed to get duration for {}", fact_name));
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
    let unit = Select::new(&format!("Select duration unit for {}", fact_name), units)
        .prompt()
        .context(format!("Failed to get duration unit for {}", fact_name))?;

    let value = prompt_decimal_input(
        &format!("Enter duration value for {}", fact_name),
        None,
        &NumericConstraints {
            minimum: None,
            maximum: None,
            decimals: None,
            help: help.map(|h| h.to_string()),
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
    let help_message = help.unwrap_or_else(|| format!("Example: {}", example));
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
