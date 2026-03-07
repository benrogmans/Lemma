use crate::error::Error;
use crate::limits::ResourceLimits;
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use std::sync::Arc;

pub mod ast;
pub mod expressions;
pub mod facts;
pub mod literals;
pub mod meta;
pub mod rules;
pub mod source;
pub mod types;

pub use ast::{DepthTracker, Span};
pub use source::Source;

pub use ast::*;

#[derive(Parser)]
#[grammar = "src/parsing/lemma.pest"]
pub struct LemmaParser;

pub fn parse(
    content: &str,
    attribute: &str,
    limits: &ResourceLimits,
) -> Result<Vec<LemmaDoc>, Error> {
    if content.len() > limits.max_file_size_bytes {
        return Err(Error::ResourceLimitExceeded {
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
            document_context: None,
        });
    }

    let mut depth_tracker = DepthTracker::with_max_depth(limits.max_expression_depth);

    let source_text: Arc<str> = Arc::from(content);

    match LemmaParser::parse(Rule::lemma_file, content) {
        Ok(mut pairs) => {
            let mut docs = Vec::new();
            if let Some(lemma_file_pair) = pairs.next() {
                for inner_pair in lemma_file_pair.into_inner() {
                    if inner_pair.as_rule() == Rule::doc {
                        docs.push(parse_doc(
                            inner_pair,
                            attribute,
                            &mut depth_tracker,
                            source_text.clone(),
                        )?);
                    }
                }
            }
            Ok(docs)
        }
        Err(e) => {
            let (line, col) = match e.line_col {
                pest::error::LineColLocation::Pos((l, c)) => (l, c),
                pest::error::LineColLocation::Span((l, c), _) => (l, c),
            };
            let (start_byte, end_byte) = line_col_to_byte_range(content, line, col);
            let pest_span = Span {
                start: start_byte,
                end: end_byte,
                line,
                col,
            };

            let message = humanize_pest_error(&e.variant);

            Err(Error::parsing(
                message,
                Some(crate::parsing::source::Source::new(
                    attribute,
                    pest_span,
                    "",
                    source_text,
                )),
                None::<String>,
            ))
        }
    }
}

/// Convert a 1-based line/col to a byte range covering the rest of that line.
fn line_col_to_byte_range(text: &str, line: usize, col: usize) -> (usize, usize) {
    let mut current_line = 1usize;
    let mut line_start = 0usize;
    for (i, b) in text.bytes().enumerate() {
        if current_line == line {
            let start = line_start + col.saturating_sub(1);
            let end = text[i..].find('\n').map(|n| i + n).unwrap_or(text.len());
            return (start.min(text.len()), end.max(start));
        }
        if b == b'\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    if current_line == line {
        let start = line_start + col.saturating_sub(1);
        return (start.min(text.len()), text.len());
    }
    (0, text.len().min(1))
}

fn humanize_pest_error(variant: &pest::error::ErrorVariant<Rule>) -> String {
    match variant {
        pest::error::ErrorVariant::ParsingError {
            positives,
            negatives,
        } => {
            let readable: Vec<&str> = positives.iter().filter_map(|r| rule_to_human(*r)).collect();
            if readable.is_empty() {
                if !negatives.is_empty() {
                    "Unexpected token".to_string()
                } else {
                    "Syntax error".to_string()
                }
            } else if readable.len() == 1 {
                format!("Expected {}", readable[0])
            } else {
                format!("Expected one of: {}", readable.join(", "))
            }
        }
        pest::error::ErrorVariant::CustomError { message } => message.clone(),
    }
}

