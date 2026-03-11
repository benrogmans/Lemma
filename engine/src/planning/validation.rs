//! Semantic validation for Lemma specs
//!
//! Validates spec structure and type declarations
//! to catch errors early with clear messages.

use crate::parsing::ast::{DateTimeValue, LemmaSpec, TimeValue};
use crate::planning::semantics::{
    Expression, ExpressionKind, FactPath, LemmaType, RulePath, SemanticConversionTarget,
    TypeSpecification,
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
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate precision is positive if set
            if let Some(prec) = precision {
                if *prec <= Decimal::ZERO {
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid precision: {}. Must be positive",
                            type_name, prec
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate default value constraints
            if let Some((def_value, def_unit)) = default {
                // Validate that the default unit exists
                if !units.iter().any(|u| u.name == *def_unit) {
                    errors.push(Error::validation(
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
                    ));
                }
                if let Some(min) = minimum {
                    if *def_value < *min {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default value {} {} is less than minimum {}",
                                type_name, def_value, def_unit, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def_value > *max {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default value {} {} is greater than maximum {}",
                                type_name, def_value, def_unit, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
            }

            // Scale types must have at least one unit (required for parsing and conversion)
            if units.is_empty() {
                errors.push(Error::validation(
                    format!(
                        "Type '{}' is a scale type but has no units. Scale types must define at least one unit (e.g. -> unit eur 1).",
                        type_name
                    ),
                    Some(source.clone()),
                    None::<String>,
                ));
            }

            // Validate units (if present)
            if !units.is_empty() {
                let mut seen_names: Vec<String> = Vec::new();
                for unit in units.iter() {
                    // Validate unit name is not empty
                    if unit.name.trim().is_empty() {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' has a unit with empty name. Unit names cannot be empty.",
                                type_name
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }

                    // Validate unit names are unique within the type (case-insensitive)
                    let lower_name = unit.name.to_lowercase();
                    if seen_names
                        .iter()
                        .any(|seen| seen.to_lowercase() == lower_name)
                    {
                        errors.push(Error::validation(
                            format!("Type '{}' has duplicate unit name '{}' (case-insensitive). Unit names must be unique within a type.", type_name, unit.name),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    // Validate unit values are positive (conversion factors relative to type base of 1)
                    if unit.value <= Decimal::ZERO {
                        errors.push(Error::validation(
                            format!("Type '{}' has unit '{}' with invalid value {}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value),
                            Some(source.clone()),
                            None::<String>,
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
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate precision is positive if set
            if let Some(prec) = precision {
                if *prec <= Decimal::ZERO {
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid precision: {}. Must be positive",
                            type_name, prec
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
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
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
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
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' has a unit with empty name. Unit names cannot be empty.",
                                type_name
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }

                    // Validate unit names are unique within the type (case-insensitive)
                    let lower_name = unit.name.to_lowercase();
                    if seen_names
                        .iter()
                        .any(|seen| seen.to_lowercase() == lower_name)
                    {
                        errors.push(Error::validation(
                            format!("Type '{}' has duplicate unit name '{}' (case-insensitive). Unit names must be unique within a type.", type_name, unit.name),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    // Validate unit values are positive (conversion factors relative to type base of 1)
                    if unit.value <= Decimal::ZERO {
                        errors.push(Error::validation(
                            format!("Type '{}' has unit '{}' with invalid value {}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value),
                            Some(source.clone()),
                            None::<String>,
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
                    errors.push(Error::validation(
                        format!("Type '{}' has invalid range: minimum length {} is greater than maximum length {}", type_name, min, max),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate length consistency
            if let Some(len) = length {
                if let Some(min) = minimum {
                    if *len < *min {
                        errors.push(Error::validation(
                            format!("Type '{}' has inconsistent length constraint: length {} is less than minimum {}", type_name, len, min),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *len > *max {
                        errors.push(Error::validation(
                            format!("Type '{}' has inconsistent length constraint: length {} is greater than maximum {}", type_name, len, max),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                let def_len = def.len();

                if let Some(min) = minimum {
                    if def_len < *min {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default value length {} is less than minimum {}",
                                type_name, def_len, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if def_len > *max {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default value length {} is greater than maximum {}",
                                type_name, def_len, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if let Some(len) = length {
                    if def_len != *len {
                        errors.push(Error::validation(
                            format!("Type '{}' default value length {} does not match required length {}", type_name, def_len, len),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if !options.is_empty() && !options.contains(def) {
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' default value '{}' is not in allowed options: {:?}",
                            type_name, def, options
                        ),
                        Some(source.clone()),
                        None::<String>,
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
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid date range: minimum {} is after maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if compare_date_values(def, min) == Ordering::Less {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default date {} is before minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if compare_date_values(def, max) == Ordering::Greater {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default date {} is after maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
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
                    errors.push(Error::validation(
                        format!(
                            "Type '{}' has invalid time range: minimum {} is after maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if compare_time_values(def, min) == Ordering::Less {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default time {} is before minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if compare_time_values(def, max) == Ordering::Greater {
                        errors.push(Error::validation(
                            format!(
                                "Type '{}' default time {} is after maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
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

/// Rule data needed to validate spec interfaces (avoids validation depending on graph).
pub struct RuleEntryForBindingCheck {
    pub rule_type: LemmaType,
    pub depends_on_rules: std::collections::BTreeSet<RulePath>,
    pub branches: Vec<(Option<Expression>, Expression)>,
}

/// Expected type constraint at a rule reference use site (from parent expression).
#[derive(Clone, Copy, Debug)]
enum ExpectedRuleTypeConstraint {
    Numeric,
    Boolean,
    Comparable,
    Number,
    Duration,
    Ratio,
    Scale,
    Any,
}

/// Map a rule's result type to the strictest ExpectedRuleTypeConstraint it satisfies,
/// for spec interface type checking.
fn lemma_type_to_expected_constraint(lemma_type: &LemmaType) -> ExpectedRuleTypeConstraint {
    if lemma_type.is_boolean() {
        return ExpectedRuleTypeConstraint::Boolean;
    }
    if lemma_type.is_number() {
        return ExpectedRuleTypeConstraint::Number;
    }
    if lemma_type.is_scale() {
        return ExpectedRuleTypeConstraint::Scale;
    }
    if lemma_type.is_duration() {
        return ExpectedRuleTypeConstraint::Duration;
    }
    if lemma_type.is_ratio() {
        return ExpectedRuleTypeConstraint::Ratio;
    }
    if lemma_type.is_text() || lemma_type.is_date() || lemma_type.is_time() {
        return ExpectedRuleTypeConstraint::Comparable;
    }
    ExpectedRuleTypeConstraint::Any
}

fn rule_type_satisfies_constraint(
    lemma_type: &LemmaType,
    constraint: ExpectedRuleTypeConstraint,
) -> bool {
    match constraint {
        ExpectedRuleTypeConstraint::Any => true,
        ExpectedRuleTypeConstraint::Boolean => lemma_type.is_boolean(),
        ExpectedRuleTypeConstraint::Number => lemma_type.is_number(),
        ExpectedRuleTypeConstraint::Duration => lemma_type.is_duration(),
        ExpectedRuleTypeConstraint::Ratio => lemma_type.is_ratio(),
        ExpectedRuleTypeConstraint::Scale => lemma_type.is_scale(),
        ExpectedRuleTypeConstraint::Numeric => {
            lemma_type.is_number() || lemma_type.is_scale() || lemma_type.is_ratio()
        }
        ExpectedRuleTypeConstraint::Comparable => {
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

fn collect_expected_constraints_for_rule_ref(
    expr: &Expression,
    rule_path: &RulePath,
    expected: ExpectedRuleTypeConstraint,
) -> Vec<(Option<Source>, ExpectedRuleTypeConstraint)> {
    let mut out = Vec::new();
    match &expr.kind {
        ExpressionKind::RulePath(rp) => {
            if rp == rule_path {
                out.push((expr.source_location.clone(), expected));
            }
        }
        ExpressionKind::LogicalAnd(left, right) => {
            out.extend(collect_expected_constraints_for_rule_ref(
                left,
                rule_path,
                ExpectedRuleTypeConstraint::Boolean,
            ));
            out.extend(collect_expected_constraints_for_rule_ref(
                right,
                rule_path,
                ExpectedRuleTypeConstraint::Boolean,
            ));
        }
        ExpressionKind::LogicalNegation(operand, _) => {
            out.extend(collect_expected_constraints_for_rule_ref(
                operand,
                rule_path,
                ExpectedRuleTypeConstraint::Boolean,
            ));
        }
        ExpressionKind::Comparison(left, _, right) => {
            out.extend(collect_expected_constraints_for_rule_ref(
                left,
                rule_path,
                ExpectedRuleTypeConstraint::Comparable,
            ));
            out.extend(collect_expected_constraints_for_rule_ref(
                right,
                rule_path,
                ExpectedRuleTypeConstraint::Comparable,
            ));
        }
        ExpressionKind::Arithmetic(left, _, right) => {
            out.extend(collect_expected_constraints_for_rule_ref(
                left,
                rule_path,
                ExpectedRuleTypeConstraint::Numeric,
            ));
            out.extend(collect_expected_constraints_for_rule_ref(
                right,
                rule_path,
                ExpectedRuleTypeConstraint::Numeric,
            ));
        }
        ExpressionKind::UnitConversion(source, target) => {
            let constraint = match target {
                SemanticConversionTarget::Duration(_) => ExpectedRuleTypeConstraint::Duration,
                SemanticConversionTarget::ScaleUnit(_) => ExpectedRuleTypeConstraint::Scale,
                SemanticConversionTarget::RatioUnit(_) => ExpectedRuleTypeConstraint::Ratio,
            };
            out.extend(collect_expected_constraints_for_rule_ref(
                source, rule_path, constraint,
            ));
        }
        ExpressionKind::MathematicalComputation(_, operand) => {
            out.extend(collect_expected_constraints_for_rule_ref(
                operand,
                rule_path,
                ExpectedRuleTypeConstraint::Number,
            ));
        }
        ExpressionKind::DateRelative(_, date_expr, tolerance) => {
            out.extend(collect_expected_constraints_for_rule_ref(
                date_expr,
                rule_path,
                ExpectedRuleTypeConstraint::Comparable,
            ));
            if let Some(tol) = tolerance {
                out.extend(collect_expected_constraints_for_rule_ref(
                    tol,
                    rule_path,
                    ExpectedRuleTypeConstraint::Duration,
                ));
            }
        }
        ExpressionKind::DateCalendar(_, _, date_expr) => {
            out.extend(collect_expected_constraints_for_rule_ref(
                date_expr,
                rule_path,
                ExpectedRuleTypeConstraint::Comparable,
            ));
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::FactPath(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Now => {}
    }
    out
}

fn expected_constraint_name(c: ExpectedRuleTypeConstraint) -> &'static str {
    match c {
        ExpectedRuleTypeConstraint::Numeric => "numeric (number, scale, or ratio)",
        ExpectedRuleTypeConstraint::Boolean => "boolean",
        ExpectedRuleTypeConstraint::Comparable => "comparable",
        ExpectedRuleTypeConstraint::Number => "number",
        ExpectedRuleTypeConstraint::Duration => "duration",
        ExpectedRuleTypeConstraint::Ratio => "ratio",
        ExpectedRuleTypeConstraint::Scale => "scale",
        ExpectedRuleTypeConstraint::Any => "any",
    }
}

fn spec_interface_error(
    source: &Source,
    message: impl Into<String>,
    related_spec: Option<Arc<LemmaSpec>>,
) -> Error {
    Error::validation_with_context(
        message.into(),
        Some(source.clone()),
        None::<String>,
        related_spec,
    )
}

/// Validate that every spec-ref fact path's referenced spec has the required rules
/// and that each such rule's result type satisfies what the referencing rules expect.
/// Type errors are reported at the binding fact's source when a binding changed the spec ref.
pub fn validate_spec_interfaces(
    referenced_rules: &HashMap<Vec<String>, HashSet<String>>,
    spec_ref_facts: &[(FactPath, Arc<LemmaSpec>, Source)],
    rule_entries: &IndexMap<RulePath, RuleEntryForBindingCheck>,
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
                    Some(Arc::clone(spec_arc)),
                ));
                continue;
            }

            let ref_rule_path = RulePath::new(fact_path.segments.clone(), required_rule.clone());
            let Some(ref_entry) = rule_entries.get(&ref_rule_path) else {
                continue;
            };
            let ref_rule_type = &ref_entry.rule_type;

            for (_referencing_path, entry) in rule_entries {
                if !entry.depends_on_rules.contains(&ref_rule_path) {
                    continue;
                }
                let expected = lemma_type_to_expected_constraint(&entry.rule_type);
                for (_condition, result_expr) in &entry.branches {
                    let constraints = collect_expected_constraints_for_rule_ref(
                        result_expr,
                        &ref_rule_path,
                        expected,
                    );
                    for (_source, constraint) in constraints {
                        if !rule_type_satisfies_constraint(ref_rule_type, constraint) {
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
                                    expected_constraint_name(constraint),
                                ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::CommandArg;
    use crate::planning::semantics::TypeSpecification;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    fn test_source() -> Source {
        Source::new(
            "<test>",
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            Arc::from("spec test\nfact x: 1"),
        )
    }

    #[test]
    fn validate_number_minimum_greater_than_maximum() {
        let mut specs = TypeSpecification::number();
        specs = specs
            .apply_constraint("minimum", &[CommandArg::Number("100".to_string())])
            .unwrap();
        specs = specs
            .apply_constraint("maximum", &[CommandArg::Number("50".to_string())])
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum 100 is greater than maximum 50"));
    }

    #[test]
    fn validate_number_valid_range() {
        let mut specs = TypeSpecification::number();
        specs = specs
            .apply_constraint("minimum", &[CommandArg::Number("0".to_string())])
            .unwrap();
        specs = specs
            .apply_constraint("maximum", &[CommandArg::Number("100".to_string())])
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src);
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
        let errors = validate_type_specifications(&specs, "test", &src);
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
        let errors = validate_type_specifications(&specs, "test", &src);
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
        let errors = validate_type_specifications(&specs, "test", &src);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_text_minimum_greater_than_maximum() {
        let mut specs = TypeSpecification::text();
        specs = specs
            .apply_constraint("minimum", &[CommandArg::Number("100".to_string())])
            .unwrap();
        specs = specs
            .apply_constraint("maximum", &[CommandArg::Number("50".to_string())])
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum length 100 is greater than maximum length 50"));
    }

    #[test]
    fn validate_text_length_inconsistent_with_minimum() {
        let mut specs = TypeSpecification::text();
        specs = specs
            .apply_constraint("minimum", &[CommandArg::Number("10".to_string())])
            .unwrap();
        specs = specs
            .apply_constraint("length", &[CommandArg::Number("5".to_string())])
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src);
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
        let errors = validate_type_specifications(&specs, "test", &src);
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
        let errors = validate_type_specifications(&specs, "test", &src);
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
        let errors = validate_type_specifications(&specs, "test", &src);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum 2 is greater than maximum 1"));
    }

    #[test]
    fn validate_date_minimum_after_maximum() {
        let mut specs = TypeSpecification::date();
        specs = specs
            .apply_constraint("minimum", &[CommandArg::Label("2024-12-31".to_string())])
            .unwrap();
        specs = specs
            .apply_constraint("maximum", &[CommandArg::Label("2024-01-01".to_string())])
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src);
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
            .apply_constraint("minimum", &[CommandArg::Label("2024-01-01".to_string())])
            .unwrap();
        specs = specs
            .apply_constraint("maximum", &[CommandArg::Label("2024-12-31".to_string())])
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_time_minimum_after_maximum() {
        let mut specs = TypeSpecification::time();
        specs = specs
            .apply_constraint("minimum", &[CommandArg::Label("23:00:00".to_string())])
            .unwrap();
        specs = specs
            .apply_constraint("maximum", &[CommandArg::Label("10:00:00".to_string())])
            .unwrap();

        let src = test_source();
        let errors = validate_type_specifications(&specs, "test", &src);
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
        use crate::parsing::ast::{LemmaSpec, TypeDef};
        use crate::planning::types::TypeResolver;
        use std::sync::Arc;

        let spec = Arc::new(LemmaSpec::new("test".to_string()));
        let type_def = TypeDef::Regular {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                Arc::from("spec test\nfact x: 1"),
            ),
            name: "invalid_money".to_string(),
            parent: "number".to_string(),
            constraints: Some(vec![
                (
                    "minimum".to_string(),
                    vec![CommandArg::Number("100".to_string())],
                ),
                (
                    "maximum".to_string(),
                    vec![CommandArg::Number("50".to_string())],
                ),
            ]),
        };

        // Register and resolve the type to get its specifications
        let mut sources = HashMap::new();
        sources.insert("<test>".to_string(), String::new());
        let mut type_resolver = TypeResolver::new();
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
            validate_type_specifications(&lemma_type.specifications, "invalid_money", &src);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e
            .to_string()
            .contains("minimum 100 is greater than maximum 50")));
    }
}
