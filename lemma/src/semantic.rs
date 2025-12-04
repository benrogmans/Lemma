use crate::error::LemmaError;
use crate::parsing::source::Source;
use chrono::{Datelike, Timelike};
use rust_decimal::Decimal;
use serde::Serialize;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

/// A Lemma document containing facts, rules
#[derive(Debug, Clone, PartialEq)]
pub struct LemmaDoc {
    pub name: String,
    pub source_text: Option<String>,
    pub start_line: usize,
    pub commentary: Option<String>,
    pub facts: Vec<LemmaFact>,
    pub rules: Vec<LemmaRule>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LemmaFact {
    pub reference: FactReference,
    pub value: FactValue,
    pub source: Source,
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
    pub source: Source,
}

/// A rule with a single expression and optional unless clauses
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LemmaRule {
    pub name: String,
    pub expression: Expression,
    pub unless_clauses: Vec<UnlessClause>,
    pub source: Source,
}

/// An expression that can be evaluated, with source location
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub source: Source,
}

impl Expression {
    /// Create a new expression with kind and source location
    #[must_use]
    pub fn new(kind: ExpressionKind, source: Source) -> Self {
        Self { kind, source }
    }

    /// Compute a semantic hash of this expression
    ///
    /// Hashes only the semantic content (kind and recursive children), excluding source location.
    /// Semantically equal expressions produce the same hash.
    pub fn semantic_hash<H: Hasher>(&self, state: &mut H) {
        match &self.kind {
            ExpressionKind::Literal(lit) => {
                0u8.hash(state);
                lit.hash(state);
            }
            ExpressionKind::FactPath(path) => {
                1u8.hash(state);
                path.hash(state);
            }
            ExpressionKind::RulePath(path) => {
                2u8.hash(state);
                path.hash(state);
            }
            ExpressionKind::Arithmetic(left, op, right) => {
                3u8.hash(state);
                op.hash(state);
                left.semantic_hash(state);
                right.semantic_hash(state);
            }
            ExpressionKind::LogicalAnd(left, right) => {
                4u8.hash(state);
                left.semantic_hash(state);
                right.semantic_hash(state);
            }
            ExpressionKind::LogicalOr(left, right) => {
                5u8.hash(state);
                left.semantic_hash(state);
                right.semantic_hash(state);
            }
            ExpressionKind::Comparison(left, op, right) => {
                6u8.hash(state);
                op.hash(state);
                left.semantic_hash(state);
                right.semantic_hash(state);
            }
            ExpressionKind::LogicalNegation(inner, neg_type) => {
                7u8.hash(state);
                neg_type.hash(state);
                inner.semantic_hash(state);
            }
            ExpressionKind::MathematicalComputation(op, inner) => {
                8u8.hash(state);
                op.hash(state);
                inner.semantic_hash(state);
            }
            ExpressionKind::UnitConversion(inner, target) => {
                9u8.hash(state);
                target.hash(state);
                inner.semantic_hash(state);
            }
            ExpressionKind::Veto(veto) => {
                10u8.hash(state);
                veto.message.hash(state);
            }
            ExpressionKind::FactReference(fref) => {
                11u8.hash(state);
                fref.hash(state);
            }
            ExpressionKind::RuleReference(rref) => {
                12u8.hash(state);
                rref.hash(state);
            }
        }
    }

    /// Collect all FactPath references from this expression tree.
    pub fn collect_fact_paths(&self, facts: &mut std::collections::HashSet<FactPath>) {
        match &self.kind {
            ExpressionKind::FactPath(fp) => {
                facts.insert(fp.clone());
            }
            ExpressionKind::LogicalAnd(left, right)
            | ExpressionKind::LogicalOr(left, right)
            | ExpressionKind::Arithmetic(left, _, right)
            | ExpressionKind::Comparison(left, _, right) => {
                left.collect_fact_paths(facts);
                right.collect_fact_paths(facts);
            }
            ExpressionKind::UnitConversion(inner, _)
            | ExpressionKind::LogicalNegation(inner, _)
            | ExpressionKind::MathematicalComputation(_, inner) => {
                inner.collect_fact_paths(facts);
            }
            ExpressionKind::Literal(_)
            | ExpressionKind::FactReference(_)
            | ExpressionKind::RuleReference(_)
            | ExpressionKind::Veto(_)
            | ExpressionKind::RulePath(_) => {}
        }
    }

    /// Returns true if this expression is a boolean false literal
    #[must_use]
    pub fn is_boolean_false(&self) -> bool {
        matches!(
            self.kind,
            ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False))
        )
    }

    /// Returns true if this expression is a boolean true literal
    #[must_use]
    pub fn is_boolean_true(&self) -> bool {
        matches!(
            self.kind,
            ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True))
        )
    }

    /// Check if this expression is semantically equal to another expression
    ///
    /// Compares semantic hashes for equality.
    #[must_use]
    pub fn semantically_equal(&self, other: &Expression) -> bool {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        self.semantic_hash(&mut hasher1);
        other.semantic_hash(&mut hasher2);
        hasher1.finish() == hasher2.finish()
    }
}

impl Hash for Expression {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.semantic_hash(state);
    }
}

impl Eq for Expression {}

/// The kind/type of expression
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExpressionKind {
    Literal(LiteralValue),
    FactReference(FactReference),
    RuleReference(RuleReference),
    LogicalAnd(Box<Expression>, Box<Expression>),
    LogicalOr(Box<Expression>, Box<Expression>),
    Arithmetic(Box<Expression>, ArithmeticComputation, Box<Expression>),
    Comparison(Box<Expression>, ComparisonComputation, Box<Expression>),
    UnitConversion(Box<Expression>, ConversionTarget),
    LogicalNegation(Box<Expression>, NegationType),
    MathematicalComputation(MathematicalComputation, Box<Expression>),
    Veto(VetoExpression),
    /// Resolved fact path (used after planning, converted from FactReference)
    FactPath(FactPath),
    /// Resolved rule path (used after planning, converted from RuleReference)
    RulePath(RulePath),
}

/// Reference to a fact
///
/// Fact references use dot notation to traverse documents.
/// Examples:
/// - Local fact "age": segments=[], fact="age"
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

/// A single segment in a path traversal
///
/// Used in both FactPath and RulePath to represent document traversal.
/// Each segment contains a fact name that points to a document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PathSegment {
    /// Fact name at this segment
    pub fact: String,

    /// Document name this fact points to
    pub doc: String,
}

/// A resolved path to a fact, with document traversal segments
///
/// Used after planning to represent fully resolved fact references.
/// Public because used in ExecutionPlan and evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FactPath {
    /// Path segments: each segment is a fact name that points to a document
    pub segments: Vec<PathSegment>,

    /// Final fact name
    pub fact: String,
}

