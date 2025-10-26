use crate::ast::{ExpressionIdGenerator, Span};
use crate::error::LemmaError;
use crate::resource_limits::ResourceLimits;
use crate::semantic::*;
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use std::sync::Arc;

pub mod expressions;
pub mod facts;
pub mod literals;
pub mod rules;
pub mod units;

#[derive(Parser)]
#[grammar = "src/parser/lemma.pest"]
pub struct LemmaParser;

pub fn parse(
    content: &str,
    filename: Option<String>,
    limits: &ResourceLimits,
) -> Result<Vec<LemmaDoc>, LemmaError> {
    // Check file size limit
    if content.len() > limits.max_file_size_bytes {
        return Err(LemmaError::ResourceLimitExceeded {
            limit_name: "max_file_size_bytes".to_string(),
            limit_value: format!(
                "{} bytes ({} MB)",
                limits.max_file_size_bytes,
                limits.max_file_size_bytes / (1024 * 1024)
            ),
            actual_value: format!(
                "{} bytes ({:.2} MB)",
                content.len(),
                content.len() as f64 / (1024.0 * 1024.0)
            ),
            suggestion: "Reduce file size or split into multiple documents".to_string(),
        });
    }

    let mut id_gen = ExpressionIdGenerator::with_max_depth(limits.max_expression_depth);
    let filename = filename.unwrap_or_else(|| "<input>".to_string());

    match LemmaParser::parse(Rule::lemma_file, content) {
        Ok(pairs) => {
            let mut docs = Vec::new();
            for pair in pairs {
                if pair.as_rule() == Rule::lemma_file {
                    for inner_pair in pair.into_inner() {
                        if inner_pair.as_rule() == Rule::doc {
                            docs.push(parse_doc(inner_pair, &filename, content, &mut id_gen)?);
                        }
                    }
                }
            }
            Ok(docs)
        }
        Err(e) => {
            let pest_span = match e.line_col {
                pest::error::LineColLocation::Pos((line, col)) => Span {
                    start: 0,
                    end: 0,
                    line,
                    col,
                },
                pest::error::LineColLocation::Span((start_line, start_col), (_, _)) => Span {
                    start: 0,
                    end: 0,
                    line: start_line,
                    col: start_col,
                },
            };

            Err(LemmaError::parse(
                format!("Parse error: {}", e.variant),
                pest_span,
                filename,
                Arc::from(content),
                "<parse-error>",
                1,
            ))
        }
    }
}

pub fn parse_facts(fact_strings: &[&str]) -> Result<Vec<LemmaFact>, LemmaError> {
    let mut facts = Vec::new();

    for fact_str in fact_strings {
        let fact_input = format!("fact {}", fact_str);
        let pairs = LemmaParser::parse(Rule::fact, &fact_input).map_err(|e| {
            LemmaError::Engine(format!("Failed to parse fact '{}': {}", fact_str, e))
        })?;

        let fact_pair = pairs.into_iter().next().ok_or_else(|| {
            LemmaError::Engine(format!("No parse result for fact '{}'", fact_str))
        })?;

        let inner_pair = fact_pair
            .into_inner()
            .next()
            .ok_or_else(|| LemmaError::Engine(format!("No inner rule for fact '{}'", fact_str)))?;

        let fact = match inner_pair.as_rule() {
            Rule::fact_definition => crate::parser::facts::parse_fact_definition(inner_pair)?,
            Rule::fact_override => crate::parser::facts::parse_fact_override(inner_pair)?,
            _ => {
                return Err(LemmaError::Engine(format!(
                    "Unexpected rule type for fact '{}'",
                    fact_str
                )))
            }
        };

        facts.push(fact);
    }

    Ok(facts)
}

fn parse_doc(
    pair: Pair<Rule>,
    filename: &str,
    _source: &str,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<LemmaDoc, LemmaError> {
    let doc_start_line = pair.as_span().start_pos().line_col().0;

    let mut doc_name: Option<String> = None;
    let mut commentary: Option<String> = None;
    let mut facts = Vec::new();
    let mut rules = Vec::new();

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::doc_declaration => {
                for decl_inner in inner_pair.into_inner() {
                    if decl_inner.as_rule() == Rule::doc_name {
                        doc_name = Some(parse_doc_name(decl_inner)?);
                        break;
                    }
                }
            }
            Rule::commentary_content => {
                commentary = Some(inner_pair.as_str().trim().to_string());
            }
            Rule::fact_definition => {
                let fact = crate::parser::facts::parse_fact_definition(inner_pair)?;
                facts.push(fact);
            }
            Rule::fact_override => {
                let fact = crate::parser::facts::parse_fact_override(inner_pair)?;
                facts.push(fact);
            }
            Rule::rule_definition => {
                let rule = crate::parser::rules::parse_rule_definition(inner_pair, id_gen)?;
                rules.push(rule);
            }
            _ => {}
        }
    }

    let name = doc_name.unwrap_or_else(|| "default".to_string());
    let mut doc = LemmaDoc::new(name)
        .with_source(filename.to_string())
        .with_start_line(doc_start_line);

    if let Some(commentary_text) = commentary {
        doc = doc.set_commentary(commentary_text);
    }

    for fact in facts {
        doc = doc.add_fact(fact);
    }
    for rule in rules {
        doc = doc.add_rule(rule);
    }

    Ok(doc)
}

fn parse_doc_name(pair: Pair<Rule>) -> Result<String, LemmaError> {
    Ok(pair.as_str().to_string())
}
