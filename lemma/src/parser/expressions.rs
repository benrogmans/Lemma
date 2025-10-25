use crate::ast::{ExpressionIdGenerator, Span};
use crate::error::LemmaError;
use crate::parser::Rule;
use crate::semantic::*;
use pest::iterators::Pair;

// Helper to create a traceable Expression with source span and unique ID
fn traceable_expr(
    kind: ExpressionKind,
    pair: &Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Expression {
    Expression::new(
        kind,
        Some(Span::from_pest_span(pair.as_span())),
        id_gen.next_id(),
    )
}

/// Helper function to parse any literal rule into an Expression.
/// Handles both wrapped literals (Rule::literal) and direct literal types.
fn parse_literal_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    // Handle wrapped literals (Rule::literal contains the actual literal type)
    let literal_pair = if pair.as_rule() == Rule::literal {
        pair.into_inner()
            .next()
            .ok_or_else(|| LemmaError::Engine("Empty literal wrapper".to_string()))?
    } else {
        pair
    };

    let literal_value = crate::parser::literals::parse_literal(literal_pair.clone())?;
    Ok(traceable_expr(
        ExpressionKind::Literal(literal_value),
        &literal_pair,
        id_gen,
    ))
}

fn parse_primary(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
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
                return parse_literal_expression(inner, id_gen);
            }
            Rule::reference_expression => {
                return parse_reference_expression(inner, id_gen);
            }
            Rule::rule_reference => {
                let rule_ref = parse_rule_reference(inner.clone())?;
                return Ok(traceable_expr(
                    ExpressionKind::RuleReference(rule_ref),
                    &inner,
                    id_gen,
                ));
            }
            Rule::fact_reference => {
                let fact_ref = parse_fact_reference(inner.clone())?;
                return Ok(traceable_expr(
                    ExpressionKind::FactReference(fact_ref),
                    &inner,
                    id_gen,
                ));
            }
            Rule::expression_group => {
                return parse_or_expression(inner, id_gen);
            }
            _ => {}
        }
    }
    Err(LemmaError::Engine("Empty primary expression".to_string()))
}

pub(crate) fn parse_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    // Check and increment depth
    if let Err(msg) = id_gen.push_depth() {
        return Err(LemmaError::ResourceLimitExceeded {
            limit_name: "max_expression_depth".to_string(),
            limit_value: "100".to_string(),
            actual_value: msg.split_whitespace().nth(2).unwrap_or("unknown").to_string(),
            suggestion: "Simplify nested expressions to reduce depth".to_string(),
        });
    }

    let result = parse_expression_impl(pair, id_gen);
    id_gen.pop_depth();
    result
}

