use crate::evaluation::evaluation_trace::EvaluationTrace;
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::parsing::ast::DateTimeValue;
use crate::planning::semantics::{
    range_element_type_specification, DataPath, LemmaType, RulePath, SemanticDateTime,
    SemanticTime, Source, TypeSpecification, ValueKind,
};
use indexmap::IndexMap;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
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

/// Grouped data from a specific spec (semantics types only).
#[derive(Debug, Clone, Serialize)]
pub struct DataGroup {
    pub data_path: String,
    pub referencing_data_name: String,
    pub data: Vec<crate::planning::semantics::Data>,
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
    pub quantity: Option<BTreeMap<String, String>>,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<DataGroup>,
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
    pub quantity: Option<BTreeMap<String, String>>,
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
    #[serde(rename = "explanation")]
    pub trace: Option<EvaluationTrace>,
}

impl RuleResult {
    /// Materialize a rule evaluation result for the wire/API.
    ///
    /// `expression_units` is the plan's expression-scope index
    /// ([`crate::planning::ExecutionPlan::expression_unit_index`]).
    /// Declared units on `rule_type` are used first; the expression index covers compound signatures.
    pub fn from_operation_result(
        rule: EvaluatedRule,
        operation_result: OperationResult,
        rule_type: &LemmaType,
        expression_units: &std::collections::HashMap<String, Arc<LemmaType>>,
        trace: Option<EvaluationTrace>,
    ) -> Self {
        let rule_type_name = rule_type.name().to_string();
        match operation_result {
            OperationResult::Veto(veto) => Self {
                rule,
                veto_detail: Some(veto.clone()),
                vetoed: true,
                display: None,
                veto_reason: Some(veto.to_string()),
                rule_type: rule_type_name,
                quantity: None,
                ratio: None,
                number: None,
                boolean: None,
                text: None,
                date: None,
                time: None,
                calendar: None,
                range: None,
                trace,
            },
            OperationResult::Value(literal) => {
                let mut result = Self {
                    rule,
                    veto_detail: None,
                    vetoed: false,
                    display: None,
                    veto_reason: None,
                    rule_type: rule_type_name,
                    quantity: None,
                    ratio: None,
                    number: None,
                    boolean: None,
                    text: None,
                    date: None,
                    time: None,
                    calendar: None,
                    range: None,
                    trace,
                };
                match &literal.value {
                    ValueKind::Range(from, to) => {
                        let endpoint_type = element_type_from_range_rule(rule_type)
                            .unwrap_or_else(|| rule_type.clone());
                        result.range = Some(RangeResult {
                            from: materialize_payload(
                                from,
                                &endpoint_materialization_type(from, &endpoint_type),
                                expression_units,
                            ),
                            to: materialize_payload(
                                to,
                                &endpoint_materialization_type(to, &endpoint_type),
                                expression_units,
                            ),
                        });
                        result.display = Some(literal.to_string());
                    }
                    _ => {
                        let payload =
                            materialize_payload(literal.as_ref(), rule_type, expression_units);
                        result.quantity = payload.quantity;
                        result.ratio = payload.ratio;
                        result.number = payload.number;
                        result.boolean = payload.boolean;
                        result.text = payload.text;
                        result.date = payload.date;
                        result.time = payload.time;
                        result.calendar = payload.calendar;
                        result.display = Some(literal.to_string());
                    }
                }
                result
            }
        }
    }
}

fn element_type_from_range_rule(rule_type: &LemmaType) -> Option<LemmaType> {
    range_element_type_specification(&rule_type.specifications).map(LemmaType::primitive)
}

fn endpoint_materialization_type(
    endpoint: &crate::planning::semantics::LiteralValue,
    range_element_type: &LemmaType,
) -> LemmaType {
    if endpoint.lemma_type.quantity_unit_names().is_some() {
        endpoint.lemma_type.as_ref().clone()
    } else {
        range_element_type.clone()
    }
}

