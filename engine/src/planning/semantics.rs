//! Resolved semantic types for Lemma
//!
//! This module contains all types that represent resolved semantics after planning.
//! These types are created during the planning phase and used by evaluation, inversion, etc.

// Re-exported parsing types: downstream modules (evaluation, inversion, computation,
// serialization) import these from `planning::semantics`, never from `parsing` directly.
pub use crate::parsing::ast::{
    ArithmeticComputation, ComparisonComputation, MathematicalComputation, NegationType, Span,
    VetoExpression,
};
pub use crate::parsing::source::Source;

/// Logical computation operators (defined in semantics, not used by the parser).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicalComputation {
    And,
    Or,
    Not,
}

/// Returns the logical negation of a comparison (for displaying conditions as true in explanations).
#[must_use]
pub fn negated_comparison(op: ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThanOrEqual,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThan,
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThanOrEqual,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThan,
        ComparisonComputation::Is => ComparisonComputation::IsNot,
        ComparisonComputation::IsNot => ComparisonComputation::Is,
    }
}

// Internal-only parsing imports (used only within this module for value/type resolution).
use crate::computation::rational::RationalInteger;
use crate::parsing::ast::Constraint;
use crate::parsing::ast::{
    BooleanValue, CalendarPeriodUnit, CalendarUnit, CommandArg, ConversionTarget, DateCalendarKind,
    DateRelativeKind, DateTimeValue, LemmaSpec, PrimitiveKind, TimeValue, TypeConstraintCommand,
};
use crate::Error;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

// -----------------------------------------------------------------------------
// Type specification and units (resolved type shape; apply constraints is planning)
// -----------------------------------------------------------------------------

// Unit tables live in `crate::literals` (no dependency on parsing/ast). Re-exported
// here so downstream modules importing from `planning::semantics` keep working.
pub use crate::literals::{BaseQuantityVector, QuantityUnit, QuantityUnits, RatioUnit, RatioUnits};

/// Combine two `BaseQuantityVector`s by adding (for multiply) or subtracting (for divide) exponents.
/// Entries that reach zero exponent are removed (they cancel out).
pub fn combine_decompositions(
    left: &BaseQuantityVector,
    right: &BaseQuantityVector,
    is_multiply: bool,
) -> BaseQuantityVector {
    let mut result = left.clone();
    for (dim, &exp) in right {
        let delta = if is_multiply { exp } else { -exp };
        let entry = result.entry(dim.clone()).or_insert(0);
        *entry += delta;
        if *entry == 0 {
            result.remove(dim);
        }
    }
    result
}

pub const DURATION_DIMENSION: &str = "duration";
pub const CALENDAR_DIMENSION: &str = "calendar";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityTrait {
    Duration,
}

pub fn duration_decomposition() -> BaseQuantityVector {
    [(DURATION_DIMENSION.to_string(), 1i32)]
        .into_iter()
        .collect()
}

pub fn calendar_decomposition() -> BaseQuantityVector {
    [(CALENDAR_DIMENSION.to_string(), 1i32)]
        .into_iter()
        .collect()
}

mod stored_quantity_declared_bound_serde {
    use super::RationalInteger;
    use crate::computation::rational::commit_rational_to_decimal;
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    fn lift(decimal: Decimal) -> Result<RationalInteger, String> {
        crate::computation::rational::decimal_to_rational(decimal)
            .map_err(|failure| failure.to_string())
    }

    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(
            value: &Option<(RationalInteger, String)>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match value {
                None => serializer.serialize_none(),
                Some((magnitude, unit_name)) => {
                    let decimal =
                        commit_rational_to_decimal(magnitude).map_err(serde::ser::Error::custom)?;
                    (decimal, unit_name.as_str()).serialize(serializer)
                }
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<(RationalInteger, String)>, D::Error> {
            let parsed: Option<(Decimal, String)> = Option::deserialize(deserializer)?;
            parsed
                .map(|(decimal, unit_name)| lift(decimal).map(|magnitude| (magnitude, unit_name)))
                .transpose()
                .map_err(serde::de::Error::custom)
        }
    }
}

mod stored_calendar_bound_serde {
    use super::{RationalInteger, SemanticCalendarUnit};
    use crate::computation::rational::commit_rational_to_decimal;
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    fn lift(decimal: Decimal) -> Result<RationalInteger, String> {
        crate::computation::rational::decimal_to_rational(decimal)
            .map_err(|failure| failure.to_string())
    }

    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(
            value: &Option<(RationalInteger, SemanticCalendarUnit)>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match value {
                None => serializer.serialize_none(),
                Some((magnitude, unit)) => {
                    let decimal =
                        commit_rational_to_decimal(magnitude).map_err(serde::ser::Error::custom)?;
                    (decimal, unit).serialize(serializer)
                }
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<(RationalInteger, SemanticCalendarUnit)>, D::Error> {
            let parsed: Option<(Decimal, SemanticCalendarUnit)> =
                Option::deserialize(deserializer)?;
            parsed
                .map(|(decimal, unit)| lift(decimal).map(|magnitude| (magnitude, unit)))
                .transpose()
                .map_err(serde::de::Error::custom)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TypeSpecification {
    Boolean {
        help: String,
    },
    Quantity {
        #[serde(with = "stored_quantity_declared_bound_serde::option", default)]
        minimum: Option<(RationalInteger, String)>,
        #[serde(with = "stored_quantity_declared_bound_serde::option", default)]
        maximum: Option<(RationalInteger, String)>,
        decimals: Option<u8>,
        units: QuantityUnits,
        #[serde(default)]
        traits: Vec<QuantityTrait>,
        /// Common dimensional decomposition vector shared by all units in this quantity.
        /// Empty until the decomposition pass runs. Base quantities (no compound unit expression)
        /// are assigned `{quantity_name: 1}` by the pass.
        #[serde(default)]
        decomposition: BaseQuantityVector,
        /// Name of the canonical unit (the one with `value == 1`). Empty until the
        /// decomposition pass validates and assigns it. Must be exactly one such unit.
        #[serde(default)]
        canonical_unit: String,
        help: String,
    },
    Number {
        #[serde(with = "crate::literals::stored_rational_serde::option", default)]
        minimum: Option<RationalInteger>,
        #[serde(with = "crate::literals::stored_rational_serde::option", default)]
        maximum: Option<RationalInteger>,
        decimals: Option<u8>,
        help: String,
    },
    NumberRange {
        help: String,
    },
    Ratio {
        #[serde(with = "crate::literals::stored_rational_serde::option", default)]
        minimum: Option<RationalInteger>,
        #[serde(with = "crate::literals::stored_rational_serde::option", default)]
        maximum: Option<RationalInteger>,
        decimals: Option<u8>,
        units: RatioUnits,
        help: String,
    },
    RatioRange {
        units: RatioUnits,
        help: String,
    },
    Text {
        length: Option<usize>,
        options: Vec<String>,
        help: String,
    },
    Date {
        minimum: Option<DateTimeValue>,
        maximum: Option<DateTimeValue>,
        help: String,
    },
    DateRange {
        help: String,
    },
    Time {
        minimum: Option<TimeValue>,
        maximum: Option<TimeValue>,
        help: String,
    },
    Calendar {
        #[serde(with = "stored_calendar_bound_serde::option", default)]
        minimum: Option<(RationalInteger, SemanticCalendarUnit)>,
        #[serde(with = "stored_calendar_bound_serde::option", default)]
        maximum: Option<(RationalInteger, SemanticCalendarUnit)>,
        help: String,
    },
    CalendarRange {
        help: String,
    },
    QuantityRange {
        units: QuantityUnits,
        #[serde(default)]
        decomposition: BaseQuantityVector,
        #[serde(default)]
        canonical_unit: String,
        help: String,
    },
    Veto {
        message: Option<String>,
    },
    /// Sentinel used during type inference when the type could not be determined.
    /// Propagates through expressions without generating cascading errors.
    /// Must never appear in a successfully validated graph or execution plan.
    Undetermined,
}

/// Extract a typed [`Value`] from the first `CommandArg`, requiring `Literal` shape.
///
/// `Label` args carry identifiers (unit names, option keywords) and never satisfy a
/// command position that wants a literal value. Returning a typed `Value` keeps the
/// caller's match exhaustive over [`Value`] variants — no string coercion path.
fn require_literal<'a>(
    args: &'a [CommandArg],
    cmd: &str,
) -> Result<&'a crate::literals::Value, String> {
    let arg = args
        .first()
        .ok_or_else(|| format!("{} requires an argument", cmd))?;
    match arg {
        CommandArg::Literal(v) => Ok(v),
        CommandArg::Label(name) => Err(format!(
            "{} requires a literal value, got identifier '{}'",
            cmd, name
        )),
        CommandArg::UnitExpr(_) => Err(format!(
            "{} requires a literal value, got a unit expression (only valid for 'unit' command)",
            cmd
        )),
    }
}

fn apply_type_help_command(help: &mut String, args: &[CommandArg]) -> Result<(), String> {
    match require_literal(args, "help")? {
        crate::literals::Value::Text(s) => {
            *help = s.clone();
            Ok(())
        }
        other => Err(format!(
            "help requires a text literal (quoted string), got {}",
            value_kind_name(other)
        )),
    }
}

fn calendar_unit_singular_label(unit: &crate::literals::CalendarUnit) -> &'static str {
    match unit {
        crate::literals::CalendarUnit::Month => "month",
        crate::literals::CalendarUnit::Year => "year",
    }
}

fn format_quantity_units_list(units: &QuantityUnits) -> String {
    units
        .iter()
        .map(|u| u.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What kind of value `-> default` expects when rejecting a calendar literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefaultExpectation {
    QuantityUnits,
    Text,
    Number,
    Boolean,
    Date,
    Time,
    Ratio,
    NumberRange,
    DateRange,
    QuantityRange,
    RatioRange,
    CalendarRange,
}

pub(crate) fn default_value_mismatch_error(
    calendar_unit: &crate::literals::CalendarUnit,
    type_name: &str,
    expectation: DefaultExpectation,
    quantity_units: Option<&QuantityUnits>,
) -> String {
    let unit_label = calendar_unit_singular_label(calendar_unit);
    let first = format!("Unit '{unit_label}' is for calendar data.");
    match expectation {
        DefaultExpectation::QuantityUnits => {
            let list = quantity_units
                .map(format_quantity_units_list)
                .unwrap_or_default();
            format!("{first} Valid '{type_name}' units are: {list}.")
        }
        DefaultExpectation::Text => format!(
            "{first} Please provide a text value in double quotes, for example `-> default \"my default value\"`."
        ),
        DefaultExpectation::Number => format!(
            "{first} Please provide a number, for example `-> default 42`."
        ),
        DefaultExpectation::Boolean => format!(
            "{first} Please provide true or false, for example `-> default true`."
        ),
        DefaultExpectation::Date => format!(
            "{first} Please provide a date, for example `-> default 2024-06-15`."
        ),
        DefaultExpectation::Time => format!(
            "{first} Please provide a time, for example `-> default 09:00:00`."
        ),
        DefaultExpectation::Ratio | DefaultExpectation::RatioRange => format!(
            "{first} Please provide a ratio, for example `-> default 25%`."
        ),
        DefaultExpectation::NumberRange => format!(
            "{first} Please provide a number range, for example `-> default 10...100`."
        ),
        DefaultExpectation::DateRange => format!(
            "{first} Please provide a date range, for example `-> default 2024-01-01...2024-12-31`."
        ),
        DefaultExpectation::QuantityRange => format!(
            "{first} Please provide a range with units valid for '{type_name}', for example `-> default 30 kilogram...35 kilogram`."
        ),
        DefaultExpectation::CalendarRange => format!(
            "{first} Please provide a calendar range, for example `-> default 18 years...67 years`."
        ),
    }
}

fn quantity_default_unit_error(unit: &str, type_name: &str, units: &QuantityUnits) -> String {
    format!(
        "Unit '{unit}' is not defined on '{type_name}'. Valid '{type_name}' units are: {}.",
        format_quantity_units_list(units)
    )
}

fn quantity_default_wrong_shape_error(type_name: &str, traits: &[QuantityTrait]) -> String {
    let example = if traits.contains(&QuantityTrait::Duration) {
        "4 weeks"
    } else {
        "30 kilogram"
    };
    format!(
        "Please provide a value with a unit valid for '{type_name}', for example `-> default {example}`."
    )
}

fn validate_quantity_default_literal(
    args: &[CommandArg],
    type_name: &str,
    units: &QuantityUnits,
    traits: &[QuantityTrait],
) -> Result<ValueKind, String> {
    let (magnitude, unit_name) = match args {
        [CommandArg::Literal(crate::literals::Value::NumberWithUnit(m, u))] => (*m, u.as_str()),
        _ => return Err(quantity_default_wrong_shape_error(type_name, traits)),
    };
    if units.get(unit_name).is_err() {
        return Err(quantity_default_unit_error(unit_name, type_name, units));
    }
    Ok(ValueKind::Quantity(
        lift_parser_decimal(magnitude)?,
        unit_name.to_string(),
        BaseQuantityVector::new(),
    ))
}

fn reject_calendar_for_default(
    value: &crate::literals::Value,
    type_name: &str,
    expectation: DefaultExpectation,
    quantity_units: Option<&QuantityUnits>,
) -> Result<(), String> {
    if let crate::literals::Value::Calendar(_, unit) = value {
        return Err(default_value_mismatch_error(
            unit,
            type_name,
            expectation,
            quantity_units,
        ));
    }
    Ok(())
}

/// Human-readable name for a [`Value`] variant — used in mismatch error messages.
fn value_kind_name(v: &crate::literals::Value) -> &'static str {
    use crate::literals::Value;
    match v {
        Value::Number(_) => "number",
        Value::NumberWithUnit(_, _) => "number_with_unit",
        Value::Text(_) => "text",
        Value::Date(_) => "date",
        Value::Time(_) => "time",
        Value::Boolean(_) => "boolean",
        Value::Calendar(_, _) => "calendar",
        Value::Range(_, _) => "range",
    }
}

fn require_default_range_endpoints<'a>(
    args: &'a [CommandArg],
    type_name: &str,
    expectation: DefaultExpectation,
    quantity_units: Option<&QuantityUnits>,
) -> Result<(&'a crate::literals::Value, &'a crate::literals::Value), String> {
    match require_literal(args, "default")? {
        crate::literals::Value::Calendar(_, unit) => Err(default_value_mismatch_error(
            unit,
            type_name,
            expectation,
            quantity_units,
        )),
        crate::literals::Value::Range(left, right) => Ok((left.as_ref(), right.as_ref())),
        _ => Err(match expectation {
            DefaultExpectation::NumberRange => {
                "Please provide a number range, for example `-> default 10...100`.".to_string()
            }
            DefaultExpectation::DateRange => {
                "Please provide a date range, for example `-> default 2024-01-01...2024-12-31`."
                    .to_string()
            }
            DefaultExpectation::RatioRange => {
                "Please provide a ratio range, for example `-> default 10%...50%`.".to_string()
            }
            DefaultExpectation::QuantityRange => format!(
                "Please provide a range with units valid for '{type_name}', for example `-> default 30 kilogram...35 kilogram`."
            ),
            DefaultExpectation::CalendarRange => {
                "Please provide a calendar range, for example `-> default 18 years...67 years`."
                    .to_string()
            }
            _ => unreachable!("BUG: require_default_range_endpoints called with non-range expectation"),
        }),
    }
}

fn lift_parser_decimal(decimal: rust_decimal::Decimal) -> Result<RationalInteger, String> {
    crate::computation::rational::decimal_to_rational(decimal)
        .map_err(|failure| format!("literal failed rational lift: {failure}"))
}

/// Element spec for a range type, used for parsing endpoints and lifting literal endpoints.
/// Returns the per-endpoint primitive spec (e.g. `RatioRange { units }` -> `Ratio { units }`).
pub fn range_element_type_specification(
    range_spec: &TypeSpecification,
) -> Option<TypeSpecification> {
    match range_spec {
        TypeSpecification::NumberRange { .. } => Some(TypeSpecification::number()),
        TypeSpecification::QuantityRange {
            units,
            decomposition,
            canonical_unit,
            ..
        } => Some(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units: units.clone(),
            traits: Vec::new(),
            decomposition: decomposition.clone(),
            canonical_unit: canonical_unit.clone(),
            help: String::new(),
        }),
        TypeSpecification::DateRange { .. } => Some(TypeSpecification::date()),
        TypeSpecification::CalendarRange { .. } => Some(TypeSpecification::calendar()),
        TypeSpecification::RatioRange { units, .. } => Some(TypeSpecification::Ratio {
            minimum: None,
            maximum: None,
            decimals: None,
            units: units.clone(),
            help: String::new(),
        }),
        _ => None,
    }
}

/// Lift a parser literal range endpoint to a [`LiteralValue`] with the element's primitive type.
/// Routes [`Value::NumberWithUnit`] through [`parser_value_to_value_kind`] so ratio endpoints
/// (e.g. `10%` in a `ratio range`) canonicalize to ratios, not anonymous quantities.
fn lift_range_endpoint(
    value: &crate::parsing::ast::Value,
    element_spec: &TypeSpecification,
) -> Result<LiteralValue, String> {
    use crate::parsing::ast::Value;
    let kind = match value {
        Value::NumberWithUnit(_, _) => parser_value_to_value_kind(value, element_spec)?,
        _ => value_to_semantic(value)?,
    };
    Ok(LiteralValue {
        value: kind,
        lemma_type: LemmaType::primitive(element_spec.clone()),
    })
}

fn literal_value_from_parser_value(
    value: &crate::parsing::ast::Value,
) -> Result<LiteralValue, String> {
    use crate::parsing::ast::Value;

    match value {
        Value::Number(n) => Ok(LiteralValue::number(lift_parser_decimal(*n)?)),
        Value::NumberWithUnit(n, unit) => Ok(LiteralValue::number_interpreted_as_quantity(
            lift_parser_decimal(*n)?,
            unit.clone(),
        )),
        Value::Text(s) => Ok(LiteralValue::text(s.clone())),
        Value::Date(dt) => Ok(LiteralValue::date(date_time_to_semantic(dt))),
        Value::Time(t) => Ok(LiteralValue::time(time_to_semantic(t))),
        Value::Boolean(b) => Ok(LiteralValue::from_bool(bool::from(*b))),
        Value::Calendar(n, unit) => Ok(LiteralValue::calendar(
            lift_parser_decimal(*n)?,
            calendar_unit_to_semantic(unit),
        )),
        Value::Range(left, right) => {
            let left = literal_value_from_parser_value(left)?;
            let right = literal_value_from_parser_value(right)?;
            let compatible = match (
                &left.lemma_type.specifications,
                &right.lemma_type.specifications,
            ) {
                (TypeSpecification::Date { .. }, TypeSpecification::Date { .. }) => true,
                (TypeSpecification::Number { .. }, TypeSpecification::Number { .. }) => true,
                (TypeSpecification::Quantity { .. }, TypeSpecification::Quantity { .. }) => {
                    left.lemma_type.same_quantity_family(&right.lemma_type)
                        || left
                            .lemma_type
                            .compatible_with_anonymous_quantity(&right.lemma_type)
                        || right
                            .lemma_type
                            .compatible_with_anonymous_quantity(&left.lemma_type)
                }
                (TypeSpecification::Ratio { .. }, TypeSpecification::Ratio { .. }) => true,
                (TypeSpecification::Calendar { .. }, TypeSpecification::Calendar { .. }) => true,
                _ => false,
            };
            if !compatible {
                return Err(format!(
                    "range endpoints must have the same supported base type, got {} and {}",
                    left.lemma_type.name(),
                    right.lemma_type.name()
                ));
            }
            Ok(LiteralValue::range(left, right))
        }
    }
}

/// Cast a [`RationalInteger`] to `u8`, requiring it to be a non-negative whole number that fits.
fn decimal_to_u8(d: RationalInteger, ctx: &str) -> Result<u8, String> {
    if *d.denom() != 1 {
        return Err(format!(
            "{} requires a whole number, got fractional value",
            ctx
        ));
    }
    u8::try_from(*d.numer()).map_err(|_| format!("{} value out of range for u8", ctx))
}

/// Cast a [`RationalInteger`] to `usize`, requiring it to be a non-negative whole number that fits.
fn decimal_to_usize(d: RationalInteger, ctx: &str) -> Result<usize, String> {
    if *d.denom() != 1 {
        return Err(format!(
            "{} requires a whole number, got fractional value",
            ctx
        ));
    }
    usize::try_from(*d.numer()).map_err(|_| format!("{} value out of range for usize", ctx))
}

/// Extract a number literal from a [`Value::Number`] arg and lift it to [`RationalInteger`].
///
/// Numeric meta-constraints (`decimals`, `length`, `minimum`/`maximum`
/// on `Number` and `Quantity`) take a bare number literal — not a ratio, not a quantity. Reject
/// any other variant to honour the no-coercion contract.
fn ratio_bound_to_canonical_rational(
    args: &[CommandArg],
    cmd: &str,
    units: &RatioUnits,
) -> Result<RationalInteger, String> {
    use crate::computation::rational::{checked_div, decimal_to_rational};
    let lit = require_literal(args, cmd)?;
    match lit {
        crate::literals::Value::NumberWithUnit(magnitude, unit_name) => {
            let unit = units.get(unit_name.as_str())?;
            let magnitude_rational = decimal_to_rational(*magnitude)
                .map_err(|failure| format!("{cmd} literal failed rational lift: {failure}"))?;
            checked_div(&magnitude_rational, &unit.value)
                .map_err(|failure| format!("{cmd}: unit conversion failed: {failure}"))
        }
        other => Err(format!(
            "{cmd} requires a ratio literal with a unit, got {}",
            value_kind_name(other)
        )),
    }
}

fn require_decimal_literal(args: &[CommandArg], cmd: &str) -> Result<RationalInteger, String> {
    use crate::computation::rational::decimal_to_rational;
    match require_literal(args, cmd)? {
        crate::literals::Value::Number(d) => decimal_to_rational(*d)
            .map_err(|failure| format!("{} literal failed rational lift: {}", cmd, failure)),
        other => Err(format!(
            "{} requires a number literal, got {}",
            cmd,
            value_kind_name(other)
        )),
    }
}

enum UnitConstraintField {
    Minimum,
    Maximum,
    DefaultMagnitude,
}

fn quantity_declared_bound_to_canonical(
    magnitude: &RationalInteger,
    unit_name: &str,
    units: &QuantityUnits,
    type_name: &str,
    command: &str,
) -> Result<RationalInteger, String> {
    use crate::computation::rational::checked_mul;
    let unit = units.get(unit_name).map_err(|_| {
        format!(
            "Unit '{unit_name}' is not defined on '{type_name}'. Valid units are: {}.",
            format_quantity_units_list(units)
        )
    })?;
    checked_mul(magnitude, &unit.factor)
        .map_err(|failure| format!("{command}: unit conversion overflow: {failure}"))
}

fn parse_quantity_declared_bound(
    args: &[CommandArg],
    cmd: &str,
    units: &QuantityUnits,
    type_name: &str,
) -> Result<(RationalInteger, String), String> {
    use crate::computation::rational::decimal_to_rational;
    let lit = require_literal(args, cmd)?;
    let (magnitude, unit_name) = match lit {
        crate::literals::Value::NumberWithUnit(n, unit) => (*n, unit.clone()),
        other => {
            return Err(format!(
                "{cmd} requires a quantity literal with a unit, got {}",
                value_kind_name(other)
            ));
        }
    };
    units.get(unit_name.as_str()).map_err(|_| {
        format!(
            "Unit '{unit_name}' is not defined on '{type_name}'. Valid units are: {}.",
            format_quantity_units_list(units)
        )
    })?;
    let magnitude_rational = decimal_to_rational(magnitude)
        .map_err(|failure| format!("{cmd} literal failed rational lift: {failure}"))?;
    Ok((magnitude_rational, unit_name))
}

fn sync_quantity_units_from_canonical(
    units: &mut QuantityUnits,
    canonical: &RationalInteger,
    field: UnitConstraintField,
) -> Result<(), String> {
    use crate::computation::rational::checked_div;
    for unit in &mut units.0 {
        let magnitude = checked_div(canonical, &unit.factor).map_err(|failure| {
            format!(
                "cannot derive per-unit constraint for unit '{}': {failure}",
                unit.name
            )
        })?;
        match field {
            UnitConstraintField::Minimum => unit.minimum = Some(magnitude),
            UnitConstraintField::Maximum => unit.maximum = Some(magnitude),
            UnitConstraintField::DefaultMagnitude => unit.default_magnitude = Some(magnitude),
        }
    }
    Ok(())
}

fn sync_ratio_units_from_canonical(
    units: &mut RatioUnits,
    canonical: &RationalInteger,
    field: UnitConstraintField,
) -> Result<(), String> {
    use crate::computation::rational::checked_mul;
    for unit in &mut units.0 {
        let magnitude = checked_mul(canonical, &unit.value).map_err(|failure| {
            format!(
                "cannot derive per-unit constraint for ratio unit '{}': {failure}",
                unit.name
            )
        })?;
        match field {
            UnitConstraintField::Minimum => unit.minimum = Some(magnitude),
            UnitConstraintField::Maximum => unit.maximum = Some(magnitude),
            UnitConstraintField::DefaultMagnitude => unit.default_magnitude = Some(magnitude),
        }
    }
    Ok(())
}

fn sync_quantity_default_units(
    units: &mut QuantityUnits,
    default: &ValueKind,
    type_name: &str,
) -> Result<(), String> {
    use crate::computation::rational::checked_mul;
    let ValueKind::Quantity(magnitude, unit_name, _) = default else {
        return Ok(());
    };
    let unit = units.get(unit_name).map_err(|_| {
        format!("Default unit '{unit_name}' is not defined on quantity type '{type_name}'.")
    })?;
    let canonical = checked_mul(magnitude, &unit.factor)
        .map_err(|failure| format!("default: unit conversion overflow: {failure}"))?;
    sync_quantity_units_from_canonical(units, &canonical, UnitConstraintField::DefaultMagnitude)
}

pub(crate) fn finalize_quantity_unit_constraint_magnitudes(
    specification: &mut TypeSpecification,
    declared_default: Option<&ValueKind>,
    type_name: &str,
) -> Result<(), String> {
    let TypeSpecification::Quantity {
        minimum,
        maximum,
        units,
        ..
    } = specification
    else {
        return Ok(());
    };

    if let Some((magnitude, unit_name)) = minimum.clone() {
        let canonical = quantity_declared_bound_to_canonical(
            &magnitude, &unit_name, units, type_name, "minimum",
        )?;
        sync_quantity_units_from_canonical(units, &canonical, UnitConstraintField::Minimum)?;
    }
    if let Some((magnitude, unit_name)) = maximum.clone() {
        let canonical = quantity_declared_bound_to_canonical(
            &magnitude, &unit_name, units, type_name, "maximum",
        )?;
        sync_quantity_units_from_canonical(units, &canonical, UnitConstraintField::Maximum)?;
    }
    if let Some(default) = declared_default {
        sync_quantity_default_units(units, default, type_name)?;
    }
    Ok(())
}

pub(crate) fn quantity_declared_bound_canonical(
    bound: &(RationalInteger, String),
    units: &QuantityUnits,
    type_name: &str,
    command: &str,
) -> Result<RationalInteger, String> {
    let (magnitude, unit_name) = bound;
    quantity_declared_bound_to_canonical(magnitude, unit_name, units, type_name, command)
}

fn sync_ratio_default_units(units: &mut RatioUnits, default: &ValueKind) -> Result<(), String> {
    let ValueKind::Ratio(canonical, _) = default else {
        return Ok(());
    };
    sync_ratio_units_from_canonical(units, canonical, UnitConstraintField::DefaultMagnitude)
}

/// Extract an option name from a single arg.
///
/// Both `option red` (bare identifier, parsed as `Label`) and `option "red"`
/// (quoted text literal) are valid lemma syntax for option enumeration; the
/// grammar accepts either form. All other variants are rejected.
fn option_name(arg: &CommandArg, cmd: &str) -> Result<String, String> {
    match arg {
        CommandArg::Literal(crate::literals::Value::Text(s)) => Ok(s.clone()),
        CommandArg::Label(name) => Ok(name.clone()),
        CommandArg::Literal(other) => Err(format!(
            "{} requires a text literal or identifier, got {}",
            cmd,
            value_kind_name(other)
        )),
        CommandArg::UnitExpr(_) => Err(format!(
            "{} requires a text literal or identifier, got a unit expression",
            cmd
        )),
    }
}

fn label_name(arg: &CommandArg, cmd: &str) -> Result<String, String> {
    match arg {
        CommandArg::Label(name) => Ok(name.clone()),
        CommandArg::Literal(other) => Err(format!(
            "{} requires an identifier, got {}",
            cmd,
            value_kind_name(other)
        )),
        CommandArg::UnitExpr(_) => Err(format!(
            "{} requires an identifier, got a unit expression",
            cmd
        )),
    }
}

fn quantity_trait_name(quantity_trait: QuantityTrait) -> &'static str {
    match quantity_trait {
        QuantityTrait::Duration => "duration",
    }
}

