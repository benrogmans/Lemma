use anyhow::{Context, Result};
use inquire::{DateSelect, MultiSelect, Select, Text};
use lemma::{Engine, LemmaType, TypeAnnotation};

pub fn run_interactive(
    engine: &Engine,
    doc_name: Option<String>,
    rule_names: Option<Vec<String>>,
) -> Result<(String, Option<Vec<String>>, Vec<String>)> {
    let doc = match doc_name {
        Some(name) => name,
        None => select_document(engine)?,
    };

    let rules = match rule_names {
        Some(names) => Some(names),
        None => select_rules(engine, &doc)?,
    };

    let facts = prompt_facts(engine, &doc, &rules)?;

    Ok((doc, rules, facts))
}

fn select_document(engine: &Engine) -> Result<String> {
    let documents = engine.list_documents();

    if documents.is_empty() {
        anyhow::bail!("No documents found in workspace. Add .lemma files to get started.");
    }

    if documents.len() == 1 {
        return Ok(documents[0].clone());
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
    rule_names: &Option<Vec<String>>,
) -> Result<Vec<String>> {
    let all_rules = engine.get_document_rules(doc_name);
    let doc_facts = engine.get_document_facts(doc_name);

    let all_rules_vec: Vec<_> = all_rules.iter().map(|r| (*r).clone()).collect();
    let doc_facts_vec: Vec<_> = doc_facts.iter().map(|f| (*f).clone()).collect();

    let required_fact_names = if let Some(rules) = rule_names {
        let mut required = std::collections::HashSet::new();
        for rule_name in rules {
            if let Some(rule) = all_rules.iter().find(|r| &r.name == rule_name) {
                let rule_facts = lemma::analysis::find_required_facts_recursive(
                    rule,
                    &all_rules_vec,
                    &doc_facts_vec,
                );
                required.extend(rule_facts);
            }
        }
        required
    } else {
        doc_facts
            .iter()
            .map(|f| lemma::analysis::fact_display_name(f))
            .collect()
    };

    let required_facts: Vec<_> = doc_facts
        .into_iter()
        .filter(|f| required_fact_names.contains(&lemma::analysis::fact_display_name(f)))
        .collect();

    if required_facts.is_empty() {
        return Ok(Vec::new());
    }

    let mut fact_values = Vec::new();

    println!("\nEnter fact values:");

    for fact in required_facts {
        let fact_name = lemma::analysis::fact_display_name(fact);

        let (type_ann, default_value) = match &fact.value {
            lemma::FactValue::TypeAnnotation(type_ann) => (type_ann.clone(), None),
            lemma::FactValue::Literal(lit) => {
                (TypeAnnotation::LemmaType(lit.to_type()), Some(format!("{}", lit)))
            }
            lemma::FactValue::DocumentReference(_) => continue,
        };

        let type_str = type_ann.to_string();

        let value = match &type_ann {
            TypeAnnotation::LemmaType(LemmaType::Date) => {
                let date = DateSelect::new(&format!("{} [date]", fact_name))
                    .with_help_message("Use arrow keys to navigate, Enter to select")
                    .prompt()
                    .context(format!("Failed to get date for {}", fact_name))?;

                format!("{}T00:00:00Z", date.format("%Y-%m-%d"))
            }
            _ => {
                let prompt_message = format!("{} [{}]", fact_name, type_str);

                if let Some(default) = &default_value {
                    Text::new(&prompt_message)
                        .with_help_message(&format!("Example: {}", type_ann.example_value()))
                        .with_default(default)
                        .prompt()
                        .context(format!("Failed to get value for {}", fact_name))?
                } else {
                    Text::new(&prompt_message)
                        .with_help_message(&format!("Example: {}", type_ann.example_value()))
                        .prompt()
                        .context(format!("Failed to get value for {}", fact_name))?
                }
            }
        };

        fact_values.push(format!("{}={}", fact_name, value));
    }

    Ok(fact_values)
}
