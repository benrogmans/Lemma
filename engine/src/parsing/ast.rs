//! AST types
//!
//! Infrastructure (Span, DepthTracker) and spec/data/rule/expression/value types from parsing.
//!
//! # Human `Display` vs canonical `AsLemmaSource`
//!
//! [`MetaValue`], [`DataValue`], and [`CommandArg`] use human-oriented
//! `Display` (stable for `to_string()`, logs, APIs). [`Expression`] and
//! [`LemmaRule`]/[`LemmaSpec`] use canonical Lemma source for literals via
//! [`AsLemmaSource`] around [`Value`]. Wrap [`MetaValue`]/[`DataValue`]
//! in [`AsLemmaSource`] when emitting round-trippable source (e.g. the formatter).

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
// Spec, data, rule, expression and value types
// -----------------------------------------------------------------------------

use crate::parsing::source::Source;
use rust_decimal::Decimal;
use serde::Serialize;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub use crate::literals::{
    BooleanValue, DateTimeValue, DurationUnit, TimeValue, TimezoneValue, Value,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EffectiveDate {
    Origin,
    DateTimeValue(crate::DateTimeValue),
}

impl EffectiveDate {
    pub fn as_ref(&self) -> Option<&crate::DateTimeValue> {
        match self {
            EffectiveDate::Origin => None,
            EffectiveDate::DateTimeValue(dt) => Some(dt),
        }
    }

    pub fn from_option(opt: Option<crate::DateTimeValue>) -> Self {
        match opt {
            None => EffectiveDate::Origin,
            Some(dt) => EffectiveDate::DateTimeValue(dt),
        }
    }

    pub fn to_option(&self) -> Option<crate::DateTimeValue> {
        match self {
            EffectiveDate::Origin => None,
            EffectiveDate::DateTimeValue(dt) => Some(dt.clone()),
        }
    }

    pub fn is_origin(&self) -> bool {
        matches!(self, EffectiveDate::Origin)
    }
}

impl PartialOrd for EffectiveDate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EffectiveDate {
    // As ref returns None for Origin, so Origin < DateTimeValue(_).
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_ref().cmp(&other.as_ref())
    }
}

impl fmt::Display for EffectiveDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EffectiveDate::Origin => Ok(()),
            EffectiveDate::DateTimeValue(dt) => write!(f, "{}", dt),
        }
    }
}

/// A Lemma spec containing data and rules.
/// Ordered and compared by (name, effective_from) for use in BTreeSet; Origin < DateTimeValue(_).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LemmaSpec {
    /// Base spec name. Includes `@` for registry specs.
    pub name: String,
    /// `true` when the spec was declared with the `@` qualifier (registry spec).
    pub from_registry: bool,
    pub effective_from: EffectiveDate,
    pub attribute: Option<String>,
    pub start_line: usize,
    pub commentary: Option<String>,
    pub data: Vec<LemmaData>,
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
pub struct LemmaData {
    pub reference: Reference,
    pub value: DataValue,
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
    /// Unresolved reference (identifier or dot path). Resolved during planning to DataPath or RulePath.
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
/// Reference to a data or rule (identifier or dot path).
///
/// Used in expressions and in LemmaData. During planning, references
/// are resolved to DataPath or RulePath (semantics layer).
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

/// Comparison computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonComputation {
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Is,
    IsNot,
}

impl ComparisonComputation {
    /// Check if this is an equality comparison (`is`)
    #[must_use]
    pub fn is_equal(&self) -> bool {
        matches!(self, ComparisonComputation::Is)
    }

    /// Check if this is an inequality comparison (`is not`)
    #[must_use]
    pub fn is_not_equal(&self) -> bool {
        matches!(self, ComparisonComputation::IsNot)
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
#[serde(rename_all = "snake_case")]
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

/// A reference to a spec, with optional effective datetime.
/// For registry references the `name` includes the leading `@` (e.g. `@org/repo/spec`);
/// for local references it is a plain base name. `from_registry` mirrors whether
/// the source used the `@` qualifier.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpecRef {
    /// Spec name as written in source. Includes `@` for registry references.
    pub name: String,
    /// `true` when the source used the `@` qualifier (registry reference).
    pub from_registry: bool,
    /// Optional effective datetime for temporal resolution.
    pub effective: Option<DateTimeValue>,
}

impl std::fmt::Display for SpecRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some(d) = &self.effective {
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
            effective: None,
        }
    }

    /// Create a registry spec reference.
    pub fn registry(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            from_registry: true,
            effective: None,
        }
    }

    pub fn resolution_key(&self) -> String {
        self.name.clone()
    }

    /// Resolve the effective instant for this reference given the planning slice's `effective`.
    /// Explicit qualifier on the reference wins; otherwise inherits the slice instant.
    pub fn at(&self, effective: &EffectiveDate) -> EffectiveDate {
        self.effective
            .clone()
            .map_or_else(|| effective.clone(), EffectiveDate::DateTimeValue)
    }
}

