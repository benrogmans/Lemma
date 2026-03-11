use super::ast::{DepthTracker, Span};
use super::Rule;
use crate::error::Error;
use crate::parsing::ast::*;
use crate::Source;
use pest::iterators::Pair;
use std::sync::Arc;

fn create_expression_with_location(
    kind: ExpressionKind,
    pair: &Pair<Rule>,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Expression {
    let span = Span::from_pest_span(pair.as_span());
    Expression::new(
        kind,
        Source::new(
            attribute.to_string(),
            span,
            spec_name.to_string(),
            source_text.clone(),
        ),
    )
}

fn parse_literal_expression(
    pair: Pair<Rule>,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let literal_pair = if pair.as_rule() == Rule::literal {
        pair.into_inner()
            .next()
            .expect("BUG: grammar guarantees literal has inner value")
    } else {
        pair.clone()
    };

    // Handle number+unit literals specially - they create UnresolvedUnitLiteral expressions
    if literal_pair.as_rule() == Rule::number_unit_literal {
        let (number, unit_name) = crate::parsing::literals::parse_number_unit_literal(
            literal_pair.clone(),
            attribute,
            spec_name,
            source_text.clone(),
        )?;
        return Ok(create_expression_with_location(
            ExpressionKind::UnresolvedUnitLiteral(number, unit_name),
            &literal_pair,
            attribute,
            spec_name,
            source_text.clone(),
        ));
    }

    let literal_value = crate::parsing::literals::parse_literal(
        literal_pair.clone(),
        attribute,
        spec_name,
        source_text.clone(),
    )?;

    Ok(create_expression_with_location(
        ExpressionKind::Literal(literal_value),
        &literal_pair,
        attribute,
        spec_name,
        source_text.clone(),
    ))
}

pub(crate) fn parse_primary(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let rule = pair.as_rule();
    match rule {
        Rule::now_literal => {
            return Ok(create_expression_with_location(
                ExpressionKind::Now,
                &pair,
                attribute,
                spec_name,
                source_text.clone(),
            ));
        }
        Rule::literal
        | Rule::number_literal
        | Rule::text_literal
        | Rule::boolean_literal
        | Rule::percent_literal
        | Rule::permille_literal
        | Rule::date_time_literal
        | Rule::time_literal
        | Rule::duration_literal
        | Rule::number_unit_literal => {
            return parse_literal_expression(pair, attribute, spec_name, source_text.clone());
        }
        Rule::reference => {
            let reference = parse_reference(pair.clone())?;
            return Ok(create_expression_with_location(
                ExpressionKind::Reference(reference),
                &pair,
                attribute,
                spec_name,
                source_text.clone(),
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
            return parse_logical_expression(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            );
        }
        _ => {}
    }

    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::now_literal => {
                return Ok(create_expression_with_location(
                    ExpressionKind::Now,
                    &inner,
                    attribute,
                    spec_name,
                    source_text.clone(),
                ));
            }
            Rule::literal
            | Rule::number_literal
            | Rule::text_literal
            | Rule::boolean_literal
            | Rule::percent_literal
            | Rule::permille_literal
            | Rule::date_time_literal
            | Rule::time_literal
            | Rule::duration_literal
            | Rule::number_unit_literal => {
                return parse_literal_expression(inner, attribute, spec_name, source_text.clone());
            }
            Rule::reference => {
                let reference = parse_reference(inner.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::Reference(reference),
                    &inner,
                    attribute,
                    spec_name,
                    source_text.clone(),
                ));
            }
            Rule::expression => {
                return parse_expression(
                    inner,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text.clone(),
                );
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
                return parse_logical_expression(
                    inner,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text.clone(),
                );
            }
            _ => {}
        }
    }
    unreachable!("BUG: grammar guarantees primary expression is non-empty")
}

pub(crate) fn parse_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    if let Err(msg) = depth_tracker.push_depth() {
        let source = Source::new(
            attribute,
            Span::from_pest_span(pair.as_span()),
            spec_name,
            source_text.clone(),
        );
        let actual_depth = msg
            .split_whitespace()
            .nth(2)
            .and_then(|s| s.parse::<usize>().ok())
            .map(|d| d.to_string())
            .unwrap_or_else(|| format!("parse error: {}", msg));
        return Err(Error::resource_limit_exceeded(
            "max_expression_depth",
            depth_tracker.max_depth().to_string(),
            actual_depth,
            "Simplify nested expressions to reduce depth",
            Some(source),
        ));
    }

    let result = parse_expression_impl(
        pair,
        depth_tracker,
        attribute,
        spec_name,
        source_text.clone(),
    );
    depth_tracker.pop_depth();
    result
}