fn parse_expression_impl(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    // Check the current rule first before descending to children
    match pair.as_rule() {
        Rule::comparable_base => return parse_comparable_base(pair, id_gen),
        Rule::term => return parse_term(pair, id_gen),
        Rule::power => return parse_power(pair, id_gen),
        Rule::factor => return parse_factor(pair, id_gen),
        Rule::primary => return parse_primary(pair, id_gen),
        Rule::arithmetic_expression => return parse_arithmetic_expression(pair, id_gen),
        Rule::comparison_expression => return parse_comparison_expression(pair, id_gen),
        Rule::boolean_expression => return parse_logical_expression(pair, id_gen),
        Rule::and_expression => return parse_and_expression(pair, id_gen),
        Rule::or_expression => return parse_or_expression(pair, id_gen),
        Rule::and_operand => return parse_and_operand(pair, id_gen),
        Rule::expression_group => return parse_or_expression(pair, id_gen),
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
                return parse_literal_expression(inner_pair, id_gen);
            }

            // References
            Rule::reference_expression => return parse_reference_expression(inner_pair, id_gen),

            Rule::rule_reference => {
                let rule_ref = parse_rule_reference(inner_pair.clone())?;
                return Ok(traceable_expr(
                    ExpressionKind::RuleReference(rule_ref),
                    &inner_pair,
                    id_gen,
                ));
            }

            Rule::fact_reference => {
                let fact_ref = parse_fact_reference(inner_pair.clone())?;
                return Ok(traceable_expr(
                    ExpressionKind::FactReference(fact_ref),
                    &inner_pair,
                    id_gen,
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
                return parse_expression(inner_pair, id_gen);
            }

            // Logical and mathematical operations
            Rule::not_expr
            | Rule::have_expr
            | Rule::have_not_expr
            | Rule::not_have_expr
            | Rule::sqrt_expr
            | Rule::sin_expr
            | Rule::cos_expr
            | Rule::tan_expr
            | Rule::asin_expr
            | Rule::acos_expr
            | Rule::atan_expr
            | Rule::log_expr
            | Rule::exp_expr => {
                return parse_logical_expression(inner_pair, id_gen);
            }

            Rule::comparable_base | Rule::term | Rule::power | Rule::factor | Rule::expression => {
                return parse_expression(inner_pair, id_gen);
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
) -> Result<Expression, LemmaError> {
    if let Some(inner_pair) = pair.clone().into_inner().next() {
        match inner_pair.as_rule() {
            Rule::rule_reference => {
                let rule_ref = parse_rule_reference(inner_pair)?;
                let kind = ExpressionKind::RuleReference(rule_ref);
                return Ok(traceable_expr(kind, &pair, id_gen));
            }
            Rule::fact_name => {
                let kind = ExpressionKind::FactReference(FactReference {
                    reference: vec![inner_pair.as_str().to_string()],
                });
                return Ok(traceable_expr(kind, &pair, id_gen));
            }
            Rule::fact_reference => {
                let fact_ref = parse_fact_reference(inner_pair)?;
                let kind = ExpressionKind::FactReference(fact_ref);
                return Ok(traceable_expr(kind, &pair, id_gen));
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
    Ok(FactReference { reference })
}

fn parse_rule_reference(pair: Pair<Rule>) -> Result<RuleReference, LemmaError> {
    let mut reference = Vec::new();
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::label {
            reference.push(inner_pair.as_str().to_string());
        }
    }
    Ok(RuleReference { reference })
}

fn parse_and_operand(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    // Grammar: boolean_expression | comparable_base ~ (SPACE* ~ comp_operator ~ SPACE* ~ comparable_base)?
    let mut pairs = pair.into_inner();
    let first = pairs
        .next()
        .ok_or_else(|| LemmaError::Engine("Empty and_operand".to_string()))?;

    // Check if it's a boolean_expression
    if first.as_rule() == Rule::boolean_expression {
        return parse_logical_expression(first, id_gen);
    }

    // Otherwise it's comparable_base with optional comparison
    let left = parse_expression(first, id_gen)?;

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
                Rule::comp_gt => ComparisonOperator::GreaterThan,
                Rule::comp_lt => ComparisonOperator::LessThan,
                Rule::comp_gte => ComparisonOperator::GreaterThanOrEqual,
                Rule::comp_lte => ComparisonOperator::LessThanOrEqual,
                Rule::comp_eq => ComparisonOperator::Equal,
                Rule::comp_ne => ComparisonOperator::NotEqual,
                Rule::comp_is => ComparisonOperator::Is,
                Rule::comp_is_not => ComparisonOperator::IsNot,
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
            )?;
            let kind = ExpressionKind::Comparison(Box::new(left), operator, Box::new(right));
            return Ok(traceable_expr(kind, &op_pair, id_gen));
        }
    }

    // No operator, just return the left side
    Ok(left)
}

fn parse_and_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.into_inner();
    let mut left = parse_and_operand(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("Missing left operand in logical AND expression".to_string())
        })?,
        id_gen,
    )?;

    // The grammar structure is: and_operand ~ (SPACE+ ~ ^"and" ~ SPACE+ ~ and_operand)*
    // We only process and_operand tokens, skipping SPACE and keywords
    for right_pair in pairs {
        if right_pair.as_rule() == Rule::and_operand {
            let right = parse_and_operand(right_pair.clone(), id_gen)?;
            let kind = ExpressionKind::LogicalAnd(Box::new(left), Box::new(right));
            left = traceable_expr(kind, &right_pair, id_gen);
        }
    }

    Ok(left)
}

pub(crate) fn parse_or_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    // Handle expression_group wrapper: expression_group = { or_expression }
    let or_pair = if pair.as_rule() == Rule::expression_group {
        pair.into_inner()
            .next()
            .ok_or_else(|| LemmaError::Engine("Empty expression_group".to_string()))?
    } else {
        pair
    };

    let mut pairs = or_pair.into_inner();
    let mut left = parse_and_expression(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("Missing left operand in logical OR expression".to_string())
        })?,
        id_gen,
    )?;

    // The grammar structure is: and_expression ~ (SPACE+ ~ ^"or" ~ SPACE+ ~ and_expression)*
    // We only process and_expression tokens, skipping SPACE and keywords
    for right_pair in pairs {
        if right_pair.as_rule() == Rule::and_expression {
            let right = parse_and_expression(right_pair.clone(), id_gen)?;
            let kind = ExpressionKind::LogicalOr(Box::new(left), Box::new(right));
            left = traceable_expr(kind, &right_pair, id_gen);
        }
    }

    Ok(left)
}