fn rule_to_human(rule: Rule) -> Option<&'static str> {
    match rule {
        Rule::lemma_file => Some("a document declaration (e.g. 'doc my_document')"),
        Rule::doc => Some("a document declaration"),
        Rule::doc_declaration => Some("a document declaration (e.g. 'doc name')"),
        Rule::doc_name => Some("a document name"),
        Rule::doc_body => Some("document body (facts, rules, or types)"),
        Rule::fact_definition => Some("a fact definition (e.g. 'fact x: 10')"),
        Rule::rule_definition => Some("a rule definition (e.g. 'rule y: x + 1')"),
        Rule::type_definition => Some("a type definition (e.g. 'type money: scale')"),
        Rule::type_import => Some("a type import (e.g. 'type money from other_doc')"),
        Rule::meta_definition => Some("a meta field (e.g. 'meta key: value')"),
        Rule::expression => Some("an expression"),
        Rule::unless_statement => Some("an unless clause"),
        Rule::commentary_block => Some("a commentary block (triple-quoted string)"),
        Rule::literal => Some("a value (number, text, boolean, date, etc.)"),
        Rule::number_literal => Some("a number"),
        Rule::text_literal => Some("a string (double-quoted)"),
        Rule::boolean_literal => Some("a boolean (true/false/yes/no)"),
        Rule::label => Some("an identifier"),
        Rule::assignment_symbol => Some("':'"),
        Rule::EOI => Some("end of file"),
        _ => None,
    }
}

