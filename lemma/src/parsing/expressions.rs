use super::ast::{DepthTracker, Span};
use super::Rule;
use crate::error::LemmaError;
use crate::semantic::*;
use crate::Source;
use pest::iterators::Pair;
use std::sync::Arc;

fn create_expression_with_location(
    kind: ExpressionKind,
    pair: &Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Expression {
    let span = Span::from_pest_span(pair.as_span());
    Expression::new(
        kind,
        Some(Source::new(
            attribute.to_string(),
            span,
            doc_name.to_string(),
        )),
    )
}

fn parse_literal_expression(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let literal_pair = if pair.as_rule() == Rule::literal {
        let span = Span::from_pest_span(pair.as_span());
        pair.into_inner().next().ok_or_else(|| {
            LemmaError::engine(
                "Empty literal wrapper",
                span,
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            )
        })?
    } else {
        pair.clone()
    };

    // Handle number+unit literals specially - they create UnresolvedUnitLiteral expressions
    if literal_pair.as_rule() == Rule::number_unit_literal {
        let (number, unit_name) =
            crate::parsing::literals::parse_number_unit_literal(literal_pair.clone())?;
        return Ok(create_expression_with_location(
            ExpressionKind::UnresolvedUnitLiteral(number, unit_name),
            &literal_pair,
            attribute,
            doc_name,
        ));
    }

    let literal_value = crate::parsing::literals::parse_literal(literal_pair.clone())?;

    Ok(create_expression_with_location(
        ExpressionKind::Literal(literal_value),
        &literal_pair,
        attribute,
        doc_name,
    ))
}

pub(crate) fn parse_primary(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let rule = pair.as_rule();
    match rule {
        Rule::literal
        | Rule::number_literal
        | Rule::text_literal
        | Rule::boolean_literal
        | Rule::percent_literal
        | Rule::date_time_literal
        | Rule::time_literal
        | Rule::duration_literal
        | Rule::number_unit_literal => {
            return parse_literal_expression(pair, attribute, doc_name);
        }
        Rule::rule_reference => {
            let rule_ref = parse_rule_reference(pair.clone())?;
            return Ok(create_expression_with_location(
                ExpressionKind::RuleReference(rule_ref),
                &pair,
                attribute,
                doc_name,
            ));
        }
        Rule::fact_reference => {
            let reference = parse_fact_reference(pair.clone())?;
            return Ok(create_expression_with_location(
                ExpressionKind::FactReference(reference),
                &pair,
                attribute,
                doc_name,
            ));
        }
        Rule::sqrt_expr
        | Rule::sin_expr
        | Rule::cos_expr
        | Rule::tan_expr
        | Rule::asin_expr
        | Rule::acos_expr
        | Rule::atan_expr
        | Rule::log_expr
        | Rule::exp_expr
        | Rule::abs_expr
        | Rule::floor_expr
        | Rule::ceil_expr
        | Rule::round_expr => {
            return parse_logical_expression(pair, depth_tracker, attribute, doc_name);
        }
        _ => {}
    }

    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::literal
            | Rule::number_literal
            | Rule::text_literal
            | Rule::boolean_literal
            | Rule::percent_literal
            | Rule::date_time_literal
            | Rule::time_literal
            | Rule::duration_literal
            | Rule::number_unit_literal => {
                return parse_literal_expression(inner, attribute, doc_name);
            }
            Rule::rule_reference => {
                let rule_ref = parse_rule_reference(inner.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::RuleReference(rule_ref),
                    &inner,
                    attribute,
                    doc_name,
                ));
            }
            Rule::fact_reference => {
                let reference = parse_fact_reference(inner.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::FactReference(reference),
                    &inner,
                    attribute,
                    doc_name,
                ));
            }
            Rule::expression => {
                return parse_expression(inner, depth_tracker, attribute, doc_name);
            }
            Rule::sqrt_expr
            | Rule::sin_expr
            | Rule::cos_expr
            | Rule::tan_expr
            | Rule::asin_expr
            | Rule::acos_expr
            | Rule::atan_expr
            | Rule::log_expr
            | Rule::exp_expr
            | Rule::abs_expr
            | Rule::floor_expr
            | Rule::ceil_expr
            | Rule::round_expr => {
                return parse_logical_expression(inner, depth_tracker, attribute, doc_name);
            }
            _ => {}
        }
    }
    Err(LemmaError::engine(
        "Empty primary expression",
        Span {
            start: 0,
            end: 0,
            line: 1,
            col: 0,
        },
        attribute,
        Arc::from(""),
        doc_name,
        1,
        None::<String>,
    ))
}

