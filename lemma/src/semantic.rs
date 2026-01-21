use crate::error::LemmaError;
use crate::parsing::ast::Span;
use crate::parsing::source::Source;
use chrono::{Datelike, Timelike};
use rust_decimal::Decimal;
use serde::Serialize;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

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

/// An expression that can be evaluated, with source location
///
/// Expressions use semantic equality and hashing - two expressions with the same
/// structure (kind) are equal/hash-equal regardless of source location.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub source_location: Option<Source>,
}

impl Expression {
    /// Create a new expression with kind and source location
    #[must_use]
    pub fn new(kind: ExpressionKind, source_location: Option<Source>) -> Self {
        Self {
            kind,
            source_location,
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
                .get(&loc.attribute)
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
            | ExpressionKind::Reference(_)
            | ExpressionKind::UnresolvedUnitLiteral(_, _)
            | ExpressionKind::FactReference(_)
            | ExpressionKind::RuleReference(_)
            | ExpressionKind::Veto(_)
            | ExpressionKind::RulePath(_) => {}
        }
    }

    /// Compute semantic hash - hashes the expression structure, ignoring source location
    fn semantic_hash<H: Hasher>(&self, state: &mut H) {
        self.kind.semantic_hash(state);
    }
}

/// Semantic equality - compares expressions by structure only, ignoring source location
impl PartialEq for Expression {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Eq for Expression {}

/// Semantic hashing - hashes expression structure only, ignoring source location
impl Hash for Expression {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.semantic_hash(state);
    }
}

/// The kind/type of expression
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExpressionKind {
    Literal(LiteralValue),
    /// Unresolved reference from parser (resolved during planning)
    Reference(Reference),
    /// Unresolved unit literal from parser (resolved during planning)
    /// Contains (number, unit_name) - the unit name will be resolved to its type during semantic analysis
    UnresolvedUnitLiteral(Decimal, String),
    /// Resolved fact reference (converted from Reference during planning)
    FactReference(FactReference),
    RuleReference(RuleReference),
    LogicalAnd(Arc<Expression>, Arc<Expression>),
    LogicalOr(Arc<Expression>, Arc<Expression>),
    Arithmetic(Arc<Expression>, ArithmeticComputation, Arc<Expression>),
    Comparison(Arc<Expression>, ComparisonComputation, Arc<Expression>),
    UnitConversion(Arc<Expression>, ConversionTarget),
    LogicalNegation(Arc<Expression>, NegationType),
    MathematicalComputation(MathematicalComputation, Arc<Expression>),
    Veto(VetoExpression),
    /// Resolved fact path (used after planning, converted from FactReference)
    FactPath(FactPath),
    /// Resolved rule path (used after planning, converted from RuleReference)
    RulePath(RulePath),
}

impl ExpressionKind {
    /// Compute semantic hash for expression kinds
    fn semantic_hash<H: Hasher>(&self, state: &mut H) {
        // Hash discriminant first
        std::mem::discriminant(self).hash(state);

        match self {
            ExpressionKind::Literal(lit) => lit.semantic_hash(state),
            ExpressionKind::Reference(r) => r.hash(state),
            ExpressionKind::UnresolvedUnitLiteral(_, _) => {
                // UnresolvedUnitLiteral should never be hashed - it should be resolved during planning
                panic!("UnresolvedUnitLiteral found during hashing - this indicates a bug: unresolved units should be resolved during planning");
            }
            ExpressionKind::FactReference(fr) => fr.hash(state),
            ExpressionKind::RuleReference(rr) => rr.hash(state),
            ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
                left.semantic_hash(state);
                right.semantic_hash(state);
            }
            ExpressionKind::Arithmetic(left, op, right) => {
                left.semantic_hash(state);
                op.hash(state);
                right.semantic_hash(state);
            }
            ExpressionKind::Comparison(left, op, right) => {
                left.semantic_hash(state);
                op.hash(state);
                right.semantic_hash(state);
            }
            ExpressionKind::UnitConversion(expr, target) => {
                expr.semantic_hash(state);
                target.hash(state);
            }
            ExpressionKind::LogicalNegation(expr, neg_type) => {
                expr.semantic_hash(state);
                neg_type.hash(state);
            }
            ExpressionKind::MathematicalComputation(op, expr) => {
                op.hash(state);
                expr.semantic_hash(state);
            }
            ExpressionKind::Veto(veto) => veto.semantic_hash(state),
            ExpressionKind::FactPath(fp) => fp.hash(state),
            ExpressionKind::RulePath(rp) => rp.hash(state),
        }
    }
}

/// Unresolved reference from parser
///
/// During parsing, identifiers are captured as References.
/// During planning, they are resolved to FactReference.
/// Examples:
/// - Local reference "age": segments=[], name="age"
/// - Cross-document "employee.salary": segments=["employee"], name="salary"
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Reference {
    pub segments: Vec<String>,
    pub name: String,
}

/// Reference to a fact (resolved from Reference during planning)
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

/// Comparison computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
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

/// The target unit for unit conversion expressions
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConversionTarget {
    Duration(DurationUnit),
    Percentage,
}

/// Types of logical negation
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NegationType {
    Not,
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

impl VetoExpression {
    fn semantic_hash<H: Hasher>(&self, state: &mut H) {
        self.message.hash(state);
    }
}

/// Mathematical computations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FactValue {
    Literal(LiteralValue),
    DocumentReference(String),
    TypeDeclaration {
        base: String,
        overrides: Option<Vec<(String, Vec<String>)>>,
        from: Option<String>,
    },
}

/// A type for type declarations
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

/// The actual value data (without type information)
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub enum Value {
    Number(Decimal),
    Scale(Decimal, Option<String>), // value, optional unit name (e.g., "eur", "usd", "kilogram")
    Text(String),
    Date(DateTimeValue),
    Time(TimeValue),
    Boolean(BooleanValue),
    Duration(Decimal, DurationUnit),
    Ratio(Decimal, Option<String>), // value, optional unit name (e.g., "percent", "permille")
}

/// A literal value with its type
///
/// Every literal value knows its type - no distinction between standard and custom types.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct LiteralValue {
    pub value: Value,
    pub lemma_type: LemmaType,
}

impl LiteralValue {
    /// Create a Number literal value from any type that can convert to Decimal
    /// Uses STANDARD_NUMBER as the type
    pub fn number<T: Into<Decimal>>(value: T) -> Self {
        LiteralValue {
            value: Value::Number(value.into()),
            lemma_type: standard_number().clone(),
        }
    }

    /// Create a Number literal value with a custom type
    pub fn number_with_type<T: Into<Decimal>>(value: T, lemma_type: LemmaType) -> Self {
        LiteralValue {
            value: Value::Number(value.into()),
            lemma_type,
        }
    }

    /// Create a Scale literal value
    /// Uses the provided type (must be a Scale type)
    pub fn scale<T: Into<Decimal>>(value: T, unit: Option<String>) -> Self {
        LiteralValue {
            value: Value::Scale(value.into(), unit),
            lemma_type: crate::semantic::standard_scale().clone(),
        }
    }

    /// Create a Scale literal value with a custom type
    pub fn scale_with_type<T: Into<Decimal>>(
        value: T,
        unit: Option<String>,
        lemma_type: LemmaType,
    ) -> Self {
        LiteralValue {
            value: Value::Scale(value.into(), unit),
            lemma_type,
        }
    }

