use super::ast::{DepthTracker, Span};
use super::Rule;
use crate::error::LemmaError;
use crate::parsing::ast::*;
use crate::Source;
use pest::iterators::Pair;
use std::sync::Arc;

pub(crate) fn parse_rule_definition(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<LemmaRule, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let mut rule_name = None;
    let mut rule_expression = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::rule_name => rule_name = Some(inner_pair.as_str().to_string()),
            Rule::rule_expression => {
                rule_expression = Some(parse_rule_expression(
                    inner_pair,
                    depth_tracker,
                    attribute,
                    doc_name,
                    source_text.clone(),
                )?)
            }
            _ => {}
        }
    }

    let name = rule_name.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: rule_definition missing rule_name",
            Some(Source::new(
                attribute,
                span.clone(),
                doc_name,
                source_text.clone(),
            )),
            None::<String>,
        )
    })?;
    let (expression, unless_clauses) = rule_expression.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: rule_definition missing rule_expression",
            Some(Source::new(
                attribute,
                span.clone(),
                doc_name,
                source_text.clone(),
            )),
            None::<String>,
        )
    })?;

    Ok(LemmaRule {
        name,
        expression,
        unless_clauses,
        source_location: Source::new(attribute, span.clone(), doc_name, source_text.clone()),
    })
}

fn parse_rule_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<(Expression, Vec<UnlessClause>), LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let mut expression = None;
    let mut unless_clauses = Vec::new();

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::expression => {
                expression = Some(crate::parsing::expressions::parse_expression(
                    inner_pair,
                    depth_tracker,
                    attribute,
                    doc_name,
                    source_text.clone(),
                )?);
            }
            Rule::veto_expression => {
                expression = Some(parse_veto_expression(
                    inner_pair,
                    attribute,
                    doc_name,
                    source_text.clone(),
                )?);
            }
            Rule::unless_statement => {
                let unless_clause = parse_unless_statement(
                    inner_pair,
                    depth_tracker,
                    attribute,
                    doc_name,
                    source_text.clone(),
                )?;
                unless_clauses.push(unless_clause);
            }
            _ => {}
        }
    }

    let expr = expression.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: rule_expression missing expression",
            Some(Source::new(attribute, span, doc_name, source_text.clone())),
            None::<String>,
        )
    })?;
    Ok((expr, unless_clauses))
}