impl FactPath {
    /// Returns true if this is a local fact (no document traversal)
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.segments.is_empty()
    }

    /// Create a new FactPath from segments and fact name
    #[must_use]
    pub fn new(segments: Vec<PathSegment>, fact: String) -> Self {
        Self { segments, fact }
    }

    /// Create a local fact path (no document traversal)
    #[must_use]
    pub fn local(fact: String) -> Self {
        Self {
            segments: Vec::new(),
            fact,
        }
    }

    /// Create a FactPath from a full path of strings
    ///
    /// The last element becomes the fact name, all others become segments.
    /// Segment doc fields are left empty since we only have fact names.
    /// This is for backward compatibility with tests.
    #[must_use]
    pub fn from_path(mut path: Vec<String>) -> Self {
        if path.is_empty() {
            return Self {
                segments: Vec::new(),
                fact: String::new(),
            };
        }
        let fact = path.pop().unwrap_or_default();
        let segments = path
            .into_iter()
            .map(|fact_name| PathSegment {
                fact: fact_name,
                doc: String::new(),
            })
            .collect();
        Self { segments, fact }
    }

    /// Get all path segments as fact names including the final fact name
    #[must_use]
    pub fn full_path(&self) -> Vec<String> {
        let mut path: Vec<String> = self.segments.iter().map(|s| s.fact.clone()).collect();
        path.push(self.fact.clone());
        path
    }
}

/// A resolved path to a rule, with document traversal segments
///
/// Used after planning to represent fully resolved rule references.
/// Public because used in ExecutionPlan and evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RulePath {
    /// Path segments: each segment is a fact name that points to a document
    pub segments: Vec<PathSegment>,

    /// Final rule name
    pub rule: String,
}

impl RulePath {
    /// Returns true if this is a local rule (no document traversal)
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.segments.is_empty()
    }

    /// Create a local rule path (no document traversal)
    #[must_use]
    pub fn local(rule: String) -> Self {
        Self {
            segments: Vec::new(),
            rule,
        }
    }
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

    /// Returns true if this is a local rule reference (no path segments)
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.segments.is_empty()
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
pub enum ArithmeticComputation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
}

impl ArithmeticComputation {
    /// Returns a human-readable name for the computation
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            ArithmeticComputation::Add => "addition",
            ArithmeticComputation::Subtract => "subtraction",
            ArithmeticComputation::Multiply => "multiplication",
            ArithmeticComputation::Divide => "division",
            ArithmeticComputation::Modulo => "modulo",
            ArithmeticComputation::Power => "exponentiation",
        }
    }

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

/// Notation style for equality comparisons (for display purposes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize, Default)]
pub enum EqualityNotation {
    /// Symbol notation: == or !=
    #[default]
    Symbol,
    /// Word notation: is or is not
    Word,
}

/// Comparison computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub enum ComparisonComputation {
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Equal(EqualityNotation),
    NotEqual(EqualityNotation),
}

impl ComparisonComputation {
    /// Returns a human-readable name for the computation
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            ComparisonComputation::GreaterThan => "greater than",
            ComparisonComputation::LessThan => "less than",
            ComparisonComputation::GreaterThanOrEqual => "greater than or equal",
            ComparisonComputation::LessThanOrEqual => "less than or equal",
            ComparisonComputation::Equal(EqualityNotation::Symbol) => "equal",
            ComparisonComputation::Equal(EqualityNotation::Word) => "is",
            ComparisonComputation::NotEqual(EqualityNotation::Symbol) => "not equal",
            ComparisonComputation::NotEqual(EqualityNotation::Word) => "is not",
        }
    }

    /// Returns the operator symbol
    #[must_use]
    pub fn symbol(&self) -> &'static str {
        match self {
            ComparisonComputation::GreaterThan => ">",
            ComparisonComputation::LessThan => "<",
            ComparisonComputation::GreaterThanOrEqual => ">=",
            ComparisonComputation::LessThanOrEqual => "<=",
            ComparisonComputation::Equal(EqualityNotation::Symbol) => "==",
            ComparisonComputation::Equal(EqualityNotation::Word) => "is",
            ComparisonComputation::NotEqual(EqualityNotation::Symbol) => "!=",
            ComparisonComputation::NotEqual(EqualityNotation::Word) => "is not",
        }
    }

    /// Check if this is an equality comparison (== or is)
    #[must_use]
    pub fn is_equal(&self) -> bool {
        matches!(self, ComparisonComputation::Equal(_))
    }

    /// Check if this is an inequality comparison (!= or is not)
    #[must_use]
    pub fn is_not_equal(&self) -> bool {
        matches!(self, ComparisonComputation::NotEqual(_))
    }
}

/// The target unit for unit conversion expressions
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConversionTarget {
    Mass(MassUnit),
    Length(LengthUnit),
    Volume(VolumeUnit),
    Duration(DurationUnit),
    Temperature(TemperatureUnit),
    Power(PowerUnit),
    Force(ForceUnit),
    Pressure(PressureUnit),
    Energy(EnergyUnit),
    Frequency(FrequencyUnit),
    Data(DataUnit),
    Percentage,
}

/// Types of logical negation
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NegationType {
    Not, // "not expression"
}

/// Logical computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum LogicalComputation {
    And,
    Or,
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
pub enum MathematicalComputation {
    Sqrt,  // Square root
    Sin,   // Sine
    Cos,   // Cosine
    Tan,   // Tangent
    Asin,  // Arc sine
    Acos,  // Arc cosine
    Atan,  // Arc tangent
    Log,   // Natural logarithm
    Exp,   // Exponential (e^x)
    Abs,   // Absolute value
    Floor, // Round down
    Ceil,  // Round up
    Round, // Round to nearest
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FactValue {
    Literal(LiteralValue),
    DocumentReference(String),
    TypeAnnotation(TypeAnnotation),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TypeAnnotation {
    LemmaType(LemmaType),
}

/// A type for type annotations (both literal types and document types)
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LemmaType {
    Text,
    Number,
    Date,
    Boolean,
    Regex,
    Percentage,
    Mass,
    Length,
    Volume,
    Duration,
    Temperature,
    Power,
    Energy,
    Force,
    Pressure,
    Frequency,
    Data,
}

impl LemmaType {
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            LemmaType::Number
                | LemmaType::Percentage
                | LemmaType::Mass
                | LemmaType::Length
                | LemmaType::Volume
                | LemmaType::Duration
                | LemmaType::Temperature
                | LemmaType::Power
                | LemmaType::Energy
                | LemmaType::Force
                | LemmaType::Pressure
                | LemmaType::Frequency
                | LemmaType::Data
        )
    }

    pub fn is_temporal(&self) -> bool {
        matches!(self, LemmaType::Date)
    }

    pub fn is_unit(&self) -> bool {
        matches!(
            self,
            LemmaType::Mass
                | LemmaType::Length
                | LemmaType::Volume
                | LemmaType::Duration
                | LemmaType::Temperature
                | LemmaType::Power
                | LemmaType::Energy
                | LemmaType::Force
                | LemmaType::Pressure
                | LemmaType::Frequency
                | LemmaType::Data
        )
    }

    /// Parse a raw string value into a LiteralValue according to this type.
    /// This is the main entry point for type-aware parsing from user input.
    pub fn parse_value(&self, raw: &str) -> Result<LiteralValue, LemmaError> {
        match self {
            LemmaType::Text => Self::parse_text(raw),
            LemmaType::Number => Self::parse_number(raw),
            LemmaType::Boolean => Self::parse_boolean(raw),
            LemmaType::Percentage => Self::parse_percentage(raw),
            LemmaType::Date => Self::parse_date(raw),
            LemmaType::Regex => Self::parse_regex(raw),
            LemmaType::Mass => Self::parse_unit_value(raw, LemmaType::Mass),
            LemmaType::Length => Self::parse_unit_value(raw, LemmaType::Length),
            LemmaType::Volume => Self::parse_unit_value(raw, LemmaType::Volume),
            LemmaType::Duration => Self::parse_unit_value(raw, LemmaType::Duration),
            LemmaType::Temperature => Self::parse_unit_value(raw, LemmaType::Temperature),
            LemmaType::Power => Self::parse_unit_value(raw, LemmaType::Power),
            LemmaType::Energy => Self::parse_unit_value(raw, LemmaType::Energy),
            LemmaType::Force => Self::parse_unit_value(raw, LemmaType::Force),
            LemmaType::Pressure => Self::parse_unit_value(raw, LemmaType::Pressure),
            LemmaType::Frequency => Self::parse_unit_value(raw, LemmaType::Frequency),
            LemmaType::Data => Self::parse_unit_value(raw, LemmaType::Data),
        }
    }

