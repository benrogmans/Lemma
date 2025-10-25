use crate::ast::{ExpressionId, Span};
use rust_decimal::Decimal;
use serde::Serialize;
use std::fmt;

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

#[derive(Debug, Clone, PartialEq)]
pub struct LemmaFact {
    pub fact_type: FactType,
    pub value: FactValue,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FactType {
    Local(String),
    Foreign(ForeignFact),
}

/// A fact that references another document
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignFact {
    pub reference: Vec<String>,
}

/// An unless clause that provides an alternative result
///
/// Unless clauses are evaluated in order, and the last matching condition wins.
/// This matches natural language: "X unless A then Y, unless B then Z" - if both
/// A and B are true, Z is returned (the last match).
#[derive(Debug, Clone, PartialEq)]
pub struct UnlessClause {
    pub condition: Expression,
    pub result: Expression,
    pub span: Option<Span>,
}

/// A rule with a single expression and optional unless clauses
#[derive(Debug, Clone, PartialEq)]
pub struct LemmaRule {
    pub name: String,
    pub expression: Expression,
    pub unless_clauses: Vec<UnlessClause>,
    pub span: Option<Span>,
}

/// An expression that can be evaluated, with source location and unique ID
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: Option<Span>,
    pub id: ExpressionId,
}

impl Expression {
    /// Create a new expression with kind, span, and ID
    pub fn new(kind: ExpressionKind, span: Option<Span>, id: ExpressionId) -> Self {
        Self { kind, span, id }
    }
}

/// The kind/type of expression
#[derive(Debug, Clone, PartialEq)]
pub enum ExpressionKind {
    Literal(LiteralValue),
    FactReference(FactReference),
    RuleReference(RuleReference),
    LogicalAnd(Box<Expression>, Box<Expression>),
    LogicalOr(Box<Expression>, Box<Expression>),
    Arithmetic(Box<Expression>, ArithmeticOperation, Box<Expression>),
    Comparison(Box<Expression>, ComparisonOperator, Box<Expression>),
    FactHasAnyValue(FactReference),
    UnitConversion(Box<Expression>, ConversionTarget),
    LogicalNegation(Box<Expression>, NegationType),
    MathematicalOperator(MathematicalOperator, Box<Expression>),
    Veto(VetoExpression),
}

/// Reference to a fact
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FactReference {
    pub reference: Vec<String>, // ["file", "size"]
}

/// Reference to a rule
///
/// Rule references use a question mark suffix to distinguish them from fact references.
/// Example: `has_license?` references the `has_license` rule.
/// Cross-document example: `employee.is_eligible?` references the `is_eligible` rule from the `employee` document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleReference {
    pub reference: Vec<String>, // ["employee", "is_eligible"] or just ["is_eligible"]
}

/// Arithmetic operations
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArithmeticOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
}

impl ArithmeticOperation {
    /// Returns a human-readable name for the operation
    pub fn name(&self) -> &'static str {
        match self {
            ArithmeticOperation::Add => "addition",
            ArithmeticOperation::Subtract => "subtraction",
            ArithmeticOperation::Multiply => "multiplication",
            ArithmeticOperation::Divide => "division",
            ArithmeticOperation::Modulo => "modulo",
            ArithmeticOperation::Power => "exponentiation",
        }
    }
}

/// Comparison operators
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ComparisonOperator {
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Equal,
    NotEqual,
    Is,
    IsNot,
}

impl ComparisonOperator {
    /// Returns a human-readable name for the operator
    pub fn name(&self) -> &'static str {
        match self {
            ComparisonOperator::GreaterThan => "greater than",
            ComparisonOperator::LessThan => "less than",
            ComparisonOperator::GreaterThanOrEqual => "greater than or equal",
            ComparisonOperator::LessThanOrEqual => "less than or equal",
            ComparisonOperator::Equal => "equal",
            ComparisonOperator::NotEqual => "not equal",
            ComparisonOperator::Is => "is",
            ComparisonOperator::IsNot => "is not",
        }
    }
}

/// The target unit for unit conversion expressions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    Money(MoneyUnit),
    Percentage,
}

