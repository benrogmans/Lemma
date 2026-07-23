use crate::computation::rational::NumericFailure;
use crate::evaluation::explanations::Explanation;
use crate::evaluation::operations::{OperationResult, VetoType};

use crate::parsing::ast::DateTimeValue;
use crate::planning::semantics::{
    range_element_type_specification, LemmaType, LiteralUnitMapFailure, LiteralValue, RulePath,
    SemanticDateTime, SemanticTime, Source, TypeSpecification, ValueKind,
};
use indexmap::IndexMap;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Rule info with resolved expressions for use in evaluation response.
/// Evaluation uses only semantics types; no parsing types.
#[derive(Debug, Clone, Serialize)]
pub struct EvaluatedRule {
    pub name: String,
    pub path: RulePath,
    pub source_location: Source,
    pub rule_type: LemmaType,
}

/// Calendar value on a rule result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CalendarResult {
    pub value: String,
    pub unit: String,
}

/// One endpoint of a range rule result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct RuleResultPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measure: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boolean: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<SemanticDateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<SemanticTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<CalendarResult>,
}

/// Range rule result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RangeResult {
    pub from: RuleResultPayload,
    pub to: RuleResultPayload,
}

/// Response from evaluating a Lemma spec
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    #[serde(rename = "spec")]
    pub spec_name: String,
    pub effective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_effective_from: Option<DateTimeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_effective_to: Option<DateTimeValue>,
    pub results: IndexMap<String, RuleResult>,
}

/// Result of evaluating a single rule. Struct fields match the API JSON shape.
#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    #[serde(skip)]
    pub rule: EvaluatedRule,
    #[serde(skip)]
    pub veto_detail: Option<VetoType>,

    pub vetoed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub veto_reason: Option<String>,
    pub rule_type: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub measure: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boolean: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<SemanticDateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<SemanticTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<CalendarResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<Explanation>,
    /// Unbound caller data paths still live for this rule under the current overlay
    /// (`DataPath::input_key` strings, same keys as `Show.data`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_data: Vec<String>,
}

impl RuleResult {
    /// Materialize a rule evaluation result for API output.
    ///
    /// Measure and ratio payloads expand into every unit declared on `rule_type`.
    pub fn from_operation_result(
        rule: EvaluatedRule,
        operation_result: &OperationResult,
        rule_type: &LemmaType,
        explanation: Option<Explanation>,
        missing_data: Vec<String>,
    ) -> Self {
        let rule_type_name = rule_type.name().to_string();
        match operation_result {
            OperationResult::Veto(veto) => Self {
                rule,
                veto_detail: Some(veto.clone()),
                vetoed: true,
                display: None,
                veto_reason: match &veto {
                    VetoType::UserDefined { message: None } => None,
                    _ => Some(veto.to_string()),
                },
                rule_type: rule_type_name,
                measure: None,
                ratio: None,
                number: None,
                boolean: None,
                text: None,
                date: None,
                time: None,
                calendar: None,
                range: None,
                explanation,
                missing_data,
            },
            OperationResult::Value(literal) => match &literal.value {
                ValueKind::Range(from, to) => {
                    let endpoint_type = element_type_from_range_rule(rule_type)
                        .unwrap_or_else(|| rule_type.clone());
                    let from_type = endpoint_materialization_type(from, &endpoint_type);
                    let to_type = endpoint_materialization_type(to, &endpoint_type);
                    match (
                        materialize_payload(from, &from_type),
                        materialize_payload(to, &to_type),
                    ) {
                        (Ok(from_payload), Ok(to_payload)) => Self {
                            rule,
                            veto_detail: None,
                            vetoed: false,
                            display: Some(literal.to_string()),
                            veto_reason: None,
                            rule_type: rule_type_name,
                            measure: None,
                            ratio: None,
                            number: None,
                            boolean: None,
                            text: None,
                            date: None,
                            time: None,
                            calendar: None,
                            range: Some(RangeResult {
                                from: from_payload,
                                to: to_payload,
                            }),
                            explanation,
                            missing_data,
                        },
                        (Err(failure), _) | (_, Err(failure)) => {
                            vetoed_rule_result_for_materialization_failure(
                                rule,
                                rule_type_name,
                                explanation,
                                failure,
                                missing_data,
                            )
                        }
                    }
                }
                _ => match materialize_payload(literal, rule_type) {
                    Ok(payload) => Self {
                        rule,
                        veto_detail: None,
                        vetoed: false,
                        display: Some(literal.to_string()),
                        veto_reason: None,
                        rule_type: rule_type_name,
                        measure: payload.measure,
                        ratio: payload.ratio,
                        number: payload.number,
                        boolean: payload.boolean,
                        text: payload.text,
                        date: payload.date,
                        time: payload.time,
                        calendar: payload.calendar,
                        range: None,
                        explanation,
                        missing_data,
                    },
                    Err(failure) => vetoed_rule_result_for_materialization_failure(
                        rule,
                        rule_type_name,
                        explanation,
                        failure,
                        missing_data,
                    ),
                },
            },
        }
    }