/// A parsed constraint command argument, preserving the literal kind from the
/// grammar rule `command_arg: { number_literal | boolean_literal | text_literal | label }`.
///
/// Two grammatical kinds appear after a constraint command:
/// - **Literal** — a fully-typed value carrying the literal kind the parser
///   recognised (`Number`, `Ratio`, `Scale`, `Duration`, `Date`, `Time`,
///   `Boolean`, `Text`). Stored as the canonical [`crate::literals::Value`]
///   so downstream consumers match on the variant rather than re-parsing strings.
/// - **Label** — a bare identifier used as a name (e.g. the unit name `eur`
///   in `unit eur 1.00`, or a primitive type keyword used as an option label).
///
/// Planning validates each command's args against the variant kinds it accepts
/// and rejects mismatches without coercion (a `Text` literal is never a `Number`,
/// a `Ratio` literal is never a bare `Number`, etc.).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CommandArg {
    /// A typed literal value parsed by [`crate::parsing::parser::Parser::parse_literal_value`].
    Literal(crate::literals::Value),
    /// An identifier used as a name (unit name, option keyword, etc.).
    Label(String),
}

impl fmt::Display for CommandArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandArg::Literal(v) => write!(f, "{}", v),
            CommandArg::Label(s) => write!(f, "{}", s),
        }
    }
}

/// Constraint command for type definitions. Derived from lexer tokens; no string matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeConstraintCommand {
    Help,
    Default,
    Unit,
    Minimum,
    Maximum,
    Decimals,
    Precision,
    Option,
    Options,
    Length,
}

impl fmt::Display for TypeConstraintCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TypeConstraintCommand::Help => "help",
            TypeConstraintCommand::Default => "default",
            TypeConstraintCommand::Unit => "unit",
            TypeConstraintCommand::Minimum => "minimum",
            TypeConstraintCommand::Maximum => "maximum",
            TypeConstraintCommand::Decimals => "decimals",
            TypeConstraintCommand::Precision => "precision",
            TypeConstraintCommand::Option => "option",
            TypeConstraintCommand::Options => "options",
            TypeConstraintCommand::Length => "length",
        };
        write!(f, "{}", s)
    }
}

/// Parses a constraint command name. Returns None for unknown (parser returns error).
#[must_use]
pub fn try_parse_type_constraint_command(s: &str) -> Option<TypeConstraintCommand> {
    match s.trim().to_lowercase().as_str() {
        "help" => Some(TypeConstraintCommand::Help),
        "default" => Some(TypeConstraintCommand::Default),
        "unit" => Some(TypeConstraintCommand::Unit),
        "minimum" => Some(TypeConstraintCommand::Minimum),
        "maximum" => Some(TypeConstraintCommand::Maximum),
        "decimals" => Some(TypeConstraintCommand::Decimals),
        "precision" => Some(TypeConstraintCommand::Precision),
        "option" => Some(TypeConstraintCommand::Option),
        "options" => Some(TypeConstraintCommand::Options),
        "length" => Some(TypeConstraintCommand::Length),
        _ => None,
    }
}

/// A single constraint command and its typed arguments.
pub type Constraint = (TypeConstraintCommand, Vec<CommandArg>);

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
/// Parse-time data value (before type resolution)
pub enum DataValue {
    /// A literal value (parse-time; type will be resolved during planning)
    Literal(Value),
    /// A reference to another spec
    SpecReference(SpecRef),
    /// A type declaration: `data x: number -> minimum 5` or `data y: x -> minimum 5`
    TypeDeclaration {
        base: ParentType,
        constraints: Option<Vec<Constraint>>,
        from: Option<SpecRef>,
    },
    /// A value-copy reference to another data or rule, with optional additional constraints.
    ///
    /// Two surface forms produce this variant:
    /// 1. **Dotted RHS** in any position — e.g. `data license2: law.other` or
    ///    `data license2: law.other -> minimum 5`. A dotted RHS is never a
    ///    typedef name, so it unambiguously means "copy from this data or rule."
    /// 2. **Non-dotted RHS in a binding LHS** — e.g. `data license.other: src`.
    ///    When the LHS has segments (a binding path into another spec) the RHS
    ///    is read as a value-copy reference to a name in the enclosing spec,
    ///    not as a typedef.
    ///
    /// `data x: someident` (LHS without segments, RHS without dots) is the one
    /// case that stays a `TypeDeclaration` — `someident` is treated as a typedef
    /// name. See parser `parse_data_value` for the discriminator.
    ///
    /// The target is resolved during planning to either a `DataPath` or a `RulePath`.
    Reference {
        target: Reference,
        constraints: Option<Vec<Constraint>>,
    },
}