pub(crate) fn parse_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    if let Err(msg) = depth_tracker.push_depth() {
        let actual_depth = msg
            .split_whitespace()
            .nth(2)
            .and_then(|s| s.parse::<usize>().ok())
            .map(|d| d.to_string())
            .unwrap_or_else(|| format!("parse error: {}", msg));
        return Err(LemmaError::ResourceLimitExceeded {
            limit_name: "max_expression_depth".to_string(),
            limit_value: depth_tracker.max_depth().to_string(),
            actual_value: actual_depth,
            suggestion: "Simplify nested expressions to reduce depth".to_string(),
        });
    }

    let result = parse_expression_impl(pair, depth_tracker, attribute, doc_name);
    depth_tracker.pop_depth();
    result
}

fn parse_expression_impl(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    match pair.as_rule() {
        Rule::expression => {
            let original = pair.clone();
            let mut inner = pair.into_inner();

            let span = Span::from_pest_span(original.as_span());
            let mut left = parse_and_expression(
                inner.next().ok_or_else(|| {
                    LemmaError::engine(
                        "Missing left operand in logical OR expression",
                        span,
                        attribute,
                        Arc::from(""),
                        doc_name,
                        1,
                        None::<String>,
                    )
                })?,
                depth_tracker,
                attribute,
                doc_name,
            )?;

            for child in inner {
                if child.as_rule() == Rule::and_expression {
                    let right =
                        parse_and_expression(child.clone(), depth_tracker, attribute, doc_name)?;
                    let kind = ExpressionKind::LogicalOr(Arc::new(left), Arc::new(right));
                    left = create_expression_with_location(kind, &original, attribute, doc_name);
                }
            }

            return Ok(left);
        }

        Rule::and_expression => {
            return parse_and_expression(pair, depth_tracker, attribute, doc_name);
        }

        Rule::and_operand => {
            return parse_and_operand(pair, depth_tracker, attribute, doc_name);
        }

        Rule::base_expression => {
            return parse_base_expression(pair, depth_tracker, attribute, doc_name);
        }
        Rule::term => return parse_term(pair, depth_tracker, attribute, doc_name),
        Rule::power => return parse_power(pair, depth_tracker, attribute, doc_name),
        Rule::factor => return parse_factor(pair, depth_tracker, attribute, doc_name),
        Rule::primary => return parse_primary(pair, depth_tracker, attribute, doc_name),

        Rule::conversion_expression => {
            return parse_conversion_expression(pair, depth_tracker, attribute, doc_name);
        }

        Rule::comparison_expression => {
            return parse_comparison_expression(pair, depth_tracker, attribute, doc_name)
        }

        Rule::sqrt_expr
        | Rule::sin_expr
        | Rule::cos_expr
        | Rule::tan_expr
        | Rule::asin_expr
        | Rule::acos_expr
        | Rule::atan_expr
        | Rule::log_expr
        | Rule::exp_expr
        | Rule::abs_expr
        | Rule::floor_expr
        | Rule::ceil_expr
        | Rule::round_expr
        | Rule::not_expr => {
            return parse_logical_expression(pair, depth_tracker, attribute, doc_name)
        }
        _ => {}
    }

    for inner_pair in pair.clone().into_inner() {
        match inner_pair.as_rule() {
            Rule::literal
            | Rule::number_literal
            | Rule::text_literal
            | Rule::boolean_literal
            | Rule::percent_literal
            | Rule::date_time_literal
            | Rule::time_literal
            | Rule::duration_literal => {
                return parse_literal_expression(inner_pair, attribute, doc_name);
            }

            Rule::rule_reference => {
                let rule_ref = parse_rule_reference(inner_pair.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::RuleReference(rule_ref),
                    &inner_pair,
                    attribute,
                    doc_name,
                ));
            }

            Rule::fact_reference => {
                let reference = parse_fact_reference(inner_pair.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::FactReference(reference),
                    &inner_pair,
                    attribute,
                    doc_name,
                ));
            }

            Rule::conversion_expression => {
                return parse_conversion_expression(inner_pair, depth_tracker, attribute, doc_name);
            }
            Rule::expression
            | Rule::and_expression
            | Rule::and_operand
            | Rule::comparison_expression
            | Rule::base_expression
            | Rule::term
            | Rule::power
            | Rule::factor
            | Rule::primary => {
                return parse_expression(inner_pair, depth_tracker, attribute, doc_name);
            }

            Rule::not_expr
            | Rule::sqrt_expr
            | Rule::sin_expr
            | Rule::cos_expr
            | Rule::tan_expr
            | Rule::asin_expr
            | Rule::acos_expr
            | Rule::atan_expr
            | Rule::log_expr
            | Rule::exp_expr
            | Rule::abs_expr
            | Rule::floor_expr
            | Rule::ceil_expr
            | Rule::round_expr => {
                return parse_logical_expression(inner_pair, depth_tracker, attribute, doc_name);
            }

            _ => {}
        }
    }

    let span = Span::from_pest_span(pair.as_span());
    Err(LemmaError::engine(
        format!(
            "Invalid expression: unable to parse '{}' as any valid expression type",
            pair.as_str()
        ),
        span,
        attribute,
        Arc::from(""),
        doc_name,
        1,
        None::<String>,
    ))
}

