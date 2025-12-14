//! Mathematical function properties
//!
//! Contains knowledge about mathematical functions:
//! - Range constraints (codomain)
//! - Domain restrictions
//!
//! Used for detecting impossible constraints during planning and inversion.

use crate::algebra::constraints::{DomainRestriction, UnsatReason};
use crate::semantic::{
    ArithmeticComputation, ComparisonComputation, Expression, ExpressionKind, FactPath,
    LiteralValue, MathematicalComputation,
};
use rust_decimal::Decimal;

/// Get the range (codomain) for a mathematical function
///
/// Returns (min, max) where None means unbounded.
/// Both bounds are inclusive.
pub fn function_range(
    op: &MathematicalComputation,
) -> (Option<LiteralValue>, Option<LiteralValue>) {
    match op {
        // Trigonometric functions with bounded range
        MathematicalComputation::Sin | MathematicalComputation::Cos => {
            let min = LiteralValue::Number(Decimal::from(-1));
            let max = LiteralValue::Number(Decimal::from(1));
            (Some(min), Some(max))
        }

        // Inverse trig with bounded range
        MathematicalComputation::Asin | MathematicalComputation::Acos => {
            // asin: [-π/2, π/2], acos: [0, π]
            // For practical purposes, we don't restrict the output range
            // since we care more about the domain restriction
            (None, None)
        }

        // Functions with non-negative range
        MathematicalComputation::Sqrt | MathematicalComputation::Abs => {
            let min = LiteralValue::Number(Decimal::from(0));
            (Some(min), None)
        }

        // exp(x) > 0 always
        MathematicalComputation::Exp => {
            // exp(x) is always positive, never reaches 0
            let min = LiteralValue::Number(Decimal::from(0));
            (Some(min), None)
        }

        // tan, atan, log have unbounded range
        MathematicalComputation::Tan
        | MathematicalComputation::Atan
        | MathematicalComputation::Log => (None, None),

        // Rounding functions preserve sign but don't have bounded range
        MathematicalComputation::Floor
        | MathematicalComputation::Ceil
        | MathematicalComputation::Round => (None, None),
    }
}

/// Check if a comparison with a function violates the function's range
///
/// Returns Some(UnsatReason) if the comparison is impossible given the function's range.
pub fn check_function_range_violation(
    math_op: &MathematicalComputation,
    comparison_op: &ComparisonComputation,
    value: &LiteralValue,
) -> Option<UnsatReason> {
    let (range_min, range_max) = function_range(math_op);

    // Extract numeric value for comparison
    let numeric_value = match value {
        LiteralValue::Number(decimal) => decimal,
        _ => return None, // Non-numeric comparisons can't violate numeric ranges
    };

    match comparison_op {
        // f(x) == value: impossible if value outside range
        ComparisonComputation::Equal(_) => {
            if let Some(LiteralValue::Number(min)) = &range_min {
                if numeric_value < min {
                    return Some(UnsatReason::FunctionRangeViolation {
                        function: format!("{}", math_op),
                        comparison_op: "==".to_string(),
                        required_value: value.clone(),
                        valid_range_min: range_min,
                        valid_range_max: range_max,
                    });
                }
            }
            if let Some(LiteralValue::Number(max)) = &range_max {
                if numeric_value > max {
                    return Some(UnsatReason::FunctionRangeViolation {
                        function: format!("{}", math_op),
                        comparison_op: "==".to_string(),
                        required_value: value.clone(),
                        valid_range_min: range_min,
                        valid_range_max: range_max,
                    });
                }
            }
            // Special case for exp: exp(x) is never exactly 0
            if matches!(math_op, MathematicalComputation::Exp)
                && numeric_value == &Decimal::from(0)
            {
                return Some(UnsatReason::FunctionRangeViolation {
                    function: "exp".to_string(),
                    comparison_op: "==".to_string(),
                    required_value: value.clone(),
                    valid_range_min: range_min,
                    valid_range_max: range_max,
                });
            }
            None
        }

        // f(x) > value: impossible if value >= max
        ComparisonComputation::GreaterThan => {
            if let Some(LiteralValue::Number(max)) = &range_max {
                if numeric_value >= max {
                    return Some(UnsatReason::FunctionRangeViolation {
                        function: format!("{}", math_op),
                        comparison_op: ">".to_string(),
                        required_value: value.clone(),
                        valid_range_min: range_min,
                        valid_range_max: range_max,
                    });
                }
            }
            None
        }

        // f(x) >= value: impossible if value > max
        ComparisonComputation::GreaterThanOrEqual => {
            if let Some(LiteralValue::Number(max)) = &range_max {
                if numeric_value > max {
                    return Some(UnsatReason::FunctionRangeViolation {
                        function: format!("{}", math_op),
                        comparison_op: ">=".to_string(),
                        required_value: value.clone(),
                        valid_range_min: range_min,
                        valid_range_max: range_max,
                    });
                }
            }
            None
        }

        // f(x) < value: impossible if value <= min
        ComparisonComputation::LessThan => {
            if let Some(LiteralValue::Number(min)) = &range_min {
                if numeric_value <= min {
                    return Some(UnsatReason::FunctionRangeViolation {
                        function: format!("{}", math_op),
                        comparison_op: "<".to_string(),
                        required_value: value.clone(),
                        valid_range_min: range_min,
                        valid_range_max: range_max,
                    });
                }
            }
            None
        }

        // f(x) <= value: impossible if value < min
        ComparisonComputation::LessThanOrEqual => {
            if let Some(LiteralValue::Number(min)) = &range_min {
                if numeric_value < min {
                    return Some(UnsatReason::FunctionRangeViolation {
                        function: format!("{}", math_op),
                        comparison_op: "<=".to_string(),
                        required_value: value.clone(),
                        valid_range_min: range_min,
                        valid_range_max: range_max,
                    });
                }
            }
            None
        }

        // f(x) != value: always satisfiable for range checks
        ComparisonComputation::NotEqual(_) => None,
    }
}