fn materialize_payload(
    literal: &crate::planning::semantics::LiteralValue,
    result_type: &LemmaType,
    _expression_units: &std::collections::HashMap<String, Arc<LemmaType>>,
) -> RuleResultPayload {
    match &literal.value {
        ValueKind::Quantity(rational, sig) if literal.lemma_type.is_calendar_like() => {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(sig);
            RuleResultPayload {
                calendar: Some(CalendarResult {
                    value: rational_to_wire_string(rational),
                    unit: unit.to_string(),
                }),
                ..RuleResultPayload::default()
            }
        }
        ValueKind::Quantity(_, _) => RuleResultPayload {
            quantity: Some(quantity_to_unit_map(literal, result_type)),
            ..RuleResultPayload::default()
        },
        ValueKind::Ratio(_, _) => RuleResultPayload {
            ratio: Some(ratio_to_unit_map(literal, result_type)),
            ..RuleResultPayload::default()
        },
        ValueKind::Number(rational) => RuleResultPayload {
            number: Some(rational_to_wire_string(rational)),
            ..RuleResultPayload::default()
        },
        ValueKind::Boolean(b) => RuleResultPayload {
            boolean: Some(*b),
            ..RuleResultPayload::default()
        },
        ValueKind::Text(s) => RuleResultPayload {
            text: Some(s.clone()),
            ..RuleResultPayload::default()
        },
        ValueKind::Date(d) => RuleResultPayload {
            date: Some(d.clone()),
            ..RuleResultPayload::default()
        },
        ValueKind::Time(t) => RuleResultPayload {
            time: Some(t.clone()),
            ..RuleResultPayload::default()
        },
        ValueKind::Range(_, _) => {
            panic!("BUG: range payload must be built at RuleResult level, not RuleResultPayload")
        }
    }
}

fn rational_to_wire_string(rational: &crate::computation::rational::RationalInteger) -> String {
    crate::literals::rational_to_serialized_str(rational)
        .expect("BUG: rule result magnitude must serialize to decimal string")
}

fn quantity_to_unit_map(
    literal: &crate::planning::semantics::LiteralValue,
    result_type: &LemmaType,
) -> BTreeMap<String, String> {
    use crate::computation::rational::checked_div;

    let unit_names = result_type
        .quantity_unit_names()
        .expect("BUG: rule result quantity must have declared units");
    let ValueKind::Quantity(magnitude, _signature) = &literal.value else {
        panic!("BUG: quantity_to_unit_map called with non-quantity value");
    };
    let mut map = BTreeMap::new();
    for unit_name in unit_names {
        let to_factor = result_type.quantity_unit_factor(unit_name);
        let converted = checked_div(magnitude, to_factor).unwrap_or_else(|failure| {
            panic!(
                "BUG: quantity unit conversion to '{}' failed at rule result materialization: {}",
                unit_name, failure
            )
        });
        map.insert(unit_name.to_string(), rational_to_wire_string(&converted));
    }
    map
}