    /// Reconstruct the evaluated [`LiteralValue`] from committed materialized fields.
    ///
    /// Panics if the rule is vetoed or materialized fields cannot be reconstructed.
    pub fn materialized_literal(&self) -> LiteralValue {
        assert!(
            !self.vetoed,
            "BUG: materialized_literal called on vetoed rule '{}'",
            self.rule.name
        );
        let rule_type = Arc::new(self.rule.rule_type.clone());

        if let Some(b) = self.boolean {
            return LiteralValue {
                value: ValueKind::Boolean(b),
                lemma_type: rule_type,
            };
        }
        if let Some(number) = &self.number {
            return LiteralValue::number_with_type_from_decimal(
                decimal_from_materialized_string(number),
                rule_type,
            );
        }
        if let Some(calendar) = &self.calendar {
            use crate::literals::rational_from_parsed_decimal;
            let rational =
                rational_from_parsed_decimal(decimal_from_materialized_string(&calendar.value))
                    .expect("BUG: calendar rule result value must lift to rational");
            return LiteralValue::measure_with_type(rational, calendar.unit.clone(), rule_type);
        }
        if let Some(measure) = &self.measure {
            return literal_from_measure_map(measure, &rule_type);
        }
        if let Some(ratio) = &self.ratio {
            return literal_from_ratio_map(ratio, &rule_type);
        }
        if let Some(date) = &self.date {
            return LiteralValue {
                value: ValueKind::Date(date.clone()),
                lemma_type: rule_type,
            };
        }
        if let Some(time) = &self.time {
            return LiteralValue {
                value: ValueKind::Time(time.clone()),
                lemma_type: rule_type,
            };
        }
        if let Some(text) = &self.text {
            return LiteralValue {
                value: ValueKind::Text(text.clone()),
                lemma_type: rule_type,
            };
        }
        if let Some(range) = &self.range {
            let endpoint_type = element_type_from_range_rule(&rule_type)
                .unwrap_or_else(|| rule_type.as_ref().clone());
            let left = payload_to_literal(&range.from, &endpoint_type);
            let right = payload_to_literal(&range.to, &endpoint_type);
            return LiteralValue::range(left, right);
        }
        panic!(
            "BUG: rule '{}' materialized fields cannot reconstruct literal",
            self.rule.name
        );
    }
}

fn decimal_from_materialized_string(value: &str) -> rust_decimal::Decimal {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    Decimal::from_str(value)
        .unwrap_or_else(|_| panic!("BUG: rule result materialized string must parse as decimal"))
}

