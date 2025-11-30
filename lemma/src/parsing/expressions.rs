use super::ast::{ExpressionIdGenerator, Span};
use super::Rule;
use crate::error::LemmaError;
use crate::semantic::*;
use crate::Source;
use pest::iterators::Pair;

/// Create an Expression with source location and unique ID from a parser pair
fn create_expression_with_location(
    kind: ExpressionKind,
    pair: &Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Expression {
    let span = Span::from_pest_span(pair.as_span());
    Expression::new(
        kind,
        Some(Source::new(
            source_id.to_string(),
            span,
            doc_name.to_string(),
        )),
        id_gen.next_id(),
    )
}

/// Helper function to parse any literal rule into an Expression.
/// Handles both wrapped literals (Rule::literal) and direct literal types.
fn parse_literal_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // Handle wrapped literals (Rule::literal contains the actual literal type)
    let literal_pair = if pair.as_rule() == Rule::literal {
        pair.into_inner()
            .next()
            .ok_or_else(|| LemmaError::Engine("Empty literal wrapper".to_string()))?
    } else {
        pair
    };

    let literal_value = crate::parsing::literals::parse_literal(literal_pair.clone())?;
    Ok(create_expression_with_location(
        ExpressionKind::Literal(literal_value),
        &literal_pair,
        id_gen,
        source_id,
        doc_name,
    ))
}

fn parse_primary(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // primary = { literal | reference_expression | "(" ~ expression_group ~ ")" }
    for inner in pair.clone().into_inner() {
        match inner.as_rule() {
            Rule::literal
            | Rule::number_literal
            | Rule::string_literal
            | Rule::boolean_literal
            | Rule::regex_literal
            | Rule::percentage_literal
            | Rule::date_time_literal
            | Rule::time_literal
            | Rule::unit_literal => {
                return parse_literal_expression(inner, id_gen, source_id, doc_name);
            }
            Rule::reference_expression => {
                return parse_reference_expression(inner, id_gen, source_id, doc_name);
            }
            Rule::rule_reference => {
                let rule_ref = parse_rule_reference(inner.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::RuleReference(rule_ref),
                    &inner,
                    id_gen,
                    source_id,
                    doc_name,
                ));
            }
            Rule::fact_reference => {
                let fact_ref = parse_fact_reference(inner.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::FactReference(fact_ref),
                    &inner,
                    id_gen,
                    source_id,
                    doc_name,
                ));
            }
            Rule::expression_group => {
                return parse_or_expression(inner, id_gen, source_id, doc_name);
            }
            _ => {}
        }
    }
    Err(LemmaError::Engine("Empty primary expression".to_string()))
}

pub(crate) fn parse_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // Check and increment depth
    if let Err(msg) = id_gen.push_depth() {
        return Err(LemmaError::ResourceLimitExceeded {
            limit_name: "max_expression_depth".to_string(),
            limit_value: id_gen.max_depth().to_string(),
            actual_value: msg
                .split_whitespace()
                .nth(2)
                .unwrap_or("unknown")
                .to_string(),
            suggestion: "Simplify nested expressions to reduce depth".to_string(),
        });
    }

    let result = parse_expression_impl(pair, id_gen, source_id, doc_name);
    id_gen.pop_depth();
    result
}