fn parse_expression_impl(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    match pair.as_rule() {
        Rule::expression => {
            let mut inner = pair.into_inner();
            let and_expr = inner
                .next()
                .expect("BUG: grammar guarantees expression has one and_expression");
            return parse_and_expression(
                and_expr,
                depth_tracker,
                attribute,
                spec_name,
                source_text,
            );
        }

        Rule::and_expression => {
            return parse_and_expression(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            );
        }

        Rule::and_operand => {
            return parse_and_operand(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            );
        }

        Rule::base_expression => {
            return parse_base_expression(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            );
        }
        Rule::term => {
            return parse_term(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            )
        }
        Rule::power => {
            return parse_power(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            )
        }
        Rule::factor => {
            return parse_factor(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            )
        }
        Rule::primary => {
            return parse_primary(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            )
        }

        Rule::base_with_suffix => {
            return parse_base_with_suffix(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            );
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
            return parse_logical_expression(
                pair,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            )
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
            | Rule::permille_literal
            | Rule::date_time_literal
            | Rule::time_literal
            | Rule::duration_literal => {
                return parse_literal_expression(
                    inner_pair,
                    attribute,
                    spec_name,
                    source_text.clone(),
                );
            }

            Rule::reference => {
                let reference = parse_reference(inner_pair.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::Reference(reference),
                    &inner_pair,
                    attribute,
                    spec_name,
                    source_text.clone(),
                ));
            }

            Rule::base_with_suffix => {
                return parse_base_with_suffix(
                    inner_pair,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text.clone(),
                );
            }
            Rule::expression
            | Rule::and_expression
            | Rule::and_operand
            | Rule::base_expression
            | Rule::term
            | Rule::power
            | Rule::factor
            | Rule::primary => {
                return parse_expression(
                    inner_pair,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text.clone(),
                );
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
                return parse_logical_expression(
                    inner_pair,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text.clone(),
                );
            }

            _ => {}
        }
    }

    let span = Span::from_pest_span(pair.as_span());
    Err(Error::parsing(
        format!(
            "Invalid expression: unable to parse '{}' as any valid expression type",
            pair.as_str()
        ),
        Some(Source::new(attribute, span, spec_name, source_text.clone())),
        None::<String>,
    ))
}

fn parse_reference(pair: Pair<Rule>) -> Result<Reference, Error> {
    let parts: Vec<String> = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::reference_segment)
        .map(|p| p.as_str().to_string())
        .collect();
    let reference = Reference::from_path(parts);
    Ok(reference)
}

fn parse_and_operand(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    match pair.as_rule() {
        Rule::and_operand => {
            let first = pair
                .into_inner()
                .next()
                .expect("BUG: grammar guarantees and_operand is non-empty");
            parse_and_operand(first, depth_tracker, attribute, spec_name, source_text)
        }
        Rule::not_expr => {
            parse_not_expression(pair, depth_tracker, attribute, spec_name, source_text)
        }
        Rule::base_with_suffix => {
            parse_base_with_suffix(pair, depth_tracker, attribute, spec_name, source_text)
        }
        Rule::base_expression => {
            parse_base_expression(pair, depth_tracker, attribute, spec_name, source_text)
        }
        Rule::term | Rule::power | Rule::factor | Rule::primary => {
            parse_expression_impl(pair, depth_tracker, attribute, spec_name, source_text)
        }
        _ => parse_expression_impl(pair, depth_tracker, attribute, spec_name, source_text),
    }
}

fn parse_base_with_suffix(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let original_pair = pair.clone();
    let mut inner = pair.into_inner();

    let base_pair = inner
        .next()
        .expect("BUG: grammar guarantees base_with_suffix has base_expression");
    let base = parse_base_expression(
        base_pair,
        depth_tracker,
        attribute,
        spec_name,
        source_text.clone(),
    )?;

    let Some(suffix) = inner.next() else {
        return Ok(base);
    };

    match suffix.as_rule() {
        Rule::date_not_in_calendar_suffix => {
            let unit = extract_calendar_unit(&suffix);
            Ok(create_expression_with_location(
                ExpressionKind::DateCalendar(DateCalendarKind::NotIn, unit, Arc::new(base)),
                &original_pair,
                attribute,
                spec_name,
                source_text,
            ))
        }
        Rule::in_suffix => parse_in_suffix_expr(
            base,
            suffix,
            &original_pair,
            depth_tracker,
            attribute,
            spec_name,
            source_text,
        ),
        Rule::comparison_suffix => parse_comparison_suffix_expr(
            base,
            suffix,
            &original_pair,
            depth_tracker,
            attribute,
            spec_name,
            source_text,
        ),
        _ => unreachable!(
            "BUG: unexpected suffix in base_with_suffix: {:?}",
            suffix.as_rule()
        ),
    }
}