fn literal_from_measure_map(
    measure: &BTreeMap<String, String>,
    rule_type: &LemmaType,
) -> LiteralValue {
    use crate::computation::rational::checked_mul;
    use crate::literals::rational_from_parsed_decimal;

    let unit_names = rule_type
        .measure_unit_names()
        .expect("BUG: measure rule result must have declared units");
    let unit_name = unit_names
        .first()
        .expect("BUG: measure rule result type must declare at least one unit");
    let display = measure
        .get(*unit_name)
        .unwrap_or_else(|| panic!("BUG: measure map missing unit '{unit_name}'"));
    let rational = rational_from_parsed_decimal(decimal_from_materialized_string(display))
        .expect("BUG: measure rule result value must lift to rational");
    let factor = rule_type.measure_unit_factor(unit_name);
    let canonical = checked_mul(&rational, factor).unwrap_or_else(|failure| {
        panic!("BUG: measure canonicalization from materialized fields failed: {failure}")
    });
    LiteralValue::measure_with_type(
        canonical,
        (*unit_name).to_string(),
        Arc::new(rule_type.clone()),
    )
}

fn literal_from_ratio_map(ratio: &BTreeMap<String, String>, rule_type: &LemmaType) -> LiteralValue {
    use crate::computation::rational::checked_div;
    use crate::literals::rational_from_parsed_decimal;

    let units = match &rule_type.specifications {
        TypeSpecification::Ratio { units, .. } => units,
        TypeSpecification::RatioRange { .. } => {
            let element = range_element_type_specification(&rule_type.specifications)
                .expect("BUG: ratio range rule type must have ratio element specification");
            let TypeSpecification::Ratio { units, .. } = element else {
                panic!("BUG: ratio range element spec must be Ratio");
            };
            return literal_from_ratio_map(
                ratio,
                &LemmaType::primitive(TypeSpecification::Ratio {
                    minimum: None,
                    maximum: None,
                    decimals: None,
                    units,
                    help: String::new(),
                }),
            );
        }
        _ => panic!(
            "BUG: ratio rule result type must be Ratio, got {}",
            rule_type.name()
        ),
    };
    let unit = units
        .iter()
        .next()
        .expect("BUG: ratio rule result type must declare at least one unit");
    let display = ratio
        .get(&unit.name)
        .unwrap_or_else(|| panic!("BUG: ratio map missing unit '{}'", unit.name));
    let display_rational = rational_from_parsed_decimal(decimal_from_materialized_string(display))
        .expect("BUG: ratio rule result value must lift to rational");
    let canonical = checked_div(&display_rational, &unit.value).unwrap_or_else(|failure| {
        panic!("BUG: ratio canonicalization from materialized fields failed: {failure}")
    });
    LiteralValue::ratio_with_type(canonical, None, Arc::new(rule_type.clone()))
}

fn payload_to_literal(payload: &RuleResultPayload, rule_type: &LemmaType) -> LiteralValue {
    if let Some(b) = payload.boolean {
        return LiteralValue {
            value: ValueKind::Boolean(b),
            lemma_type: Arc::new(rule_type.clone()),
        };
    }
    if let Some(number) = &payload.number {
        return LiteralValue::number_with_type_from_decimal(
            decimal_from_materialized_string(number),
            Arc::new(rule_type.clone()),
        );
    }
    if let Some(calendar) = &payload.calendar {
        use crate::literals::rational_from_parsed_decimal;
        let rational =
            rational_from_parsed_decimal(decimal_from_materialized_string(&calendar.value))
                .expect("BUG: calendar payload value must lift to rational");
        return LiteralValue::measure_with_type(
            rational,
            calendar.unit.clone(),
            Arc::new(rule_type.clone()),
        );
    }
    if let Some(measure) = &payload.measure {
        return literal_from_measure_map(measure, rule_type);
    }
    if let Some(ratio) = &payload.ratio {
        return literal_from_ratio_map(ratio, rule_type);
    }
    if let Some(date) = &payload.date {
        return LiteralValue {
            value: ValueKind::Date(date.clone()),
            lemma_type: Arc::new(rule_type.clone()),
        };
    }
    if let Some(time) = &payload.time {
        return LiteralValue {
            value: ValueKind::Time(time.clone()),
            lemma_type: Arc::new(rule_type.clone()),
        };
    }
    if let Some(text) = &payload.text {
        return LiteralValue {
            value: ValueKind::Text(text.clone()),
            lemma_type: Arc::new(rule_type.clone()),
        };
    }
    panic!("BUG: range endpoint payload cannot reconstruct literal");
}