/// Types of logical negation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NegationType {
    Not,     // "not expression"
    HaveNot, // "have not expression"
    NotHave, // "not have expression"
}

/// A veto expression that prohibits any valid verdict from the rule
///
/// Unlike `reject` (which is just an alias for boolean `false`), a veto
/// prevents the rule from producing any valid result. This is used for
/// validation and constraint enforcement.
///
/// Example: `veto "Must be over 18"` - blocks the rule entirely with a message
#[derive(Debug, Clone, PartialEq)]
pub struct VetoExpression {
    pub message: Option<String>,
}

/// Mathematical operators
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MathematicalOperator {
    Sqrt, // Square root
    Sin,  // Sine
    Cos,  // Cosine
    Tan,  // Tangent
    Asin, // Arc sine
    Acos, // Arc cosine
    Atan, // Arc tangent
    Log,  // Natural logarithm
    Exp,  // Exponential (e^x)
}

#[derive(Debug, Clone, PartialEq)]
pub enum FactValue {
    Literal(LiteralValue),
    DocumentReference(String),
    TypeAnnotation(TypeAnnotation),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    LemmaType(LemmaType),
}

/// A type for type annotations (both literal types and document types)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    Money,
}

/// A literal value
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum LiteralValue {
    Number(Decimal),
    Text(String),
    Date(DateTimeValue), // Date with time and timezone information preserved
    Time(TimeValue),     // Standalone time with optional timezone
    Boolean(bool),
    Percentage(Decimal),
    Unit(NumericUnit), // All physical units and money
    Regex(String),     // e.g., "/pattern/"
}

impl LiteralValue {
    /// Get the display value as a string (uses the Display implementation)
    pub fn display_value(&self) -> String {
        self.to_string()
    }