fn parse_expression_impl(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // Check the current rule first before descending to children
    match pair.as_rule() {
        Rule::comparable_base => return parse_comparable_base(pair, id_gen, source_id, doc_name),
        Rule::term => return parse_term(pair, id_gen, source_id, doc_name),
        Rule::power => return parse_power(pair, id_gen, source_id, doc_name),
        Rule::factor => return parse_factor(pair, id_gen, source_id, doc_name),
        Rule::primary => return parse_primary(pair, id_gen, source_id, doc_name),
        Rule::arithmetic_expression => {
            return parse_arithmetic_expression(pair, id_gen, source_id, doc_name)
        }
        Rule::comparison_expression => {
            return parse_comparison_expression(pair, id_gen, source_id, doc_name)
        }
        Rule::boolean_expression => {
            return parse_logical_expression(pair, id_gen, source_id, doc_name)
        }
        // Directly handle mathematical operator nodes here so they don't get flattened
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
        | Rule::round_expr => return parse_logical_expression(pair, id_gen, source_id, doc_name),
        Rule::and_expression => return parse_and_expression(pair, id_gen, source_id, doc_name),
        Rule::or_expression => return parse_or_expression(pair, id_gen, source_id, doc_name),
        Rule::and_operand => return parse_and_operand(pair, id_gen, source_id, doc_name),
        Rule::expression_group => return parse_or_expression(pair, id_gen, source_id, doc_name),
        Rule::expression => {} // Continue to iterate children
        _ => {}
    }

    for inner_pair in pair.clone().into_inner() {
        match inner_pair.as_rule() {
            // Literals - can appear wrapped in Rule::literal or directly as specific types
            Rule::literal
            | Rule::number_literal
            | Rule::string_literal
            | Rule::boolean_literal
            | Rule::regex_literal
            | Rule::percentage_literal
            | Rule::date_time_literal
            | Rule::time_literal
            | Rule::unit_literal => {
                return parse_literal_expression(inner_pair, id_gen, source_id, doc_name);
            }

            // References
            Rule::reference_expression => {
                return parse_reference_expression(inner_pair, id_gen, source_id, doc_name)
            }

            Rule::rule_reference => {
                let rule_ref = parse_rule_reference(inner_pair.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::RuleReference(rule_ref),
                    &inner_pair,
                    id_gen,
                    source_id,
                    doc_name,
                ));
            }

            Rule::fact_reference => {
                let fact_ref = parse_fact_reference(inner_pair.clone())?;
                return Ok(create_expression_with_location(
                    ExpressionKind::FactReference(fact_ref),
                    &inner_pair,
                    id_gen,
                    source_id,
                    doc_name,
                ));
            }

            Rule::primary
            | Rule::arithmetic_expression
            | Rule::comparison_expression
            | Rule::boolean_expression
            | Rule::and_expression
            | Rule::or_expression
            | Rule::and_operand
            | Rule::expression_group => {
                return parse_expression(inner_pair, id_gen, source_id, doc_name);
            }

            // Logical and mathematical operations
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
                return parse_logical_expression(inner_pair, id_gen, source_id, doc_name);
            }

            Rule::comparable_base | Rule::term | Rule::power | Rule::factor | Rule::expression => {
                return parse_expression(inner_pair, id_gen, source_id, doc_name);
            }

            _ => {}
        }
    }

    Err(LemmaError::Engine(format!(
        "Invalid expression: unable to parse '{}' as any valid expression type. Available rules: {:?}",
        pair.as_str(),
        pair.into_inner().map(|p| p.as_rule()).collect::<Vec<_>>()
    )))
}

fn parse_reference_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    if let Some(inner_pair) = pair.clone().into_inner().next() {
        match inner_pair.as_rule() {
            Rule::rule_reference => {
                let rule_ref = parse_rule_reference(inner_pair)?;
                let kind = ExpressionKind::RuleReference(rule_ref);
                return Ok(create_expression_with_location(
                    kind, &pair, id_gen, source_id, doc_name,
                ));
            }
            Rule::fact_name => {
                let kind = ExpressionKind::FactReference(FactReference::local(
                    inner_pair.as_str().to_string(),
                ));
                return Ok(create_expression_with_location(
                    kind, &pair, id_gen, source_id, doc_name,
                ));
            }
            Rule::fact_reference => {
                let fact_ref = parse_fact_reference(inner_pair)?;
                let kind = ExpressionKind::FactReference(fact_ref);
                return Ok(create_expression_with_location(
                    kind, &pair, id_gen, source_id, doc_name,
                ));
            }
            _ => {}
        }
    }
    Err(LemmaError::Engine(
        "Invalid reference expression".to_string(),
    ))
}

