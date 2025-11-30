use anyhow::{Context, Result};
use inquire::validator::Validation;
use inquire::{DateSelect, MultiSelect, Select, Text};
use lemma::{Engine, FactValue, LemmaType, TypeAnnotation};
use std::collections::HashMap;

pub type InteractiveResult = (String, Option<Vec<String>>, HashMap<String, String>);

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

    let facts = prompt_facts(engine, &doc, &rules, provided_facts)?;

    Ok((doc, rules, facts))
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

        let (type_ann, default_value) = match &fact.value {
            FactValue::TypeAnnotation(type_ann) => (type_ann.clone(), None),
            FactValue::Literal(lit) => {
                let default = match lit {
                    lemma::LiteralValue::Text(s) => s.clone(),
                    _ => format!("{}", lit),
                };
                (TypeAnnotation::LemmaType(lit.to_type()), Some(default))
            }
            FactValue::DocumentReference(_) => continue,
        };

        let type_str = type_ann.to_string();

        let input_value = match &type_ann {
            TypeAnnotation::LemmaType(LemmaType::Date) => {
                prompt_date_fact(&fact_name, default_value.as_deref())?
            }
            TypeAnnotation::LemmaType(LemmaType::Boolean) => {
                prompt_boolean_fact(&fact_name, default_value.as_deref())?
            }
            _ => prompt_text_fact(&fact_name, &type_str, &type_ann, default_value.as_deref())?,
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
    type_ann: &TypeAnnotation,
    default_value: Option<&str>,
) -> Result<String> {
    let prompt_message = format!("{} [{}]", fact_name, type_str);

    match default_value {
        Some(default) => {
            let help_message = format!(
                "Press Enter to keep current value, or type a new value. Example: {}",
                type_ann.example_value()
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

            let help_message = format!("Example: {}", type_ann.example_value());

            Text::new(&prompt_message)
                .with_help_message(&help_message)
                .with_validator(validator)
                .prompt()
                .context(format!("Failed to get value for {}", fact_name))
        }
    }
}