fn element_type_from_range_rule(rule_type: &LemmaType) -> Option<LemmaType> {
    range_element_type_specification(&rule_type.specifications).map(LemmaType::primitive)
}

fn endpoint_materialization_type(
    endpoint: &crate::planning::semantics::LiteralValue,
    range_element_type: &LemmaType,
) -> LemmaType {
    if endpoint.lemma_type.measure_unit_names().is_some() {
        endpoint.lemma_type.as_ref().clone()
    } else {
        range_element_type.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaterializationFailure {
    DecimalLimit,
    NumericOverflow,
    OutOfMemory,
}

fn map_materialization_failure(failure: NumericFailure) -> MaterializationFailure {
    match failure {
        NumericFailure::Overflow => MaterializationFailure::DecimalLimit,
        NumericFailure::OutOfMemory => MaterializationFailure::OutOfMemory,
        NumericFailure::DivisionByZero => {
            panic!("BUG: decimal commit encountered division by zero during materialization")
        }
        NumericFailure::Irrational => {
            panic!("BUG: decimal commit encountered irrational result during materialization")
        }
    }
}

fn map_unit_conversion_failure(failure: NumericFailure) -> MaterializationFailure {
    match failure {
        NumericFailure::Overflow => MaterializationFailure::NumericOverflow,
        NumericFailure::OutOfMemory => MaterializationFailure::OutOfMemory,
        NumericFailure::DivisionByZero => {
            panic!("BUG: unit conversion encountered division by zero during materialization")
        }
        NumericFailure::Irrational => {
            panic!("BUG: unit conversion encountered irrational result during materialization")
        }
    }
}

fn materialization_failure_message(failure: MaterializationFailure) -> &'static str {
    match failure {
        MaterializationFailure::DecimalLimit => "Calculated result exceeds decimal value limit",
        MaterializationFailure::NumericOverflow => "numeric overflow",
        MaterializationFailure::OutOfMemory => "out of memory",
    }
}

fn vetoed_rule_result_for_materialization_failure(
    rule: EvaluatedRule,
    rule_type_name: String,
    explanation: Option<Explanation>,
    failure: MaterializationFailure,
    missing_data: Vec<String>,
) -> RuleResult {
    let veto = VetoType::computation(materialization_failure_message(failure).to_string());
    RuleResult {
        rule,
        veto_detail: Some(veto.clone()),
        vetoed: true,
        display: None,
        veto_reason: Some(veto.to_string()),
        rule_type: rule_type_name,
        measure: None,
        ratio: None,
        number: None,
        boolean: None,
        text: None,
        date: None,
        time: None,
        calendar: None,
        range: None,
        explanation,
        missing_data,
    }
}

fn materialize_payload(
    literal: &crate::planning::semantics::LiteralValue,
    result_type: &LemmaType,
) -> Result<RuleResultPayload, MaterializationFailure> {
    match &literal.value {
        ValueKind::Measure(rational, sig) if literal.lemma_type.is_calendar_like() => {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_measure_signature(sig);
            let value = literal
                .lemma_type
                .try_materialize_rational_as_decimal_string(rational)
                .map_err(map_materialization_failure)?;
            Ok(RuleResultPayload {
                calendar: Some(CalendarResult {
                    value,
                    unit: unit.to_string(),
                }),
                ..RuleResultPayload::default()
            })
        }
        ValueKind::Measure(_, _) => Ok(RuleResultPayload {
            measure: Some(measure_to_unit_map(literal, result_type)?),
            ..RuleResultPayload::default()
        }),
        ValueKind::Ratio(_, _) => Ok(RuleResultPayload {
            ratio: Some(ratio_to_unit_map(literal, result_type)?),
            ..RuleResultPayload::default()
        }),
        ValueKind::Number(rational) => {
            let number = result_type
                .try_materialize_rational_as_decimal_string(rational)
                .map_err(map_materialization_failure)?;
            Ok(RuleResultPayload {
                number: Some(number),
                ..RuleResultPayload::default()
            })
        }
        ValueKind::Boolean(b) => Ok(RuleResultPayload {
            boolean: Some(*b),
            ..RuleResultPayload::default()
        }),
        ValueKind::Text(s) => Ok(RuleResultPayload {
            text: Some(s.clone()),
            ..RuleResultPayload::default()
        }),
        ValueKind::Date(d) => Ok(RuleResultPayload {
            date: Some(d.clone()),
            ..RuleResultPayload::default()
        }),
        ValueKind::Time(t) => Ok(RuleResultPayload {
            time: Some(t.clone()),
            ..RuleResultPayload::default()
        }),
        ValueKind::Range(_, _) => {
            panic!("BUG: range payload must be built at RuleResult level, not RuleResultPayload")
        }
    }
}

fn map_literal_unit_map_failure(failure: LiteralUnitMapFailure) -> MaterializationFailure {
    match failure {
        LiteralUnitMapFailure::Commit(nf) => map_materialization_failure(nf),
        LiteralUnitMapFailure::UnitConversion(nf) => map_unit_conversion_failure(nf),
    }
}

fn measure_to_unit_map(
    literal: &crate::planning::semantics::LiteralValue,
    result_type: &LemmaType,
) -> Result<BTreeMap<String, String>, MaterializationFailure> {
    result_type
        .measure_literal_in_all_units(literal)
        .map_err(map_literal_unit_map_failure)
}

fn ratio_to_unit_map(
    literal: &crate::planning::semantics::LiteralValue,
    result_type: &LemmaType,
) -> Result<BTreeMap<String, String>, MaterializationFailure> {
    result_type
        .ratio_literal_in_all_units(literal)
        .map_err(map_literal_unit_map_failure)
}

impl Response {
    /// Looks up a rule result by name.
    ///
    /// Returns an error if the rule is not found.
    pub fn get(&self, rule_name: &str) -> Result<&RuleResult, crate::error::Error> {
        self.results
            .get(rule_name)
            .ok_or_else(|| crate::error::Error::rule_not_found(rule_name, None::<String>))
    }

    pub fn add_result(&mut self, result: RuleResult) {
        self.results.insert(result.rule.name.clone(), result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::literals::DateGranularity;
    use crate::planning::semantics::{
        primitive_number, BaseMeasureVector, DataPath, LemmaType, LiteralValue, MeasureUnit,
        MeasureUnits, RatioUnit, RatioUnits, RulePath, Span, TypeExtends, TypeSpecification,
    };
    use rust_decimal::Decimal;
    use std::sync::Arc;

    fn dummy_source() -> Source {
        Source::new(
            crate::parsing::source::SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 1,
            },
        )
    }

    fn dummy_evaluated_rule(name: &str, rule_type: &LemmaType) -> EvaluatedRule {
        EvaluatedRule {
            name: name.to_string(),
            path: RulePath::new(vec![], name.to_string()),
            source_location: dummy_source(),
            rule_type: rule_type.clone(),
        }
    }

    #[test]
    fn test_response_serialization() {
        let mut results = IndexMap::new();
        results.insert(
            "test_rule".to_string(),
            RuleResult::from_operation_result(
                dummy_evaluated_rule("test_rule", primitive_number()),
                &OperationResult::from_literal(LiteralValue::number_from_decimal(Decimal::from(
                    42,
                ))),
                primitive_number(),
                None,
                Vec::new(),
            ),
        );
        let response = Response {
            spec_name: "test_spec".to_string(),
            effective: "2026-01-01".to_string(),
            spec_hash: None,
            spec_effective_from: None,
            spec_effective_to: None,
            results,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test_spec"));
        assert!(json.contains("test_rule"));
        assert!(json.contains("\"number\":\"42\""));
        assert!(!json.contains("lemma_type"));
    }

    #[test]
    fn response_number_json_never_uses_fraction_notation() {
        use crate::computation::rational::decimal_to_rational;

        let rational = decimal_to_rational(Decimal::new(1, 1) / Decimal::new(3, 1)).unwrap();
        let decimal_string = rational.try_to_decimal().unwrap().to_string();
        let mut results = IndexMap::new();
        results.insert(
            "third".to_string(),
            RuleResult::from_operation_result(
                dummy_evaluated_rule("third", primitive_number()),
                &OperationResult::from_literal(LiteralValue::number_from_decimal(
                    rational.try_to_decimal().unwrap(),
                )),
                primitive_number(),
                None,
                Vec::new(),
            ),
        );
        // Override committed decimal number field to match serialization path under test
        if let Some(rule) = results.get_mut("third") {
            rule.number = Some(decimal_string.clone());
            rule.display = Some(decimal_string);
        }

        let response = Response {
            spec_name: "test".to_string(),
            effective: "test".to_string(),
            spec_hash: None,
            spec_effective_from: None,
            spec_effective_to: None,
            results,
        };

        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
        let number = json["results"]["third"]["number"]
            .as_str()
            .expect("number must be a JSON string");
        assert!(
            !number.contains('/'),
            "API decimal string must not use fraction notation, got {number}"
        );
    }

    #[test]
    fn test_rule_result_veto() {
        let missing = RuleResult::from_operation_result(
            dummy_evaluated_rule("rule3", &LemmaType::veto_type()),
            &OperationResult::Veto(VetoType::missing_data(
                DataPath::new(vec![], "data1".to_string()),
                None,
            )),
            &LemmaType::veto_type(),
            None,
            Vec::new(),
        );
        assert!(missing.vetoed);
        assert!(missing.veto_reason.as_ref().unwrap().contains("data1"));

        let veto = RuleResult::from_operation_result(
            dummy_evaluated_rule("rule4", &LemmaType::veto_type()),
            &OperationResult::Veto(VetoType::UserDefined {
                message: Some("Vetoed".to_string()),
            }),
            &LemmaType::veto_type(),
            None,
            Vec::new(),
        );
        assert_eq!(veto.veto_reason.as_deref(), Some("Vetoed"));
    }

    #[test]
    fn materialization_out_of_memory_is_not_decimal_limit_veto() {
        let result = vetoed_rule_result_for_materialization_failure(
            dummy_evaluated_rule("rule", primitive_number()),
            "number".to_string(),
            None,
            MaterializationFailure::OutOfMemory,
            Vec::new(),
        );
        assert_eq!(result.veto_reason.as_deref(), Some("out of memory"));
        assert_ne!(
            result.veto_reason.as_deref(),
            Some("Calculated result exceeds decimal value limit")
        );
    }

    #[test]
    fn materialization_decimal_limit_uses_commit_message() {
        let result = vetoed_rule_result_for_materialization_failure(
            dummy_evaluated_rule("rule", primitive_number()),
            "number".to_string(),
            None,
            MaterializationFailure::DecimalLimit,
            Vec::new(),
        );
        assert_eq!(
            result.veto_reason.as_deref(),
            Some("Calculated result exceeds decimal value limit")
        );
    }

    fn test_money_type() -> LemmaType {
        LemmaType::new(
            "money".to_string(),
            TypeSpecification::Measure {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                units: MeasureUnits::from(vec![
                    MeasureUnit {
                        name: "eur".to_string(),
                        factor: crate::computation::rational::rational_one(),
                        derived_measure_factors: Vec::new(),
                        decomposition: BaseMeasureVector::new(),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                    MeasureUnit {
                        name: "usd".to_string(),
                        factor: crate::computation::rational::decimal_to_rational(Decimal::new(
                            91, 2,
                        ))
                        .expect("factor"),
                        derived_measure_factors: Vec::new(),
                        decomposition: BaseMeasureVector::new(),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                ]),
                traits: Vec::new(),
                decomposition: Some(BaseMeasureVector::new()),
                help: String::new(),
            },
            TypeExtends::Primitive,
        )
    }

    #[test]
    fn measure_materialization_uses_rule_type_when_expression_index_empty() {
        let money = test_money_type();
        let ten_usd = LiteralValue {
            value: ValueKind::Measure(
                crate::computation::rational::checked_mul(
                    &crate::computation::rational::decimal_to_rational(Decimal::from(10))
                        .expect("ten"),
                    &crate::computation::rational::decimal_to_rational(Decimal::new(91, 2))
                        .expect("usd factor"),
                )
                .expect("canonical usd"),
                vec![("usd".to_string(), 1)],
            ),
            lemma_type: Arc::new(money.clone()),
        };
        let result = RuleResult::from_operation_result(
            dummy_evaluated_rule("total", &money),
            &OperationResult::from_literal(ten_usd),
            &money,
            None,
            Vec::new(),
        );
        let measure = result.measure.expect("measure map");
        assert_eq!(measure.get("usd"), Some(&"10.00".to_string()));
        assert!(measure.contains_key("eur"));
    }

    #[test]
    fn test_measure_materialization_multi_unit() {
        let money = test_money_type();
        let ten_eur = LiteralValue {
            value: ValueKind::Measure(
                crate::computation::rational::decimal_to_rational(Decimal::from(10)).expect("ten"),
                vec![],
            ),
            lemma_type: Arc::new(money.clone()),
        };
        let result = RuleResult::from_operation_result(
            dummy_evaluated_rule("total", &money),
            &OperationResult::from_literal(ten_eur),
            &money,
            None,
            Vec::new(),
        );
        let measure = result.measure.expect("measure map");
        assert_eq!(measure.get("eur"), Some(&"10.00".to_string()));
        assert_eq!(measure.get("usd"), Some(&"10.99".to_string()));
    }

    #[test]
    fn measure_materialization_respects_decimals_on_unit_conversion() {
        let money = LemmaType::new(
            "money".to_string(),
            TypeSpecification::Measure {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                units: MeasureUnits::from(vec![
                    MeasureUnit {
                        name: "eur".to_string(),
                        factor: crate::computation::rational::rational_one(),
                        derived_measure_factors: Vec::new(),
                        decomposition: BaseMeasureVector::new(),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                    MeasureUnit {
                        name: "usd".to_string(),
                        factor: crate::computation::rational::decimal_to_rational(Decimal::new(
                            84, 2,
                        ))
                        .expect("usd factor"),
                        derived_measure_factors: Vec::new(),
                        decomposition: BaseMeasureVector::new(),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                ]),
                traits: Vec::new(),
                decomposition: Some(BaseMeasureVector::new()),
                help: String::new(),
            },
            TypeExtends::Primitive,
        );
        let three_twelve_eur = LiteralValue {
            value: ValueKind::Measure(
                crate::computation::rational::decimal_to_rational(Decimal::new(312, 2))
                    .expect("3.12 eur canonical"),
                vec![],
            ),
            lemma_type: Arc::new(money.clone()),
        };
        let result = RuleResult::from_operation_result(
            dummy_evaluated_rule("delivery_cost", &money),
            &OperationResult::from_literal(three_twelve_eur),
            &money,
            None,
            Vec::new(),
        );
        let measure = result.measure.expect("measure map");
        assert_eq!(measure.get("eur"), Some(&"3.12".to_string()));
        assert_eq!(measure.get("usd"), Some(&"3.71".to_string()));
    }

    #[test]
    fn test_ratio_materialization_multi_unit() {
        let ratio_type = LemmaType::new(
            "rate".to_string(),
            TypeSpecification::Ratio {
                minimum: None,
                maximum: None,
                decimals: None,
                units: RatioUnits::from(vec![
                    RatioUnit {
                        name: "percent".to_string(),
                        value: crate::computation::rational::decimal_to_rational(Decimal::from(
                            100,
                        ))
                        .expect("percent"),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                    RatioUnit {
                        name: "basis_points".to_string(),
                        value: crate::computation::rational::decimal_to_rational(Decimal::from(
                            10_000,
                        ))
                        .expect("bp"),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                ]),
                help: String::new(),
            },
            TypeExtends::Primitive,
        );
        let half = crate::computation::rational::rational_new(1, 2);
        let lit = LiteralValue {
            value: ValueKind::Ratio(half, Some("percent".to_string())),
            lemma_type: Arc::new(ratio_type.clone()),
        };
        let result = RuleResult::from_operation_result(
            dummy_evaluated_rule("rate_out", &ratio_type),
            &OperationResult::from_literal(lit),
            &ratio_type,
            None,
            Vec::new(),
        );
        let ratio = result.ratio.expect("ratio map");
        assert_eq!(ratio.get("percent"), Some(&"50".to_string()));
        assert_eq!(ratio.get("basis_points"), Some(&"5000".to_string()));
    }

    #[test]
    fn test_measure_materialization_cross_spec_import() {
        use crate::parsing::source::SourceType;
        use crate::Engine;

        let mut engine = Engine::new();
        engine
            .load([(
                SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("t.lemma"))),
                r#"
spec consumer 2025-01-01
uses d: dep 2025-10-01
rule out: d.doubled

spec dep 2025-01-01
uses c: child 2025-06-01
data money: c.money
data p: 5 usd
rule doubled: p * 2

spec child 2025-01-01
data money: measure
 -> unit eur 1.00
 -> decimals 2

spec child 2025-06-01
data money: measure
 -> unit eur 1.00
 -> unit usd 0.91
 -> decimals 2
"#
                .to_string(),
            )])
            .expect("load");
        let effective = crate::literals::DateTimeValue {
            year: 2025,
            month: 3,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };
        let response = engine
            .run(
                None,
                "consumer",
                Some(&effective),
                std::collections::HashMap::new(),
                None,
                false,
            )
            .expect("run");
        let out = response.results.get("out").expect("out rule");
        assert!(!out.vetoed);
        let measure = out.measure.as_ref().expect("measure map");
        assert!(measure.contains_key("usd"));
        assert!(measure.contains_key("eur"));
    }

    #[test]
    fn materialized_literal_roundtrips_number() {
        let literal = LiteralValue::number_from_decimal(Decimal::from(42));
        let rule_result = RuleResult::from_operation_result(
            dummy_evaluated_rule("answer", primitive_number()),
            &OperationResult::from_literal(literal.clone()),
            primitive_number(),
            None,
            Vec::new(),
        );
        assert_eq!(rule_result.materialized_literal(), literal);
    }

    #[test]
    fn materialized_literal_roundtrips_measure() {
        let money = test_money_type();
        let literal = LiteralValue::measure_with_type(
            crate::computation::rational::rational_new(60, 1),
            "eur".into(),
            Arc::new(money.clone()),
        );
        let rule_result = RuleResult::from_operation_result(
            dummy_evaluated_rule("pay", &money),
            &OperationResult::from_literal(literal.clone()),
            &money,
            None,
            Vec::new(),
        );
        assert_eq!(rule_result.materialized_literal(), literal);
    }
}
