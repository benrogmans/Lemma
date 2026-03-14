//! AST types
//!
//! Infrastructure (Span, DepthTracker) and spec/fact/rule/expression/value types from parsing.
//!
//! # `AsLemmaSource<T>` wrapper
//!
//! For types that need to emit valid, round-trippable Lemma source (e.g. constraint
//! args like `help`, `default`, `option`), wrap a reference in [`AsLemmaSource`] and
//! use its `Display` implementation. The regular `Display` impls on AST types are for
//! human-readable output (error messages, debug); `AsLemmaSource` emits **valid Lemma syntax**.
//!
//! ```ignore
//! use lemma::parsing::ast::{AsLemmaSource, FactValue};
//! let s = format!("{}", AsLemmaSource(&fact_value));
//! ```

/// Span representing a location in source code
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

/// Tracks expression nesting depth during parsing to prevent stack overflow
pub struct DepthTracker {
    depth: usize,
    max_depth: usize,
}

impl DepthTracker {
    pub fn with_max_depth(max_depth: usize) -> Self {
        Self {
            depth: 0,
            max_depth,
        }
    }

    /// Returns Ok(()) if within limits, Err(current_depth) if exceeded.
    pub fn push_depth(&mut self) -> Result<(), usize> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(self.depth);
        }
        Ok(())
    }

    pub fn pop_depth(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    pub fn max_depth(&self) -> usize {
        self.max_depth
    }
}

impl Default for DepthTracker {
    fn default() -> Self {
        Self {
            depth: 0,
            max_depth: 5,
        }
    }
}

// -----------------------------------------------------------------------------
// Spec, fact, rule, expression and value types
// -----------------------------------------------------------------------------

use crate::parsing::source::Source;
use chrono::{Datelike, Timelike};
use rust_decimal::Decimal;
use serde::Serialize;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// A Lemma spec containing facts and rules.
/// Ordered and compared by (name, effective_from) for use in BTreeSet; None < Some(_) for Option<DateTimeValue>.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LemmaSpec {
    /// Base spec name. Includes `@` for registry specs.
    pub name: String,
    /// `true` when the spec was declared with the `@` qualifier (registry spec).
    pub from_registry: bool,
    pub effective_from: Option<DateTimeValue>,
    pub attribute: Option<String>,
    pub start_line: usize,
    pub commentary: Option<String>,
    pub types: Vec<TypeDef>,
    pub facts: Vec<LemmaFact>,
    pub rules: Vec<LemmaRule>,
    pub meta_fields: Vec<MetaField>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MetaField {
    pub key: String,
    pub value: MetaValue,
    pub source_location: Source,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaValue {
    Literal(Value),
    Unquoted(String),
}

impl fmt::Display for MetaValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetaValue::Literal(v) => write!(f, "{}", v),
            MetaValue::Unquoted(s) => write!(f, "{}", s),
        }
    }
}

impl fmt::Display for MetaField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "meta {}: {}", self.key, self.value)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LemmaFact {
    pub reference: Reference,
    pub value: FactValue,
    pub source_location: Source,
}

/// An unless clause that provides an alternative result
///
/// Unless clauses are evaluated in order, and the last matching condition wins.
/// This matches natural language: "X unless A then Y, unless B then Z" - if both
/// A and B are true, Z is returned (the last match).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnlessClause {
    pub condition: Expression,
    pub result: Expression,
    pub source_location: Source,
}

/// A rule with a single expression and optional unless clauses
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LemmaRule {
    pub name: String,
    pub expression: Expression,
    pub unless_clauses: Vec<UnlessClause>,
    pub source_location: Source,
}

/// An expression that can be evaluated, with source location
///
/// Expressions use semantic equality - two expressions with the same
/// structure (kind) are equal regardless of source location.
/// Hash is not implemented for AST Expression; use planning::semantics::Expression as map keys.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub source_location: Option<Source>,
}

impl Expression {
    /// Create a new expression with kind and source location
    #[must_use]
    pub fn new(kind: ExpressionKind, source_location: Source) -> Self {
        Self {
            kind,
            source_location: Some(source_location),
        }
    }

    /// Get the source text for this expression from the given sources map
    ///
    /// Returns `None` if the source is not found.
    pub fn get_source_text(
        &self,
        sources: &std::collections::HashMap<String, String>,
    ) -> Option<String> {
        let loc = self.source_location.as_ref()?;
        sources
            .get(&loc.attribute)
            .and_then(|source| loc.extract_text(source))
    }
}

/// Semantic equality - compares expressions by structure only, ignoring source location
impl PartialEq for Expression {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Eq for Expression {}

/// Whether a date is relative to `now` in the past or future direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateRelativeKind {
    InPast,
    InFuture,
}

/// Calendar-period membership checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateCalendarKind {
    Current,
    Past,
    Future,
    NotIn,
}

/// Granularity of a calendar-period check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarUnit {
    Year,
    Month,
    Week,
}

impl fmt::Display for DateRelativeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DateRelativeKind::InPast => write!(f, "in past"),
            DateRelativeKind::InFuture => write!(f, "in future"),
        }
    }
}

impl fmt::Display for DateCalendarKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DateCalendarKind::Current => write!(f, "in calendar"),
            DateCalendarKind::Past => write!(f, "in past calendar"),
            DateCalendarKind::Future => write!(f, "in future calendar"),
            DateCalendarKind::NotIn => write!(f, "not in calendar"),
        }
    }
}

impl fmt::Display for CalendarUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalendarUnit::Year => write!(f, "year"),
            CalendarUnit::Month => write!(f, "month"),
            CalendarUnit::Week => write!(f, "week"),
        }
    }
}

/// The kind/type of expression
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionKind {
    /// Parse-time literal value (type will be resolved during planning)
    Literal(Value),
    /// Unresolved reference (identifier or dot path). Resolved during planning to FactPath or RulePath.
    Reference(Reference),
    /// Unresolved unit literal from parser (resolved during planning)
    /// Contains (number, unit_name) - the unit name will be resolved to its type during semantic analysis
    UnresolvedUnitLiteral(Decimal, String),
    /// The `now` keyword — resolves to the evaluation datetime (= effective).
    Now,
    /// Date-relative sugar: `<date_expr> in past [<duration_expr>]` / `<date_expr> in future [<duration_expr>]`
    /// Fields: (kind, date_expression, optional_tolerance_expression)
    DateRelative(DateRelativeKind, Arc<Expression>, Option<Arc<Expression>>),
    /// Calendar-period sugar: `<date_expr> in [past|future] calendar year|month|week`
    /// Fields: (kind, unit, date_expression)
    DateCalendar(DateCalendarKind, CalendarUnit, Arc<Expression>),
    LogicalAnd(Arc<Expression>, Arc<Expression>),
    Arithmetic(Arc<Expression>, ArithmeticComputation, Arc<Expression>),
    Comparison(Arc<Expression>, ComparisonComputation, Arc<Expression>),
    UnitConversion(Arc<Expression>, ConversionTarget),
    LogicalNegation(Arc<Expression>, NegationType),
    MathematicalComputation(MathematicalComputation, Arc<Expression>),
    Veto(VetoExpression),
}

