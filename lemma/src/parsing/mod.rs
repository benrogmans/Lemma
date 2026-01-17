use crate::error::LemmaError;
use crate::limits::ResourceLimits;
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use std::sync::Arc;

pub mod ast;
pub mod expressions;
pub mod facts;
pub mod literals;
pub mod rules;
pub mod source;
pub mod types;
pub mod units;

pub use ast::{DepthTracker, Span};
pub use source::Source;

pub use crate::semantic::*;

#[derive(Parser)]
#[grammar = "src/parsing/lemma.pest"]
pub struct LemmaParser;

pub fn parse(
    content: &str,
    attribute: &str,
    limits: &ResourceLimits,
) -> Result<Vec<LemmaDoc>, LemmaError> {
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

    let mut depth_tracker = DepthTracker::with_max_depth(limits.max_expression_depth);

    match LemmaParser::parse(Rule::lemma_file, content) {
        Ok(mut pairs) => {
            let mut docs = Vec::new();
            if let Some(lemma_file_pair) = pairs.next() {
                for inner_pair in lemma_file_pair.into_inner() {
                    if inner_pair.as_rule() == Rule::doc {
                        docs.push(parse_doc(inner_pair, attribute, &mut depth_tracker)?);
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
                attribute,
                Arc::from(content),
                "<parse-error>",
                1,
                None::<String>,
            ))
        }
    }
}

fn parse_doc(
    pair: Pair<Rule>,
    attribute: &str,
    depth_tracker: &mut DepthTracker,
) -> Result<LemmaDoc, LemmaError> {
    let doc_start_line = pair.as_span().start_pos().line_col().0;

    let mut doc_name: Option<String> = None;
    let mut commentary: Option<String> = None;
    let mut facts = Vec::new();
    let mut rules = Vec::new();
    let mut types = Vec::new();

    // First, extract doc_header to get commentary and doc_declaration
    for inner_pair in pair.clone().into_inner() {
        if inner_pair.as_rule() == Rule::doc_header {
            for header_item in inner_pair.into_inner() {
                match header_item.as_rule() {
                    Rule::commentary_block => {
                        for block_inner in header_item.into_inner() {
                            if block_inner.as_rule() == Rule::commentary {
                                commentary = Some(block_inner.as_str().trim().to_string());
                                break;
                            }
                        }
                    }
                    Rule::doc_declaration => {
                        for decl_inner in header_item.into_inner() {
                            if decl_inner.as_rule() == Rule::doc_name {
                                doc_name = Some(decl_inner.as_str().to_string());
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let name = doc_name.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: doc missing doc_declaration",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<unknown>",
            std::sync::Arc::from(""),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;

    // First pass: collect all named type definitions from doc_body
    // These are explicit type definitions like: `type money = number -> minimum 0`
    // and type imports like: `type money from "other_doc"`
    // Note: Inline type definitions (e.g., `fact price = [number -> minimum 0]`) are
    // anonymous and handled during fact parsing, not collected here.
    for inner_pair in pair.clone().into_inner() {
        if inner_pair.as_rule() == Rule::doc_body {
            for body_item in inner_pair.into_inner() {
                match body_item.as_rule() {
                    Rule::type_definition => {
                        let type_def = crate::parsing::types::parse_type_definition(body_item)?;
                        types.push(type_def);
                    }
                    Rule::type_import => {
                        let type_def = crate::parsing::types::parse_type_import(body_item)?;
                        types.push(type_def);
                    }
                    _ => {}
                }
            }
        }
    }

    // Second pass: parse facts and rules from doc_body (which may reference named types via type_declaration
    // or use inline_type_definition for anonymous types)
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::doc_body {
            for body_item in inner_pair.into_inner() {
                match body_item.as_rule() {
                    Rule::fact_definition => {
                        let fact = crate::parsing::facts::parse_fact_definition(
                            body_item,
                            attribute,
                            &name,
                            depth_tracker,
                            &types,
                        )?;
                        facts.push(fact);
                    }
                    Rule::fact_override => {
                        let fact = crate::parsing::facts::parse_fact_override(
                            body_item,
                            attribute,
                            &name,
                            depth_tracker,
                            &types,
                        )?;
                        facts.push(fact);
                    }
                    Rule::rule_definition => {
                        let rule = crate::parsing::rules::parse_rule_definition(
                            body_item,
                            depth_tracker,
                            attribute,
                            &name,
                        )?;
                        rules.push(rule);
                    }
                    _ => {}
                }
            }
        }
    }
    let mut doc = LemmaDoc::new(name)
        .with_attribute(attribute.to_string())
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
    for type_def in types {
        doc = doc.add_type(type_def);
    }

    Ok(doc)
}
