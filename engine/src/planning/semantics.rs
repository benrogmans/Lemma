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
use crate::parsing::ast::Constraint;
use crate::parsing::ast::{
    BooleanValue, CalendarUnit, CommandArg, ConversionTarget, DateCalendarKind, DateRelativeKind,
    DateTimeValue, DurationUnit, LemmaSpec, PrimitiveKind, TimeValue, TypeConstraintCommand,
};
use crate::Error;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::sync::{Arc, OnceLock};

// -----------------------------------------------------------------------------
// Type specification and units (resolved type shape; apply constraints is planning)
// -----------------------------------------------------------------------------

// Unit tables live in `crate::literals` (no dependency on parsing/ast). Re-exported
// here so downstream modules importing from `planning::semantics` keep working.
pub use crate::literals::{RatioUnit, RatioUnits, ScaleUnit, ScaleUnits};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TypeSpecification {
    Boolean {
        help: String,
    },
    Scale {
        minimum: Option<Decimal>,
        maximum: Option<Decimal>,
        decimals: Option<u8>,
        precision: Option<Decimal>,
        units: ScaleUnits,
        help: String,
    },
    Number {
        minimum: Option<Decimal>,
        maximum: Option<Decimal>,
        decimals: Option<u8>,
        precision: Option<Decimal>,
        help: String,
    },
    Ratio {
        minimum: Option<Decimal>,
        maximum: Option<Decimal>,
        decimals: Option<u8>,
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
    Time {
        minimum: Option<TimeValue>,
        maximum: Option<TimeValue>,
        help: String,
    },
    Duration {
        minimum: Option<(Decimal, SemanticDurationUnit)>,
        maximum: Option<(Decimal, SemanticDurationUnit)>,
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

/// Human-readable name for a [`Value`] variant — used in mismatch error messages.
fn value_kind_name(v: &crate::literals::Value) -> &'static str {
    use crate::literals::Value;
    match v {
        Value::Number(_) => "number",
        Value::Scale(_, _) => "scale",
        Value::Text(_) => "text",
        Value::Date(_) => "date",
        Value::Time(_) => "time",
        Value::Boolean(_) => "boolean",
        Value::Duration(_, _) => "duration",
        Value::Ratio(_, _) => "ratio",
    }
}

/// Cast a [`Decimal`] to `u8`, requiring it to be a non-negative whole number that fits.
fn decimal_to_u8(d: Decimal, ctx: &str) -> Result<u8, String> {
    use rust_decimal::prelude::ToPrimitive;
    if !d.fract().is_zero() {
        return Err(format!(
            "{} requires a whole number, got fractional value {}",
            ctx, d
        ));
    }
    d.to_u8()
        .ok_or_else(|| format!("{} value out of range for u8: {}", ctx, d))
}

/// Cast a [`Decimal`] to `usize`, requiring it to be a non-negative whole number that fits.
fn decimal_to_usize(d: Decimal, ctx: &str) -> Result<usize, String> {
    use rust_decimal::prelude::ToPrimitive;
    if !d.fract().is_zero() {
        return Err(format!(
            "{} requires a whole number, got fractional value {}",
            ctx, d
        ));
    }
    d.to_usize()
        .ok_or_else(|| format!("{} value out of range for usize: {}", ctx, d))
}

/// Extract a bare [`Decimal`] from a [`Value::Number`] literal arg.
///
/// Numeric meta-constraints (`decimals`, `precision`, `length`, `minimum`/`maximum`
/// on `Number` and `Scale`) take a bare decimal — not a ratio, not a scale. Reject
/// any other variant to honour the no-coercion contract.
fn require_decimal_literal(args: &[CommandArg], cmd: &str) -> Result<Decimal, String> {
    match require_literal(args, cmd)? {
        crate::literals::Value::Number(d) => Ok(*d),
        other => Err(format!(
            "{} requires a number literal, got {}",
            cmd,
            value_kind_name(other)
        )),
    }
}

/// Resolve a scale constraint arg to a canonical decimal in the scale's base unit.
///
/// Accepts:
/// - `Value::Scale(d, unit)` — looks `unit` up in the scale's `units` table and
///   multiplies by the unit's conversion factor (`5 eur` with `unit eur 1.00`
///   becomes `5`). The unit must be defined before the bound is applied;
///   otherwise the lookup fails.
/// - `Value::Number(d)` — treated as already in base units.
fn require_scale_literal(
    args: &[CommandArg],
    units: &ScaleUnits,
    cmd: &str,
) -> Result<Decimal, String> {
    use crate::literals::Value;
    match require_literal(args, cmd)? {
        Value::Scale(d, unit_name) => {
            let unit = units.get(unit_name)?;
            Ok(*d * unit.value)
        }
        Value::Number(d) => Ok(*d),
        other => Err(format!(
            "{} requires a scale or number literal, got {}",
            cmd,
            value_kind_name(other)
        )),
    }
}

/// Resolve a ratio constraint arg to a canonical 0..1 decimal.
///
/// Accepts:
/// - `Value::Ratio(d, _)` — already canonicalised by the parser (`5%` → `0.05`).
/// - `Value::Number(d)` — bare decimal interpreted as a unit-less ratio (`0.5`).
///
/// All other [`Value`] variants are rejected. Unit-named ratios with non-canonical
/// units (e.g. user-defined `unit basis_point 0.0001`) are not yet representable
/// at the literal layer and route through the same path once added.
fn require_ratio_literal(args: &[CommandArg], cmd: &str) -> Result<Decimal, String> {
    use crate::literals::Value;
    match require_literal(args, cmd)? {
        Value::Ratio(d, _) => Ok(*d),
        Value::Number(d) => Ok(*d),
        other => Err(format!(
            "{} requires a ratio or number literal, got {}",
            cmd,
            value_kind_name(other)
        )),
    }
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
    }
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

/// Extract a `(value, unit)` pair from a [`Value::Duration`] literal arg.
fn require_duration_literal(
    args: &[CommandArg],
    cmd: &str,
) -> Result<(Decimal, DurationUnit), String> {
    match require_literal(args, cmd)? {
        crate::literals::Value::Duration(d, unit) => Ok((*d, unit.clone())),
        other => Err(format!(
            "{} requires a duration literal (e.g. 1 day), got {}",
            cmd,
            value_kind_name(other)
        )),
    }
}

impl TypeSpecification {
    pub fn boolean() -> Self {
        TypeSpecification::Boolean {
            help: "Values: true, false".to_string(),
        }
    }
    pub fn scale() -> Self {
        TypeSpecification::Scale {
            minimum: None,
            maximum: None,
            decimals: None,
            precision: None,
            units: ScaleUnits::new(),
            help: "Format: {value} {unit} (e.g. 100 kilograms)".to_string(),
        }
    }
    pub fn number() -> Self {
        TypeSpecification::Number {
            minimum: None,
            maximum: None,
            decimals: None,
            precision: None,
            help: "Numeric value".to_string(),
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
                    value: Decimal::from(100),
                },
                RatioUnit {
                    name: "permille".to_string(),
                    value: Decimal::from(1000),
                },
            ]),
            help: "Format: {value} {unit} (e.g. 21 percent)".to_string(),
        }
    }
    pub fn text() -> Self {
        TypeSpecification::Text {
            length: None,
            options: vec![],
            help: "Text value".to_string(),
        }
    }
    pub fn date() -> Self {
        TypeSpecification::Date {
            minimum: None,
            maximum: None,
            help: "Format: YYYY-MM-DD (e.g. 2024-01-15)".to_string(),
        }
    }
    pub fn time() -> Self {
        TypeSpecification::Time {
            minimum: None,
            maximum: None,
            help: "Format: HH:MM:SS (e.g. 14:30:00)".to_string(),
        }
    }
    pub fn duration() -> Self {
        TypeSpecification::Duration {
            minimum: None,
            maximum: None,
            help: "Format: {value} {unit} (e.g. 40 hours). Units: years, months, weeks, days, hours, minutes, seconds".to_string(),
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
        command: TypeConstraintCommand,
        args: &[CommandArg],
        declared_default: &mut Option<ValueKind>,
    ) -> Result<Self, String> {
        match &mut self {
            TypeSpecification::Boolean { help } => match command {
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => match require_literal(args, "default")? {
                    crate::literals::Value::Boolean(bv) => {
                        *declared_default = Some(ValueKind::Boolean(bool::from(*bv)));
                    }
                    other => {
                        return Err(format!(
                            "default for boolean type requires a boolean literal (true/false/yes/no/accept/reject), got {}",
                            value_kind_name(other)
                        ));
                    }
                },
                other => {
                    return Err(format!(
                        "Invalid command '{}' for boolean type. Valid commands: help, default",
                        other
                    ));
                }
            },
            TypeSpecification::Scale {
                decimals,
                minimum,
                maximum,
                precision,
                units,
                help,
            } => match command {
                TypeConstraintCommand::Decimals => {
                    let d = require_decimal_literal(args, "decimals")?;
                    *decimals = Some(decimal_to_u8(d, "decimals")?);
                }
                TypeConstraintCommand::Unit => {
                    let (unit_name, value) = match args {
                        [CommandArg::Label(name), CommandArg::Literal(crate::literals::Value::Number(v))] => {
                            (name.clone(), *v)
                        }
                        _ => {
                            return Err(
                                "unit requires a unit name followed by a numeric conversion factor (e.g., 'unit eur 1.00')"
                                    .to_string(),
                            );
                        }
                    };
                    if units.iter().any(|u| u.name == unit_name) {
                        return Err(format!(
                            "Unit '{}' is already defined in this scale type.",
                            unit_name
                        ));
                    }
                    units.0.push(ScaleUnit {
                        name: unit_name,
                        value,
                    });
                }
                TypeConstraintCommand::Minimum => {
                    *minimum = Some(require_scale_literal(args, units, "minimum")?);
                }
                TypeConstraintCommand::Maximum => {
                    *maximum = Some(require_scale_literal(args, units, "maximum")?);
                }
                TypeConstraintCommand::Precision => {
                    *precision = Some(require_scale_literal(args, units, "precision")?);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => match require_literal(args, "default")? {
                    crate::literals::Value::Scale(value, unit_name) => {
                        *declared_default = Some(ValueKind::Scale(*value, unit_name.clone()));
                    }
                    other => {
                        return Err(format!(
                            "default for scale type requires a scale literal '{{value}} {{unit}}' (e.g. '1 kilogram'), got {}",
                            value_kind_name(other)
                        ));
                    }
                },
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for scale type. Valid commands: unit, minimum, maximum, decimals, precision, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Number {
                decimals,
                minimum,
                maximum,
                precision,
                help,
            } => match command {
                TypeConstraintCommand::Decimals => {
                    let d = require_decimal_literal(args, "decimals")?;
                    *decimals = Some(decimal_to_u8(d, "decimals")?);
                }
                TypeConstraintCommand::Unit => {
                    return Err(
                        "Invalid command 'unit' for number type. Number types are dimensionless and cannot have units. Use 'scale' type instead.".to_string()
                    );
                }
                TypeConstraintCommand::Minimum => {
                    *minimum = Some(require_decimal_literal(args, "minimum")?);
                }
                TypeConstraintCommand::Maximum => {
                    *maximum = Some(require_decimal_literal(args, "maximum")?);
                }
                TypeConstraintCommand::Precision => {
                    *precision = Some(require_decimal_literal(args, "precision")?);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => match require_literal(args, "default")? {
                    crate::literals::Value::Number(d) => {
                        *declared_default = Some(ValueKind::Number(*d));
                    }
                    other => {
                        return Err(format!(
                            "default for number type requires a number literal, got {}",
                            value_kind_name(other)
                        ));
                    }
                },
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for number type. Valid commands: minimum, maximum, decimals, precision, help, default",
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
                    let (unit_name, value) = match args {
                        [CommandArg::Label(name), CommandArg::Literal(crate::literals::Value::Number(v))] => {
                            (name.clone(), *v)
                        }
                        _ => {
                            return Err(
                                "unit requires a unit name followed by a numeric conversion factor (e.g., 'unit percent 100')"
                                    .to_string(),
                            );
                        }
                    };
                    if units.iter().any(|u| u.name == unit_name) {
                        return Err(format!(
                            "Unit '{}' is already defined in this ratio type.",
                            unit_name
                        ));
                    }
                    units.0.push(RatioUnit {
                        name: unit_name,
                        value,
                    });
                }
                TypeConstraintCommand::Minimum => {
                    *minimum = Some(require_ratio_literal(args, "minimum")?);
                }
                TypeConstraintCommand::Maximum => {
                    *maximum = Some(require_ratio_literal(args, "maximum")?);
                }
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Default => {
                    let d = require_ratio_literal(args, "default")?;
                    *declared_default = Some(ValueKind::Ratio(d, None));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for ratio type. Valid commands: unit, minimum, maximum, decimals, help, default",
                        command
                    ));
                }
            },
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
                TypeConstraintCommand::Default => match require_literal(args, "default")? {
                    crate::literals::Value::Text(s) => {
                        *declared_default = Some(ValueKind::Text(s.clone()));
                    }
                    other => {
                        return Err(format!(
                            "default for text type requires a text literal (quoted string), got {}",
                            value_kind_name(other)
                        ));
                    }
                },
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
                    let dt = require_date_literal(args, "default")?;
                    *declared_default = Some(ValueKind::Date(date_time_to_semantic(&dt)));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for date type. Valid commands: minimum, maximum, help, default",
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
                    let t = require_time_literal(args, "default")?;
                    *declared_default = Some(ValueKind::Time(time_to_semantic(&t)));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for time type. Valid commands: minimum, maximum, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Duration {
                minimum,
                maximum,
                help,
            } => match command {
                TypeConstraintCommand::Help => {
                    apply_type_help_command(help, args)?;
                }
                TypeConstraintCommand::Minimum => {
                    let (value, unit) = require_duration_literal(args, "minimum")?;
                    *minimum = Some((value, duration_unit_to_semantic(&unit)));
                }
                TypeConstraintCommand::Maximum => {
                    let (value, unit) = require_duration_literal(args, "maximum")?;
                    *maximum = Some((value, duration_unit_to_semantic(&unit)));
                }
                TypeConstraintCommand::Default => {
                    let (value, unit) = require_duration_literal(args, "default")?;
                    *declared_default =
                        Some(ValueKind::Duration(value, duration_unit_to_semantic(&unit)));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for duration type. Valid commands: minimum, maximum, help, default",
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

/// Parse a "number unit" string into a Scale or Ratio value according to the type.
/// Caller must have obtained the TypeSpecification via unit_index from the unit in the string.
pub fn parse_number_unit(
    value_str: &str,
    type_spec: &TypeSpecification,
) -> Result<crate::parsing::ast::Value, String> {
    use crate::literals::{NumberWithUnit, RatioLiteral};
    use crate::parsing::ast::Value;

    let trimmed = value_str.trim();
    match type_spec {
        TypeSpecification::Scale { units, .. } => {
            if units.is_empty() {
                unreachable!(
                    "BUG: Scale type has no units; should have been validated during planning"
                );
            }
            match trimmed.parse::<NumberWithUnit>() {
                Ok(n) => {
                    let unit = units.get(&n.1).map_err(|e| e.to_string())?;
                    Ok(Value::Scale(n.0, unit.name.clone()))
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
                            "Scale value must include a unit, for example: '{} {}'. Valid units: {}.",
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
                RatioLiteral::Bare(n) => Ok(Value::Ratio(n, None)),
                // Sigils are language-level constants. Built-in `ratio()` constructor
                // seeds `percent`=100 and `permille`=1000, and the duplicate-name guard
                // in `apply_constraint` (TypeConstraintCommand::Unit) rejects user redefinition,
                // so these unit names are guaranteed present in every Ratio type.
                RatioLiteral::Percent(n) => {
                    let unit = units.get("percent").map_err(|e| e.to_string())?;
                    Ok(Value::Ratio(n, Some(unit.name.clone())))
                }
                RatioLiteral::Permille(n) => {
                    let unit = units.get("permille").map_err(|e| e.to_string())?;
                    Ok(Value::Ratio(n, Some(unit.name.clone())))
                }
                RatioLiteral::Named { value, unit } => {
                    let resolved = units.get(&unit).map_err(|e| e.to_string())?;
                    Ok(Value::Ratio(
                        value / resolved.value,
                        Some(resolved.name.clone()),
                    ))
                }
            }
        }
        _ => Err("parse_number_unit only accepts Scale or Ratio type".to_string()),
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

    match type_spec {
        TypeSpecification::Text { .. } => value_str
            .parse::<crate::literals::TextLiteral>()
            .map(|t| Value::Text(t.0))
            .map_err(to_err),
        TypeSpecification::Number { .. } => value_str
            .parse::<crate::literals::NumberLiteral>()
            .map(|n| Value::Number(n.0))
            .map_err(to_err),
        TypeSpecification::Scale { .. } => {
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
        TypeSpecification::Duration { .. } => value_str
            .parse::<crate::literals::DurationLiteral>()
            .map(|d| Value::Duration(d.0, d.1))
            .map_err(to_err),
        TypeSpecification::Ratio { .. } => {
            parse_number_unit(value_str, type_spec).map_err(to_err)
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

/// Duration unit for semantic values (duplicated from parser to avoid parser dependency)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDurationUnit {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
}

impl fmt::Display for SemanticDurationUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SemanticDurationUnit::Year => "years",
            SemanticDurationUnit::Month => "months",
            SemanticDurationUnit::Week => "weeks",
            SemanticDurationUnit::Day => "days",
            SemanticDurationUnit::Hour => "hours",
            SemanticDurationUnit::Minute => "minutes",
            SemanticDurationUnit::Second => "seconds",
            SemanticDurationUnit::Millisecond => "milliseconds",
            SemanticDurationUnit::Microsecond => "microseconds",
        };
        write!(f, "{}", s)
    }
}

/// Target unit for conversion (semantic; used by evaluation/computation).
/// Planning converts AST ConversionTarget into this so computation does not depend on parsing.
/// Ratio vs scale is determined by looking up the unit in the type registry's unit index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticConversionTarget {
    Duration(SemanticDurationUnit),
    ScaleUnit(String),
    RatioUnit(String),
}

impl fmt::Display for SemanticConversionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SemanticConversionTarget::Duration(u) => write!(f, "{}", u),
            SemanticConversionTarget::ScaleUnit(s) => write!(f, "{}", s),
            SemanticConversionTarget::RatioUnit(s) => write!(f, "{}", s),
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
    pub timezone: Option<SemanticTimezone>,
}

impl fmt::Display for SemanticTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
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
/// Scale unit is required; Ratio unit is optional (see plan ratio-units-optional.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Number(Decimal),
    /// Scale: value + unit (unit required)
    Scale(Decimal, String),
    Text(String),
    Date(SemanticDateTime),
    Time(SemanticTime),
    Boolean(bool),
    /// Duration: value + unit
    Duration(Decimal, SemanticDurationUnit),
    /// Ratio: value + optional unit
    Ratio(Decimal, Option<String>),
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::parsing::ast::Value;
        match self {
            ValueKind::Number(n) => {
                let norm = n.normalize();
                let s = if norm.fract().is_zero() {
                    norm.trunc().to_string()
                } else {
                    norm.to_string()
                };
                write!(f, "{}", s)
            }
            ValueKind::Scale(n, u) => write!(f, "{}", Value::Scale(*n, u.clone())),
            ValueKind::Text(s) => write!(f, "{}", Value::Text(s.clone())),
            ValueKind::Ratio(r, u) => write!(f, "{}", Value::Ratio(*r, u.clone())),
            ValueKind::Date(dt) => write!(f, "{}", dt),
            ValueKind::Time(t) => write!(
                f,
                "{}",
                Value::Time(crate::parsing::ast::TimeValue {
                    hour: t.hour as u8,
                    minute: t.minute as u8,
                    second: t.second as u8,
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
            ValueKind::Duration(v, u) => write!(f, "{} {}", v, u),
        }
    }
}

// -----------------------------------------------------------------------------
// Resolved path types (moved from parsing::ast)
// -----------------------------------------------------------------------------

/// A single segment in a resolved path traversal
///
/// Used in both DataPath and RulePath to represent spec traversal.
/// Each segment contains a data name that points to a spec.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PathSegment {
    /// The data name in this segment
    pub data: String,
    /// The spec this data references (resolved during planning)
    pub spec: String,
}

/// Resolved path to a data (created during planning from AST DataReference)
///
/// Represents a fully resolved path through specs to reach a data.
/// All spec references are resolved during planning.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DataPath {
    /// Path segments (each is a spec traversal)
    pub segments: Vec<PathSegment>,
    /// Final data name
    pub data: String,
}

impl DataPath {
    /// Create a data path from segments and data name (matches AST DataReference shape)
    pub fn new(segments: Vec<PathSegment>, data: String) -> Self {
        Self { segments, data }
    }

    /// Create a local data path (no spec traversal)
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
    /// Path segments (each is a spec traversal)
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
    Arithmetic(Arc<Expression>, ArithmeticComputation, Arc<Expression>),
    Comparison(Arc<Expression>, ComparisonComputation, Arc<Expression>),
    UnitConversion(Arc<Expression>, SemanticConversionTarget),
    LogicalNegation(Arc<Expression>, NegationType),
    MathematicalComputation(MathematicalComputation, Arc<Expression>),
    Veto(VetoExpression),
    /// The `now` keyword — resolved at evaluation to the effective datetime.
    Now,
    /// Date-relative sugar: `<date_expr> in past [<duration_expr>]` / `in future [...]`
    DateRelative(DateRelativeKind, Arc<Expression>, Option<Arc<Expression>>),
    /// Calendar-period sugar: `<date_expr> in [past|future] calendar year|month|week`
    DateCalendar(DateCalendarKind, CalendarUnit, Arc<Expression>),
}

impl ExpressionKind {
    /// Collect all DataPath references from this expression kind
    pub(crate) fn collect_data_paths(&self, data: &mut std::collections::HashSet<DataPath>) {
        match self {
            ExpressionKind::DataPath(fp) => {
                data.insert(fp.clone());
            }
            ExpressionKind::LogicalAnd(left, right)
            | ExpressionKind::Arithmetic(left, _, right)
            | ExpressionKind::Comparison(left, _, right) => {
                left.collect_data_paths(data);
                right.collect_data_paths(data);
            }
            ExpressionKind::UnitConversion(inner, _)
            | ExpressionKind::LogicalNegation(inner, _)
            | ExpressionKind::MathematicalComputation(_, inner) => {
                inner.collect_data_paths(data);
            }
            ExpressionKind::DateRelative(_, date_expr, tolerance) => {
                date_expr.collect_data_paths(data);
                if let Some(tol) = tolerance {
                    tol.collect_data_paths(data);
                }
            }
            ExpressionKind::DateCalendar(_, _, date_expr) => {
                date_expr.collect_data_paths(data);
            }
            ExpressionKind::Literal(_)
            | ExpressionKind::RulePath(_)
            | ExpressionKind::Veto(_)
            | ExpressionKind::Now => {}
        }
    }
}

// -----------------------------------------------------------------------------
// Resolved types and values
// -----------------------------------------------------------------------------

/// Whether two resolved specs are the same temporal slice (same `name` and `effective_from` as [`LemmaSpec`]'s `PartialEq`).
/// Not `Arc` pointer identity: [`Arc`] equality uses the inner value.
#[inline]
#[must_use]
pub fn is_same_spec(left: &LemmaSpec, right: &LemmaSpec) -> bool {
    left == right
}

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
    /// `defining_spec` records whether the parent chain is local or imported from another spec; see [`TypeDefiningSpec`].
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
                        ) => is_same_spec(left, right),
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

impl Eq for TypeExtends {}

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LemmaType {
    /// Optional type name (e.g., "age", "temperature")
    pub name: Option<String>,
    /// The type specification (Boolean, Number, Scale, etc.).
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
                TypeSpecification::Scale { .. } => "scale",
                TypeSpecification::Number { .. } => "number",
                TypeSpecification::Text { .. } => "text",
                TypeSpecification::Date { .. } => "date",
                TypeSpecification::Time { .. } => "time",
                TypeSpecification::Duration { .. } => "duration",
                TypeSpecification::Ratio { .. } => "ratio",
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

    /// Check if this type is scale
    pub fn is_scale(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Scale { .. })
    }

    /// Check if this type is number (dimensionless)
    pub fn is_number(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Number { .. })
    }

    /// Check if this type is numeric (either scale or number)
    pub fn is_numeric(&self) -> bool {
        matches!(
            &self.specifications,
            TypeSpecification::Scale { .. } | TypeSpecification::Number { .. }
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

    /// Check if this type is time
    pub fn is_time(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Time { .. })
    }

    /// Check if this type is duration
    pub fn is_duration(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Duration { .. })
    }

    /// Check if this type is ratio
    pub fn is_ratio(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Ratio { .. })
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
                | (Scale { .. }, Scale { .. })
                | (Text { .. }, Text { .. })
                | (Date { .. }, Date { .. })
                | (Time { .. }, Time { .. })
                | (Duration { .. }, Duration { .. })
                | (Ratio { .. }, Ratio { .. })
                | (Veto { .. }, Veto { .. })
                | (Undetermined, Undetermined)
        )
    }

    /// For scale types, returns the family name (root of the extension chain). For Custom extends, returns the family field; for Primitive, returns the type's own name (the type is the root). For non-scale types, returns None.
    #[must_use]
    pub fn scale_family_name(&self) -> Option<&str> {
        if !self.is_scale() {
            return None;
        }
        match &self.extends {
            TypeExtends::Custom { family, .. } => Some(family.as_str()),
            TypeExtends::Primitive => self.name.as_deref(),
        }
    }

    /// Returns true if both types are scale and belong to the same scale family (same family name).
    /// Two anonymous primitive scales (no name, no family) are considered compatible.
    #[must_use]
    pub fn same_scale_family(&self, other: &LemmaType) -> bool {
        if !self.is_scale() || !other.is_scale() {
            return false;
        }
        match (self.scale_family_name(), other.scale_family_name()) {
            (Some(self_family), Some(other_family)) => self_family == other_family,
            // Two anonymous primitive scales are compatible with each other.
            (None, None) => true,
            _ => false,
        }
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

    /// Decimal places for display (Number, Scale, and Ratio). Used by formatters.
    /// Ratio: optional, no default; when None display is normalized (no trailing zeros).
    pub fn decimal_places(&self) -> Option<u8> {
        match &self.specifications {
            TypeSpecification::Number { decimals, .. } => *decimals,
            TypeSpecification::Scale { decimals, .. } => *decimals,
            TypeSpecification::Ratio { decimals, .. } => *decimals,
            _ => None,
        }
    }

    /// Get an example value string for this type, suitable for UI help text
    pub fn example_value(&self) -> &'static str {
        match &self.specifications {
            TypeSpecification::Text { .. } => "\"hello world\"",
            TypeSpecification::Scale { .. } => "12.50 eur",
            TypeSpecification::Number { .. } => "3.14",
            TypeSpecification::Boolean { .. } => "true",
            TypeSpecification::Date { .. } => "2023-12-25T14:30:00Z",
            TypeSpecification::Veto { .. } => "veto",
            TypeSpecification::Time { .. } => "14:30:00",
            TypeSpecification::Duration { .. } => "90 minutes",
            TypeSpecification::Ratio { .. } => "50%",
            TypeSpecification::Undetermined => unreachable!(
                "BUG: example_value called on Undetermined sentinel type; this type must never reach user-facing code"
            ),
        }
    }

    /// Factor for a unit of this scale type (for unit conversion during evaluation only).
    /// Planning must validate conversions first and return Error for invalid units.
    /// If called with a non-scale type or unknown unit name, panics (invariant violation).
    #[must_use]
    pub fn scale_unit_factor(&self, unit_name: &str) -> Decimal {
        let units = match &self.specifications {
            TypeSpecification::Scale { units, .. } => units,
            _ => unreachable!(
                "BUG: scale_unit_factor called with non-scale type {}; only call during evaluation after planning validated scale conversion",
                self.name()
            ),
        };
        match units
            .iter()
            .find(|u| u.name.eq_ignore_ascii_case(unit_name))
        {
            Some(ScaleUnit { value, .. }) => *value,
            None => {
                let valid: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
                unreachable!(
                    "BUG: unknown unit '{}' for scale type {} (valid: {}); planning must reject invalid conversions with Error",
                    unit_name,
                    self.name(),
                    valid.join(", ")
                );
            }
        }
    }
}

