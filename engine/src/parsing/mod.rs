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
                e.variant.to_string(),
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

fn parse_doc(
    pair: Pair<Rule>,
    attribute: &str,
    depth_tracker: &mut DepthTracker,
    source_text: Arc<str>,
) -> Result<LemmaDoc, LemmaError> {
    let doc_start_line = pair.as_span().start_pos().line_col().0;

    let mut doc_name: Option<String> = None;
    let mut commentary: Option<String> = None;
    let mut facts = Vec::new();
    let mut rules = Vec::new();
    let mut types = Vec::new();

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
                    if decl_inner.as_rule() == Rule::doc_name_local {
                        doc_name = Some(decl_inner.as_str().to_string());
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    let name = doc_name.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: doc missing doc_declaration",
            Some(crate::parsing::source::Source::new(
                attribute,
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "",
                source_text.clone(),
            )),
            None::<String>,
        )
    })?;

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
                            &types,
                        )?;
                        facts.push(fact);
                    }
                    Rule::fact_binding => {
                        let fact = crate::parsing::facts::parse_fact_binding(
                            body_item,
                            attribute,
                            &name,
                            source_text.clone(),
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
                            source_text.clone(),
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::LemmaError;
    use crate::ResourceLimits;

    #[test]
    fn parse_empty_input_returns_no_documents() {
        let result = parse("", "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn parse_workspace_file_yields_expected_doc_facts_and_rules() {
        let input = r#"doc person
fact name = "John Doe"
rule adult = true"#;
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
fact name = "John"
rule is_adult = age >= 18
fact age = 25
rule can_drink = age >= 21
fact status = "active"
rule is_eligible = is_adult and status == "active""#;

        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 3);
        assert_eq!(result[0].rules.len(), 3);
    }

    #[test]
    fn parse_simple_document_collects_facts() {
        let input = r#"doc person
fact name = "John"
fact age = 25"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[0].facts.len(), 2);
    }

    #[test]
    fn parse_doc_name_with_slashes_is_preserved() {
        let input = r#"doc contracts/employment/jack
fact name = "Jack""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "contracts/employment/jack");
    }

    #[test]
    fn parse_commentary_block_is_attached_to_doc() {
        let input = r#"doc person
"""
This is a markdown comment
with **bold** text
"""
fact name = "John""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].commentary.is_some());
        assert!(result[0].commentary.as_ref().unwrap().contains("**bold**"));
    }

    #[test]
    fn parse_document_with_rule_collects_rule() {
        let input = r#"doc person
rule is_adult = age >= 18"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].name, "is_adult");
    }

    #[test]
    fn parse_multiple_documents_returns_all_docs() {
        let input = r#"doc person
fact name = "John"

doc company
fact name = "Acme Corp""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[1].name, "company");
    }

    #[test]
    fn parse_allows_duplicate_fact_names() {
        // Duplicate fact names are rejected during planning/validation, not parsing.
        let input = r#"doc person
fact name = "John"
fact name = "Jane""#;
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
rule is_adult = age >= 18
rule is_adult = age >= 21"#;
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
            ("doc test\nrule test = 2+3", "no spaces in arithmetic"),
            ("doc test\nrule test = age>=18", "no spaces in comparison"),
            (
                "doc test\nrule test = age >= 18 and salary>50000",
                "spaces around and keyword",
            ),
            (
                "doc test\nrule test = age  >=  18  and  salary  >  50000",
                "extra spaces",
            ),
            (
                "doc test\nrule test = \n  age >= 18 \n  and \n  salary > 50000",
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
                "doc test\nfact name = \"unclosed string",
                "unclosed string literal",
            ),
            ("doc test\nrule test = 2 + + 3", "double operator"),
            ("doc test\nrule test = (2 + 3", "unclosed parenthesis"),
            ("doc test\nrule test = 2 + 3)", "extra closing paren"),
            // Note: "invalid unit" now parses as a user-defined unit (validated during planning)
            ("doc test\nfact doc = 123", "reserved keyword as fact name"),
            (
                "doc test\nrule rule = true",
                "reserved keyword as rule name",
            ),
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
            let input = format!("doc test\nrule test = {}", expr);
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
            let input = format!("doc test\nrule test = {}", expr);
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
fact name = "Unclosed string
fact age = 25
"#,
            "test.lemma",
            &ResourceLimits::default(),
        );

        match result {
            Err(LemmaError::Parse(details)) => {
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
fact name = "Alice""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "user/workspace/somedoc");
    }

    #[test]
    fn parse_fact_doc_reference_with_at_prefix() {
        let input = r#"doc example
fact external = doc @user/workspace/somedoc"#;
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
fact price = [money]"#;
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
fact x = 10

doc user/workspace/doc_b
fact y = 20
fact a = doc @user/workspace/doc_a"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "user/workspace/doc_a");
        assert_eq!(result[1].name, "user/workspace/doc_b");
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
            Err(LemmaError::Parse { .. }) => {
                // Expected
            }
            Err(e) => panic!("Expected Parse error, got: {e:?}"),
            Ok(_) => panic!("Expected parse error"),
        }
    }
}