fn parse_quantity_trait(args: &[CommandArg]) -> Result<QuantityTrait, String> {
    if args.len() != 1 {
        return Err("trait requires exactly one identifier argument".to_string());
    }
    match label_name(&args[0], "trait")?
        .trim()
        .to_lowercase()
        .as_str()
    {
        "duration" => Ok(QuantityTrait::Duration),
        other => Err(format!("Unknown quantity trait '{}'", other)),
    }
}

fn validate_duration_trait_requirements(units: &QuantityUnits) -> Result<(), String> {
    let second_unit = units
        .iter()
        .find(|unit| unit.name == "second")
        .ok_or_else(|| {
            "trait duration requires a canonical 'second' unit declared before 'trait duration'"
                .to_string()
        })?;
    if !second_unit.is_canonical_factor() {
        return Err("trait duration requires unit second 1".to_string());
    }
    Ok(())
}

/// Extract a [`DateTimeValue`] from a [`Value::Date`] literal arg.
fn require_date_literal(args: &[CommandArg], cmd: &str) -> Result<DateTimeValue, String> {
    match require_literal(args, cmd)? {
        crate::literals::Value::Date(dt) => Ok(dt.clone()),
        other => Err(format!(
            "{} requires a date literal (e.g. 2024-01-01), got {}",
            cmd,
            value_kind_name(other)
        )),
    }
}

/// Extract a [`TimeValue`] from a [`Value::Time`] literal arg.
fn require_time_literal(args: &[CommandArg], cmd: &str) -> Result<TimeValue, String> {
    match require_literal(args, cmd)? {
        crate::literals::Value::Time(t) => Ok(t.clone()),
        other => Err(format!(
            "{} requires a time literal (e.g. 12:30:00), got {}",
            cmd,
            value_kind_name(other)
        )),
    }
}

fn require_calendar_literal(
    args: &[CommandArg],
    cmd: &str,
) -> Result<(RationalInteger, CalendarUnit), String> {
    match require_literal(args, cmd)? {
        crate::literals::Value::Calendar(d, unit) => {
            lift_parser_decimal(*d).map(|value| (value, unit.clone()))
        }
        other => Err(format!(
            "{} requires a calendar literal (e.g. 1 month), got {}",
            cmd,
            value_kind_name(other)
        )),
    }
}

/// Default `help` for a built-in primitive (goal-oriented; syntax lives in [`LemmaType::example_value`]).
#[must_use]
pub fn default_help_for_primitive(kind: PrimitiveKind) -> &'static str {
    use PrimitiveKind::*;
    match kind {
        Boolean => "Whether this holds (true or false).",
        Number => "A dimensionless number.",
        NumberRange => "The lower and upper bound of the number range.",
        Text => "A text value.",
        Quantity => "A numeric amount in one of this type's units.",
        QuantityRange => "The lower and upper bound of the quantity range in the same unit.",
        Ratio | Percent => "A ratio in one of this type's units (e.g. percent).",
        RatioRange => "The lower and upper bound of the ratio range.",
        Date => "A date, or a date and time with optional timezone.",
        DateRange => "The start date and end date of the date range.",
        Time => "A time of day, with optional timezone.",
        Calendar => "A length in years or months.",
        CalendarRange => "The lower and upper bound of the calendar range in years or months.",
    }
}