fn parse_rule_reference(pair: Pair<Rule>) -> Result<RuleReference, LemmaError> {
    let parts: Vec<String> = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::rule_reference_segment)
        .map(|p| p.as_str().to_string())
        .collect();
    let reference = RuleReference::from_path(parts);
    Ok(reference)
}

fn parse_fact_reference(pair: Pair<Rule>) -> Result<FactReference, LemmaError> {
    let parts: Vec<String> = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::fact_reference_segment)
        .map(|p| p.as_str().to_string())
        .collect();
    let reference = FactReference::from_path(parts);
    Ok(reference)
}

fn parse_and_operand(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    match pair.as_rule() {
        Rule::and_operand => {
            let span = Span::from_pest_span(pair.as_span());
            let mut inner = pair.into_inner();
            let first = inner.next().ok_or_else(|| {
                LemmaError::engine(
                    "Empty and_operand",
                    span,
                    attribute,
                    Arc::from(""),
                    doc_name,
                    1,
                    None::<String>,
                )
            })?;
            parse_and_operand(first, depth_tracker, attribute, doc_name)
        }
        Rule::not_expr => parse_not_expression(pair, depth_tracker, attribute, doc_name),
        Rule::comparison_expression => {
            parse_comparison_expression(pair, depth_tracker, attribute, doc_name)
        }
        Rule::conversion_expression => {
            parse_conversion_expression(pair, depth_tracker, attribute, doc_name)
        }
        Rule::base_expression => parse_base_expression(pair, depth_tracker, attribute, doc_name),
        Rule::term | Rule::power | Rule::factor | Rule::primary => {
            parse_expression_impl(pair, depth_tracker, attribute, doc_name)
        }
        _ => parse_expression_impl(pair, depth_tracker, attribute, doc_name),
    }
}

fn parse_and_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let original_pair = pair.clone();
    let span = Span::from_pest_span(original_pair.as_span());
    let mut pairs = pair.into_inner();
    let mut left = parse_and_operand(
        pairs.next().ok_or_else(|| {
            LemmaError::engine(
                "Missing left operand in logical AND expression",
                span,
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            )
        })?,
        depth_tracker,
        attribute,
        doc_name,
    )?;

    for right_pair in pairs {
        if right_pair.as_rule() == Rule::and_operand {
            let right = parse_and_operand(right_pair.clone(), depth_tracker, attribute, doc_name)?;
            let kind = ExpressionKind::LogicalAnd(Arc::new(left), Arc::new(right));
            left = create_expression_with_location(kind, &original_pair, attribute, doc_name);
        }
    }

    Ok(left)
}

fn parse_base_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let original_pair = pair.clone();
    let span = Span::from_pest_span(original_pair.as_span());
    let mut inner = pair.into_inner();

    let mut left = parse_term(
        inner.next().ok_or_else(|| {
            LemmaError::engine(
                "Missing left term in base_expression",
                span.clone(),
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            )
        })?,
        depth_tracker,
        attribute,
        doc_name,
    )?;

    while let Some(op_pair) = inner.next() {
        let operation = match op_pair.as_rule() {
            Rule::op_add => ArithmeticComputation::Add,
            Rule::op_sub => ArithmeticComputation::Subtract,
            other => {
                let span = Span::from_pest_span(op_pair.as_span());
                return Err(LemmaError::engine(
                    format!("Unexpected operator in base_expression: {:?}", other),
                    span,
                    attribute,
                    Arc::from(""),
                    doc_name,
                    1,
                    None::<String>,
                ));
            }
        };

        let right_term_pair = inner.next().ok_or_else(|| {
            LemmaError::engine(
                "Missing right term after + or - in base_expression",
                span.clone(),
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            )
        })?;

        let right = parse_term(right_term_pair, depth_tracker, attribute, doc_name)?;

        let kind = ExpressionKind::Arithmetic(Arc::new(left), operation, Arc::new(right));
        left = create_expression_with_location(kind, &original_pair, attribute, doc_name);
    }

    Ok(left)
}