    /// Convert a LiteralValue to its corresponding LemmaType
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
                NumericUnit::Money(_, _) => LemmaType::Money,
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
    DataUnit,
    MoneyUnit
);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MassUnit {
    Kilogram,
    Gram,
    Milligram,
    Ton,
    Pound,
    Ounce,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LengthUnit {
    Kilometer,
    Mile,
    NauticalMile,
    Meter,
    Decimeter,
    Centimeter,
    Millimeter,
    Yard,
    Foot,
    Inch,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VolumeUnit {
    CubicMeter,
    CubicCentimeter,
    Liter,
    Deciliter,
    Centiliter,
    Milliliter,
    Gallon,
    Quart,
    Pint,
    FluidOunce,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
    Kelvin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PowerUnit {
    Megawatt,
    Kilowatt,
    Watt,
    Milliwatt,
    Horsepower,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ForceUnit {
    Newton,
    Kilonewton,
    Lbf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FrequencyUnit {
    Hertz,
    Kilohertz,
    Megahertz,
    Gigahertz,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MoneyUnit {
    Eur,
    Usd,
    Gbp,
    Jpy,
    Cny,
    Chf,
    Cad,
    Aud,
    Inr,
}

/// A unified type for all numeric units (physical quantities and money)
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
    Money(Decimal, MoneyUnit),
}

impl NumericUnit {
    /// Extract the numeric value from any unit
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
            | NumericUnit::Data(v, _)
            | NumericUnit::Money(v, _) => *v,
        }
    }

    /// Check if two units are the same category
    pub fn same_category(&self, other: &NumericUnit) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// Create a new NumericUnit with the same unit type but different value
    /// This is the key method that eliminates type enumeration in operations
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
            NumericUnit::Money(_, u) => NumericUnit::Money(new_value, u.clone()),
        }
    }

    /// Validate that two Money units have the same currency
    /// Returns Ok for non-Money units or matching currencies
    pub fn validate_same_currency(&self, other: &NumericUnit) -> Result<(), crate::LemmaError> {
        if let (NumericUnit::Money(_, l_curr), NumericUnit::Money(_, r_curr)) = (self, other) {
            if l_curr != r_curr {
                return Err(crate::LemmaError::Engine(format!(
                    "Cannot operate on different currencies: {:?} and {:?}",
                    l_curr, r_curr
                )));
            }
        }
        Ok(())
    }
}

impl fmt::Display for NumericUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumericUnit::Mass(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Length(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Volume(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Duration(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Temperature(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Power(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Force(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Pressure(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Energy(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Frequency(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Data(v, u) => write!(f, "{} {}", v, u),
            NumericUnit::Money(v, u) => write!(f, "{} {}", v, u),
        }
    }
}

impl LemmaRule {
    pub fn new(name: String, expression: Expression) -> Self {
        Self {
            name,
            expression,
            unless_clauses: Vec::new(),
            span: None,
        }
    }

    pub fn add_unless_clause(mut self, unless_clause: UnlessClause) -> Self {
        self.unless_clauses.push(unless_clause);
        self
    }
}

impl LemmaFact {
    pub fn new(fact_type: FactType, value: FactValue) -> Self {
        Self {
            fact_type,
            value,
            span: None,
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
}

impl LemmaDoc {
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

    pub fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_start_line(mut self, start_line: usize) -> Self {
        self.start_line = start_line;
        self
    }

    pub fn set_commentary(mut self, commentary: String) -> Self {
        self.commentary = Some(commentary);
        self
    }

    pub fn add_fact(mut self, fact: LemmaFact) -> Self {
        self.facts.push(fact);
        self
    }

    pub fn add_rule(mut self, rule: LemmaRule) -> Self {
        self.rules.push(rule);
        self
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

impl fmt::Display for LemmaFact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fact {} = {}", self.fact_type, self.value)
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
            ExpressionKind::RuleReference(rule_ref) => write!(f, "{}", rule_ref),
            ExpressionKind::Arithmetic(left, op, right) => {
                write!(f, "{} {} {}", left, op, right)
            }
            ExpressionKind::Comparison(left, op, right) => {
                write!(f, "{} {} {}", left, op, right)
            }
            ExpressionKind::FactHasAnyValue(fact_ref) => {
                write!(f, "have {}", fact_ref)
            }
            ExpressionKind::UnitConversion(value, target) => {
                write!(f, "{} in {}", value, target)
            }
            ExpressionKind::LogicalNegation(expr, negation_type) => {
                let prefix = match negation_type {
                    NegationType::Not => "not",
                    NegationType::HaveNot => "have not",
                    NegationType::NotHave => "not have",
                };
                write!(f, "{} {}", prefix, expr)
            }
            ExpressionKind::LogicalAnd(left, right) => {
                write!(f, "{} and {}", left, right)
            }
            ExpressionKind::LogicalOr(left, right) => {
                write!(f, "{} or {}", left, right)
            }
            ExpressionKind::MathematicalOperator(op, operand) => {
                let op_name = match op {
                    MathematicalOperator::Sqrt => "sqrt",
                    MathematicalOperator::Sin => "sin",
                    MathematicalOperator::Cos => "cos",
                    MathematicalOperator::Tan => "tan",
                    MathematicalOperator::Asin => "asin",
                    MathematicalOperator::Acos => "acos",
                    MathematicalOperator::Atan => "atan",
                    MathematicalOperator::Log => "log",
                    MathematicalOperator::Exp => "exp",
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
            LiteralValue::Number(n) => write!(f, "{}", n),
            LiteralValue::Text(s) => write!(f, "\"{}\"", s),
            LiteralValue::Date(dt) => write!(f, "{}", dt),
            LiteralValue::Boolean(b) => write!(f, "{}", b),
            LiteralValue::Percentage(p) => write!(f, "{}%", p),
            LiteralValue::Unit(unit) => write!(f, "{}", unit),
            LiteralValue::Regex(s) => write!(f, "{}", s),
            LiteralValue::Time(time) => {
                write!(f, "time({}, {}, {})", time.hour, time.minute, time.second)
            }
        }
    }
}

impl LiteralValue {
    /// Provides a descriptive string for error messages and debugging
    pub fn describe(&self) -> String {
        match self {
            LiteralValue::Text(s) => format!("text value \"{}\"", s),
            LiteralValue::Number(n) => format!("number {}", n),
            LiteralValue::Boolean(b) => format!("boolean {}", b),
            LiteralValue::Percentage(p) => format!("percentage {}%", p),
            LiteralValue::Date(_) => "date value".to_string(),
            LiteralValue::Unit(unit) => {
                format!(
                    "{} value {}",
                    LiteralValue::Unit(unit.clone()).to_type(),
                    unit
                )
            }
            LiteralValue::Regex(s) => format!("regex value {}", s),
            LiteralValue::Time(time) => {
                format!("time value {}:{}:{}", time.hour, time.minute, time.second)
            }
        }
    }
}

impl fmt::Display for MassUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MassUnit::Kilogram => write!(f, "kilogram"),
            MassUnit::Gram => write!(f, "gram"),
            MassUnit::Milligram => write!(f, "milligram"),
            MassUnit::Ton => write!(f, "ton"),
            MassUnit::Pound => write!(f, "pound"),
            MassUnit::Ounce => write!(f, "ounce"),
        }
    }
}

impl fmt::Display for LengthUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LengthUnit::Kilometer => write!(f, "kilometer"),
            LengthUnit::Mile => write!(f, "mile"),
            LengthUnit::NauticalMile => write!(f, "nautical_mile"),
            LengthUnit::Meter => write!(f, "meter"),
            LengthUnit::Decimeter => write!(f, "decimeter"),
            LengthUnit::Centimeter => write!(f, "centimeter"),
            LengthUnit::Millimeter => write!(f, "millimeter"),
            LengthUnit::Yard => write!(f, "yard"),
            LengthUnit::Foot => write!(f, "foot"),
            LengthUnit::Inch => write!(f, "inch"),
        }
    }
}

impl fmt::Display for VolumeUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VolumeUnit::CubicMeter => write!(f, "cubic_meter"),
            VolumeUnit::CubicCentimeter => write!(f, "cubic_centimeter"),
            VolumeUnit::Liter => write!(f, "liter"),
            VolumeUnit::Deciliter => write!(f, "deciliter"),
            VolumeUnit::Centiliter => write!(f, "centiliter"),
            VolumeUnit::Milliliter => write!(f, "milliliter"),
            VolumeUnit::Gallon => write!(f, "gallon"),
            VolumeUnit::Quart => write!(f, "quart"),
            VolumeUnit::Pint => write!(f, "pint"),
            VolumeUnit::FluidOunce => write!(f, "fluid_ounce"),
        }
    }
}

impl fmt::Display for DurationUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DurationUnit::Year => write!(f, "year"),
            DurationUnit::Month => write!(f, "month"),
            DurationUnit::Week => write!(f, "week"),
            DurationUnit::Day => write!(f, "day"),
            DurationUnit::Hour => write!(f, "hour"),
            DurationUnit::Minute => write!(f, "minute"),
            DurationUnit::Second => write!(f, "second"),
            DurationUnit::Millisecond => write!(f, "millisecond"),
            DurationUnit::Microsecond => write!(f, "microsecond"),
        }
    }
}