/// Collect domain restrictions from an expression
///
/// Walks the expression tree and identifies where functions have restricted domains.
pub fn collect_domain_restrictions(expression: &Expression) -> Vec<DomainRestriction> {
    let mut restrictions = Vec::new();
    collect_domain_restrictions_recursive(expression, &mut restrictions);
    restrictions
}

fn collect_domain_restrictions_recursive(
    expression: &Expression,
    restrictions: &mut Vec<DomainRestriction>,
) {
    match &expression.kind {
        ExpressionKind::MathematicalComputation(op, inner) => {
            // Check if this function has domain restrictions
            if let Some(restriction) = function_domain_restriction(op, inner) {
                restrictions.push(restriction);
            }
            // Recurse into inner expression
            collect_domain_restrictions_recursive(inner, restrictions);
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            // Check for division by zero
            if matches!(op, ArithmeticComputation::Divide) {
                let facts = extract_facts_from_expression(right);
                if !facts.is_empty() {
                    restrictions.push(DomainRestriction {
                        facts,
                        description: "divisor must not be zero".to_string(),
                        source: "division".to_string(),
                    });
                }
            }
            collect_domain_restrictions_recursive(left, restrictions);
            collect_domain_restrictions_recursive(right, restrictions);
        }

        ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
            collect_domain_restrictions_recursive(left, restrictions);
            collect_domain_restrictions_recursive(right, restrictions);
        }

        ExpressionKind::LogicalNegation(inner, _) => {
            collect_domain_restrictions_recursive(inner, restrictions);
        }

        ExpressionKind::Comparison(left, _, right) => {
            collect_domain_restrictions_recursive(left, restrictions);
            collect_domain_restrictions_recursive(right, restrictions);
        }

        ExpressionKind::UnitConversion(inner, _) => {
            collect_domain_restrictions_recursive(inner, restrictions);
        }

        // Leaf nodes - no restrictions
        _ => {}
    }
}

/// Get domain restriction for a mathematical function
fn function_domain_restriction(
    op: &MathematicalComputation,
    argument: &Expression,
) -> Option<DomainRestriction> {
    let facts = extract_facts_from_expression(argument);
    if facts.is_empty() {
        return None;
    }

    match op {
        // sqrt(x) requires x >= 0
        MathematicalComputation::Sqrt => Some(DomainRestriction {
            facts,
            description: "argument must be non-negative".to_string(),
            source: "sqrt domain".to_string(),
        }),

        // log(x) requires x > 0
        MathematicalComputation::Log => Some(DomainRestriction {
            facts,
            description: "argument must be positive".to_string(),
            source: "log domain".to_string(),
        }),

        // tan(x) undefined at π/2 + nπ
        MathematicalComputation::Tan => Some(DomainRestriction {
            facts,
            description: "argument must not equal π/2 + nπ for integer n".to_string(),
            source: "tan domain".to_string(),
        }),

        // asin(x) and acos(x) require x in [-1, 1]
        MathematicalComputation::Asin | MathematicalComputation::Acos => Some(DomainRestriction {
            facts,
            description: "argument must be in [-1, 1]".to_string(),
            source: format!("{} domain", op),
        }),

        // Other functions have unrestricted domains
        _ => None,
    }
}

/// Extract all fact paths from an expression
fn extract_facts_from_expression(expression: &Expression) -> Vec<FactPath> {
    let mut facts = Vec::new();
    extract_facts_recursive(expression, &mut facts);
    facts
}