fn parse_veto_expression(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, LemmaError> {
    let veto_span = Span::from_pest_span(pair.as_span());
    // Pest grammar: ^"veto" ~ (SPACE+ ~ text_literal)?
    // If text_literal child exists, parse it via the existing literal parser (same path as other types).
    let message = match pair
        .clone()
        .into_inner()
        .find(|p| p.as_rule() == Rule::text_literal)
    {
        Some(string_pair) => {
            let value = crate::parsing::literals::parse_literal(
                string_pair.clone(),
                attribute,
                doc_name,
                source_text.clone(),
            )?;
            match value {
                Value::Text(s) => Some(s),
                _ => {
                    let span = Span::from_pest_span(string_pair.as_span());
                    return Err(LemmaError::engine(
                        "veto message must be a text literal",
                        Some(Source::new(attribute, span, doc_name, source_text.clone())),
                        None::<String>,
                    ));
                }
            }
        }
        None => None,
    };
    let kind = ExpressionKind::Veto(VetoExpression { message });
    Ok(Expression::new(
        kind,
        Source::new(attribute, veto_span, doc_name, source_text.clone()),
    ))
}

fn parse_unless_statement(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<UnlessClause, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let mut condition = None;
    let mut result = None;

    for inner_pair in pair.clone().into_inner() {
        match inner_pair.as_rule() {
            Rule::expression => {
                if condition.is_none() {
                    condition = Some(crate::parsing::expressions::parse_expression(
                        inner_pair,
                        depth_tracker,
                        attribute,
                        doc_name,
                        source_text.clone(),
                    )?);
                } else {
                    result = Some(crate::parsing::expressions::parse_expression(
                        inner_pair,
                        depth_tracker,
                        attribute,
                        doc_name,
                        source_text.clone(),
                    )?);
                }
            }
            Rule::veto_expression => {
                result = Some(parse_veto_expression(
                    inner_pair,
                    attribute,
                    doc_name,
                    source_text.clone(),
                )?);
            }
            _ => {}
        }
    }

    let cond = condition.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: unless_statement missing condition",
            Some(Source::new(
                attribute,
                span.clone(),
                doc_name,
                source_text.clone(),
            )),
            None::<String>,
        )
    })?;
    let res = result.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: unless_statement missing result",
            Some(Source::new(
                attribute,
                span.clone(),
                doc_name,
                source_text.clone(),
            )),
            None::<String>,
        )
    })?;

    Ok(UnlessClause {
        condition: cond,
        result: res,
        source_location: Source::new(attribute, span.clone(), doc_name, source_text.clone()),
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::parsing::parse;
    use crate::{ExpressionKind, ResourceLimits, Value};

    #[test]
    fn parse_document_with_unless_clause_records_unless_clause() {
        let input = r#"doc person
rule is_active = service_started? and not service_ended?
unless maintenance_mode then false"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].unless_clauses.len(), 1);
    }

    #[test]
    fn parse_multiple_unless_clauses_records_all_unless_clauses() {
        let input = r#"doc test
rule is_eligible = age >= 18 and has_license
unless emergency_mode then true
unless system_override then accept"#;

        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].unless_clauses.len(), 2);
    }

    #[test]
    fn parse_multiple_rules_in_document_preserves_rule_names() {
        let input = r#"doc test
rule is_adult = age >= 18
rule is_senior = age >= 65
rule is_minor = age < 18
rule can_vote = age >= 18 and is_citizen"#;

        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 4);
        assert_eq!(result[0].rules[0].name, "is_adult");
        assert_eq!(result[0].rules[1].name, "is_senior");
        assert_eq!(result[0].rules[2].name, "is_minor");
        assert_eq!(result[0].rules[3].name, "can_vote");
    }

    #[test]
    fn veto_in_unless_clauses_parses_with_message() {
        let input = r#"doc test
rule is_adult = age >= 18 unless age < 0 then veto "Age must be 0 or higher""#;
        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].rules.len(), 1);

        let rule = &docs[0].rules[0];
        assert_eq!(rule.name, "is_adult");
        assert_eq!(rule.unless_clauses.len(), 1);

        match &rule.unless_clauses[0].result.kind {
            ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, Some("Age must be 0 or higher".to_string()));
            }
            other => panic!("Expected veto expression, got {:?}", other),
        }

        let input = r#"doc test
rule is_adult = age >= 18
  unless age > 150 then veto "Age cannot be over 150"
  unless age < 0 then veto "Age must be 0 or higher""#;
        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let rule = &docs[0].rules[0];
        assert_eq!(rule.unless_clauses.len(), 2);

        match &rule.unless_clauses[0].result.kind {
            ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, Some("Age cannot be over 150".to_string()));
            }
            other => panic!("Expected veto expression, got {:?}", other),
        }

        match &rule.unless_clauses[1].result.kind {
            ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, Some("Age must be 0 or higher".to_string()));
            }
            other => panic!("Expected veto expression, got {:?}", other),
        }
    }

    #[test]
    fn veto_without_message_parses_as_veto_with_no_message() {
        let input = r#"doc test
rule adult = age >= 18 unless age > 150 then veto"#;
        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let rule = &docs[0].rules[0];
        assert_eq!(rule.unless_clauses.len(), 1);

        match &rule.unless_clauses[0].result.kind {
            ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, None);
            }
            other => panic!("Expected veto expression, got {:?}", other),
        }
    }

    #[test]
    fn mixed_veto_and_regular_unless_parses_both_results() {
        let input = r#"doc test
rule adjusted_age = age + 1
  unless age < 0 then veto "Invalid age"
  unless age > 100 then 100"#;
        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let rule = &docs[0].rules[0];
        assert_eq!(rule.unless_clauses.len(), 2);

        match &rule.unless_clauses[0].result.kind {
            ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, Some("Invalid age".to_string()));
            }
            other => panic!("Expected veto expression, got {:?}", other),
        }

        match &rule.unless_clauses[1].result.kind {
            ExpressionKind::Literal(lit) => match lit {
                Value::Number(n) => assert_eq!(*n, rust_decimal::Decimal::new(100, 0)),
                other => panic!("Expected literal number, got {:?}", other),
            },
            other => panic!("Expected literal result, got {:?}", other),
        }
    }
}