fn parse_in_suffix_expr(
    base: Expression,
    suffix: Pair<Rule>,
    original_pair: &Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let in_kind = suffix
        .into_inner()
        .next()
        .expect("BUG: grammar guarantees in_suffix has inner rule");

    match in_kind.as_rule() {
        Rule::in_date_calendar_relative => {
            let (cal_kind, unit) = extract_date_calendar_relative(&in_kind);
            Ok(create_expression_with_location(
                ExpressionKind::DateCalendar(cal_kind, unit, Arc::new(base)),
                original_pair,
                attribute,
                spec_name,
                source_text,
            ))
        }
        Rule::in_date_calendar => {
            let unit = extract_calendar_unit(&in_kind);
            Ok(create_expression_with_location(
                ExpressionKind::DateCalendar(DateCalendarKind::Current, unit, Arc::new(base)),
                original_pair,
                attribute,
                spec_name,
                source_text,
            ))
        }
        Rule::in_date_relative => {
            let mut rel_kind = None;
            let mut tolerance = None;
            for child in in_kind.into_inner() {
                match child.as_rule() {
                    Rule::date_relative_kind => {
                        rel_kind = Some(parse_date_relative_kind(&child));
                    }
                    Rule::base_expression => {
                        tolerance = Some(parse_base_expression(
                            child,
                            depth_tracker,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        )?);
                    }
                    _ => {}
                }
            }
            let rel_kind =
                rel_kind.expect("BUG: grammar guarantees in_date_relative has date_relative_kind");
            Ok(create_expression_with_location(
                ExpressionKind::DateRelative(rel_kind, Arc::new(base), tolerance.map(Arc::new)),
                original_pair,
                attribute,
                spec_name,
                source_text,
            ))
        }
        Rule::in_conversion => {
            let mut unit_name = None;
            let mut comp_suffix = None;
            for child in in_kind.into_inner() {
                match child.as_rule() {
                    Rule::conversion_target_name => {
                        unit_name = Some(child.as_str().to_string());
                    }
                    Rule::comparison_suffix => {
                        comp_suffix = Some(child);
                    }
                    _ => {}
                }
            }
            let unit = unit_name
                .expect("BUG: grammar guarantees in_conversion has conversion_target_name");
            let target = parse_conversion_target(&unit);
            let converted = create_expression_with_location(
                ExpressionKind::UnitConversion(Arc::new(base), target),
                original_pair,
                attribute,
                spec_name,
                source_text.clone(),
            );
            if let Some(comp) = comp_suffix {
                parse_comparison_suffix_expr(
                    converted,
                    comp,
                    original_pair,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text,
                )
            } else {
                Ok(converted)
            }
        }
        _ => unreachable!(
            "BUG: unexpected in_suffix inner rule: {:?}",
            in_kind.as_rule()
        ),
    }
}

fn parse_comparison_suffix_expr(
    left: Expression,
    suffix: Pair<Rule>,
    original_pair: &Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let mut inner = suffix.into_inner();

    let op_pair = inner
        .next()
        .expect("BUG: grammar guarantees comparison_suffix has operator");
    let operator = extract_comp_operator(op_pair);

    let rhs_pair = inner
        .next()
        .expect("BUG: grammar guarantees comparison_suffix has rhs");
    let right = parse_comparison_rhs(
        rhs_pair,
        depth_tracker,
        attribute,
        spec_name,
        source_text.clone(),
    )?;

    Ok(create_expression_with_location(
        ExpressionKind::Comparison(Arc::new(left), operator, Arc::new(right)),
        original_pair,
        attribute,
        spec_name,
        source_text,
    ))
}

fn parse_comparison_rhs(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let original_pair = pair.clone();
    let mut inner = pair.into_inner();
    let first = inner
        .next()
        .expect("BUG: grammar guarantees comparison_rhs is non-empty");

    match first.as_rule() {
        Rule::not_expr => {
            parse_not_expression(first, depth_tracker, attribute, spec_name, source_text)
        }
        Rule::base_expression => {
            let base = parse_base_expression(
                first,
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            )?;
            if let Some(conv_suffix) = inner.next() {
                let unit = conv_suffix
                    .into_inner()
                    .next()
                    .expect("BUG: grammar guarantees conversion_rhs_suffix has target")
                    .as_str()
                    .to_string();
                let target = parse_conversion_target(&unit);
                Ok(create_expression_with_location(
                    ExpressionKind::UnitConversion(Arc::new(base), target),
                    &original_pair,
                    attribute,
                    spec_name,
                    source_text,
                ))
            } else {
                Ok(base)
            }
        }
        _ => unreachable!(
            "BUG: unexpected comparison_rhs inner rule: {:?}",
            first.as_rule()
        ),
    }
}

