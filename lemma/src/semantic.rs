use crate::error::LemmaError;
use crate::parsing::ast::ExpressionId;
use crate::parsing::source::Source;
use chrono::{Datelike, Timelike};
use rust_decimal::Decimal;
use serde::Serialize;
use std::fmt;
use std::str::FromStr;

/// A Lemma document containing facts, rules
#[derive(Debug, Clone, PartialEq)]
pub struct LemmaDoc {
    pub name: String,
    pub source: Option<String>,
    pub start_line: usize,
    pub commentary: Option<String>,
    pub facts: Vec<LemmaFact>,
    pub rules: Vec<LemmaRule>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LemmaFact {
    pub reference: FactReference,
    pub value: FactValue,
    pub source_location: Option<Source>,
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
    pub source_location: Option<Source>,
}

/// A rule with a single expression and optional unless clauses
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LemmaRule {
    pub name: String,
    pub expression: Expression,
    pub unless_clauses: Vec<UnlessClause>,
    pub source_location: Option<Source>,
}

/// An expression that can be evaluated, with source location and unique ID
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub source_location: Option<Source>,
    pub id: ExpressionId,
}

impl Expression {
    /// Create a new expression with kind, source location, and ID
    #[must_use]
    pub fn new(kind: ExpressionKind, source_location: Option<Source>, id: ExpressionId) -> Self {
        Self {
            kind,
            source_location,
            id,
        }
    }

    /// Get the source text for this expression from the given sources map
    ///
    /// Returns `None` if the expression has no source location or the source is not found.
    pub fn get_source_text(
        &self,
        sources: &std::collections::HashMap<String, String>,
    ) -> Option<String> {
        self.source_location.as_ref().and_then(|loc| {
            sources
                .get(&loc.source_id)
                .and_then(|source| loc.extract_text(source))
        })
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
}

/// The kind/type of expression
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct RuleReference {
    pub segments: Vec<String>,
    pub rule: String,
}

/// A single segment in a path traversal
///
/// Used in both FactPath and RulePath to represent document traversal.
/// Each segment contains a fact name that points to a document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
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

/// Comparison computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
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
    /// Returns a human-readable name for the computation
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            ComparisonComputation::GreaterThan => "greater than",
            ComparisonComputation::LessThan => "less than",
            ComparisonComputation::GreaterThanOrEqual => "greater than or equal",
            ComparisonComputation::LessThanOrEqual => "less than or equal",
            ComparisonComputation::Equal => "equal",
            ComparisonComputation::NotEqual => "not equal",
            ComparisonComputation::Is => "is",
            ComparisonComputation::IsNot => "is not",
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
            ComparisonComputation::Equal => "==",
            ComparisonComputation::NotEqual => "!=",
            ComparisonComputation::Is => "is",
            ComparisonComputation::IsNot => "is not",
        }
    }
}

/// The target unit for unit conversion expressions
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VetoExpression {
    pub message: Option<String>,
}

/// Mathematical computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
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

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum FactValue {
    Literal(LiteralValue),
    DocumentReference(String),
    TypeAnnotation(TypeAnnotation),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TypeAnnotation {
    LemmaType(LemmaType),
}

/// A type for type annotations (both literal types and document types)
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, strum_macros::EnumString, strum_macros::Display)]
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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

/// A time value
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct TimeValue {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub timezone: Option<TimezoneValue>,
}

/// A timezone value
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimezoneValue {
    pub offset_hours: i8,
    pub offset_minutes: u8,
}

/// A datetime value that preserves timezone information
#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum MassUnit {
    Kilogram,
    Gram,
    Milligram,
    Ton,
    Pound,
    Ounce,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
    Kelvin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum PowerUnit {
    Megawatt,
    Kilowatt,
    Watt,
    Milliwatt,
    Horsepower,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ForceUnit {
    Newton,
    Kilonewton,
    Lbf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum FrequencyUnit {
    Hertz,
    Kilohertz,
    Megahertz,
    Gigahertz,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum_macros::Display, strum_macros::EnumString)]
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
            let fact = path.last().unwrap().clone();
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
            source_location: None,
        }
    }

    #[must_use]
    pub fn with_source_location(mut self, source_location: Source) -> Self {
        self.source_location = Some(source_location);
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
            source: None,
            start_line: 1,
            commentary: None,
            facts: Vec::new(),
            rules: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
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