fn ratio_to_unit_map(
    literal: &crate::planning::semantics::LiteralValue,
    result_type: &LemmaType,
) -> BTreeMap<String, String> {
    use crate::computation::rational::checked_mul;

    let units = match &result_type.specifications {
        TypeSpecification::Ratio { units, .. } => units,
        TypeSpecification::RatioRange { .. } => {
            let element = range_element_type_specification(&result_type.specifications)
                .expect("BUG: ratio range rule type must have ratio element specification");
            let TypeSpecification::Ratio { units, .. } = element else {
                panic!("BUG: ratio range element spec must be Ratio");
            };
            return ratio_to_unit_map(
                literal,
                &LemmaType::primitive(TypeSpecification::Ratio {
                    minimum: None,
                    maximum: None,
                    decimals: None,
                    units,
                    help: String::new(),
                }),
            );
        }
        _ => {
            panic!(
                "BUG: ratio_to_unit_map called with non-ratio type {}",
                result_type.name()
            );
        }
    };
    let ValueKind::Ratio(canonical, _) = &literal.value else {
        panic!("BUG: ratio_to_unit_map called with non-ratio value");
    };
    if units.is_empty() {
        panic!(
            "BUG: rule result ratio type '{}' must have declared units",
            result_type.name()
        );
    }
    let mut map = BTreeMap::new();
    for unit in units.iter() {
        let display = checked_mul(canonical, &unit.value).unwrap_or_else(|failure| {
            panic!(
                "BUG: ratio unit conversion to '{}' failed at rule result materialization: {}",
                unit.name, failure
            )
        });
        map.insert(unit.name.clone(), rational_to_wire_string(&display));
    }
    map
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

    pub fn filter_rules(&mut self, rule_names: &[String]) {
        self.results.retain(|name, _| rule_names.contains(name));
    }

    /// All [`DataPath`]s reported as missing by any rule result (`VetoType::MissingData`).
    #[must_use]
    pub fn missing_data(&self) -> BTreeSet<DataPath> {
        self.missing_data_ordered().into_iter().collect()
    }

    /// [`DataPath`]s with `MissingData` vetos, in **rule result order** (matches evaluation order),
    /// first occurrence only.
    #[must_use]
    pub fn missing_data_ordered(&self) -> Vec<DataPath> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for rr in self.results.values() {
            if let Some(VetoType::MissingData { data }) = &rr.veto_detail {
                if seen.insert(data.clone()) {
                    out.push(data.clone());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::semantics::{
        primitive_boolean, primitive_number, BaseQuantityVector, LemmaType, LiteralValue,
        QuantityUnit, QuantityUnits, RatioUnit, RatioUnits, RulePath, Span, TypeExtends,
        TypeSpecification,
    };
    use rust_decimal::Decimal;
    use std::collections::HashMap;
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

    fn dummy_evaluated_rule(name: &str) -> EvaluatedRule {
        EvaluatedRule {
            name: name.to_string(),
            path: RulePath::new(vec![], name.to_string()),
            source_location: dummy_source(),
            rule_type: primitive_number().clone(),
        }
    }

    #[test]
    fn test_response_serialization() {
        let mut results = IndexMap::new();
        let expression_units = std::collections::HashMap::new();
        results.insert(
            "test_rule".to_string(),
            RuleResult::from_operation_result(
                dummy_evaluated_rule("test_rule"),
                OperationResult::Value(Box::new(LiteralValue::number_from_decimal(Decimal::from(
                    42,
                )))),
                primitive_number(),
                &expression_units,
                None,
            ),
        );
        let response = Response {
            spec_name: "test_spec".to_string(),
            effective: "2026-01-01".to_string(),
            spec_hash: None,
            spec_effective_from: None,
            spec_effective_to: None,
            data: vec![],
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
        use crate::computation::rational::{commit_rational_to_decimal, decimal_to_rational};

        let rational = decimal_to_rational(Decimal::new(1, 1) / Decimal::new(3, 1)).unwrap();
        let wire_number = commit_rational_to_decimal(&rational).unwrap().to_string();
        let mut results = IndexMap::new();
        results.insert(
            "third".to_string(),
            RuleResult::from_operation_result(
                dummy_evaluated_rule("third"),
                OperationResult::Value(Box::new(LiteralValue::number_from_decimal(
                    commit_rational_to_decimal(&rational).unwrap(),
                ))),
                primitive_number(),
                &std::collections::HashMap::new(),
                None,
            ),
        );
        // Override wire number field to match serialization path under test
        if let Some(rule) = results.get_mut("third") {
            rule.number = Some(wire_number.clone());
            rule.display = Some(wire_number);
        }

        let response = Response {
            spec_name: "test".to_string(),
            effective: "test".to_string(),
            spec_hash: None,
            spec_effective_from: None,
            spec_effective_to: None,
            data: vec![],
            results,
        };

        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
        let number = json["results"]["third"]["number"]
            .as_str()
            .expect("number must be a JSON string");
        assert!(
            !number.contains('/'),
            "wire number must not use fraction notation, got {number}"
        );
    }

    #[test]
    fn test_response_filter_rules() {
        let mut results = IndexMap::new();
        let expression_units = std::collections::HashMap::new();
        results.insert(
            "rule1".to_string(),
            RuleResult::from_operation_result(
                dummy_evaluated_rule("rule1"),
                OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
                primitive_boolean(),
                &expression_units,
                None,
            ),
        );
        results.insert(
            "rule2".to_string(),
            RuleResult::from_operation_result(
                dummy_evaluated_rule("rule2"),
                OperationResult::Value(Box::new(LiteralValue::from_bool(false))),
                primitive_boolean(),
                &expression_units,
                None,
            ),
        );
        let mut response = Response {
            spec_name: "test_spec".to_string(),
            effective: "2026-01-01".to_string(),
            spec_hash: None,
            spec_effective_from: None,
            spec_effective_to: None,
            data: vec![],
            results,
        };

        response.filter_rules(&["rule1".to_string()]);

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results.values().next().unwrap().rule.name, "rule1");
    }

    #[test]
    fn test_rule_result_veto() {
        let expression_units = std::collections::HashMap::new();
        let missing = RuleResult::from_operation_result(
            dummy_evaluated_rule("rule3"),
            OperationResult::Veto(VetoType::MissingData {
                data: DataPath::new(vec![], "data1".to_string()),
            }),
            &LemmaType::veto_type(),
            &expression_units,
            None,
        );
        assert!(missing.vetoed);
        assert!(missing.veto_reason.as_ref().unwrap().contains("data1"));

        let veto = RuleResult::from_operation_result(
            dummy_evaluated_rule("rule4"),
            OperationResult::Veto(VetoType::UserDefined {
                message: Some("Vetoed".to_string()),
            }),
            &LemmaType::veto_type(),
            &expression_units,
            None,
        );
        assert_eq!(veto.veto_reason.as_deref(), Some("Vetoed"));
    }

    fn test_money_type() -> LemmaType {
        LemmaType::new(
            "money".to_string(),
            TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: Some(2),
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
                decomposition: Some(BaseQuantityVector::new()),
                help: String::new(),
            },
            TypeExtends::Primitive,
        )
    }

    #[test]
    fn quantity_materialization_uses_rule_type_when_expression_index_empty() {
        let money = test_money_type();
        let ten_usd = LiteralValue {
            value: ValueKind::Quantity(
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
        let expression_units = HashMap::new();
        let result = RuleResult::from_operation_result(
            dummy_evaluated_rule("total"),
            OperationResult::Value(Box::new(ten_usd)),
            &money,
            &expression_units,
            None,
        );
        let quantity = result.quantity.expect("quantity map");
        assert_eq!(quantity.get("usd"), Some(&"10".to_string()));
        assert!(quantity.contains_key("eur"));
    }

    #[test]
    fn test_quantity_materialization_multi_unit() {
        let money = test_money_type();
        let expression_units = HashMap::new();
        let ten_eur = LiteralValue {
            value: ValueKind::Quantity(
                crate::computation::rational::decimal_to_rational(Decimal::from(10)).expect("ten"),
                vec![],
            ),
            lemma_type: Arc::new(money.clone()),
        };
        let result = RuleResult::from_operation_result(
            dummy_evaluated_rule("total"),
            OperationResult::Value(Box::new(ten_eur)),
            &money,
            &expression_units,
            None,
        );
        let quantity = result.quantity.expect("quantity map");
        assert_eq!(quantity.get("eur"), Some(&"10".to_string()));
        assert!(quantity.contains_key("usd"));
        assert!(quantity["usd"].starts_with("10.9"));
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
                        default_magnitude: None,
                    },
                    RatioUnit {
                        name: "basis_points".to_string(),
                        value: crate::computation::rational::decimal_to_rational(Decimal::from(
                            10_000,
                        ))
                        .expect("bp"),
                        minimum: None,
                        maximum: None,
                        default_magnitude: None,
                    },
                ]),
                help: String::new(),
            },
            TypeExtends::Primitive,
        );
        let expression_units = HashMap::new();
        let half = crate::computation::rational::RationalInteger::new(1, 2);
        let lit = LiteralValue {
            value: ValueKind::Ratio(half, Some("percent".to_string())),
            lemma_type: Arc::new(ratio_type.clone()),
        };
        let result = RuleResult::from_operation_result(
            dummy_evaluated_rule("rate_out"),
            OperationResult::Value(Box::new(lit)),
            &ratio_type,
            &expression_units,
            None,
        );
        let ratio = result.ratio.expect("ratio map");
        assert_eq!(ratio.get("percent"), Some(&"50".to_string()));
        assert_eq!(ratio.get("basis_points"), Some(&"5000".to_string()));
    }

    #[test]
    fn test_quantity_materialization_cross_spec_import() {
        use crate::parsing::source::SourceType;
        use crate::Engine;

        let mut engine = Engine::new();
        engine
            .load(
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
data money: quantity
 -> unit eur 1.00
 -> decimals 2

spec child 2025-06-01
data money: quantity
 -> unit eur 1.00
 -> unit usd 0.91
 -> decimals 2
"#,
                SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("t.lemma"))),
            )
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
        };
        let response = engine
            .run(
                None,
                "consumer",
                Some(&effective),
                std::collections::HashMap::new(),
                false,
            )
            .expect("run");
        let out = response.results.get("out").expect("out rule");
        assert!(!out.vetoed);
        let quantity = out.quantity.as_ref().expect("quantity map");
        assert!(quantity.contains_key("usd"));
        assert!(quantity.contains_key("eur"));
    }
}