    fn parse_text(raw: &str) -> Result<LiteralValue, LemmaError> {
        Ok(LiteralValue::Text(raw.to_string()))
    }

    fn parse_number(raw: &str) -> Result<LiteralValue, LemmaError> {
        let clean_number = raw.replace(['_', ','], "");
        let decimal = Decimal::from_str(&clean_number).map_err(|_| {
            LemmaError::Engine(format!(
                "Invalid number: '{}'. Expected a valid decimal number (e.g., 42, 3.14, 1_000_000)",
                raw
            ))
        })?;
        Ok(LiteralValue::Number(decimal))
    }

    fn parse_boolean(raw: &str) -> Result<LiteralValue, LemmaError> {
        let boolean_value: BooleanValue = raw.parse().map_err(|_| {
            LemmaError::Engine(format!(
                "Invalid boolean: '{}'. Expected one of: true, false, yes, no, accept, reject",
                raw
            ))
        })?;
        Ok(LiteralValue::Boolean(boolean_value))
    }

    fn parse_percentage(raw: &str) -> Result<LiteralValue, LemmaError> {
        let trimmed = raw.trim();
        let number_str = if trimmed.ends_with('%') {
            trimmed.strip_suffix('%').unwrap_or(trimmed)
        } else if trimmed.to_lowercase().ends_with("percent") {
            trimmed.strip_suffix("percent").unwrap_or(trimmed).trim()
        } else {
            trimmed
        };

        let clean_number = number_str.replace(['_', ','], "");
        let decimal = Decimal::from_str(&clean_number).map_err(|_| {
            LemmaError::Engine(format!(
                "Invalid percentage: '{}'. Expected a number optionally followed by % (e.g., 50, 50%, 50 percent)",
                raw
            ))
        })?;
        Ok(LiteralValue::Percentage(decimal))
    }

    fn parse_date(raw: &str) -> Result<LiteralValue, LemmaError> {
        let datetime_str = raw.trim();

        if let Ok(dt) = datetime_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
            let offset = dt.offset().local_minus_utc();
            return Ok(LiteralValue::Date(DateTimeValue {
                year: dt.year(),
                month: dt.month(),
                day: dt.day(),
                hour: dt.hour(),
                minute: dt.minute(),
                second: dt.second(),
                timezone: Some(TimezoneValue {
                    offset_hours: (offset / 3600) as i8,
                    offset_minutes: ((offset % 3600) / 60) as u8,
                }),
            }));
        }

        if let Ok(dt) = datetime_str.parse::<chrono::NaiveDateTime>() {
            return Ok(LiteralValue::Date(DateTimeValue {
                year: dt.year(),
                month: dt.month(),
                day: dt.day(),
                hour: dt.hour(),
                minute: dt.minute(),
                second: dt.second(),
                timezone: None,
            }));
        }

        if let Ok(d) = datetime_str.parse::<chrono::NaiveDate>() {
            return Ok(LiteralValue::Date(DateTimeValue {
                year: d.year(),
                month: d.month(),
                day: d.day(),
                hour: 0,
                minute: 0,
                second: 0,
                timezone: None,
            }));
        }

        Err(LemmaError::Engine(format!(
            "Invalid date/time format: '{}'. Expected one of: YYYY-MM-DD, YYYY-MM-DDTHH:MM:SS, or YYYY-MM-DDTHH:MM:SSZ",
            raw
        )))
    }

    fn parse_regex(raw: &str) -> Result<LiteralValue, LemmaError> {
        let trimmed = raw.trim();
        let pattern = if trimmed.starts_with('/') && trimmed.ends_with('/') && trimmed.len() >= 2 {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        };

        regex::Regex::new(pattern)
            .map_err(|e| LemmaError::Engine(format!("Invalid regex pattern '{}': {}", raw, e)))?;

        if trimmed.starts_with('/') && trimmed.ends_with('/') {
            Ok(LiteralValue::Regex(trimmed.to_string()))
        } else {
            Ok(LiteralValue::Regex(format!("/{}/", pattern)))
        }
    }

    fn parse_unit_value(raw: &str, expected_type: LemmaType) -> Result<LiteralValue, LemmaError> {
        let trimmed = raw.trim();
        let parts: Vec<&str> = trimmed.splitn(2, |c: char| c.is_whitespace()).collect();

        if parts.len() != 2 {
            return Err(LemmaError::Engine(format!(
                "Invalid {} value: '{}'. Expected format: '<number> <unit>' (e.g., '100 kilogram')",
                expected_type, raw
            )));
        }

        let number_str = parts[0];
        let unit_str = parts[1].trim();

        let clean_number = number_str.replace(['_', ','], "");
        let value = Decimal::from_str(&clean_number).map_err(|_| {
            LemmaError::Engine(format!(
                "Invalid number in {} value: '{}'. Expected a valid decimal number",
                expected_type, number_str
            ))
        })?;

        let literal = crate::parsing::units::resolve_unit(value, unit_str)?;

        let actual_type = literal.to_type();
        if actual_type != expected_type {
            return Err(LemmaError::Engine(format!(
                "Unit type mismatch: '{}' is a {} unit, but expected {}",
                unit_str, actual_type, expected_type
            )));
        }

        Ok(literal)
    }
}

/// Boolean value with original input preserved
#[derive(
    Debug,
    Clone,
    PartialEq,
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

/// A literal value
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub enum LiteralValue {
    Number(Decimal),
    Text(String),
    Date(DateTimeValue), // Date with time and timezone information preserved
    Time(TimeValue),     // Standalone time with optional timezone
    Boolean(BooleanValue),
    Percentage(Decimal),
    Unit(NumericUnit), // All physical units and money
    Regex(String),     // e.g., "/pattern/"
}

impl LiteralValue {
    /// Create a Number literal value from any type that can convert to Decimal
    pub fn number<T: Into<Decimal>>(value: T) -> Self {
        LiteralValue::Number(value.into())
    }

    /// Get the display value as a string (uses the Display implementation)
    #[must_use]
    pub fn display_value(&self) -> String {
        self.to_string()
    }

    /// Get the byte size of this literal value for resource limiting
    pub fn byte_size(&self) -> usize {
        match self {
            LiteralValue::Text(s) | LiteralValue::Regex(s) => s.len(),
            LiteralValue::Number(d) | LiteralValue::Percentage(d) => {
                // Decimal internal representation size
                std::mem::size_of_val(d)
            }
            LiteralValue::Boolean(_) => std::mem::size_of::<bool>(),
            LiteralValue::Date(_) => std::mem::size_of::<DateTimeValue>(),
            LiteralValue::Time(_) => std::mem::size_of::<TimeValue>(),
            LiteralValue::Unit(_) => std::mem::size_of::<NumericUnit>(),
        }
    }