fn parse_arithmetic_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let mut left = parse_term(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("Missing left term in arithmetic expression".to_string())
        })?,
        id_gen,
    )?;

    while let Some(op_pair) = pairs.next() {
        let operation = match op_pair.as_rule() {
            Rule::add_plus => ArithmeticOperation::Add,
            Rule::add_minus => ArithmeticOperation::Subtract,
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
        )?;

        let kind = ExpressionKind::Arithmetic(Box::new(left), operation, Box::new(right));
        left = traceable_expr(kind, &pair, id_gen);
    }

    Ok(left)
}

fn parse_term(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let mut left = parse_power(
        pairs
            .next()
            .ok_or_else(|| LemmaError::Engine("Missing left power in term".to_string()))?,
        id_gen,
    )?;

    while let Some(op_pair) = pairs.next() {
        let operation = match op_pair.as_rule() {
            Rule::mul_star => ArithmeticOperation::Multiply,
            Rule::mul_slash => ArithmeticOperation::Divide,
            Rule::mul_percent => ArithmeticOperation::Modulo,
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
        )?;

        let kind = ExpressionKind::Arithmetic(Box::new(left), operation, Box::new(right));
        left = traceable_expr(kind, &pair, id_gen);
    }

    Ok(left)
}

fn parse_power(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let left = parse_factor(
        pairs
            .next()
            .ok_or_else(|| LemmaError::Engine("Missing factor in power".to_string()))?,
        id_gen,
    )?;

    if let Some(op_pair) = pairs.next() {
        if op_pair.as_rule() == Rule::pow_caret {
            let right = parse_power(
                pairs.next().ok_or_else(|| {
                    LemmaError::Engine("Missing right power in power expression".to_string())
                })?,
                id_gen,
            )?;

            let kind = ExpressionKind::Arithmetic(
                Box::new(left),
                ArithmeticOperation::Power,
                Box::new(right),
            );
            return Ok(traceable_expr(kind, &pair, id_gen));
        }
    }

    Ok(left)
}

fn parse_factor(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
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
                let expr = parse_expression(first_pair, id_gen)?;
                return Ok(expr);
            }
        }
    }

    // Parse the actual expression after unary operator
    let expr = if let Some(expr_pair) = pairs.next() {
        parse_expression(expr_pair, id_gen)?
    } else {
        return Err(LemmaError::Engine(
            "Missing expression after unary operator".to_string(),
        ));
    };

    // Apply unary operator if present
    if is_negative {
        let zero = traceable_expr(
            ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::ZERO)),
            &pair,
            id_gen,
        );
        let kind = ExpressionKind::Arithmetic(
            Box::new(zero),
            ArithmeticOperation::Subtract,
            Box::new(expr),
        );
        Ok(traceable_expr(kind, &pair, id_gen))
    } else {
        Ok(expr)
    }
}

fn parse_comparison_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    let mut pairs = pair.clone().into_inner();
    let left = parse_expression(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("Missing left operand in comparison expression".to_string())
        })?,
        id_gen,
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
                    Rule::comp_gt => ComparisonOperator::GreaterThan,
                    Rule::comp_lt => ComparisonOperator::LessThan,
                    Rule::comp_gte => ComparisonOperator::GreaterThanOrEqual,
                    Rule::comp_lte => ComparisonOperator::LessThanOrEqual,
                    Rule::comp_eq => ComparisonOperator::Equal,
                    Rule::comp_ne => ComparisonOperator::NotEqual,
                    Rule::comp_is => ComparisonOperator::Is,
                    Rule::comp_is_not => ComparisonOperator::IsNot,
                    _ => {
                        return Err(LemmaError::Engine(format!(
                            "Invalid comparison operator: {:?}",
                            inner_pair.as_rule()
                        )))
                    }
                }
            }
            Rule::comp_gt => ComparisonOperator::GreaterThan,
            Rule::comp_lt => ComparisonOperator::LessThan,
            Rule::comp_gte => ComparisonOperator::GreaterThanOrEqual,
            Rule::comp_lte => ComparisonOperator::LessThanOrEqual,
            Rule::comp_eq => ComparisonOperator::Equal,
            Rule::comp_ne => ComparisonOperator::NotEqual,
            Rule::comp_is => ComparisonOperator::Is,
            Rule::comp_is_not => ComparisonOperator::IsNot,
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
        )?;

        let kind = ExpressionKind::Comparison(Box::new(left), operator, Box::new(right));
        return Ok(traceable_expr(kind, &pair, id_gen));
    }

    Ok(left)
}