/// Render a chain of `-> command args ...` constraints for display purposes.
/// Shared between `DataValue::TypeDeclaration` and `DataValue::Reference`.
fn format_constraint_chain(constraints: &[Constraint]) -> String {
    constraints
        .iter()
        .map(|(cmd, args)| {
            let args_str: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let joined = args_str.join(" ");
            if joined.is_empty() {
                format!("{}", cmd)
            } else {
                format!("{} {}", cmd, joined)
            }
        })
        .collect::<Vec<_>>()
        .join(" -> ")
}

impl fmt::Display for DataValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataValue::Literal(v) => write!(f, "{}", v),
            DataValue::SpecReference(spec_ref) => {
                write!(f, "with {}", spec_ref)
            }
            DataValue::TypeDeclaration {
                base,
                constraints,
                from,
            } => {
                let base_str = if let Some(from_spec) = from {
                    format!("{} from {}", base, from_spec)
                } else {
                    format!("{}", base)
                };
                if let Some(ref constraints_vec) = constraints {
                    let constraint_str = format_constraint_chain(constraints_vec);
                    write!(f, "{} -> {}", base_str, constraint_str)
                } else {
                    write!(f, "{}", base_str)
                }
            }
            DataValue::Reference {
                target,
                constraints,
            } => {
                if let Some(ref constraints_vec) = constraints {
                    let constraint_str = format_constraint_chain(constraints_vec);
                    write!(f, "{} -> {}", target, constraint_str)
                } else {
                    write!(f, "{}", target)
                }
            }
        }
    }
}

impl LemmaData {
    #[must_use]
    pub fn new(reference: Reference, value: DataValue, source_location: Source) -> Self {
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
            effective_from: EffectiveDate::Origin,
            attribute: None,
            start_line: 1,
            commentary: None,
            data: Vec::new(),
            rules: Vec::new(),
            meta_fields: Vec::new(),
        }
    }

    /// Temporal range start. Origin (None) means −∞.
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
    pub fn add_data(mut self, data: LemmaData) -> Self {
        self.data.push(data);
        self
    }

    #[must_use]
    pub fn add_rule(mut self, rule: LemmaRule) -> Self {
        self.rules.push(rule);
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
        if let EffectiveDate::DateTimeValue(ref af) = self.effective_from {
            write!(f, " {}", af)?;
        }
        writeln!(f)?;

        if let Some(ref commentary) = self.commentary {
            writeln!(f, "\"\"\"")?;
            writeln!(f, "{}", commentary)?;
            writeln!(f, "\"\"\"")?;
        }

        if !self.data.is_empty() {
            writeln!(f)?;
            for data in &self.data {
                write!(f, "{}", data)?;
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

impl fmt::Display for LemmaData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "data {}: {}", self.reference, self.value)
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
            ExpressionKind::Literal(lit) => write!(f, "{}", AsLemmaSource(lit)),
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

// -----------------------------------------------------------------------------
// Primitive type kinds and parent type references
// -----------------------------------------------------------------------------

/// Built-in primitive type kind. Single source of truth for type keywords.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveKind {
    Boolean,
    Scale,
    Number,
    Percent,
    Ratio,
    Text,
    Date,
    Time,
    Duration,
}

impl std::fmt::Display for PrimitiveKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PrimitiveKind::Boolean => "boolean",
            PrimitiveKind::Scale => "scale",
            PrimitiveKind::Number => "number",
            PrimitiveKind::Percent => "percent",
            PrimitiveKind::Ratio => "ratio",
            PrimitiveKind::Text => "text",
            PrimitiveKind::Date => "date",
            PrimitiveKind::Time => "time",
            PrimitiveKind::Duration => "duration",
        };
        write!(f, "{}", s)
    }
}

/// Parent type in a type definition: built-in primitive or custom type name.
///
/// `name` is the declared type name (the data name that introduces this type).
/// For `data temperature: scale`, name = "temperature", primitive = Scale.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParentType {
    Primitive { primitive: PrimitiveKind },
    Custom { name: String },
}