impl fmt::Display for TemperatureUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemperatureUnit::Celsius => write!(f, "celsius"),
            TemperatureUnit::Fahrenheit => write!(f, "fahrenheit"),
            TemperatureUnit::Kelvin => write!(f, "kelvin"),
        }
    }
}

impl fmt::Display for PowerUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PowerUnit::Megawatt => write!(f, "megawatt"),
            PowerUnit::Kilowatt => write!(f, "kilowatt"),
            PowerUnit::Watt => write!(f, "watt"),
            PowerUnit::Milliwatt => write!(f, "milliwatt"),
            PowerUnit::Horsepower => write!(f, "horsepower"),
        }
    }
}

impl fmt::Display for ForceUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ForceUnit::Newton => write!(f, "newton"),
            ForceUnit::Kilonewton => write!(f, "kilonewton"),
            ForceUnit::Lbf => write!(f, "lbf"),
        }
    }
}

impl fmt::Display for PressureUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PressureUnit::Megapascal => write!(f, "megapascal"),
            PressureUnit::Kilopascal => write!(f, "kilopascal"),
            PressureUnit::Pascal => write!(f, "pascal"),
            PressureUnit::Atmosphere => write!(f, "atmosphere"),
            PressureUnit::Bar => write!(f, "bar"),
            PressureUnit::Psi => write!(f, "psi"),
            PressureUnit::Torr => write!(f, "torr"),
            PressureUnit::Mmhg => write!(f, "mmhg"),
        }
    }
}