/// Literal value with type. The single value type in semantics.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
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

    pub fn number(n: Decimal) -> Self {
        Self {
            value: ValueKind::Number(n),
            lemma_type: primitive_number().clone(),
        }
    }

    pub fn number_with_type(n: Decimal, lemma_type: LemmaType) -> Self {
        Self {
            value: ValueKind::Number(n),
            lemma_type,
        }
    }

    pub fn scale_with_type(n: Decimal, unit: String, lemma_type: LemmaType) -> Self {
        Self {
            value: ValueKind::Scale(n, unit),
            lemma_type,
        }
    }

    /// Number interpreted as a scale value in the given unit (e.g. "3 in usd" where 3 is a number).
    /// Creates an anonymous one-unit scale type so computation does not depend on parsing types.
    pub fn number_interpreted_as_scale(value: Decimal, unit_name: String) -> Self {
        let lemma_type = LemmaType {
            name: None,
            specifications: TypeSpecification::Scale {
                minimum: None,
                maximum: None,
                decimals: None,
                precision: None,
                units: ScaleUnits::from(vec![ScaleUnit {
                    name: unit_name.clone(),
                    value: Decimal::from(1),
                }]),
                help: "Format: {value} {unit} (e.g. 100 kilograms)".to_string(),
            },
            extends: TypeExtends::Primitive,
        };
        Self {
            value: ValueKind::Scale(value, unit_name),
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

    pub fn duration(value: Decimal, unit: SemanticDurationUnit) -> Self {
        Self {
            value: ValueKind::Duration(value, unit),
            lemma_type: primitive_duration().clone(),
        }
    }

    pub fn duration_with_type(
        value: Decimal,
        unit: SemanticDurationUnit,
        lemma_type: LemmaType,
    ) -> Self {
        Self {
            value: ValueKind::Duration(value, unit),
            lemma_type,
        }
    }

    pub fn ratio(r: Decimal, unit: Option<String>) -> Self {
        Self {
            value: ValueKind::Ratio(r, unit),
            lemma_type: primitive_ratio().clone(),
        }
    }

    pub fn ratio_with_type(r: Decimal, unit: Option<String>, lemma_type: LemmaType) -> Self {
        Self {
            value: ValueKind::Ratio(r, unit),
            lemma_type,
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

/// Data value: literal, type declaration (resolved type only), or spec reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataValue {
    Literal(LiteralValue),
    TypeDeclaration { resolved_type: LemmaType },
    SpecReference(String),
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Spec reference data: holds the resolved spec.
    SpecRef {
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
    /// constraint list during type resolution; the evaluator falls back to it
    /// when the target value/rule is missing or vetoes for missing data so the
    /// downstream sees the declared default instead of a missing-data veto.
    ///
    /// The reference itself is evaluated by copying the target's value (data path)
    /// or the target rule's result in topological order; `with_data_values`
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
    /// Returns the schema type for value, type-declaration, and reference data; `None` for spec references.
    pub fn schema_type(&self) -> Option<&LemmaType> {
        match self {
            DataDefinition::Value { value, .. } => Some(&value.lemma_type),
            DataDefinition::TypeDeclaration { resolved_type, .. } => Some(resolved_type),
            DataDefinition::Reference { resolved_type, .. } => Some(resolved_type),
            DataDefinition::SpecRef { .. } => None,
        }
    }

    /// Returns the literal value when the data already holds one. A `Reference`'s
    /// value is produced by the evaluator at runtime, so at plan-time it has no
    /// value yet.
    pub fn value(&self) -> Option<&LiteralValue> {
        match self {
            DataDefinition::Value { value, .. } => Some(value),
            DataDefinition::TypeDeclaration { .. }
            | DataDefinition::SpecRef { .. }
            | DataDefinition::Reference { .. } => None,
        }
    }

    /// Schema-level default for this data: the value to surface in
    /// [`SpecSchema::data`]'s `default` field.
    ///
    /// Differs from [`Self::value`]: `Value` data already carries the literal
    /// (a default that planning promoted to a value), so both return the
    /// same thing for that variant. For `Reference` and `TypeDeclaration` the
    /// schema-level default lives separately from the runtime value (the
    /// reference's copied target value, or the type-only data's user-supplied
    /// value); we synthesize the `LiteralValue` from the declared default and
    /// the `resolved_type` here so callers don't have to. `SpecRef` has no
    /// schema default.
    pub fn schema_default(&self) -> Option<LiteralValue> {
        match self {
            DataDefinition::Value { value, .. } => Some(value.clone()),
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
            DataDefinition::TypeDeclaration { .. }
            | DataDefinition::Reference { .. }
            | DataDefinition::SpecRef { .. } => None,
        }
    }

    /// Returns the source location for this data.
    pub fn source(&self) -> &Source {
        match self {
            DataDefinition::Value { source, .. } => source,
            DataDefinition::TypeDeclaration { source, .. } => source,
            DataDefinition::SpecRef { source, .. } => source,
            DataDefinition::Reference { source, .. } => source,
        }
    }

    /// Returns the referenced spec Arc for spec reference data; `None` otherwise.
    pub fn spec_arc(&self) -> Option<&Arc<crate::parsing::ast::LemmaSpec>> {
        match self {
            DataDefinition::Value { .. }
            | DataDefinition::TypeDeclaration { .. }
            | DataDefinition::Reference { .. } => None,
            DataDefinition::SpecRef { spec: spec_arc, .. } => Some(spec_arc),
        }
    }

    /// Returns the referenced spec name for spec reference data; `None` otherwise.
    pub fn spec_ref(&self) -> Option<&str> {
        match self {
            DataDefinition::Value { .. }
            | DataDefinition::TypeDeclaration { .. }
            | DataDefinition::Reference { .. } => None,
            DataDefinition::SpecRef { spec, .. } => Some(&spec.name),
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

/// Convert parser Value to ValueKind. Fails if Scale/Ratio have no unit (strict).
pub fn value_to_semantic(value: &crate::parsing::ast::Value) -> Result<ValueKind, String> {
    use crate::parsing::ast::Value;
    Ok(match value {
        Value::Number(n) => ValueKind::Number(*n),
        Value::Text(s) => ValueKind::Text(s.clone()),
        Value::Boolean(b) => ValueKind::Boolean(bool::from(*b)),
        Value::Date(dt) => ValueKind::Date(date_time_to_semantic(dt)),
        Value::Time(t) => ValueKind::Time(time_to_semantic(t)),
        Value::Duration(n, u) => ValueKind::Duration(*n, duration_unit_to_semantic(u)),
        Value::Scale(n, unit) => ValueKind::Scale(*n, unit.clone()),
        Value::Ratio(n, unit) => ValueKind::Ratio(*n, unit.clone()),
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
        timezone: t.timezone.as_ref().map(|tz| SemanticTimezone {
            offset_hours: tz.offset_hours,
            offset_minutes: tz.offset_minutes,
        }),
    }
}

/// Compare two semantic date-time values by year, month, day, hour, minute, second.
///
/// Microsecond and timezone are intentionally excluded so the ordering matches
/// what user-facing date constraints can express (lemma date literals do not
/// expose sub-second precision, and timezone normalisation is a separate concern
/// handled at evaluation time).
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
}

/// Compare two semantic time values by hour, minute, second.
///
/// Timezone is excluded for the same reason as [`compare_semantic_dates`].
pub(crate) fn compare_semantic_times(
    left: &SemanticTime,
    right: &SemanticTime,
) -> std::cmp::Ordering {
    left.hour
        .cmp(&right.hour)
        .then_with(|| left.minute.cmp(&right.minute))
        .then_with(|| left.second.cmp(&right.second))
}

/// Convert AST duration unit to semantic (for tests and planning).
pub(crate) fn duration_unit_to_semantic(
    u: &crate::parsing::ast::DurationUnit,
) -> SemanticDurationUnit {
    use crate::parsing::ast::DurationUnit as DU;
    match u {
        DU::Year => SemanticDurationUnit::Year,
        DU::Month => SemanticDurationUnit::Month,
        DU::Week => SemanticDurationUnit::Week,
        DU::Day => SemanticDurationUnit::Day,
        DU::Hour => SemanticDurationUnit::Hour,
        DU::Minute => SemanticDurationUnit::Minute,
        DU::Second => SemanticDurationUnit::Second,
        DU::Millisecond => SemanticDurationUnit::Millisecond,
        DU::Microsecond => SemanticDurationUnit::Microsecond,
    }
}

/// Convert AST conversion target to semantic (planning boundary; evaluation/computation use only semantic).
///
/// The AST uses `ConversionTarget::Unit(name)` for non-duration units; this function looks up `name`
/// in the spec's unit index and returns `RatioUnit` or `ScaleUnit` based on the type that defines
/// the unit. The unit must be defined by a scale or ratio type in the spec (e.g. primitive ratio for
/// "percent", "permille").
pub fn conversion_target_to_semantic(
    ct: &ConversionTarget,
    unit_index: Option<&HashMap<String, LemmaType>>,
) -> Result<SemanticConversionTarget, String> {
    match ct {
        ConversionTarget::Duration(u) => Ok(SemanticConversionTarget::Duration(
            duration_unit_to_semantic(u),
        )),
        ConversionTarget::Unit(name) => {
            let index = unit_index.ok_or_else(|| {
                "Unit conversion requires type resolution; unit index not available.".to_string()
            })?;
            let lemma_type = index.get(name).ok_or_else(|| {
                format!(
                    "Unknown unit '{}'. Unit must be defined by a scale or ratio type.",
                    name
                )
            })?;
            if lemma_type.is_ratio() {
                Ok(SemanticConversionTarget::RatioUnit(name.clone()))
            } else if lemma_type.is_scale() {
                Ok(SemanticConversionTarget::ScaleUnit(name.clone()))
            } else {
                Err(format!(
                    "Unit '{}' is not a ratio or scale type; cannot use it in conversion.",
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
static PRIMITIVE_SCALE: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_NUMBER: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_TEXT: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_DATE: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_TIME: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_DURATION: OnceLock<LemmaType> = OnceLock::new();
static PRIMITIVE_RATIO: OnceLock<LemmaType> = OnceLock::new();

/// Primitive types use the default TypeSpecification from the parser (single source of truth).
#[must_use]
pub fn primitive_boolean() -> &'static LemmaType {
    PRIMITIVE_BOOLEAN.get_or_init(|| LemmaType::primitive(TypeSpecification::boolean()))
}

#[must_use]
pub fn primitive_scale() -> &'static LemmaType {
    PRIMITIVE_SCALE.get_or_init(|| LemmaType::primitive(TypeSpecification::scale()))
}

#[must_use]
pub fn primitive_number() -> &'static LemmaType {
    PRIMITIVE_NUMBER.get_or_init(|| LemmaType::primitive(TypeSpecification::number()))
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
pub fn primitive_time() -> &'static LemmaType {
    PRIMITIVE_TIME.get_or_init(|| LemmaType::primitive(TypeSpecification::time()))
}

#[must_use]
pub fn primitive_duration() -> &'static LemmaType {
    PRIMITIVE_DURATION.get_or_init(|| LemmaType::primitive(TypeSpecification::duration()))
}

#[must_use]
pub fn primitive_ratio() -> &'static LemmaType {
    PRIMITIVE_RATIO.get_or_init(|| LemmaType::primitive(TypeSpecification::ratio()))
}

/// Map PrimitiveKind to TypeSpecification. Single source of truth for primitive type resolution.
#[must_use]
pub fn type_spec_for_primitive(kind: PrimitiveKind) -> TypeSpecification {
    match kind {
        PrimitiveKind::Boolean => TypeSpecification::boolean(),
        PrimitiveKind::Scale => TypeSpecification::scale(),
        PrimitiveKind::Number => TypeSpecification::number(),
        PrimitiveKind::Percent | PrimitiveKind::Ratio => TypeSpecification::ratio(),
        PrimitiveKind::Text => TypeSpecification::text(),
        PrimitiveKind::Date => TypeSpecification::date(),
        PrimitiveKind::Time => TypeSpecification::time(),
        PrimitiveKind::Duration => TypeSpecification::duration(),
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
        match &self.value {
            ValueKind::Scale(n, u) => {
                if let TypeSpecification::Scale { decimals, .. } = &self.lemma_type.specifications {
                    let s = match decimals {
                        Some(d) => {
                            let dp = u32::from(*d);
                            let rounded = n.round_dp(dp);
                            format!("{:.prec$}", rounded, prec = *d as usize)
                        }
                        None => n.normalize().to_string(),
                    };
                    return write!(f, "{} {}", s, u);
                }
                write!(f, "{}", self.value)
            }
            ValueKind::Ratio(r, Some(unit_name)) => {
                if let TypeSpecification::Ratio { units, .. } = &self.lemma_type.specifications {
                    if let Ok(unit) = units.get(unit_name) {
                        let display_value = (*r * unit.value).normalize();
                        let s = if display_value.fract().is_zero() {
                            display_value.trunc().to_string()
                        } else {
                            display_value.to_string()
                        };
                        // Use shorthand symbols for percent (%) and permille (%%)
                        return match unit_name.as_str() {
                            "percent" => write!(f, "{}%", s),
                            "permille" => write!(f, "{}%%", s),
                            _ => write!(f, "{} {}", s, unit_name),
                        };
                    }
                }
                write!(f, "{}", self.value)
            }
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
    use crate::parsing::ast::{BooleanValue, DateTimeValue, DurationUnit, LemmaSpec, TimeValue};
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use std::sync::Arc;

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
    fn test_literal_value_to_primitive_type() {
        let one = Decimal::from_str("1").unwrap();

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
            LiteralValue::ratio(one / Decimal::from(100), Some("percent".to_string()))
                .lemma_type
                .name(),
            "ratio"
        );
        assert_eq!(
            LiteralValue::duration(one, duration_unit_to_semantic(&DurationUnit::Second))
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
        let ten = Decimal::from_str("10").unwrap();

        assert_eq!(
            LiteralValue::text("hello".to_string()).display_value(),
            "hello"
        );
        assert_eq!(LiteralValue::number(ten).display_value(), "10");
        assert_eq!(LiteralValue::from_bool(true).display_value(), "true");
        assert_eq!(LiteralValue::from_bool(false).display_value(), "false");

        // 0.10 ratio with "percent" unit displays as 10% (unit conversion applied)
        let ten_percent_ratio = LiteralValue::ratio(
            Decimal::from_str("0.10").unwrap(),
            Some("percent".to_string()),
        );
        assert_eq!(ten_percent_ratio.display_value(), "10%");

        let time = TimeValue {
            hour: 14,
            minute: 30,
            second: 0,
            timezone: None,
        };
        let time_display = LiteralValue::time(time_to_semantic(&time)).display_value();
        assert!(time_display.contains("14"));
        assert!(time_display.contains("30"));
    }

    #[test]
    fn test_scale_display_respects_type_decimals() {
        let money_type = LemmaType {
            name: Some("money".to_string()),
            specifications: TypeSpecification::Scale {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                precision: None,
                units: ScaleUnits::from(vec![ScaleUnit {
                    name: "eur".to_string(),
                    value: Decimal::from(1),
                }]),
                help: String::new(),
            },
            extends: TypeExtends::Primitive,
        };
        let val = LiteralValue::scale_with_type(
            Decimal::from_str("1.8").unwrap(),
            "eur".to_string(),
            money_type.clone(),
        );
        assert_eq!(val.display_value(), "1.80 eur");
        let more_precision = LiteralValue::scale_with_type(
            Decimal::from_str("1.80000").unwrap(),
            "eur".to_string(),
            money_type,
        );
        assert_eq!(more_precision.display_value(), "1.80 eur");
        let scale_no_decimals = LemmaType {
            name: Some("count".to_string()),
            specifications: TypeSpecification::Scale {
                minimum: None,
                maximum: None,
                decimals: None,
                precision: None,
                units: ScaleUnits::from(vec![ScaleUnit {
                    name: "items".to_string(),
                    value: Decimal::from(1),
                }]),
                help: String::new(),
            },
            extends: TypeExtends::Primitive,
        };
        let val_any = LiteralValue::scale_with_type(
            Decimal::from_str("42.50").unwrap(),
            "items".to_string(),
            scale_no_decimals,
        );
        assert_eq!(val_any.display_value(), "42.5 items");
    }

    #[test]
    fn test_literal_value_time_type() {
        let time = TimeValue {
            hour: 14,
            minute: 30,
            second: 0,
            timezone: None,
        };
        let lit = LiteralValue::time(time_to_semantic(&time));
        assert_eq!(lit.lemma_type.name(), "time");
    }

    #[test]
    fn test_scale_family_name_primitive_root() {
        let scale_spec = TypeSpecification::scale();
        let money_primitive = LemmaType::new(
            "money".to_string(),
            scale_spec.clone(),
            TypeExtends::Primitive,
        );
        assert_eq!(money_primitive.scale_family_name(), Some("money"));
    }

    #[test]
    fn test_scale_family_name_custom() {
        let scale_spec = TypeSpecification::scale();
        let money_custom = LemmaType::new(
            "money".to_string(),
            scale_spec,
            TypeExtends::custom_local("money".to_string(), "money".to_string()),
        );
        assert_eq!(money_custom.scale_family_name(), Some("money"));
    }

    #[test]
    fn test_same_scale_family_same_name_different_extends() {
        let scale_spec = TypeSpecification::scale();
        let money_primitive = LemmaType::new(
            "money".to_string(),
            scale_spec.clone(),
            TypeExtends::Primitive,
        );
        let money_custom = LemmaType::new(
            "money".to_string(),
            scale_spec,
            TypeExtends::custom_local("money".to_string(), "money".to_string()),
        );
        assert!(money_primitive.same_scale_family(&money_custom));
        assert!(money_custom.same_scale_family(&money_primitive));
    }

    #[test]
    fn test_same_scale_family_parent_and_child() {
        let scale_spec = TypeSpecification::scale();
        let type_x = LemmaType::new("x".to_string(), scale_spec.clone(), TypeExtends::Primitive);
        let type_x2 = LemmaType::new(
            "x2".to_string(),
            scale_spec,
            TypeExtends::custom_local("x".to_string(), "x".to_string()),
        );
        assert_eq!(type_x.scale_family_name(), Some("x"));
        assert_eq!(type_x2.scale_family_name(), Some("x"));
        assert!(type_x.same_scale_family(&type_x2));
        assert!(type_x2.same_scale_family(&type_x));
    }

    #[test]
    fn test_same_scale_family_siblings() {
        let scale_spec = TypeSpecification::scale();
        let type_x2_a = LemmaType::new(
            "x2a".to_string(),
            scale_spec.clone(),
            TypeExtends::custom_local("x".to_string(), "x".to_string()),
        );
        let type_x2_b = LemmaType::new(
            "x2b".to_string(),
            scale_spec,
            TypeExtends::custom_local("x".to_string(), "x".to_string()),
        );
        assert!(type_x2_a.same_scale_family(&type_x2_b));
    }

    #[test]
    fn test_same_scale_family_different_families() {
        let scale_spec = TypeSpecification::scale();
        let money = LemmaType::new(
            "money".to_string(),
            scale_spec.clone(),
            TypeExtends::Primitive,
        );
        let temperature = LemmaType::new(
            "temperature".to_string(),
            scale_spec,
            TypeExtends::Primitive,
        );
        assert!(!money.same_scale_family(&temperature));
        assert!(!temperature.same_scale_family(&money));
    }

    #[test]
    fn test_same_scale_family_scale_vs_non_scale() {
        let scale_spec = TypeSpecification::scale();
        let number_spec = TypeSpecification::number();
        let scale_type = LemmaType::new("money".to_string(), scale_spec, TypeExtends::Primitive);
        let number_type = LemmaType::new("amount".to_string(), number_spec, TypeExtends::Primitive);
        assert!(!scale_type.same_scale_family(&number_type));
        assert!(!number_type.same_scale_family(&scale_type));
    }

    #[test]
    fn test_scale_family_name_non_scale_returns_none() {
        let number_spec = TypeSpecification::number();
        let number_type = LemmaType::new("amount".to_string(), number_spec, TypeExtends::Primitive);
        assert_eq!(number_type.scale_family_name(), None);
    }

    #[test]
    fn test_lemma_type_inequality_local_vs_import_same_shape() {
        let dep = Arc::new(LemmaSpec::new("dep".to_string()));
        let scale_spec = TypeSpecification::scale();
        let local = LemmaType::new(
            "t".to_string(),
            scale_spec.clone(),
            TypeExtends::custom_local("money".to_string(), "money".to_string()),
        );
        let imported = LemmaType::new(
            "t".to_string(),
            scale_spec,
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
    fn test_lemma_type_equality_import_same_resolved_spec_semantics() {
        let spec_a = Arc::new(LemmaSpec::new("dep".to_string()));
        let spec_b = Arc::new(LemmaSpec::new("dep".to_string()));
        assert!(is_same_spec(spec_a.as_ref(), spec_b.as_ref()));
        let scale_spec = TypeSpecification::scale();
        let left = LemmaType::new(
            "t".to_string(),
            scale_spec.clone(),
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
            scale_spec,
            TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: TypeDefiningSpec::Import { spec: spec_b },
            },
        );
        assert_eq!(left, right);
    }
}
