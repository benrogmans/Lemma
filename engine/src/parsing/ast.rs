//! AST types
//!
//! Infrastructure (Span, DepthTracker) and document/fact/rule/expression/value types from parsing.
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

impl Span {
    pub fn from_pest_span(span: pest::Span) -> Self {
        let (line, col) = span.start_pos().line_col();
        Self {
            start: span.start(),
            end: span.end(),
            line,
            col,
        }
    }
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

    pub fn push_depth(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(format!(
                "Expression depth {} exceeds maximum of {}",
                self.depth, self.max_depth
            ));
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
            max_depth: 100,
        }
    }
}

// -----------------------------------------------------------------------------
// Document, fact, rule, expression and value types
// -----------------------------------------------------------------------------

use crate::parsing::source::Source;
use rust_decimal::Decimal;
use serde::Serialize;
use std::fmt;
use std::sync::Arc;

/// A Lemma document containing facts and rules
#[derive(Debug, Clone, PartialEq)]
pub struct LemmaDoc {
    pub name: String,
    pub attribute: Option<String>,
    pub start_line: usize,
    pub commentary: Option<String>,
    pub types: Vec<TypeDef>,
    pub facts: Vec<LemmaFact>,
    pub rules: Vec<LemmaRule>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LemmaFact {
    pub reference: FactReference,
    pub value: FactValue,
    pub source_location: Source,
}

/// An unless clause that provides an alternative result
///
/// Unless clauses are evaluated in order, and the last matching condition wins.
/// This matches natural language: "X unless A then Y, unless B then Z" - if both
/// A and B are true, Z is returned (the last match).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UnlessClause {
    pub condition: Expression,
    pub result: Expression,
    pub source_location: Source,
}

/// A rule with a single expression and optional unless clauses
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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

/// The kind/type of expression
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionKind {
    /// Parse-time literal value (type will be resolved during planning)
    Literal(Value),
    /// Fact reference (identifier or dot path); resolved to FactPath during planning
    FactReference(FactReference),
    /// Unresolved unit literal from parser (resolved during planning)
    /// Contains (number, unit_name) - the unit name will be resolved to its type during semantic analysis
    UnresolvedUnitLiteral(Decimal, String),
    RuleReference(RuleReference),
    LogicalAnd(Arc<Expression>, Arc<Expression>),
    LogicalOr(Arc<Expression>, Arc<Expression>),
    Arithmetic(Arc<Expression>, ArithmeticComputation, Arc<Expression>),
    Comparison(Arc<Expression>, ComparisonComputation, Arc<Expression>),
    UnitConversion(Arc<Expression>, ConversionTarget),
    LogicalNegation(Arc<Expression>, NegationType),
    MathematicalComputation(MathematicalComputation, Arc<Expression>),
    Veto(VetoExpression),
}

/// Unresolved reference from parser
///
/// Reference to a fact (identifier or dot path).
///
/// Used in expressions and in LemmaFact. During planning, fact references
/// are resolved to FactPath (semantics layer).
/// Examples:
/// - Local "age": segments=[], fact="age"
/// - Cross-document "employee.salary": segments=["employee"], fact="salary"
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FactReference {
    pub segments: Vec<String>,
    pub fact: String,
}

/// Reference to a rule
///
/// Rule references use a question mark suffix to distinguish them from fact references.
/// Examples:
/// - Local rule "has_license?": segments=[], rule="has_license"
/// - Cross-document "employee.is_eligible?": segments=["employee"], rule="is_eligible"
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RuleReference {
    pub segments: Vec<String>,
    pub rule: String,
}

impl RuleReference {
    /// Create from a full path (last element becomes rule)
    pub fn from_path(mut full_path: Vec<String>) -> Self {
        let rule = full_path.pop().unwrap_or_default();
        Self {
            segments: full_path,
            rule,
        }
    }

    /// Get all path segments including the rule name
    #[must_use]
    pub fn full_path(&self) -> Vec<String> {
        let mut path = self.segments.clone();
        path.push(self.rule.clone());
        path
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

/// A reference to a document, with a flag indicating whether the `@` registry
/// qualifier was present in the source.  The `name` field always contains the
/// plain document name (without `@`); `is_registry` is `true` when the author
/// wrote `@name`, signalling that the document should be fetched from a registry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DocRef {
    /// Plain document name (never contains `@`).
    pub name: String,
    /// `true` when the source used the `@` qualifier (registry reference).
    pub is_registry: bool,
}

impl std::fmt::Display for DocRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_registry {
            write!(f, "@{}", self.name)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

impl DocRef {
    /// Create a local (non-registry) document reference.
    pub fn local(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_registry: false,
        }
    }

    /// Create a registry document reference.
    pub fn registry(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_registry: true,
        }
    }

    /// Parse a raw reference string that may start with `@`.
    /// Strips the `@` and sets `is_registry` accordingly.
    pub fn parse(raw: &str) -> Self {
        match raw.strip_prefix('@') {
            Some(stripped) => Self::registry(stripped),
            None => Self::local(raw),
        }
    }
}