    /// Create a Text literal value
    /// Uses STANDARD_TEXT as the type
    pub fn text(value: String) -> Self {
        LiteralValue {
            value: Value::Text(value),
            lemma_type: standard_text().clone(),
        }
    }

    /// Create a Text literal value with a custom type
    pub fn text_with_type(value: String, lemma_type: LemmaType) -> Self {
        LiteralValue {
            value: Value::Text(value),
            lemma_type,
        }
    }

    /// Create a Boolean literal value
    /// Uses STANDARD_BOOLEAN as the type
    pub fn boolean(value: BooleanValue) -> Self {
        let canonical: BooleanValue = bool::from(&value).into();
        LiteralValue {
            value: Value::Boolean(canonical),
            lemma_type: standard_boolean().clone(),
        }
    }

    /// Create a Boolean literal value with a custom type
    pub fn boolean_with_type(value: BooleanValue, lemma_type: LemmaType) -> Self {
        let canonical: BooleanValue = bool::from(&value).into();
        LiteralValue {
            value: Value::Boolean(canonical),
            lemma_type,
        }
    }

    /// Create a Date literal value
    /// Uses STANDARD_DATE as the type
    pub fn date(value: DateTimeValue) -> Self {
        LiteralValue {
            value: Value::Date(value),
            lemma_type: standard_date().clone(),
        }
    }

    /// Create a Date literal value with a custom type
    pub fn date_with_type(value: DateTimeValue, lemma_type: LemmaType) -> Self {
        LiteralValue {
            value: Value::Date(value),
            lemma_type,
        }
    }

    /// Create a Time literal value
    /// Uses STANDARD_TIME as the type
    pub fn time(value: TimeValue) -> Self {
        LiteralValue {
            value: Value::Time(value),
            lemma_type: standard_time().clone(),
        }
    }

    /// Create a Time literal value with a custom type
    pub fn time_with_type(value: TimeValue, lemma_type: LemmaType) -> Self {
        LiteralValue {
            value: Value::Time(value),
            lemma_type,
        }
    }

    /// Create a Duration literal value
    /// Uses STANDARD_DURATION as the type
    pub fn duration(value: Decimal, unit: DurationUnit) -> Self {
        LiteralValue {
            value: Value::Duration(value, unit),
            lemma_type: standard_duration().clone(),
        }
    }

    /// Create a Duration literal value with a custom type
    pub fn duration_with_type(value: Decimal, unit: DurationUnit, lemma_type: LemmaType) -> Self {
        LiteralValue {
            value: Value::Duration(value, unit),
            lemma_type,
        }
    }

    /// Create a Ratio literal value
    /// Uses STANDARD_RATIO as the type
    pub fn ratio<T: Into<Decimal>>(value: T, unit: Option<String>) -> Self {
        LiteralValue {
            value: Value::Ratio(value.into(), unit),
            lemma_type: standard_ratio().clone(),
        }
    }

    /// Create a Ratio literal value with a custom type
    pub fn ratio_with_type<T: Into<Decimal>>(
        value: T,
        unit: Option<String>,
        lemma_type: LemmaType,
    ) -> Self {
        LiteralValue {
            value: Value::Ratio(value.into(), unit),
            lemma_type,
        }
    }

    /// Get the type of this literal value
    pub fn get_type(&self) -> &LemmaType {
        &self.lemma_type
    }

    /// Get the display value as a string (uses the Display implementation)
    #[must_use]
    pub fn display_value(&self) -> String {
        self.to_string()
    }

    /// Get the byte size of this literal value for resource limiting
    pub fn byte_size(&self) -> usize {
        match &self.value {
            Value::Text(s) => s.len(),
            Value::Number(d) => std::mem::size_of_val(d),
            Value::Scale(d, _) => std::mem::size_of_val(d),
            Value::Boolean(_) => std::mem::size_of::<bool>(),
            Value::Date(_) => std::mem::size_of::<DateTimeValue>(),
            Value::Time(_) => std::mem::size_of::<TimeValue>(),
            Value::Duration(value, _) => std::mem::size_of_val(value),
            Value::Ratio(value, _) => std::mem::size_of_val(value),
        }
    }