/// Unresolved reference from parser
///
/// Reference to a fact or rule (identifier or dot path).
///
/// Used in expressions and in LemmaFact. During planning, references
/// are resolved to FactPath or RulePath (semantics layer).
/// Examples:
/// - Local "age": segments=[], name="age"
/// - Cross-spec "employee.salary": segments=["employee"], name="salary"
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Reference {
    pub segments: Vec<String>,
    pub name: String,
}

impl Reference {
    #[must_use]
    pub fn local(name: String) -> Self {
        Self {
            segments: Vec::new(),
            name,
        }
    }

    #[must_use]
    pub fn from_path(path: Vec<String>) -> Self {
        if path.is_empty() {
            Self {
                segments: Vec::new(),
                name: String::new(),
            }
        } else {
            // Safe: path is non-empty.
            let name = path[path.len() - 1].clone();
            let segments = path[..path.len() - 1].to_vec();
            Self { segments, name }
        }
    }

    #[must_use]
    pub fn is_local(&self) -> bool {
        self.segments.is_empty()
    }

    #[must_use]
    pub fn full_path(&self) -> Vec<String> {
        let mut path = self.segments.clone();
        path.push(self.name.clone());
        path
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            write!(f, "{}.", segment)?;
        }
        write!(f, "{}", self.name)
    }
}

/// Arithmetic computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArithmeticComputation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
}

impl ArithmeticComputation {
    /// Returns the operator symbol
    #[must_use]
    pub fn symbol(&self) -> &'static str {
        match self {
            ArithmeticComputation::Add => "+",
            ArithmeticComputation::Subtract => "-",
            ArithmeticComputation::Multiply => "*",
            ArithmeticComputation::Divide => "/",
            ArithmeticComputation::Modulo => "%",
            ArithmeticComputation::Power => "^",
        }
    }
}

/// Comparison computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonComputation {
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Equal,
    NotEqual,
    Is,
    IsNot,
}

impl ComparisonComputation {
    /// Returns the operator symbol
    #[must_use]
    pub fn symbol(&self) -> &'static str {
        match self {
            ComparisonComputation::GreaterThan => ">",
            ComparisonComputation::LessThan => "<",
            ComparisonComputation::GreaterThanOrEqual => ">=",
            ComparisonComputation::LessThanOrEqual => "<=",
            ComparisonComputation::Equal => "==",
            ComparisonComputation::NotEqual => "!=",
            ComparisonComputation::Is => "is",
            ComparisonComputation::IsNot => "is not",
        }
    }

    /// Check if this is an equality comparison (== or is)
    #[must_use]
    pub fn is_equal(&self) -> bool {
        matches!(
            self,
            ComparisonComputation::Equal | ComparisonComputation::Is
        )
    }

    /// Check if this is an inequality comparison (!= or is not)
    #[must_use]
    pub fn is_not_equal(&self) -> bool {
        matches!(
            self,
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot
        )
    }
}

/// The target unit for unit conversion expressions.
/// Non-duration units (e.g. "percent", "eur") are stored as Unit and resolved to ratio or scale during planning via the unit index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionTarget {
    Duration(DurationUnit),
    Unit(String),
}

/// Types of logical negation
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NegationType {
    Not,
}

/// A veto expression that prohibits any valid verdict from the rule
///
/// Unlike `reject` (which is just an alias for boolean `false`), a veto
/// prevents the rule from producing any valid result. This is used for
/// validation and constraint enforcement.
///
/// Example: `veto "Must be over 18"` - blocks the rule entirely with a message
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VetoExpression {
    pub message: Option<String>,
}

/// Mathematical computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MathematicalComputation {
    Sqrt,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Log,
    Exp,
    Abs,
    Floor,
    Ceil,
    Round,
}

/// A reference to a spec, with optional hash pin and optional effective datetime.
/// For registry references the `name` includes the leading `@` (e.g. `@org/repo/spec`);
/// for local references it is a plain base name.  `from_registry` mirrors whether
/// the source used the `@` qualifier; `hash_pin` pins to a specific temporal version
/// by content hash; `effective` requests temporal resolution at that datetime.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpecRef {
    /// Spec name as written in source. Includes `@` for registry references.
    pub name: String,
    /// `true` when the source used the `@` qualifier (registry reference).
    pub from_registry: bool,
    /// Optional content hash pin to resolve to a specific spec version.
    pub hash_pin: Option<String>,
    /// Optional effective datetime for temporal resolution. When used with `hash_pin`, resolve by hash then verify that version was active at this datetime.
    pub effective: Option<DateTimeValue>,
}

impl std::fmt::Display for SpecRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some(ref h) = self.hash_pin {
            write!(f, "~{}", h)?;
        }
        if let Some(ref d) = self.effective {
            write!(f, " {}", d)?;
        }
        Ok(())
    }
}

impl SpecRef {
    /// Create a local (non-registry) spec reference.
    pub fn local(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            from_registry: false,
            hash_pin: None,
            effective: None,
        }
    }

    /// Create a registry spec reference.
    pub fn registry(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            from_registry: true,
            hash_pin: None,
            effective: None,
        }
    }

    pub fn resolution_key(&self) -> String {
        self.name.clone()
    }
}

/// A parsed constraint command argument, preserving the literal kind from the
/// grammar rule `command_arg: { number_literal | boolean_literal | text_literal | label }`.
///
/// The parser sets the variant based on which grammar alternative matched.
/// This information is used by:
/// - **Planning** to validate that argument literal kinds match the expected type
///   (e.g. reject a `Text` literal where a `Number` is required).
/// - **Formatting** to emit correct Lemma syntax (quote `Text`, emit others as-is).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CommandArg {
    /// Matched `number_literal` (e.g. `10`, `3.14`)
    Number(String),
    /// Matched `boolean_literal` (e.g. `true`, `false`, `yes`, `no`, `accept`, `reject`)
    Boolean(String),
    /// Matched `text_literal` (e.g. `"hello"`) — stores the content between quotes,
    /// without surrounding quote characters.
    Text(String),
    /// Matched `label` (an identifier: `eur`, `kilogram`, `hours`)
    Label(String),
}