fn extract_comp_operator(pair: Pair<Rule>) -> ComparisonComputation {
    let inner = if pair.as_rule() == Rule::comp_operator {
        pair.into_inner()
            .next()
            .expect("BUG: grammar guarantees comp_operator has inner rule")
    } else {
        pair
    };
    match inner.as_rule() {
        Rule::comp_gt => ComparisonComputation::GreaterThan,
        Rule::comp_lt => ComparisonComputation::LessThan,
        Rule::comp_gte => ComparisonComputation::GreaterThanOrEqual,
        Rule::comp_lte => ComparisonComputation::LessThanOrEqual,
        Rule::comp_eq => ComparisonComputation::Equal,
        Rule::comp_ne => ComparisonComputation::NotEqual,
        Rule::comp_is => ComparisonComputation::Is,
        Rule::comp_is_not => ComparisonComputation::IsNot,
        _ => unreachable!("BUG: invalid comparison operator: {:?}", inner.as_rule()),
    }
}

fn extract_calendar_unit(pair: &Pair<Rule>) -> CalendarUnit {
    for child in pair.clone().into_inner() {
        if child.as_rule() == Rule::calendar_unit_keyword {
            return match child.as_str().to_lowercase().as_str() {
                "year" => CalendarUnit::Year,
                "month" => CalendarUnit::Month,
                "week" => CalendarUnit::Week,
                other => unreachable!("BUG: unexpected calendar unit: {}", other),
            };
        }
    }
    unreachable!("BUG: grammar guarantees suffix has calendar_unit_keyword")
}

fn extract_date_calendar_relative(pair: &Pair<Rule>) -> (DateCalendarKind, CalendarUnit) {
    let mut kind = None;
    let mut unit = None;
    for child in pair.clone().into_inner() {
        match child.as_rule() {
            Rule::date_relative_kind => {
                kind = Some(match child.as_str().to_lowercase().as_str() {
                    "past" => DateCalendarKind::Past,
                    "future" => DateCalendarKind::Future,
                    other => unreachable!("BUG: unexpected date_relative_kind: {}", other),
                });
            }
            Rule::calendar_unit_keyword => {
                unit = Some(match child.as_str().to_lowercase().as_str() {
                    "year" => CalendarUnit::Year,
                    "month" => CalendarUnit::Month,
                    "week" => CalendarUnit::Week,
                    other => unreachable!("BUG: unexpected calendar unit: {}", other),
                });
            }
            _ => {}
        }
    }
    (
        kind.expect("BUG: grammar guarantees has date_relative_kind"),
        unit.expect("BUG: grammar guarantees has calendar_unit_keyword"),
    )
}

fn parse_date_relative_kind(pair: &Pair<Rule>) -> DateRelativeKind {
    match pair.as_str().to_lowercase().as_str() {
        "past" => DateRelativeKind::InPast,
        "future" => DateRelativeKind::InFuture,
        other => unreachable!(
            "BUG: grammar guarantees date_relative_kind is 'past' or 'future', got '{}'",
            other
        ),
    }
}

fn parse_and_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let original_pair = pair.clone();
    let mut pairs = pair.into_inner();
    let mut left = parse_and_operand(
        pairs
            .next()
            .expect("BUG: grammar guarantees AND expression has left operand"),
        depth_tracker,
        attribute,
        spec_name,
        source_text.clone(),
    )?;

    for right_pair in pairs {
        if right_pair.as_rule() == Rule::and_operand {
            let right = parse_and_operand(
                right_pair.clone(),
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            )?;
            let kind = ExpressionKind::LogicalAnd(Arc::new(left), Arc::new(right));
            left = create_expression_with_location(
                kind,
                &original_pair,
                attribute,
                spec_name,
                source_text.clone(),
            );
        }
    }

    Ok(left)
}

fn parse_base_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let original_pair = pair.clone();
    let mut inner = pair.into_inner();

    let mut left = parse_term(
        inner
            .next()
            .expect("BUG: grammar guarantees base_expression has left term"),
        depth_tracker,
        attribute,
        spec_name,
        source_text.clone(),
    )?;

    while let Some(op_pair) = inner.next() {
        let operation = match op_pair.as_rule() {
            Rule::op_add => ArithmeticComputation::Add,
            Rule::op_sub => ArithmeticComputation::Subtract,
            other => {
                unreachable!("BUG: unexpected operator in base_expression: {:?}", other)
            }
        };

        let right_term_pair = inner
            .next()
            .expect("BUG: grammar guarantees right term after + or - in base_expression");

        let right = parse_term(
            right_term_pair,
            depth_tracker,
            attribute,
            spec_name,
            source_text.clone(),
        )?;

        let kind = ExpressionKind::Arithmetic(Arc::new(left), operation, Arc::new(right));
        left = create_expression_with_location(
            kind,
            &original_pair,
            attribute,
            spec_name,
            source_text.clone(),
        );
    }

    Ok(left)
}

