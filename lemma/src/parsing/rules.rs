use super::ast::{DepthTracker, Span};
use super::Rule;
use crate::error::LemmaError;
use crate::semantic::*;
use crate::Source;
use pest::iterators::Pair;
use std::sync::Arc;

pub(crate) fn parse_rule_definition(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<LemmaRule, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let pair_str = pair.as_str();
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
                )?)
            }
            _ => {}
        }
    }

    let name = rule_name.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: rule_definition missing rule_name",
            span.clone(),
            attribute,
            Arc::from(pair_str),
            doc_name,
            1,
            None::<String>,
        )
    })?;
    let (expression, unless_clauses) = rule_expression.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: rule_definition missing rule_expression",
            span.clone(),
            attribute,
            Arc::from(pair_str),
            doc_name,
            1,
            None::<String>,
        )
    })?;

    Ok(LemmaRule {
        name,
        expression,
        unless_clauses,
        source_location: Some(Source::new(
            attribute.to_string(),
            span.clone(),
            doc_name.to_string(),
        )),
    })
}

fn parse_rule_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<(Expression, Vec<UnlessClause>), LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let pair_str = pair.as_str();
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
                )?);
            }
            Rule::veto_expression => {
                expression = Some(parse_veto_expression(inner_pair, attribute, doc_name)?);
            }
            Rule::unless_statement => {
                let unless_clause =
                    parse_unless_statement(inner_pair, depth_tracker, attribute, doc_name)?;
                unless_clauses.push(unless_clause);
            }
            _ => {}
        }
    }

    let expr = expression.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: rule_expression missing expression",
            span,
            attribute,
            Arc::from(pair_str),
            doc_name,
            1,
            None::<String>,
        )
    })?;
    Ok((expr, unless_clauses))
}

fn parse_veto_expression(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let veto_span = Span::from_pest_span(pair.as_span());
    // Pest grammar: ^"veto" ~ (SPACE+ ~ text_literal)?
    // If text_literal child exists, extract the string content (without quotes)
    let message = pair
        .clone()
        .into_inner()
        .find(|p| p.as_rule() == Rule::text_literal)
        .map(|string_pair| {
            let content = string_pair.as_str();
            content[1..content.len() - 1].to_string()
        });
    let kind = ExpressionKind::Veto(VetoExpression { message });
    Ok(Expression::new(
        kind,
        Some(Source::new(
            attribute.to_string(),
            veto_span,
            doc_name.to_string(),
        )),
    ))
}

fn parse_unless_statement(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
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
                    )?);
                } else {
                    result = Some(crate::parsing::expressions::parse_expression(
                        inner_pair,
                        depth_tracker,
                        attribute,
                        doc_name,
                    )?);
                }
            }
            Rule::veto_expression => {
                result = Some(parse_veto_expression(inner_pair, attribute, doc_name)?);
            }
            _ => {}
        }
    }

    let cond = condition.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: unless_statement missing condition",
            span.clone(),
            attribute,
            Arc::from(pair.as_str()),
            doc_name,
            1,
            None::<String>,
        )
    })?;
    let res = result.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: unless_statement missing result",
            span.clone(),
            attribute,
            Arc::from(pair.as_str()),
            doc_name,
            1,
            None::<String>,
        )
    })?;

    Ok(UnlessClause {
        condition: cond,
        result: res,
        source_location: Some(Source::new(
            attribute.to_string(),
            span.clone(),
            doc_name.to_string(),
        )),
    })
}