fn extract_facts_recursive(expression: &Expression, facts: &mut Vec<FactPath>) {
    match &expression.kind {
        ExpressionKind::FactPath(fact_path) => {
            if !facts.contains(fact_path) {
                facts.push(fact_path.clone());
            }
        }

        ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right)
        | ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right) => {
            extract_facts_recursive(left, facts);
            extract_facts_recursive(right, facts);
        }

        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            extract_facts_recursive(inner, facts);
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use std::sync::Arc;

    #[test]
    fn test_sin_range() {
        let (min, max) = function_range(&MathematicalComputation::Sin);
        assert_eq!(min, Some(LiteralValue::Number(Decimal::from(-1))));
        assert_eq!(max, Some(LiteralValue::Number(Decimal::from(1))));
    }

    #[test]
    fn test_cos_range() {
        let (min, max) = function_range(&MathematicalComputation::Cos);
        assert_eq!(min, Some(LiteralValue::Number(Decimal::from(-1))));
        assert_eq!(max, Some(LiteralValue::Number(Decimal::from(1))));
    }

    #[test]
    fn test_sqrt_range() {
        let (min, max) = function_range(&MathematicalComputation::Sqrt);
        assert_eq!(min, Some(LiteralValue::Number(Decimal::from(0))));
        assert_eq!(max, None);
    }

    #[test]
    fn test_exp_range() {
        let (min, max) = function_range(&MathematicalComputation::Exp);
        assert_eq!(min, Some(LiteralValue::Number(Decimal::from(0))));
        assert_eq!(max, None);
    }

    #[test]
    fn test_sin_greater_than_2_is_violation() {
        let result = check_function_range_violation(
            &MathematicalComputation::Sin,
            &ComparisonComputation::GreaterThan,
            &LiteralValue::Number(Decimal::from(2)),
        );
        assert!(matches!(
            result,
            Some(UnsatReason::FunctionRangeViolation { .. })
        ));
    }

    #[test]
    fn test_sin_equals_half_is_valid() {
        let result = check_function_range_violation(
            &MathematicalComputation::Sin,
            &ComparisonComputation::Equal(crate::semantic::EqualityNotation::Symbol),
            &LiteralValue::Number(Decimal::from_str("0.5").unwrap()),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_cos_equals_5_is_violation() {
        let result = check_function_range_violation(
            &MathematicalComputation::Cos,
            &ComparisonComputation::Equal(crate::semantic::EqualityNotation::Symbol),
            &LiteralValue::Number(Decimal::from(5)),
        );
        assert!(matches!(
            result,
            Some(UnsatReason::FunctionRangeViolation { .. })
        ));
    }

    #[test]
    fn test_exp_less_than_0_is_violation() {
        let result = check_function_range_violation(
            &MathematicalComputation::Exp,
            &ComparisonComputation::LessThan,
            &LiteralValue::Number(Decimal::from(0)),
        );
        assert!(matches!(
            result,
            Some(UnsatReason::FunctionRangeViolation { .. })
        ));
    }

    #[test]
    fn test_sqrt_less_than_0_is_violation() {
        let result = check_function_range_violation(
            &MathematicalComputation::Sqrt,
            &ComparisonComputation::LessThan,
            &LiteralValue::Number(Decimal::from(0)),
        );
        assert!(matches!(
            result,
            Some(UnsatReason::FunctionRangeViolation { .. })
        ));
    }

    #[test]
    fn test_sqrt_less_than_5_is_valid() {
        let result = check_function_range_violation(
            &MathematicalComputation::Sqrt,
            &ComparisonComputation::LessThan,
            &LiteralValue::Number(Decimal::from(5)),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_collect_domain_restrictions_sqrt() {
        let fact_path = FactPath::local("x".to_string());
        let expression = Expression::new(
            ExpressionKind::MathematicalComputation(
                MathematicalComputation::Sqrt,
                Arc::new(Expression::new(ExpressionKind::FactPath(fact_path), None)),
            ),
            None,
        );

        let restrictions = collect_domain_restrictions(&expression);
        assert_eq!(restrictions.len(), 1);
        assert_eq!(restrictions[0].source, "sqrt domain");
    }

    #[test]
    fn test_collect_domain_restrictions_division() {
        let x = FactPath::local("x".to_string());
        let y = FactPath::local("y".to_string());
        let expression = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(Expression::new(ExpressionKind::FactPath(x), None)),
                ArithmeticComputation::Divide,
                Arc::new(Expression::new(ExpressionKind::FactPath(y), None)),
            ),
            None,
        );

        let restrictions = collect_domain_restrictions(&expression);
        assert_eq!(restrictions.len(), 1);
        assert_eq!(restrictions[0].source, "division");
    }
}

