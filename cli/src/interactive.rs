use anyhow::{Context, Result};
use chrono::NaiveDate;
use inquire::validator::Validation;
use inquire::{DateSelect, MultiSelect, Select, Text};
use lemma::parsing::ast::{DateTimeValue, LemmaRepository, LemmaSpec};
use lemma::{checked_mul, commit_rational_to_decimal, rational_to_display_str, RationalInteger};
use lemma::{EffectiveDate, Engine, LemmaType, LiteralValue, TypeSpecification, ValueKind};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) fn repo_label(repository: &LemmaRepository) -> String {
    match &repository.name {
        Some(name) => name.clone(),
        None => "(workspace)".to_string(),
    }
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

fn optional_rational_bound_for_prompt(bound: Option<RationalInteger>) -> Option<Decimal> {
    bound.and_then(|rational| commit_rational_to_decimal(&rational).ok())
}

fn optional_quantity_bound_for_prompt(
    bound: &Option<(RationalInteger, String)>,
) -> Option<Decimal> {
    optional_rational_bound_for_prompt(bound.as_ref().map(|(rational, _)| *rational))
}

fn rational_default_string(rational: &RationalInteger) -> String {
    rational_to_display_str(rational)
}

fn get_plan_for_interactive<'a>(
    engine: &'a Engine,
    repo: Option<&str>,
    name: &str,
    now: &DateTimeValue,
) -> Result<&'a lemma::ExecutionPlan> {
    engine
        .get_plan(repo, name, Some(now))
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
            // Validate the spec exists (will error on ambiguity when repository is None).
            get_plan_for_interactive(engine, cli_repository_qualifier, &name, now)?;
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

