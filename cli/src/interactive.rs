use anyhow::{Context, Result};
use inquire::validator::Validation;
use inquire::{DateSelect, MultiSelect, Select, Text};
use lemma::{Engine, FactValue, LemmaType};
use std::collections::HashMap;

pub type InteractiveResult = (
    String,
    Option<Vec<String>>,
    HashMap<String, String>,
    Option<String>,
);

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

    let target = prompt_target(engine, &doc, &rules)?;
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
            let facts_count = engine.get_document_facts(doc_name).len();
            let rules_count = engine.get_document_rules(doc_name).len();
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
    let all_rules = engine.get_document_rules(doc_name);

    if all_rules.is_empty() {
        return Ok(None);
    }

    let rule_names: Vec<String> = all_rules.iter().map(|r| r.name.clone()).collect();

    if rule_names.len() == 1 {
        return Ok(None);
    }

    let selected = MultiSelect::new("Select rules to evaluate:", rule_names.clone())
        .with_default(&(0..rule_names.len()).collect::<Vec<_>>())
        .prompt()
        .context("Failed to get rule selection")?;

    if selected.is_empty() || selected.len() == all_rules.len() {
        Ok(None)
    } else {
        Ok(Some(selected))
    }
}

fn prompt_target(
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
    _rule_names: &Option<Vec<String>>,
    provided_facts: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let doc_facts = engine.get_document_facts(doc_name);

    let promptable_facts: Vec<_> = doc_facts
        .into_iter()
        .filter(|f| {
            let fact_name = f.reference.to_string();
            let is_document_reference = matches!(f.value, FactValue::DocumentReference(_));
            !is_document_reference && !provided_facts.contains_key(&fact_name)
        })
        .collect();

    if promptable_facts.is_empty() {
        return Ok(HashMap::new());
    }

    let mut facts = HashMap::new();

    println!("\nEnter values for facts (press Enter to accept defaults):");

    for fact in promptable_facts {
        let fact_name = fact.reference.to_string();

        let (lemma_type, default_value): (LemmaType, Option<String>) = match &fact.value {
            FactValue::TypeDeclaration {
                base,
                overrides,
                from: _,
            } => {
                // For now, only handle standard types in interactive mode
                // Custom types will need to be resolved through the Engine
                let base_specs = match base.as_str() {
                    "boolean" => lemma::TypeSpecification::boolean(),
                    "scale" => lemma::TypeSpecification::scale(),
                    "number" => lemma::TypeSpecification::number(),
                    "text" => lemma::TypeSpecification::text(),
                    "date" => lemma::TypeSpecification::date(),
                    "time" => lemma::TypeSpecification::time(),
                    "duration" => lemma::TypeSpecification::duration(),
                    "ratio" => lemma::TypeSpecification::ratio(),
                    "percent" => lemma::TypeSpecification::ratio(),
                    _ => {
                        // Custom type - skip for now (would need Engine to resolve)
                        eprintln!("Warning: Custom type '{}' not yet supported in interactive mode, skipping", base);
                        continue;
                    }
                };

                // Apply overrides if any
                let mut specs = base_specs;
                if let Some(ref overrides_vec) = overrides {
                    for (command, args) in overrides_vec {
                        specs = specs.apply_override(command, args).map_err(|e| {
                            anyhow::anyhow!(
                                "Invalid command '{}' for type '{}': {}",
                                command,
                                base,
                                e
                            )
                        })?;
                    }
                }

                (LemmaType::without_name(specs), None)
            }
            FactValue::Literal(lit) => {
                let lemma_type = lit.lemma_type.clone();
                let default_value: Option<String> = None;
                let type_str = lemma_type.to_string();
                let input_value = match lemma_type.name() {
                    "date" => prompt_date_fact(&fact_name, default_value.as_deref())?,
                    "boolean" => prompt_boolean_fact(&fact_name, default_value.as_deref())?,
                    _ => prompt_text_fact(
                        &fact_name,
                        &type_str,
                        &lemma_type,
                        default_value.as_deref(),
                    )?,
                };
                facts.insert(fact_name, input_value);
                continue;
            }
            FactValue::DocumentReference(_) => continue,
        };

        let type_str = lemma_type.to_string();
        let input_value = match lemma_type.name() {
            "date" => prompt_date_fact(&fact_name, default_value.as_deref())?,
            "boolean" => prompt_boolean_fact(&fact_name, default_value.as_deref())?,
            _ => prompt_text_fact(&fact_name, &type_str, &lemma_type, default_value.as_deref())?,
        };

        facts.insert(fact_name, input_value);
    }

    Ok(facts)
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

fn prompt_text_fact(
    fact_name: &str,
    type_str: &str,
    lemma_type: &LemmaType,
    default_value: Option<&str>,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", fact_name, type_str);

    match default_value {
        Some(default) => {
            let help_message = format!(
                "Press Enter to keep current value, or type a new value. Example: {}",
                lemma_type.example_value()
            );

            Text::new(&prompt_message)
                .with_help_message(&help_message)
                .with_default(default)
                .prompt()
                .context(format!("Failed to get value for {}", fact_name))
        }
        None => {
            let validator = |input: &str| {
                if input.trim().is_empty() {
                    Ok(Validation::Invalid("Value is required".into()))
                } else {
                    Ok(Validation::Valid)
                }
            };

            let help_message = format!("Example: {}", lemma_type.example_value());

            Text::new(&prompt_message)
                .with_help_message(&help_message)
                .with_validator(validator)
                .prompt()
                .context(format!("Failed to get value for {}", fact_name))
        }
    }
}