impl std::fmt::Display for ParentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParentType::Primitive { primitive } => write!(f, "{}", primitive),
            ParentType::Custom { name } => write!(f, "{}", name),
        }
    }
}

// =============================================================================
// AsLemmaSource<Value> — canonical literal formatting
// =============================================================================

/// Wrap a value to emit canonical Lemma source (round-trippable). See module docs.
pub struct AsLemmaSource<'a, T: ?Sized>(pub &'a T);

/// Escape a string and wrap it in double quotes for Lemma source output.
/// Handles `\` and `"` escaping.
pub fn quote_lemma_text(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

/// Format a Decimal for Lemma source, preserving precision (trailing zeros).
/// Strips the fractional part only when it is zero (e.g. `100` stays `"100"`,
/// `1.00` stays `"1.00"`). Inserts underscore separators in the integer part
/// when it has 4+ digits (e.g. `30000000.50` → `"30_000_000.50"`).
fn format_decimal_source(n: &Decimal) -> String {
    let raw = if n.fract().is_zero() {
        n.trunc().to_string()
    } else {
        n.to_string()
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

impl<'a> fmt::Display for AsLemmaSource<'a, CommandArg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::literals::Value;
        match self.0 {
            CommandArg::Literal(Value::Text(s)) => write!(f, "{}", quote_lemma_text(s)),
            CommandArg::Literal(Value::Number(d)) => {
                write!(f, "{}", group_digits(&d.to_string()))
            }
            CommandArg::Literal(Value::Boolean(bv)) => write!(f, "{}", bv),
            CommandArg::Literal(Value::Scale(d, unit)) => {
                write!(f, "{} {}", group_digits(&d.to_string()), unit)
            }
            CommandArg::Literal(Value::Duration(d, unit)) => {
                write!(f, "{} {}", group_digits(&d.to_string()), unit)
            }
            CommandArg::Literal(value @ Value::Ratio(_, _)) => write!(f, "{}", value),
            CommandArg::Literal(Value::Date(dt)) => write!(f, "{}", dt),
            CommandArg::Literal(Value::Time(t)) => write!(f, "{}", t),
            CommandArg::Label(s) => write!(f, "{}", s),
        }
    }
}

/// Format a single constraint command and its args as valid Lemma source.
fn format_constraint_as_source(cmd: &TypeConstraintCommand, args: &[CommandArg]) -> String {
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
fn format_constraints_as_source(constraints: &[Constraint], separator: &str) -> String {
    constraints
        .iter()
        .map(|(cmd, args)| format_constraint_as_source(cmd, args))
        .collect::<Vec<_>>()
        .join(separator)
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

// -- AsLemmaSource: MetaValue, DataValue (formatter / round-trip) ---

impl<'a> fmt::Display for AsLemmaSource<'a, MetaValue> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            MetaValue::Literal(v) => write!(f, "{}", AsLemmaSource(v)),
            MetaValue::Unquoted(s) => write!(f, "{}", s),
        }
    }
}