fn parse_fact_reference(pair: Pair<Rule>) -> Result<FactReference, LemmaError> {
    let mut reference = Vec::new();
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::label {
            reference.push(inner_pair.as_str().to_string());
        }
    }
    Ok(FactReference::from_path(reference))
}

fn parse_rule_reference(pair: Pair<Rule>) -> Result<RuleReference, LemmaError> {
    let mut reference = Vec::new();
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::label {
            reference.push(inner_pair.as_str().to_string());
        }
    }
    Ok(RuleReference::from_path(reference))
}

fn parse_and_operand(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // Grammar: boolean_expression | comparable_base ~ (SPACE* ~ comp_operator ~ SPACE* ~ comparable_base)?
    let mut pairs = pair.clone().into_inner();
    let first = pairs
        .next()
        .ok_or_else(|| LemmaError::Engine("Empty and_operand".to_string()))?;

    // Check if it's a boolean_expression
    if first.as_rule() == Rule::boolean_expression {
        return parse_logical_expression(first, id_gen, source_id, doc_name);
    }

    // Otherwise it's comparable_base with optional comparison
    let left = parse_expression(first, id_gen, source_id, doc_name)?;

    // Check for comparison operator
    if let Some(op_pair) = pairs.next() {
        if op_pair.as_rule() == Rule::comp_operator {
            // Parse the specific operator from within comp_operator
            let inner_pair = op_pair
                .clone()
                .into_inner()
                .next()
                .ok_or_else(|| LemmaError::Engine("Empty comparison operator".to_string()))?;
            let operator = match inner_pair.as_rule() {
                Rule::comp_gt => ComparisonComputation::GreaterThan,
                Rule::comp_lt => ComparisonComputation::LessThan,
                Rule::comp_gte => ComparisonComputation::GreaterThanOrEqual,
                Rule::comp_lte => ComparisonComputation::LessThanOrEqual,
                Rule::comp_eq => ComparisonComputation::Equal,
                Rule::comp_ne => ComparisonComputation::NotEqual,
                Rule::comp_is => ComparisonComputation::Is,
                Rule::comp_is_not => ComparisonComputation::IsNot,
                _ => {
                    return Err(LemmaError::Engine(format!(
                        "Invalid comparison operator: {:?}",
                        inner_pair.as_rule()
                    )))
                }
            };
            let right = parse_expression(
                pairs.next().ok_or_else(|| {
                    LemmaError::Engine("Missing right operand in comparison".to_string())
                })?,
                id_gen,
                source_id,
                doc_name,
            )?;
            let kind = ExpressionKind::Comparison(Box::new(left), operator, Box::new(right));
            return Ok(create_expression_with_location(
                kind, &pair, id_gen, source_id, doc_name,
            ));
        }
    }

    // No operator, just return the left side
    Ok(left)
}

fn parse_and_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // Clone the pair before consuming it for source location
    let original_pair = pair.clone();
    let mut pairs = pair.into_inner();
    let mut left = parse_and_operand(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("Missing left operand in logical AND expression".to_string())
        })?,
        id_gen,
        source_id,
        doc_name,
    )?;

    // The grammar structure is: and_operand ~ (SPACE+ ~ ^"and" ~ SPACE+ ~ and_operand)*
    // We only process and_operand tokens, skipping SPACE and keywords
    // Use the original pair for source location to capture the full expression
    for right_pair in pairs {
        if right_pair.as_rule() == Rule::and_operand {
            let right = parse_and_operand(right_pair.clone(), id_gen, source_id, doc_name)?;
            let kind = ExpressionKind::LogicalAnd(Box::new(left), Box::new(right));
            left =
                create_expression_with_location(kind, &original_pair, id_gen, source_id, doc_name);
        }
    }

    Ok(left)
}