    /// Compute semantic hash for literal values
    /// Uses string representation for Decimal to avoid Hash trait requirement
    fn semantic_hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(&self.value).hash(state);
        match &self.value {
            Value::Number(d) | Value::Scale(d, _) | Value::Ratio(d, _) => {
                d.to_string().hash(state);
            }
            Value::Text(s) => s.hash(state),
            Value::Boolean(b) => std::mem::discriminant(b).hash(state),
            Value::Date(dt) => {
                dt.year.hash(state);
                dt.month.hash(state);
                dt.day.hash(state);
                dt.hour.hash(state);
                dt.minute.hash(state);
                dt.second.hash(state);
                if let Some(tz) = &dt.timezone {
                    tz.offset_hours.hash(state);
                    tz.offset_minutes.hash(state);
                }
            }
            Value::Time(t) => {
                t.hour.hash(state);
                t.minute.hash(state);
                t.second.hash(state);
                if let Some(tz) = &t.timezone {
                    tz.offset_hours.hash(state);
                    tz.offset_minutes.hash(state);
                }
            }
            Value::Duration(value, unit) => {
                value.to_string().hash(state);
                std::mem::discriminant(unit).hash(state);
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

impl Reference {
    #[must_use]
    pub fn new(segments: Vec<String>, name: String) -> Self {
        Self { segments, name }
    }

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

    /// Convert to FactReference (used during planning resolution)
    #[must_use]
    pub fn to_fact_reference(&self) -> FactReference {
        FactReference {
            segments: self.segments.clone(),
            fact: self.name.clone(),
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
            // Safe: path is non-empty.
            let fact = path[path.len() - 1].clone();
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
                FactValue::Literal(lit) => Some(lit.get_type().clone()),
                FactValue::TypeDeclaration { .. } => {
                    // Type resolution happens during planning phase
                    None
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
            ExpressionKind::Reference(r) => {
                if r.segments.is_empty() {
                    write!(f, "{}", r.name)
                } else {
                    write!(f, "{}.{}", r.segments.join("."), r.name)
                }
            }
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
            ExpressionKind::UnresolvedUnitLiteral(number, unit_name) => {
                write!(f, "{} {}", number, unit_name)
            }
        }
    }
}

impl fmt::Display for LiteralValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            Value::Number(n) => {
                // Get decimals from type specification if available
                let decimals_opt = match &self.lemma_type.specifications {
                    TypeSpecification::Number { decimals, .. } => *decimals,
                    _ => None,
                };

                if let Some(decimals) = decimals_opt {
                    // Format with fixed decimal places, always showing all decimals
                    let rounded = n.round_dp(decimals as u32);
                    let mut s = rounded.to_string();
                    // Ensure we have the right number of decimal places
                    if let Some(dot_pos) = s.find('.') {
                        let current_decimals = s.len() - dot_pos - 1;
                        if current_decimals < decimals as usize {
                            // Pad with zeros
                            s.push_str(&"0".repeat(decimals as usize - current_decimals));
                        } else if current_decimals > decimals as usize {
                            // This shouldn't happen due to round_dp, but handle it
                            let truncate_pos = dot_pos + 1 + decimals as usize;
                            s = s[..truncate_pos].to_string();
                        }
                    } else {
                        // No decimal point, add it with zeros
                        s.push('.');
                        s.push_str(&"0".repeat(decimals as usize));
                    }
                    write!(f, "{}", s)
                } else {
                    // No decimals specified: normalize (remove trailing zeros)
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
            }
            Value::Text(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                write!(f, "\"{}\"", escaped)
            }
            Value::Date(dt) => write!(f, "{}", dt),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Time(time) => {
                write!(f, "time({}, {}, {})", time.hour, time.minute, time.second)
            }
            Value::Scale(n, unit_opt) => {
                // Format the number part (same as Number)
                let decimals_opt = match &self.lemma_type.specifications {
                    TypeSpecification::Scale { decimals, .. } => *decimals,
                    _ => None,
                };

                let number_str = if let Some(decimals) = decimals_opt {
                    // Format with fixed decimal places
                    let rounded = n.round_dp(decimals as u32);
                    let mut s = rounded.to_string();
                    if let Some(dot_pos) = s.find('.') {
                        let current_decimals = s.len() - dot_pos - 1;
                        if current_decimals < decimals as usize {
                            s.push_str(&"0".repeat(decimals as usize - current_decimals));
                        }
                    } else {
                        s.push('.');
                        s.push_str(&"0".repeat(decimals as usize));
                    }
                    s
                } else {
                    // No decimals specified: normalize (remove trailing zeros)
                    let normalized = n.normalize();
                    if normalized.fract().is_zero() {
                        normalized.trunc().to_string()
                    } else {
                        normalized.to_string()
                    }
                };

                // Append unit if present
                if let Some(unit) = unit_opt {
                    write!(f, "{} {}", number_str, unit)
                } else {
                    write!(f, "{}", number_str)
                }
            }
            Value::Duration(value, unit) => write!(f, "{} {}", value, unit),
            Value::Ratio(r, unit_opt) => {
                // Use tracked unit if present
                if let Some(unit) = unit_opt {
                    if unit == "percent" {
                        // Display as percent: convert ratio (0.50) to percent (50%)
                        let percentage_value = *r * rust_decimal::Decimal::from(100);
                        let rounded = percentage_value.round_dp(2);
                        if rounded.fract().is_zero() {
                            write!(f, "{}%", rounded.trunc())
                        } else {
                            write!(f, "{}%", rounded)
                        }
                    } else {
                        // Display with unit
                        let normalized = r.normalize();
                        if normalized.fract().is_zero() {
                            write!(f, "{} {}", normalized.trunc(), unit)
                        } else {
                            write!(f, "{} {}", normalized, unit)
                        }
                    }
                } else {
                    // Display as regular ratio (no unit)
                    let normalized = r.normalize();
                    if normalized.fract().is_zero() {
                        write!(f, "{}", normalized.trunc())
                    } else {
                        write!(f, "{}", normalized)
                    }
                }
            }
        }
    }
}

impl fmt::Display for ConversionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConversionTarget::Duration(unit) => write!(f, "{}", unit),
            ConversionTarget::Percentage => write!(f, "percent"),
        }
    }
}