impl TypeSpecification {
    pub fn boolean() -> Self {
        TypeSpecification::Boolean {
            help: default_help_for_primitive(PrimitiveKind::Boolean).to_string(),
        }
    }
    pub fn quantity() -> Self {
        TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units: QuantityUnits::new(),
            traits: Vec::new(),
            decomposition: BaseQuantityVector::new(),
            canonical_unit: String::new(),
            help: default_help_for_primitive(PrimitiveKind::Quantity).to_string(),
        }
    }
    pub fn number() -> Self {
        TypeSpecification::Number {
            minimum: None,
            maximum: None,
            decimals: None,
            help: default_help_for_primitive(PrimitiveKind::Number).to_string(),
        }
    }
    pub fn number_range() -> Self {
        TypeSpecification::NumberRange {
            help: default_help_for_primitive(PrimitiveKind::NumberRange).to_string(),
        }
    }
    pub fn ratio() -> Self {
        TypeSpecification::Ratio {
            minimum: None,
            maximum: None,
            decimals: None,
            units: RatioUnits(vec![
                RatioUnit {
                    name: "percent".to_string(),
                    value: crate::computation::rational::RationalInteger::new(100, 1),
                    minimum: None,
                    maximum: None,
                    default_magnitude: None,
                },
                RatioUnit {
                    name: "permille".to_string(),
                    value: crate::computation::rational::RationalInteger::new(1000, 1),
                    minimum: None,
                    maximum: None,
                    default_magnitude: None,
                },
            ]),
            help: default_help_for_primitive(PrimitiveKind::Ratio).to_string(),
        }
    }
    pub fn ratio_range() -> Self {
        TypeSpecification::RatioRange {
            units: match TypeSpecification::ratio() {
                TypeSpecification::Ratio { units, .. } => units,
                _ => unreachable!("BUG: ratio constructor must return a ratio type"),
            },
            help: default_help_for_primitive(PrimitiveKind::RatioRange).to_string(),
        }
    }
    pub fn text() -> Self {
        TypeSpecification::Text {
            length: None,
            options: vec![],
            help: default_help_for_primitive(PrimitiveKind::Text).to_string(),
        }
    }
    pub fn date() -> Self {
        TypeSpecification::Date {
            minimum: None,
            maximum: None,
            help: default_help_for_primitive(PrimitiveKind::Date).to_string(),
        }
    }
    pub fn date_range() -> Self {
        TypeSpecification::DateRange {
            help: default_help_for_primitive(PrimitiveKind::DateRange).to_string(),
        }
    }
    pub fn time() -> Self {
        TypeSpecification::Time {
            minimum: None,
            maximum: None,
            help: default_help_for_primitive(PrimitiveKind::Time).to_string(),
        }
    }
    pub fn calendar() -> Self {
        TypeSpecification::Calendar {
            minimum: None,
            maximum: None,
            help: default_help_for_primitive(PrimitiveKind::Calendar).to_string(),
        }
    }
    pub fn calendar_range() -> Self {
        TypeSpecification::CalendarRange {
            help: default_help_for_primitive(PrimitiveKind::CalendarRange).to_string(),
        }
    }
    pub fn quantity_range() -> Self {
        TypeSpecification::QuantityRange {
            units: QuantityUnits::new(),
            decomposition: BaseQuantityVector::new(),
            canonical_unit: String::new(),
            help: default_help_for_primitive(PrimitiveKind::QuantityRange).to_string(),
        }
    }
    pub fn veto() -> Self {
        TypeSpecification::Veto { message: None }
    }

    /// Apply a single constraint command to this spec.
    ///
    /// The `declared_default` out-parameter receives the default value (if the command
    /// is `Default`), encoded as [`ValueKind`]. Defaults are owned by the data binding
    /// or typedef entry, not by the type specification itself; callers thread a single
    /// `&mut Option<ValueKind>` across all constraint applications for one type so the
    /// latest `-> default` command wins.
    pub fn apply_constraint(
        mut self,
        type_name: &str,
        command: TypeConstraintCommand,
        args: &[CommandArg],
        declared_default: &mut Option<ValueKind>,
    ) -> Result<Self, String> {
        if command == TypeConstraintCommand::Trait
            && !matches!(&self, TypeSpecification::Quantity { .. })
        {
            return Err("trait command is only valid on quantity types".to_string());
        }
        match &mut self {
            TypeSpecification::Boolean { help } => match command {
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let lit = require_literal(args, "default")?;
                    reject_calendar_for_default(lit, type_name, DefaultExpectation::Boolean, None)?;
                    match lit {
                        crate::literals::Value::Boolean(bv) => {
                            *declared_default = Some(ValueKind::Boolean(bool::from(bv)));
                        }
                        _ => {
                            return Err(
                                "Please provide true or false, for example `-> default true`."
                                    .to_string(),
                            );
                        }
                    }
                }
                other => {
                    return Err(format!(
                        "Invalid command '{}' for boolean type. Valid commands: help, default",
                        other
                    ));
                }
            },
            TypeSpecification::Quantity {
                decimals,
                minimum,
                maximum,
                units,
                traits,
                help,
                ..
            } => match command {
                TypeConstraintCommand::Decimals => {
                    let d = require_decimal_literal(args, "decimals")?;
                    *decimals = Some(decimal_to_u8(d, "decimals")?);
                }
                TypeConstraintCommand::Unit => {
                    let (unit_name, value, derived_quantity_factors) = match args {
                        [CommandArg::Label(name), CommandArg::UnitExpr(crate::parsing::ast::UnitArg::Factor(v))] => {
                            (name.clone(), *v, Vec::new())
                        }
                        [CommandArg::Label(name), CommandArg::UnitExpr(crate::parsing::ast::UnitArg::Expr(
                            prefix,
                            factors,
                        ))] => {
                            let raw: Vec<(String, i32)> = factors
                                .iter()
                                .map(|f| (f.quantity_ref.clone(), f.exp))
                                .collect();
                            (name.clone(), *prefix, raw)
                        }
                        _ => {
                            return Err(
                                "unit requires a unit name followed by a conversion factor or compound unit expression (e.g., 'unit eur 1.00' or 'unit mps meter/second')"
                                    .to_string(),
                            );
                        }
                    };
                    if let Some(u) = units.0.iter_mut().find(|u| u.name == unit_name) {
                        u.factor = crate::computation::rational::decimal_to_rational(value)
                            .map_err(|failure| failure.to_string())?;
                        u.derived_quantity_factors = derived_quantity_factors;
                    } else {
                        units.0.push(QuantityUnit::from_decimal_factor(
                            unit_name,
                            value,
                            derived_quantity_factors,
                        )?);
                    }
                }
                TypeConstraintCommand::Trait => {
                    let quantity_trait = parse_quantity_trait(args)?;
                    if traits.contains(&quantity_trait) {
                        return Err(format!(
                            "Duplicate trait '{}' for quantity type.",
                            quantity_trait_name(quantity_trait)
                        ));
                    }
                    if quantity_trait == QuantityTrait::Duration {
                        validate_duration_trait_requirements(units)?;
                    }
                    traits.push(quantity_trait);
                }
                TypeConstraintCommand::Minimum => {
                    *minimum = Some(parse_quantity_declared_bound(
                        args, "minimum", units, type_name,
                    )?);
                }
                TypeConstraintCommand::Maximum => {
                    *maximum = Some(parse_quantity_declared_bound(
                        args, "maximum", units, type_name,
                    )?);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let lit = require_literal(args, "default")?;
                    reject_calendar_for_default(
                        lit,
                        type_name,
                        DefaultExpectation::QuantityUnits,
                        Some(units),
                    )?;
                    let default =
                        validate_quantity_default_literal(args, type_name, units, traits)?;
                    *declared_default = Some(default);
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for quantity type. Valid commands: unit, trait, minimum, maximum, decimals, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Number {
                decimals,
                minimum,
                maximum,
                help,
            } => match command {
                TypeConstraintCommand::Decimals => {
                    let d = require_decimal_literal(args, "decimals")?;
                    *decimals = Some(decimal_to_u8(d, "decimals")?);
                }
                TypeConstraintCommand::Unit => {
                    return Err(
                        "Invalid command 'unit' for number type. Number types are dimensionless and cannot have units. Use 'quantity' type instead.".to_string()
                    );
                }
                TypeConstraintCommand::Minimum => {
                    *minimum = Some(require_decimal_literal(args, "minimum")?);
                }
                TypeConstraintCommand::Maximum => {
                    *maximum = Some(require_decimal_literal(args, "maximum")?);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let lit = require_literal(args, "default")?;
                    reject_calendar_for_default(lit, type_name, DefaultExpectation::Number, None)?;
                    match lit {
                        crate::literals::Value::Number(d) => {
                            *declared_default = Some(ValueKind::Number(lift_parser_decimal(*d)?));
                        }
                        _ => {
                            return Err(
                                "Please provide a number, for example `-> default 42`.".to_string()
                            );
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for number type. Valid commands: minimum, maximum, decimals, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::NumberRange { help } => match command {
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let (left, right) = require_default_range_endpoints(
                        args,
                        type_name,
                        DefaultExpectation::NumberRange,
                        None,
                    )?;
                    let left = literal_value_from_parser_value(left)?;
                    let right = literal_value_from_parser_value(right)?;
                    if !left.lemma_type.is_number() || !right.lemma_type.is_number() {
                        return Err(
                            "Please provide a number range, for example `-> default 10...100`."
                                .to_string(),
                        );
                    }
                    *declared_default = Some(ValueKind::Range(Box::new(left), Box::new(right)));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for number range type. Valid commands: help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Ratio {
                decimals,
                minimum,
                maximum,
                units,
                help,
            } => match command {
                TypeConstraintCommand::Decimals => {
                    let d = require_decimal_literal(args, "decimals")?;
                    *decimals = Some(decimal_to_u8(d, "decimals")?);
                }
                TypeConstraintCommand::Unit => {
                    let (unit_name, value_dec) = match args {
                        [CommandArg::Label(name), CommandArg::UnitExpr(crate::parsing::ast::UnitArg::Factor(v))] => {
                            (name.clone(), *v)
                        }
                        _ => {
                            return Err(
                                "unit requires a unit name followed by a numeric conversion factor (e.g., 'unit percent 100'). Compound unit expressions are not supported for ratio types."
                                    .to_string(),
                            );
                        }
                    };
                    let value = crate::computation::rational::decimal_to_rational(value_dec)
                        .map_err(|failure| {
                            format!(
                                "ratio unit value is not exactly representable as a rational: {}",
                                failure
                            )
                        })?;
                    if let Some(u) = units.0.iter_mut().find(|u| u.name == unit_name) {
                        u.value = value;
                    } else {
                        units.0.push(RatioUnit {
                            name: unit_name,
                            value,
                            minimum: None,
                            maximum: None,
                            default_magnitude: None,
                        });
                    }
                }
                TypeConstraintCommand::Minimum => {
                    let canonical = ratio_bound_to_canonical_rational(args, "minimum", units)?;
                    sync_ratio_units_from_canonical(
                        units,
                        &canonical,
                        UnitConstraintField::Minimum,
                    )?;
                    *minimum = Some(canonical);
                }
                TypeConstraintCommand::Maximum => {
                    let canonical = ratio_bound_to_canonical_rational(args, "maximum", units)?;
                    sync_ratio_units_from_canonical(
                        units,
                        &canonical,
                        UnitConstraintField::Maximum,
                    )?;
                    *maximum = Some(canonical);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let lit = require_literal(args, "default")?;
                    reject_calendar_for_default(lit, type_name, DefaultExpectation::Ratio, None)?;
                    let default = match lit {
                        crate::literals::Value::Number(d) => {
                            ValueKind::Ratio(lift_parser_decimal(*d)?, None)
                        }
                        crate::literals::Value::NumberWithUnit(_, _) => {
                            let element_spec = TypeSpecification::Ratio {
                                decimals: *decimals,
                                minimum: *minimum,
                                maximum: *maximum,
                                units: units.clone(),
                                help: help.clone(),
                            };
                            parser_value_to_value_kind(lit, &element_spec)?
                        }
                        _ => {
                            return Err("Please provide a ratio value, for example `-> default 0.25` or `-> default 25%`.".to_string());
                        }
                    };
                    sync_ratio_default_units(units, &default)?;
                    *declared_default = Some(default);
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for ratio type. Valid commands: unit, minimum, maximum, decimals, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::RatioRange { units, help } => {
                match command {
                    TypeConstraintCommand::Unit => {
                        let (unit_name, value_dec) = match args {
                            [CommandArg::Label(name), CommandArg::UnitExpr(crate::parsing::ast::UnitArg::Factor(v))] => {
                                (name.clone(), *v)
                            }
                            _ => {
                                return Err(
                                "unit requires a unit name followed by a numeric conversion factor (e.g., 'unit percent 100'). Compound unit expressions are not supported for ratio range types."
                                    .to_string(),
                            );
                            }
                        };
                        let value = crate::computation::rational::decimal_to_rational(value_dec)
                        .map_err(|e| format!("ratio unit value is not exactly representable as a rational: {e}"))?;
                        if let Some(u) = units.0.iter_mut().find(|u| u.name == unit_name) {
                            u.value = value;
                        } else {
                            units.0.push(RatioUnit {
                                name: unit_name,
                                value,
                                minimum: None,
                                maximum: None,
                                default_magnitude: None,
                            });
                        }
                    }
                    TypeConstraintCommand::Help => {
                        apply_type_help_command(help, args)?;
                    }
                    TypeConstraintCommand::Default => {
                        let (left, right) = require_default_range_endpoints(
                            args,
                            type_name,
                            DefaultExpectation::RatioRange,
                            None,
                        )?;
                        let element_spec = TypeSpecification::Ratio {
                            decimals: None,
                            minimum: None,
                            maximum: None,
                            units: units.clone(),
                            help: String::new(),
                        };
                        let left = lift_range_endpoint(left, &element_spec)?;
                        let right = lift_range_endpoint(right, &element_spec)?;
                        if !left.lemma_type.is_ratio() || !right.lemma_type.is_ratio() {
                            return Err(
                                "Please provide a ratio range, for example `-> default 10%...50%`."
                                    .to_string(),
                            );
                        }
                        *declared_default = Some(ValueKind::Range(Box::new(left), Box::new(right)));
                    }
                    _ => {
                        return Err(format!(
                        "Invalid command '{}' for ratio range type. Valid commands: unit, help, default",
                        command
                    ));
                    }
                }
            }
            TypeSpecification::Text {
                length,
                options,
                help,
            } => match command {
                TypeConstraintCommand::Option => {
                    if args.len() != 1 {
                        return Err("option takes exactly one argument".to_string());
                    }
                    options.push(option_name(&args[0], "option")?);
                }
                TypeConstraintCommand::Options => {
                    let mut collected = Vec::with_capacity(args.len());
                    for arg in args {
                        collected.push(option_name(arg, "options")?);
                    }
                    *options = collected;
                }
                TypeConstraintCommand::Length => {
                    let d = require_decimal_literal(args, "length")?;
                    *length = Some(decimal_to_usize(d, "length")?);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let lit = require_literal(args, "default")?;
                    reject_calendar_for_default(lit, type_name, DefaultExpectation::Text, None)?;
                    match lit {
                        crate::literals::Value::Text(s) => {
                            *declared_default = Some(ValueKind::Text(s.clone()));
                        }
                        _ => {
                            return Err(
                                "Please provide a text value in double quotes, for example `-> default \"my default value\"`."
                                    .to_string(),
                            );
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for text type. Valid commands: options, length, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Date {
                minimum,
                maximum,
                help,
            } => match command {
                TypeConstraintCommand::Minimum => {
                    let dt = require_date_literal(args, "minimum")?;
                    *minimum = Some(dt);
                }
                TypeConstraintCommand::Maximum => {
                    let dt = require_date_literal(args, "maximum")?;
                    *maximum = Some(dt);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let lit = require_literal(args, "default")?;
                    reject_calendar_for_default(lit, type_name, DefaultExpectation::Date, None)?;
                    match lit {
                        crate::literals::Value::Date(dt) => {
                            *declared_default = Some(ValueKind::Date(date_time_to_semantic(dt)));
                        }
                        _ => {
                            return Err(
                                "Please provide a date, for example `-> default 2024-06-15`."
                                    .to_string(),
                            );
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for date type. Valid commands: minimum, maximum, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::DateRange { help } => match command {
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let (left, right) = require_default_range_endpoints(
                        args,
                        type_name,
                        DefaultExpectation::DateRange,
                        None,
                    )?;
                    let left = literal_value_from_parser_value(left)?;
                    let right = literal_value_from_parser_value(right)?;
                    if !left.lemma_type.is_date() || !right.lemma_type.is_date() {
                        return Err(
                            "Please provide a date range, for example `-> default 2024-01-01...2024-12-31`."
                                .to_string(),
                        );
                    }
                    *declared_default = Some(ValueKind::Range(Box::new(left), Box::new(right)));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for date range type. Valid commands: help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Time {
                minimum,
                maximum,
                help,
            } => match command {
                TypeConstraintCommand::Minimum => {
                    let t = require_time_literal(args, "minimum")?;
                    *minimum = Some(t);
                }
                TypeConstraintCommand::Maximum => {
                    let t = require_time_literal(args, "maximum")?;
                    *maximum = Some(t);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let lit = require_literal(args, "default")?;
                    reject_calendar_for_default(lit, type_name, DefaultExpectation::Time, None)?;
                    match lit {
                        crate::literals::Value::Time(t) => {
                            *declared_default = Some(ValueKind::Time(time_to_semantic(t)));
                        }
                        _ => {
                            return Err(
                                "Please provide a time, for example `-> default 09:00:00`."
                                    .to_string(),
                            );
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for time type. Valid commands: minimum, maximum, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Calendar {
                minimum,
                maximum,
                help,
            } => match command {
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Minimum => {
                    let (value, unit) = require_calendar_literal(args, "minimum")?;
                    *minimum = Some((value, calendar_unit_to_semantic(&unit)));
                }
                TypeConstraintCommand::Maximum => {
                    let (value, unit) = require_calendar_literal(args, "maximum")?;
                    *maximum = Some((value, calendar_unit_to_semantic(&unit)));
                }
                TypeConstraintCommand::Default => {
                    let (value, unit) = require_calendar_literal(args, "default")?;
                    *declared_default =
                        Some(ValueKind::Calendar(value, calendar_unit_to_semantic(&unit)));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for calendar type. Valid commands: minimum, maximum, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::CalendarRange { help } => match command {
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let (left, right) = require_default_range_endpoints(
                        args,
                        type_name,
                        DefaultExpectation::CalendarRange,
                        None,
                    )?;
                    let left = literal_value_from_parser_value(left)?;
                    let right = literal_value_from_parser_value(right)?;
                    if !left.lemma_type.is_calendar() || !right.lemma_type.is_calendar() {
                        return Err(
                            "Please provide a calendar range, for example `-> default 18 years...67 years`."
                                .to_string(),
                        );
                    }
                    *declared_default = Some(ValueKind::Range(Box::new(left), Box::new(right)));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for calendar range type. Valid commands: help, default",
                        command
                    ));
                }
            },
            TypeSpecification::QuantityRange { units, help, .. } => match command {
                TypeConstraintCommand::Unit => {
                    let (unit_name, value, derived_quantity_factors) = match args {
                        [CommandArg::Label(name), CommandArg::UnitExpr(crate::parsing::ast::UnitArg::Factor(v))] => {
                            (name.clone(), *v, Vec::new())
                        }
                        [CommandArg::Label(name), CommandArg::UnitExpr(crate::parsing::ast::UnitArg::Expr(
                            prefix,
                            factors,
                        ))] => {
                            let raw: Vec<(String, i32)> = factors
                                .iter()
                                .map(|f| (f.quantity_ref.clone(), f.exp))
                                .collect();
                            (name.clone(), *prefix, raw)
                        }
                        _ => {
                            return Err(
                                "unit requires a unit name followed by a conversion factor or compound unit expression (e.g., 'unit eur 1.00' or 'unit mps meter/second')"
                                    .to_string(),
                            );
                        }
                    };
                    if let Some(u) = units.0.iter_mut().find(|u| u.name == unit_name) {
                        u.factor = crate::computation::rational::decimal_to_rational(value)
                            .map_err(|failure| failure.to_string())?;
                        u.derived_quantity_factors = derived_quantity_factors;
                    } else {
                        units.0.push(QuantityUnit::from_decimal_factor(
                            unit_name,
                            value,
                            derived_quantity_factors,
                        )?);
                    }
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let (left, right) = require_default_range_endpoints(
                        args,
                        type_name,
                        DefaultExpectation::QuantityRange,
                        Some(units),
                    )?;
                    let left = literal_value_from_parser_value(left)?;
                    let right = literal_value_from_parser_value(right)?;
                    if !left.lemma_type.is_quantity() || !right.lemma_type.is_quantity() {
                        return Err(format!(
                            "Please provide a range with units valid for '{type_name}', for example `-> default 30 kilogram...35 kilogram`."
                        ));
                    }
                    *declared_default = Some(ValueKind::Range(Box::new(left), Box::new(right)));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for quantity range type. Valid commands: unit, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Veto { .. } => {
                return Err(format!(
                    "Invalid command '{}' for veto type. Veto is not a user-declarable type and cannot have constraints",
                    command
                ));
            }
            TypeSpecification::Undetermined => {
                return Err(format!(
                    "Invalid command '{}' for undetermined sentinel type. Undetermined is an internal type used during type inference and cannot have constraints",
                    command
                ));
            }
        }
        Ok(self)
    }
}

/// Parse a "number unit" string into a Quantity or Ratio value according to the type.
/// Caller must have obtained the TypeSpecification via unit_index from the unit in the string.
pub fn parse_number_unit(
    value_str: &str,
    type_spec: &TypeSpecification,
) -> Result<crate::parsing::ast::Value, String> {
    use crate::literals::{NumberWithUnit, RatioLiteral};
    use crate::parsing::ast::Value;

    let trimmed = value_str.trim();
    match type_spec {
        TypeSpecification::Quantity { units, .. } => {
            if units.is_empty() {
                unreachable!(
                    "BUG: Quantity type has no units; should have been validated during planning"
                );
            }
            match trimmed.parse::<NumberWithUnit>() {
                Ok(n) => {
                    let unit = units.get(&n.1).map_err(|e| e.to_string())?;
                    Ok(Value::NumberWithUnit(n.0, unit.name.clone()))
                }
                Err(e) => {
                    if trimmed.split_whitespace().count() == 1 && !trimmed.is_empty() {
                        let valid: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
                        let example_unit = units
                            .iter()
                            .next()
                            .expect("BUG: units non-empty after guard")
                            .name
                            .as_str();
                        Err(format!(
                            "Quantity value must include a unit, for example: '{} {}'. Valid units: {}.",
                            trimmed,
                            example_unit,
                            valid.join(", ")
                        ))
                    } else {
                        Err(e)
                    }
                }
            }
        }
        TypeSpecification::Ratio { units, .. } => {
            if units.is_empty() {
                unreachable!(
                    "BUG: Ratio type has no units; should have been validated during planning"
                );
            }
            match trimmed.parse::<RatioLiteral>()? {
                RatioLiteral::Bare(_) => {
                    Err("Ratio value requires a unit (e.g. '50%', '500 basis_points').".to_string())
                }
                RatioLiteral::Percent(n) => {
                    let unit = units.get("percent").map_err(|e| e.to_string())?;
                    Ok(Value::NumberWithUnit(n, unit.name.clone()))
                }
                RatioLiteral::Permille(n) => {
                    let unit = units.get("permille").map_err(|e| e.to_string())?;
                    Ok(Value::NumberWithUnit(n, unit.name.clone()))
                }
                RatioLiteral::Named { value, unit } => {
                    let resolved = units.get(&unit).map_err(|e| e.to_string())?;
                    Ok(Value::NumberWithUnit(value, resolved.name.clone()))
                }
            }
        }
        _ => Err("parse_number_unit only accepts Quantity or Ratio type".to_string()),
    }
}

/// Parse one data field from JSON: convenience strings or serialized objects.
pub fn parse_data_value_from_json(
    value: &serde_json::Value,
    type_spec: &TypeSpecification,
    lemma_type: &LemmaType,
    source: &Source,
) -> Result<LiteralValue, Error> {
    let to_err = |msg: String| Error::validation(msg, Some(source.clone()), None::<String>);

    let kind = if let Some(s) = value.as_str() {
        let parsed = parse_value_from_string(s, type_spec, source)?;
        parser_value_to_value_kind(&parsed, type_spec).map_err(to_err)?
    } else if let Some(b) = value.as_bool() {
        if !matches!(type_spec, TypeSpecification::Boolean { .. }) {
            return Err(to_err(format!(
                "JSON boolean is only valid for boolean data, not {}",
                value_kind_tag_for_type(type_spec)
            )));
        }
        ValueKind::Boolean(b)
    } else if let Some(n) = value.as_number() {
        let parsed = parse_value_from_string(&n.to_string(), type_spec, source)?;
        parser_value_to_value_kind(&parsed, type_spec).map_err(to_err)?
    } else if let Some(obj) = value.as_object() {
        if obj.len() == 2 && obj.contains_key("value") && obj.contains_key("unit") {
            let tagged = serde_json::json!({ value_kind_tag_for_type(type_spec): value });
            serde_json::from_value::<ValueKind>(tagged).map_err(|e| to_err(e.to_string()))?
        } else {
            serde_json::from_value::<ValueKind>(value.clone()).map_err(|e| to_err(e.to_string()))?
        }
    } else {
        return Err(to_err("unsupported JSON value for data input".to_string()));
    };

    Ok(LiteralValue {
        value: kind,
        lemma_type: lemma_type.clone(),
    })
}

fn value_kind_tag_for_type(spec: &TypeSpecification) -> &'static str {
    match spec {
        TypeSpecification::Boolean { .. } => "boolean",
        TypeSpecification::Quantity { .. } => "quantity",
        TypeSpecification::Number { .. } => "number",
        TypeSpecification::NumberRange { .. }
        | TypeSpecification::QuantityRange { .. }
        | TypeSpecification::DateRange { .. }
        | TypeSpecification::RatioRange { .. }
        | TypeSpecification::CalendarRange { .. } => "range",
        TypeSpecification::Ratio { .. } => "ratio",
        TypeSpecification::Text { .. } => "text",
        TypeSpecification::Date { .. } => "date",
        TypeSpecification::Time { .. } => "time",
        TypeSpecification::Calendar { .. } => "calendar",
        TypeSpecification::Veto { .. } => "veto",
        TypeSpecification::Undetermined => "undetermined",
    }
}

/// Parse a string value according to a TypeSpecification.
/// Used to parse runtime user input into typed values.
pub fn parse_value_from_string(
    value_str: &str,
    type_spec: &TypeSpecification,
    source: &Source,
) -> Result<crate::parsing::ast::Value, Error> {
    use crate::parsing::ast::Value;

    let to_err = |msg: String| Error::validation(msg, Some(source.clone()), None::<String>);

    let parse_range_value = |element_spec: TypeSpecification| -> Result<Value, Error> {
        let (left_str, right_str) = value_str.split_once("...").ok_or_else(|| {
            to_err("Range value must use '...' between the two endpoints".to_string())
        })?;
        if left_str.trim().is_empty() || right_str.trim().is_empty() {
            return Err(to_err(
                "Range value must contain a non-empty left and right endpoint".to_string(),
            ));
        }
        let left = parse_value_from_string(left_str.trim(), &element_spec, source)?;
        let right = parse_value_from_string(right_str.trim(), &element_spec, source)?;
        Ok(Value::Range(Box::new(left), Box::new(right)))
    };

    match type_spec {
        TypeSpecification::Text { .. } => value_str
            .parse::<crate::literals::TextLiteral>()
            .map(|t| Value::Text(t.0))
            .map_err(to_err),
        TypeSpecification::Number { .. } => value_str
            .parse::<crate::literals::NumberLiteral>()
            .map(|n| Value::Number(n.0))
            .map_err(to_err),
        TypeSpecification::Quantity { .. } => {
            parse_number_unit(value_str, type_spec).map_err(to_err)
        }
        TypeSpecification::Boolean { .. } => value_str
            .parse::<BooleanValue>()
            .map(Value::Boolean)
            .map_err(to_err),
        TypeSpecification::Date { .. } => {
            let date = value_str.parse::<DateTimeValue>().map_err(to_err)?;
            Ok(Value::Date(date))
        }
        TypeSpecification::Time { .. } => {
            let time = value_str.parse::<TimeValue>().map_err(to_err)?;
            Ok(Value::Time(time))
        }
        TypeSpecification::Calendar { .. } => value_str
            .parse::<crate::literals::CalendarLiteral>()
            .map(|d| Value::Calendar(d.0, d.1))
            .map_err(to_err),
        TypeSpecification::Ratio { .. } => {
            parse_number_unit(value_str, type_spec).map_err(to_err)
        }
        TypeSpecification::NumberRange { .. }
        | TypeSpecification::QuantityRange { .. }
        | TypeSpecification::DateRange { .. }
        | TypeSpecification::CalendarRange { .. }
        | TypeSpecification::RatioRange { .. } => {
            let element_spec = range_element_type_specification(type_spec).unwrap_or_else(|| {
                unreachable!("BUG: range_element_type_specification missing arm for known range type")
            });
            parse_range_value(element_spec)
        }
        TypeSpecification::Veto { .. } => Err(to_err(
            "Veto type cannot be parsed from string".to_string(),
        )),
        TypeSpecification::Undetermined => unreachable!(
            "BUG: parse_value_from_string called with Undetermined sentinel type; this type exists only during type inference"
        ),
    }
}

// -----------------------------------------------------------------------------
// Semantic value types (no parser dependency - used by evaluation, inversion, etc.)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCalendarUnit {
    Month,
    Year,
}

impl fmt::Display for SemanticCalendarUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SemanticCalendarUnit::Month => "months",
            SemanticCalendarUnit::Year => "years",
        };
        write!(f, "{}", s)
    }
}

/// Target unit for conversion (semantic; used by evaluation/computation).
/// Planning converts AST ConversionTarget into this so computation does not depend on parsing.
/// Ratio vs quantity is determined by looking up the unit in the type registry's unit index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticConversionTarget {
    Calendar(SemanticCalendarUnit),
    QuantityUnit(String),
    RatioUnit(String),
    /// Strip unit label and return the raw numeric value as a Number.
    Number,
}

impl fmt::Display for SemanticConversionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SemanticConversionTarget::Calendar(u) => write!(f, "{}", u),
            SemanticConversionTarget::QuantityUnit(s) => write!(f, "{}", s),
            SemanticConversionTarget::RatioUnit(s) => write!(f, "{}", s),
            SemanticConversionTarget::Number => write!(f, "number"),
        }
    }
}

/// Timezone for semantic date/time values
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SemanticTimezone {
    pub offset_hours: i8,
    pub offset_minutes: u8,
}

impl fmt::Display for SemanticTimezone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.offset_hours == 0 && self.offset_minutes == 0 {
            write!(f, "Z")
        } else {
            let sign = if self.offset_hours >= 0 { "+" } else { "-" };
            let hours = self.offset_hours.abs();
            write!(f, "{}{:02}:{:02}", sign, hours, self.offset_minutes)
        }
    }
}

/// Time-of-day for semantic values
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SemanticTime {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub microsecond: u32,
    pub timezone: Option<SemanticTimezone>,
}

impl fmt::Display for SemanticTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)?;
        if self.microsecond != 0 {
            write!(f, ".{:06}", self.microsecond)?;
        }
        if let Some(timezone) = &self.timezone {
            write!(f, "{}", timezone)?;
        }
        Ok(())
    }
}

/// Date-time for semantic values
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SemanticDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    #[serde(default)]
    pub microsecond: u32,
    pub timezone: Option<SemanticTimezone>,
}

impl fmt::Display for SemanticDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let has_time = self.hour != 0
            || self.minute != 0
            || self.second != 0
            || self.microsecond != 0
            || self.timezone.is_some();
        if !has_time {
            write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
        } else {
            write!(
                f,
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                self.year, self.month, self.day, self.hour, self.minute, self.second
            )?;
            if self.microsecond != 0 {
                write!(f, ".{:06}", self.microsecond)?;
            }
            if let Some(tz) = &self.timezone {
                write!(f, "{}", tz)?;
            }
            Ok(())
        }
    }
}

/// Value payload (shape of a literal). No type attached.
/// Quantity unit is required; Ratio unit is optional (see plan ratio-units-optional.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueKind {
    Number(RationalInteger),
    /// Quantity: value + unit + decomposition.
    ///
    /// - For values bound to a named typedef, `decomposition` is empty (the type's
    ///   `TypeSpecification::Quantity.decomposition` is authoritative).
    /// - For anonymous cross-axis intermediates produced by `*`/`/`, `decomposition`
    ///   is non-empty and carries the combined dimensional vector.
    /// - For anonymous intermediates the unit string is empty (`""`).
    Quantity(RationalInteger, String, BaseQuantityVector),
    Text(String),
    Date(SemanticDateTime),
    Time(SemanticTime),
    Boolean(bool),
    /// Calendar: value + unit
    Calendar(RationalInteger, SemanticCalendarUnit),
    /// Ratio: value + optional unit
    Ratio(RationalInteger, Option<String>),
    Range(Box<LiteralValue>, Box<LiteralValue>),
}

fn format_rational_magnitude_for_display(rational: &RationalInteger) -> String {
    crate::computation::rational::rational_to_display_str(rational)
}

fn format_number_with_unit_for_display(rational: &RationalInteger, unit: &str) -> String {
    use crate::computation::rational::{commit_rational_to_decimal, rational_to_display_str};
    use crate::parsing::ast::Value;
    match commit_rational_to_decimal(rational) {
        Ok(decimal) => format!("{}", Value::NumberWithUnit(decimal, unit.to_string())),
        Err(_) => format!("{} {}", rational_to_display_str(rational), unit),
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::computation::rational::{checked_mul, rational_to_display_str};
        match self {
            ValueKind::Number(rational) => {
                write!(f, "{}", format_rational_magnitude_for_display(rational))
            }
            ValueKind::Quantity(rational, unit, _decomp) => {
                write!(f, "{}", format_number_with_unit_for_display(rational, unit))
            }
            ValueKind::Text(s) => write!(f, "{}", crate::parsing::ast::Value::Text(s.clone())),
            ValueKind::Ratio(rational, unit) => match unit.as_deref() {
                Some("percent") => {
                    let display = match checked_mul(rational, &RationalInteger::new(100, 1)) {
                        Ok(scaled) => format_number_with_unit_for_display(&scaled, "percent"),
                        Err(_) => format!("{} percent", rational_to_display_str(rational)),
                    };
                    write!(f, "{}", display)
                }
                Some("permille") => {
                    let display = match checked_mul(rational, &RationalInteger::new(1000, 1)) {
                        Ok(scaled) => format_number_with_unit_for_display(&scaled, "permille"),
                        Err(_) => format!("{} permille", rational_to_display_str(rational)),
                    };
                    write!(f, "{}", display)
                }
                Some(unit_name) => {
                    write!(
                        f,
                        "{}",
                        format_number_with_unit_for_display(rational, unit_name)
                    )
                }
                None => write!(f, "{}", format_rational_magnitude_for_display(rational)),
            },
            ValueKind::Date(dt) => write!(f, "{}", dt),
            ValueKind::Time(t) => write!(
                f,
                "{}",
                crate::parsing::ast::Value::Time(crate::parsing::ast::TimeValue {
                    hour: t.hour as u8,
                    minute: t.minute as u8,
                    second: t.second as u8,
                    microsecond: t.microsecond,
                    timezone: t
                        .timezone
                        .as_ref()
                        .map(|tz| crate::parsing::ast::TimezoneValue {
                            offset_hours: tz.offset_hours,
                            offset_minutes: tz.offset_minutes,
                        }),
                })
            ),
            ValueKind::Boolean(b) => write!(f, "{}", b),
            ValueKind::Calendar(rational, unit) => write!(
                f,
                "{} {}",
                format_rational_magnitude_for_display(rational),
                unit
            ),
            ValueKind::Range(left, right) => write!(f, "{}...{}", left, right),
        }
    }
}

fn decimal_from_serialized_str(s: &str) -> Result<Decimal, String> {
    Decimal::from_str(s.trim()).map_err(|e| format!("invalid decimal '{s}': {e}"))
}

#[derive(Serialize, Deserialize)]
struct SerializedValueUnit {
    value: String,
    unit: String,
}

#[derive(Serialize, Deserialize)]
struct SerializedRange {
    from: ValueKind,
    to: ValueKind,
}

impl Serialize for ValueKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            ValueKind::Number(rational) => {
                map.serialize_entry(
                    "number",
                    &crate::literals::rational_to_serialized_str(rational)
                        .map_err(serde::ser::Error::custom)?,
                )?;
            }
            ValueKind::Quantity(rational, unit, _) => {
                map.serialize_entry(
                    "quantity",
                    &SerializedValueUnit {
                        value: crate::literals::rational_to_serialized_str(rational)
                            .map_err(serde::ser::Error::custom)?,
                        unit: unit.clone(),
                    },
                )?;
            }
            ValueKind::Text(s) => {
                map.serialize_entry("text", s)?;
            }
            ValueKind::Date(dt) => {
                map.serialize_entry("date", dt)?;
            }
            ValueKind::Time(t) => {
                map.serialize_entry("time", t)?;
            }
            ValueKind::Boolean(b) => {
                map.serialize_entry("boolean", b)?;
            }
            ValueKind::Calendar(rational, unit) => {
                map.serialize_entry(
                    "calendar",
                    &SerializedValueUnit {
                        value: crate::literals::rational_to_serialized_str(rational)
                            .map_err(serde::ser::Error::custom)?,
                        unit: unit.to_string(),
                    },
                )?;
            }
            ValueKind::Ratio(rational, unit) => {
                map.serialize_entry(
                    "ratio",
                    &SerializedValueUnit {
                        value: crate::literals::rational_to_serialized_str(rational)
                            .map_err(serde::ser::Error::custom)?,
                        unit: unit.clone().unwrap_or_default(),
                    },
                )?;
            }
            ValueKind::Range(left, right) => {
                map.serialize_entry(
                    "range",
                    &SerializedRange {
                        from: left.value.clone(),
                        to: right.value.clone(),
                    },
                )?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ValueKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = <serde_json::Map<String, serde_json::Value>>::deserialize(deserializer)?;
        if map.len() != 1 {
            return Err(serde::de::Error::custom(format!(
                "ValueKind must have exactly one variant key, got {}",
                map.len()
            )));
        }
        let (tag, payload) = map.into_iter().next().expect("BUG: len checked");
        deserialize_value_kind_variant(&tag, payload).map_err(serde::de::Error::custom)
    }
}

fn deserialize_value_kind_variant(
    tag: &str,
    payload: serde_json::Value,
) -> Result<ValueKind, String> {
    match tag {
        "number" => {
            let s = payload
                .as_str()
                .ok_or_else(|| "number must be a JSON string".to_string())?;
            let decimal = decimal_from_serialized_str(s)?;
            Ok(ValueKind::Number(
                crate::literals::rational_from_parsed_decimal(decimal)?,
            ))
        }
        "quantity" => {
            let pair: SerializedValueUnit =
                serde_json::from_value(payload).map_err(|e| e.to_string())?;
            let decimal = decimal_from_serialized_str(&pair.value)?;
            Ok(ValueKind::Quantity(
                crate::literals::rational_from_parsed_decimal(decimal)?,
                pair.unit,
                BaseQuantityVector::new(),
            ))
        }
        "ratio" => {
            let pair: SerializedValueUnit =
                serde_json::from_value(payload).map_err(|e| e.to_string())?;
            let unit = if pair.unit.is_empty() {
                None
            } else {
                Some(pair.unit)
            };
            let decimal = decimal_from_serialized_str(&pair.value)?;
            Ok(ValueKind::Ratio(
                crate::literals::rational_from_parsed_decimal(decimal)?,
                unit,
            ))
        }
        "calendar" => {
            let pair: SerializedValueUnit =
                serde_json::from_value(payload).map_err(|e| e.to_string())?;
            let unit = match pair.unit.as_str() {
                "months" => SemanticCalendarUnit::Month,
                "years" => SemanticCalendarUnit::Year,
                other => {
                    return Err(format!(
                        "unknown calendar unit '{other}' (expected 'months' or 'years')"
                    ));
                }
            };
            let decimal = decimal_from_serialized_str(&pair.value)?;
            Ok(ValueKind::Calendar(
                crate::literals::rational_from_parsed_decimal(decimal)?,
                unit,
            ))
        }
        "text" => {
            let s = payload
                .as_str()
                .ok_or_else(|| "text must be a JSON string".to_string())?;
            Ok(ValueKind::Text(s.to_string()))
        }
        "date" => {
            let dt: SemanticDateTime =
                serde_json::from_value(payload).map_err(|e| e.to_string())?;
            Ok(ValueKind::Date(dt))
        }
        "time" => {
            let t: SemanticTime = serde_json::from_value(payload).map_err(|e| e.to_string())?;
            Ok(ValueKind::Time(t))
        }
        "boolean" => {
            let b = payload
                .as_bool()
                .ok_or_else(|| "boolean must be a JSON bool".to_string())?;
            Ok(ValueKind::Boolean(b))
        }
        "range" => {
            let range: SerializedRange =
                serde_json::from_value(payload).map_err(|e| e.to_string())?;
            Ok(ValueKind::Range(
                Box::new(LiteralValue {
                    value: range.from,
                    lemma_type: primitive_number().clone(),
                }),
                Box::new(LiteralValue {
                    value: range.to,
                    lemma_type: primitive_number().clone(),
                }),
            ))
        }
        other => Err(format!("unknown ValueKind variant '{other}'")),
    }
}

// -----------------------------------------------------------------------------
// Resolved path types (moved from parsing::ast)
// -----------------------------------------------------------------------------

/// A single segment in a resolved path traversal
///
/// Used in both DataPath and RulePath for cross-spec traversal.
/// Each segment contains a data name that resolves to another spec.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PathSegment {
    /// The data name in this segment
    pub data: String,
    /// The spec this data references (resolved during planning)
    pub spec: String,
}

/// Resolved path to a data (created during planning from AST DataReference)
///
/// Represents a fully resolved path through specs to reach a datum.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DataPath {
    /// Path segments (each is a cross-spec step)
    pub segments: Vec<PathSegment>,
    /// Final data name
    pub data: String,
}