impl<'a> fmt::Display for AsLemmaSource<'a, DataValue> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            DataValue::Literal(v) => write!(f, "{}", AsLemmaSource(v)),
            DataValue::SpecReference(spec_ref) => {
                write!(f, "with {}", spec_ref)
            }
            DataValue::TypeDeclaration {
                base,
                constraints,
                from,
            } => {
                let base_str = if let Some(from_spec) = from {
                    format!("{} from {}", base, from_spec)
                } else {
                    format!("{}", base)
                };
                if let Some(ref constraints_vec) = constraints {
                    let constraint_str = format_constraints_as_source(constraints_vec, " -> ");
                    write!(f, "{} -> {}", base_str, constraint_str)
                } else {
                    write!(f, "{}", base_str)
                }
            }
            DataValue::Reference {
                target,
                constraints,
            } => {
                if let Some(ref constraints_vec) = constraints {
                    let constraint_str = format_constraints_as_source(constraints_vec, " -> ");
                    write!(f, "{} -> {}", target, constraint_str)
                } else {
                    write!(f, "{}", target)
                }
            }
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
        let dt: DateTimeValue = "2026-W01".parse().unwrap();
        assert_eq!(dt.year, 2025);
        assert_eq!(dt.month, 12);
        assert_eq!(dt.day, 29);
        assert_eq!(dt.microsecond, 0);
    }

    #[test]
    fn test_negation_types() {
        let json = serde_json::to_string(&NegationType::Not).expect("serialize NegationType");
        let decoded: NegationType = serde_json::from_str(&json).expect("deserialize NegationType");
        assert_eq!(decoded, NegationType::Not);
    }

    #[test]
    fn parent_type_primitive_serde_internally_tagged() {
        let p = ParentType::Primitive {
            primitive: PrimitiveKind::Number,
        };
        let json = serde_json::to_string(&p).expect("ParentType::Primitive must serialize");
        assert!(json.contains("\"kind\"") && json.contains("\"primitive\""));
        let back: ParentType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, p);
    }

    // =====================================================================
    // DataValue Display — constraint formatting
    // =====================================================================

    fn text_arg(s: &str) -> CommandArg {
        CommandArg::Literal(crate::literals::Value::Text(s.to_string()))
    }

    fn number_arg(s: &str) -> CommandArg {
        let d: rust_decimal::Decimal = s.parse().expect("decimal");
        CommandArg::Literal(crate::literals::Value::Number(d))
    }

    fn boolean_arg(b: BooleanValue) -> CommandArg {
        CommandArg::Literal(crate::literals::Value::Boolean(b))
    }

    fn scale_arg(value: &str, unit: &str) -> CommandArg {
        let d: rust_decimal::Decimal = value.parse().expect("decimal");
        CommandArg::Literal(crate::literals::Value::Scale(d, unit.to_string()))
    }

    fn duration_arg(value: &str, unit: DurationUnit) -> CommandArg {
        let d: rust_decimal::Decimal = value.parse().expect("decimal");
        CommandArg::Literal(crate::literals::Value::Duration(d, unit))
    }

    #[test]
    fn as_lemma_source_text_default_is_quoted() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Text,
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Default,
                vec![text_arg("single")],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "text -> default \"single\""
        );
    }

    #[test]
    fn as_lemma_source_number_default_not_quoted() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Default,
                vec![number_arg("10")],
            )]),
            from: None,
        };
        assert_eq!(format!("{}", AsLemmaSource(&fv)), "number -> default 10");
    }

    #[test]
    fn as_lemma_source_help_always_quoted() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Help,
                vec![text_arg("Enter a quantity")],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "number -> help \"Enter a quantity\""
        );
    }

    #[test]
    fn as_lemma_source_text_option_quoted() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Text,
            },
            constraints: Some(vec![
                (TypeConstraintCommand::Option, vec![text_arg("active")]),
                (TypeConstraintCommand::Option, vec![text_arg("inactive")]),
            ]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "text -> option \"active\" -> option \"inactive\""
        );
    }

    #[test]
    fn as_lemma_source_scale_unit_not_quoted() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Scale,
            },
            constraints: Some(vec![
                (
                    TypeConstraintCommand::Unit,
                    vec![CommandArg::Label("eur".to_string()), number_arg("1.00")],
                ),
                (
                    TypeConstraintCommand::Unit,
                    vec![CommandArg::Label("usd".to_string()), number_arg("1.10")],
                ),
            ]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "scale -> unit eur 1.00 -> unit usd 1.10"
        );
    }

    #[test]
    fn as_lemma_source_scale_minimum_with_unit() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Scale,
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Minimum,
                vec![scale_arg("0", "eur")],
            )]),
            from: None,
        };
        assert_eq!(format!("{}", AsLemmaSource(&fv)), "scale -> minimum 0 eur");
    }

    #[test]
    fn as_lemma_source_boolean_default() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Boolean,
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Default,
                vec![boolean_arg(BooleanValue::True)],
            )]),
            from: None,
        };
        assert_eq!(format!("{}", AsLemmaSource(&fv)), "boolean -> default true");
    }

    #[test]
    fn as_lemma_source_duration_default() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Duration,
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Default,
                vec![duration_arg("40", DurationUnit::Hour)],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "duration -> default 40 hours"
        );
    }

    #[test]
    fn as_lemma_source_named_type_default_quoted() {
        // Named types (user-defined): the parser produces a typed Text literal for
        // quoted default values like `default "single"`.
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Custom {
                name: "filing_status_type".to_string(),
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Default,
                vec![text_arg("single")],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "filing_status_type -> default \"single\""
        );
    }

    #[test]
    fn as_lemma_source_help_escapes_quotes() {
        let fv = DataValue::TypeDeclaration {
            base: ParentType::Primitive {
                primitive: PrimitiveKind::Text,
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Help,
                vec![text_arg("say \"hello\"")],
            )]),
            from: None,
        };
        assert_eq!(
            format!("{}", AsLemmaSource(&fv)),
            "text -> help \"say \\\"hello\\\"\""
        );
    }
}