impl fmt::Display for EnergyUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnergyUnit::Megajoule => write!(f, "megajoule"),
            EnergyUnit::Kilojoule => write!(f, "kilojoule"),
            EnergyUnit::Joule => write!(f, "joule"),
            EnergyUnit::Kilowatthour => write!(f, "kilowatthour"),
            EnergyUnit::Watthour => write!(f, "watthour"),
            EnergyUnit::Kilocalorie => write!(f, "kilocalorie"),
            EnergyUnit::Calorie => write!(f, "calorie"),
            EnergyUnit::Btu => write!(f, "btu"),
        }
    }
}

impl fmt::Display for FrequencyUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrequencyUnit::Hertz => write!(f, "hertz"),
            FrequencyUnit::Kilohertz => write!(f, "kilohertz"),
            FrequencyUnit::Megahertz => write!(f, "megahertz"),
            FrequencyUnit::Gigahertz => write!(f, "gigahertz"),
        }
    }
}

impl fmt::Display for DataUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataUnit::Petabyte => write!(f, "petabyte"),
            DataUnit::Terabyte => write!(f, "terabyte"),
            DataUnit::Gigabyte => write!(f, "gigabyte"),
            DataUnit::Megabyte => write!(f, "megabyte"),
            DataUnit::Kilobyte => write!(f, "kilobyte"),
            DataUnit::Byte => write!(f, "byte"),
            DataUnit::Tebibyte => write!(f, "tebibyte"),
            DataUnit::Gibibyte => write!(f, "gibibyte"),
            DataUnit::Mebibyte => write!(f, "mebibyte"),
            DataUnit::Kibibyte => write!(f, "kibibyte"),
        }
    }
}

impl fmt::Display for MoneyUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoneyUnit::Eur => write!(f, "EUR"),
            MoneyUnit::Usd => write!(f, "USD"),
            MoneyUnit::Gbp => write!(f, "GBP"),
            MoneyUnit::Jpy => write!(f, "JPY"),
            MoneyUnit::Cny => write!(f, "CNY"),
            MoneyUnit::Chf => write!(f, "CHF"),
            MoneyUnit::Cad => write!(f, "CAD"),
            MoneyUnit::Aud => write!(f, "AUD"),
            MoneyUnit::Inr => write!(f, "INR"),
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
            ConversionTarget::Money(unit) => write!(f, "{}", unit),
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
            LemmaType::Money => write!(f, "money"),
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
    pub fn example_value(&self) -> &'static str {
        match self {
            LemmaType::Text => "\"hello world\"",
            LemmaType::Number => "3.14",
            LemmaType::Boolean => "true",
            LemmaType::Money => "99.99 EUR",
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

impl fmt::Display for FactReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reference.join("."))
    }
}

impl fmt::Display for ForeignFact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reference.join("."))
    }
}

impl fmt::Display for FactType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FactType::Local(name) => write!(f, "{}", name),
            FactType::Foreign(foreign_ref) => write!(f, "{}", foreign_ref),
        }
    }
}

impl fmt::Display for RuleReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}?", self.reference.join("."))
    }
}

impl fmt::Display for ArithmeticOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArithmeticOperation::Add => write!(f, "+"),
            ArithmeticOperation::Subtract => write!(f, "-"),
            ArithmeticOperation::Multiply => write!(f, "*"),
            ArithmeticOperation::Divide => write!(f, "/"),
            ArithmeticOperation::Modulo => write!(f, "%"),
            ArithmeticOperation::Power => write!(f, "^"),
        }
    }
}

impl fmt::Display for ComparisonOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComparisonOperator::GreaterThan => write!(f, ">"),
            ComparisonOperator::LessThan => write!(f, "<"),
            ComparisonOperator::GreaterThanOrEqual => write!(f, ">="),
            ComparisonOperator::LessThanOrEqual => write!(f, "<="),
            ComparisonOperator::Equal => write!(f, "=="),
            ComparisonOperator::NotEqual => write!(f, "!="),
            ComparisonOperator::Is => write!(f, "is"),
            ComparisonOperator::IsNot => write!(f, "is not"),
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