fn parse_conversion_target(unit_str: &str) -> ConversionTarget {
    let unit_lower = unit_str.to_lowercase();

    match unit_lower.as_str() {
        "year" | "years" => ConversionTarget::Duration(DurationUnit::Year),
        "month" | "months" => ConversionTarget::Duration(DurationUnit::Month),
        "week" | "weeks" => ConversionTarget::Duration(DurationUnit::Week),
        "day" | "days" => ConversionTarget::Duration(DurationUnit::Day),
        "hour" | "hours" => ConversionTarget::Duration(DurationUnit::Hour),
        "minute" | "minutes" => ConversionTarget::Duration(DurationUnit::Minute),
        "second" | "seconds" => ConversionTarget::Duration(DurationUnit::Second),
        "millisecond" | "milliseconds" => ConversionTarget::Duration(DurationUnit::Millisecond),
        "microsecond" | "microseconds" => ConversionTarget::Duration(DurationUnit::Microsecond),
        _ => ConversionTarget::Unit(unit_lower),
    }
}

fn parse_term(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let mut pairs = pair.clone().into_inner();
    let mut left = parse_power(
        pairs
            .next()
            .expect("BUG: grammar guarantees term has left power"),
        depth_tracker,
        attribute,
        spec_name,
        source_text.clone(),
    )?;

    while let Some(op_pair) = pairs.next() {
        let operation = match op_pair.as_rule() {
            Rule::op_mul => ArithmeticComputation::Multiply,
            Rule::op_div => ArithmeticComputation::Divide,
            Rule::op_mod => ArithmeticComputation::Modulo,
            _ => {
                unreachable!("BUG: unexpected operator in term: {:?}", op_pair.as_rule())
            }
        };

        let right = parse_power(
            pairs
                .next()
                .expect("BUG: grammar guarantees right power after operator in term"),
            depth_tracker,
            attribute,
            spec_name,
            source_text.clone(),
        )?;

        let kind = ExpressionKind::Arithmetic(Arc::new(left), operation, Arc::new(right));
        left =
            create_expression_with_location(kind, &pair, attribute, spec_name, source_text.clone());
    }

    Ok(left)
}

fn parse_power(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let mut pairs = pair.clone().into_inner();
    let left = parse_factor(
        pairs
            .next()
            .expect("BUG: grammar guarantees power has factor"),
        depth_tracker,
        attribute,
        spec_name,
        source_text.clone(),
    )?;

    if let Some(op_pair) = pairs.next() {
        if op_pair.as_rule() == Rule::op_pow {
            let right = parse_power(
                pairs
                    .next()
                    .expect("BUG: grammar guarantees right operand after ^ in power"),
                depth_tracker,
                attribute,
                spec_name,
                source_text.clone(),
            )?;

            let kind = ExpressionKind::Arithmetic(
                Arc::new(left),
                ArithmeticComputation::Power,
                Arc::new(right),
            );
            return Ok(create_expression_with_location(
                kind,
                &pair,
                attribute,
                spec_name,
                source_text.clone(),
            ));
        }
    }

    Ok(left)
}

fn parse_factor(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let mut pairs = pair.clone().into_inner();
    let mut is_negative = false;

    if let Some(first_pair) = pairs.next() {
        match first_pair.as_rule() {
            Rule::op_sub => {
                is_negative = true;
            }
            Rule::op_add => {}
            _ => {
                let expr = parse_primary(
                    first_pair,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text.clone(),
                )?;
                return Ok(expr);
            }
        }
    }

    let expr = if let Some(expr_pair) = pairs.next() {
        parse_primary(
            expr_pair,
            depth_tracker,
            attribute,
            spec_name,
            source_text.clone(),
        )?
    } else {
        unreachable!("BUG: grammar guarantees expression after unary operator");
    };

    if is_negative {
        let zero = create_expression_with_location(
            ExpressionKind::Literal(Value::Number(rust_decimal::Decimal::ZERO)),
            &pair,
            attribute,
            spec_name,
            source_text.clone(),
        );
        let kind = ExpressionKind::Arithmetic(
            Arc::new(zero),
            ArithmeticComputation::Subtract,
            Arc::new(expr),
        );
        Ok(create_expression_with_location(
            kind,
            &pair,
            attribute,
            spec_name,
            source_text.clone(),
        ))
    } else {
        Ok(expr)
    }
}