impl CommandArg {
    /// Returns the inner string value regardless of which literal kind was parsed.
    ///
    /// Use this when you need the raw string content for further processing
    /// (e.g. `.parse::<Decimal>()`) but do not need to distinguish the literal kind.
    pub fn value(&self) -> &str {
        match self {
            CommandArg::Number(s)
            | CommandArg::Boolean(s)
            | CommandArg::Text(s)
            | CommandArg::Label(s) => s,
        }
    }
}

impl fmt::Display for CommandArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// A single constraint command: name and its typed arguments.
pub type Constraint = (String, Vec<CommandArg>);

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
/// Parse-time fact value (before type resolution)
pub enum FactValue {
    /// A literal value (parse-time; type will be resolved during planning)
    Literal(Value),
    /// A reference to another spec
    SpecReference(SpecRef),
    /// A type declaration (inline type annotation on a fact)
    TypeDeclaration {
        base: String,
        constraints: Option<Vec<Constraint>>,
        from: Option<SpecRef>,
    },
}

/// A type for type declarations
/// Boolean value with original input preserved
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    serde::Deserialize,
    strum_macros::EnumString,
    strum_macros::Display,
)]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum BooleanValue {
    True,
    False,
    Yes,
    No,
    Accept,
    Reject,
}

impl From<BooleanValue> for bool {
    fn from(value: BooleanValue) -> bool {
        match value {
            BooleanValue::True | BooleanValue::Yes | BooleanValue::Accept => true,
            BooleanValue::False | BooleanValue::No | BooleanValue::Reject => false,
        }
    }
}

impl From<&BooleanValue> for bool {
    fn from(value: &BooleanValue) -> bool {
        match value {
            BooleanValue::True | BooleanValue::Yes | BooleanValue::Accept => true,
            BooleanValue::False | BooleanValue::No | BooleanValue::Reject => false,
        }
    }
}

impl From<bool> for BooleanValue {
    fn from(value: bool) -> BooleanValue {
        if value {
            BooleanValue::True
        } else {
            BooleanValue::False
        }
    }
}

impl std::ops::Not for BooleanValue {
    type Output = BooleanValue;

    fn not(self) -> Self::Output {
        if self.into() {
            BooleanValue::False
        } else {
            BooleanValue::True
        }
    }
}

impl std::ops::Not for &BooleanValue {
    type Output = BooleanValue;

    fn not(self) -> Self::Output {
        if self.into() {
            BooleanValue::False
        } else {
            BooleanValue::True
        }
    }
}

/// The actual value data (without type information)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Number(Decimal),
    Scale(Decimal, String),
    Text(String),
    Date(DateTimeValue),
    Time(TimeValue),
    Boolean(BooleanValue),
    Duration(Decimal, DurationUnit),
    Ratio(Decimal, Option<String>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{}", n),
            Value::Text(s) => write!(f, "{}", s),
            Value::Date(dt) => write!(f, "{}", dt),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Time(time) => write!(f, "{}", time),
            Value::Scale(n, u) => write!(f, "{} {}", n, u),
            Value::Duration(n, u) => write!(f, "{} {}", n, u),
            Value::Ratio(n, u) => match u.as_deref() {
                Some("percent") => {
                    let display_value = *n * Decimal::from(100);
                    let norm = display_value.normalize();
                    let s = if norm.fract().is_zero() {
                        norm.trunc().to_string()
                    } else {
                        norm.to_string()
                    };
                    write!(f, "{}%", s)
                }
                Some("permille") => {
                    let display_value = *n * Decimal::from(1000);
                    let norm = display_value.normalize();
                    let s = if norm.fract().is_zero() {
                        norm.trunc().to_string()
                    } else {
                        norm.to_string()
                    };
                    write!(f, "{}%%", s)
                }
                Some(unit) => {
                    let norm = n.normalize();
                    let s = if norm.fract().is_zero() {
                        norm.trunc().to_string()
                    } else {
                        norm.to_string()
                    };
                    write!(f, "{} {}", s, unit)
                }
                None => {
                    let norm = n.normalize();
                    let s = if norm.fract().is_zero() {
                        norm.trunc().to_string()
                    } else {
                        norm.to_string()
                    };
                    write!(f, "{}", s)
                }
            },
        }
    }
}

impl fmt::Display for FactValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FactValue::Literal(v) => write!(f, "{}", v),
            FactValue::SpecReference(spec_ref) => {
                write!(f, "spec {}", spec_ref)
            }
            FactValue::TypeDeclaration {
                base,
                constraints,
                from,
            } => {
                let base_str = if let Some(from_spec) = from {
                    format!("{} from {}", base, from_spec)
                } else {
                    base.clone()
                };
                if let Some(ref constraints_vec) = constraints {
                    let constraint_str = constraints_vec
                        .iter()
                        .map(|(cmd, args)| {
                            let args_str: Vec<&str> = args.iter().map(|a| a.value()).collect();
                            let joined = args_str.join(" ");
                            if joined.is_empty() {
                                cmd.clone()
                            } else {
                                format!("{} {}", cmd, joined)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" -> ");
                    write!(f, "[{} -> {}]", base_str, constraint_str)
                } else {
                    write!(f, "[{}]", base_str)
                }
            }
        }
    }
}

/// A time value
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, serde::Deserialize,
)]
pub struct TimeValue {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub timezone: Option<TimezoneValue>,
}

/// A timezone value
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, serde::Deserialize)]
pub struct TimezoneValue {
    pub offset_hours: i8,
    pub offset_minutes: u8,
}

/// A datetime value that preserves timezone information.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, serde::Deserialize)]
pub struct DateTimeValue {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    #[serde(default)]
    pub microsecond: u32,
    pub timezone: Option<TimezoneValue>,
}

impl DateTimeValue {
    pub fn now() -> Self {
        let now = chrono::Local::now();
        let offset_secs = now.offset().local_minus_utc();
        Self {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            hour: now.time().hour(),
            minute: now.time().minute(),
            second: now.time().second(),
            microsecond: now.time().nanosecond() / 1000 % 1_000_000,
            timezone: Some(TimezoneValue {
                offset_hours: (offset_secs / 3600) as i8,
                offset_minutes: ((offset_secs.abs() % 3600) / 60) as u8,
            }),
        }
    }