fn parse_logical_expression(
    pair: Pair<Rule>,
    id_gen: &mut ExpressionIdGenerator,
) -> Result<Expression, LemmaError> {
    if let Some(node) = pair.into_inner().next() {
        match node.as_rule() {
            Rule::reference_expression => return parse_reference_expression(node, id_gen),
            Rule::literal => return parse_expression(node, id_gen),
            Rule::primary => return parse_primary(node, id_gen),
            Rule::have_expr => {
                for inner in node.clone().into_inner() {
                    if inner.as_rule() == Rule::reference_expression {
                        let ref_expr = parse_reference_expression(inner.clone(), id_gen)?;
                        if let ExpressionKind::FactReference(f) = &ref_expr.kind {
                            let kind = ExpressionKind::FactHasAnyValue(f.clone());
                            return Ok(traceable_expr(kind, &node, id_gen));
                        }
                        return Ok(ref_expr);
                    }
                }
                return Err(LemmaError::Engine("have: missing reference".to_string()));
            }
            Rule::have_not_expr | Rule::not_have_expr | Rule::not_expr => {
                let rule_type = node.as_rule();
                for inner in node.clone().into_inner() {
                    if inner.as_rule() == Rule::reference_expression {
                        let negated_expr = parse_reference_expression(inner, id_gen)?;
                        let negation_type = match rule_type {
                            Rule::not_expr => NegationType::Not,
                            Rule::have_not_expr => NegationType::HaveNot,
                            Rule::not_have_expr => NegationType::NotHave,
                            _ => NegationType::Not,
                        };
                        let kind =
                            ExpressionKind::LogicalNegation(Box::new(negated_expr), negation_type);
                        return Ok(traceable_expr(kind, &node, id_gen));
                    } else if inner.as_rule() == Rule::primary {
                        let negated_expr = parse_primary(inner, id_gen)?;
                        let negation_type = match rule_type {
                            Rule::not_expr => NegationType::Not,
                            Rule::have_not_expr => NegationType::HaveNot,
                            Rule::not_have_expr => NegationType::NotHave,
                            _ => NegationType::Not,
                        };
                        let kind =
                            ExpressionKind::LogicalNegation(Box::new(negated_expr), negation_type);
                        return Ok(traceable_expr(kind, &node, id_gen));
                    } else if inner.as_rule() == Rule::literal {
                        let negated_expr = parse_expression(inner, id_gen)?;
                        let negation_type = match rule_type {
                            Rule::not_expr => NegationType::Not,
                            Rule::have_not_expr => NegationType::HaveNot,
                            Rule::not_have_expr => NegationType::NotHave,
                            _ => NegationType::Not,
                        };
                        let kind =
                            ExpressionKind::LogicalNegation(Box::new(negated_expr), negation_type);
                        return Ok(traceable_expr(kind, &node, id_gen));
                    }
                }
                return Err(LemmaError::Engine(
                    "not/have not: missing reference".to_string(),
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
            | Rule::exp_expr => {
                let operator = match node.as_rule() {
                    Rule::sqrt_expr => MathematicalOperator::Sqrt,
                    Rule::sin_expr => MathematicalOperator::Sin,
                    Rule::cos_expr => MathematicalOperator::Cos,
                    Rule::tan_expr => MathematicalOperator::Tan,
                    Rule::asin_expr => MathematicalOperator::Asin,
                    Rule::acos_expr => MathematicalOperator::Acos,
                    Rule::atan_expr => MathematicalOperator::Atan,
                    Rule::log_expr => MathematicalOperator::Log,
                    Rule::exp_expr => MathematicalOperator::Exp,
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
                        let operand = parse_expression(inner, id_gen)?;
                        let kind =
                            ExpressionKind::MathematicalOperator(operator, Box::new(operand));
                        return Ok(traceable_expr(kind, &node, id_gen));
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
) -> Result<Expression, LemmaError> {
    // comparable_base = { arithmetic_expression ~ (SPACE+ ~ ^"in" ~ SPACE+ ~ unit_types)? }
    let mut pairs = pair.clone().into_inner();

    let arith_expr = parse_expression(
        pairs.next().ok_or_else(|| {
            LemmaError::Engine("No arithmetic expression in comparable_base".to_string())
        })?,
        id_gen,
    )?;

    // Check for optional "in" unit conversion
    if let Some(unit_pair) = pairs.next() {
        if unit_pair.as_rule() == Rule::unit_word {
            let target_unit = super::units::resolve_conversion_target(unit_pair.as_str())?;
            let kind = ExpressionKind::UnitConversion(Box::new(arith_expr), target_unit);
            return Ok(traceable_expr(kind, &pair, id_gen));
        }
    }

    // No unit conversion, just return the arithmetic expression
    Ok(arith_expr)
}