fn parse_not_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
    let original_pair = pair.clone();
    let mut inner = pair.into_inner();
    let operand_pair = inner
        .next()
        .expect("BUG: grammar guarantees not expression has operand");

    let operand = parse_expression(
        operand_pair,
        depth_tracker,
        attribute,
        spec_name,
        source_text.clone(),
    )?;
    let kind = ExpressionKind::LogicalNegation(Arc::new(operand), NegationType::Not);

    Ok(create_expression_with_location(
        kind,
        &original_pair,
        attribute,
        spec_name,
        source_text.clone(),
    ))
}

fn parse_logical_expression(
    pair: Pair<Rule>,
    depth_tracker: &mut DepthTracker,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<Expression, Error> {
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
                        "BUG: unexpected rule '{:?}' in mathematical expression parser (attribute={}, spec={})",
                        unexpected, attribute, spec_name
                    )
                }
            };

            for inner in pair.clone().into_inner() {
                match inner.as_rule() {
                    Rule::base_expression => {
                        let operand = parse_base_expression(
                            inner,
                            depth_tracker,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        )?;
                        let kind =
                            ExpressionKind::MathematicalComputation(operator, Arc::new(operand));
                        return Ok(create_expression_with_location(
                            kind,
                            &pair,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        ));
                    }
                    Rule::term | Rule::primary => {
                        let operand = parse_expression(
                            inner,
                            depth_tracker,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        )?;
                        let kind =
                            ExpressionKind::MathematicalComputation(operator, Arc::new(operand));
                        return Ok(create_expression_with_location(
                            kind,
                            &pair,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        ));
                    }
                    _ => {}
                }
            }
            unreachable!("BUG: grammar guarantees mathematical operator has operand");
        }
        _ => {}
    }
    if let Some(node) = pair.into_inner().next() {
        match node.as_rule() {
            Rule::literal => {
                return parse_expression(
                    node,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text.clone(),
                )
            }
            Rule::primary => {
                return parse_primary(
                    node,
                    depth_tracker,
                    attribute,
                    spec_name,
                    source_text.clone(),
                )
            }
            Rule::not_expr => {
                for inner in node.clone().into_inner() {
                    let negated_expr = match inner.as_rule() {
                        Rule::primary => parse_primary(
                            inner,
                            depth_tracker,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        )?,
                        Rule::literal => parse_expression(
                            inner,
                            depth_tracker,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        )?,
                        _ => continue,
                    };
                    let kind =
                        ExpressionKind::LogicalNegation(Arc::new(negated_expr), NegationType::Not);
                    return Ok(create_expression_with_location(
                        kind,
                        &node,
                        attribute,
                        spec_name,
                        source_text.clone(),
                    ));
                }
                unreachable!("BUG: grammar guarantees not expression has operand");
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
                        unreachable!("BUG: unknown mathematical operator: {:?}", node.as_rule())
                    }
                };

                for inner in node.clone().into_inner() {
                    match inner.as_rule() {
                        Rule::base_expression => {
                            let operand = parse_base_expression(
                                inner,
                                depth_tracker,
                                attribute,
                                spec_name,
                                source_text.clone(),
                            )?;
                            let kind = ExpressionKind::MathematicalComputation(
                                operator,
                                Arc::new(operand),
                            );
                            return Ok(create_expression_with_location(
                                kind,
                                &node,
                                attribute,
                                spec_name,
                                source_text.clone(),
                            ));
                        }
                        Rule::term | Rule::primary => {
                            let operand = parse_expression(
                                inner,
                                depth_tracker,
                                attribute,
                                spec_name,
                                source_text.clone(),
                            )?;
                            let kind = ExpressionKind::MathematicalComputation(
                                operator,
                                Arc::new(operand),
                            );
                            return Ok(create_expression_with_location(
                                kind,
                                &node,
                                attribute,
                                spec_name,
                                source_text.clone(),
                            ));
                        }
                        _ => {}
                    }
                }
                unreachable!("BUG: grammar guarantees mathematical operator has operand");
            }
            _ => {}
        }
    }
    unreachable!("BUG: grammar guarantees logical expression is non-empty")
}

#[cfg(test)]
mod tests {
    use crate::parsing::parse;