    /// Convert a LiteralValue to its corresponding LemmaType
    #[must_use]
    pub fn to_type(&self) -> LemmaType {
        match self {
            LiteralValue::Text(_) => LemmaType::Text,
            LiteralValue::Number(_) => LemmaType::Number,
            LiteralValue::Date(_) => LemmaType::Date,
            LiteralValue::Time(_) => LemmaType::Date,
            LiteralValue::Boolean(_) => LemmaType::Boolean,
            LiteralValue::Percentage(_) => LemmaType::Percentage,
            LiteralValue::Regex(_) => LemmaType::Regex,
            LiteralValue::Unit(unit) => match unit {
                NumericUnit::Mass(_, _) => LemmaType::Mass,
                NumericUnit::Length(_, _) => LemmaType::Length,
                NumericUnit::Volume(_, _) => LemmaType::Volume,
                NumericUnit::Duration(_, _) => LemmaType::Duration,
                NumericUnit::Temperature(_, _) => LemmaType::Temperature,
                NumericUnit::Power(_, _) => LemmaType::Power,
                NumericUnit::Force(_, _) => LemmaType::Force,
                NumericUnit::Pressure(_, _) => LemmaType::Pressure,
                NumericUnit::Energy(_, _) => LemmaType::Energy,
                NumericUnit::Frequency(_, _) => LemmaType::Frequency,
                NumericUnit::Data(_, _) => LemmaType::Data,
            },
        }
    }
}

impl Hash for LiteralValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            LiteralValue::Number(d) => {
                0u8.hash(state);
                d.to_string().hash(state);
            }
            LiteralValue::Text(s) => {
                1u8.hash(state);
                s.hash(state);
            }
            LiteralValue::Date(dt) => {
                2u8.hash(state);
                dt.year.hash(state);
                dt.month.hash(state);
                dt.day.hash(state);
                dt.hour.hash(state);
                dt.minute.hash(state);
                dt.second.hash(state);
                dt.timezone.hash(state);
            }
            LiteralValue::Time(t) => {
                3u8.hash(state);
                t.hour.hash(state);
                t.minute.hash(state);
                t.second.hash(state);
                t.timezone.hash(state);
            }
            LiteralValue::Boolean(b) => {
                4u8.hash(state);
                format!("{:?}", b).hash(state);
            }
            LiteralValue::Percentage(d) => {
                5u8.hash(state);
                d.to_string().hash(state);
            }
            LiteralValue::Unit(u) => {
                6u8.hash(state);
                u.hash(state);
            }
            LiteralValue::Regex(s) => {
                7u8.hash(state);
                s.hash(state);
            }
        }
    }
}