impl DataPath {
    /// Create a data path from segments and data name (matches AST DataReference shape)
    pub fn new(segments: Vec<PathSegment>, data: String) -> Self {
        Self { segments, data }
    }

    /// Create a local data path (no cross-spec steps)
    pub fn local(data: String) -> Self {
        Self {
            segments: vec![],
            data,
        }
    }

    /// Dot-separated key used for matching user-provided data values (e.g. `"order.payment_method"`).
    /// Unlike `Display`, this omits the resolved spec name.
    pub fn input_key(&self) -> String {
        let mut s = String::new();
        for segment in &self.segments {
            s.push_str(&segment.data);
            s.push('.');
        }
        s.push_str(&self.data);
        s
    }
}

/// Resolved path to a rule (created during planning from RuleReference)
///
/// Represents a fully resolved path through specs to reach a rule.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RulePath {
    /// Path segments (each is a cross-spec step)
    pub segments: Vec<PathSegment>,
    /// Final rule name
    pub rule: String,
}

impl RulePath {
    /// Create a rule path from segments and rule name (matches AST RuleReference shape)
    pub fn new(segments: Vec<PathSegment>, rule: String) -> Self {
        Self { segments, rule }
    }
}

// -----------------------------------------------------------------------------
// Resolved expression types (created during planning)
// -----------------------------------------------------------------------------