fn parse_conversion_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let original_pair = pair.clone();
    let mut base: Option<Expression> = None;
    let mut unit: Option<String> = None;

    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::base_expression => {
                base = Some(parse_base_expression(
                    inner,
                    depth_tracker,
                    attribute,
                    doc_name,
                )?);
            }
            Rule::duration_unit => {
                unit = Some(inner.as_str().to_string());
            }
            _ => {}
        }
    }

    let span = Span::from_pest_span(original_pair.as_span());
    let base_expr = base.ok_or_else(|| {
        LemmaError::engine(
            "Missing base expression in conversion_expression",
            span.clone(),
            attribute,
            Arc::from(""),
            doc_name,
            1,
            None::<String>,
        )
    })?;
    let unit_name = unit.ok_or_else(|| {
        LemmaError::engine(
            "Missing unit in conversion_expression",
            span.clone(),
            attribute,
            Arc::from(""),
            doc_name,
            1,
            None::<String>,
        )
    })?;

    let lower = unit_name.to_ascii_lowercase();
    let target = match lower.as_str() {
        "percent" => ConversionTarget::Percentage,
        "year" | "years" => ConversionTarget::Duration(DurationUnit::Year),
        "month" | "months" => ConversionTarget::Duration(DurationUnit::Month),
        "week" | "weeks" => ConversionTarget::Duration(DurationUnit::Week),
        "day" | "days" => ConversionTarget::Duration(DurationUnit::Day),
        "hour" | "hours" => ConversionTarget::Duration(DurationUnit::Hour),
        "minute" | "minutes" => ConversionTarget::Duration(DurationUnit::Minute),
        "second" | "seconds" => ConversionTarget::Duration(DurationUnit::Second),
        "millisecond" | "milliseconds" => ConversionTarget::Duration(DurationUnit::Millisecond),
        "microsecond" | "microseconds" => ConversionTarget::Duration(DurationUnit::Microsecond),
        _ => {
            let span = Span::from_pest_span(original_pair.as_span());
            return Err(LemmaError::engine(
                format!(
                    "Unknown conversion target: '{}'. Expected one of: percent, years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds",
                    unit_name
                ),
                span,
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            ));
        }
    };

    let kind = ExpressionKind::UnitConversion(Arc::new(base_expr), target);

    Ok(create_expression_with_location(
        kind,
        &original_pair,
        attribute,
        doc_name,
    ))
}

fn parse_term(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let mut pairs = pair.clone().into_inner();
    let mut left = parse_power(
        pairs.next().ok_or_else(|| {
            LemmaError::engine(
                "Missing left power in term",
                span.clone(),
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            )
        })?,
        depth_tracker,
        attribute,
        doc_name,
    )?;

    while let Some(op_pair) = pairs.next() {
        let operation = match op_pair.as_rule() {
            Rule::op_mul => ArithmeticComputation::Multiply,
            Rule::op_div => ArithmeticComputation::Divide,
            Rule::op_mod => ArithmeticComputation::Modulo,
            _ => {
                let span = Span::from_pest_span(op_pair.as_span());
                return Err(LemmaError::engine(
                    format!("Unexpected operator in term: {:?}", op_pair.as_rule()),
                    span,
                    attribute,
                    Arc::from(""),
                    doc_name,
                    1,
                    None::<String>,
                ));
            }
        };

        let right = parse_power(
            pairs.next().ok_or_else(|| {
                LemmaError::engine(
                    "Missing right power in term",
                    span.clone(),
                    attribute,
                    Arc::from(""),
                    doc_name,
                    1,
                    None::<String>,
                )
            })?,
            depth_tracker,
            attribute,
            doc_name,
        )?;

        let kind = ExpressionKind::Arithmetic(Arc::new(left), operation, Arc::new(right));
        left = create_expression_with_location(kind, &pair, attribute, doc_name);
    }

    Ok(left)
}