fn select_spec(
    engine: &Engine,
    now: &DateTimeValue,
    cli_repository_qualifier: Option<&str>,
) -> Result<(Option<String>, String)> {
    let effective_instant = EffectiveDate::DateTimeValue(now.clone());
    let mut pairs: Vec<(Arc<LemmaRepository>, Arc<LemmaSpec>)> = engine
        .list()
        .iter()
        .flat_map(|repo| {
            let repo_arc = Arc::clone(&repo.repository);
            let instant = effective_instant.clone();
            repo.specs.iter().filter_map(move |ss| {
                ss.spec_at(&instant)
                    .map(|spec| (Arc::clone(&repo_arc), spec))
            })
        })
        .collect();
    if let Some(q) = cli_repository_qualifier {
        let resolved = engine
            .get_repository(q)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        pairs.retain(|(repo, _)| Arc::ptr_eq(repo, &resolved.repository));
    }

    if pairs.is_empty() {
        anyhow::bail!("No specs found in workspace. Add .lemma files to get started.");
    }

    let needs_repo_qualifier = {
        let mut names: Vec<&str> = pairs.iter().map(|(_, s)| s.name.as_str()).collect();
        names.sort();
        names.windows(2).any(|w| w[0] == w[1])
    };

    let display_options: Vec<String> = pairs
        .iter()
        .map(|(repo, spec)| {
            let label = repo_label(repo);
            let rq = if needs_repo_qualifier {
                Some(label.as_str())
            } else {
                cli_repository_qualifier
            };
            let (data_count, rules_count) = get_plan_for_interactive(engine, rq, &spec.name, now)
                .ok()
                .map(|p| (p.data.len(), p.rules.len()))
                .unwrap_or((0, 0));
            format!(
                "{} ({}) — {} data, {} rules",
                spec.name, label, data_count, rules_count
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

    let (r, s) = pairs
        .into_iter()
        .nth(spec_index)
        .context("Failed to match selected spec")?;
    let rq = if needs_repo_qualifier {
        Some(repo_label(&r))
    } else {
        cli_repository_qualifier.map(String::from)
    };
    Ok((rq, s.name.clone()))
}

fn select_rules(
    engine: &Engine,
    repo: Option<&str>,
    spec_name: &str,
    now: &DateTimeValue,
) -> Result<Option<Vec<String>>> {
    let plan = get_plan_for_interactive(engine, repo, spec_name, now)
        .context(format!("Spec '{}' not found", spec_name))?;
    let rule_names: Vec<String> = plan.schema().rules.keys().cloned().collect();

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
    let plan = get_plan_for_interactive(engine, repo, spec_name, now)
        .context(format!("Spec '{}' not found", spec_name))?;

    let full_schema = match rule_names {
        Some(names) if !names.is_empty() => plan
            .schema_for_rules(names)
            .map_err(|e| anyhow::anyhow!("{}", e))
            .context("Failed to build schema for selected rules")?,
        _ => plan.schema(),
    };

    let mut collected = HashMap::new();
    let mut header_printed = false;

    for (name, entry) in &full_schema.data {
        if entry.bound_value.is_some() || provided_data.contains_key(name) {
            continue;
        }

        if !header_printed {
            println!("\nEnter values for data (press Enter to accept defaults):");
            header_printed = true;
        }

        loop {
            let input_value =
                prompt_value_for_type(name, &entry.lemma_type, entry.default.as_ref())?;

            let mut trial = provided_data.clone();
            trial.extend(collected.clone());
            trial.insert(name.clone(), input_value.clone());

            match engine.run_plan_without_defaults(
                plan,
                Some(now),
                lemma::serialization::data_values_from_strings(trial),
                false,
                lemma::EvaluationRequest::default(),
            ) {
                Ok(_) => {
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

fn prompt_value_for_type(
    data_name: &str,
    lemma_type: &LemmaType,
    schema_default: Option<&LiteralValue>,
) -> Result<String> {
    let type_str = lemma_type.to_string();

    match &lemma_type.specifications {
        TypeSpecification::Boolean { .. } => prompt_boolean_data(data_name, schema_default),
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
                if let Some(lit) = schema_default {
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
                    schema_default,
                    &constraints,
                )
            }
        }
        TypeSpecification::Quantity {
            minimum,
            maximum,
            decimals,
            units,
            help,
            ..
        } => {
            let constraints = NumericConstraints {
                minimum: optional_quantity_bound_for_prompt(minimum),
                maximum: optional_quantity_bound_for_prompt(maximum),
                decimals: *decimals,
                help: help.clone(),
            };
            prompt_quantity_data(data_name, &type_str, schema_default, units, &constraints)
        }
        TypeSpecification::Number {
            minimum,
            maximum,
            decimals,
            help,
            ..
        } => {
            let constraints = NumericConstraints {
                minimum: optional_rational_bound_for_prompt(*minimum),
                maximum: optional_rational_bound_for_prompt(*maximum),
                decimals: *decimals,
                help: help.clone(),
            };
            prompt_number_data(data_name, &type_str, schema_default, &constraints)
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
            schema_default,
            units,
            optional_rational_bound_for_prompt(*minimum),
            optional_rational_bound_for_prompt(*maximum),
            help.as_str(),
        ),
        TypeSpecification::Date { .. } => prompt_date_data(data_name, schema_default),
        TypeSpecification::Time { help, .. } => {
            let def = schema_default
                .filter(|l| matches!(l.value, ValueKind::Time(_)))
                .map(|l| l.to_string());
            prompt_simple_text(data_name, &type_str, def.as_deref(), help.as_str(), "12:34:56")
        }
        TypeSpecification::Calendar { help, .. } => {
            prompt_calendar_data(data_name, &type_str, schema_default, help.as_str())
        }
        TypeSpecification::NumberRange { help, .. }
        | TypeSpecification::DateRange { help, .. }
        | TypeSpecification::QuantityRange { help, .. }
        | TypeSpecification::RatioRange { help, .. }
        | TypeSpecification::CalendarRange { help, .. } => {
            prompt_range_data(data_name, &type_str, lemma_type, schema_default, help.as_str())
        }
        TypeSpecification::Veto { .. } => {
            anyhow::bail!("Data '{}' has veto type which is not promptable", data_name)
        }
        TypeSpecification::Undetermined => unreachable!(
            "BUG: prompt_value_for_type called with Error sentinel type; this type must never reach interactive mode"
        ),
    }
}

fn prompt_date_data(data_name: &str, schema_default: Option<&LiteralValue>) -> Result<String> {
    let help_message = if schema_default.is_some() {
        "Use arrow keys to navigate, Enter to select (or accept default)"
    } else {
        "Use arrow keys to navigate, Enter to select"
    };

    let prompt_title = format!("{} [date]", data_name);
    let mut ds = DateSelect::new(&prompt_title).with_help_message(help_message);

    if let Some(lit) = schema_default {
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

fn prompt_boolean_data(data_name: &str, schema_default: Option<&LiteralValue>) -> Result<String> {
    let options = vec!["true", "false"];

    let default_index = match schema_default.and_then(|lit| match &lit.value {
        ValueKind::Boolean(b) => Some(*b),
        _ => None,
    }) {
        Some(true) => 0,
        Some(false) => 1,
        None => 0,
    };

    let help_message = if schema_default.is_some() {
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
    schema_default: Option<&LiteralValue>,
    constraints: &TextConstraints,
) -> Result<String> {
    let default_str = schema_default.map(|l| l.to_string());
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
    schema_default: Option<&LiteralValue>,
    help: &str,
) -> Result<String> {
    let (left_default, right_default) = match schema_default {
        Some(LiteralValue {
            value: ValueKind::Range(left, right),
            ..
        }) => (Some(left.display_value()), Some(right.display_value())),
        _ => (None, None),
    };

    let endpoint_example = match &lemma_type.specifications {
        TypeSpecification::DateRange { .. } => "2024-01-01",
        TypeSpecification::NumberRange { .. } => "0",
        TypeSpecification::QuantityRange { .. } => "30 kilogram",
        TypeSpecification::RatioRange { .. } => "10%",
        TypeSpecification::CalendarRange { .. } => "18 years...67 years",
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
    schema_default: Option<&LiteralValue>,
    constraints: &NumericConstraints,
) -> Result<String> {
    let default_str = schema_default.map(|l| l.to_string());
    let prompt_message = format!("{} [{}]", data_name, type_str);
    prompt_decimal_input(&prompt_message, default_str.as_deref(), constraints, "10")
}

fn prompt_quantity_data(
    data_name: &str,
    type_str: &str,
    schema_default: Option<&LiteralValue>,
    units: &lemma::planning::semantics::QuantityUnits,
    constraints: &NumericConstraints,
) -> Result<String> {
    let parsed = schema_default.and_then(|lit| match &lit.value {
        ValueKind::Quantity(n, u, _decomposition) => Some((*n, u.as_str())),
        _ => None,
    });

    let prompt_message = format!("{} [{}]", data_name, type_str);

    if units.is_empty() {
        let default_str = parsed.map(|(v, _)| rational_default_string(&v));
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

    let numeric_default = parsed.map(|(value, _def_unit)| rational_default_string(&value));

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

fn ratio_default_numeric_for_decimal_prompt(lit: &LiteralValue) -> Option<String> {
    match &lit.value {
        ValueKind::Ratio(n, Some(u)) if u == "percent" => {
            let scaled = checked_mul(n, &RationalInteger::new(100, 1)).ok()?;
            Some(rational_default_string(&scaled))
        }
        ValueKind::Ratio(n, Some(u)) if u == "permille" => {
            let scaled = checked_mul(n, &RationalInteger::new(1000, 1)).ok()?;
            Some(rational_default_string(&scaled))
        }
        ValueKind::Ratio(n, _) => Some(rational_default_string(n)),
        _ => None,
    }
}

fn prompt_ratio_data(
    data_name: &str,
    type_str: &str,
    schema_default: Option<&LiteralValue>,
    units: &lemma::planning::semantics::RatioUnits,
    minimum: Option<Decimal>,
    maximum: Option<Decimal>,
    help: &str,
) -> Result<String> {
    // Ratio types typically support percent/permille; offer a unit selector.
    let prompt_message = format!("{} [{}]", data_name, type_str);
    let default_decimal = schema_default.and_then(ratio_default_numeric_for_decimal_prompt);
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

fn prompt_calendar_data(
    data_name: &str,
    type_str: &str,
    schema_default: Option<&LiteralValue>,
    help: &str,
) -> Result<String> {
    if let Some(lit) = schema_default.filter(|l| matches!(l.value, ValueKind::Calendar(_, _))) {
        let default = lit.to_string();
        let prompt_message = format!("{} [{}]", data_name, type_str);
        let help_message = if help.is_empty() {
            "Example: 6 months".to_string()
        } else {
            help.to_string()
        };
        return Text::new(&prompt_message)
            .with_help_message(&help_message)
            .with_default(&default)
            .prompt()
            .context(format!("Failed to get calendar value for {}", data_name));
    }

    let units = vec!["months", "years"];
    let unit = Select::new(&format!("Select calendar unit for {}", data_name), units)
        .prompt()
        .context(format!("Failed to get calendar unit for {}", data_name))?;

    let value = prompt_decimal_input(
        &format!("Enter calendar value for {}", data_name),
        None,
        &NumericConstraints {
            minimum: None,
            maximum: None,
            decimals: None,
            help: help.to_string(),
        },
        "6",
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