/// Resolved expression (all references resolved to paths, all literals typed)
///
/// Created during planning from AST Expression. All unresolved references
/// are converted to DataPath/RulePath, and all literals are typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub source_location: Option<Source>,
}

impl Expression {
    pub fn new(kind: ExpressionKind, source_location: Source) -> Self {
        Self {
            kind,
            source_location: Some(source_location),
        }
    }

    /// Create an expression with an optional source location
    pub fn with_source(kind: ExpressionKind, source_location: Option<Source>) -> Self {
        Self {
            kind,
            source_location,
        }
    }

    /// Collect all DataPath references from this resolved expression tree
    pub fn collect_data_paths(&self, data: &mut std::collections::HashSet<DataPath>) {
        self.kind.collect_data_paths(data);
    }
}

/// Resolved expression kind (only resolved variants, no unresolved references)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionKind {
    /// Resolved literal with type (boxed to keep enum small)
    Literal(Box<LiteralValue>),
    /// Resolved data path
    DataPath(DataPath),
    /// Resolved rule path
    RulePath(RulePath),
    LogicalAnd(Arc<Expression>, Arc<Expression>),
    LogicalOr(Arc<Expression>, Arc<Expression>),
    Arithmetic(Arc<Expression>, ArithmeticComputation, Arc<Expression>),
    Comparison(Arc<Expression>, ComparisonComputation, Arc<Expression>),
    UnitConversion(Arc<Expression>, SemanticConversionTarget),
    LogicalNegation(Arc<Expression>, NegationType),
    MathematicalComputation(MathematicalComputation, Arc<Expression>),
    Veto(VetoExpression),
    /// The `now` keyword — resolved at evaluation to the effective datetime.
    Now,
    /// Date-relative sugar: `<date_expr> in past` / `in future`
    DateRelative(DateRelativeKind, Arc<Expression>),
    /// Calendar-period sugar: `<date_expr> in [past|future] calendar year|month|week`
    DateCalendar(DateCalendarKind, CalendarPeriodUnit, Arc<Expression>),
    RangeLiteral(Arc<Expression>, Arc<Expression>),
    PastFutureRange(DateRelativeKind, Arc<Expression>),
    RangeContainment(Arc<Expression>, Arc<Expression>),
    /// Whether evaluating the operand produced a veto (no value). Parses as `is veto` syntax.
    ResultIsVeto(Arc<Expression>),
}

impl ExpressionKind {
    /// Collect all DataPath references from this expression kind
    pub(crate) fn collect_data_paths(&self, data: &mut std::collections::HashSet<DataPath>) {
        match self {
            ExpressionKind::DataPath(fp) => {
                data.insert(fp.clone());
            }
            ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
                left.collect_data_paths(data);
                right.collect_data_paths(data);
            }
            ExpressionKind::Arithmetic(left, _, right)
            | ExpressionKind::Comparison(left, _, right)
            | ExpressionKind::RangeLiteral(left, right)
            | ExpressionKind::RangeContainment(left, right) => {
                left.collect_data_paths(data);
                right.collect_data_paths(data);
            }
            ExpressionKind::UnitConversion(inner, _)
            | ExpressionKind::LogicalNegation(inner, _)
            | ExpressionKind::MathematicalComputation(_, inner)
            | ExpressionKind::PastFutureRange(_, inner) => {
                inner.collect_data_paths(data);
            }
            ExpressionKind::DateRelative(_, date_expr) => {
                date_expr.collect_data_paths(data);
            }
            ExpressionKind::DateCalendar(_, _, date_expr) => {
                date_expr.collect_data_paths(data);
            }
            ExpressionKind::Literal(_)
            | ExpressionKind::RulePath(_)
            | ExpressionKind::Veto(_)
            | ExpressionKind::Now => {}
            ExpressionKind::ResultIsVeto(operand) => {
                operand.collect_data_paths(data);
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Resolved types and values
// -----------------------------------------------------------------------------

/// Where the custom extension chain is rooted: same spec as this type, or imported from another resolved spec.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeDefiningSpec {
    /// Parent type is defined in the same spec as this type.
    Local,
    /// Parent type was resolved from types loaded from this dependency.
    Import { spec: Arc<LemmaSpec> },
}

/// What this type extends (primitive built-in or custom type by name).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeExtends {
    /// Extends a primitive built-in type (number, boolean, text, etc.)
    Primitive,
    /// Extends a custom type: parent is the immediate parent type name; family is the root of the extension chain (topmost custom type name).
    /// `defining_spec` records whether the parent chain is local or imported from another spec.
    Custom {
        parent: String,
        family: String,
        defining_spec: TypeDefiningSpec,
    },
}

impl PartialEq for TypeExtends {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TypeExtends::Primitive, TypeExtends::Primitive) => true,
            (
                TypeExtends::Custom {
                    parent: lp,
                    family: lf,
                    defining_spec: ld,
                },
                TypeExtends::Custom {
                    parent: rp,
                    family: rf,
                    defining_spec: rd,
                },
            ) => {
                lp == rp
                    && lf == rf
                    && match (ld, rd) {
                        (TypeDefiningSpec::Local, TypeDefiningSpec::Local) => true,
                        (
                            TypeDefiningSpec::Import { spec: left },
                            TypeDefiningSpec::Import { spec: right },
                        ) => Arc::ptr_eq(left, right),
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

impl Eq for TypeExtends {}

impl std::hash::Hash for TypeDefiningSpec {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            TypeDefiningSpec::Local => {
                0u8.hash(state);
            }
            TypeDefiningSpec::Import { spec } => {
                1u8.hash(state);
                Arc::as_ptr(spec).hash(state);
            }
        }
    }
}

impl std::hash::Hash for TypeExtends {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            TypeExtends::Primitive => {
                0u8.hash(state);
            }
            TypeExtends::Custom {
                parent,
                family,
                defining_spec,
            } => {
                1u8.hash(state);
                parent.hash(state);
                family.hash(state);
                defining_spec.hash(state);
            }
        }
    }
}

impl TypeExtends {
    /// Custom extension in the same spec as the defining type (no cross-spec import for the parent chain).
    #[must_use]
    pub fn custom_local(parent: String, family: String) -> Self {
        TypeExtends::Custom {
            parent,
            family,
            defining_spec: TypeDefiningSpec::Local,
        }
    }

    /// Returns the parent type name if this type extends a custom type.
    #[must_use]
    pub fn parent_name(&self) -> Option<&str> {
        match self {
            TypeExtends::Primitive => None,
            TypeExtends::Custom { parent, .. } => Some(parent.as_str()),
        }
    }
}

/// Resolved type after planning
///
/// Contains a type specification and optional name. Created during planning
/// from TypeSpecification in the AST.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LemmaType {
    /// Optional type name (e.g., "age", "temperature")
    pub name: Option<String>,
    /// The type specification (Boolean, Number, Quantity, etc.).
    /// Serialized as a discriminated union: the variant tag appears as
    /// `"kind"` alongside `name` and `extends`, and the variant's fields
    /// are flattened to the top level.
    #[serde(flatten)]
    pub specifications: TypeSpecification,
    /// What this type extends (primitive or custom from a spec)
    pub extends: TypeExtends,
}

impl LemmaType {
    /// Create a new type with a name
    pub fn new(name: String, specifications: TypeSpecification, extends: TypeExtends) -> Self {
        Self {
            name: Some(name),
            specifications,
            extends,
        }
    }

    /// Create a type without a name (anonymous/inline type)
    pub fn without_name(specifications: TypeSpecification, extends: TypeExtends) -> Self {
        Self {
            name: None,
            specifications,
            extends,
        }
    }

    /// Create a primitive type (no name, extends Primitive)
    pub fn primitive(specifications: TypeSpecification) -> Self {
        Self {
            name: None,
            specifications,
            extends: TypeExtends::Primitive,
        }
    }

    /// Get the type name, or a default based on the type specification
    pub fn name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            match &self.specifications {
                TypeSpecification::Boolean { .. } => "boolean",
                TypeSpecification::Quantity { .. } => "quantity",
                TypeSpecification::QuantityRange { .. } => "quantity range",
                TypeSpecification::Number { .. } => "number",
                TypeSpecification::NumberRange { .. } => "number range",
                TypeSpecification::Text { .. } => "text",
                TypeSpecification::Date { .. } => "date",
                TypeSpecification::DateRange { .. } => "date range",
                TypeSpecification::Time { .. } => "time",
                TypeSpecification::Calendar { .. } => "calendar",
                TypeSpecification::CalendarRange { .. } => "calendar range",
                TypeSpecification::Ratio { .. } => "ratio",
                TypeSpecification::RatioRange { .. } => "ratio range",
                TypeSpecification::Veto { .. } => "veto",
                TypeSpecification::Undetermined => "undetermined",
            }
            .to_string()
        })
    }

    /// Check if this type is boolean
    pub fn is_boolean(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Boolean { .. })
    }

    /// Check if this type is quantity
    pub fn is_quantity(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Quantity { .. })
    }

    pub fn is_quantity_range(&self) -> bool {
        matches!(
            &self.specifications,
            TypeSpecification::QuantityRange { .. }
        )
    }

    /// Check if this type is number (dimensionless)
    pub fn is_number(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Number { .. })
    }

    pub fn is_number_range(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::NumberRange { .. })
    }

    /// Check if this type is numeric (either quantity or number)
    pub fn is_numeric(&self) -> bool {
        matches!(
            &self.specifications,
            TypeSpecification::Quantity { .. } | TypeSpecification::Number { .. }
        )
    }

    /// Check if this type is text
    pub fn is_text(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Text { .. })
    }

    /// Check if this type is date
    pub fn is_date(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Date { .. })
    }

    pub fn is_date_range(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::DateRange { .. })
    }

    /// Check if this type is time
    pub fn is_time(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Time { .. })
    }

    pub fn has_trait_duration(&self) -> bool {
        match &self.specifications {
            TypeSpecification::Quantity { traits, .. } => traits.contains(&QuantityTrait::Duration),
            _ => false,
        }
    }

    pub fn is_duration_like_quantity(&self) -> bool {
        if !self.is_quantity() {
            return false;
        }
        if self.has_trait_duration() {
            return true;
        }
        self.is_anonymous_quantity()
            && self.quantity_type_decomposition() == &duration_decomposition()
    }

    pub fn is_duration_like(&self) -> bool {
        self.is_duration_like_quantity()
    }

    /// Check if this type is calendar
    pub fn is_calendar(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Calendar { .. })
    }

    /// Check if this type is ratio
    pub fn is_ratio(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Ratio { .. })
    }

    pub fn is_ratio_range(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::RatioRange { .. })
    }

    pub fn is_calendar_range(&self) -> bool {
        matches!(
            &self.specifications,
            TypeSpecification::CalendarRange { .. }
        )
    }

    pub fn is_range(&self) -> bool {
        matches!(
            &self.specifications,
            TypeSpecification::DateRange { .. }
                | TypeSpecification::NumberRange { .. }
                | TypeSpecification::QuantityRange { .. }
                | TypeSpecification::RatioRange { .. }
                | TypeSpecification::CalendarRange { .. }
        )
    }

    /// Check if this type is veto
    pub fn vetoed(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Veto { .. })
    }

    /// True if this type is the undetermined sentinel (type could not be inferred).
    pub fn is_undetermined(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Undetermined)
    }

    /// Check if two types have the same base type specification (ignoring constraints)
    pub fn has_same_base_type(&self, other: &LemmaType) -> bool {
        use TypeSpecification::*;
        matches!(
            (&self.specifications, &other.specifications),
            (Boolean { .. }, Boolean { .. })
                | (Number { .. }, Number { .. })
                | (NumberRange { .. }, NumberRange { .. })
                | (Quantity { .. }, Quantity { .. })
                | (QuantityRange { .. }, QuantityRange { .. })
                | (Text { .. }, Text { .. })
                | (Date { .. }, Date { .. })
                | (DateRange { .. }, DateRange { .. })
                | (Time { .. }, Time { .. })
                | (Calendar { .. }, Calendar { .. })
                | (CalendarRange { .. }, CalendarRange { .. })
                | (Ratio { .. }, Ratio { .. })
                | (RatioRange { .. }, RatioRange { .. })
                | (Veto { .. }, Veto { .. })
                | (Undetermined, Undetermined)
        )
    }

    /// For quantity types, returns the family name (root of the extension chain). For Custom extends, returns the family field; for Primitive, returns the type's own name (the type is the root). For non-quantity types, returns None.
    #[must_use]
    pub fn quantity_family_name(&self) -> Option<&str> {
        if !self.is_quantity() {
            return None;
        }
        match &self.extends {
            TypeExtends::Custom { family, .. } => Some(family.as_str()),
            TypeExtends::Primitive => self.name.as_deref(),
        }
    }

    /// Returns true if both types are quantity and belong to the same named quantity family.
    #[must_use]
    pub fn same_quantity_family(&self, other: &LemmaType) -> bool {
        if !self.is_quantity() || !other.is_quantity() {
            return false;
        }
        match (self.quantity_family_name(), other.quantity_family_name()) {
            (Some(self_family), Some(other_family)) => self_family == other_family,
            _ => false,
        }
    }

    #[must_use]
    pub fn compatible_with_anonymous_quantity(&self, other: &LemmaType) -> bool {
        if !self.is_quantity() || !other.is_quantity() {
            return false;
        }
        if !self.is_anonymous_quantity() && !other.is_anonymous_quantity() {
            return false;
        }
        let self_decomposition = self.quantity_type_decomposition();
        let other_decomposition = other.quantity_type_decomposition();
        !self_decomposition.is_empty() && self_decomposition == other_decomposition
    }

    /// Create a Veto LemmaType
    pub fn veto_type() -> Self {
        Self::primitive(TypeSpecification::veto())
    }

    /// LemmaType sentinel for undetermined type (used during inference when a type cannot be determined).
    /// Propagates through expressions and is never present in a validated graph.
    pub fn undetermined_type() -> Self {
        Self::primitive(TypeSpecification::Undetermined)
    }

    /// Decimal places for display (Number, Quantity, and Ratio). Used by formatters.
    /// Ratio: optional, no default; when None display is normalized (no trailing zeros).
    pub fn decimal_places(&self) -> Option<u8> {
        match &self.specifications {
            TypeSpecification::Number { decimals, .. } => *decimals,
            TypeSpecification::Quantity { decimals, .. } => *decimals,
            TypeSpecification::Ratio { decimals, .. } => *decimals,
            _ => None,
        }
    }

    /// Get an example value string for this type, suitable for UI help text
    pub fn example_value(&self) -> &'static str {
        match &self.specifications {
            TypeSpecification::Text { .. } => "\"hello world\"",
            TypeSpecification::Quantity { .. } => "12.50 eur",
            TypeSpecification::QuantityRange { .. } => "30 kilogram...35 kilogram",
            TypeSpecification::Number { .. } => "3.14",
            TypeSpecification::NumberRange { .. } => "0...100",
            TypeSpecification::Boolean { .. } => "true",
            TypeSpecification::Date { .. } => "2023-12-25T14:30:00Z",
            TypeSpecification::DateRange { .. } => "2024-01-01...2024-12-31",
            TypeSpecification::Veto { .. } => "veto",
            TypeSpecification::Time { .. } => "14:30:00",
            TypeSpecification::Calendar { .. } => "6 months",
            TypeSpecification::CalendarRange { .. } => "18 years...67 years",
            TypeSpecification::Ratio { .. } => "50%",
            TypeSpecification::RatioRange { .. } => "10%...50%",
            TypeSpecification::Undetermined => unreachable!(
                "BUG: example_value called on Undetermined sentinel type; this type must never reach user-facing code"
            ),
        }
    }

    /// Factor for a unit of this quantity type (for unit conversion during evaluation only).
    /// Planning must validate conversions first and return Error for invalid units.
    /// If called with a non-quantity type or unknown unit name, panics (invariant violation).
    #[must_use]
    /// Returns the `BaseQuantityVector` for Quantity types.
    /// For base quantitys (after decomposition pass) this is `{type_name: 1}`.
    /// For derived quantitys it is the combined dimensional vector.
    /// Panics if called on non-Quantity types.
    pub fn quantity_type_decomposition(&self) -> &BaseQuantityVector {
        match &self.specifications {
            TypeSpecification::Quantity { decomposition, .. } => decomposition,
            _ => unreachable!(
                "BUG: quantity_type_decomposition called on non-quantity type {}",
                self.name()
            ),
        }
    }

    /// Returns true if this is an anonymous (no-name) Quantity — i.e. an anonymous
    /// intermediate produced by cross-axis arithmetic.
    pub fn is_anonymous_quantity(&self) -> bool {
        self.name.is_none() && matches!(&self.specifications, TypeSpecification::Quantity { .. })
    }

    /// Build an anonymous `LemmaType` for a given dimensional decomposition.
    /// Used at plan time to represent the inferred type of cross-axis intermediates.
    pub fn anonymous_for_decomposition(decomposition: BaseQuantityVector) -> Self {
        Self {
            name: None,
            specifications: TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: crate::literals::QuantityUnits::new(),
                traits: Vec::new(),
                decomposition,
                canonical_unit: String::new(),
                help: String::new(),
            },
            extends: TypeExtends::Primitive,
        }
    }

    /// Declared unit names for a named quantity type (`None` for non-quantity or anonymous quantity).
    #[must_use]
    pub fn quantity_unit_names(&self) -> Option<Vec<&str>> {
        if !self.is_quantity() || self.is_anonymous_quantity() {
            return None;
        }
        match &self.specifications {
            TypeSpecification::Quantity { units, .. } => {
                Some(units.iter().map(|unit| unit.name.as_str()).collect())
            }
            _ => None,
        }
    }

    /// Whether a value of this type may be expressed in `target_unit` (typed named quantity only).
    ///
    /// Used by planning (`as` on typed quantity operands) and evaluation API rule-result conversion.
    pub fn validate_quantity_result_unit(&self, target_unit: &str) -> Result<(), String> {
        let target_unit =
            crate::parsing::ast::ascii_lowercase_logical_name(target_unit.to_string());
        let units = match &self.specifications {
            TypeSpecification::Quantity { units, .. } => units,
            _ => {
                return Err(format!(
                    "Cannot convert {} to quantity unit '{}'.",
                    self.name(),
                    target_unit
                ));
            }
        };
        if self.is_anonymous_quantity() {
            return Err(format!(
                "Cannot convert {} to quantity unit '{}'.",
                self.name(),
                target_unit
            ));
        }
        let matched = units.get(&target_unit)?;
        if crate::computation::rational::rational_is_zero(&matched.factor) {
            return Err(format!(
                "Unit '{}' has a zero conversion factor in quantity type {}.",
                matched.name,
                self.name()
            ));
        }
        Ok(())
    }

    fn validate_ratio_result_unit(&self, target_unit: &str) -> Result<(), String> {
        let target_unit =
            crate::parsing::ast::ascii_lowercase_logical_name(target_unit.to_string());
        let units = match &self.specifications {
            TypeSpecification::Ratio { units, .. } => units,
            _ => {
                return Err(format!(
                    "Cannot convert {} to ratio unit '{}'.",
                    self.name(),
                    target_unit
                ));
            }
        };
        let matched = units.get(&target_unit)?;
        if crate::computation::rational::rational_is_zero(&matched.value) {
            return Err(format!(
                "Unit '{}' has a zero conversion value in ratio type {}.",
                matched.name,
                self.name()
            ));
        }
        Ok(())
    }

    /// Whether `source_type` may be converted to `target_unit` for `as` / API rule-result display.
    ///
    /// Mirrors planning [`check_unit_conversion_types`](crate::planning::graph) for quantity- and
    /// ratio-unit targets. Callers must reject impossible conversions before evaluation.
    pub fn validate_rule_result_unit_conversion(
        &self,
        target_unit: &str,
        unit_index: &std::collections::HashMap<String, LemmaType>,
        spec_name: &str,
    ) -> Result<SemanticConversionTarget, String> {
        if self.is_ratio() {
            self.validate_ratio_result_unit(target_unit)?;
            match unit_index.get(target_unit) {
                Some(target_type) if target_type.is_ratio() => {}
                Some(_) => {
                    return Err(format!(
                        "Unit '{}' does not belong to a ratio type.",
                        target_unit
                    ));
                }
                None => {
                    return Err(format!(
                        "Unknown unit '{}': no ratio type in spec '{}' owns this unit.",
                        target_unit, spec_name
                    ));
                }
            }
            return Ok(SemanticConversionTarget::RatioUnit(target_unit.to_string()));
        }

        if !self.is_quantity() {
            return Err(format!(
                "Cannot convert {} to unit '{}': requires quantity or ratio result type.",
                self.name(),
                target_unit
            ));
        }

        if self.is_anonymous_quantity() {
            let target_type = unit_index.get(target_unit).ok_or_else(|| {
                format!(
                    "Unknown unit '{}': no quantity type in spec '{}' owns this unit.",
                    target_unit, spec_name
                )
            })?;
            let source_decomp = self.quantity_type_decomposition();
            let target_decomp = match &target_type.specifications {
                TypeSpecification::Quantity { decomposition, .. } => decomposition,
                _ => {
                    return Err(format!(
                        "Unit '{}' does not belong to a quantity type.",
                        target_unit
                    ));
                }
            };
            if source_decomp != target_decomp {
                let target_quantity_family = target_type
                    .quantity_family_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| target_type.name().to_string());
                return Err(format!(
                    "Cannot cast to '{}' (quantity '{}'): source dimensions {:?} do not \
                     match target dimensions {:?}. The intermediate result has a different \
                     physical quantity than the target type.",
                    target_unit, target_quantity_family, source_decomp, target_decomp
                ));
            }
            target_type.validate_quantity_result_unit(target_unit)?;
            return Ok(SemanticConversionTarget::QuantityUnit(
                target_unit.to_string(),
            ));
        }

        self.validate_quantity_result_unit(target_unit)?;
        if unit_index.get(target_unit).is_none() {
            return Err(format!(
                "Unknown unit '{}': no quantity type in spec '{}' owns this unit.",
                target_unit, spec_name
            ));
        }
        Ok(SemanticConversionTarget::QuantityUnit(
            target_unit.to_string(),
        ))
    }

    pub fn quantity_unit_factor(
        &self,
        unit_name: &str,
    ) -> &crate::computation::rational::RationalInteger {
        use crate::computation::rational::rational_one;
        use std::sync::LazyLock;
        static EMPTY_UNIT_FACTOR: LazyLock<crate::computation::rational::RationalInteger> =
            LazyLock::new(rational_one);
        if unit_name.is_empty() {
            return &EMPTY_UNIT_FACTOR;
        }
        let units = match &self.specifications {
            TypeSpecification::Quantity { units, .. } => units,
            _ => unreachable!(
                "BUG: quantity_unit_factor called with non-quantity type {}; only call during evaluation after planning validated quantity conversion",
                self.name()
            ),
        };
        match units.get(unit_name) {
            Ok(QuantityUnit { factor, .. }) => factor,
            Err(_) => {
                let valid: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
                unreachable!(
                    "BUG: unknown unit '{}' for quantity type {} (valid: {}); planning must reject invalid conversions with Error",
                    unit_name,
                    self.name(),
                    valid.join(", ")
                );
            }
        }
    }

    pub fn ratio_unit_factor(
        &self,
        unit_name: &str,
    ) -> &crate::computation::rational::RationalInteger {
        let units = match &self.specifications {
            TypeSpecification::Ratio { units, .. } => units,
            _ => unreachable!(
                "BUG: ratio_unit_factor called with non-ratio type {}; only call during evaluation after planning validated ratio conversion",
                self.name()
            ),
        };
        match units.get(unit_name) {
            Ok(RatioUnit { value, .. }) => value,
            Err(_) => {
                let valid: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
                unreachable!(
                    "BUG: unknown unit '{}' for ratio type {} (valid: {}); planning must reject invalid conversions with Error",
                    unit_name,
                    self.name(),
                    valid.join(", ")
                );
            }
        }
    }
}