fn parse_power(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let mut pairs = pair.clone().into_inner();
    let left = parse_factor(
        pairs.next().ok_or_else(|| {
            LemmaError::engine(
                "Missing factor in power",
                span.clone(),
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            )
        })?,
        depth_tracker,
        attribute,
        doc_name,
    )?;

    if let Some(op_pair) = pairs.next() {
        if op_pair.as_rule() == Rule::op_pow {
            let right = parse_power(
                pairs.next().ok_or_else(|| {
                    LemmaError::engine(
                        "Missing right power in power expression",
                        span.clone(),
                        attribute,
                        Arc::from(""),
                        doc_name,
                        1,
                        None::<String>,
                    )
                })?,
                depth_tracker,
                attribute,
                doc_name,
            )?;

            let kind = ExpressionKind::Arithmetic(
                Arc::new(left),
                ArithmeticComputation::Power,
                Arc::new(right),
            );
            return Ok(create_expression_with_location(
                kind, &pair, attribute, doc_name,
            ));
        }
    }

    Ok(left)
}

fn parse_factor(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let mut is_negative = false;

    if let Some(first_pair) = pairs.next() {
        match first_pair.as_rule() {
            Rule::op_sub => {
                is_negative = true;
            }
            Rule::op_add => {}
            _ => {
                let expr = parse_primary(first_pair, depth_tracker, attribute, doc_name)?;
                return Ok(expr);
            }
        }
    }

    let span = Span::from_pest_span(pair.as_span());
    let expr = if let Some(expr_pair) = pairs.next() {
        parse_primary(expr_pair, depth_tracker, attribute, doc_name)?
    } else {
        return Err(LemmaError::engine(
            "Missing expression after unary operator",
            span,
            attribute,
            Arc::from(""),
            doc_name,
            1,
            None::<String>,
        ));
    };

    if is_negative {
        let zero = create_expression_with_location(
            ExpressionKind::Literal(LiteralValue::number(rust_decimal::Decimal::ZERO)),
            &pair,
            attribute,
            doc_name,
        );
        let kind = ExpressionKind::Arithmetic(
            Arc::new(zero),
            ArithmeticComputation::Subtract,
            Arc::new(expr),
        );
        Ok(create_expression_with_location(
            kind, &pair, attribute, doc_name,
        ))
    } else {
        Ok(expr)
    }
}

fn parse_comparison_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let mut pairs = pair.clone().into_inner();
    let left = parse_expression(
        pairs.next().ok_or_else(|| {
            LemmaError::engine(
                "Missing left operand in comparison expression",
                span.clone(),
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            )
        })?,
        depth_tracker,
        attribute,
        doc_name,
    )?;

    if let Some(op_pair) = pairs.next() {
        let operator = match op_pair.as_rule() {
            Rule::comp_operator => {
                let inner_span = Span::from_pest_span(op_pair.as_span());
                let inner_pair = op_pair.into_inner().next().ok_or_else(|| {
                    LemmaError::engine(
                        "Empty comparison operator",
                        inner_span,
                        attribute,
                        Arc::from(""),
                        doc_name,
                        1,
                        None::<String>,
                    )
                })?;
                match inner_pair.as_rule() {
                    Rule::comp_gt => ComparisonComputation::GreaterThan,
                    Rule::comp_lt => ComparisonComputation::LessThan,
                    Rule::comp_gte => ComparisonComputation::GreaterThanOrEqual,
                    Rule::comp_lte => ComparisonComputation::LessThanOrEqual,
                    Rule::comp_eq => ComparisonComputation::Equal,
                    Rule::comp_ne => ComparisonComputation::NotEqual,
                    Rule::comp_is => ComparisonComputation::Is,
                    Rule::comp_is_not => ComparisonComputation::IsNot,
                    _ => {
                        let inner_span = Span::from_pest_span(inner_pair.as_span());
                        return Err(LemmaError::engine(
                            format!("Invalid comparison operator: {:?}", inner_pair.as_rule()),
                            inner_span,
                            attribute,
                            Arc::from(""),
                            doc_name,
                            1,
                            None::<String>,
                        ));
                    }
                }
            }
            Rule::comp_gt => ComparisonComputation::GreaterThan,
            Rule::comp_lt => ComparisonComputation::LessThan,
            Rule::comp_gte => ComparisonComputation::GreaterThanOrEqual,
            Rule::comp_lte => ComparisonComputation::LessThanOrEqual,
            Rule::comp_eq => ComparisonComputation::Equal,
            Rule::comp_ne => ComparisonComputation::NotEqual,
            Rule::comp_is => ComparisonComputation::Is,
            Rule::comp_is_not => ComparisonComputation::IsNot,
            _ => {
                let op_span = Span::from_pest_span(op_pair.as_span());
                return Err(LemmaError::engine(
                    format!("Invalid comparison operator: {:?}", op_pair.as_rule()),
                    op_span,
                    attribute,
                    Arc::from(""),
                    doc_name,
                    1,
                    None::<String>,
                ));
            }
        };

        let right = parse_expression(
            pairs.next().ok_or_else(|| {
                LemmaError::engine(
                    "Missing right operand in comparison expression",
                    span.clone(),
                    attribute,
                    Arc::from(""),
                    doc_name,
                    1,
                    None::<String>,
                )
            })?,
            depth_tracker,
            attribute,
            doc_name,
        )?;

        let kind = ExpressionKind::Comparison(Arc::new(left), operator, Arc::new(right));
        return Ok(create_expression_with_location(
            kind, &pair, attribute, doc_name,
        ));
    }

    Ok(left)
}