pub(crate) fn parse_or_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // Handle expression_group wrapper: expression_group = { or_expression }
    let or_pair = if pair.as_rule() == Rule::expression_group {
        pair.into_inner()
            .next()
            .ok_or_else(|| LemmaError::Engine("Empty expression_group".to_string()))?
    } else {
        pair
    };

    // Clone the or_pair before consuming it for source location
    let original_or_pair = or_pair.clone();
    let mut pairs = or_pair.into_inner();
    let mut left = parse_and_expression(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("Missing left operand in logical OR expression".to_string())
        })?,
        id_gen,
        source_id,
        doc_name,
    )?;

    // The grammar structure is: and_expression ~ (SPACE+ ~ ^"or" ~ SPACE+ ~ and_expression)*
    // We only process and_expression tokens, skipping SPACE and keywords
    // Use the original or_pair for source location to capture the full expression
    for right_pair in pairs {
        if right_pair.as_rule() == Rule::and_expression {
            let right = parse_and_expression(right_pair.clone(), id_gen, source_id, doc_name)?;
            let kind = ExpressionKind::LogicalOr(Box::new(left), Box::new(right));
            left = create_expression_with_location(
                kind,
                &original_or_pair,
                id_gen,
                source_id,
                doc_name,
            );
        }
    }

    Ok(left)
}

fn parse_arithmetic_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let mut left = parse_term(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("Missing left term in arithmetic expression".to_string())
        })?,
        id_gen,
        source_id,
        doc_name,
    )?;

    while let Some(op_pair) = pairs.next() {
        let operation = match op_pair.as_rule() {
            Rule::add_plus => ArithmeticComputation::Add,
            Rule::add_minus => ArithmeticComputation::Subtract,
            _ => {
                return Err(LemmaError::Engine(format!(
                    "Unexpected operator in arithmetic expression: {:?}",
                    op_pair.as_rule()
                )))
            }
        };

        let right = parse_term(
            pairs.next().ok_or_else(|| {
                LemmaError::Engine("Missing right term in arithmetic expression".to_string())
            })?,
            id_gen,
            source_id,
            doc_name,
        )?;

        let kind = ExpressionKind::Arithmetic(Box::new(left), operation, Box::new(right));
        left = create_expression_with_location(kind, &pair, id_gen, source_id, doc_name);
    }

    Ok(left)
}

fn parse_term(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let mut left = parse_power(
        pairs
            .next()
            .ok_or_else(|| LemmaError::Engine("Missing left power in term".to_string()))?,
        id_gen,
        source_id,
        doc_name,
    )?;

    while let Some(op_pair) = pairs.next() {
        let operation = match op_pair.as_rule() {
            Rule::mul_star => ArithmeticComputation::Multiply,
            Rule::mul_slash => ArithmeticComputation::Divide,
            Rule::mul_percent => ArithmeticComputation::Modulo,
            _ => {
                return Err(LemmaError::Engine(format!(
                    "Unexpected operator in term: {:?}",
                    op_pair.as_rule()
                )))
            }
        };

        let right = parse_power(
            pairs
                .next()
                .ok_or_else(|| LemmaError::Engine("Missing right power in term".to_string()))?,
            id_gen,
            source_id,
            doc_name,
        )?;

        let kind = ExpressionKind::Arithmetic(Box::new(left), operation, Box::new(right));
        left = create_expression_with_location(kind, &pair, id_gen, source_id, doc_name);
    }

    Ok(left)
}

fn parse_power(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let left = parse_factor(
        pairs
            .next()
            .ok_or_else(|| LemmaError::Engine("Missing factor in power".to_string()))?,
        id_gen,
        source_id,
        doc_name,
    )?;

    if let Some(op_pair) = pairs.next() {
        if op_pair.as_rule() == Rule::pow_caret {
            let right = parse_power(
                pairs.next().ok_or_else(|| {
                    LemmaError::Engine("Missing right power in power expression".to_string())
                })?,
                id_gen,
                source_id,
                doc_name,
            )?;

            let kind = ExpressionKind::Arithmetic(
                Box::new(left),
                ArithmeticComputation::Power,
                Box::new(right),
            );
            return Ok(create_expression_with_location(
                kind, &pair, id_gen, source_id, doc_name,
            ));
        }
    }

    Ok(left)
}