/// A parsed constraint command argument, preserving the literal kind from the
/// grammar rule `command_arg = { number_literal | boolean_literal | text_literal | label }`.
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
    /// A reference to another document
    DocumentReference(DocRef),
    /// A type declaration (inline type annotation on a fact)
    TypeDeclaration {
        base: String,
        constraints: Option<Vec<Constraint>>,
        from: Option<DocRef>,
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
            FactValue::DocumentReference(doc_ref) => {
                if doc_ref.is_registry {
                    write!(f, "doc @{}", doc_ref.name)
                } else {
                    write!(f, "doc {}", doc_ref.name)
                }
            }
            FactValue::TypeDeclaration {
                base,
                constraints,
                from,
            } => {
                let base_str = if let Some(from_doc) = from {
                    format!("{} from {}", base, from_doc)
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

/// A datetime value that preserves timezone information
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, serde::Deserialize)]
pub struct DateTimeValue {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub timezone: Option<TimezoneValue>,
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

impl FactReference {
    #[must_use]
    pub fn local(fact: String) -> Self {
        Self {
            segments: Vec::new(),
            fact,
        }
    }

    #[must_use]
    pub fn from_path(path: Vec<String>) -> Self {
        if path.is_empty() {
            Self {
                segments: Vec::new(),
                fact: String::new(),
            }
        } else {
            // Safe: path is non-empty.
            let fact = path[path.len() - 1].clone();
            let segments = path[..path.len() - 1].to_vec();
            Self { segments, fact }
        }
    }

    #[must_use]
    pub fn is_local(&self) -> bool {
        self.segments.is_empty()
    }

    #[must_use]
    pub fn full_path(&self) -> Vec<String> {
        let mut path = self.segments.clone();
        path.push(self.fact.clone());
        path
    }
}

impl LemmaFact {
    #[must_use]
    pub fn new(reference: FactReference, value: FactValue, source_location: Source) -> Self {
        Self {
            reference,
            value,
            source_location,
        }
    }
}

impl LemmaDoc {
    #[must_use]
    pub fn new(name: String) -> Self {
        Self {
            name,
            attribute: None,
            start_line: 1,
            commentary: None,
            types: Vec::new(),
            facts: Vec::new(),
            rules: Vec::new(),
        }
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
}

impl fmt::Display for LemmaDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "doc {}", self.name)?;
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

        Ok(())
    }
}

impl fmt::Display for FactReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            write!(f, "{}.", segment)?;
        }
        write!(f, "{}", self.fact)
    }
}

impl fmt::Display for LemmaFact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fact {} = {}", self.reference, self.value)
    }
}

impl fmt::Display for LemmaRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rule {} = {}", self.name, self.expression)?;
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
        ExpressionKind::LogicalOr(..) => 1,
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
        ExpressionKind::Literal(..)
        | ExpressionKind::FactReference(..)
        | ExpressionKind::RuleReference(..)
        | ExpressionKind::UnresolvedUnitLiteral(..)
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
            ExpressionKind::FactReference(r) => write!(f, "{}", r),
            ExpressionKind::RuleReference(rule_ref) => write!(f, "{}", rule_ref),
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
            ExpressionKind::LogicalOr(left, right) => {
                let my_prec = expression_precedence(&self.kind);
                write_expression_child(f, left, my_prec)?;
                write!(f, " or ")?;
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
        let is_date_only =
            self.hour == 0 && self.minute == 0 && self.second == 0 && self.timezone.is_none();
        if is_date_only {
            write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
        } else {
            write!(
                f,
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                self.year, self.month, self.day, self.hour, self.minute, self.second
            )?;
            if let Some(tz) = &self.timezone {
                write!(f, "{}", tz)?;
            }
            Ok(())
        }
    }
}

impl fmt::Display for RuleReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            write!(f, "{}?", self.rule)
        } else {
            write!(f, "{}.{}?", self.segments.join("."), self.rule)
        }
    }
}

/// Type definition (named, import, or inline).
/// Applying constraints to produce TypeSpecification is done in planning (semantics).
#[derive(Debug, Clone, PartialEq)]
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
        from: DocRef,
        constraints: Option<Vec<Constraint>>,
    },
    Inline {
        source_location: Source,
        parent: String,
        constraints: Option<Vec<Constraint>>,
        fact_ref: FactReference,
        from: Option<DocRef>,
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
                write!(f, "type {} = {}", name, parent)?;
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
/// and strip the fractional part when it is zero (e.g. `100.00` → `"100"`).
fn format_decimal_source(n: &Decimal) -> String {
    let norm = n.normalize();
    if norm.fract().is_zero() {
        norm.trunc().to_string()
    } else {
        norm.to_string()
    }
}

// -- Display for AsLemmaSource<CommandArg> ------------------------------------

impl<'a> fmt::Display for AsLemmaSource<'a, CommandArg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            CommandArg::Text(s) => write!(f, "{}", quote_lemma_text(s)),
            CommandArg::Number(s) | CommandArg::Boolean(s) | CommandArg::Label(s) => {
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
            FactValue::DocumentReference(doc_ref) => {
                if doc_ref.is_registry {
                    write!(f, "doc @{}", doc_ref.name)
                } else {
                    write!(f, "doc {}", doc_ref.name)
                }
            }
            FactValue::TypeDeclaration {
                base,
                constraints,
                from,
            } => {
                let base_str = if let Some(from_doc) = from {
                    format!("{} from {}", base, from_doc)
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
                write!(f, "type {} = {}", name, parent)?;
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
            timezone: Some(TimezoneValue {
                offset_hours: 1,
                offset_minutes: 0,
            }),
        };
        let display = format!("{}", dt);
        assert!(display.contains("2024"));
        assert!(display.contains("12"));
        assert!(display.contains("25"));
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
                "test",
                std::sync::Arc::from("doc test\nfact x = 1"),
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
                "test",
                std::sync::Arc::from("doc test\nfact x = 1"),
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