fn parse_not_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let original_pair = pair.clone();
    let span = Span::from_pest_span(original_pair.as_span());
    let mut inner = pair.into_inner();
    let operand_pair = inner.next().ok_or_else(|| {
        LemmaError::engine(
            "not: missing expression",
            span,
            attribute,
            Arc::from(""),
            doc_name,
            1,
            None::<String>,
        )
    })?;

    let operand = parse_expression(operand_pair, depth_tracker, attribute, doc_name)?;
    let kind = ExpressionKind::LogicalNegation(Arc::new(operand), NegationType::Not);

    Ok(create_expression_with_location(
        kind,
        &original_pair,
        attribute,
        doc_name,
    ))
}

fn parse_logical_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    match pair.as_rule() {
        Rule::sqrt_expr
        | Rule::sin_expr
        | Rule::cos_expr
        | Rule::tan_expr
        | Rule::asin_expr
        | Rule::acos_expr
        | Rule::atan_expr
        | Rule::log_expr
        | Rule::exp_expr
        | Rule::abs_expr
        | Rule::floor_expr
        | Rule::ceil_expr
        | Rule::round_expr => {
            let operator = match pair.as_rule() {
                Rule::sqrt_expr => MathematicalComputation::Sqrt,
                Rule::sin_expr => MathematicalComputation::Sin,
                Rule::cos_expr => MathematicalComputation::Cos,
                Rule::tan_expr => MathematicalComputation::Tan,
                Rule::asin_expr => MathematicalComputation::Asin,
                Rule::acos_expr => MathematicalComputation::Acos,
                Rule::atan_expr => MathematicalComputation::Atan,
                Rule::log_expr => MathematicalComputation::Log,
                Rule::exp_expr => MathematicalComputation::Exp,
                Rule::abs_expr => MathematicalComputation::Abs,
                Rule::floor_expr => MathematicalComputation::Floor,
                Rule::ceil_expr => MathematicalComputation::Ceil,
                Rule::round_expr => MathematicalComputation::Round,
                unexpected => {
                    unreachable!(
                        "BUG: unexpected rule '{:?}' in mathematical expression parser (attribute={}, doc={})",
                        unexpected, attribute, doc_name
                    )
                }
            };

            for inner in pair.clone().into_inner() {
                match inner.as_rule() {
                    Rule::base_expression => {
                        let operand =
                            parse_base_expression(inner, depth_tracker, attribute, doc_name)?;
                        let kind =
                            ExpressionKind::MathematicalComputation(operator, Arc::new(operand));
                        return Ok(create_expression_with_location(
                            kind, &pair, attribute, doc_name,
                        ));
                    }
                    Rule::term | Rule::primary => {
                        let operand = parse_expression(inner, depth_tracker, attribute, doc_name)?;
                        let kind =
                            ExpressionKind::MathematicalComputation(operator, Arc::new(operand));
                        return Ok(create_expression_with_location(
                            kind, &pair, attribute, doc_name,
                        ));
                    }
                    _ => {}
                }
            }
            let span = Span::from_pest_span(pair.as_span());
            return Err(LemmaError::engine(
                "Mathematical operator missing operand",
                span,
                attribute,
                Arc::from(""),
                doc_name,
                1,
                None::<String>,
            ));
        }
        _ => {}
    }
    let span = Span::from_pest_span(pair.as_span());
    if let Some(node) = pair.into_inner().next() {
        match node.as_rule() {
            Rule::literal => return parse_expression(node, depth_tracker, attribute, doc_name),
            Rule::primary => return parse_primary(node, depth_tracker, attribute, doc_name),
            Rule::not_expr => {
                for inner in node.clone().into_inner() {
                    let negated_expr = match inner.as_rule() {
                        Rule::primary => parse_primary(inner, depth_tracker, attribute, doc_name)?,
                        Rule::literal => {
                            parse_expression(inner, depth_tracker, attribute, doc_name)?
                        }
                        _ => continue,
                    };
                    let kind =
                        ExpressionKind::LogicalNegation(Arc::new(negated_expr), NegationType::Not);
                    return Ok(create_expression_with_location(
                        kind, &node, attribute, doc_name,
                    ));
                }
                let span = Span::from_pest_span(node.as_span());
                return Err(LemmaError::engine(
                    "not: missing expression",
                    span,
                    attribute,
                    Arc::from(""),
                    doc_name,
                    1,
                    None::<String>,
                ));
            }
            Rule::sqrt_expr
            | Rule::sin_expr
            | Rule::cos_expr
            | Rule::tan_expr
            | Rule::asin_expr
            | Rule::acos_expr
            | Rule::atan_expr
            | Rule::log_expr
            | Rule::exp_expr
            | Rule::abs_expr
            | Rule::floor_expr
            | Rule::ceil_expr
            | Rule::round_expr => {
                let operator = match node.as_rule() {
                    Rule::sqrt_expr => MathematicalComputation::Sqrt,
                    Rule::sin_expr => MathematicalComputation::Sin,
                    Rule::cos_expr => MathematicalComputation::Cos,
                    Rule::tan_expr => MathematicalComputation::Tan,
                    Rule::asin_expr => MathematicalComputation::Asin,
                    Rule::acos_expr => MathematicalComputation::Acos,
                    Rule::atan_expr => MathematicalComputation::Atan,
                    Rule::log_expr => MathematicalComputation::Log,
                    Rule::exp_expr => MathematicalComputation::Exp,
                    Rule::abs_expr => MathematicalComputation::Abs,
                    Rule::floor_expr => MathematicalComputation::Floor,
                    Rule::ceil_expr => MathematicalComputation::Ceil,
                    Rule::round_expr => MathematicalComputation::Round,
                    _ => {
                        let span = Span::from_pest_span(node.as_span());
                        return Err(LemmaError::engine(
                            "Unknown mathematical operator",
                            span,
                            attribute,
                            Arc::from(""),
                            doc_name,
                            1,
                            None::<String>,
                        ));
                    }
                };

                for inner in node.clone().into_inner() {
                    match inner.as_rule() {
                        Rule::base_expression => {
                            let operand =
                                parse_base_expression(inner, depth_tracker, attribute, doc_name)?;
                            let kind = ExpressionKind::MathematicalComputation(
                                operator,
                                Arc::new(operand),
                            );
                            return Ok(create_expression_with_location(
                                kind, &node, attribute, doc_name,
                            ));
                        }
                        Rule::term | Rule::primary => {
                            let operand =
                                parse_expression(inner, depth_tracker, attribute, doc_name)?;
                            let kind = ExpressionKind::MathematicalComputation(
                                operator,
                                Arc::new(operand),
                            );
                            return Ok(create_expression_with_location(
                                kind, &node, attribute, doc_name,
                            ));
                        }
                        _ => {}
                    }
                }
                let span = Span::from_pest_span(node.as_span());
                return Err(LemmaError::engine(
                    "Mathematical operator missing operand",
                    span,
                    attribute,
                    Arc::from(""),
                    doc_name,
                    1,
                    None::<String>,
                ));
            }
            _ => {}
        }
    }
    Err(LemmaError::engine(
        "Empty logical expression",
        span,
        attribute,
        Arc::from(""),
        doc_name,
        1,
        None::<String>,
    ))
}

