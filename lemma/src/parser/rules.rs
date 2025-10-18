use crate::ast::ExpressionIdGenerator;
use crate::error::LemmaError;
use crate::parser::Rule;
use crate::semantic::*;
use pest::iterators::Pair;

pub(crate) fn parse_rule_definition(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<LemmaRule, LemmaError> {
    let span = crate::ast::Span::from_pest_span(pair.as_span());
    let mut rule_name = None;
    let mut rule_expression = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::rule_name => rule_name = Some(inner_pair.as_str().to_string()),
            Rule::rule_expression => {
                rule_expression = Some(parse_rule_expression(inner_pair, id_gen)?)
            }
            _ => {}
        }
    }

    let name = rule_name.ok_or_else(|| {
        LemmaError::Engine("Grammar error: rule_definition missing rule_name".to_string())
    })?;
    let (expression, unless_clauses) = rule_expression.ok_or_else(|| {
        LemmaError::Engine("Grammar error: rule_definition missing rule_expression".to_string())
    })?;

    Ok(LemmaRule {
        name,
        expression,
        unless_clauses,
        span: Some(span),
    })
}

fn parse_rule_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<(Expression, Vec<UnlessClause>), LemmaError> {
    let mut expression = None;
    let mut unless_clauses = Vec::new();

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::expression_group => {
                expression = Some(crate::parser::expressions::parse_or_expression(
                    inner_pair, id_gen,
                )?);
            }
            Rule::unless_statement => {
                let unless_clause = parse_unless_statement(inner_pair, id_gen)?;
                unless_clauses.push(unless_clause);
            }
            _ => {}
        }
    }

    let expr = expression.ok_or_else(|| {
        LemmaError::Engine("Grammar error: rule_expression missing expression_group".to_string())
    })?;
    Ok((expr, unless_clauses))
}

fn parse_unless_statement(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<UnlessClause, LemmaError> {
    let span = crate::ast::Span::from_pest_span(pair.as_span());
    let mut condition = None;
    let mut result = None;

    for inner_pair in pair.clone().into_inner() {
        match inner_pair.as_rule() {
            Rule::expression_group => {
                if condition.is_none() {
                    condition = Some(crate::parser::expressions::parse_or_expression(
                        inner_pair, id_gen,
                    )?);
                } else {
                    result = Some(crate::parser::expressions::parse_or_expression(
                        inner_pair, id_gen,
                    )?);
                }
            }
            Rule::veto_expression => {
                let veto_span = crate::ast::Span::from_pest_span(inner_pair.as_span());
                // Pest grammar: ^"veto" ~ (SPACE+ ~ string_literal)?
                // If string_literal child exists, extract the string content (without quotes)
                let message = inner_pair
                    .clone()
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::string_literal)
                    .map(|string_pair| {
                        let content = string_pair.as_str();
                        content[1..content.len() - 1].to_string()
                    });
                let kind = ExpressionKind::Veto(VetoExpression { message });
                result = Some(Expression::new(kind, Some(veto_span), id_gen.next_id()));
            }
            _ => {}
        }
    }

    let cond = condition.ok_or_else(|| {
        LemmaError::Engine("Grammar error: unless_statement missing condition".to_string())
    })?;
    let res = result.ok_or_else(|| {
        LemmaError::Engine("Grammar error: unless_statement missing result".to_string())
    })?;

    Ok(UnlessClause {
        condition: cond,
        result: res,
        span: Some(span),
    })
}