fn parse_factor(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let mut is_negative = false;

    // Check for unary operators
    if let Some(first_pair) = pairs.next() {
        match first_pair.as_rule() {
            Rule::unary_minus => {
                is_negative = true;
            }
            Rule::unary_plus => {
                // Just ignore unary plus
            }
            _ => {
                let expr = parse_expression(first_pair, id_gen, source_id, doc_name)?;
                return Ok(expr);
            }
        }
    }

    // Parse the actual expression after unary operator
    let expr = if let Some(expr_pair) = pairs.next() {
        parse_expression(expr_pair, id_gen, source_id, doc_name)?
    } else {
        return Err(LemmaError::Engine(
            "Missing expression after unary operator".to_string(),
        ));
    };

    // Apply unary operator if present
    if is_negative {
        let zero = create_expression_with_location(
            ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::ZERO)),
            &pair,
            id_gen,
            source_id,
            doc_name,
        );
        let kind = ExpressionKind::Arithmetic(
            Box::new(zero),
            ArithmeticComputation::Subtract,
            Box::new(expr),
        );
        Ok(create_expression_with_location(
            kind, &pair, id_gen, source_id, doc_name,
        ))
    } else {
        Ok(expr)
    }
}

fn parse_comparison_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let left = parse_expression(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("Missing left operand in comparison expression".to_string())
        })?,
        id_gen,
        source_id,
        doc_name,
    )?;

    if let Some(op_pair) = pairs.next() {
        let operator = match op_pair.as_rule() {
            Rule::comp_operator => {
                // Parse the specific operator from within comp_operator
                let inner_pair = op_pair
                    .into_inner()
                    .next()
                    .ok_or_else(|| LemmaError::Engine("Empty comparison operator".to_string()))?;
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
                        return Err(LemmaError::Engine(format!(
                            "Invalid comparison operator: {:?}",
                            inner_pair.as_rule()
                        )))
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
                return Err(LemmaError::Engine(format!(
                    "Invalid comparison operator: {:?}",
                    op_pair.as_rule()
                )))
            }
        };

        let right = parse_expression(
            pairs.next().ok_or_else(|| {
                LemmaError::Engine("Missing right operand in comparison expression".to_string())
            })?,
            id_gen,
            source_id,
            doc_name,
        )?;

        let kind = ExpressionKind::Comparison(Box::new(left), operator, Box::new(right));
        return Ok(create_expression_with_location(
            kind, &pair, id_gen, source_id, doc_name,
        ));
    }

    Ok(left)
}