    #[test]
    fn test_simple_number() {
        let input = r#"spec test
rule num: 42"#;
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse simple number: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_fact_reference_parsing() {
        let input = r#"spec test
rule simple_ref: age"#;
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse fact reference: {:?}",
            result.err()
        );

        let input = r#"spec test
rule nested_ref: employee.salary"#;
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
            let input = format!("spec test\nrule test: {}", expr);
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
        let input = r#"spec test
fact income: 80000
fact total_tax: 20000
rule effective_tax_rate: total_tax / income in percent"#;

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
            let input = format!("spec test\nrule test: {}", expr);
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
            ("not is_blocked", "simple not"),
            ("sqrt 16", "square root"),
            ("sin 0", "sine function"),
        ];

        for (expr, description) in test_cases {
            let input = format!("spec test\nrule test: {}", expr);
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
            ("sqrt(x) ^ 2", "sqrt with parens and power operator"),
            ("sin(x) * cos(x)", "multiple function calls"),
        ];

        for (expr, description) in test_cases {
            let input = format!(
                "spec test\nfact x: true\nfact y: false\nrule test: {}",
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

    #[test]
    fn test_now_keyword_parses_as_expression_kind_now() {
        use crate::parsing::ast::ExpressionKind;
        let input = "spec test\nrule current_time: now";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse now keyword");
        let spec = &specs[0];
        let rule = &spec.rules[0];
        assert!(
            matches!(rule.expression.kind, ExpressionKind::Now),
            "Expected ExpressionKind::Now, got {:?}",
            rule.expression.kind
        );
    }

    #[test]
    fn test_now_is_reserved_keyword() {
        let input = "spec test\nfact now: 42";
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_err(),
            "'now' should be reserved and not usable as fact name"
        );
    }

    #[test]
    fn test_now_in_comparison() {
        let input = "spec test\nfact deadline: 2026-03-07\nrule is_overdue: deadline < now";
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse 'now' in comparison: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_now_in_arithmetic() {
        let input = "spec test\nrule offset: now + 1 days";
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse 'now' in arithmetic: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_now_display_round_trips() {
        let input = "spec test\nrule current_time: now";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse now keyword");
        let rule = &specs[0].rules[0];
        assert_eq!(format!("{}", rule.expression), "now");
    }

    #[test]
    fn test_fractional_seconds_parse() {
        let input = "spec test\nfact ts: 2026-02-26T14:30:00.123456Z";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse fractional seconds");
        let spec = &specs[0];
        let fact = &spec.facts[0];
        match &fact.value {
            crate::parsing::ast::FactValue::Literal(crate::parsing::ast::Value::Date(dtv)) => {
                assert_eq!(dtv.microsecond, 123456, "Expected 123456 microseconds");
                assert_eq!(dtv.second, 0);
                assert_eq!(dtv.minute, 30);
                assert_eq!(dtv.hour, 14);
            }
            other => panic!("Expected Date literal, got {:?}", other),
        }
    }

    #[test]
    fn test_fractional_seconds_three_digits() {
        let input = "spec test\nfact ts: 2026-02-26T14:30:00.123Z";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse fractional seconds (3 digits)");
        let spec = &specs[0];
        let fact = &spec.facts[0];
        match &fact.value {
            crate::parsing::ast::FactValue::Literal(crate::parsing::ast::Value::Date(dtv)) => {
                assert_eq!(
                    dtv.microsecond, 123000,
                    "Expected 123000 microseconds for .123"
                );
            }
            other => panic!("Expected Date literal, got {:?}", other),
        }
    }

    #[test]
    fn test_date_sugar_in_past() {
        use crate::parsing::ast::{DateRelativeKind, ExpressionKind};
        let input = "spec test\nfact deadline: 2026-01-01\nrule overdue: deadline in past";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse 'in past'");
        let rule = &specs[0].rules[0];
        match &rule.expression.kind {
            ExpressionKind::DateRelative(kind, _date, tolerance) => {
                assert_eq!(*kind, DateRelativeKind::InPast);
                assert!(tolerance.is_none());
            }
            other => panic!("Expected DateRelative, got {:?}", other),
        }
    }

    #[test]
    fn test_date_sugar_in_past_with_tolerance() {
        use crate::parsing::ast::{DateRelativeKind, ExpressionKind};
        let input = "spec test\nfact deadline: 2026-01-01\nrule recent: deadline in past 7 days";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse 'in past 7 days'");
        let rule = &specs[0].rules[0];
        match &rule.expression.kind {
            ExpressionKind::DateRelative(kind, _date, tolerance) => {
                assert_eq!(*kind, DateRelativeKind::InPast);
                assert!(tolerance.is_some(), "Expected tolerance expression");
            }
            other => panic!("Expected DateRelative, got {:?}", other),
        }
    }

    #[test]
    fn test_date_sugar_in_future() {
        use crate::parsing::ast::{DateRelativeKind, ExpressionKind};
        let input = "spec test\nfact deadline: 2026-01-01\nrule upcoming: deadline in future";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse 'in future'");
        let rule = &specs[0].rules[0];
        match &rule.expression.kind {
            ExpressionKind::DateRelative(kind, _date, tolerance) => {
                assert_eq!(*kind, DateRelativeKind::InFuture);
                assert!(tolerance.is_none());
            }
            other => panic!("Expected DateRelative, got {:?}", other),
        }
    }

    #[test]
    fn test_date_sugar_in_future_with_tolerance() {
        use crate::parsing::ast::{DateRelativeKind, ExpressionKind};
        let input = "spec test\nfact deadline: 2026-01-01\nrule soon: deadline in future 30 days";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse 'in future 30 days'");
        let rule = &specs[0].rules[0];
        match &rule.expression.kind {
            ExpressionKind::DateRelative(kind, _date, tolerance) => {
                assert_eq!(*kind, DateRelativeKind::InFuture);
                assert!(tolerance.is_some(), "Expected tolerance expression");
            }
            other => panic!("Expected DateRelative, got {:?}", other),
        }
    }

    #[test]
    fn test_date_sugar_in_calendar_year() {
        use crate::parsing::ast::{CalendarUnit, DateCalendarKind, ExpressionKind};
        let input =
            "spec test\nfact deadline: 2026-01-01\nrule this_year: deadline in calendar year";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse 'in calendar year'");
        let rule = &specs[0].rules[0];
        match &rule.expression.kind {
            ExpressionKind::DateCalendar(kind, unit, _date) => {
                assert_eq!(*kind, DateCalendarKind::Current);
                assert_eq!(*unit, CalendarUnit::Year);
            }
            other => panic!("Expected DateCalendar, got {:?}", other),
        }
    }

    #[test]
    fn test_date_sugar_in_past_calendar_month() {
        use crate::parsing::ast::{CalendarUnit, DateCalendarKind, ExpressionKind};
        let input =
            "spec test\nfact deadline: 2026-01-01\nrule last_month: deadline in past calendar month";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse 'in past calendar month'");
        let rule = &specs[0].rules[0];
        match &rule.expression.kind {
            ExpressionKind::DateCalendar(kind, unit, _date) => {
                assert_eq!(*kind, DateCalendarKind::Past);
                assert_eq!(*unit, CalendarUnit::Month);
            }
            other => panic!("Expected DateCalendar, got {:?}", other),
        }
    }

    #[test]
    fn test_date_sugar_in_future_calendar_week() {
        use crate::parsing::ast::{CalendarUnit, DateCalendarKind, ExpressionKind};
        let input =
            "spec test\nfact deadline: 2026-01-01\nrule next_week: deadline in future calendar week";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse 'in future calendar week'");
        let rule = &specs[0].rules[0];
        match &rule.expression.kind {
            ExpressionKind::DateCalendar(kind, unit, _date) => {
                assert_eq!(*kind, DateCalendarKind::Future);
                assert_eq!(*unit, CalendarUnit::Week);
            }
            other => panic!("Expected DateCalendar, got {:?}", other),
        }
    }

    #[test]
    fn test_date_sugar_not_in_calendar_year() {
        use crate::parsing::ast::{CalendarUnit, DateCalendarKind, ExpressionKind};
        let input =
            "spec test\nfact deadline: 2026-01-01\nrule other_year: deadline not in calendar year";
        let specs = parse(input, "test.lemma", &crate::ResourceLimits::default())
            .expect("Failed to parse 'not in calendar year'");
        let rule = &specs[0].rules[0];
        match &rule.expression.kind {
            ExpressionKind::DateCalendar(kind, unit, _date) => {
                assert_eq!(*kind, DateCalendarKind::NotIn);
                assert_eq!(*unit, CalendarUnit::Year);
            }
            other => panic!("Expected DateCalendar, got {:?}", other),
        }
    }

    #[test]
    fn test_unit_conversion_still_works() {
        let input = "spec test\ntype money: scale\n -> unit eur 1.00\n -> unit usd 1.10\nfact price: 100 eur\nrule converted: price in usd";
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Unit conversion 'in usd' should still work: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_date_sugar_in_past_and_conjunction() {
        use crate::parsing::ast::ExpressionKind;
        let input = "spec test\nfact a: 2026-01-01\nfact b: true\nrule check: a in past and b";
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse 'X in past and Y': {:?}",
            result.err()
        );
        let specs = result.unwrap();
        let rule = &specs[0].rules[0];
        assert!(
            matches!(rule.expression.kind, ExpressionKind::LogicalAnd(..)),
            "Expected LogicalAnd at top level, got {:?}",
            rule.expression.kind
        );
    }
}