/// Literal value with type. The single value type in semantics.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct LiteralValue {
    pub value: ValueKind,
    pub lemma_type: LemmaType,
}

impl Serialize for LiteralValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("LiteralValue", 3)?;
        state.serialize_field("value", &self.value)?;
        state.serialize_field("lemma_type", &self.lemma_type)?;
        state.serialize_field("display_value", &self.display_value())?;
        state.end()
    }
}

impl LiteralValue {
    pub fn text(s: String) -> Self {
        Self {
            value: ValueKind::Text(s),
            lemma_type: primitive_text().clone(),
        }
    }

    pub fn text_with_type(s: String, lemma_type: LemmaType) -> Self {
        Self {
            value: ValueKind::Text(s),
            lemma_type,
        }
    }

    pub fn number(n: RationalInteger) -> Self {
        Self {
            value: ValueKind::Number(n),
            lemma_type: primitive_number().clone(),
        }
    }

    pub fn number_from_decimal(decimal: Decimal) -> Self {
        Self::number(
            crate::literals::rational_from_parsed_decimal(decimal)
                .expect("BUG: literal number from decimal must lift at boundary"),
        )
    }

    pub fn number_with_type(n: RationalInteger, lemma_type: LemmaType) -> Self {
        Self {
            value: ValueKind::Number(n),
            lemma_type,
        }
    }

    pub fn number_with_type_from_decimal(decimal: Decimal, lemma_type: LemmaType) -> Self {
        Self::number_with_type(
            crate::literals::rational_from_parsed_decimal(decimal)
                .expect("BUG: literal number from decimal must lift at boundary"),
            lemma_type,
        )
    }

    pub fn quantity_with_type(n: RationalInteger, unit: String, lemma_type: LemmaType) -> Self {
        Self {
            value: ValueKind::Quantity(n, unit, BaseQuantityVector::new()),
            lemma_type,
        }
    }

    /// Create an anonymous intermediate Quantity value with a non-empty decomposition.
    /// Used by cross-axis arithmetic to represent dimensioned values without a named typedef.
    pub fn quantity_anonymous(n: RationalInteger, decomposition: BaseQuantityVector) -> Self {
        let lemma_type = LemmaType {
            name: None,
            specifications: TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: crate::literals::QuantityUnits::new(),
                traits: Vec::new(),
                decomposition: decomposition.clone(),
                canonical_unit: String::new(),
                help: String::new(),
            },
            extends: TypeExtends::Primitive,
        };
        Self {
            value: ValueKind::Quantity(n, String::new(), decomposition),
            lemma_type,
        }
    }

    /// Number interpreted as a quantity value in the given unit (e.g. "3 as usd" where 3 is a number).
    /// Creates an anonymous one-unit quantity type so computation does not depend on parsing types.
    pub fn number_interpreted_as_quantity(value: RationalInteger, unit_name: String) -> Self {
        let lemma_type = LemmaType {
            name: None,
            specifications: TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: QuantityUnits::from(vec![QuantityUnit {
                    name: unit_name.clone(),
                    factor: crate::computation::rational::rational_one(),
                    derived_quantity_factors: Vec::new(),
                    decomposition: BaseQuantityVector::new(),
                    minimum: None,
                    maximum: None,
                    default_magnitude: None,
                }]),
                traits: Vec::new(),
                decomposition: BaseQuantityVector::new(),
                canonical_unit: unit_name.clone(),
                help: default_help_for_primitive(PrimitiveKind::Quantity).to_string(),
            },
            extends: TypeExtends::Primitive,
        };
        Self {
            value: ValueKind::Quantity(value, unit_name, BaseQuantityVector::new()),
            lemma_type,
        }
    }

    pub fn from_bool(b: bool) -> Self {
        Self {
            value: ValueKind::Boolean(b),
            lemma_type: primitive_boolean().clone(),
        }
    }

    pub fn date(dt: SemanticDateTime) -> Self {
        Self {
            value: ValueKind::Date(dt),
            lemma_type: primitive_date().clone(),
        }
    }

    pub fn date_with_type(dt: SemanticDateTime, lemma_type: LemmaType) -> Self {
        Self {
            value: ValueKind::Date(dt),
            lemma_type,
        }
    }

    pub fn time(t: SemanticTime) -> Self {
        Self {
            value: ValueKind::Time(t),
            lemma_type: primitive_time().clone(),
        }
    }

    pub fn time_with_type(t: SemanticTime, lemma_type: LemmaType) -> Self {
        Self {
            value: ValueKind::Time(t),
            lemma_type,
        }
    }

    pub fn calendar(value: RationalInteger, unit: SemanticCalendarUnit) -> Self {
        Self {
            value: ValueKind::Calendar(value, unit),
            lemma_type: primitive_calendar().clone(),
        }
    }

    pub fn calendar_from_decimal(value: Decimal, unit: SemanticCalendarUnit) -> Self {
        Self::calendar(
            crate::literals::rational_from_parsed_decimal(value)
                .expect("BUG: calendar literal from decimal must lift at boundary"),
            unit,
        )
    }

    pub fn calendar_with_type(
        value: RationalInteger,
        unit: SemanticCalendarUnit,
        lemma_type: LemmaType,
    ) -> Self {
        Self {
            value: ValueKind::Calendar(value, unit),
            lemma_type,
        }
    }

    pub fn ratio(r: RationalInteger, unit: Option<String>) -> Self {
        Self {
            value: ValueKind::Ratio(r, unit),
            lemma_type: primitive_ratio().clone(),
        }
    }

    pub fn ratio_from_decimal(r: Decimal, unit: Option<String>) -> Self {
        Self::ratio(
            crate::literals::rational_from_parsed_decimal(r)
                .expect("BUG: ratio literal from decimal must lift at boundary"),
            unit,
        )
    }

    pub fn ratio_with_type(
        r: RationalInteger,
        unit: Option<String>,
        lemma_type: LemmaType,
    ) -> Self {
        Self {
            value: ValueKind::Ratio(r, unit),
            lemma_type,
        }
    }

    pub fn range(left: LiteralValue, right: LiteralValue) -> Self {
        let specifications = match (
            &left.lemma_type.specifications,
            &right.lemma_type.specifications,
        ) {
            (TypeSpecification::Date { .. }, TypeSpecification::Date { .. }) => {
                TypeSpecification::date_range()
            }
            (TypeSpecification::Number { .. }, TypeSpecification::Number { .. }) => {
                TypeSpecification::number_range()
            }
            (
                TypeSpecification::Quantity {
                    units,
                    decomposition,
                    canonical_unit,
                    ..
                },
                TypeSpecification::Quantity { .. },
            ) if left.lemma_type.same_quantity_family(&right.lemma_type) => {
                let mut spec = TypeSpecification::quantity_range();
                if let TypeSpecification::QuantityRange {
                    units: range_units,
                    decomposition: range_decomposition,
                    canonical_unit: range_canonical_unit,
                    ..
                } = &mut spec
                {
                    *range_units = units.clone();
                    *range_decomposition = decomposition.clone();
                    *range_canonical_unit = canonical_unit.clone();
                }
                spec
            }
            (TypeSpecification::Ratio { units, .. }, TypeSpecification::Ratio { .. }) => {
                let mut spec = TypeSpecification::ratio_range();
                if let TypeSpecification::RatioRange {
                    units: range_units, ..
                } = &mut spec
                {
                    *range_units = units.clone();
                }
                spec
            }
            (TypeSpecification::Calendar { .. }, TypeSpecification::Calendar { .. }) => {
                TypeSpecification::calendar_range()
            }
            _ => unreachable!(
                "BUG: attempted to construct a range literal from incompatible endpoint types"
            ),
        };

        Self {
            value: ValueKind::Range(Box::new(left), Box::new(right)),
            lemma_type: LemmaType::primitive(specifications),
        }
    }

    /// Get a display string for this value (for UI/output)
    pub fn display_value(&self) -> String {
        format!("{}", self)
    }

    /// Approximate byte size for resource limit checks (string representation length)
    pub fn byte_size(&self) -> usize {
        format!("{}", self).len()
    }

    /// Get the resolved type of this literal
    pub fn get_type(&self) -> &LemmaType {
        &self.lemma_type
    }
}

/// Response/UI row for spec data: [`LemmaType`] plus optional bound literal (mirrors parse-time `Definition`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataValue {
    Definition {
        schema_type: LemmaType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bound_value: Option<LiteralValue>,
    },
}

impl DataValue {
    #[must_use]
    pub fn from_bound_literal(value: LiteralValue) -> Self {
        let schema_type = value.get_type().clone();
        Self::Definition {
            schema_type,
            bound_value: Some(value),
        }
    }
}

/// Data: path, value, and source location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Data {
    pub path: DataPath,
    pub value: DataValue,
    pub source: Option<Source>,
}

/// What a [`DataDefinition::Reference`] copies its value from: either another data path
/// or a rule whose result becomes this data's value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ReferenceTarget {
    Data(DataPath),
    Rule(RulePath),
}

/// Resolved data value for the execution plan: aligned with [`DataValue`] but with source per variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataDefinition {
    /// Value-holding data: current value (literal or default); type is on the value.
    Value { value: LiteralValue, source: Source },
    /// Type-only data: schema known, value to be supplied (e.g. via with_values).
    /// `declared_default` carries the `-> default ...` payload for this binding or
    /// the default inherited from the parent type chain, if any; value-promoting code
    /// uses it instead of re-deriving defaults from [`TypeSpecification`].
    TypeDeclaration {
        resolved_type: LemmaType,
        declared_default: Option<ValueKind>,
        source: Source,
    },
    /// Import (`uses`): resolved target lemma for this alias.
    Import {
        spec: Arc<crate::parsing::ast::LemmaSpec>,
        source: Source,
    },
    /// Value-copy reference to another data or a rule result.
    ///
    /// `resolved_type` is the merged type that the copied value must satisfy at
    /// evaluation time. Merging folds together: (1) the LHS's own declared type,
    /// if any; (2) the target's type (data schema type or rule return type);
    /// (3) any `local_constraints` written after the `->` on the reference itself.
    /// Merging happens in a dedicated pass once all data and rule types are
    /// known; before that pass, `resolved_type` holds a provisional value and
    /// must not be consumed for type checking.
    ///
    /// `local_constraints` preserves the raw constraint list from the reference's
    /// `-> ...` tail (e.g. `minimum 5` in `data license2: law.other -> minimum 5`)
    /// for that merging pass. It is `None` when the reference has no trailing
    /// constraints.
    ///
    /// `local_default` carries any `default <value>` constraint from the
    /// reference's `-> ...` tail. The reference-merge pass extracts it from the
    /// constraint list during type resolution. It is materialized into a
    /// concrete value by [`crate::planning::ExecutionPlan::with_defaults`]
    /// before evaluation (or remains a schema suggestion when callers use
    /// [`Engine::run_plan_without_defaults`]).
    ///
    /// The reference itself is evaluated by copying the target's value (data path)
    /// or the target rule's result in topological order; `set_data_values`
    /// entries for a referenced path override the reference with a literal.
    Reference {
        target: ReferenceTarget,
        resolved_type: LemmaType,
        local_constraints: Option<Vec<Constraint>>,
        local_default: Option<ValueKind>,
        source: Source,
    },
}

impl DataDefinition {
    /// Schema type for value, type-declaration, and reference data; `None` for imports.
    pub fn schema_type(&self) -> Option<&LemmaType> {
        match self {
            DataDefinition::Value { value, .. } => Some(&value.lemma_type),
            DataDefinition::TypeDeclaration { resolved_type, .. } => Some(resolved_type),
            DataDefinition::Reference { resolved_type, .. } => Some(resolved_type),
            DataDefinition::Import { .. } => None,
        }
    }

    /// Returns the literal value when the data already holds one. A `Reference`'s
    /// value is produced by the evaluator at runtime, so at plan-time it has no
    /// value yet.
    pub fn value(&self) -> Option<&LiteralValue> {
        match self {
            DataDefinition::Value { value, .. } => Some(value),
            DataDefinition::TypeDeclaration { .. }
            | DataDefinition::Import { .. }
            | DataDefinition::Reference { .. } => None,
        }
    }

    /// Literal explicitly bound in the spec (`data x: literal`) or substituted
    /// by the caller via `set_data_values` as [`DataDefinition::Value`].
    /// Not a suggestion; see [`Self::default_suggestion`].
    #[inline]
    pub fn bound_value(&self) -> Option<&LiteralValue> {
        self.value()
    }

    /// Suggestion from `-> default ...` on a type declaration or reference.
    /// Surfaces in [`crate::planning::execution_plan::DataEntry::default`] for
    /// prefill/UI; omitted from [`Self::bound_value`] until applied via
    /// [`crate::planning::ExecutionPlan::with_defaults`].
    pub fn default_suggestion(&self) -> Option<LiteralValue> {
        match self {
            DataDefinition::TypeDeclaration {
                resolved_type,
                declared_default: Some(dv),
                ..
            } => Some(LiteralValue {
                value: dv.clone(),
                lemma_type: resolved_type.clone(),
            }),
            DataDefinition::Reference {
                resolved_type,
                local_default: Some(dv),
                ..
            } => Some(LiteralValue {
                value: dv.clone(),
                lemma_type: resolved_type.clone(),
            }),
            DataDefinition::Value { .. }
            | DataDefinition::TypeDeclaration { .. }
            | DataDefinition::Reference { .. }
            | DataDefinition::Import { .. } => None,
        }
    }

    /// Returns the source location for this data.
    pub fn source(&self) -> &Source {
        match self {
            DataDefinition::Value { source, .. } => source,
            DataDefinition::TypeDeclaration { source, .. } => source,
            DataDefinition::Import { source, .. } => source,
            DataDefinition::Reference { source, .. } => source,
        }
    }

    /// Returns the reference target when this data copies a value from another
    /// data path or rule result; `None` otherwise.
    pub fn reference_target(&self) -> Option<&ReferenceTarget> {
        match self {
            DataDefinition::Reference { target, .. } => Some(target),
            _ => None,
        }
    }
}

