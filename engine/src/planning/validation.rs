//! Semantic validation for Lemma specs
//!
//! Validates spec structure and type declarations
//! to catch errors early with clear messages.

use crate::parsing::ast::{
    ComparisonComputation, DateTimeValue, FactValue, LemmaSpec, TimeValue, TypeDef,
};
use crate::planning::semantics::{
    Expression, ExpressionKind, FactData, FactPath, LemmaType, RulePath, SemanticConversionTarget,
    TypeSpecification, ValueKind,
};
use crate::Error;
use crate::Source;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Validate that TypeSpecification constraints are internally consistent
///
/// This checks:
/// - minimum <= maximum (for types that support ranges)
/// - default values satisfy all constraints
/// - length constraints are consistent (for Text)
/// - precision/decimals are within valid ranges
///
/// Returns a vector of errors (empty if valid)
pub fn validate_type_specifications(
    specs: &TypeSpecification,
    type_name: &str,
    source: &Source,
    spec_context: Option<Arc<LemmaSpec>>,
) -> Vec<Error> {
    let mut errors = Vec::new();

    match specs {
        TypeSpecification::Scale {
            minimum,
            maximum,
            decimals,
            precision,
            default,
            units,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate precision is positive if set
            if let Some(prec) = precision {
                if *prec <= Decimal::ZERO {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid precision: {}. Must be positive",
                            type_name, prec
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate default value constraints
            if let Some((def_value, def_unit)) = default {
                // Validate that the default unit exists
                if !units.iter().any(|u| u.name == *def_unit) {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' default unit '{}' is not a valid unit. Valid units: {}",
                            type_name,
                            def_unit,
                            units
                                .iter()
                                .map(|u| u.name.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
                if let Some(min) = minimum {
                    if *def_value < *min {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} {} is less than minimum {}",
                                type_name, def_value, def_unit, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def_value > *max {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} {} is greater than maximum {}",
                                type_name, def_value, def_unit, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }

            // Scale types must have at least one unit (required for parsing and conversion)
            if units.is_empty() {
                errors.push(Error::validation_with_context(
                    format!(
                        "Type '{}' is a scale type but has no units. Scale types must define at least one unit (e.g. -> unit eur 1).",
                        type_name
                    ),
                    Some(source.clone()),
                    None::<String>,
                    spec_context.clone(),
                    None,
                ));
            }

            // Validate units (if present)
            if !units.is_empty() {
                let mut seen_names: Vec<String> = Vec::new();
                for unit in units.iter() {
                    // Validate unit name is not empty
                    if unit.name.trim().is_empty() {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' has a unit with empty name. Unit names cannot be empty.",
                                type_name
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }

                    // Validate unit names are unique within the type (case-insensitive)
                    let lower_name = unit.name.to_lowercase();
                    if seen_names
                        .iter()
                        .any(|seen| seen.to_lowercase() == lower_name)
                    {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has duplicate unit name '{}' (case-insensitive). Unit names must be unique within a type.", type_name, unit.name),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    // Validate unit values are positive (conversion factors relative to type base of 1)
                    if unit.value <= Decimal::ZERO {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has unit '{}' with invalid value {}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
        }
        TypeSpecification::Number {
            minimum,
            maximum,
            decimals,
            precision,
            default,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate precision is positive if set
            if let Some(prec) = precision {
                if *prec <= Decimal::ZERO {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid precision: {}. Must be positive",
                            type_name, prec
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
            // Note: Number types are dimensionless and cannot have units (validated in apply_constraint)
        }

        TypeSpecification::Ratio {
            minimum,
            maximum,
            decimals,
            default,
            units,
            ..
        } => {
            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }

            // Validate units (if present)
            // Types can have zero units (e.g., type ratio: number -> ratio) - this is valid
            // Only validate if units are defined
            if !units.is_empty() {
                let mut seen_names: Vec<String> = Vec::new();
                for unit in units.iter() {
                    // Validate unit name is not empty
                    if unit.name.trim().is_empty() {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' has a unit with empty name. Unit names cannot be empty.",
                                type_name
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }

                    // Validate unit names are unique within the type (case-insensitive)
                    let lower_name = unit.name.to_lowercase();
                    if seen_names
                        .iter()
                        .any(|seen| seen.to_lowercase() == lower_name)
                    {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has duplicate unit name '{}' (case-insensitive). Unit names must be unique within a type.", type_name, unit.name),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    // Validate unit values are positive (conversion factors relative to type base of 1)
                    if unit.value <= Decimal::ZERO {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has unit '{}' with invalid value {}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
        }

        TypeSpecification::Text {
            minimum,
            maximum,
            length,
            options,
            default,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(Error::validation_with_context(
                        format!("Type '{}' has invalid range: minimum length {} is greater than maximum length {}", type_name, min, max),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate length consistency
            if let Some(len) = length {
                if let Some(min) = minimum {
                    if *len < *min {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has inconsistent length constraint: length {} is less than minimum {}", type_name, len, min),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *len > *max {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has inconsistent length constraint: length {} is greater than maximum {}", type_name, len, max),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                let def_len = def.len();

                if let Some(min) = minimum {
                    if def_len < *min {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value length {} is less than minimum {}",
                                type_name, def_len, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if def_len > *max {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value length {} is greater than maximum {}",
                                type_name, def_len, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(len) = length {
                    if def_len != *len {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' default value length {} does not match required length {}", type_name, def_len, len),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if !options.is_empty() && !options.contains(def) {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' default value '{}' is not in allowed options: {:?}",
                            type_name, def, options
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }
        }

        TypeSpecification::Date {
            minimum,
            maximum,
            default,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if compare_date_values(min, max) == Ordering::Greater {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid date range: minimum {} is after maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if compare_date_values(def, min) == Ordering::Less {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default date {} is before minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if compare_date_values(def, max) == Ordering::Greater {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default date {} is after maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
        }

        TypeSpecification::Time {
            minimum,
            maximum,
            default,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if compare_time_values(min, max) == Ordering::Greater {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid time range: minimum {} is after maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if compare_time_values(def, min) == Ordering::Less {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default time {} is before minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if compare_time_values(def, max) == Ordering::Greater {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default time {} is after maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
        }

        TypeSpecification::Boolean { .. } | TypeSpecification::Duration { .. } => {
            // No constraint validation needed for these types
        }
        TypeSpecification::Veto { .. } => {
            // Veto is not a user-declarable type, so validation should not be called on it
            // But if it is, there's nothing to validate
        }
        TypeSpecification::Undetermined => unreachable!(
            "BUG: validate_type_specification_constraints called with Undetermined sentinel type; this type exists only during type inference"
        ),
    }

    errors
}

/// Compare two DateTimeValue instances for ordering
fn compare_date_values(left: &DateTimeValue, right: &DateTimeValue) -> Ordering {
    // Compare by year, month, day, hour, minute, second
    left.year
        .cmp(&right.year)
        .then_with(|| left.month.cmp(&right.month))
        .then_with(|| left.day.cmp(&right.day))
        .then_with(|| left.hour.cmp(&right.hour))
        .then_with(|| left.minute.cmp(&right.minute))
        .then_with(|| left.second.cmp(&right.second))
}

/// Compare two TimeValue instances for ordering
fn compare_time_values(left: &TimeValue, right: &TimeValue) -> Ordering {
    // Compare by hour, minute, second
    left.hour
        .cmp(&right.hour)
        .then_with(|| left.minute.cmp(&right.minute))
        .then_with(|| left.second.cmp(&right.second))
}

// -----------------------------------------------------------------------------
// Spec interface validation (required rule names + rule result types)
// -----------------------------------------------------------------------------

/// Rule data needed to validate spec interfaces (inference snapshot before apply).
pub struct RuleEntryForBindingCheck {
    pub rule_type: LemmaType,
    pub depends_on_rules: std::collections::BTreeSet<RulePath>,
    pub branches: Vec<(Option<Expression>, Expression)>,
}

#[derive(Clone, Copy, Debug)]
enum BaseTypeRequirement {
    Any,
    Boolean,
    Number,
    Duration,
    Ratio,
    Scale,
    Text,
    Date,
    Time,
    Comparable,
    Numeric,
}

#[derive(Clone, Debug)]
struct NumericLiteralConstraint {
    op: ComparisonComputation,
    literal: Decimal,
    reference_on_left: bool,
}

#[derive(Clone, Debug)]
enum RuleRefRequirement {
    Base(BaseTypeRequirement),
    ScaleMustContainUnit(String),
    RatioMustContainUnit(String),
    SameBaseAs(LemmaType),
    SameScaleFamilyAs(LemmaType),
    ArithmeticCompatibleWithNumber,
    ArithmeticCompatibleWithRatio,
    ArithmeticCompatibleWithScale(LemmaType),
    ArithmeticCompatibleWithDuration,
    NumericLiteral(NumericLiteralConstraint),
}

impl RuleRefRequirement {
    fn describe(&self) -> String {
        match self {
            RuleRefRequirement::Base(kind) => match kind {
                BaseTypeRequirement::Any => "any".to_string(),
                BaseTypeRequirement::Boolean => "boolean".to_string(),
                BaseTypeRequirement::Number => "number".to_string(),
                BaseTypeRequirement::Duration => "duration".to_string(),
                BaseTypeRequirement::Ratio => "ratio".to_string(),
                BaseTypeRequirement::Scale => "scale".to_string(),
                BaseTypeRequirement::Text => "text".to_string(),
                BaseTypeRequirement::Date => "date".to_string(),
                BaseTypeRequirement::Time => "time".to_string(),
                BaseTypeRequirement::Comparable => "comparable".to_string(),
                BaseTypeRequirement::Numeric => "numeric (number, scale, or ratio)".to_string(),
            },
            RuleRefRequirement::ScaleMustContainUnit(unit) => {
                format!("scale type containing unit '{}'", unit)
            }
            RuleRefRequirement::RatioMustContainUnit(unit) => {
                format!("ratio type containing unit '{}'", unit)
            }
            RuleRefRequirement::SameBaseAs(other) => {
                format!("same base type as {}", other.name())
            }
            RuleRefRequirement::SameScaleFamilyAs(other) => {
                format!("same scale family as {}", other.name())
            }
            RuleRefRequirement::ArithmeticCompatibleWithNumber => {
                "arithmetic-compatible with number (number or ratio)".to_string()
            }
            RuleRefRequirement::ArithmeticCompatibleWithRatio => {
                "arithmetic-compatible with ratio".to_string()
            }
            RuleRefRequirement::ArithmeticCompatibleWithScale(other) => {
                format!("arithmetic-compatible with scale family {}", other.name())
            }
            RuleRefRequirement::ArithmeticCompatibleWithDuration => {
                "arithmetic-compatible with duration".to_string()
            }
            RuleRefRequirement::NumericLiteral(rule) => {
                let side = if rule.reference_on_left {
                    "left"
                } else {
                    "right"
                };
                format!(
                    "numeric range compatible with comparison (rule-ref {} side, op {:?}, literal {})",
                    side, rule.op, rule.literal
                )
            }
        }
    }
}

fn lemma_type_to_base_requirement(lemma_type: &LemmaType) -> BaseTypeRequirement {
    if lemma_type.is_boolean() {
        return BaseTypeRequirement::Boolean;
    }
    if lemma_type.is_number() {
        return BaseTypeRequirement::Number;
    }
    if lemma_type.is_scale() {
        return BaseTypeRequirement::Scale;
    }
    if lemma_type.is_duration() {
        return BaseTypeRequirement::Duration;
    }
    if lemma_type.is_ratio() {
        return BaseTypeRequirement::Ratio;
    }
    if lemma_type.is_text() {
        return BaseTypeRequirement::Text;
    }
    if lemma_type.is_date() {
        return BaseTypeRequirement::Date;
    }
    if lemma_type.is_time() {
        return BaseTypeRequirement::Time;
    }
    BaseTypeRequirement::Any
}

fn base_requirement_satisfied(lemma_type: &LemmaType, constraint: BaseTypeRequirement) -> bool {
    match constraint {
        BaseTypeRequirement::Any => true,
        BaseTypeRequirement::Boolean => lemma_type.is_boolean(),
        BaseTypeRequirement::Number => lemma_type.is_number(),
        BaseTypeRequirement::Duration => lemma_type.is_duration(),
        BaseTypeRequirement::Ratio => lemma_type.is_ratio(),
        BaseTypeRequirement::Scale => lemma_type.is_scale(),
        BaseTypeRequirement::Text => lemma_type.is_text(),
        BaseTypeRequirement::Date => lemma_type.is_date(),
        BaseTypeRequirement::Time => lemma_type.is_time(),
        BaseTypeRequirement::Numeric => {
            lemma_type.is_number() || lemma_type.is_scale() || lemma_type.is_ratio()
        }
        BaseTypeRequirement::Comparable => {
            lemma_type.is_boolean()
                || lemma_type.is_text()
                || lemma_type.is_number()
                || lemma_type.is_ratio()
                || lemma_type.is_date()
                || lemma_type.is_time()
                || lemma_type.is_scale()
                || lemma_type.is_duration()
        }
    }
}

fn has_scale_unit(lemma_type: &LemmaType, unit: &str) -> bool {
    match &lemma_type.specifications {
        TypeSpecification::Scale { units, .. } => {
            units.iter().any(|u| u.name.eq_ignore_ascii_case(unit))
        }
        _ => false,
    }
}

fn has_ratio_unit(lemma_type: &LemmaType, unit: &str) -> bool {
    match &lemma_type.specifications {
        TypeSpecification::Ratio { units, .. } => {
            units.iter().any(|u| u.name.eq_ignore_ascii_case(unit))
        }
        _ => false,
    }
}

fn numeric_bounds(lemma_type: &LemmaType) -> Option<(Option<Decimal>, Option<Decimal>)> {
    match &lemma_type.specifications {
        TypeSpecification::Number {
            minimum, maximum, ..
        }
        | TypeSpecification::Scale {
            minimum, maximum, ..
        }
        | TypeSpecification::Ratio {
            minimum, maximum, ..
        } => Some((*minimum, *maximum)),
        _ => None,
    }
}

fn normalize_literal_constraint(rule: NumericLiteralConstraint) -> NumericLiteralConstraint {
    if rule.reference_on_left {
        return rule;
    }
    let op = match rule.op {
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThan,
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThan,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThanOrEqual,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThanOrEqual,
        ComparisonComputation::Is => ComparisonComputation::Is,
        ComparisonComputation::IsNot => ComparisonComputation::IsNot,
    };
    NumericLiteralConstraint {
        op,
        literal: rule.literal,
        reference_on_left: true,
    }
}

fn numeric_literal_constraint_satisfied(
    lemma_type: &LemmaType,
    rule: NumericLiteralConstraint,
) -> bool {
    let Some((minimum, maximum)) = numeric_bounds(lemma_type) else {
        return false;
    };
    let normalized = normalize_literal_constraint(rule);
    match normalized.op {
        ComparisonComputation::GreaterThan => maximum.is_none_or(|max| max > normalized.literal),
        ComparisonComputation::GreaterThanOrEqual => {
            maximum.is_none_or(|max| max >= normalized.literal)
        }
        ComparisonComputation::LessThan => minimum.is_none_or(|min| min < normalized.literal),
        ComparisonComputation::LessThanOrEqual => {
            minimum.is_none_or(|min| min <= normalized.literal)
        }
        ComparisonComputation::Is => {
            minimum.is_none_or(|min| min <= normalized.literal)
                && maximum.is_none_or(|max| max >= normalized.literal)
        }
        ComparisonComputation::IsNot => {
            !(minimum == Some(normalized.literal) && maximum == Some(normalized.literal))
        }
    }
}

fn rule_type_satisfies_requirement(
    lemma_type: &LemmaType,
    requirement: &RuleRefRequirement,
) -> bool {
    if lemma_type.is_undetermined() {
        unreachable!("BUG: rule_type_satisfies_requirement called with undetermined type; this type exists only during type inference")
    }
    // veto is control flow, not a type incompatibility -- it propagates at runtime
    if lemma_type.vetoed() {
        return true;
    }
    match requirement {
        RuleRefRequirement::Base(kind) => base_requirement_satisfied(lemma_type, *kind),
        RuleRefRequirement::ScaleMustContainUnit(unit) => {
            lemma_type.is_scale() && has_scale_unit(lemma_type, unit)
        }
        RuleRefRequirement::RatioMustContainUnit(unit) => {
            lemma_type.is_ratio() && has_ratio_unit(lemma_type, unit)
        }
        RuleRefRequirement::SameBaseAs(other) => lemma_type.has_same_base_type(other),
        RuleRefRequirement::SameScaleFamilyAs(other) => {
            lemma_type.is_scale() && other.is_scale() && lemma_type.same_scale_family(other)
        }
        RuleRefRequirement::ArithmeticCompatibleWithNumber => {
            lemma_type.is_number() || lemma_type.is_ratio()
        }
        RuleRefRequirement::ArithmeticCompatibleWithRatio => {
            lemma_type.is_number()
                || lemma_type.is_ratio()
                || lemma_type.is_scale()
                || lemma_type.is_duration()
        }
        RuleRefRequirement::ArithmeticCompatibleWithScale(other) => {
            lemma_type.is_number()
                || lemma_type.is_ratio()
                || (lemma_type.is_scale()
                    && other.is_scale()
                    && lemma_type.same_scale_family(other))
        }
        RuleRefRequirement::ArithmeticCompatibleWithDuration => {
            lemma_type.is_number() || lemma_type.is_ratio() || lemma_type.is_duration()
        }
        RuleRefRequirement::NumericLiteral(rule) => {
            numeric_literal_constraint_satisfied(lemma_type, rule.clone())
        }
    }
}

fn infer_interface_expression_type(
    expr: &Expression,
    rule_entries: &IndexMap<RulePath, RuleEntryForBindingCheck>,
    facts: &IndexMap<FactPath, FactData>,
) -> Option<LemmaType> {
    match &expr.kind {
        ExpressionKind::Literal(lv) => Some(lv.lemma_type.clone()),
        ExpressionKind::FactPath(fp) => facts.get(fp).and_then(|f| f.schema_type().cloned()),
        ExpressionKind::RulePath(rp) => rule_entries.get(rp).map(|r| r.rule_type.clone()),
        _ => None,
    }
}

fn numeric_literal_from_expression(expr: &Expression) -> Option<Decimal> {
    let ExpressionKind::Literal(lv) = &expr.kind else {
        return None;
    };
    match &lv.value {
        ValueKind::Number(n) => Some(*n),
        _ => None,
    }
}

fn collect_expected_requirements_for_rule_ref(
    expr: &Expression,
    rule_path: &RulePath,
    expected: RuleRefRequirement,
    rule_entries: &IndexMap<RulePath, RuleEntryForBindingCheck>,
    facts: &IndexMap<FactPath, FactData>,
) -> Vec<(Option<Source>, RuleRefRequirement)> {
    let mut out = Vec::new();
    match &expr.kind {
        ExpressionKind::RulePath(rp) => {
            if rp == rule_path {
                out.push((expr.source_location.clone(), expected));
            }
        }
        ExpressionKind::LogicalAnd(left, right) => {
            out.extend(collect_expected_requirements_for_rule_ref(
                left,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Boolean),
                rule_entries,
                facts,
            ));
            out.extend(collect_expected_requirements_for_rule_ref(
                right,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Boolean),
                rule_entries,
                facts,
            ));
        }
        ExpressionKind::LogicalNegation(operand, _) => {
            out.extend(collect_expected_requirements_for_rule_ref(
                operand,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Boolean),
                rule_entries,
                facts,
            ));
        }
        ExpressionKind::Comparison(left, op, right) => {
            out.extend(collect_expected_requirements_for_rule_ref(
                left,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Comparable),
                rule_entries,
                facts,
            ));
            out.extend(collect_expected_requirements_for_rule_ref(
                right,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Comparable),
                rule_entries,
                facts,
            ));

            if let ExpressionKind::RulePath(rp) = &left.kind {
                if rp == rule_path {
                    if let Some(other_type) =
                        infer_interface_expression_type(right, rule_entries, facts)
                    {
                        out.push((
                            expr.source_location.clone(),
                            RuleRefRequirement::SameBaseAs(other_type.clone()),
                        ));
                        if other_type.is_scale() {
                            out.push((
                                expr.source_location.clone(),
                                RuleRefRequirement::SameScaleFamilyAs(other_type),
                            ));
                        }
                    }
                    if let Some(lit) = numeric_literal_from_expression(right) {
                        out.push((
                            expr.source_location.clone(),
                            RuleRefRequirement::NumericLiteral(NumericLiteralConstraint {
                                op: op.clone(),
                                literal: lit,
                                reference_on_left: true,
                            }),
                        ));
                    }
                }
            }
            if let ExpressionKind::RulePath(rp) = &right.kind {
                if rp == rule_path {
                    if let Some(other_type) =
                        infer_interface_expression_type(left, rule_entries, facts)
                    {
                        out.push((
                            expr.source_location.clone(),
                            RuleRefRequirement::SameBaseAs(other_type.clone()),
                        ));
                        if other_type.is_scale() {
                            out.push((
                                expr.source_location.clone(),
                                RuleRefRequirement::SameScaleFamilyAs(other_type),
                            ));
                        }
                    }
                    if let Some(lit) = numeric_literal_from_expression(left) {
                        out.push((
                            expr.source_location.clone(),
                            RuleRefRequirement::NumericLiteral(NumericLiteralConstraint {
                                op: op.clone(),
                                literal: lit,
                                reference_on_left: false,
                            }),
                        ));
                    }
                }
            }
        }
        ExpressionKind::Arithmetic(left, _, right) => {
            out.extend(collect_expected_requirements_for_rule_ref(
                left,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Numeric),
                rule_entries,
                facts,
            ));
            out.extend(collect_expected_requirements_for_rule_ref(
                right,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Numeric),
                rule_entries,
                facts,
            ));

            if let ExpressionKind::RulePath(rp) = &left.kind {
                if rp == rule_path {
                    if let Some(other_type) =
                        infer_interface_expression_type(right, rule_entries, facts)
                    {
                        if other_type.is_scale() {
                            out.push((
                                expr.source_location.clone(),
                                RuleRefRequirement::ArithmeticCompatibleWithScale(other_type),
                            ));
                        } else if other_type.is_number() || other_type.is_ratio() {
                            out.push((
                                expr.source_location.clone(),
                                if other_type.is_number() {
                                    RuleRefRequirement::ArithmeticCompatibleWithNumber
                                } else {
                                    RuleRefRequirement::ArithmeticCompatibleWithRatio
                                },
                            ));
                        } else if other_type.is_duration() {
                            out.push((
                                expr.source_location.clone(),
                                RuleRefRequirement::ArithmeticCompatibleWithDuration,
                            ));
                        }
                    }
                }
            }
            if let ExpressionKind::RulePath(rp) = &right.kind {
                if rp == rule_path {
                    if let Some(other_type) =
                        infer_interface_expression_type(left, rule_entries, facts)
                    {
                        if other_type.is_scale() {
                            out.push((
                                expr.source_location.clone(),
                                RuleRefRequirement::ArithmeticCompatibleWithScale(other_type),
                            ));
                        } else if other_type.is_number() || other_type.is_ratio() {
                            out.push((
                                expr.source_location.clone(),
                                if other_type.is_number() {
                                    RuleRefRequirement::ArithmeticCompatibleWithNumber
                                } else {
                                    RuleRefRequirement::ArithmeticCompatibleWithRatio
                                },
                            ));
                        } else if other_type.is_duration() {
                            out.push((
                                expr.source_location.clone(),
                                RuleRefRequirement::ArithmeticCompatibleWithDuration,
                            ));
                        }
                    }
                }
            }
        }
        ExpressionKind::UnitConversion(source, target) => {
            let constraint = match target {
                SemanticConversionTarget::Duration(_) => {
                    RuleRefRequirement::Base(BaseTypeRequirement::Duration)
                }
                SemanticConversionTarget::ScaleUnit(unit) => {
                    RuleRefRequirement::ScaleMustContainUnit(unit.clone())
                }
                SemanticConversionTarget::RatioUnit(unit) => {
                    RuleRefRequirement::RatioMustContainUnit(unit.clone())
                }
            };
            out.extend(collect_expected_requirements_for_rule_ref(
                source,
                rule_path,
                constraint,
                rule_entries,
                facts,
            ));
        }
        ExpressionKind::MathematicalComputation(_, operand) => {
            out.extend(collect_expected_requirements_for_rule_ref(
                operand,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Number),
                rule_entries,
                facts,
            ));
        }
        ExpressionKind::DateRelative(_, date_expr, tolerance) => {
            out.extend(collect_expected_requirements_for_rule_ref(
                date_expr,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Date),
                rule_entries,
                facts,
            ));
            if let Some(tol) = tolerance {
                out.extend(collect_expected_requirements_for_rule_ref(
                    tol,
                    rule_path,
                    RuleRefRequirement::Base(BaseTypeRequirement::Duration),
                    rule_entries,
                    facts,
                ));
            }
        }
        ExpressionKind::DateCalendar(_, _, date_expr) => {
            out.extend(collect_expected_requirements_for_rule_ref(
                date_expr,
                rule_path,
                RuleRefRequirement::Base(BaseTypeRequirement::Date),
                rule_entries,
                facts,
            ));
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::FactPath(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Now => {}
    }
    out
}

fn spec_interface_error(
    source: &Source,
    message: impl Into<String>,
    spec_context: Option<Arc<LemmaSpec>>,
    related_spec: Option<Arc<LemmaSpec>>,
) -> Error {
    Error::validation_with_context(
        message.into(),
        Some(source.clone()),
        None::<String>,
        spec_context,
        related_spec,
    )
}

/// Validate cross-spec IO contracts for spec-reference bindings.
///
/// Enforces:
/// - required referenced rule names exist on the provider spec
/// - referenced rule result types satisfy structural requirements implied by
///   the consumer expression context (base kind, units, scale family, bounds)
///
/// This runs at planning time and reports binding-site errors when a provider
/// interface is incompatible with consumer expectations.
pub fn validate_spec_interfaces(
    referenced_rules: &HashMap<Vec<String>, HashSet<String>>,
    spec_ref_facts: &[(FactPath, Arc<LemmaSpec>, Source)],
    facts: &IndexMap<FactPath, FactData>,
    rule_entries: &IndexMap<RulePath, RuleEntryForBindingCheck>,
    main_spec: &Arc<LemmaSpec>,
) -> Result<(), Vec<Error>> {
    let mut errors = Vec::new();

    for (fact_path, spec_arc, fact_source) in spec_ref_facts {
        let mut full_path: Vec<String> =
            fact_path.segments.iter().map(|s| s.fact.clone()).collect();
        full_path.push(fact_path.fact.clone());

        let Some(required_rules) = referenced_rules.get(&full_path) else {
            continue;
        };

        let spec = spec_arc.as_ref();
        let spec_rule_names: HashSet<&str> = spec.rules.iter().map(|r| r.name.as_str()).collect();

        for required_rule in required_rules {
            if !spec_rule_names.contains(required_rule.as_str()) {
                errors.push(spec_interface_error(
                    fact_source,
                    format!(
                        "Spec '{}' referenced by '{}' is missing required rule '{}'",
                        spec.name, fact_path, required_rule
                    ),
                    Some(Arc::clone(main_spec)),
                    Some(Arc::clone(spec_arc)),
                ));
                continue;
            }

            let mut ref_segments = fact_path.segments.clone();
            ref_segments.push(crate::planning::semantics::PathSegment {
                fact: fact_path.fact.clone(),
                spec: spec.name.clone(),
            });
            let ref_rule_path = RulePath::new(ref_segments, required_rule.clone());
            let Some(ref_entry) = rule_entries.get(&ref_rule_path) else {
                let binding_path_str = fact_path
                    .segments
                    .iter()
                    .map(|s| s.fact.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                let binding_path_str = if binding_path_str.is_empty() {
                    fact_path.fact.clone()
                } else {
                    format!("{}.{}", binding_path_str, fact_path.fact)
                };
                errors.push(spec_interface_error(
                    fact_source,
                    format!(
                        "Fact binding '{}' sets spec reference to '{}', but interface validation could not resolve rule path '{}.{}' for contract checking",
                        binding_path_str, spec.name, fact_path.fact, required_rule
                    ),
                    Some(Arc::clone(main_spec)),
                    Some(Arc::clone(spec_arc)),
                ));
                continue;
            };
            let ref_rule_type = &ref_entry.rule_type;

            for (_referencing_path, entry) in rule_entries {
                if !entry.depends_on_rules.contains(&ref_rule_path) {
                    continue;
                }
                let expected =
                    RuleRefRequirement::Base(lemma_type_to_base_requirement(&entry.rule_type));
                for (_condition, result_expr) in &entry.branches {
                    let requirements = collect_expected_requirements_for_rule_ref(
                        result_expr,
                        &ref_rule_path,
                        expected.clone(),
                        rule_entries,
                        facts,
                    );
                    for (_source, requirement) in requirements {
                        if !rule_type_satisfies_requirement(ref_rule_type, &requirement) {
                            let report_source = fact_source;

                            let binding_path_str = fact_path
                                .segments
                                .iter()
                                .map(|s| s.fact.as_str())
                                .collect::<Vec<_>>()
                                .join(".");
                            let binding_path_str = if binding_path_str.is_empty() {
                                fact_path.fact.clone()
                            } else {
                                format!("{}.{}", binding_path_str, fact_path.fact)
                            };

                            errors.push(spec_interface_error(
                                report_source,
                                format!(
                                    "Fact binding '{}' sets spec reference to '{}', but that spec's rule '{}' has result type {}; the referencing expression expects a {} value",
                                    binding_path_str,
                                    spec.name,
                                    required_rule,
                                    ref_rule_type.name(),
                                    requirement.describe(),
                                ),
                                Some(Arc::clone(main_spec)),
                                Some(Arc::clone(spec_arc)),
                            ));
                        }
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate that a registry spec (`from_registry == true`) does not contain
/// bare (non-`@`) references. The registry is responsible for rewriting all
/// spec references to use `@`-prefixed names before serving the bundle.
///
/// Returns a list of bare reference names found, empty if valid.
pub fn collect_bare_registry_refs(spec: &LemmaSpec) -> Vec<String> {
    if !spec.from_registry {
        return Vec::new();
    }
    let mut bare: Vec<String> = Vec::new();
    for fact in &spec.facts {
        match &fact.value {
            FactValue::SpecReference(r) if !r.from_registry => {
                bare.push(r.name.clone());
            }
            FactValue::TypeDeclaration { from: Some(r), .. } if !r.from_registry => {
                bare.push(r.name.clone());
            }
            _ => {}
        }
    }
    for type_def in &spec.types {
        match type_def {
            TypeDef::Import { from, .. } if !from.from_registry => {
                bare.push(from.name.clone());
            }
            TypeDef::Inline { from: Some(r), .. } if !r.from_registry => {
                bare.push(r.name.clone());
            }
            _ => {}
        }
    }
    bare
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::{CommandArg, TypeConstraintCommand};
    use crate::planning::semantics::{
        LemmaType, RatioUnit, RatioUnits, ScaleUnit, ScaleUnits, TypeSpecification,
    };
    use rust_decimal::Decimal;

    fn test_source() -> Source {
        Source::new(
            "<test>",
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        )
    }

    #[test]
    fn validate_number_minimum_greater_than_maximum() {
        let mut specs = TypeSpecification::number();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Minimum,
                &[CommandArg::Number("100".to_string())],
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Maximum,
                &[CommandArg::Number("50".to_string())],
            )
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum 100 is greater than maximum 50"));
    }

    #[test]
    fn validate_number_valid_range() {
        let mut specs = TypeSpecification::number();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Minimum,
                &[CommandArg::Number("0".to_string())],
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Maximum,
                &[CommandArg::Number("100".to_string())],
            )
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_number_default_below_minimum() {
        let specs = TypeSpecification::Number {
            minimum: Some(Decimal::from(10)),
            maximum: None,
            decimals: None,
            precision: None,
            help: String::new(),
            default: Some(Decimal::from(5)),
        };

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("default value 5 is less than minimum 10"));
    }

    #[test]
    fn validate_number_default_above_maximum() {
        let specs = TypeSpecification::Number {
            minimum: None,
            maximum: Some(Decimal::from(100)),
            decimals: None,
            precision: None,
            help: String::new(),
            default: Some(Decimal::from(150)),
        };

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("default value 150 is greater than maximum 100"));
    }

    #[test]
    fn validate_number_default_valid() {
        let specs = TypeSpecification::Number {
            minimum: Some(Decimal::from(0)),
            maximum: Some(Decimal::from(100)),
            decimals: None,
            precision: None,
            help: String::new(),
            default: Some(Decimal::from(50)),
        };

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_text_minimum_greater_than_maximum() {
        let mut specs = TypeSpecification::text();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Minimum,
                &[CommandArg::Number("100".to_string())],
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Maximum,
                &[CommandArg::Number("50".to_string())],
            )
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum length 100 is greater than maximum length 50"));
    }

    #[test]
    fn validate_text_length_inconsistent_with_minimum() {
        let mut specs = TypeSpecification::text();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Minimum,
                &[CommandArg::Number("10".to_string())],
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Length,
                &[CommandArg::Number("5".to_string())],
            )
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("length 5 is less than minimum 10"));
    }

    #[test]
    fn validate_text_default_not_in_options() {
        let specs = TypeSpecification::Text {
            minimum: None,
            maximum: None,
            length: None,
            options: vec!["red".to_string(), "blue".to_string()],
            help: String::new(),
            default: Some("green".to_string()),
        };

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("default value 'green' is not in allowed options"));
    }

    #[test]
    fn validate_text_default_valid_in_options() {
        let specs = TypeSpecification::Text {
            minimum: None,
            maximum: None,
            length: None,
            options: vec!["red".to_string(), "blue".to_string()],
            help: String::new(),
            default: Some("red".to_string()),
        };

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_ratio_minimum_greater_than_maximum() {
        let specs = TypeSpecification::Ratio {
            minimum: Some(Decimal::from(2)),
            maximum: Some(Decimal::from(1)),
            decimals: None,
            units: crate::planning::semantics::RatioUnits::new(),
            help: String::new(),
            default: None,
        };

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum 2 is greater than maximum 1"));
    }

    #[test]
    fn validate_date_minimum_after_maximum() {
        let mut specs = TypeSpecification::date();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Minimum,
                &[CommandArg::Label("2024-12-31".to_string())],
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Maximum,
                &[CommandArg::Label("2024-01-01".to_string())],
            )
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].to_string().contains("minimum")
                && errors[0].to_string().contains("is after maximum")
        );
    }

    #[test]
    fn validate_date_valid_range() {
        let mut specs = TypeSpecification::date();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Minimum,
                &[CommandArg::Label("2024-01-01".to_string())],
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Maximum,
                &[CommandArg::Label("2024-12-31".to_string())],
            )
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_time_minimum_after_maximum() {
        let mut specs = TypeSpecification::time();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Minimum,
                &[CommandArg::Label("23:00:00".to_string())],
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                TypeConstraintCommand::Maximum,
                &[CommandArg::Label("10:00:00".to_string())],
            )
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].to_string().contains("minimum")
                && errors[0].to_string().contains("is after maximum")
        );
    }

    #[test]
    fn validate_type_definition_with_invalid_constraints() {
        // This test now validates that type specification validation works correctly.
        // The actual validation happens during graph building, but we test the validation
        // function directly here.
        use crate::engine::Context;
        use crate::parsing::ast::{LemmaSpec, ParentType, PrimitiveKind, TypeDef};
        use crate::planning::types::PerSliceTypeResolver;
        use std::sync::Arc;

        let spec = Arc::new(LemmaSpec::new("test".to_string()));
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::clone(&spec), false)
            .expect("insert test spec");
        let type_def = TypeDef::Regular {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "invalid_money".to_string(),
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: Some(vec![
                (
                    TypeConstraintCommand::Minimum,
                    vec![CommandArg::Number("100".to_string())],
                ),
                (
                    TypeConstraintCommand::Maximum,
                    vec![CommandArg::Number("50".to_string())],
                ),
            ]),
        };

        let plan_hashes = crate::planning::PlanHashRegistry::default();
        let mut type_resolver = PerSliceTypeResolver::new(&ctx, None, &plan_hashes);
        type_resolver
            .register_type(&spec, type_def)
            .expect("Should register type");
        let resolved_types = type_resolver
            .resolve_named_types(&spec)
            .expect("Should resolve types");

        // Validate the specifications
        let lemma_type = resolved_types
            .named_types
            .get("invalid_money")
            .expect("Should have invalid_money type");
        let src = test_source();
        let errors =
            validate_type_specifications(&lemma_type.specifications, "invalid_money", &src, None);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e
            .to_string()
            .contains("minimum 100 is greater than maximum 50")));
    }

    fn lt(spec: TypeSpecification) -> LemmaType {
        LemmaType::primitive(spec)
    }

    #[test]
    fn interface_requirement_matrix_all_types_base_checks() {
        let bool_t = lt(TypeSpecification::boolean());
        let num_t = lt(TypeSpecification::number());
        let scale_t = lt(TypeSpecification::Scale {
            minimum: None,
            maximum: None,
            decimals: None,
            precision: None,
            units: ScaleUnits::from(vec![ScaleUnit {
                name: "eur".to_string(),
                value: Decimal::ONE,
            }]),
            help: String::new(),
            default: None,
        });
        let ratio_t = lt(TypeSpecification::Ratio {
            minimum: None,
            maximum: None,
            decimals: None,
            units: RatioUnits::from(vec![RatioUnit {
                name: "percent".to_string(),
                value: Decimal::from(100),
            }]),
            help: String::new(),
            default: None,
        });
        let text_t = lt(TypeSpecification::text());
        let date_t = lt(TypeSpecification::date());
        let time_t = lt(TypeSpecification::time());
        let duration_t = lt(TypeSpecification::duration());
        let veto_t = LemmaType::veto_type();
        let undetermined_t = LemmaType::undetermined_type();

        assert!(rule_type_satisfies_requirement(
            &bool_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Boolean)
        ));
        assert!(rule_type_satisfies_requirement(
            &num_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Number)
        ));
        assert!(rule_type_satisfies_requirement(
            &scale_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Scale)
        ));
        assert!(rule_type_satisfies_requirement(
            &ratio_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Ratio)
        ));
        assert!(rule_type_satisfies_requirement(
            &text_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Text)
        ));
        assert!(rule_type_satisfies_requirement(
            &date_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Date)
        ));
        assert!(rule_type_satisfies_requirement(
            &time_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Time)
        ));
        assert!(rule_type_satisfies_requirement(
            &duration_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Duration)
        ));

        assert!(!rule_type_satisfies_requirement(
            &num_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Boolean)
        ));
        assert!(!rule_type_satisfies_requirement(
            &scale_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Number)
        ));
        // veto is control flow, not type incompatibility -- satisfies any requirement
        assert!(rule_type_satisfies_requirement(
            &veto_t,
            &RuleRefRequirement::Base(BaseTypeRequirement::Any)
        ));
        assert!(
            std::panic::catch_unwind(|| {
                rule_type_satisfies_requirement(
                    &undetermined_t,
                    &RuleRefRequirement::Base(BaseTypeRequirement::Any),
                )
            })
            .is_err(),
            "should panic when rule_type_satisfies_requirement is called with undetermined type"
        );
    }

    #[test]
    fn interface_requirement_matrix_unit_family_and_bounds_checks() {
        let money = LemmaType::new(
            "money".to_string(),
            TypeSpecification::Scale {
                minimum: Some(Decimal::ZERO),
                maximum: Some(Decimal::from(100)),
                decimals: None,
                precision: None,
                units: ScaleUnits::from(vec![
                    ScaleUnit {
                        name: "eur".to_string(),
                        value: Decimal::ONE,
                    },
                    ScaleUnit {
                        name: "usd".to_string(),
                        value: Decimal::new(11, 1),
                    },
                ]),
                help: String::new(),
                default: None,
            },
            crate::planning::semantics::TypeExtends::Primitive,
        );
        let weight = LemmaType::new(
            "weight".to_string(),
            TypeSpecification::Scale {
                minimum: None,
                maximum: None,
                decimals: None,
                precision: None,
                units: ScaleUnits::from(vec![ScaleUnit {
                    name: "kg".to_string(),
                    value: Decimal::ONE,
                }]),
                help: String::new(),
                default: None,
            },
            crate::planning::semantics::TypeExtends::Primitive,
        );
        let ratio = lt(TypeSpecification::Ratio {
            minimum: Some(Decimal::ZERO),
            maximum: Some(Decimal::from(100)),
            decimals: None,
            units: RatioUnits::from(vec![RatioUnit {
                name: "percent".to_string(),
                value: Decimal::from(100),
            }]),
            help: String::new(),
            default: None,
        });
        let bounded_number = lt(TypeSpecification::Number {
            minimum: Some(Decimal::ZERO),
            maximum: Some(Decimal::from(100)),
            decimals: None,
            precision: None,
            help: String::new(),
            default: None,
        });

        assert!(rule_type_satisfies_requirement(
            &money,
            &RuleRefRequirement::ScaleMustContainUnit("eur".to_string())
        ));
        assert!(!rule_type_satisfies_requirement(
            &money,
            &RuleRefRequirement::ScaleMustContainUnit("gbp".to_string())
        ));
        assert!(rule_type_satisfies_requirement(
            &ratio,
            &RuleRefRequirement::RatioMustContainUnit("percent".to_string())
        ));
        assert!(!rule_type_satisfies_requirement(
            &ratio,
            &RuleRefRequirement::RatioMustContainUnit("permille".to_string())
        ));
        assert!(rule_type_satisfies_requirement(
            &money,
            &RuleRefRequirement::SameScaleFamilyAs(money.clone())
        ));
        assert!(!rule_type_satisfies_requirement(
            &money,
            &RuleRefRequirement::SameScaleFamilyAs(weight)
        ));

        assert!(rule_type_satisfies_requirement(
            &bounded_number,
            &RuleRefRequirement::NumericLiteral(NumericLiteralConstraint {
                op: ComparisonComputation::GreaterThan,
                literal: Decimal::from(50),
                reference_on_left: true,
            })
        ));
        assert!(!rule_type_satisfies_requirement(
            &bounded_number,
            &RuleRefRequirement::NumericLiteral(NumericLiteralConstraint {
                op: ComparisonComputation::GreaterThan,
                literal: Decimal::from(500),
                reference_on_left: true,
            })
        ));
    }
}