    /// Parse a datetime string. Accepts:
    /// - Full ISO 8601: `2026-03-04T10:30:00Z`, `2026-03-04T10:30:00+02:00`
    /// - Date + time without tz: `2026-03-04T10:30:00`
    /// - Date only: `2026-03-04` (midnight)
    /// - ISO week: `2026-W08` (Monday of ISO week)
    /// - Year-month: `2026-03` (first of month)
    /// - Year only: `2026` (Jan 1)
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(dtv) = crate::parsing::literals::parse_datetime_str(s) {
            return Some(dtv);
        }
        if let Some(week_val) = Self::parse_iso_week(s) {
            return Some(week_val);
        }
        if let Ok(ym) = chrono::NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d") {
            return Some(Self {
                year: ym.year(),
                month: ym.month(),
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                microsecond: 0,
                timezone: None,
            });
        }
        if let Ok(year) = s.parse::<i32>() {
            if (1..=9999).contains(&year) {
                return Some(Self {
                    year,
                    month: 1,
                    day: 1,
                    hour: 0,
                    minute: 0,
                    second: 0,
                    microsecond: 0,
                    timezone: None,
                });
            }
        }
        None
    }

    /// Parse ISO week date format: `YYYY-Www` (e.g. `2026-W08`).
    /// Returns the Monday of that ISO week.
    fn parse_iso_week(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split("-W").collect();
        if parts.len() != 2 {
            return None;
        }
        let year: i32 = parts[0].parse().ok()?;
        let week: u32 = parts[1].parse().ok()?;
        if week == 0 || week > 53 {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)?;
        Some(Self {
            year: date.year(),
            month: date.month(),
            day: date.day(),
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        })
    }
}

/// Unit types for different physical quantities
macro_rules! impl_unit_serialize {
    ($($unit_type:ty),+) => {
        $(
            impl Serialize for $unit_type {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: serde::Serializer,
                {
                    serializer.serialize_str(&self.to_string())
                }
            }
        )+
    };
}

impl_unit_serialize!(DurationUnit);

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize, strum_macros::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum DurationUnit {
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

impl fmt::Display for DurationUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            DurationUnit::Year => "years",
            DurationUnit::Month => "months",
            DurationUnit::Week => "weeks",
            DurationUnit::Day => "days",
            DurationUnit::Hour => "hours",
            DurationUnit::Minute => "minutes",
            DurationUnit::Second => "seconds",
            DurationUnit::Millisecond => "milliseconds",
            DurationUnit::Microsecond => "microseconds",
        };
        write!(f, "{}", s)
    }
}

//

impl LemmaFact {
    #[must_use]
    pub fn new(reference: Reference, value: FactValue, source_location: Source) -> Self {
        Self {
            reference,
            value,
            source_location,
        }
    }
}

impl LemmaSpec {
    #[must_use]
    pub fn new(name: String) -> Self {
        let from_registry = name.starts_with('@');
        Self {
            name,
            from_registry,
            effective_from: None,
            attribute: None,
            start_line: 1,
            commentary: None,
            types: Vec::new(),
            facts: Vec::new(),
            rules: Vec::new(),
            meta_fields: Vec::new(),
        }
    }

    /// Temporal range start. None means −∞.
    pub fn effective_from(&self) -> Option<&DateTimeValue> {
        self.effective_from.as_ref()
    }

    #[must_use]
    pub fn with_attribute(mut self, attribute: String) -> Self {
        self.attribute = Some(attribute);
        self
    }

    #[must_use]
    pub fn with_start_line(mut self, start_line: usize) -> Self {
        self.start_line = start_line;
        self
    }

    #[must_use]
    pub fn set_commentary(mut self, commentary: String) -> Self {
        self.commentary = Some(commentary);
        self
    }

    #[must_use]
    pub fn add_fact(mut self, fact: LemmaFact) -> Self {
        self.facts.push(fact);
        self
    }

    #[must_use]
    pub fn add_rule(mut self, rule: LemmaRule) -> Self {
        self.rules.push(rule);
        self
    }

    #[must_use]
    pub fn add_type(mut self, type_def: TypeDef) -> Self {
        self.types.push(type_def);
        self
    }

    #[must_use]
    pub fn add_meta_field(mut self, meta: MetaField) -> Self {
        self.meta_fields.push(meta);
        self
    }
}

impl PartialEq for LemmaSpec {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.effective_from() == other.effective_from()
    }
}

impl Eq for LemmaSpec {}

impl PartialOrd for LemmaSpec {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LemmaSpec {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.name.as_str(), self.effective_from())
            .cmp(&(other.name.as_str(), other.effective_from()))
    }
}

impl Hash for LemmaSpec {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        match self.effective_from() {
            Some(d) => d.hash(state),
            None => 0u8.hash(state),
        }
    }
}

impl fmt::Display for LemmaSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "spec {}", self.name)?;
        if let Some(ref af) = self.effective_from {
            write!(f, " {}", af)?;
        }
        writeln!(f)?;

        if let Some(ref commentary) = self.commentary {
            writeln!(f, "\"\"\"")?;
            writeln!(f, "{}", commentary)?;
            writeln!(f, "\"\"\"")?;
        }

        let named_types: Vec<_> = self
            .types
            .iter()
            .filter(|t| !matches!(t, TypeDef::Inline { .. }))
            .collect();
        if !named_types.is_empty() {
            writeln!(f)?;
            for (index, type_def) in named_types.iter().enumerate() {
                if index > 0 {
                    writeln!(f)?;
                }
                write!(f, "{}", type_def)?;
                writeln!(f)?;
            }
        }

        if !self.facts.is_empty() {
            writeln!(f)?;
            for fact in &self.facts {
                write!(f, "{}", fact)?;
            }
        }

        if !self.rules.is_empty() {
            writeln!(f)?;
            for (index, rule) in self.rules.iter().enumerate() {
                if index > 0 {
                    writeln!(f)?;
                }
                write!(f, "{}", rule)?;
            }
        }

        if !self.meta_fields.is_empty() {
            writeln!(f)?;
            for meta in &self.meta_fields {
                writeln!(f, "{}", meta)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for LemmaFact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fact {}: {}", self.reference, self.value)
    }
}

impl fmt::Display for LemmaRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rule {}: {}", self.name, self.expression)?;
        for unless_clause in &self.unless_clauses {
            write!(
                f,
                "\n  unless {} then {}",
                unless_clause.condition, unless_clause.result
            )?;
        }
        writeln!(f)?;
        Ok(())
    }
}