impl Hash for NumericUnit {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            NumericUnit::Mass(v, unit) => {
                0u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Length(v, unit) => {
                1u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Volume(v, unit) => {
                2u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Duration(v, unit) => {
                3u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Temperature(v, unit) => {
                4u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Power(v, unit) => {
                5u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Force(v, unit) => {
                6u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Pressure(v, unit) => {
                7u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Energy(v, unit) => {
                8u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Frequency(v, unit) => {
                9u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
            NumericUnit::Data(v, unit) => {
                10u8.hash(state);
                v.to_string().hash(state);
                format!("{:?}", unit).hash(state);
            }
        }
    }
}

/// A time value
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, serde::Deserialize)]
pub struct TimeValue {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub timezone: Option<TimezoneValue>,
}

/// A timezone value
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub struct TimezoneValue {
    pub offset_hours: i8,
    pub offset_minutes: u8,
}

/// A datetime value that preserves timezone information
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
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

impl_unit_serialize!(
    MassUnit,
    LengthUnit,
    VolumeUnit,
    DurationUnit,
    TemperatureUnit,
    PowerUnit,
    ForceUnit,
    PressureUnit,
    EnergyUnit,
    FrequencyUnit,
    DataUnit
);

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum MassUnit {
    Kilogram,
    Gram,
    Milligram,
    Ton,
    Pound,
    Ounce,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum LengthUnit {
    Kilometer,
    Mile,
    #[strum(serialize = "nautical_mile")]
    NauticalMile,
    Meter,
    Decimeter,
    Centimeter,
    Millimeter,
    Yard,
    Foot,
    Inch,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum VolumeUnit {
    #[strum(serialize = "cubic_meter")]
    CubicMeter,
    #[strum(serialize = "cubic_centimeter")]
    CubicCentimeter,
    Liter,
    Deciliter,
    Centiliter,
    Milliliter,
    Gallon,
    Quart,
    Pint,
    #[strum(serialize = "fluid_ounce")]
    FluidOunce,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
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

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
    Kelvin,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum PowerUnit {
    Megawatt,
    Kilowatt,
    Watt,
    Milliwatt,
    Horsepower,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum ForceUnit {
    Newton,
    Kilonewton,
    Lbf,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum PressureUnit {
    Megapascal,
    Kilopascal,
    Pascal,
    Atmosphere,
    Bar,
    Psi,
    Torr,
    Mmhg,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum EnergyUnit {
    Megajoule,
    Kilojoule,
    Joule,
    Kilowatthour,
    Watthour,
    Kilocalorie,
    Calorie,
    Btu,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum FrequencyUnit {
    Hertz,
    Kilohertz,
    Megahertz,
    Gigahertz,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum DataUnit {
    Petabyte,
    Terabyte,
    Gigabyte,
    Megabyte,
    Kilobyte,
    Byte,
    Tebibyte,
    Gibibyte,
    Mebibyte,
    Kibibyte,
}

/// A unified type for all numeric units (physical quantities)
///
/// This provides consistent behavior for all unit types:
/// - Comparisons always compare numeric values (ignoring units)
/// - Same-unit arithmetic preserves the unit
/// - Cross-unit arithmetic produces dimensionless numbers
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub enum NumericUnit {
    Mass(Decimal, MassUnit),
    Length(Decimal, LengthUnit),
    Volume(Decimal, VolumeUnit),
    Duration(Decimal, DurationUnit),
    Temperature(Decimal, TemperatureUnit),
    Power(Decimal, PowerUnit),
    Force(Decimal, ForceUnit),
    Pressure(Decimal, PressureUnit),
    Energy(Decimal, EnergyUnit),
    Frequency(Decimal, FrequencyUnit),
    Data(Decimal, DataUnit),
}

impl NumericUnit {
    /// Extract the numeric value from any unit
    #[must_use]
    pub fn value(&self) -> Decimal {
        match self {
            NumericUnit::Mass(v, _)
            | NumericUnit::Length(v, _)
            | NumericUnit::Volume(v, _)
            | NumericUnit::Duration(v, _)
            | NumericUnit::Temperature(v, _)
            | NumericUnit::Power(v, _)
            | NumericUnit::Force(v, _)
            | NumericUnit::Pressure(v, _)
            | NumericUnit::Energy(v, _)
            | NumericUnit::Frequency(v, _)
            | NumericUnit::Data(v, _) => *v,
        }
    }

    /// Check if two units are the same category
    pub fn same_category(&self, other: &NumericUnit) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// Create a new NumericUnit with the same unit type but different value
    /// This is the key method that eliminates type enumeration in operations
    #[must_use]
    pub fn with_value(&self, new_value: Decimal) -> NumericUnit {
        match self {
            NumericUnit::Mass(_, u) => NumericUnit::Mass(new_value, u.clone()),
            NumericUnit::Length(_, u) => NumericUnit::Length(new_value, u.clone()),
            NumericUnit::Volume(_, u) => NumericUnit::Volume(new_value, u.clone()),
            NumericUnit::Duration(_, u) => NumericUnit::Duration(new_value, u.clone()),
            NumericUnit::Temperature(_, u) => NumericUnit::Temperature(new_value, u.clone()),
            NumericUnit::Power(_, u) => NumericUnit::Power(new_value, u.clone()),
            NumericUnit::Force(_, u) => NumericUnit::Force(new_value, u.clone()),
            NumericUnit::Pressure(_, u) => NumericUnit::Pressure(new_value, u.clone()),
            NumericUnit::Energy(_, u) => NumericUnit::Energy(new_value, u.clone()),
            NumericUnit::Frequency(_, u) => NumericUnit::Frequency(new_value, u.clone()),
            NumericUnit::Data(_, u) => NumericUnit::Data(new_value, u.clone()),
        }
    }
}

fn format_decimal_with_unit(value: &Decimal, unit: &impl fmt::Display) -> String {
    let normalized = value.normalize();
    if normalized.fract().is_zero() {
        let int_part = normalized.trunc().to_string();
        let formatted = int_part
            .chars()
            .rev()
            .enumerate()
            .flat_map(|(i, c)| {
                if i > 0 && i % 3 == 0 && c != '-' {
                    vec![',', c]
                } else {
                    vec![c]
                }
            })
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        format!("{} {}", formatted, unit)
    } else {
        format!("{} {}", normalized, unit)
    }
}

impl fmt::Display for NumericUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumericUnit::Mass(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Length(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Volume(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Duration(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Temperature(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Power(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Force(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Pressure(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Energy(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Frequency(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
            NumericUnit::Data(v, u) => write!(f, "{}", format_decimal_with_unit(v, u)),
        }
    }
}

impl FactReference {
    /// Create a new FactReference from segments and fact name
    #[must_use]
    pub fn new(segments: Vec<String>, fact: String) -> Self {
        Self { segments, fact }
    }

    /// Create a FactReference from a single fact name (local reference)
    #[must_use]
    pub fn local(fact: String) -> Self {
        Self {
            segments: Vec::new(),
            fact,
        }
    }

    /// Create a FactReference from a Vec<String> path (for backward compatibility during migration)
    #[must_use]
    pub fn from_path(path: Vec<String>) -> Self {
        if path.is_empty() {
            Self {
                segments: Vec::new(),
                fact: String::new(),
            }
        } else {
            let fact = path
                .last()
                .expect("bug: path was checked for empty but last() returned None")
                .clone();
            let segments = path[..path.len() - 1].to_vec();
            Self { segments, fact }
        }
    }

    /// Returns true if this is a local reference (no path segments)
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.segments.is_empty()
    }

    /// Get all path segments including the fact name
    #[must_use]
    pub fn full_path(&self) -> Vec<String> {
        let mut path = self.segments.clone();
        path.push(self.fact.clone());
        path
    }
}

impl LemmaFact {
    #[must_use]
    pub fn new(reference: FactReference, value: FactValue) -> Self {
        Self {
            reference,
            value,
            source: Source::new(
                "<unknown>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "unknown",
            ),
        }
    }

    #[must_use]
    pub fn with_source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    /// Returns true if this fact is local (not a cross-document reference)
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.reference.is_local()
    }
}

impl LemmaDoc {
    #[must_use]
    pub fn new(name: String) -> Self {
        Self {
            name,
            source_text: None,
            start_line: 1,
            commentary: None,
            facts: Vec::new(),
            rules: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_source_text(mut self, source_text: String) -> Self {
        self.source_text = Some(source_text);
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

    /// Get the expected type for a fact by path
    /// Returns None if the fact is not found in this document or if the fact is a document reference
    pub fn get_fact_type(&self, fact_ref: &[String]) -> Option<LemmaType> {
        let fact_path: Vec<String> = fact_ref.to_vec();
        let fact_name = fact_path.last()?.clone();
        let segments: Vec<String> = fact_path[..fact_path.len().saturating_sub(1)].to_vec();
        let target_ref = FactReference {
            segments,
            fact: fact_name,
        };
        self.facts
            .iter()
            .find(|fact| fact.reference == target_ref)
            .and_then(|fact| match &fact.value {
                FactValue::Literal(lit) => Some(lit.to_type()),
                FactValue::TypeAnnotation(TypeAnnotation::LemmaType(lemma_type)) => {
                    Some(lemma_type.clone())
                }
                FactValue::DocumentReference(_) => None,
            })
    }
}

impl fmt::Display for LemmaDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "doc {}", self.name)?;
        writeln!(f)?;

        if let Some(ref commentary) = self.commentary {
            writeln!(f, "\"\"\"{}", commentary)?;
            writeln!(f, "\"\"\"")?;
        }

        for fact in &self.facts {
            write!(f, "{}", fact)?;
        }

        for rule in &self.rules {
            write!(f, "{}", rule)?;
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
                " unless {} then {}",
                unless_clause.condition, unless_clause.result
            )?;
        }

        writeln!(f)?;
        Ok(())
    }
}

impl fmt::Display for Expression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ExpressionKind::Literal(lit) => write!(f, "{}", lit),
            ExpressionKind::FactReference(fact_ref) => write!(f, "{}", fact_ref),
            ExpressionKind::FactPath(fact_path) => write!(f, "{}", fact_path),
            ExpressionKind::RuleReference(rule_ref) => write!(f, "{}", rule_ref),
            ExpressionKind::RulePath(rule_path) => write!(f, "{}", rule_path),
            ExpressionKind::Arithmetic(left, op, right) => {
                write!(f, "{} {} {}", left, op, right)
            }
            ExpressionKind::Comparison(left, op, right) => {
                write!(f, "{} {} {}", left, op, right)
            }
            ExpressionKind::UnitConversion(value, target) => {
                write!(f, "{} in {}", value, target)
            }
            ExpressionKind::LogicalNegation(expr, _) => {
                write!(f, "not {}", expr)
            }
            ExpressionKind::LogicalAnd(left, right) => {
                write!(f, "{} and {}", left, right)
            }
            ExpressionKind::LogicalOr(left, right) => {
                write!(f, "{} or {}", left, right)
            }
            ExpressionKind::MathematicalComputation(op, operand) => {
                let op_name = match op {
                    MathematicalComputation::Sqrt => "sqrt",
                    MathematicalComputation::Sin => "sin",
                    MathematicalComputation::Cos => "cos",
                    MathematicalComputation::Tan => "tan",
                    MathematicalComputation::Asin => "asin",
                    MathematicalComputation::Acos => "acos",
                    MathematicalComputation::Atan => "atan",
                    MathematicalComputation::Log => "log",
                    MathematicalComputation::Exp => "exp",
                    MathematicalComputation::Abs => "abs",
                    MathematicalComputation::Floor => "floor",
                    MathematicalComputation::Ceil => "ceil",
                    MathematicalComputation::Round => "round",
                };
                write!(f, "{} {}", op_name, operand)
            }
            ExpressionKind::Veto(veto) => match &veto.message {
                Some(msg) => write!(f, "veto \"{}\"", msg),
                None => write!(f, "veto"),
            },
        }
    }
}

impl fmt::Display for LiteralValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiteralValue::Number(n) => {
                let normalized = n.normalize();
                if normalized.fract().is_zero() {
                    let int_part = normalized.trunc().to_string();
                    let formatted = int_part
                        .chars()
                        .rev()
                        .enumerate()
                        .flat_map(|(i, c)| {
                            if i > 0 && i % 3 == 0 && c != '-' {
                                vec![',', c]
                            } else {
                                vec![c]
                            }
                        })
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>();
                    write!(f, "{}", formatted)
                } else {
                    write!(f, "{}", normalized)
                }
            }
            LiteralValue::Text(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                write!(f, "\"{}\"", escaped)
            }
            LiteralValue::Date(dt) => write!(f, "{}", dt),
            LiteralValue::Boolean(b) => write!(f, "{}", b),
            LiteralValue::Percentage(p) => {
                let rounded = p.round_dp(2);
                if rounded.fract().is_zero() {
                    write!(f, "{}%", rounded.trunc())
                } else {
                    write!(f, "{}%", rounded)
                }
            }
            LiteralValue::Unit(unit) => write!(f, "{}", unit),
            LiteralValue::Regex(s) => write!(f, "{}", s),
            LiteralValue::Time(time) => {
                write!(f, "time({}, {}, {})", time.hour, time.minute, time.second)
            }
        }
    }
}

impl fmt::Display for ConversionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConversionTarget::Mass(unit) => write!(f, "{}", unit),
            ConversionTarget::Length(unit) => write!(f, "{}", unit),
            ConversionTarget::Volume(unit) => write!(f, "{}", unit),
            ConversionTarget::Duration(unit) => write!(f, "{}", unit),
            ConversionTarget::Temperature(unit) => write!(f, "{}", unit),
            ConversionTarget::Power(unit) => write!(f, "{}", unit),
            ConversionTarget::Force(unit) => write!(f, "{}", unit),
            ConversionTarget::Pressure(unit) => write!(f, "{}", unit),
            ConversionTarget::Energy(unit) => write!(f, "{}", unit),
            ConversionTarget::Frequency(unit) => write!(f, "{}", unit),
            ConversionTarget::Data(unit) => write!(f, "{}", unit),
            ConversionTarget::Percentage => write!(f, "percentage"),
        }
    }
}

impl fmt::Display for LemmaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LemmaType::Text => write!(f, "text"),
            LemmaType::Number => write!(f, "number"),
            LemmaType::Date => write!(f, "date"),
            LemmaType::Boolean => write!(f, "boolean"),
            LemmaType::Regex => write!(f, "regex"),
            LemmaType::Percentage => write!(f, "percentage"),
            LemmaType::Mass => write!(f, "mass"),
            LemmaType::Length => write!(f, "length"),
            LemmaType::Volume => write!(f, "volume"),
            LemmaType::Duration => write!(f, "duration"),
            LemmaType::Temperature => write!(f, "temperature"),
            LemmaType::Power => write!(f, "power"),
            LemmaType::Force => write!(f, "force"),
            LemmaType::Pressure => write!(f, "pressure"),
            LemmaType::Energy => write!(f, "energy"),
            LemmaType::Frequency => write!(f, "frequency"),
            LemmaType::Data => write!(f, "data"),
        }
    }
}

impl fmt::Display for TypeAnnotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeAnnotation::LemmaType(lemma_type) => write!(f, "{}", lemma_type),
        }
    }
}

impl LemmaType {
    /// Get an example value string for this type, suitable for UI help text
    #[must_use]
    pub fn example_value(&self) -> &'static str {
        match self {
            LemmaType::Text => "\"hello world\"",
            LemmaType::Number => "3.14",
            LemmaType::Boolean => "true",
            LemmaType::Date => "2023-12-25T14:30:00Z",
            LemmaType::Duration => "90 minutes",
            LemmaType::Mass => "5.5 kilograms",
            LemmaType::Length => "10 meters",
            LemmaType::Percentage => "50%",
            LemmaType::Temperature => "25 celsius",
            LemmaType::Regex => "/pattern/",
            LemmaType::Volume => "1.2 liter",
            LemmaType::Power => "100 watts",
            LemmaType::Energy => "1000 joules",
            LemmaType::Force => "10 newtons",
            LemmaType::Pressure => "101325 pascals",
            LemmaType::Frequency => "880 hertz",
            LemmaType::Data => "800 megabytes",
        }
    }
}

impl TypeAnnotation {
    /// Get an example value string for this type annotation, suitable for UI help text
    #[must_use]
    pub fn example_value(&self) -> &'static str {
        match self {
            TypeAnnotation::LemmaType(lemma_type) => lemma_type.example_value(),
        }
    }
}

impl fmt::Display for FactValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FactValue::Literal(lit) => write!(f, "{}", lit),
            FactValue::TypeAnnotation(type_ann) => write!(f, "[{}]", type_ann),
            FactValue::DocumentReference(doc_name) => write!(f, "doc {}", doc_name),
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
            ComparisonComputation::Equal(EqualityNotation::Symbol) => write!(f, "=="),
            ComparisonComputation::Equal(EqualityNotation::Word) => write!(f, "is"),
            ComparisonComputation::NotEqual(EqualityNotation::Symbol) => write!(f, "!="),
            ComparisonComputation::NotEqual(EqualityNotation::Word) => write!(f, "is not"),
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

impl fmt::Display for RuleReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            write!(f, "{}?", self.rule)
        } else {
            write!(f, "{}.{}?", self.segments.join("."), self.rule)
        }
    }
}