fn parse_logical_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // Handle direct mathematical operator nodes (abs, floor, etc.)
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
                _ => unreachable!(),
            };

            for inner in pair.clone().into_inner() {
                if inner.as_rule() == Rule::arithmetic_expression
                    || inner.as_rule() == Rule::primary
                {
                    let operand = parse_expression(inner, id_gen, source_id, doc_name)?;
                    let kind = ExpressionKind::MathematicalComputation(operator, Box::new(operand));
                    return Ok(create_expression_with_location(
                        kind, &pair, id_gen, source_id, doc_name,
                    ));
                }
            }
            return Err(LemmaError::Engine(
                "Mathematical operator missing operand".to_string(),
            ));
        }
        _ => {}
    }
    if let Some(node) = pair.into_inner().next() {
        match node.as_rule() {
            Rule::reference_expression => {
                return parse_reference_expression(node, id_gen, source_id, doc_name)
            }
            Rule::literal => return parse_expression(node, id_gen, source_id, doc_name),
            Rule::primary => return parse_primary(node, id_gen, source_id, doc_name),
            Rule::not_expr => {
                for inner in node.clone().into_inner() {
                    if inner.as_rule() == Rule::reference_expression {
                        let negated_expr =
                            parse_reference_expression(inner, id_gen, source_id, doc_name)?;
                        let kind = ExpressionKind::LogicalNegation(
                            Box::new(negated_expr),
                            NegationType::Not,
                        );
                        return Ok(create_expression_with_location(
                            kind, &node, id_gen, source_id, doc_name,
                        ));
                    } else if inner.as_rule() == Rule::primary {
                        let negated_expr = parse_primary(inner, id_gen, source_id, doc_name)?;
                        let kind = ExpressionKind::LogicalNegation(
                            Box::new(negated_expr),
                            NegationType::Not,
                        );
                        return Ok(create_expression_with_location(
                            kind, &node, id_gen, source_id, doc_name,
                        ));
                    } else if inner.as_rule() == Rule::literal {
                        let negated_expr = parse_expression(inner, id_gen, source_id, doc_name)?;
                        let kind = ExpressionKind::LogicalNegation(
                            Box::new(negated_expr),
                            NegationType::Not,
                        );
                        return Ok(create_expression_with_location(
                            kind, &node, id_gen, source_id, doc_name,
                        ));
                    }
                }
                return Err(LemmaError::Engine("not: missing expression".to_string()));
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
                        return Err(LemmaError::Engine(
                            "Unknown mathematical operator".to_string(),
                        ))
                    }
                };

                for inner in node.clone().into_inner() {
                    if inner.as_rule() == Rule::arithmetic_expression
                        || inner.as_rule() == Rule::primary
                    {
                        let operand = parse_expression(inner, id_gen, source_id, doc_name)?;
                        let kind =
                            ExpressionKind::MathematicalComputation(operator, Box::new(operand));
                        return Ok(create_expression_with_location(
                            kind, &node, id_gen, source_id, doc_name,
                        ));
                    }
                }
                return Err(LemmaError::Engine(
                    "Mathematical operator missing operand".to_string(),
                ));
            }
            _ => {}
        }
    }
    Err(LemmaError::Engine("Empty logical expression".to_string()))
}

fn parse_comparable_base(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
    source_id: &str,
    doc_name: &str,
) -> Result<Expression, LemmaError> {
    // comparable_base = { arithmetic_expression ~ (SPACE+ ~ ^"in" ~ SPACE+ ~ unit_types)? }
    let mut pairs = pair.clone().into_inner();

    let arith_expr = parse_expression(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("No arithmetic expression in comparable_base".to_string())
        })?,
        id_gen,
        source_id,
        doc_name,
    )?;

    // Check for optional "in" unit conversion
    if let Some(unit_pair) = pairs.next() {
        if unit_pair.as_rule() == Rule::unit_word {
            let target_unit = super::units::resolve_conversion_target(unit_pair.as_str())?;
            let kind = ExpressionKind::UnitConversion(Box::new(arith_expr), target_unit);
            return Ok(create_expression_with_location(
                kind, &pair, id_gen, source_id, doc_name,
            ));
        }
    }

    // No unit conversion, just return the arithmetic expression
    Ok(arith_expr)
}

#[cfg(test)]
mod tests {
    use crate::parsing::parse;