/// Precedence level for an expression kind.
///
/// Higher values bind tighter. Used by `Expression::Display` and the formatter
/// to insert parentheses only where needed.
pub fn expression_precedence(kind: &ExpressionKind) -> u8 {
    match kind {
        ExpressionKind::LogicalAnd(..) => 2,
        ExpressionKind::LogicalNegation(..) => 3,
        ExpressionKind::Comparison(..) => 4,
        ExpressionKind::UnitConversion(..) => 4,
        ExpressionKind::Arithmetic(_, op, _) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => 5,
            ArithmeticComputation::Multiply
            | ArithmeticComputation::Divide
            | ArithmeticComputation::Modulo => 6,
            ArithmeticComputation::Power => 7,
        },
        ExpressionKind::MathematicalComputation(..) => 8,
        ExpressionKind::DateRelative(..) | ExpressionKind::DateCalendar(..) => 4,
        ExpressionKind::Literal(..)
        | ExpressionKind::Reference(..)
        | ExpressionKind::UnresolvedUnitLiteral(..)
        | ExpressionKind::Now
        | ExpressionKind::Veto(..) => 10,
    }
}

fn write_expression_child(
    f: &mut fmt::Formatter<'_>,
    child: &Expression,
    parent_prec: u8,
) -> fmt::Result {
    let child_prec = expression_precedence(&child.kind);
    if child_prec < parent_prec {
        write!(f, "({})", child)
    } else {
        write!(f, "{}", child)
    }
}

impl fmt::Display for Expression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ExpressionKind::Literal(lit) => write!(f, "{}", lit),
            ExpressionKind::Reference(r) => write!(f, "{}", r),
            ExpressionKind::Arithmetic(left, op, right) => {
                let my_prec = expression_precedence(&self.kind);
                write_expression_child(f, left, my_prec)?;
                write!(f, " {} ", op)?;
                write_expression_child(f, right, my_prec)
            }
            ExpressionKind::Comparison(left, op, right) => {
                let my_prec = expression_precedence(&self.kind);
                write_expression_child(f, left, my_prec)?;
                write!(f, " {} ", op)?;
                write_expression_child(f, right, my_prec)
            }
            ExpressionKind::UnitConversion(value, target) => {
                let my_prec = expression_precedence(&self.kind);
                write_expression_child(f, value, my_prec)?;
                write!(f, " in {}", target)
            }
            ExpressionKind::LogicalNegation(expr, _) => {
                let my_prec = expression_precedence(&self.kind);
                write!(f, "not ")?;
                write_expression_child(f, expr, my_prec)
            }
            ExpressionKind::LogicalAnd(left, right) => {
                let my_prec = expression_precedence(&self.kind);
                write_expression_child(f, left, my_prec)?;
                write!(f, " and ")?;
                write_expression_child(f, right, my_prec)
            }
            ExpressionKind::MathematicalComputation(op, operand) => {
                let my_prec = expression_precedence(&self.kind);
                write!(f, "{} ", op)?;
                write_expression_child(f, operand, my_prec)
            }
            ExpressionKind::Veto(veto) => match &veto.message {
                Some(msg) => write!(f, "veto {}", quote_lemma_text(msg)),
                None => write!(f, "veto"),
            },
            ExpressionKind::UnresolvedUnitLiteral(number, unit_name) => {
                write!(f, "{} {}", format_decimal_source(number), unit_name)
            }
            ExpressionKind::Now => write!(f, "now"),
            ExpressionKind::DateRelative(kind, date_expr, tolerance) => {
                write!(f, "{} {}", date_expr, kind)?;
                if let Some(tol) = tolerance {
                    write!(f, " {}", tol)?;
                }
                Ok(())
            }
            ExpressionKind::DateCalendar(kind, unit, date_expr) => {
                write!(f, "{} {} {}", date_expr, kind, unit)
            }
        }
    }
}

impl fmt::Display for ConversionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConversionTarget::Duration(unit) => write!(f, "{}", unit),
            ConversionTarget::Unit(unit) => write!(f, "{}", unit),
        }
    }
}

impl fmt::Display for ArithmeticComputation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArithmeticComputation::Add => write!(f, "+"),
            ArithmeticComputation::Subtract => write!(f, "-"),
            ArithmeticComputation::Multiply => write!(f, "*"),
            ArithmeticComputation::Divide => write!(f, "/"),
            ArithmeticComputation::Modulo => write!(f, "%"),
            ArithmeticComputation::Power => write!(f, "^"),
        }
    }
}

impl fmt::Display for ComparisonComputation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComparisonComputation::GreaterThan => write!(f, ">"),
            ComparisonComputation::LessThan => write!(f, "<"),
            ComparisonComputation::GreaterThanOrEqual => write!(f, ">="),
            ComparisonComputation::LessThanOrEqual => write!(f, "<="),
            ComparisonComputation::Equal => write!(f, "=="),
            ComparisonComputation::NotEqual => write!(f, "!="),
            ComparisonComputation::Is => write!(f, "is"),
            ComparisonComputation::IsNot => write!(f, "is not"),
        }
    }
}

impl fmt::Display for MathematicalComputation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MathematicalComputation::Sqrt => write!(f, "sqrt"),
            MathematicalComputation::Sin => write!(f, "sin"),
            MathematicalComputation::Cos => write!(f, "cos"),
            MathematicalComputation::Tan => write!(f, "tan"),
            MathematicalComputation::Asin => write!(f, "asin"),
            MathematicalComputation::Acos => write!(f, "acos"),
            MathematicalComputation::Atan => write!(f, "atan"),
            MathematicalComputation::Log => write!(f, "log"),
            MathematicalComputation::Exp => write!(f, "exp"),
            MathematicalComputation::Abs => write!(f, "abs"),
            MathematicalComputation::Floor => write!(f, "floor"),
            MathematicalComputation::Ceil => write!(f, "ceil"),
            MathematicalComputation::Round => write!(f, "round"),
        }
    }
}

impl fmt::Display for TimeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