/// Bind a type-agnostic [`Value::NumberWithUnit`] using the unit index entry for `unit_name`.
pub fn number_with_unit_to_value_kind(
    magnitude: rust_decimal::Decimal,
    unit_name: &str,
    lemma_type: &LemmaType,
) -> Result<ValueKind, String> {
    match &lemma_type.specifications {
        TypeSpecification::Ratio { units, .. } => {
            use crate::computation::rational::{checked_div, decimal_to_rational};
            let unit = units.get(unit_name)?;
            let magnitude_rational = decimal_to_rational(magnitude)
                .map_err(|failure| format!("ratio literal failed rational lift: {failure}"))?;
            let canonical_rational = checked_div(&magnitude_rational, &unit.value)
                .map_err(|failure| format!("ratio literal: unit conversion failed: {failure}"))?;
            Ok(ValueKind::Ratio(
                canonical_rational,
                Some(unit.name.clone()),
            ))
        }
        TypeSpecification::Quantity { .. } => Ok(ValueKind::Quantity(
            lift_parser_decimal(magnitude)?,
            unit_name.to_string(),
            BaseQuantityVector::new(),
        )),
        _ => Err(format!(
            "Unit '{}' is defined on type '{}' which is not quantity or ratio",
            unit_name,
            lemma_type.name()
        )),
    }
}

/// Convert parser [`Value`] to [`ValueKind`] using the target type (canonicalizes ratio at bind).
pub fn parser_value_to_value_kind(
    value: &crate::literals::Value,
    type_spec: &TypeSpecification,
) -> Result<ValueKind, String> {
    use crate::literals::Value;
    match (value, type_spec) {
        (Value::NumberWithUnit(magnitude, unit_name), TypeSpecification::Ratio { units, .. }) => {
            use crate::computation::rational::{checked_div, decimal_to_rational};
            let unit = units.get(unit_name.as_str())?;
            let magnitude_rational = decimal_to_rational(*magnitude)
                .map_err(|failure| format!("ratio literal failed rational lift: {failure}"))?;
            let canonical_rational = checked_div(&magnitude_rational, &unit.value)
                .map_err(|failure| format!("ratio literal: unit conversion failed: {failure}"))?;
            Ok(ValueKind::Ratio(
                canonical_rational,
                Some(unit.name.clone()),
            ))
        }
        (Value::NumberWithUnit(magnitude, unit_name), TypeSpecification::Quantity { .. }) => {
            Ok(ValueKind::Quantity(
                lift_parser_decimal(*magnitude)?,
                unit_name.clone(),
                BaseQuantityVector::new(),
            ))
        }
        (Value::NumberWithUnit(_, _), _) => {
            Err("number_with_unit literal requires a quantity or ratio type".to_string())
        }
        _ => value_to_semantic(value),
    }
}

/// Convert parser Value to ValueKind for primitives and ranges only.
///
/// [`Value::NumberWithUnit`] requires [`parser_value_to_value_kind`] with a quantity or ratio type.
pub fn value_to_semantic(value: &crate::parsing::ast::Value) -> Result<ValueKind, String> {
    use crate::parsing::ast::Value;
    Ok(match value {
        Value::Number(n) => ValueKind::Number(lift_parser_decimal(*n)?),
        Value::Text(s) => ValueKind::Text(s.clone()),
        Value::Boolean(b) => ValueKind::Boolean(bool::from(*b)),
        Value::Date(dt) => ValueKind::Date(date_time_to_semantic(dt)),
        Value::Time(t) => ValueKind::Time(time_to_semantic(t)),
        Value::Calendar(n, u) => {
            ValueKind::Calendar(lift_parser_decimal(*n)?, calendar_unit_to_semantic(u))
        }
        Value::NumberWithUnit(_, _) => {
            return Err(
                "number_with_unit literal requires type context (quantity or ratio)".to_string(),
            );
        }
        Value::Range(left, right) => ValueKind::Range(
            Box::new(literal_value_from_parser_value(left)?),
            Box::new(literal_value_from_parser_value(right)?),
        ),
    })
}

/// Convert AST date-time to semantic (for tests and planning).
pub(crate) fn date_time_to_semantic(dt: &crate::parsing::ast::DateTimeValue) -> SemanticDateTime {
    SemanticDateTime {
        year: dt.year,
        month: dt.month,
        day: dt.day,
        hour: dt.hour,
        minute: dt.minute,
        second: dt.second,
        microsecond: dt.microsecond,
        timezone: dt.timezone.as_ref().map(|tz| SemanticTimezone {
            offset_hours: tz.offset_hours,
            offset_minutes: tz.offset_minutes,
        }),
    }
}

/// Convert AST time to semantic (for tests and planning).
pub(crate) fn time_to_semantic(t: &crate::parsing::ast::TimeValue) -> SemanticTime {
    SemanticTime {
        hour: t.hour.into(),
        minute: t.minute.into(),
        second: t.second.into(),
        microsecond: t.microsecond,
        timezone: t.timezone.as_ref().map(|tz| SemanticTimezone {
            offset_hours: tz.offset_hours,
            offset_minutes: tz.offset_minutes,
        }),
    }
}

/// Compare two semantic date-time values by year, month, day, hour, minute,
/// second, then microsecond. Timezone normalisation is a separate concern
/// handled at evaluation time.
pub(crate) fn compare_semantic_dates(
    left: &SemanticDateTime,
    right: &SemanticDateTime,
) -> std::cmp::Ordering {
    left.year
        .cmp(&right.year)
        .then_with(|| left.month.cmp(&right.month))
        .then_with(|| left.day.cmp(&right.day))
        .then_with(|| left.hour.cmp(&right.hour))
        .then_with(|| left.minute.cmp(&right.minute))
        .then_with(|| left.second.cmp(&right.second))
        .then_with(|| left.microsecond.cmp(&right.microsecond))
}

/// Compare two semantic time values by hour, minute, second, then microsecond.
/// Timezone is excluded for the same reason as [`compare_semantic_dates`].
pub(crate) fn compare_semantic_times(
    left: &SemanticTime,
    right: &SemanticTime,
) -> std::cmp::Ordering {
    left.hour
        .cmp(&right.hour)
        .then_with(|| left.minute.cmp(&right.minute))
        .then_with(|| left.second.cmp(&right.second))
        .then_with(|| left.microsecond.cmp(&right.microsecond))
}

pub(crate) fn calendar_unit_to_semantic(
    u: &crate::parsing::ast::CalendarUnit,
) -> SemanticCalendarUnit {
    use crate::parsing::ast::CalendarUnit as CU;
    match u {
        CU::Month => SemanticCalendarUnit::Month,
        CU::Year => SemanticCalendarUnit::Year,
    }
}

/// Convert AST conversion target to semantic (planning boundary; evaluation/computation use only semantic).
///
/// The AST uses [`ConversionTarget::Unit`] for unit names (including duration unit words such as
/// `hours`); this function looks up `name` in the spec's unit index and returns [`SemanticConversionTarget::RatioUnit`]
/// or [`SemanticConversionTarget::QuantityUnit`] based on the type that defines the unit.
pub fn conversion_target_to_semantic(
    ct: &ConversionTarget,
    unit_index: Option<&HashMap<String, LemmaType>>,
) -> Result<SemanticConversionTarget, String> {
    match ct {
        ConversionTarget::Calendar(u) => Ok(SemanticConversionTarget::Calendar(
            calendar_unit_to_semantic(u),
        )),
        ConversionTarget::Type(PrimitiveKind::Number) => Ok(SemanticConversionTarget::Number),
        ConversionTarget::Type(kind) => Err(format!(
            "Type conversion to '{:?}' is not yet supported.",
            kind
        )),
        ConversionTarget::Unit(name) => {
            let index = unit_index.ok_or_else(|| {
                "Unit conversion requires type resolution; unit index not available.".to_string()
            })?;
            let lemma_type = index.get(name).ok_or_else(|| {
                format!(
                    "Unknown unit '{}'. Unit must be defined by a quantity or ratio type.",
                    name
                )
            })?;
            if lemma_type.is_ratio() {
                Ok(SemanticConversionTarget::RatioUnit(name.clone()))
            } else if lemma_type.is_quantity() {
                Ok(SemanticConversionTarget::QuantityUnit(name.clone()))
            } else {
                Err(format!(
                    "Unit '{}' is not a ratio or quantity type; cannot use it in conversion.",
                    name
                ))
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Primitive type constructors (moved from parsing::ast)
// -----------------------------------------------------------------------------

// Private statics for lazy initialization
static PRIMITIVE_BOOLEAN: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_QUANTITY: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_QUANTITY_RANGE: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_NUMBER: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_NUMBER_RANGE: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_TEXT: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_DATE: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_DATE_RANGE: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_TIME: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_CALENDAR: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_RATIO: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_RATIO_RANGE: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_CALENDAR_RANGE: OnceLock<LemmaType> = OnceLock::new();

/// Primitive types use the default TypeSpecification from the parser (single source of truth).
#[must_use]
pub fn primitive_boolean() -> &'static LemmaType {
    PRIMITIVE_BOOLEAN.get_or_init(|| LemmaType::primitive(TypeSpecification::boolean()))
}

#[must_use]
pub fn primitive_quantity() -> &'static LemmaType {
    PRIMITIVE_QUANTITY.get_or_init(|| LemmaType::primitive(TypeSpecification::quantity()))
}

#[must_use]
pub fn primitive_quantity_range() -> &'static LemmaType {
    PRIMITIVE_QUANTITY_RANGE
        .get_or_init(|| LemmaType::primitive(TypeSpecification::quantity_range()))
}

#[must_use]
pub fn primitive_number() -> &'static LemmaType {
    PRIMITIVE_NUMBER.get_or_init(|| LemmaType::primitive(TypeSpecification::number()))
}

#[must_use]
pub fn primitive_number_range() -> &'static LemmaType {
    PRIMITIVE_NUMBER_RANGE.get_or_init(|| LemmaType::primitive(TypeSpecification::number_range()))
}

#[must_use]
pub fn primitive_text() -> &'static LemmaType {
    PRIMITIVE_TEXT.get_or_init(|| LemmaType::primitive(TypeSpecification::text()))
}

#[must_use]
pub fn primitive_date() -> &'static LemmaType {
    PRIMITIVE_DATE.get_or_init(|| LemmaType::primitive(TypeSpecification::date()))
}

#[must_use]
pub fn primitive_date_range() -> &'static LemmaType {
    PRIMITIVE_DATE_RANGE.get_or_init(|| LemmaType::primitive(TypeSpecification::date_range()))
}

#[must_use]
pub fn primitive_time() -> &'static LemmaType {
    PRIMITIVE_TIME.get_or_init(|| LemmaType::primitive(TypeSpecification::time()))
}

#[must_use]
pub fn primitive_calendar() -> &'static LemmaType {
    PRIMITIVE_CALENDAR.get_or_init(|| LemmaType::primitive(TypeSpecification::calendar()))
}

#[must_use]
pub fn primitive_ratio() -> &'static LemmaType {
    PRIMITIVE_RATIO.get_or_init(|| LemmaType::primitive(TypeSpecification::ratio()))
}

#[must_use]
pub fn primitive_ratio_range() -> &'static LemmaType {
    PRIMITIVE_RATIO_RANGE.get_or_init(|| LemmaType::primitive(TypeSpecification::ratio_range()))
}

#[must_use]
pub fn primitive_calendar_range() -> &'static LemmaType {
    PRIMITIVE_CALENDAR_RANGE
        .get_or_init(|| LemmaType::primitive(TypeSpecification::calendar_range()))
}

/// Map PrimitiveKind to TypeSpecification. Single source of truth for primitive type resolution.
#[must_use]
pub fn type_spec_for_primitive(kind: PrimitiveKind) -> TypeSpecification {
    match kind {
        PrimitiveKind::Boolean => TypeSpecification::boolean(),
        PrimitiveKind::Quantity => TypeSpecification::quantity(),
        PrimitiveKind::QuantityRange => TypeSpecification::quantity_range(),
        PrimitiveKind::Number => TypeSpecification::number(),
        PrimitiveKind::NumberRange => TypeSpecification::number_range(),
        PrimitiveKind::Percent | PrimitiveKind::Ratio => TypeSpecification::ratio(),
        PrimitiveKind::RatioRange => TypeSpecification::ratio_range(),
        PrimitiveKind::Text => TypeSpecification::text(),
        PrimitiveKind::Date => TypeSpecification::date(),
        PrimitiveKind::DateRange => TypeSpecification::date_range(),
        PrimitiveKind::Time => TypeSpecification::time(),
        PrimitiveKind::Calendar => TypeSpecification::calendar(),
        PrimitiveKind::CalendarRange => TypeSpecification::calendar_range(),
    }
}

// -----------------------------------------------------------------------------
// Display implementations
// -----------------------------------------------------------------------------

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} → {}", self.data, self.spec)
    }
}

impl fmt::Display for DataPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            write!(f, "{}.", segment)?;
        }
        write!(f, "{}", self.data)
    }
}

impl fmt::Display for RulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            write!(f, "{}.", segment)?;
        }
        write!(f, "{}", self.rule)
    }
}