fn parse_doc(
    pair: Pair<Rule>,
    attribute: &str,
    depth_tracker: &mut DepthTracker,
    source_text: Arc<str>,
) -> Result<LemmaDoc, Error> {
    let doc_start_line = pair.as_span().start_pos().line_col().0;

    let mut doc_name: Option<String> = None;
    let mut effective_from: Option<crate::parsing::ast::DateTimeValue> = None;
    let mut commentary: Option<String> = None;
    let mut facts = Vec::new();
    let mut rules = Vec::new();
    let mut types = Vec::new();
    let mut meta_fields = Vec::new();

    // First, extract doc_header to get commentary and doc_declaration
    for header_item in pair.clone().into_inner() {
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
                    match decl_inner.as_rule() {
                        Rule::doc_name => {
                            for name_part in decl_inner.into_inner() {
                                match name_part.as_rule() {
                                    Rule::doc_name_base => {
                                        doc_name = Some(name_part.as_str().to_string());
                                    }
                                    Rule::doc_name_at => {}
                                    _ => {}
                                }
                            }
                        }
                        Rule::doc_effective_from => {
                            let raw = decl_inner.as_str();
                            effective_from =
                                Some(crate::parsing::ast::DateTimeValue::parse(raw).ok_or_else(
                                    || {
                                        Error::parsing(
                                            format!(
                                                "Invalid date/time in doc declaration: '{}'",
                                                raw
                                            ),
                                            None,
                                            None::<String>,
                                        )
                                    },
                                )?);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    let name = doc_name.expect("BUG: grammar guarantees doc has doc_declaration");

    // First pass: collect all named type definitions from doc_body
    for inner_pair in pair.clone().into_inner() {
        if inner_pair.as_rule() == Rule::doc_body {
            for body_item in inner_pair.into_inner() {
                match body_item.as_rule() {
                    Rule::type_definition => {
                        let type_def = crate::parsing::types::parse_type_definition(
                            body_item,
                            attribute,
                            &name,
                            source_text.clone(),
                        )?;
                        types.push(type_def);
                    }
                    Rule::type_import => {
                        let type_def = crate::parsing::types::parse_type_import(
                            body_item,
                            attribute,
                            &name,
                            source_text.clone(),
                        )?;
                        types.push(type_def);
                    }
                    _ => {}
                }
            }
        }
    }

    // Second pass: parse facts and rules from doc_body
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::doc_body {
            for body_item in inner_pair.into_inner() {
                match body_item.as_rule() {
                    Rule::fact_definition => {
                        let fact = crate::parsing::facts::parse_fact_definition(
                            body_item,
                            attribute,
                            &name,
                            source_text.clone(),
                        )?;
                        facts.push(fact);
                    }
                    Rule::fact_binding => {
                        let fact = crate::parsing::facts::parse_fact_binding(
                            body_item,
                            attribute,
                            &name,
                            source_text.clone(),
                        )?;
                        facts.push(fact);
                    }
                    Rule::rule_definition => {
                        let rule = crate::parsing::rules::parse_rule_definition(
                            body_item,
                            depth_tracker,
                            attribute,
                            &name,
                            source_text.clone(),
                        )?;
                        rules.push(rule);
                    }
                    Rule::meta_definition => {
                        let meta = crate::parsing::meta::parse_meta_definition(
                            body_item,
                            attribute,
                            &name,
                            source_text.clone(),
                        )?;
                        meta_fields.push(meta);
                    }
                    _ => {}
                }
            }
        }
    }
    let mut doc = LemmaDoc::new(name)
        .with_attribute(attribute.to_string())
        .with_start_line(doc_start_line);
    doc.effective_from = effective_from;

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
    for meta in meta_fields {
        doc = doc.add_meta_field(meta);
    }

    Ok(doc)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::Error;
    use crate::ResourceLimits;

    #[test]
    fn parse_empty_input_returns_no_documents() {
        let result = parse("", "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn parse_workspace_file_yields_expected_doc_facts_and_rules() {
        let input = r#"doc person
fact name: "John Doe"
rule adult: true"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[0].facts.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].name, "adult");
    }

    #[test]
    fn mixing_facts_and_rules_is_collected_into_doc() {
        let input = r#"doc test
fact name: "John"
rule is_adult: age >= 18
fact age: 25
rule can_drink: age >= 21
fact status: "active"
rule is_eligible: is_adult and status == "active""#;

        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 3);
        assert_eq!(result[0].rules.len(), 3);
    }

    #[test]
    fn parse_simple_document_collects_facts() {
        let input = r#"doc person
fact name: "John"
fact age: 25"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[0].facts.len(), 2);
    }

    #[test]
    fn parse_doc_name_with_slashes_is_preserved() {
        let input = r#"doc contracts/employment/jack
fact name: "Jack""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "contracts/employment/jack");
    }

    #[test]
    fn parse_doc_name_no_version_tag() {
        let input = "doc mydoc\nrule x: 1";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "mydoc");
        assert_eq!(result[0].effective_from(), None);
    }

    #[test]
    fn parse_commentary_block_is_attached_to_doc() {
        let input = r#"doc person
"""
This is a markdown comment
with **bold** text
"""
fact name: "John""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].commentary.is_some());
        assert!(result[0].commentary.as_ref().unwrap().contains("**bold**"));
    }

    #[test]
    fn parse_document_with_rule_collects_rule() {
        let input = r#"doc person
rule is_adult: age >= 18"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].name, "is_adult");
    }

    #[test]
    fn parse_multiple_documents_returns_all_docs() {
        let input = r#"doc person
fact name: "John"

doc company
fact name: "Acme Corp""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[1].name, "company");
    }

    #[test]
    fn parse_allows_duplicate_fact_names() {
        // Duplicate fact names are rejected during planning/validation, not parsing.
        let input = r#"doc person
fact name: "John"
fact name: "Jane""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Parser should succeed even with duplicate facts"
        );
    }

    #[test]
    fn parse_allows_duplicate_rule_names() {
        // Duplicate rule names are rejected during planning/validation, not parsing.
        let input = r#"doc person
rule is_adult: age >= 18
rule is_adult: age >= 21"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Parser should succeed even with duplicate rules"
        );
    }

    #[test]
    fn parse_rejects_malformed_input() {
        let input = "invalid syntax here";
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(result.is_err());
    }

    #[test]
    fn parse_handles_whitespace_variants_in_expressions() {
        let test_cases = vec![
            ("doc test\nrule test: 2+3", "no spaces in arithmetic"),
            ("doc test\nrule test: age>=18", "no spaces in comparison"),
            (
                "doc test\nrule test: age >= 18 and salary>50000",
                "spaces around and keyword",
            ),
            (
                "doc test\nrule test: age  >=  18  and  salary  >  50000",
                "extra spaces",
            ),
            (
                "doc test\nrule test: \n  age >= 18 \n  and \n  salary > 50000",
                "newlines in expression",
            ),
        ];

        for (input, description) in test_cases {
            let result = parse(input, "test.lemma", &ResourceLimits::default());
            assert!(
                result.is_ok(),
                "Failed to parse {} ({}): {:?}",
                input,
                description,
                result.err()
            );
        }
    }

    #[test]
    fn parse_error_cases_are_rejected() {
        let error_cases = vec![
            (
                "doc test\nfact name: \"unclosed string",
                "unclosed string literal",
            ),
            ("doc test\nrule test: 2 + + 3", "double operator"),
            ("doc test\nrule test: (2 + 3", "unclosed parenthesis"),
            ("doc test\nrule test: 2 + 3)", "extra closing paren"),
            // Note: "invalid unit" now parses as a user-defined unit (validated during planning)
            ("doc test\nfact doc: 123", "reserved keyword as fact name"),
            ("doc test\nrule rule: true", "reserved keyword as rule name"),
        ];

        for (input, description) in error_cases {
            let result = parse(input, "test.lemma", &ResourceLimits::default());
            assert!(
                result.is_err(),
                "Expected error for {} but got success",
                description
            );
        }
    }

    #[test]
    fn parse_duration_literals_in_rules() {
        let test_cases = vec![
            ("2 years", "years"),
            ("6 months", "months"),
            ("52 weeks", "weeks"),
            ("365 days", "days"),
            ("24 hours", "hours"),
            ("60 minutes", "minutes"),
            ("3600 seconds", "seconds"),
            ("1000 milliseconds", "milliseconds"),
            ("500000 microseconds", "microseconds"),
            ("50 percent", "percent"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test: {}", expr);
            let result = parse(&input, "test.lemma", &ResourceLimits::default());
            assert!(
                result.is_ok(),
                "Failed to parse literal {} ({}): {:?}",
                expr,
                description,
                result.err()
            );
        }
    }

    #[test]
    fn parse_comparisons_with_duration_unit_conversions() {
        let test_cases = vec![
            (
                "(duration in hours) > 2",
                "duration conversion in comparison with parens",
            ),
            (
                "(meeting_time in minutes) >= 30",
                "duration conversion with gte",
            ),
            (
                "(project_length in days) < 100",
                "duration conversion with lt",
            ),
            (
                "(delay in seconds) == 60",
                "duration conversion with equality",
            ),
            (
                "(1 hours) > (30 minutes)",
                "duration conversions on both sides",
            ),
            (
                "duration in hours > 2",
                "duration conversion without parens",
            ),
            (
                "meeting_time in seconds > 3600",
                "variable duration conversion in comparison",
            ),
            (
                "project_length in days > deadline_days",
                "two variables with duration conversion",
            ),
            (
                "duration in hours >= 1 and duration in hours <= 8",
                "multiple duration comparisons",
            ),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test: {}", expr);
            let result = parse(&input, "test.lemma", &ResourceLimits::default());
            assert!(
                result.is_ok(),
                "Failed to parse {} ({}): {:?}",
                expr,
                description,
                result.err()
            );
        }
    }

    #[test]
    fn parse_error_includes_attribute_and_parse_error_doc_name() {
        let result = parse(
            r#"
doc test
fact name: "Unclosed string
fact age: 25
"#,
            "test.lemma",
            &ResourceLimits::default(),
        );

        match result {
            Err(Error::Parsing(details)) => {
                let src = details.source.as_ref().expect("should have source");
                assert_eq!(src.attribute, "test.lemma");
                assert_eq!(src.doc_name, "");
            }
            Err(e) => panic!("Expected Parse error, got: {e:?}"),
            Ok(_) => panic!("Expected parse error for unclosed string"),
        }
    }

    #[test]
    fn parse_registry_style_doc_name() {
        let input = r#"doc user/workspace/somedoc
fact name: "Alice""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "user/workspace/somedoc");
    }

    #[test]
    fn parse_fact_doc_reference_with_at_prefix() {
        let input = r#"doc example
fact external: doc @user/workspace/somedoc"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 1);
        match &result[0].facts[0].value {
            crate::FactValue::DocumentReference(doc_ref) => {
                assert_eq!(doc_ref.name, "user/workspace/somedoc");
                assert!(doc_ref.is_registry, "expected registry reference");
            }
            other => panic!("Expected DocumentReference, got: {:?}", other),
        }
    }

    #[test]
    fn parse_type_import_with_at_prefix() {
        let input = r#"doc example
type money from @lemma/std/finance
fact price: [money]"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].types.len(), 1);
        match &result[0].types[0] {
            crate::TypeDef::Import { from, name, .. } => {
                assert_eq!(from.name, "lemma/std/finance");
                assert!(from.is_registry, "expected registry reference");
                assert_eq!(name, "money");
            }
            other => panic!("Expected Import type, got: {:?}", other),
        }
    }

    #[test]
    fn parse_multiple_registry_docs_in_same_file() {
        let input = r#"doc user/workspace/doc_a
fact x: 10

doc user/workspace/doc_b
fact y: 20
fact a: doc @user/workspace/doc_a"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "user/workspace/doc_a");
        assert_eq!(result[1].name, "user/workspace/doc_b");
    }

    // =====================================================================
    // Versioned document identifier tests (section 6.1 of plan)
    // =====================================================================

    #[test]
    fn parse_registry_doc_ref_name_only() {
        let input = "doc example\nfact x: doc @owner/repo/somedoc";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        match &result[0].facts[0].value {
            crate::FactValue::DocumentReference(doc_ref) => {
                assert_eq!(doc_ref.name, "owner/repo/somedoc");
                assert_eq!(doc_ref.hash_pin, None);
                assert!(doc_ref.is_registry);
            }
            other => panic!("Expected DocumentReference, got: {:?}", other),
        }
    }

    #[test]
    fn parse_registry_doc_ref_name_with_dots_is_whole_name() {
        let input = "doc example\nfact x: doc @owner/repo/somedoc";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        match &result[0].facts[0].value {
            crate::FactValue::DocumentReference(doc_ref) => {
                assert_eq!(doc_ref.name, "owner/repo/somedoc");
                assert!(doc_ref.is_registry);
            }
            other => panic!("Expected DocumentReference, got: {:?}", other),
        }
    }

    #[test]
    fn parse_local_doc_ref_name_only() {
        let input = "doc example\nfact x: doc mydoc";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        match &result[0].facts[0].value {
            crate::FactValue::DocumentReference(doc_ref) => {
                assert_eq!(doc_ref.name, "mydoc");
                assert_eq!(doc_ref.hash_pin, None);
                assert!(!doc_ref.is_registry);
            }
            other => panic!("Expected DocumentReference, got: {:?}", other),
        }
    }

    #[test]
    fn parse_doc_name_with_trailing_dot_is_error() {
        let input = "doc mydoc.\nfact x: 1";
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(
            result.is_err(),
            "Trailing dot after doc name should be a parse error"
        );
    }

    #[test]
    fn parse_type_import_from_registry() {
        let input = "doc example\ntype money from @lemma/std/finance\nfact price: [money]";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        match &result[0].types[0] {
            crate::TypeDef::Import { from, name, .. } => {
                assert_eq!(from.name, "lemma/std/finance");
                assert!(from.is_registry);
                assert_eq!(name, "money");
            }
            other => panic!("Expected Import type, got: {:?}", other),
        }
    }

    #[test]
    fn parse_doc_declaration_no_version() {
        let input = "doc mydoc\nrule x: 1";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result[0].name, "mydoc");
        assert_eq!(result[0].effective_from(), None);
    }

    #[test]
    fn parse_multiple_docs_in_same_file() {
        let input = "doc mydoc_a\nrule x: 1\n\ndoc mydoc_b\nrule x: 2";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "mydoc_a");
        assert_eq!(result[1].name, "mydoc_b");
    }

    #[test]
    fn parse_doc_reference_grammar_accepts_name_only() {
        let input = "doc consumer\nfact m: doc other";
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(result.is_ok(), "doc name without hash should parse");
        let doc_ref = match &result.as_ref().unwrap()[0].facts[0].value {
            crate::FactValue::DocumentReference(r) => r,
            _ => panic!("expected DocumentReference"),
        };
        assert_eq!(doc_ref.name, "other");
        assert_eq!(doc_ref.hash_pin, None);
    }

    #[test]
    fn parse_doc_reference_with_hash() {
        let input = "doc consumer\nfact cfg: doc config~a1b2c3d4";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc_ref = match &result[0].facts[0].value {
            crate::FactValue::DocumentReference(r) => r,
            other => panic!("expected DocumentReference, got: {:?}", other),
        };
        assert_eq!(doc_ref.name, "config");
        assert_eq!(doc_ref.hash_pin.as_deref(), Some("a1b2c3d4"));
    }

    #[test]
    fn parse_doc_reference_registry_with_hash() {
        let input = "doc consumer\nfact ext: doc @user/workspace/cfg~ab12cd34";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc_ref = match &result[0].facts[0].value {
            crate::FactValue::DocumentReference(r) => r,
            other => panic!("expected DocumentReference, got: {:?}", other),
        };
        assert_eq!(doc_ref.name, "user/workspace/cfg");
        assert!(doc_ref.is_registry);
        assert_eq!(doc_ref.hash_pin.as_deref(), Some("ab12cd34"));
    }

    #[test]
    fn parse_type_import_with_hash() {
        let input = "doc consumer\ntype money from finance a1b2c3d4\nfact p: [money]";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        match &result[0].types[0] {
            crate::TypeDef::Import { from, name, .. } => {
                assert_eq!(name, "money");
                assert_eq!(from.name, "finance");
                assert_eq!(from.hash_pin.as_deref(), Some("a1b2c3d4"));
            }
            other => panic!("expected Import, got: {:?}", other),
        }
    }

    #[test]
    fn parse_type_import_registry_with_hash() {
        let input = "doc consumer\ntype money from @lemma/std/finance ab12cd34\nfact p: [money]";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        match &result[0].types[0] {
            crate::TypeDef::Import { from, name, .. } => {
                assert_eq!(name, "money");
                assert_eq!(from.name, "lemma/std/finance");
                assert!(from.is_registry);
                assert_eq!(from.hash_pin.as_deref(), Some("ab12cd34"));
            }
            other => panic!("expected Import, got: {:?}", other),
        }
    }

    #[test]
    fn parse_inline_type_from_with_hash() {
        let input = "doc consumer\nfact price: [money from finance a1b2c3d4 -> minimum 0]";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        match &result[0].facts[0].value {
            crate::FactValue::TypeDeclaration {
                base,
                from,
                constraints,
                ..
            } => {
                assert_eq!(base, "money");
                let doc_ref = from.as_ref().expect("expected from doc ref");
                assert_eq!(doc_ref.name, "finance");
                assert_eq!(doc_ref.hash_pin.as_deref(), Some("a1b2c3d4"));
                assert!(constraints.is_some());
            }
            other => panic!("expected TypeDeclaration, got: {:?}", other),
        }
    }

    #[test]
    fn parse_type_import_doc_name_with_slashes() {
        let input = "doc consumer\ntype money from @lemma/std/finance\nfact p: [money]";
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(result.is_ok(), "type import from registry should parse");
        match &result.unwrap()[0].types[0] {
            crate::TypeDef::Import { from, .. } => assert_eq!(from.name, "lemma/std/finance"),
            _ => panic!("expected Import"),
        }
    }

    #[test]
    fn parse_error_is_returned_for_garbage_input() {
        let result = parse(
            r#"
doc test
this is not valid lemma syntax @#$%
"#,
            "test.lemma",
            &ResourceLimits::default(),
        );

        assert!(result.is_err(), "Should fail on malformed input");
        match result {
            Err(Error::Parsing { .. }) => {
                // Expected
            }
            Err(e) => panic!("Expected Parse error, got: {e:?}"),
            Ok(_) => panic!("Expected parse error"),
        }
    }
}