impl fmt::Display for TimezoneValue {
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

impl fmt::Display for DateTimeValue {
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

//

/// Type definition (named, import, or inline).
/// Applying constraints to produce TypeSpecification is done in planning (semantics).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeDef {
    Regular {
        source_location: Source,
        name: String,
        parent: String,
        constraints: Option<Vec<Constraint>>,
    },
    Import {
        source_location: Source,
        name: String,
        source_type: String,
        from: SpecRef,
        constraints: Option<Vec<Constraint>>,
    },
    Inline {
        source_location: Source,
        parent: String,
        constraints: Option<Vec<Constraint>>,
        fact_ref: Reference,
        from: Option<SpecRef>,
    },
}

impl TypeDef {
    pub fn source_location(&self) -> &Source {
        match self {
            TypeDef::Regular {
                source_location, ..
            }
            | TypeDef::Import {
                source_location, ..
            }
            | TypeDef::Inline {
                source_location, ..
            } => source_location,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            TypeDef::Regular { name, .. } | TypeDef::Import { name, .. } => name,
            TypeDef::Inline { parent, .. } => parent,
        }
    }
}

impl fmt::Display for TypeDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeDef::Regular {
                name,
                parent,
                constraints,
                ..
            } => {
                write!(f, "type {}: {}", name, parent)?;
                if let Some(constraints) = constraints {
                    for (cmd, args) in constraints {
                        write!(f, "\n  -> {}", cmd)?;
                        for arg in args {
                            write!(f, " {}", arg.value())?;
                        }
                    }
                }
                Ok(())
            }
            TypeDef::Import {
                name,
                from,
                constraints,
                ..
            } => {
                write!(f, "type {} from {}", name, from)?;
                if let Some(constraints) = constraints {
                    for (cmd, args) in constraints {
                        write!(f, " -> {}", cmd)?;
                        for arg in args {
                            write!(f, " {}", arg.value())?;
                        }
                    }
                }
                Ok(())
            }
            TypeDef::Inline { .. } => Ok(()),
        }
    }
}

// =============================================================================
// AsLemmaSource — wrapper for valid, round-trippable Lemma source output
// =============================================================================

/// Wrapper that selects the "emit valid Lemma source" `Display` implementation.
///
/// The regular `Display` on AST types is for human-readable output. Wrap a
/// reference in `AsLemmaSource` when you need syntactically valid Lemma that
/// can be parsed back (round-trip).
///
/// # Example
/// ```ignore
/// let s = format!("{}", AsLemmaSource(&fact_value));
/// ```
pub struct AsLemmaSource<'a, T: ?Sized>(pub &'a T);

/// Escape a string and wrap it in double quotes for Lemma source output.
/// Handles `\` and `"` escaping.
pub fn quote_lemma_text(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

/// Format a Decimal for Lemma source: normalize, remove trailing zeros,
/// strip the fractional part when it is zero (e.g. `100.00` → `"100"`),
/// and insert underscore separators in the integer part when it has 4+
/// digits (e.g. `30000000.50` → `"30_000_000.50"`).
fn format_decimal_source(n: &Decimal) -> String {
    let norm = n.normalize();
    let raw = if norm.fract().is_zero() {
        norm.trunc().to_string()
    } else {
        norm.to_string()
    };
    group_digits(&raw)
}

/// Insert `_` every 3 digits in the integer part of a numeric string.
/// Handles optional leading `-`/`+` sign and optional fractional part.
/// Only groups when the integer part has 4 or more digits.
fn group_digits(s: &str) -> String {
    let (sign, rest) = if s.starts_with('-') || s.starts_with('+') {
        (&s[..1], &s[1..])
    } else {
        ("", s)
    };

    let (int_part, frac_part) = match rest.find('.') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, ""),
    };

    if int_part.len() < 4 {
        return s.to_string();
    }

    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (i, ch) in int_part.chars().enumerate() {
        let digits_remaining = int_part.len() - i;
        if i > 0 && digits_remaining % 3 == 0 {
            grouped.push('_');
        }
        grouped.push(ch);
    }

    format!("{}{}{}", sign, grouped, frac_part)
}

// -- Display for AsLemmaSource<CommandArg> ------------------------------------

impl<'a> fmt::Display for AsLemmaSource<'a, CommandArg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            CommandArg::Text(s) => write!(f, "{}", quote_lemma_text(s)),
            CommandArg::Number(s) => {
                let clean: String = s.chars().filter(|c| *c != '_' && *c != ',').collect();
                write!(f, "{}", group_digits(&clean))
            }
            CommandArg::Boolean(s) | CommandArg::Label(s) => {
                write!(f, "{}", s)
            }
        }
    }
}

/// Format a single constraint command and its args as valid Lemma source.
///
/// Each `CommandArg` already knows its literal kind (from parsing), so formatting
/// is simply delegated to `AsLemmaSource<CommandArg>` — no lookup table needed.
fn format_constraint_as_source(cmd: &str, args: &[CommandArg]) -> String {
    if args.is_empty() {
        cmd.to_string()
    } else {
        let args_str: Vec<String> = args
            .iter()
            .map(|a| format!("{}", AsLemmaSource(a)))
            .collect();
        format!("{} {}", cmd, args_str.join(" "))
    }
}

/// Format a constraint list as valid Lemma source.
/// Returns the `cmd arg -> cmd arg` portion joined by `separator`.
fn format_constraints_as_source(
    constraints: &[(String, Vec<CommandArg>)],
    separator: &str,
) -> String {
    constraints
        .iter()
        .map(|(cmd, args)| format_constraint_as_source(cmd, args))
        .collect::<Vec<_>>()
        .join(separator)
}

// -- Display for AsLemmaSource<FactValue> ------------------------------------

impl<'a> fmt::Display for AsLemmaSource<'a, FactValue> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            FactValue::Literal(v) => write!(f, "{}", AsLemmaSource(v)),
            FactValue::SpecReference(spec_ref) => {
                write!(f, "spec {}", spec_ref)
            }
            FactValue::TypeDeclaration {
                base,
                constraints,
                from,
            } => {
                let base_str = if let Some(from_spec) = from {
                    format!("{} from {}", base, from_spec)
                } else {
                    base.clone()
                };
                if let Some(ref constraints_vec) = constraints {
                    let constraint_str = format_constraints_as_source(constraints_vec, " -> ");
                    write!(f, "[{} -> {}]", base_str, constraint_str)
                } else {
                    write!(f, "[{}]", base_str)
                }
            }
        }
    }
}

// -- Display for AsLemmaSource<Value> ----------------------------------------