    #[test]
    fn test_simple_number() {
        let input = r#"doc test
rule number = 42"#;
        let result = parse(input, None, &crate::ResourceLimits::default());
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
        let result = parse(input, None, &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse fact reference: {:?}",
            result.err()
        );

        let input = r#"doc test
rule nested_ref = employee.salary"#;
        let result = parse(input, None, &crate::ResourceLimits::default());
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
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
            assert!(
                result.is_ok(),
                "Failed to parse {}: {:?}",
                expr,
                result.err()
            );
        }
    }

    #[test]
    fn test_arithmetic_expressions_comprehensive() {
        let test_cases = vec![
            ("2 + 3", "addition"),
            ("10 - 4", "subtraction"),
            ("6 * 7", "multiplication"),
            ("15 / 3", "division"),
            ("17 % 5", "modulo"),
            ("2 ^ 8", "exponentiation"),
            ("2 + 3 * 4", "operator precedence"),
            ("(2 + 3) * 4", "parentheses"),
            ("2 * 3 + 4 * 5", "multiple operations"),
            ("(2 + 3) * (4 + 5)", "nested parentheses"),
            ("-5", "unary minus"),
            ("+10", "unary plus"),
            ("-(2 + 3)", "unary minus with parentheses"),
            ("+(-5)", "nested unary operators"),
            ("age + 5", "variable addition"),
            ("salary * 1.1", "variable multiplication"),
            ("-age", "unary minus on variable"),
            ("0", "zero"),
            ("1", "one"),
            ("-0", "negative zero"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
    fn test_comparison_expressions_comprehensive() {
        let test_cases = vec![
            ("age > 18", "greater than"),
            ("age < 65", "less than"),
            ("age >= 18", "greater than or equal"),
            ("age <= 65", "less than or equal"),
            ("age == 25", "equality"),
            ("age != 30", "inequality"),
            ("name == \"John\"", "string equality"),
            ("name != \"Jane\"", "string inequality"),
            ("status == \"active\"", "status comparison"),
            ("is_active == true", "boolean equality"),
            ("is_active != false", "boolean inequality"),
            ("is_active is true", "is operator"),
            ("is_active is not false", "is not operator"),
            ("age >= 18 and age <= 65", "range check"),
            (
                "salary > 50000 and status == \"active\"",
                "multiple conditions",
            ),
            ("(age + 5) > 21", "arithmetic in comparison"),
            ("age == 0", "zero comparison"),
            ("name == \"\"", "empty string"),
            ("is_active == false", "false comparison"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
    fn test_logical_expressions_comprehensive() {
        let test_cases = vec![
            ("is_active and is_verified", "simple and"),
            ("is_student or is_employee", "simple or"),
            ("not is_blocked", "simple not"),
            ("is_active and not is_blocked", "and with not"),
            (
                "(is_student or is_employee) and is_verified",
                "parentheses with and/or",
            ),
            ("not (is_blocked or is_suspended)", "not with parentheses"),
            ("sqrt(16)", "square root"),
            ("sin(0)", "sine function"),
            ("cos(0)", "cosine function"),
            ("tan(0)", "tangent function"),
            ("log(10)", "logarithm"),
            ("exp(1)", "exponential"),
            (
                "service_started? and not service_ended?",
                "fact references with logical ops",
            ),
            (
                "age >= 18 and (has_license or is_employee)",
                "comparison with logical ops",
            ),
            (
                "sqrt(age * age + salary * salary) > 1000",
                "math function with arithmetic",
            ),
            ("true and false", "boolean literals"),
            ("not true", "not with boolean"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
    fn test_fact_reference_expressions_comprehensive() {
        let test_cases = vec![
            ("age", "simple fact"),
            ("name", "string fact"),
            ("is_active", "boolean fact"),
            ("salary", "numeric fact"),
            ("service_started?", "fact with question mark"),
            ("has_license?", "has fact with question mark"),
            ("is_verified?", "is fact with question mark"),
            ("employee.salary", "nested fact reference"),
            ("person.address.street", "deeply nested fact"),
            ("company.employee.name", "multiple levels"),
            ("user.profile.settings.theme", "deep nesting"),
            ("order.customer.address.zip_code", "real-world example"),
            ("a", "single character"),
            ("very_long_fact_name_with_underscores", "long name"),
            ("fact123", "fact with numbers"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
    fn test_nested_expressions_comprehensive() {
        let test_cases = vec![
            ("(2 + 3) * (4 + 5)", "nested arithmetic"),
            ("((2 + 3) * 4) + 5", "deeply nested arithmetic"),
            ("2 * (3 + (4 * 5))", "mixed nesting"),
            ("(age + 5) > (salary / 12)", "arithmetic in comparison"),
            ("((age >= 18) and (age <= 65))", "nested comparisons"),
            (
                "(is_active and is_verified) or (is_admin and is_trusted)",
                "nested logical",
            ),
            (
                "not (is_blocked or (is_suspended and not is_appealed))",
                "complex nested logical",
            ),
            (
                "(age >= 18) and ((salary > 50000) or (has_degree))",
                "comparison and logical nesting",
            ),
            (
                "sqrt((x * x) + (y * y)) > 100",
                "math function with nested arithmetic",
            ),
            (
                "(service_started? and not service_ended?) or (is_manual and is_verified)",
                "fact refs with nesting",
            ),
            ("((((5))))", "deeply nested parentheses"),
            ("(true)", "boolean in parentheses"),
            ("(\"hello\")", "string in parentheses"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
    fn test_operator_precedence_comprehensive() {
        let test_cases = vec![
            ("2 + 3 * 4", "multiplication before addition"),
            ("2 * 3 + 4 * 5", "multiple operations"),
            ("2 ^ 3 * 4", "exponentiation before multiplication"),
            ("2 * 3 ^ 4", "exponentiation after multiplication"),
            ("2 + 3 * 4 ^ 5", "all arithmetic operators"),
            ("true and false or true", "and before or"),
            ("not true and false", "not before and"),
            ("true or false and true", "and before or"),
            (
                "age >= 18 and salary > 50000 or has_degree",
                "comparison and logical",
            ),
            ("2 + 3 > 4 and 5 * 6 < 40", "arithmetic and comparison"),
            ("(2 + 3) * 4", "parentheses override arithmetic"),
            ("true and (false or true)", "parentheses override logical"),
            (
                "(age >= 18) and (salary > 50000)",
                "parentheses in comparisons",
            ),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
    fn test_parenthesized_expression_edge_cases() {
        let test_cases = vec![
            ("(32 / 7) + 67", "division then addition"),
            ("(2 + 3) * (4 - 1)", "multiple paren groups"),
            ("(10 - 5) / 2 + 3", "paren then mixed ops"),
            ("5 + (3 * 2) - 1", "paren in middle"),
            ("(32 / 7) in kilograms", "paren with unit conversion"),
            ("(100 + 50) in meters", "addition with unit"),
            ("(temperature - 32) * 5 / 9 in celsius", "complex with unit"),
            ("(a + b) > (c + d)", "paren on both sides of comparison"),
            ("(salary * 12) >= 60000", "paren in comparison"),
            ("(x in meters) > 100", "unit conversion in comparison"),
            ("((((5))))", "deeply nested value"),
            ("(((2 + 3) * 4) - 1)", "deeply nested operations"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
    fn test_rule_references_comprehensive() {
        let test_cases = vec![
            ("is_adult?", "simple rule reference"),
            ("service_started?", "service rule reference"),
            ("is_valid? and is_active?", "multiple rule references"),
            ("not is_blocked?", "not with rule reference"),
            ("is_employee? or is_contractor?", "or with rule references"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
    fn test_complex_real_world_expressions() {
        let test_cases = vec![
            ("age >= 18 and (has_license or is_employee)", "age verification with alternatives"),
            ("salary > 50000 and status == \"active\" and not is_on_probation", "employee eligibility"),
            ("(order_total > 100) and (payment_status == \"completed\") and (shipping_address != \"\")", "order validation"),
            ("(cpu_usage < 80) and (memory_usage < 90) and (disk_space > 1024)", "system health check"),
            ("(response_time < 500) and (error_rate < 0.01) and (uptime > 0.99)", "service monitoring"),
            ("sqrt((x - center_x)^2 + (y - center_y)^2) <= radius", "point in circle"),
            ("(a^2 + b^2) == c^2", "Pythagorean theorem check"),
            ("(temperature - 32) * 5 / 9 in celsius", "Fahrenheit to Celsius"),
            ("((user.age >= 18) and (user.verified == true)) or ((user.is_employee == true) and (user.manager_approved == true))", "access control"),
            ("(order.items_count > 0) and ((order.total > 50) or (order.customer.is_vip == true)) and (order.payment.method != \"pending\")", "order processing"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
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