impl fmt::Display for FactPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            write!(f, "{}.", segment.fact)?;
        }
        write!(f, "{}", self.fact)
    }
}

impl fmt::Display for RulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            write!(f, "{}.", segment.fact)?;
        }
        write!(f, "{}?", self.rule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn test_arithmetic_operation_name() {
        assert_eq!(ArithmeticComputation::Add.name(), "addition");
        assert_eq!(ArithmeticComputation::Subtract.name(), "subtraction");
        assert_eq!(ArithmeticComputation::Multiply.name(), "multiplication");
        assert_eq!(ArithmeticComputation::Divide.name(), "division");
        assert_eq!(ArithmeticComputation::Modulo.name(), "modulo");
        assert_eq!(ArithmeticComputation::Power.name(), "exponentiation");
    }

    #[test]
    fn test_comparison_operator_name() {
        assert_eq!(ComparisonComputation::GreaterThan.name(), "greater than");
        assert_eq!(ComparisonComputation::LessThan.name(), "less than");
        assert_eq!(
            ComparisonComputation::GreaterThanOrEqual.name(),
            "greater than or equal"
        );
        assert_eq!(
            ComparisonComputation::LessThanOrEqual.name(),
            "less than or equal"
        );
        assert_eq!(
            ComparisonComputation::Equal(EqualityNotation::Symbol).name(),
            "equal"
        );
        assert_eq!(
            ComparisonComputation::NotEqual(EqualityNotation::Symbol).name(),
            "not equal"
        );
        assert_eq!(
            ComparisonComputation::Equal(EqualityNotation::Word).name(),
            "is"
        );
        assert_eq!(
            ComparisonComputation::NotEqual(EqualityNotation::Word).name(),
            "is not"
        );
    }

    #[test]
    fn test_literal_value_to_type() {
        let one = Decimal::from_str("1").unwrap();

        assert_eq!(
            LiteralValue::Text("".to_string()).to_type(),
            LemmaType::Text
        );
        assert_eq!(LiteralValue::Number(one).to_type(), LemmaType::Number);
        assert_eq!(
            LiteralValue::Boolean(crate::BooleanValue::True).to_type(),
            LemmaType::Boolean
        );

        let dt = DateTimeValue {
            year: 2024,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            timezone: None,
        };
        assert_eq!(LiteralValue::Date(dt).to_type(), LemmaType::Date);
        assert_eq!(
            LiteralValue::Percentage(one).to_type(),
            LemmaType::Percentage
        );
        assert_eq!(
            LiteralValue::Regex("".to_string()).to_type(),
            LemmaType::Regex
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Mass(one, MassUnit::Kilogram)).to_type(),
            LemmaType::Mass
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Length(one, LengthUnit::Meter)).to_type(),
            LemmaType::Length
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Volume(one, VolumeUnit::Liter)).to_type(),
            LemmaType::Volume
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Duration(one, DurationUnit::Second)).to_type(),
            LemmaType::Duration
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Temperature(one, TemperatureUnit::Celsius)).to_type(),
            LemmaType::Temperature
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Power(one, PowerUnit::Watt)).to_type(),
            LemmaType::Power
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Force(one, ForceUnit::Newton)).to_type(),
            LemmaType::Force
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Pressure(one, PressureUnit::Pascal)).to_type(),
            LemmaType::Pressure
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Energy(one, EnergyUnit::Joule)).to_type(),
            LemmaType::Energy
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Frequency(one, FrequencyUnit::Hertz)).to_type(),
            LemmaType::Frequency
        );
        assert_eq!(
            LiteralValue::Unit(NumericUnit::Data(one, DataUnit::Byte)).to_type(),
            LemmaType::Data
        );
    }

    #[test]
    fn test_numeric_unit_value() {
        let ten = Decimal::from_str("10").unwrap();
        let twenty = Decimal::from_str("20").unwrap();

        assert_eq!(NumericUnit::Mass(ten, MassUnit::Kilogram).value(), ten);
        assert_eq!(
            NumericUnit::Length(twenty, LengthUnit::Meter).value(),
            twenty
        );
        assert_eq!(
            NumericUnit::Duration(twenty, DurationUnit::Second).value(),
            twenty
        );
    }

    #[test]
    fn test_numeric_unit_same_category() {
        let ten = Decimal::from_str("10").unwrap();
        let twenty = Decimal::from_str("20").unwrap();

        let kg = NumericUnit::Mass(ten, MassUnit::Kilogram);
        let lb = NumericUnit::Mass(twenty, MassUnit::Pound);
        let meter = NumericUnit::Length(ten, LengthUnit::Meter);

        assert!(kg.same_category(&lb), "Same mass units should match");
        assert!(
            !kg.same_category(&meter),
            "Different unit types should not match"
        );
    }

    #[test]
    fn test_numeric_unit_with_value() {
        let ten = Decimal::from_str("10").unwrap();
        let fifty = Decimal::from_str("50").unwrap();

        let original = NumericUnit::Mass(ten, MassUnit::Kilogram);
        let updated = original.with_value(fifty);

        assert_eq!(updated.value(), fifty);
        assert!(original.same_category(&updated));
        assert_eq!(format!("{}", updated), "50 kilogram");
    }

    #[test]
    fn test_arithmetic_operation_display() {
        assert_eq!(format!("{}", ArithmeticComputation::Add), "+");
        assert_eq!(format!("{}", ArithmeticComputation::Subtract), "-");
        assert_eq!(format!("{}", ArithmeticComputation::Multiply), "*");
        assert_eq!(format!("{}", ArithmeticComputation::Divide), "/");
        assert_eq!(format!("{}", ArithmeticComputation::Modulo), "%");
        assert_eq!(format!("{}", ArithmeticComputation::Power), "^");
    }

    #[test]
    fn test_comparison_operator_display() {
        assert_eq!(format!("{}", ComparisonComputation::GreaterThan), ">");
        assert_eq!(format!("{}", ComparisonComputation::LessThan), "<");
        assert_eq!(
            format!("{}", ComparisonComputation::GreaterThanOrEqual),
            ">="
        );
        assert_eq!(format!("{}", ComparisonComputation::LessThanOrEqual), "<=");
        assert_eq!(
            format!("{}", ComparisonComputation::Equal(EqualityNotation::Symbol)),
            "=="
        );
        assert_eq!(
            format!(
                "{}",
                ComparisonComputation::NotEqual(EqualityNotation::Symbol)
            ),
            "!="
        );
        assert_eq!(
            format!("{}", ComparisonComputation::Equal(EqualityNotation::Word)),
            "is"
        );
        assert_eq!(
            format!(
                "{}",
                ComparisonComputation::NotEqual(EqualityNotation::Word)
            ),
            "is not"
        );
    }

    #[test]
    fn test_unit_display_formats() {
        let one = Decimal::from_str("1").unwrap();

        assert_eq!(format!("{}", MassUnit::Kilogram), "kilogram");
        assert_eq!(format!("{}", MassUnit::Pound), "pound");
        assert_eq!(
            format!("{}", NumericUnit::Mass(one, MassUnit::Gram)),
            "1 gram"
        );

        assert_eq!(format!("{}", LengthUnit::Meter), "meter");
        assert_eq!(format!("{}", LengthUnit::Mile), "mile");

        assert_eq!(format!("{}", VolumeUnit::Liter), "liter");
        assert_eq!(format!("{}", VolumeUnit::Gallon), "gallon");

        assert_eq!(format!("{}", DurationUnit::Second), "second");
        assert_eq!(format!("{}", DurationUnit::Hour), "hour");

        assert_eq!(format!("{}", TemperatureUnit::Celsius), "celsius");
        assert_eq!(format!("{}", TemperatureUnit::Fahrenheit), "fahrenheit");

        assert_eq!(format!("{}", PowerUnit::Watt), "watt");
        assert_eq!(format!("{}", PowerUnit::Kilowatt), "kilowatt");

        assert_eq!(format!("{}", ForceUnit::Newton), "newton");
        assert_eq!(format!("{}", PressureUnit::Pascal), "pascal");
        assert_eq!(format!("{}", EnergyUnit::Joule), "joule");
        assert_eq!(format!("{}", FrequencyUnit::Hertz), "hertz");
        assert_eq!(format!("{}", DataUnit::Byte), "byte");
        assert_eq!(format!("{}", DataUnit::Gigabyte), "gigabyte");
    }

    #[test]
    fn test_money_unit_display() {}

    #[test]
    fn test_conversion_target_display() {
        assert_eq!(
            format!("{}", ConversionTarget::Mass(MassUnit::Kilogram)),
            "kilogram"
        );
        assert_eq!(
            format!("{}", ConversionTarget::Length(LengthUnit::Meter)),
            "meter"
        );
        assert_eq!(format!("{}", ConversionTarget::Percentage), "percentage");
    }

    #[test]
    fn test_lemma_type_display() {
        assert_eq!(format!("{}", LemmaType::Text), "text");
        assert_eq!(format!("{}", LemmaType::Number), "number");
        assert_eq!(format!("{}", LemmaType::Date), "date");
        assert_eq!(format!("{}", LemmaType::Boolean), "boolean");
        assert_eq!(format!("{}", LemmaType::Percentage), "percentage");
        assert_eq!(format!("{}", LemmaType::Mass), "mass");
    }

    #[test]
    fn test_literal_value_display_value() {
        let ten = Decimal::from_str("10").unwrap();

        assert_eq!(
            LiteralValue::Text("hello".to_string()).display_value(),
            "\"hello\""
        );
        assert_eq!(LiteralValue::Number(ten).display_value(), "10");
        assert_eq!(
            LiteralValue::Boolean(crate::BooleanValue::True).display_value(),
            "true"
        );
        assert_eq!(
            LiteralValue::Boolean(crate::BooleanValue::False).display_value(),
            "false"
        );
        assert_eq!(LiteralValue::Percentage(ten).display_value(), "10%");

        let time = TimeValue {
            hour: 14,
            minute: 30,
            second: 0,
            timezone: None,
        };
        let time_display = LiteralValue::Time(time).display_value();
        assert!(time_display.contains("14"));
        assert!(time_display.contains("30"));
    }

    #[test]
    fn test_literal_value_time_type() {
        let time = TimeValue {
            hour: 14,
            minute: 30,
            second: 0,
            timezone: None,
        };
        assert_eq!(LiteralValue::Time(time).to_type(), LemmaType::Date);
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
    fn test_all_unit_categories() {
        let v = Decimal::from_str("1").unwrap();

        let _ = NumericUnit::Mass(v, MassUnit::Kilogram);
        let _ = NumericUnit::Length(v, LengthUnit::Meter);
        let _ = NumericUnit::Volume(v, VolumeUnit::Liter);
        let _ = NumericUnit::Duration(v, DurationUnit::Second);
        let _ = NumericUnit::Temperature(v, TemperatureUnit::Celsius);
        let _ = NumericUnit::Power(v, PowerUnit::Watt);
        let _ = NumericUnit::Force(v, ForceUnit::Newton);
        let _ = NumericUnit::Pressure(v, PressureUnit::Pascal);
        let _ = NumericUnit::Energy(v, EnergyUnit::Joule);
        let _ = NumericUnit::Frequency(v, FrequencyUnit::Hertz);
        let _ = NumericUnit::Data(v, DataUnit::Byte);
    }

    #[test]
    fn test_negation_types() {
        let _ = NegationType::Not;
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

    #[test]
    fn test_percentage_arithmetic() {
        let code = r#"
doc pricing
fact discount = 25%
rule net_multiplier = 1 - discount
"#;

        let mut engine = crate::Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();

        let response = engine.evaluate("pricing", vec![], std::collections::HashMap::new()).unwrap();
        let result = response
            .results
            .get("net_multiplier")
            .unwrap()
            .result
            .value()
            .unwrap();

        match result {
            LiteralValue::Number(n) => {
                assert_eq!(n, &rust_decimal::Decimal::from_str("0.75").unwrap())
            }
            _ => panic!("Expected Number, got {:?}", result),
        }
    }

    #[test]
    fn test_mass_operations() {
        let code = r#"
doc shipping
fact weight = 10 kilograms
rule double_weight = weight * 2
rule is_heavy = weight > 5 kilograms
"#;

        let mut engine = crate::Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();

        let response = engine.evaluate("shipping", vec![], std::collections::HashMap::new()).unwrap();
        let result = response
            .results
            .get("double_weight")
            .unwrap()
            .result
            .value()
            .unwrap();

        match result {
            LiteralValue::Unit(NumericUnit::Mass(amount, unit)) => {
                assert_eq!(amount, &rust_decimal::Decimal::from_str("20").unwrap());
                assert_eq!(*unit, MassUnit::Kilogram);
            }
            _ => panic!("Expected Mass, got {:?}", result),
        }

        let is_heavy = response.results.get("is_heavy").unwrap();
        assert_eq!(
            is_heavy.result,
            crate::OperationResult::Value(crate::LiteralValue::Boolean(
                crate::BooleanValue::True
            ))
        );
    }

    #[test]
    fn test_consistent_number_types() {
        let code = r#"
doc test
fact x = 10
fact condition = true

rule result = 5
    unless condition then 10
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_consistent_text_types() {
        let code = r#"
doc test
fact condition = true

rule status = "pending"
    unless condition then "approved"
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_consistent_boolean_types() {
        let code = r#"
doc test
fact x = 10
fact y = 20

rule check = x > 5
    unless y > 15 then y < 25
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_mixed_number_and_text_rejected() {
        let code = r#"
doc test
fact condition = true

rule result = 100
    unless condition then "text"
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("type")
                || err.to_string().contains("incompatible")
                || err.to_string().contains("Type mismatch")
        );
    }

    #[test]
    fn test_mixed_text_and_boolean_rejected() {
        let code = r#"
doc test
fact condition = true

rule result = "text"
    unless condition then true
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("type")
                || err.to_string().contains("incompatible")
                || err.to_string().contains("Type mismatch")
        );
    }

    #[test]
    fn test_mixed_number_and_boolean_rejected() {
        let code = r#"
doc test
fact condition = true

rule result = 42
    unless condition then false
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("type")
                || err.to_string().contains("incompatible")
                || err.to_string().contains("Type mismatch")
        );
    }

    #[test]
    fn test_multiple_unless_clauses_consistent() {
        let code = r#"
doc test
fact a = true
fact b = false

rule result = 1
    unless a then 2
    unless b then 3
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_unless_clauses_inconsistent() {
        let code = r#"
doc test
fact a = true
fact b = false

rule result = 1
    unless a then 2
    unless b then "three"
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("type")
                || err.to_string().contains("incompatible")
                || err.to_string().contains("Type mismatch")
        );
    }

    #[test]
    fn test_veto_with_consistent_types() {
        let code = r#"
doc test
fact blocked = true
fact condition = false

rule result = 10
    unless blocked then veto "blocked"
    unless condition then 20
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_veto_with_mixed_types() {
        let code = r#"
doc test
fact blocked = true
fact condition = false

rule result = 10
    unless blocked then veto "blocked"
    unless condition then "text"
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("type")
                || err.to_string().contains("incompatible")
                || err.to_string().contains("Type mismatch")
        );
    }

    #[test]
    fn test_all_veto_clauses_allowed() {
        let code = r#"
doc test
fact a = true
fact b = false

rule result = 10
    unless a then veto "a"
    unless b then veto "b"
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_consistent_length_types() {
        let code = r#"
doc test
fact condition = true

rule distance = 100 meters
    unless condition then 200 meters
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_mixed_length_and_number_rejected() {
        let code = r#"
doc test
fact condition = true

rule distance = 100 meters
    unless condition then 200
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("type")
                || err.to_string().contains("incompatible")
                || err.to_string().contains("Type mismatch")
        );
    }

    #[test]
    fn test_consistent_mass_types() {
        let code = r#"
doc test
fact heavy = true

rule weight = 10 kilograms
    unless heavy then 20 kilograms
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_mixed_mass_and_number_rejected() {
        let code = r#"
doc test
fact heavy = true

rule weight = 10 kilograms
    unless heavy then 20
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("type")
                || err.to_string().contains("incompatible")
                || err.to_string().contains("Type mismatch")
        );
    }

    #[test]
    fn test_complex_expression_consistent_types() {
        let code = r#"
doc test
fact x = 10
fact y = 20
fact condition = true

rule result = x + y
    unless condition then x * 2
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_comparison_expression_consistent_types() {
        let code = r#"
doc test
fact x = 10
fact condition = true

rule check = x > 5
    unless condition then x < 20
"#;

        let mut engine = crate::Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }
}