impl<'a> fmt::Display for AsLemmaSource<'a, Value> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Value::Number(n) => write!(f, "{}", format_decimal_source(n)),
            Value::Text(s) => write!(f, "{}", quote_lemma_text(s)),
            Value::Date(dt) => {
                let is_date_only =
                    dt.hour == 0 && dt.minute == 0 && dt.second == 0 && dt.timezone.is_none();
                if is_date_only {
                    write!(f, "{:04}-{:02}-{:02}", dt.year, dt.month, dt.day)
                } else {
                    write!(
                        f,
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
                    )?;
                    if let Some(tz) = &dt.timezone {
                        write!(f, "{}", tz)?;
                    }
                    Ok(())
                }
            }
            Value::Time(t) => {
                write!(f, "{:02}:{:02}:{:02}", t.hour, t.minute, t.second)?;
                if let Some(tz) = &t.timezone {
                    write!(f, "{}", tz)?;
                }
                Ok(())
            }
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Scale(n, u) => write!(f, "{} {}", format_decimal_source(n), u),
            Value::Duration(n, u) => write!(f, "{} {}", format_decimal_source(n), u),
            Value::Ratio(n, unit) => match unit.as_deref() {
                Some("percent") => {
                    let display_value = *n * Decimal::from(100);
                    write!(f, "{}%", format_decimal_source(&display_value))
                }
                Some("permille") => {
                    let display_value = *n * Decimal::from(1000);
                    write!(f, "{}%%", format_decimal_source(&display_value))
                }
                Some(unit_name) => write!(f, "{} {}", format_decimal_source(n), unit_name),
                None => write!(f, "{}", format_decimal_source(n)),
            },
        }
    }
}

// -- Display for AsLemmaSource<MetaValue> ------------------------------------

impl<'a> fmt::Display for AsLemmaSource<'a, MetaValue> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            MetaValue::Literal(v) => write!(f, "{}", AsLemmaSource(v)),
            MetaValue::Unquoted(s) => write!(f, "{}", s),
        }
    }
}

// -- Display for AsLemmaSource<TypeDef> --------------------------------------