#[cfg(test)]
mod tests {
    use crate::parsing::parse;

    #[test]
    fn test_simple_number() {
        let input = r#"doc test
rule num = 42"#;
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse simple number: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_fact_reference_parsing() {
        let input = r#"doc test
rule simple_ref = age"#;
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse fact reference: {:?}",
            result.err()
        );

        let input = r#"doc test
rule nested_ref = employee.salary"#;
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse nested fact reference: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_arithmetic_operations_work() {
        let cases = vec![
            "2 + 3", "2+1", "5 * 6", "5* 6", "7 % 3", "3%2", "2 ^ 3", "2^3",
        ];
        for expr in cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(&input, "test.lemma", &crate::ResourceLimits::default());
            assert!(
                result.is_ok(),
                "Failed to parse {}: {:?}",
                expr,
                result.err()
            );
        }
    }

    #[test]
    fn test_conversion_expression_parsing() {
        let input = r#"doc test
fact income = 80000
fact total_tax = 20000
rule effective_tax_rate = total_tax? / income in percent"#;

        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse conversion expression with 'in percent': {:?}",
            result.err()
        );
    }

    #[test]
    fn test_comparison_expressions() {
        let test_cases = vec![
            ("age > 18", "greater than"),
            ("age < 65", "less than"),
            ("age >= 18", "greater than or equal"),
            ("age <= 65", "less than or equal"),
            ("age == 25", "equality"),
            ("age != 30", "inequality"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(&input, "test.lemma", &crate::ResourceLimits::default());
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
    fn test_logical_expressions() {
        let test_cases = vec![
            ("is_active and is_verified", "simple and"),
            ("is_student or is_employee", "simple or"),
            ("not is_blocked", "simple not"),
            ("sqrt 16", "square root"),
            ("sin 0", "sine function"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(&input, "test.lemma", &crate::ResourceLimits::default());
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
    fn test_parentheses_syntax_and_spacing_edge_cases() {
        let test_cases = vec![
            // Basic parentheses syntax
            ("not(x)", "not with parentheses no space"),
            ("sqrt(16)", "sqrt with parentheses no space"),
            ("sin(0)", "sin with parentheses no space"),
            ("log(10)", "log with parentheses no space"),
            ("exp(1)", "exp with parentheses no space"),
            ("abs(-5)", "abs with parentheses no space"),
            ("floor(3.7)", "floor with parentheses no space"),
            ("ceil(3.2)", "ceil with parentheses no space"),
            ("round(3.5)", "round with parentheses no space"),
            // Space before opening paren
            ("not (x)", "not with space before paren"),
            ("sqrt (16)", "sqrt with space before paren"),
            ("sin (0)", "sin with space before paren"),
            // Multiple spaces before opening paren
            ("not     (x)", "not with multiple spaces before paren"),
            ("sqrt    (16)", "sqrt with multiple spaces before paren"),
            ("not  (  x  )", "not with spaces around paren and inside"),
            ("sqrt  (  16  )", "sqrt with spaces around paren and inside"),
            // Complex expressions with parentheses
            ("not(x and y)", "not with parentheses containing expression"),
            ("sqrt(x + 1)", "sqrt with parentheses containing arithmetic"),
            ("sin(x * 2)", "sin with parentheses containing arithmetic"),
            // Mixed forms
            ("not(x) and y", "not with parens and regular and"),
            ("sqrt(16) + 2", "sqrt with parens and arithmetic"),
            ("sin(x) * cos(y)", "mixed parentheses and space forms"),
            // Nested function calls
            ("sqrt(sin(0))", "nested function calls"),
            ("not(not(x))", "nested not expressions"),
            // Edge cases with various spacing combinations
            ("not  (  x  )", "not with multiple spaces around"),
            ("sqrt   (   16   )", "sqrt with extreme spacing"),
            ("sin ( x )", "sin with spaces inside"),
            ("log (  x + 1  )", "log with spaces around expression"),
            ("exp (  2 * 3  )", "exp with spaces in complex expr"),
            // Combined with other operators
            ("not(x) or not(y)", "not with parens in or expression"),
            ("sqrt(x) ^ 2", "sqrt with parens and power operator"),
            ("sin(x) * cos(x)", "multiple function calls"),
        ];

        for (expr, description) in test_cases {
            let input = format!(
                "doc test\nfact x = true\nfact y = false\nrule test = {}",
                expr
            );
            let result = parse(&input, "test.lemma", &crate::ResourceLimits::default());
            assert!(
                result.is_ok(),
                "Failed to parse {} ({}): {:?}",
                expr,
                description,
                result.err()
            );
        }
    }
}