impl fmt::Display for LemmaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl fmt::Display for LiteralValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::computation::rational::{commit_rational_to_decimal, rational_to_display_str};
        match &self.value {
            ValueKind::Quantity(n, u, _decomp) => {
                if let TypeSpecification::Quantity { decimals, .. } =
                    &self.lemma_type.specifications
                {
                    let s = match commit_rational_to_decimal(n) {
                        Ok(decimal) => match decimals {
                            Some(dp) => {
                                let rounded = decimal.round_dp(u32::from(*dp));
                                format!("{:.prec$}", rounded, prec = *dp as usize)
                            }
                            None => decimal.normalize().to_string(),
                        },
                        Err(_) => rational_to_display_str(n),
                    };
                    return write!(f, "{} {}", s, u);
                }
                write!(f, "{}", self.value)
            }
            ValueKind::Ratio(_, Some(_unit_name)) => write!(f, "{}", self.value),
            ValueKind::Range(left, right) => write!(f, "{}...{}", left, right),
            _ => write!(f, "{}", self.value),
        }
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::{decimal_to_rational, RationalInteger};
    use crate::literals::Value;
    use crate::parsing::ast::{BooleanValue, DateTimeValue, LemmaSpec, PrimitiveKind, TimeValue};
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use std::sync::Arc;

    #[test]
    fn default_primitive_help_is_goal_oriented() {
        let kinds = [
            PrimitiveKind::Boolean,
            PrimitiveKind::Quantity,
            PrimitiveKind::QuantityRange,
            PrimitiveKind::Number,
            PrimitiveKind::NumberRange,
            PrimitiveKind::Percent,
            PrimitiveKind::Ratio,
            PrimitiveKind::RatioRange,
            PrimitiveKind::Text,
            PrimitiveKind::Date,
            PrimitiveKind::DateRange,
            PrimitiveKind::Time,
            PrimitiveKind::Calendar,
            PrimitiveKind::CalendarRange,
        ];
        for kind in kinds {
            let spec = type_spec_for_primitive(kind);
            let help = match &spec {
                TypeSpecification::Boolean { help, .. }
                | TypeSpecification::Number { help, .. }
                | TypeSpecification::NumberRange { help }
                | TypeSpecification::Text { help, .. }
                | TypeSpecification::Quantity { help, .. }
                | TypeSpecification::QuantityRange { help, .. }
                | TypeSpecification::Ratio { help, .. }
                | TypeSpecification::RatioRange { help, .. }
                | TypeSpecification::Date { help, .. }
                | TypeSpecification::DateRange { help }
                | TypeSpecification::Time { help, .. }
                | TypeSpecification::Calendar { help, .. }
                | TypeSpecification::CalendarRange { help } => help,
                TypeSpecification::Veto { .. } | TypeSpecification::Undetermined => {
                    unreachable!(
                        "BUG: primitive kind {:?} mapped to non-primitive spec",
                        kind
                    )
                }
            };
            assert!(!help.is_empty(), "help for {:?}", kind);
            assert!(
                !help.to_ascii_lowercase().contains("format:"),
                "help for {:?} must not describe syntax: {:?}",
                kind,
                help
            );
            assert_eq!(help, default_help_for_primitive(kind));
        }
    }

    #[test]
    fn test_negated_comparison() {
        assert_eq!(
            negated_comparison(ComparisonComputation::LessThan),
            ComparisonComputation::GreaterThanOrEqual
        );
        assert_eq!(
            negated_comparison(ComparisonComputation::GreaterThanOrEqual),
            ComparisonComputation::LessThan
        );
        assert_eq!(
            negated_comparison(ComparisonComputation::Is),
            ComparisonComputation::IsNot
        );
        assert_eq!(
            negated_comparison(ComparisonComputation::IsNot),
            ComparisonComputation::Is
        );
    }

    #[test]
    fn value_to_semantic_number_is_decimal() {
        let kind = value_to_semantic(&Value::Number(Decimal::from(42))).unwrap();
        assert!(matches!(kind, ValueKind::Number(d) if d == RationalInteger::new(42, 1)));
    }

    #[test]
    fn parse_data_value_from_json_accepts_json_number_for_number_type() {
        use crate::parsing::ast::Span;
        use crate::parsing::source::SourceType;
        let source = Source::new(
            SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let ty = primitive_number();
        let lit =
            parse_data_value_from_json(&serde_json::json!(42), &ty.specifications, ty, &source)
                .unwrap();
        assert!(matches!(lit.value, ValueKind::Number(d) if d == RationalInteger::new(42, 1)));
        let lit =
            parse_data_value_from_json(&serde_json::json!(1.5), &ty.specifications, ty, &source)
                .unwrap();
        assert!(
            matches!(lit.value, ValueKind::Number(d) if d == decimal_to_rational(Decimal::from_str("1.5").unwrap()).unwrap())
        );
    }

    #[test]
    fn parse_data_value_from_json_rejects_bare_json_number_for_quantity() {
        use crate::literals::{QuantityUnit, QuantityUnits};
        use crate::parsing::ast::Span;
        use crate::parsing::source::SourceType;
        let source = Source::new(
            SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let spec = TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units: QuantityUnits::from(vec![QuantityUnit {
                name: "eur".to_string(),
                factor: crate::computation::rational::rational_one(),
                derived_quantity_factors: Vec::new(),
                decomposition: BaseQuantityVector::new(),
                minimum: None,
                maximum: None,
                default_magnitude: None,
            }]),
            traits: Vec::new(),
            decomposition: BaseQuantityVector::new(),
            canonical_unit: "eur".to_string(),
            help: String::new(),
        };
        let ty = LemmaType::primitive(spec);
        assert!(parse_data_value_from_json(
            &serde_json::json!(100),
            &ty.specifications,
            &ty,
            &source,
        )
        .is_err());
    }

    #[test]
    fn value_kind_quantity_serializes_as_value_unit_object() {
        let kind = ValueKind::Quantity(
            decimal_to_rational(Decimal::from_str("99.50").unwrap()).unwrap(),
            "eur".to_string(),
            BaseQuantityVector::new(),
        );
        let json = serde_json::to_value(&kind).unwrap();
        assert_eq!(json["quantity"]["value"], "99.5");
        assert_eq!(json["quantity"]["unit"], "eur");
    }

    #[test]
    fn literal_value_number_serde_not_rational_array() {
        let lit = LiteralValue::number_from_decimal(Decimal::from(20));
        let json = serde_json::to_value(&lit).unwrap();
        let number = json
            .get("value")
            .and_then(|v| v.get("number"))
            .expect("number field");
        assert!(number.is_string());
        assert_eq!(number.as_str(), Some("20"));
        assert!(
            !number.is_array(),
            "stored number must not serialize as [n,d]"
        );
    }

    #[test]
    fn test_literal_value_to_primitive_type() {
        let one = RationalInteger::new(1, 1);

        assert_eq!(LiteralValue::text("".to_string()).lemma_type.name(), "text");
        assert_eq!(LiteralValue::number(one).lemma_type.name(), "number");
        assert_eq!(
            LiteralValue::from_bool(bool::from(BooleanValue::True))
                .lemma_type
                .name(),
            "boolean"
        );

        let dt = DateTimeValue {
            year: 2024,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        };
        assert_eq!(
            LiteralValue::date(date_time_to_semantic(&dt))
                .lemma_type
                .name(),
            "date"
        );
        assert_eq!(
            LiteralValue::ratio_from_decimal(Decimal::new(1, 2), Some("percent".to_string()))
                .lemma_type
                .name(),
            "ratio"
        );
        let dur_type = LemmaType::new(
            "duration".to_string(),
            TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: QuantityUnits::from(vec![QuantityUnit {
                    name: "second".to_string(),
                    factor: crate::computation::rational::rational_one(),
                    derived_quantity_factors: Vec::new(),
                    decomposition: BaseQuantityVector::new(),
                    minimum: None,
                    maximum: None,
                    default_magnitude: None,
                }]),
                traits: vec![QuantityTrait::Duration],
                decomposition: BaseQuantityVector::new(),
                canonical_unit: "second".to_string(),
                help: String::new(),
            },
            TypeExtends::Primitive,
        );
        assert_eq!(
            LiteralValue::quantity_with_type(one, "second".to_string(), dur_type)
                .lemma_type
                .name(),
            "duration"
        );
    }

    #[test]
    fn test_type_display() {
        let specs = TypeSpecification::text();
        let lemma_type = LemmaType::new("name".to_string(), specs, TypeExtends::Primitive);
        assert_eq!(format!("{}", lemma_type), "name");
    }

    #[test]
    fn test_type_serialization() {
        let specs = TypeSpecification::number();
        let lemma_type = LemmaType::new("dice".to_string(), specs, TypeExtends::Primitive);
        let serialized = serde_json::to_string(&lemma_type).unwrap();
        let deserialized: LemmaType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(lemma_type, deserialized);
    }

    #[test]
    fn test_literal_value_display_value() {
        let ten = RationalInteger::new(10, 1);

        assert_eq!(
            LiteralValue::text("hello".to_string()).display_value(),
            "hello"
        );
        assert_eq!(LiteralValue::number(ten).display_value(), "10");
        assert_eq!(LiteralValue::from_bool(true).display_value(), "true");
        assert_eq!(LiteralValue::from_bool(false).display_value(), "false");

        // 0.10 ratio with "percent" unit displays as 10% (unit conversion applied)
        let ten_percent_ratio =
            LiteralValue::ratio_from_decimal(Decimal::new(1, 1), Some("percent".to_string()));
        assert_eq!(ten_percent_ratio.display_value(), "10%");

        let time = TimeValue {
            hour: 14,
            minute: 30,
            second: 0,
            microsecond: 0,
            timezone: None,
        };
        let time_display = LiteralValue::time(time_to_semantic(&time)).display_value();
        assert!(time_display.contains("14"));
        assert!(time_display.contains("30"));
    }

    #[test]
    fn test_quantity_display_respects_type_decimals() {
        let money_type = LemmaType {
            name: Some("money".to_string()),
            specifications: TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                units: QuantityUnits::from(vec![QuantityUnit {
                    name: "eur".to_string(),
                    factor: crate::computation::rational::rational_one(),
                    derived_quantity_factors: Vec::new(),
                    decomposition: BaseQuantityVector::new(),
                    minimum: None,
                    maximum: None,
                    default_magnitude: None,
                }]),
                traits: Vec::new(),
                decomposition: BaseQuantityVector::new(),
                canonical_unit: "eur".to_string(),
                help: String::new(),
            },
            extends: TypeExtends::Primitive,
        };
        let val = LiteralValue::quantity_with_type(
            decimal_to_rational(Decimal::from_str("1.8").unwrap()).unwrap(),
            "eur".to_string(),
            money_type.clone(),
        );
        assert_eq!(val.display_value(), "1.80 eur");
        let more_precision = LiteralValue::quantity_with_type(
            decimal_to_rational(Decimal::from_str("1.80000").unwrap()).unwrap(),
            "eur".to_string(),
            money_type,
        );
        assert_eq!(more_precision.display_value(), "1.80 eur");
        let quantity_no_decimals = LemmaType {
            name: Some("count".to_string()),
            specifications: TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: QuantityUnits::from(vec![QuantityUnit {
                    name: "items".to_string(),
                    factor: crate::computation::rational::rational_one(),
                    derived_quantity_factors: Vec::new(),
                    decomposition: BaseQuantityVector::new(),
                    minimum: None,
                    maximum: None,
                    default_magnitude: None,
                }]),
                traits: Vec::new(),
                decomposition: BaseQuantityVector::new(),
                canonical_unit: "items".to_string(),
                help: String::new(),
            },
            extends: TypeExtends::Primitive,
        };
        let val_any = LiteralValue::quantity_with_type(
            decimal_to_rational(Decimal::from_str("42.50").unwrap()).unwrap(),
            "items".to_string(),
            quantity_no_decimals,
        );
        assert_eq!(val_any.display_value(), "42.5 items");
    }

    #[test]
    fn test_literal_value_time_type() {
        let time = TimeValue {
            hour: 14,
            minute: 30,
            second: 0,
            microsecond: 0,
            timezone: None,
        };
        let lit = LiteralValue::time(time_to_semantic(&time));
        assert_eq!(lit.lemma_type.name(), "time");
    }

    #[test]
    fn test_quantity_family_name_primitive_root() {
        let quantity_spec = TypeSpecification::quantity();
        let money_primitive = LemmaType::new(
            "money".to_string(),
            quantity_spec.clone(),
            TypeExtends::Primitive,
        );
        assert_eq!(money_primitive.quantity_family_name(), Some("money"));
    }

    #[test]
    fn test_quantity_family_name_custom() {
        let quantity_spec = TypeSpecification::quantity();
        let money_custom = LemmaType::new(
            "money".to_string(),
            quantity_spec,
            TypeExtends::custom_local("money".to_string(), "money".to_string()),
        );
        assert_eq!(money_custom.quantity_family_name(), Some("money"));
    }

    #[test]
    fn test_same_quantity_family_same_name_different_extends() {
        let quantity_spec = TypeSpecification::quantity();
        let money_primitive = LemmaType::new(
            "money".to_string(),
            quantity_spec.clone(),
            TypeExtends::Primitive,
        );
        let money_custom = LemmaType::new(
            "money".to_string(),
            quantity_spec,
            TypeExtends::custom_local("money".to_string(), "money".to_string()),
        );
        assert!(money_primitive.same_quantity_family(&money_custom));
        assert!(money_custom.same_quantity_family(&money_primitive));
    }

    #[test]
    fn test_same_quantity_family_parent_and_child() {
        let quantity_spec = TypeSpecification::quantity();
        let type_x = LemmaType::new(
            "x".to_string(),
            quantity_spec.clone(),
            TypeExtends::Primitive,
        );
        let type_x2 = LemmaType::new(
            "x2".to_string(),
            quantity_spec,
            TypeExtends::custom_local("x".to_string(), "x".to_string()),
        );
        assert_eq!(type_x.quantity_family_name(), Some("x"));
        assert_eq!(type_x2.quantity_family_name(), Some("x"));
        assert!(type_x.same_quantity_family(&type_x2));
        assert!(type_x2.same_quantity_family(&type_x));
    }

    #[test]
    fn test_same_quantity_family_siblings() {
        let quantity_spec = TypeSpecification::quantity();
        let type_x2_a = LemmaType::new(
            "x2a".to_string(),
            quantity_spec.clone(),
            TypeExtends::custom_local("x".to_string(), "x".to_string()),
        );
        let type_x2_b = LemmaType::new(
            "x2b".to_string(),
            quantity_spec,
            TypeExtends::custom_local("x".to_string(), "x".to_string()),
        );
        assert!(type_x2_a.same_quantity_family(&type_x2_b));
    }

    #[test]
    fn test_same_quantity_family_different_families() {
        let quantity_spec = TypeSpecification::quantity();
        let money = LemmaType::new(
            "money".to_string(),
            quantity_spec.clone(),
            TypeExtends::Primitive,
        );
        let temperature = LemmaType::new(
            "temperature".to_string(),
            quantity_spec,
            TypeExtends::Primitive,
        );
        assert!(!money.same_quantity_family(&temperature));
        assert!(!temperature.same_quantity_family(&money));
    }

    #[test]
    fn test_same_quantity_family_quantity_vs_non_quantity() {
        let quantity_spec = TypeSpecification::quantity();
        let number_spec = TypeSpecification::number();
        let quantity_type =
            LemmaType::new("money".to_string(), quantity_spec, TypeExtends::Primitive);
        let number_type = LemmaType::new("amount".to_string(), number_spec, TypeExtends::Primitive);
        assert!(!quantity_type.same_quantity_family(&number_type));
        assert!(!number_type.same_quantity_family(&quantity_type));
    }

    #[test]
    fn test_same_quantity_family_anonymous_quantitys_are_not_family_compatible() {
        let left = LemmaType::anonymous_for_decomposition(duration_decomposition());
        let right = LemmaType::anonymous_for_decomposition(duration_decomposition());

        assert!(!left.same_quantity_family(&right));
        assert!(left.compatible_with_anonymous_quantity(&right));
    }

    #[test]
    fn test_quantity_family_name_non_quantity_returns_none() {
        let number_spec = TypeSpecification::number();
        let number_type = LemmaType::new("amount".to_string(), number_spec, TypeExtends::Primitive);
        assert_eq!(number_type.quantity_family_name(), None);
    }

    #[test]
    fn test_lemma_type_inequality_local_vs_import_same_shape() {
        let dep = Arc::new(LemmaSpec::new("dep".to_string()));
        let quantity_spec = TypeSpecification::quantity();
        let local = LemmaType::new(
            "t".to_string(),
            quantity_spec.clone(),
            TypeExtends::custom_local("money".to_string(), "money".to_string()),
        );
        let imported = LemmaType::new(
            "t".to_string(),
            quantity_spec,
            TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: TypeDefiningSpec::Import {
                    spec: Arc::clone(&dep),
                },
            },
        );
        assert_ne!(local, imported);
    }

    #[test]
    fn test_lemma_type_equality_import_same_arc_pointer_identity() {
        // TypeDefiningSpec equality is by Arc pointer identity (Arc::ptr_eq).
        // Two types are equal iff they hold the same interned Arc, matching
        // the Context::insert_spec invariant.
        let shared_spec = Arc::new(LemmaSpec::new("dep".to_string()));
        let quantity_spec = TypeSpecification::quantity();
        let left = LemmaType::new(
            "t".to_string(),
            quantity_spec.clone(),
            TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: TypeDefiningSpec::Import {
                    spec: Arc::clone(&shared_spec),
                },
            },
        );
        let right = LemmaType::new(
            "t".to_string(),
            quantity_spec,
            TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: TypeDefiningSpec::Import {
                    spec: Arc::clone(&shared_spec),
                },
            },
        );
        assert_eq!(left, right);
    }

    #[test]
    fn test_lemma_type_inequality_import_different_arc_pointer() {
        // Two distinct Arc<LemmaSpec> (even with identical content) are not equal.
        let spec_a = Arc::new(LemmaSpec::new("dep".to_string()));
        let spec_b = Arc::new(LemmaSpec::new("dep".to_string()));
        let quantity_spec = TypeSpecification::quantity();
        let left = LemmaType::new(
            "t".to_string(),
            quantity_spec.clone(),
            TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: TypeDefiningSpec::Import {
                    spec: Arc::clone(&spec_a),
                },
            },
        );
        let right = LemmaType::new(
            "t".to_string(),
            quantity_spec,
            TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: TypeDefiningSpec::Import { spec: spec_b },
            },
        );
        assert_ne!(left, right);
    }

    fn month_default_arg() -> CommandArg {
        CommandArg::Literal(crate::literals::Value::Calendar(
            Decimal::ONE,
            crate::literals::CalendarUnit::Month,
        ))
    }

    fn unit_factor_arg(name: &str, factor: i64) -> [CommandArg; 2] {
        [
            CommandArg::Label(name.to_string()),
            CommandArg::UnitExpr(crate::parsing::ast::UnitArg::Factor(Decimal::from(factor))),
        ]
    }

    #[test]
    fn default_calendar_on_text_reports_hint() {
        let specs = TypeSpecification::text();
        let mut default = None;
        let err = specs
            .apply_constraint(
                "notes",
                TypeConstraintCommand::Default,
                &[month_default_arg()],
                &mut default,
            )
            .unwrap_err();
        assert!(err.contains("Unit 'month' is for calendar data"));
        assert!(err.contains("double quotes"));
    }

    #[test]
    fn default_calendar_on_duration_reports_valid_units() {
        let mut specs = TypeSpecification::quantity();
        specs = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Unit,
                &unit_factor_arg("second", 1),
                &mut None,
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Unit,
                &unit_factor_arg("week", 604_800),
                &mut None,
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Trait,
                &[CommandArg::Label("duration".to_string())],
                &mut None,
            )
            .unwrap();
        let mut default = None;
        let err = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Default,
                &[month_default_arg()],
                &mut default,
            )
            .unwrap_err();
        assert!(err.contains("Unit 'month' is for calendar data"));
        assert!(err.contains("Valid 'duration' units are"));
        assert!(err.contains("week"));
    }

    #[test]
    fn default_valid_duration_weeks_accepted() {
        let mut specs = TypeSpecification::quantity();
        specs = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Unit,
                &unit_factor_arg("second", 1),
                &mut None,
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Unit,
                &unit_factor_arg("week", 604_800),
                &mut None,
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Trait,
                &[CommandArg::Label("duration".to_string())],
                &mut None,
            )
            .unwrap();
        let mut default = None;
        specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Default,
                &[CommandArg::Literal(crate::literals::Value::NumberWithUnit(
                    Decimal::from(4),
                    "week".to_string(),
                ))],
                &mut default,
            )
            .unwrap();
        assert!(matches!(default, Some(ValueKind::Quantity(_, unit, _)) if unit == "week"));
    }

    #[test]
    fn default_unknown_unit_on_duration_lists_valid_units() {
        let mut specs = TypeSpecification::quantity();
        specs = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Unit,
                &unit_factor_arg("second", 1),
                &mut None,
            )
            .unwrap();
        specs = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Trait,
                &[CommandArg::Label("duration".to_string())],
                &mut None,
            )
            .unwrap();
        let mut default = None;
        let err = specs
            .apply_constraint(
                "duration",
                TypeConstraintCommand::Default,
                &[CommandArg::Literal(crate::literals::Value::NumberWithUnit(
                    Decimal::ONE,
                    "fortnight".to_string(),
                ))],
                &mut default,
            )
            .unwrap_err();
        assert!(err.contains("fortnight"));
        assert!(err.contains("Valid 'duration' units are"));
    }

    fn money_quantity_type() -> LemmaType {
        LemmaType::new(
            "Money".to_string(),
            TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: QuantityUnits::from(vec![
                    QuantityUnit {
                        name: "eur".to_string(),
                        factor: crate::computation::rational::rational_one(),
                        derived_quantity_factors: Vec::new(),
                        decomposition: BaseQuantityVector::new(),
                        minimum: None,
                        maximum: None,
                        default_magnitude: None,
                    },
                    QuantityUnit {
                        name: "usd".to_string(),
                        factor: crate::computation::rational::decimal_to_rational(Decimal::new(
                            91, 2,
                        ))
                        .expect("factor"),
                        derived_quantity_factors: Vec::new(),
                        decomposition: BaseQuantityVector::new(),
                        minimum: None,
                        maximum: None,
                        default_magnitude: None,
                    },
                ]),
                traits: Vec::new(),
                decomposition: BaseQuantityVector::new(),
                canonical_unit: "eur".to_string(),
                help: String::new(),
            },
            TypeExtends::Primitive,
        )
    }

    #[test]
    fn validate_rule_result_unit_conversion_requires_unit_index_entry() {
        let money = money_quantity_type();
        let mut index = std::collections::HashMap::new();
        index.insert("eur".to_string(), money.clone());
        let err = money
            .validate_rule_result_unit_conversion("usd", &index, "pricing")
            .expect_err("usd missing from index");
        assert!(err.contains("Unknown unit 'usd'"), "got: {err}");
    }

    #[test]
    fn validate_rule_result_unit_conversion_accepts_declared_unit_in_index() {
        let money = money_quantity_type();
        let mut index = std::collections::HashMap::new();
        index.insert("eur".to_string(), money.clone());
        index.insert("usd".to_string(), money.clone());
        money
            .validate_rule_result_unit_conversion("usd", &index, "pricing")
            .unwrap();
    }

    #[test]
    fn validate_quantity_result_unit_accepts_declared_unit() {
        let money = money_quantity_type();
        money.validate_quantity_result_unit("usd").unwrap();
        money.validate_quantity_result_unit("EUR").unwrap();
    }

    #[test]
    fn validate_quantity_result_unit_lists_valid_units() {
        let money = money_quantity_type();
        let err = money
            .validate_quantity_result_unit("gbp")
            .expect_err("gbp not declared");
        assert!(err.contains("Valid units: eur, usd"), "got: {err}");
    }

    #[test]
    fn validate_quantity_result_unit_rejects_zero_factor() {
        let mut money = money_quantity_type();
        if let TypeSpecification::Quantity { units, .. } = &mut money.specifications {
            units.push(QuantityUnit {
                name: "zero".to_string(),
                factor: crate::computation::rational::RationalInteger::new(0, 1),
                derived_quantity_factors: Vec::new(),
                decomposition: BaseQuantityVector::new(),
                minimum: None,
                maximum: None,
                default_magnitude: None,
            });
        }
        let err = money
            .validate_quantity_result_unit("zero")
            .expect_err("zero factor");
        assert!(err.contains("zero conversion factor"), "got: {err}");
    }

    #[test]
    fn validate_quantity_result_unit_rejects_non_quantity() {
        let number = primitive_number().clone();
        let err = number
            .validate_quantity_result_unit("eur")
            .expect_err("number is not quantity");
        assert!(
            err.contains("Cannot convert number to quantity unit"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_quantity_result_unit_rejects_anonymous_quantity() {
        let mut decomposition = BaseQuantityVector::new();
        decomposition.insert("mass".to_string(), 1);
        let anonymous = LemmaType::anonymous_for_decomposition(decomposition);
        let err = anonymous
            .validate_quantity_result_unit("kilogram")
            .expect_err("anonymous");
        assert!(
            err.contains("Cannot convert quantity to quantity unit"),
            "got: {err}"
        );
    }

    #[test]
    fn quantity_unit_names_for_named_quantity() {
        let money = money_quantity_type();
        assert_eq!(money.quantity_unit_names(), Some(vec!["eur", "usd"]));
    }
}