impl<'a> fmt::Display for AsLemmaSource<'a, TypeDef> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            TypeDef::Regular {
                name,
                parent,
                constraints,
                ..
            } => {
                write!(f, "type {}: {}", name, parent)?;
                if let Some(constraints) = constraints {
                    for (cmd, args) in constraints {
                        write!(f, "\n  -> {}", format_constraint_as_source(cmd, args))?;
                    }
                }
                Ok(())
            }
            TypeDef::Import {
                name,
                from,
                constraints,
                ..
            } => {
                write!(f, "type {} from {}", name, from)?;
                if let Some(constraints) = constraints {
                    for (cmd, args) in constraints {
                        write!(f, " -> {}", format_constraint_as_source(cmd, args))?;
                    }
                }
                Ok(())
            }
            TypeDef::Inline { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_unit_display() {
        assert_eq!(format!("{}", DurationUnit::Second), "seconds");
        assert_eq!(format!("{}", DurationUnit::Minute), "minutes");
        assert_eq!(format!("{}", DurationUnit::Hour), "hours");
        assert_eq!(format!("{}", DurationUnit::Day), "days");
        assert_eq!(format!("{}", DurationUnit::Week), "weeks");
        assert_eq!(format!("{}", DurationUnit::Millisecond), "milliseconds");
        assert_eq!(format!("{}", DurationUnit::Microsecond), "microseconds");
    }

    #[test]
    fn test_conversion_target_display() {
        assert_eq!(
            format!("{}", ConversionTarget::Duration(DurationUnit::Hour)),
            "hours"
        );
        assert_eq!(
            format!("{}", ConversionTarget::Unit("usd".to_string())),
            "usd"
        );
    }

    #[test]
    fn test_value_ratio_display() {
        use rust_decimal::Decimal;
        use std::str::FromStr;
        let percent = Value::Ratio(
            Decimal::from_str("0.10").unwrap(),
            Some("percent".to_string()),
        );
        assert_eq!(format!("{}", percent), "10%");
        let permille = Value::Ratio(
            Decimal::from_str("0.005").unwrap(),
            Some("permille".to_string()),
        );
        assert_eq!(format!("{}", permille), "5%%");
    }

    #[test]
    fn test_datetime_value_display() {
        let dt = DateTimeValue {
            year: 2024,
            month: 12,
            day: 25,
            hour: 14,
            minute: 30,
            second: 45,
            microsecond: 0,
            timezone: Some(TimezoneValue {
                offset_hours: 1,
                offset_minutes: 0,
            }),
        };
        assert_eq!(format!("{}", dt), "2024-12-25T14:30:45+01:00");
    }

    #[test]
    fn test_datetime_value_display_date_only() {
        let dt = DateTimeValue {
            year: 2026,
            month: 3,
            day: 4,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        };
        assert_eq!(format!("{}", dt), "2026-03-04");
    }

    #[test]
    fn test_datetime_value_display_microseconds() {
        let dt = DateTimeValue {
            year: 2026,
            month: 2,
            day: 23,
            hour: 14,
            minute: 30,
            second: 45,
            microsecond: 123456,
            timezone: Some(TimezoneValue {
                offset_hours: 0,
                offset_minutes: 0,
            }),
        };
        assert_eq!(format!("{}", dt), "2026-02-23T14:30:45.123456Z");
    }

    #[test]
    fn test_datetime_microsecond_in_ordering() {
        let a = DateTimeValue {
            year: 2026,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 100,
            timezone: None,
        };
        let b = DateTimeValue {
            year: 2026,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 200,
            timezone: None,
        };
        assert!(a < b);
    }

    #[test]
    fn test_datetime_parse_iso_week() {
        let dt = DateTimeValue::parse("2026-W01").unwrap();
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 12);
        assert_eq!(dt.day, 29);
        assert_eq!(dt.microsecond, 0);
    }

    #[test]
    fn test_time_value_display() {
        let time = TimeValue {
            hour: 14,
            minute: 30,
            second: 45,
            timezone: Some(TimezoneValue {
                offset_hours: -5,
                offset_minutes: 30,
            }),
        };
        let display = format!("{}", time);
        assert!(display.contains("14"));
        assert!(display.contains("30"));
        assert!(display.contains("45"));
    }

    #[test]
    fn test_timezone_value() {
        let tz_positive = TimezoneValue {
            offset_hours: 5,
            offset_minutes: 30,
        };
        assert_eq!(tz_positive.offset_hours, 5);
        assert_eq!(tz_positive.offset_minutes, 30);

        let tz_negative = TimezoneValue {
            offset_hours: -8,
            offset_minutes: 0,
        };
        assert_eq!(tz_negative.offset_hours, -8);
    }

    #[test]
    fn test_negation_types() {
        let json = serde_json::to_string(&NegationType::Not).expect("serialize NegationType");
        let decoded: NegationType = serde_json::from_str(&json).expect("deserialize NegationType");
        assert_eq!(decoded, NegationType::Not);
    }

    #[test]
    fn test_veto_expression() {
        let veto_with_message = VetoExpression {
            message: Some("Must be over 18".to_string()),
        };
        assert_eq!(
            veto_with_message.message,
            Some("Must be over 18".to_string())
        );

        let veto_without_message = VetoExpression { message: None };
        assert!(veto_without_message.message.is_none());
    }

    // test_expression_get_source_text_with_location (uses Value instead of LiteralValue now)
    // test_expression_get_source_text_no_location (uses Value instead of LiteralValue now)
    // test_expression_get_source_text_source_not_found (uses Value instead of LiteralValue now)

    // =====================================================================
    // AsLemmaSource — constraint formatting tests
    // =====================================================================

    #[test]
    fn as_lemma_source_text_default_is_quoted() {
        let fv = FactValue::TypeDeclaration {
            base: "text".to_string(),
            constraints: Some(vec![(
                "default".to_string(),
                vec![CommandArg::Text("single".to_string())],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[text -> default \"single\"]"
        );
    }

    #[test]
    fn as_lemma_source_number_default_not_quoted() {
        let fv = FactValue::TypeDeclaration {
            base: "number".to_string(),
            constraints: Some(vec![(
                "default".to_string(),
                vec![CommandArg::Number("10".to_string())],
            )]),
            from: None,
        };
        assert_eq!(format!("{}", AsLemmaSource(&fv)), "[number -> default 10]");
    }

    #[test]
    fn as_lemma_source_help_always_quoted() {
        let fv = FactValue::TypeDeclaration {
            base: "number".to_string(),
            constraints: Some(vec![(
                "help".to_string(),
                vec![CommandArg::Text("Enter a quantity".to_string())],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[number -> help \"Enter a quantity\"]"
        );
    }

    #[test]
    fn as_lemma_source_text_option_quoted() {
        let fv = FactValue::TypeDeclaration {
            base: "text".to_string(),
            constraints: Some(vec![
                (
                    "option".to_string(),
                    vec![CommandArg::Text("active".to_string())],
                ),
                (
                    "option".to_string(),
                    vec![CommandArg::Text("inactive".to_string())],
                ),
            ]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[text -> option \"active\" -> option \"inactive\"]"
        );
    }

    #[test]
    fn as_lemma_source_scale_unit_not_quoted() {
        let fv = FactValue::TypeDeclaration {
            base: "scale".to_string(),
            constraints: Some(vec![
                (
                    "unit".to_string(),
                    vec![
                        CommandArg::Label("eur".to_string()),
                        CommandArg::Number("1.00".to_string()),
                    ],
                ),
                (
                    "unit".to_string(),
                    vec![
                        CommandArg::Label("usd".to_string()),
                        CommandArg::Number("1.10".to_string()),
                    ],
                ),
            ]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[scale -> unit eur 1.00 -> unit usd 1.10]"
        );
    }

    #[test]
    fn as_lemma_source_scale_minimum_with_unit() {
        let fv = FactValue::TypeDeclaration {
            base: "scale".to_string(),
            constraints: Some(vec![(
                "minimum".to_string(),
                vec![
                    CommandArg::Number("0".to_string()),
                    CommandArg::Label("eur".to_string()),
                ],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[scale -> minimum 0 eur]"
        );
    }

    #[test]
    fn as_lemma_source_boolean_default() {
        let fv = FactValue::TypeDeclaration {
            base: "boolean".to_string(),
            constraints: Some(vec![(
                "default".to_string(),
                vec![CommandArg::Boolean("true".to_string())],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[boolean -> default true]"
        );
    }

    #[test]
    fn as_lemma_source_duration_default() {
        let fv = FactValue::TypeDeclaration {
            base: "duration".to_string(),
            constraints: Some(vec![(
                "default".to_string(),
                vec![
                    CommandArg::Number("40".to_string()),
                    CommandArg::Label("hours".to_string()),
                ],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[duration -> default 40 hours]"
        );
    }

    #[test]
    fn as_lemma_source_named_type_default_quoted() {
        // Named types (user-defined): the parser produces CommandArg::Text for
        // quoted default values like `default "single"`.
        let fv = FactValue::TypeDeclaration {
            base: "filing_status_type".to_string(),
            constraints: Some(vec![(
                "default".to_string(),
                vec![CommandArg::Text("single".to_string())],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[filing_status_type -> default \"single\"]"
        );
    }

    #[test]
    fn as_lemma_source_help_escapes_quotes() {
        let fv = FactValue::TypeDeclaration {
            base: "text".to_string(),
            constraints: Some(vec![(
                "help".to_string(),
                vec![CommandArg::Text("say \"hello\"".to_string())],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "[text -> help \"say \\\"hello\\\"\"]"
        );
    }

    #[test]
    fn as_lemma_source_typedef_regular_options_quoted() {
        let td = TypeDef::Regular {
            source_location: Source::new(
                "test",
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                std::sync::Arc::from("spec test\nfact x: 1"),
            ),
            name: "status".to_string(),
            parent: "text".to_string(),
            constraints: Some(vec![
                (
                    "option".to_string(),
                    vec![CommandArg::Text("active".to_string())],
                ),
                (
                    "option".to_string(),
                    vec![CommandArg::Text("inactive".to_string())],
                ),
            ]),
        };
        let output = format!("{}", AsLemmaSource(&td));
        assert!(output.contains("option \"active\""), "got: {}", output);
        assert!(output.contains("option \"inactive\""), "got: {}", output);
    }

    #[test]
    fn as_lemma_source_typedef_scale_units_not_quoted() {
        let td = TypeDef::Regular {
            source_location: Source::new(
                "test",
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                std::sync::Arc::from("spec test\nfact x: 1"),
            ),
            name: "money".to_string(),
            parent: "scale".to_string(),
            constraints: Some(vec![
                (
                    "unit".to_string(),
                    vec![
                        CommandArg::Label("eur".to_string()),
                        CommandArg::Number("1.00".to_string()),
                    ],
                ),
                (
                    "decimals".to_string(),
                    vec![CommandArg::Number("2".to_string())],
                ),
                (
                    "minimum".to_string(),
                    vec![CommandArg::Number("0".to_string())],
                ),
            ]),
        };
        let output = format!("{}", AsLemmaSource(&td));
        assert!(output.contains("unit eur 1.00"), "got: {}", output);
        assert!(output.contains("decimals 2"), "got: {}", output);
        assert!(output.contains("minimum 0"), "got: {}", output);
    }
}