impl fmt::Display for FactValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FactValue::Literal(lit) => write!(f, "{}", lit),
            FactValue::TypeDeclaration {
                base,
                overrides,
                from,
            } => {
                let base_str = if let Some(from_doc) = from {
                    format!("{} from {}", base, from_doc)
                } else {
                    base.clone()
                };

                if let Some(ref overrides_vec) = overrides {
                    let override_str = overrides_vec
                        .iter()
                        .map(|(cmd, args)| {
                            let args_str = args.join(" ");
                            if args_str.is_empty() {
                                cmd.clone()
                            } else {
                                format!("{} {}", cmd, args_str)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" -> ");
                    write!(f, "[{} -> {}]", base_str, override_str)
                } else {
                    write!(f, "[{}]", base_str)
                }
            }
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

/// A unit for Number and Ratio types
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub struct Unit {
    pub name: String,
    pub value: Decimal,
}

/// Type specifications that define the foundational types available in Lemma,
/// including their default values and constraints.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub enum TypeSpecification {
    Boolean {
        help: Option<String>,
        default: Option<bool>,
    },
    Scale {
        minimum: Option<Decimal>,
        maximum: Option<Decimal>,
        decimals: Option<u8>,
        precision: Option<Decimal>,
        units: Vec<Unit>,
        help: Option<String>,
        default: Option<(Decimal, String)>,
    },
    Number {
        minimum: Option<Decimal>,
        maximum: Option<Decimal>,
        decimals: Option<u8>,
        precision: Option<Decimal>,
        help: Option<String>,
        default: Option<Decimal>,
    },
    Ratio {
        minimum: Option<Decimal>,
        maximum: Option<Decimal>,
        units: Vec<Unit>,
        help: Option<String>,
        default: Option<Decimal>,
    },
    Text {
        minimum: Option<usize>,
        maximum: Option<usize>,
        length: Option<usize>,
        options: Vec<String>,
        help: Option<String>,
        default: Option<String>,
    },
    Date {
        minimum: Option<DateTimeValue>,
        maximum: Option<DateTimeValue>,
        help: Option<String>,
        default: Option<DateTimeValue>,
    },
    Time {
        minimum: Option<TimeValue>,
        maximum: Option<TimeValue>,
        help: Option<String>,
        default: Option<TimeValue>,
    },
    Duration {
        help: Option<String>,
        default: Option<(Decimal, DurationUnit)>,
    },
    Veto {
        message: Option<String>,
    },
}

impl TypeSpecification {
    /// Create a Boolean type with no defaults
    pub fn boolean() -> Self {
        TypeSpecification::Boolean {
            help: None,
            default: None,
        }
    }

    /// Create a Scale type with default specifications (can have units)
    pub fn scale() -> Self {
        TypeSpecification::Scale {
            minimum: None,
            maximum: None,
            decimals: None,
            precision: None,
            units: vec![],
            help: None,
            default: None,
        }
    }

    /// Create a Number type with default specifications (dimensionless, no units)
    pub fn number() -> Self {
        TypeSpecification::Number {
            minimum: None,
            maximum: None,
            decimals: None,
            precision: None,
            help: None,
            default: None,
        }
    }

    /// Create a Ratio type with default units
    /// Default units: percent = 100, permille = 1000
    pub fn ratio() -> Self {
        TypeSpecification::Ratio {
            minimum: None,
            maximum: None,
            units: vec![
                Unit {
                    name: "percent".to_string(),
                    value: Decimal::from(100),
                },
                Unit {
                    name: "permille".to_string(),
                    value: Decimal::from(1000),
                },
            ],
            help: None,
            default: None,
        }
    }

    /// Create a Text type with default specifications
    pub fn text() -> Self {
        TypeSpecification::Text {
            minimum: None,
            maximum: None,
            length: None,
            options: vec![],
            help: None,
            default: None,
        }
    }

    /// Create a Date type with default specifications
    pub fn date() -> Self {
        TypeSpecification::Date {
            minimum: None,
            maximum: None,
            help: None,
            default: None,
        }
    }

    /// Create a Time type with default specifications
    pub fn time() -> Self {
        TypeSpecification::Time {
            minimum: None,
            maximum: None,
            help: None,
            default: None,
        }
    }

    /// Create a Duration type with default specifications
    pub fn duration() -> Self {
        TypeSpecification::Duration {
            help: None,
            default: None,
        }
    }

    /// Create a Veto type (internal use only - not user-declarable)
    pub fn veto() -> Self {
        TypeSpecification::Veto { message: None }
    }

    /// Apply a single override command to this specification
    pub fn apply_override(mut self, command: &str, args: &[String]) -> Result<Self, String> {
        match &mut self {
            TypeSpecification::Boolean { help, default } => match command {
                "help" => *help = args.first().cloned(),
                "default" => {
                    let d = args
                        .first()
                        .ok_or_else(|| "default requires an argument".to_string())?
                        .parse::<BooleanValue>()
                        .map_err(|_| format!("invalid default value: {:?}", args.first()))?;
                    *default = Some(d.into());
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for boolean type. Valid commands: help, default",
                        command
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
                default,
            } => match command {
                "decimals" => {
                    let d = args
                        .first()
                        .ok_or_else(|| "decimals requires an argument".to_string())?
                        .parse::<u8>()
                        .map_err(|_| format!("invalid decimals value: {:?}", args.first()))?;
                    *decimals = Some(d);
                }
                "unit" if args.len() >= 2 => {
                    let unit_name = args[0].clone();
                    // Check if unit name already exists in the current type
                    if units.iter().any(|u| u.name == unit_name) {
                        return Err(format!(
                            "Duplicate unit name '{}' in type definition. Unit names must be unique within a type.",
                            unit_name
                        ));
                    }
                    let value = args[1]
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid unit value: {}", args[1]))?;
                    units.push(Unit {
                        name: unit_name,
                        value,
                    });
                }
                "minimum" => {
                    let m = args
                        .first()
                        .ok_or_else(|| "minimum requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid minimum value: {:?}", args.first()))?;
                    *minimum = Some(m);
                }
                "maximum" => {
                    let m = args
                        .first()
                        .ok_or_else(|| "maximum requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid maximum value: {:?}", args.first()))?;
                    *maximum = Some(m);
                }
                "precision" => {
                    let p = args
                        .first()
                        .ok_or_else(|| "precision requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid precision value: {:?}", args.first()))?;
                    *precision = Some(p);
                }
                "help" => *help = args.first().cloned(),
                "default" => {
                    if args.len() < 2 {
                        return Err(
                            "default requires a value and unit (e.g., 'default 1 kilogram')"
                                .to_string(),
                        );
                    }
                    let value = args[0]
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid default value: {:?}", args[0]))?;
                    let unit_name = args[1].clone();
                    // Validate that the unit exists
                    if !units.iter().any(|u| u.name == unit_name) {
                        return Err(format!(
                            "Invalid unit '{}' for default. Valid units: {}",
                            unit_name,
                            units
                                .iter()
                                .map(|u| u.name.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    *default = Some((value, unit_name));
                }
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
                default,
            } => match command {
                "decimals" => {
                    let d = args
                        .first()
                        .ok_or_else(|| "decimals requires an argument".to_string())?
                        .parse::<u8>()
                        .map_err(|_| format!("invalid decimals value: {:?}", args.first()))?;
                    *decimals = Some(d);
                }
                "unit" => {
                    return Err(
                        "Invalid command 'unit' for number type. Number types are dimensionless and cannot have units. Use 'scale' type instead.".to_string()
                    );
                }
                "minimum" => {
                    let m = args
                        .first()
                        .ok_or_else(|| "minimum requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid minimum value: {:?}", args.first()))?;
                    *minimum = Some(m);
                }
                "maximum" => {
                    let m = args
                        .first()
                        .ok_or_else(|| "maximum requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid maximum value: {:?}", args.first()))?;
                    *maximum = Some(m);
                }
                "precision" => {
                    let p = args
                        .first()
                        .ok_or_else(|| "precision requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid precision value: {:?}", args.first()))?;
                    *precision = Some(p);
                }
                "help" => *help = args.first().cloned(),
                "default" => {
                    let d = args
                        .first()
                        .ok_or_else(|| "default requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid default value: {:?}", args.first()))?;
                    *default = Some(d);
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for number type. Valid commands: minimum, maximum, decimals, precision, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Ratio {
                minimum,
                maximum,
                units,
                help,
                default,
            } => match command {
                "unit" if args.len() >= 2 => {
                    let value = args[1]
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid unit value: {}", args[1]))?;
                    units.push(Unit {
                        name: args[0].clone(),
                        value,
                    });
                }
                "minimum" => {
                    let m = args
                        .first()
                        .ok_or_else(|| "minimum requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid minimum value: {:?}", args.first()))?;
                    *minimum = Some(m);
                }
                "maximum" => {
                    let m = args
                        .first()
                        .ok_or_else(|| "maximum requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid maximum value: {:?}", args.first()))?;
                    *maximum = Some(m);
                }
                "help" => *help = args.first().cloned(),
                "default" => {
                    let d = args
                        .first()
                        .ok_or_else(|| "default requires an argument".to_string())?
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid default value: {:?}", args.first()))?;
                    *default = Some(d);
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for ratio type. Valid commands: unit, minimum, maximum, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Text {
                minimum,
                maximum,
                length,
                options,
                help,
                default,
            } => match command {
                "option" if args.len() == 1 => {
                    options.push(strip_surrounding_quotes(&args[0]));
                }
                "options" => {
                    *options = args.iter().map(|s| strip_surrounding_quotes(s)).collect();
                }
                "minimum" => {
                    let m = args
                        .first()
                        .ok_or_else(|| "minimum requires an argument".to_string())?
                        .parse::<usize>()
                        .map_err(|_| format!("invalid minimum value: {:?}", args.first()))?;
                    *minimum = Some(m);
                }
                "maximum" => {
                    let m = args
                        .first()
                        .ok_or_else(|| "maximum requires an argument".to_string())?
                        .parse::<usize>()
                        .map_err(|_| format!("invalid maximum value: {:?}", args.first()))?;
                    *maximum = Some(m);
                }
                "length" => {
                    let l = args
                        .first()
                        .ok_or_else(|| "length requires an argument".to_string())?
                        .parse::<usize>()
                        .map_err(|_| format!("invalid length value: {:?}", args.first()))?;
                    *length = Some(l);
                }
                "help" => *help = args.first().cloned(),
                "default" => {
                    let arg = args
                        .first()
                        .ok_or_else(|| "default requires an argument".to_string())?;
                    *default = Some(strip_surrounding_quotes(arg));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for text type. Valid commands: options, minimum, maximum, length, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Date {
                minimum,
                maximum,
                help,
                default,
            } => match command {
                "minimum" => {
                    let arg = args
                        .first()
                        .ok_or_else(|| "default requires an argument".to_string())?;
                    let Value::Date(date) = &standard_date()
                        .parse_value(arg)
                        .map_err(|_| format!("invalid default date value: {}", arg))?
                        .value
                    else {
                        return Err(format!("invalid default date value: {}", arg));
                    };
                    *minimum = Some(date.clone());
                }
                "maximum" => {
                    let arg = args
                        .first()
                        .ok_or_else(|| "default requires an argument".to_string())?;
                    let Value::Date(date) = standard_date()
                        .parse_value(arg)
                        .map_err(|_| format!("invalid default date value: {}", arg))?
                        .value
                    else {
                        return Err(format!("invalid default date value: {}", arg));
                    };
                    *maximum = Some(date);
                }
                "help" => *help = args.first().cloned(),
                "default" => {
                    let arg = args
                        .first()
                        .ok_or_else(|| "default requires an argument".to_string())?;
                    let Value::Date(date) = standard_date()
                        .parse_value(arg)
                        .map_err(|_| format!("invalid default date value: {}", arg))?
                        .value
                    else {
                        return Err(format!("invalid default date value: {}", arg));
                    };
                    *default = Some(date);
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
                default,
            } => match command {
                "minimum" => {
                    let arg = args
                        .first()
                        .ok_or_else(|| "minimum requires an argument".to_string())?;
                    let Value::Time(time) = &standard_time()
                        .parse_value(arg)
                        .map_err(|_| format!("invalid minimum time value: {}", arg))?
                        .value
                    else {
                        return Err(format!("invalid minimum time value: {}", arg));
                    };
                    *minimum = Some(time.clone());
                }
                "maximum" => {
                    let arg = args
                        .first()
                        .ok_or_else(|| "maximum requires an argument".to_string())?;
                    let Value::Time(time) = &standard_time()
                        .parse_value(arg)
                        .map_err(|_| format!("invalid maximum time value: {}", arg))?
                        .value
                    else {
                        return Err(format!("invalid maximum time value: {}", arg));
                    };
                    *maximum = Some(time.clone());
                }
                "help" => *help = args.first().cloned(),
                "default" => {
                    let arg = args
                        .first()
                        .ok_or_else(|| "default requires an argument".to_string())?;
                    let Value::Time(time) = &standard_time()
                        .parse_value(arg)
                        .map_err(|_| format!("invalid default time value: {}", arg))?
                        .value
                    else {
                        return Err(format!("invalid default time value: {}", arg));
                    };
                    *default = Some(time.clone());
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for time type. Valid commands: minimum, maximum, help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Duration { help, default } => match command {
                "help" => *help = args.first().cloned(),
                "default" if args.len() >= 2 => {
                    let value = args[0]
                        .parse::<Decimal>()
                        .map_err(|_| format!("invalid duration value: {}", args[0]))?;
                    let unit = args[1]
                        .parse::<DurationUnit>()
                        .map_err(|_| format!("invalid duration unit: {}", args[1]))?;
                    *default = Some((value, unit));
                }
                _ => {
                    return Err(format!(
                        "Invalid command '{}' for duration type. Valid commands: help, default",
                        command
                    ));
                }
            },
            TypeSpecification::Veto { .. } => {
                return Err(format!(
                    "Invalid command '{}' for veto type. Veto is not a user-declarable type and cannot have overrides",
                    command
                ));
            }
        }
        Ok(self)
    }
}

/// User-defined type as it appears in the source (AST)
///
/// Supports these variants:
/// - Basic type: `type money = number`
/// - Basic type extension: `type money = number -> decimals 2 -> unit EUR 1.00 -> unit USD 1.18`
/// - From another custom type: `type currency from lemma`
/// - Shorthand with overrides: `type currency from lemma -> maximum 1000`
/// - Inline type definitions: `fact age = [number -> minimum 0 -> maximum 120]`
#[derive(Clone, Debug, PartialEq)]
pub enum TypeDef {
    /// Regular named type definition
    /// Example: `type money = number -> decimals 2`
    Regular {
        name: String,
        parent: String,
        overrides: Option<Vec<(String, Vec<String>)>>,
    },
    /// Imported type from another document
    /// Example: `type currency from lemma` or `type currency from lemma -> maximum 1000`
    Import {
        name: String,
        source_type: String,
        from: String,
        overrides: Option<Vec<(String, Vec<String>)>>,
    },
    /// Inline type definition
    /// Example: `fact age = [number -> minimum 0 -> maximum 120]`
    /// Example: `fact age = [age from lemma]`
    /// Example: `fact age = [age from lemma -> minimum 18]`
    Inline {
        parent: String,
        overrides: Option<Vec<(String, Vec<String>)>>,
        fact_ref: FactReference,
        from: Option<String>,
    },
}

/// A fully resolved type used during evaluation
///
/// This combines the type specifications with all overrides already applied.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub struct LemmaType {
    pub name: Option<String>,
    pub specifications: TypeSpecification,
}

impl LemmaType {
    /// Create a new LemmaType with the given name and specifications
    pub fn new(name: String, specifications: TypeSpecification) -> Self {
        Self {
            name: Some(name),
            specifications,
        }
    }

    /// Create a new LemmaType without a name (for standard types and inline fact definitions)
    pub fn without_name(specifications: TypeSpecification) -> Self {
        Self {
            name: None,
            specifications,
        }
    }

    /// Get the name of this type, deriving from specifications if name is None
    pub fn name(&self) -> &str {
        match &self.name {
            Some(n) => n.as_str(),
            None => match &self.specifications {
                TypeSpecification::Boolean { .. } => "boolean",
                TypeSpecification::Scale { .. } => "scale",
                TypeSpecification::Number { .. } => "number",
                TypeSpecification::Text { .. } => "text",
                TypeSpecification::Date { .. } => "date",
                TypeSpecification::Time { .. } => "time",
                TypeSpecification::Duration { .. } => "duration",
                TypeSpecification::Ratio { .. } => "ratio",
                TypeSpecification::Veto { .. } => "veto",
            },
        }
    }

    /// Check if this type is boolean
    pub fn is_boolean(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Boolean { .. })
    }

    /// Check if this type is scale (has units)
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
    pub fn is_veto(&self) -> bool {
        matches!(&self.specifications, TypeSpecification::Veto { .. })
    }

    /// Check if two types have the same standard type specification (ignoring constraints/overrides)
    /// This compares only the TypeSpecification variant (Boolean, Number, Scale, Text, etc.)
    /// and ignores all constraints like minimum, maximum, decimals, units, options, etc.
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
        )
    }

    /// Create a Veto LemmaType (internal use only - not user-declarable)
    pub fn veto_type() -> Self {
        Self::without_name(TypeSpecification::veto())
    }

    /// Create a LiteralValue from this type's default value, if one exists
    pub fn create_default_value(&self) -> Option<LiteralValue> {
        use TypeSpecification::*;
        match &self.specifications {
            Boolean { default, .. } => default
                .map(|b| LiteralValue::boolean_with_type(BooleanValue::from(b), self.clone())),
            Text { default, .. } => default
                .as_ref()
                .map(|s| LiteralValue::text_with_type(s.clone(), self.clone())),
            Number { default, .. } => {
                default.map(|d| LiteralValue::number_with_type(d, self.clone()))
            }
            Scale { default, .. } => default.as_ref().map(|(value, unit_name)| {
                LiteralValue::scale_with_type(*value, Some(unit_name.clone()), self.clone())
            }),
            Ratio { default, .. } => {
                default.map(|d| LiteralValue::ratio_with_type(d, None, self.clone()))
            }
            Date { default, .. } => default
                .as_ref()
                .map(|d| LiteralValue::date_with_type(d.clone(), self.clone())),
            Time { default, .. } => default
                .as_ref()
                .map(|t| LiteralValue::time_with_type(t.clone(), self.clone())),
            Duration { default, .. } => default
                .as_ref()
                .map(|(v, u)| LiteralValue::duration_with_type(*v, u.clone(), self.clone())),
            Veto { .. } => None,
        }
    }

    /// Parse a raw string value into a LiteralValue according to this type
    pub fn parse_value(&self, raw: &str) -> Result<LiteralValue, LemmaError> {
        let value = match &self.specifications {
            TypeSpecification::Boolean { .. } => Self::parse_boolean_value(raw)?,
            TypeSpecification::Scale { .. } => Self::parse_scale_value(raw, self)?,
            TypeSpecification::Number { .. } => Self::parse_number_value(raw)?,
            TypeSpecification::Text { .. } => Self::parse_text_value(raw)?,
            TypeSpecification::Date { .. } => Self::parse_date_value(raw)?,
            TypeSpecification::Time { .. } => Self::parse_time_value(raw)?,
            TypeSpecification::Duration { .. } => Self::parse_duration_value(raw)?,
            TypeSpecification::Ratio { .. } => Self::parse_ratio_value(raw)?,
            TypeSpecification::Veto { .. } => {
                return Err(LemmaError::engine(
                    "Cannot parse value for veto type - veto is not a user-declarable type",
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    Arc::from(""),
                    "<unknown>",
                    1,
                    None::<String>,
                ));
            }
        };
        // Create LiteralValue with the appropriate helper method based on value type
        Ok(match &value {
            Value::Number(n) => LiteralValue::number_with_type(*n, self.clone()),
            Value::Scale(n, u) => LiteralValue::scale_with_type(*n, u.clone(), self.clone()),
            Value::Text(s) => LiteralValue::text_with_type(s.clone(), self.clone()),
            Value::Boolean(b) => LiteralValue::boolean_with_type(b.clone(), self.clone()),
            Value::Date(d) => LiteralValue::date_with_type(d.clone(), self.clone()),
            Value::Time(t) => LiteralValue::time_with_type(t.clone(), self.clone()),
            Value::Duration(v, u) => LiteralValue::duration_with_type(*v, u.clone(), self.clone()),
            Value::Ratio(r, u) => LiteralValue::ratio_with_type(*r, u.clone(), self.clone()),
        })
    }

    fn parse_text_value(raw: &str) -> Result<Value, LemmaError> {
        Ok(Value::Text(raw.to_string()))
    }

    fn parse_scale_value(raw: &str, lemma_type: &LemmaType) -> Result<Value, LemmaError> {
        let trimmed = raw.trim();

        // Parse number and optional unit from string
        // Handles: "50", "50 celsius", "50celsius", "1,234.56 celsius", etc.

        // First, try to find where the number part ends
        // Numbers can contain: digits, decimal point, sign, underscore, comma
        let mut number_end = 0;
        let chars: Vec<char> = trimmed.chars().collect();
        let mut has_decimal = false;

        // Skip leading sign
        let start = if chars.first().is_some_and(|c| *c == '+' || *c == '-') {
            1
        } else {
            0
        };

        for (i, &ch) in chars.iter().enumerate().skip(start) {
            match ch {
                '0'..='9' => number_end = i + 1,
                '.' if !has_decimal => {
                    has_decimal = true;
                    number_end = i + 1;
                }
                '_' | ',' => {
                    // Thousand separators - continue scanning
                    number_end = i + 1;
                }
                _ => {
                    // Non-numeric character - number ends here
                    break;
                }
            }
        }

        // Extract number and unit parts
        let number_part = trimmed[..number_end].trim();
        let unit_part = trimmed[number_end..].trim();

        // Clean number part (remove separators for parsing)
        let clean_number = number_part.replace(['_', ','], "");
        let decimal = Decimal::from_str(&clean_number).map_err(|_| {
            LemmaError::engine(
                format!("Invalid scale string: '{}' is not a valid number", raw),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(raw),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;

        // Validate unit against type definition
        let allowed_units = match &lemma_type.specifications {
            TypeSpecification::Scale { units, .. } => units,
            _ => {
                return Err(LemmaError::engine(
                    format!(
                        "Internal error: expected a scale type but got {}",
                        lemma_type.name()
                    ),
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    Arc::from(raw),
                    "<unknown>",
                    1,
                    None::<String>,
                ));
            }
        };

        let unit = if unit_part.is_empty() {
            None
        } else {
            // Validate that the unit exists in the type definition
            let unit_matched = allowed_units
                .iter()
                .find(|u| u.name.eq_ignore_ascii_case(unit_part));

            if let Some(unit_def) = unit_matched {
                Some(unit_def.name.clone())
            } else {
                let valid: Vec<String> = allowed_units.iter().map(|u| u.name.clone()).collect();
                return Err(LemmaError::engine(
                    format!(
                        "Invalid unit '{}' for scale type. Valid units: {}",
                        unit_part,
                        valid.join(", ")
                    ),
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    Arc::from(raw),
                    "<unknown>",
                    1,
                    None::<String>,
                ));
            }
        };

        Ok(Value::Scale(decimal, unit))
    }

    fn parse_number_value(raw: &str) -> Result<Value, LemmaError> {
        let clean_number = raw.replace(['_', ','], "");
        let decimal = Decimal::from_str(&clean_number).map_err(|_| {
            LemmaError::engine(
                format!("Invalid number: '{}'. Expected a valid decimal number (e.g., 42, 3.14, 1_000_000)", raw),
                Span { start: 0, end: 0, line: 1, col: 0 },
                "<unknown>",
                Arc::from(raw),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;
        Ok(Value::Number(decimal))
    }

    fn parse_boolean_value(raw: &str) -> Result<Value, LemmaError> {
        let boolean_value: BooleanValue = raw.parse().map_err(|_| {
            LemmaError::engine(
                format!(
                    "Invalid boolean: '{}'. Expected one of: true, false, yes, no, accept, reject",
                    raw
                ),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(raw),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;
        Ok(Value::Boolean(boolean_value))
    }

    fn parse_date_value(raw: &str) -> Result<Value, LemmaError> {
        let datetime_str = raw.trim();

        if let Ok(dt) = datetime_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
            let offset = dt.offset().local_minus_utc();
            return Ok(Value::Date(DateTimeValue {
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
            return Ok(Value::Date(DateTimeValue {
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
            return Ok(Value::Date(DateTimeValue {
                year: d.year(),
                month: d.month(),
                day: d.day(),
                hour: 0,
                minute: 0,
                second: 0,
                timezone: None,
            }));
        }

        Err(LemmaError::engine(
            format!("Invalid date/time format: '{}'. Expected one of: YYYY-MM-DD, YYYY-MM-DDTHH:MM:SS, or YYYY-MM-DDTHH:MM:SSZ", raw),
            Span { start: 0, end: 0, line: 1, col: 0 },
            "<unknown>",
            Arc::from(raw),
            "<unknown>",
            1,
            None::<String>,
        ))
    }

    fn parse_time_value(raw: &str) -> Result<Value, LemmaError> {
        let time_str = raw.trim();

        // Try parsing with timezone (HH:MM:SSZ or HH:MM:SS+HH:MM)
        if let Ok(dt) = time_str.parse::<chrono::DateTime<chrono::FixedOffset>>() {
            let offset = dt.offset().local_minus_utc();
            return Ok(Value::Time(TimeValue {
                hour: dt.hour() as u8,
                minute: dt.minute() as u8,
                second: dt.second() as u8,
                timezone: Some(TimezoneValue {
                    offset_hours: (offset / 3600) as i8,
                    offset_minutes: ((offset % 3600) / 60) as u8,
                }),
            }));
        }

        // Try parsing as NaiveTime (HH:MM:SS or HH:MM)
        if let Ok(nt) = time_str.parse::<chrono::NaiveTime>() {
            return Ok(Value::Time(TimeValue {
                hour: nt.hour() as u8,
                minute: nt.minute() as u8,
                second: nt.second() as u8,
                timezone: None,
            }));
        }

        // Try parsing manually for formats like "14:30" or "14:30:00"
        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() == 2 || parts.len() == 3 {
            if let (Ok(hour_u32), Ok(minute_u32)) =
                (parts[0].parse::<u32>(), parts[1].parse::<u32>())
            {
                if hour_u32 < 24 && minute_u32 < 60 {
                    let second_u32 = if parts.len() == 3 {
                        parts[2].parse::<u32>().unwrap_or(0)
                    } else {
                        0
                    };
                    if second_u32 < 60 {
                        return Ok(Value::Time(TimeValue {
                            hour: hour_u32 as u8,
                            minute: minute_u32 as u8,
                            second: second_u32 as u8,
                            timezone: None,
                        }));
                    }
                }
            }
        }

        Err(LemmaError::engine(
            format!(
                "Invalid time format: '{}'. Expected: HH:MM or HH:MM:SS (e.g., 14:30 or 14:30:00)",
                raw
            ),
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<unknown>",
            Arc::from(raw),
            "<unknown>",
            1,
            None::<String>,
        ))
    }

    fn parse_duration_value(raw: &str) -> Result<Value, LemmaError> {
        // Parse duration like "90 minutes" or "2 hours"
        let parts: Vec<&str> = raw.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(LemmaError::engine(
                format!(
                    "Invalid duration: '{}'. Expected format: NUMBER UNIT (e.g., '90 minutes')",
                    raw
                ),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(raw),
                "<unknown>",
                1,
                None::<String>,
            ));
        }

        let number_str = parts[0].replace(['_', ','], "");
        let value = Decimal::from_str(&number_str).map_err(|_| {
            LemmaError::engine(
                format!("Invalid number in duration: '{}'", parts[0]),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(raw),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;
        let unit = parts[1];

        // Parse duration unit
        let unit_lower = unit.to_lowercase();
        let duration_unit = match unit_lower.as_str() {
            "year" | "years" => DurationUnit::Year,
            "month" | "months" => DurationUnit::Month,
            "week" | "weeks" => DurationUnit::Week,
            "day" | "days" => DurationUnit::Day,
            "hour" | "hours" => DurationUnit::Hour,
            "minute" | "minutes" => DurationUnit::Minute,
            "second" | "seconds" => DurationUnit::Second,
            "millisecond" | "milliseconds" => DurationUnit::Millisecond,
            "microsecond" | "microseconds" => DurationUnit::Microsecond,
            _ => {
                return Err(LemmaError::engine(
                    format!("Unknown duration unit: '{}'. Expected one of: years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds", unit),
                    Span { start: 0, end: 0, line: 1, col: 0 },
                    "<unknown>",
                    Arc::from(raw),
                    "<unknown>",
                    1,
                    None::<String>,
                ));
            }
        };
        Ok(Value::Duration(value, duration_unit))
    }

    fn parse_ratio_value(raw: &str) -> Result<Value, LemmaError> {
        // Parse ratio as a decimal number
        let clean_number = raw.replace(['_', ','], "");
        let decimal = Decimal::from_str(&clean_number).map_err(|_| {
            LemmaError::engine(
                format!("Invalid ratio: '{}'. Expected a valid decimal number", raw),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(raw),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;
        Ok(Value::Ratio(decimal, None))
    }
}

// Private statics for lazy initialization
static STANDARD_BOOLEAN: OnceLock<LemmaType> = OnceLock::new();
static STANDARD_SCALE: OnceLock<LemmaType> = OnceLock::new();
static STANDARD_NUMBER: OnceLock<LemmaType> = OnceLock::new();
static STANDARD_TEXT: OnceLock<LemmaType> = OnceLock::new();
static STANDARD_DATE: OnceLock<LemmaType> = OnceLock::new();
static STANDARD_TIME: OnceLock<LemmaType> = OnceLock::new();
static STANDARD_DURATION: OnceLock<LemmaType> = OnceLock::new();
static STANDARD_RATIO: OnceLock<LemmaType> = OnceLock::new();

/// Get the standard boolean type
pub fn standard_boolean() -> &'static LemmaType {
    STANDARD_BOOLEAN.get_or_init(|| LemmaType {
        name: None,
        specifications: TypeSpecification::Boolean {
            help: None,
            default: None,
        },
    })
}

/// Get the standard scale type (can have units)
pub fn standard_scale() -> &'static LemmaType {
    STANDARD_SCALE.get_or_init(|| LemmaType {
        name: None,
        specifications: TypeSpecification::Scale {
            minimum: None,
            maximum: None,
            decimals: None,
            precision: None,
            units: Vec::new(),
            help: None,
            default: None,
        },
    })
}

/// Get the standard number type (dimensionless, no units)
pub fn standard_number() -> &'static LemmaType {
    STANDARD_NUMBER.get_or_init(|| LemmaType {
        name: None,
        specifications: TypeSpecification::Number {
            minimum: None,
            maximum: None,
            decimals: None,
            precision: None,
            help: None,
            default: None,
        },
    })
}

/// Get the standard text type
pub fn standard_text() -> &'static LemmaType {
    STANDARD_TEXT.get_or_init(|| LemmaType {
        name: None,
        specifications: TypeSpecification::Text {
            minimum: None,
            maximum: None,
            length: None,
            options: Vec::new(),
            help: None,
            default: None,
        },
    })
}

/// Get the standard date type
pub fn standard_date() -> &'static LemmaType {
    STANDARD_DATE.get_or_init(|| LemmaType {
        name: None,
        specifications: TypeSpecification::Date {
            minimum: None,
            maximum: None,
            help: None,
            default: None,
        },
    })
}

/// Get the standard time type
pub fn standard_time() -> &'static LemmaType {
    STANDARD_TIME.get_or_init(|| LemmaType {
        name: None,
        specifications: TypeSpecification::Time {
            minimum: None,
            maximum: None,
            help: None,
            default: None,
        },
    })
}

/// Get the standard duration type
pub fn standard_duration() -> &'static LemmaType {
    STANDARD_DURATION.get_or_init(|| LemmaType {
        name: None,
        specifications: TypeSpecification::Duration {
            help: None,
            default: None,
        },
    })
}

/// Get the standard ratio type
pub fn standard_ratio() -> &'static LemmaType {
    STANDARD_RATIO.get_or_init(|| LemmaType {
        name: None,
        specifications: TypeSpecification::Ratio {
            minimum: None,
            maximum: None,
            units: vec![
                Unit {
                    name: "percent".to_string(),
                    value: Decimal::from(100),
                },
                Unit {
                    name: "permille".to_string(),
                    value: Decimal::from(1000),
                },
            ],
            help: None,
            default: None,
        },
    })
}

// Helper macros to initialize standard types

impl LemmaType {
    /// Get an example value string for this type, suitable for UI help text
    pub fn example_value(&self) -> &'static str {
        match &self.specifications {
            TypeSpecification::Text { .. } => "\"hello world\"",
            TypeSpecification::Scale { .. } => "3.14",
            TypeSpecification::Number { .. } => "3.14",
            TypeSpecification::Boolean { .. } => "true",
            TypeSpecification::Date { .. } => "2023-12-25T14:30:00Z",
            TypeSpecification::Veto { .. } => "veto",
            TypeSpecification::Time { .. } => "14:30:00",
            TypeSpecification::Duration { .. } => "90 minutes",
            TypeSpecification::Ratio { .. } => "50%",
        }
    }
}

fn strip_surrounding_quotes(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..bytes.len() - 1].to_string();
        }
    }
    s.to_string()
}

impl fmt::Display for LemmaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
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
        assert_eq!(ComparisonComputation::Equal.name(), "equal");
        assert_eq!(ComparisonComputation::NotEqual.name(), "not equal");
        assert_eq!(ComparisonComputation::Is.name(), "is");
        assert_eq!(ComparisonComputation::IsNot.name(), "is not");
    }

    #[test]
    fn test_literal_value_to_standard_type() {
        let one = Decimal::from_str("1").unwrap();

        assert_eq!(LiteralValue::text("".to_string()).lemma_type.name(), "text");
        assert_eq!(LiteralValue::number(one).lemma_type.name(), "number");
        assert_eq!(
            LiteralValue::boolean(BooleanValue::True).lemma_type.name(),
            "boolean"
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
        assert_eq!(LiteralValue::date(dt).lemma_type.name(), "date");
        assert_eq!(
            LiteralValue::ratio(one / Decimal::from(100), Some("percent".to_string()))
                .lemma_type
                .name(),
            "ratio"
        );
        assert_eq!(
            LiteralValue::duration(one, DurationUnit::Second)
                .lemma_type
                .name(),
            "duration"
        );
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
        assert_eq!(format!("{}", ComparisonComputation::Equal), "==");
        assert_eq!(format!("{}", ComparisonComputation::NotEqual), "!=");
        assert_eq!(format!("{}", ComparisonComputation::Is), "is");
        assert_eq!(format!("{}", ComparisonComputation::IsNot), "is not");
    }

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
        assert_eq!(format!("{}", ConversionTarget::Percentage), "percent");
        assert_eq!(
            format!("{}", ConversionTarget::Duration(DurationUnit::Hour)),
            "hours"
        );
    }

    #[test]
    fn test_doc_type_display() {
        assert_eq!(format!("{}", standard_text()), "text");
        assert_eq!(format!("{}", standard_number()), "number");
        assert_eq!(format!("{}", standard_date()), "date");
        assert_eq!(format!("{}", standard_boolean()), "boolean");
        assert_eq!(format!("{}", standard_duration()), "duration");
    }

    #[test]
    fn test_type_constructor() {
        let specs = TypeSpecification::number();
        let lemma_type = LemmaType::new("dice".to_string(), specs);
        assert_eq!(lemma_type.name(), "dice");
    }

    #[test]
    fn test_type_display() {
        let specs = TypeSpecification::text();
        let lemma_type = LemmaType::new("name".to_string(), specs);
        assert_eq!(format!("{}", lemma_type), "name");
    }

    #[test]
    fn test_type_equality() {
        let specs1 = TypeSpecification::number();
        let specs2 = TypeSpecification::number();
        let lemma_type1 = LemmaType::new("dice".to_string(), specs1);
        let lemma_type2 = LemmaType::new("dice".to_string(), specs2);
        assert_eq!(lemma_type1, lemma_type2);
    }

    #[test]
    fn test_type_serialization() {
        let specs = TypeSpecification::number();
        let lemma_type = LemmaType::new("dice".to_string(), specs);
        let serialized = serde_json::to_string(&lemma_type).unwrap();
        let deserialized: LemmaType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(lemma_type, deserialized);
    }

    #[test]
    fn test_literal_value_display_value() {
        let ten = Decimal::from_str("10").unwrap();

        assert_eq!(
            LiteralValue::text("hello".to_string()).display_value(),
            "\"hello\""
        );
        assert_eq!(LiteralValue::number(ten).display_value(), "10");
        assert_eq!(
            LiteralValue::boolean(BooleanValue::True).display_value(),
            "true"
        );
        assert_eq!(
            LiteralValue::boolean(BooleanValue::False).display_value(),
            "false"
        );
        // 10% stored as 0.10 ratio with "percent" unit
        let ten_percent_ratio = LiteralValue::ratio(
            Decimal::from_str("0.10").unwrap(),
            Some("percent".to_string()),
        );
        // ratio with "percent" unit should display as percent
        assert_eq!(ten_percent_ratio.display_value(), "10%");

        let time = TimeValue {
            hour: 14,
            minute: 30,
            second: 0,
            timezone: None,
        };
        let time_display = LiteralValue::time(time).display_value();
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
        assert_eq!(LiteralValue::time(time).lemma_type.name(), "time");
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

    #[test]
    fn test_expression_get_source_text_with_location() {
        use crate::{Expression, ExpressionKind, LiteralValue, Source, Span};
        use std::collections::HashMap;

        let source = "fact value = 42";
        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

        let span = Span {
            start: 13,
            end: 15,
            line: 1,
            col: 13,
        };
        let source_location = Some(Source::new("test.lemma", span, "test"));
        let expr = Expression::new(
            ExpressionKind::Literal(LiteralValue::number(rust_decimal::Decimal::new(42, 0))),
            source_location,
        );

        assert_eq!(expr.get_source_text(&sources), Some("42".to_string()));
    }

    #[test]
    fn test_expression_get_source_text_no_location() {
        use crate::{Expression, ExpressionKind, LiteralValue};
        use std::collections::HashMap;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), "fact value = 42".to_string());

        let expr = Expression::new(
            ExpressionKind::Literal(LiteralValue::number(rust_decimal::Decimal::new(42, 0))),
            None,
        );

        assert_eq!(expr.get_source_text(&sources), None);
    }

    #[test]
    fn test_expression_get_source_text_source_not_found() {
        use crate::{Expression, ExpressionKind, LiteralValue, Source, Span};
        use std::collections::HashMap;

        let sources = HashMap::new();
        let span = Span {
            start: 0,
            end: 5,
            line: 1,
            col: 0,
        };
        let source_location = Some(Source::new("missing.lemma", span, "test"));
        let expr = Expression::new(
            ExpressionKind::Literal(LiteralValue::number(rust_decimal::Decimal::new(42, 0))),
            source_location,
        );

        assert_eq!(expr.get_source_text(&sources), None);
    }
}
