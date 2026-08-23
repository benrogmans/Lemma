use crate::computation::{OperationResult, VetoType};
use crate::evaluation::explanations::Explanation;

use crate::parsing::ast::DateTimeValue;
use crate::planning::semantics::{LemmaType, LiteralValue, RulePath, Source};
use crate::result_value::{
    rule_result_value_failure_message, rule_result_value_from_literal, RuleResultValue,
    RuleResultValueFailure,
};
use indexmap::IndexMap;
use serde::Serialize;

/// Rule info with resolved expressions for use in evaluation response.
/// Evaluation uses only semantics types; no parsing types.
#[derive(Debug, Clone, Serialize)]
pub struct EvaluatedRule {
    pub name: String,
    pub path: RulePath,
    pub source_location: Source,
    pub rule_type: LemmaType,
}

/// Response from evaluating a Lemma spec
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    #[serde(rename = "spec")]
    pub spec_name: String,
    pub effective: String,
    /// Declared temporal window `[spec_effective_from, spec_effective_to)` of the
    /// resolved spec version. Set by [`crate::Engine::run`] after evaluation;
    /// `None` here (evaluation-internal construction) until that assignment.
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
    pub veto_reason: Option<String>,
    pub rule_type: String,

    /// Flattened value fields, including `display` when the rule is not vetoed.
    #[serde(flatten)]
    pub value: Option<RuleResultValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<Explanation>,
    /// Unbound caller data paths still live for this rule under the current run data
    /// (`DataPath::input_key` strings, same keys as `Show.data`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing_data: Vec<String>,
}

impl RuleResult {
    /// Engine-rendered display string from the flattened [`RuleResultValue`].
    #[must_use]
    pub fn display(&self) -> Option<&str> {
        self.value
            .as_ref()
            .and_then(|value| value.display.as_deref())
    }

    /// True when this rule still waits on unbound inputs (`MissingData` veto).
    ///
    /// Value and non-`MissingData` vetoes are settled answers; leftover live keys in
    /// [`Self::missing_data`] must not drive prompts or human "Missing data" display.
    #[must_use]
    pub fn awaits_missing_data(&self) -> bool {
        matches!(
            self.veto_detail.as_ref(),
            Some(VetoType::MissingData { .. })
        )
    }

    /// Unbound caller data paths still live for this rule (`DataPath::input_key`).
    #[must_use]
    pub fn missing_data(&self) -> &[String] {
        &self.missing_data
    }

    /// Build a [`RuleResult`] for API output from a rule evaluation result.
    ///
    /// Measure and ratio payloads expand into every unit declared on `rule_type`.
    pub fn from_operation_result(
        rule: EvaluatedRule,
        operation_result: &OperationResult,
        rule_type: &LemmaType,
        explanation: Option<Explanation>,
        missing_data: Vec<String>,
    ) -> Self {
        match operation_result {
            OperationResult::Veto(VetoType::MissingData { data, .. }) => {
                let key = data.input_key();
                if !missing_data.iter().any(|listed| listed == &key) {
                    panic!("BUG: MissingData path {key} not in missing_data {missing_data:?}");
                }
            }
            _ => {
                if !missing_data.is_empty() {
                    panic!(
                        "BUG: missing_data must be empty when result is not MissingData: {missing_data:?}"
                    );
                }
            }
        }
        let rule_type_name = rule_type.name().to_string();
        match operation_result {
            OperationResult::Veto(veto) => Self {
                rule,
                veto_detail: Some(veto.clone()),
                vetoed: true,
                veto_reason: match &veto {
                    VetoType::UserDefined { message: None } => None,
                    _ => Some(veto.to_string()),
                },
                rule_type: rule_type_name,
                value: None,
                explanation,
                missing_data,
            },
            OperationResult::Value(literal) => {
                match rule_result_value_from_literal(literal, rule_type) {
                    Ok(value) => Self {
                        rule,
                        veto_detail: None,
                        vetoed: false,
                        veto_reason: None,
                        rule_type: rule_type_name,
                        value: Some(value),
                        explanation,
                        missing_data,
                    },
                    Err(failure) => vetoed_rule_result_for_rule_result_value_failure(
                        rule,
                        rule_type,
                        explanation,
                        failure,
                        missing_data,
                    ),
                }
            }
        }
    }

    /// Reconstruct the evaluated [`LiteralValue`] from committed [`RuleResultValue`] fields.
    ///
    /// Panics if the rule is vetoed or fields cannot be reconstructed.
    pub fn to_literal(&self) -> LiteralValue {
        assert!(
            !self.vetoed,
            "BUG: to_literal called on vetoed rule '{}'",
            self.rule.name
        );
        let value = self
            .value
            .as_ref()
            .unwrap_or_else(|| panic!("BUG: non-vetoed rule '{}' missing value", self.rule.name));
        value.to_literal(&self.rule.rule_type)
    }
}

fn vetoed_rule_result_for_rule_result_value_failure(
    rule: EvaluatedRule,
    rule_type: &LemmaType,
    explanation: Option<Explanation>,
    failure: RuleResultValueFailure,
    missing_data: Vec<String>,
) -> RuleResult {
    RuleResult::from_operation_result(
        rule,
        &OperationResult::Veto(VetoType::computation(
            rule_result_value_failure_message(failure).to_string(),
        )),
        rule_type,
        explanation,
        missing_data,
    )
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
    use crate::parsing::ast::Span;
    use crate::planning::semantics::{
        primitive_number_arc, BaseMeasureVector, DataPath, LemmaType, LiteralValue, MeasureUnit,
        MeasureUnits, RatioUnit, RatioUnits, RulePath, TypeExtends, TypeSpecification, ValueKind,
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
                dummy_evaluated_rule("test_rule", primitive_number_arc().as_ref()),
                &OperationResult::from_literal(LiteralValue::number_from_decimal(Decimal::from(
                    42,
                ))),
                primitive_number_arc().as_ref(),
                None,
                Vec::new(),
            ),
        );
        let response = Response {
            spec_name: "test_spec".to_string(),
            effective: "2026-01-01".to_string(),
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
                dummy_evaluated_rule("third", primitive_number_arc().as_ref()),
                &OperationResult::from_literal(LiteralValue::number_from_decimal(
                    rational.try_to_decimal().unwrap(),
                )),
                primitive_number_arc().as_ref(),
                None,
                Vec::new(),
            ),
        );
        // Override committed decimal number field to match serialization path under test
        if let Some(rule) = results.get_mut("third") {
            rule.value = Some(crate::result_value::RuleResultValue {
                display: Some(decimal_string.clone()),
                number: Some(decimal_string.clone()),
                ..Default::default()
            });
        }

        let response = Response {
            spec_name: "test".to_string(),
            effective: "test".to_string(),
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
            vec!["data1".to_string()],
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

    /// Attach hole: MissingData + empty missing_data is a BUG, not a silent stall.
    #[test]
    #[should_panic(expected = "BUG: MissingData")]
    fn missing_data_veto_must_not_attach_empty_missing_data_list() {
        RuleResult::from_operation_result(
            dummy_evaluated_rule("rule3", &LemmaType::veto_type()),
            &OperationResult::Veto(VetoType::missing_data(
                DataPath::new(vec![], "data1".to_string()),
                None,
            )),
            &LemmaType::veto_type(),
            None,
            Vec::new(),
        );
    }

    #[test]
    #[should_panic(expected = "BUG: MissingData")]
    fn missing_data_veto_must_include_veto_path_in_list() {
        RuleResult::from_operation_result(
            dummy_evaluated_rule("rule3", &LemmaType::veto_type()),
            &OperationResult::Veto(VetoType::missing_data(
                DataPath::new(vec![], "data1".to_string()),
                None,
            )),
            &LemmaType::veto_type(),
            None,
            vec!["other".to_string()],
        );
    }

    #[test]
    #[should_panic(expected = "BUG: missing_data must be empty")]
    fn non_missing_data_result_must_not_attach_leftover_missing_data() {
        RuleResult::from_operation_result(
            dummy_evaluated_rule("rule4", &LemmaType::veto_type()),
            &OperationResult::Veto(VetoType::UserDefined {
                message: Some("Vetoed".to_string()),
            }),
            &LemmaType::veto_type(),
            None,
            vec!["leftover".to_string()],
        );
    }

    #[test]
    fn rule_result_value_out_of_memory_is_not_decimal_limit_veto() {
        let result = vetoed_rule_result_for_rule_result_value_failure(
            dummy_evaluated_rule("rule", primitive_number_arc().as_ref()),
            primitive_number_arc().as_ref(),
            None,
            RuleResultValueFailure::OutOfMemory,
            Vec::new(),
        );
        assert_eq!(result.veto_reason.as_deref(), Some("out of memory"));
        assert_ne!(
            result.veto_reason.as_deref(),
            Some("Calculated result exceeds decimal value limit")
        );
    }

    #[test]
    fn rule_result_value_decimal_limit_uses_commit_message() {
        let result = vetoed_rule_result_for_rule_result_value_failure(
            dummy_evaluated_rule("rule", primitive_number_arc().as_ref()),
            primitive_number_arc().as_ref(),
            None,
            RuleResultValueFailure::DecimalLimit,
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
    fn measure_rule_result_value_uses_rule_type_when_expression_index_empty() {
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
        let measure = result
            .value
            .as_ref()
            .expect("value")
            .measure
            .clone()
            .expect("measure map");
        assert_eq!(measure.get("usd"), Some(&"10.00".to_string()));
        assert!(measure.contains_key("eur"));
    }

    #[test]
    fn test_measure_rule_result_value_multi_unit() {
        let money = test_money_type();
        let ten_eur = LiteralValue {
            value: ValueKind::Measure(
                crate::computation::rational::decimal_to_rational(Decimal::from(10)).expect("ten"),
                vec![("eur".to_string(), 1)],
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
        let measure = result
            .value
            .as_ref()
            .expect("value")
            .measure
            .clone()
            .expect("measure map");
        assert_eq!(measure.get("eur"), Some(&"10.00".to_string()));
        assert_eq!(measure.get("usd"), Some(&"10.99".to_string()));
    }

    #[test]
    fn measure_rule_result_value_respects_decimals_on_unit_conversion() {
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
                vec![("eur".to_string(), 1)],
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
        let measure = result
            .value
            .as_ref()
            .expect("value")
            .measure
            .clone()
            .expect("measure map");
        assert_eq!(measure.get("eur"), Some(&"3.12".to_string()));
        assert_eq!(measure.get("usd"), Some(&"3.71".to_string()));
    }

    #[test]
    fn test_ratio_rule_result_value_multi_unit() {
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
        let ratio = result
            .value
            .as_ref()
            .expect("value")
            .ratio
            .clone()
            .expect("ratio map");
        assert_eq!(ratio.get("percent"), Some(&"50".to_string()));
        assert_eq!(ratio.get("basis_points"), Some(&"5000".to_string()));
    }

    #[test]
    fn test_measure_rule_result_value_cross_spec_import() {
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
 -> unit eur: 1.00
 -> decimals 2

spec child 2025-06-01
data money: measure
 -> unit eur: 1.00
 -> unit usd: 0.91
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
        let measure = out
            .value
            .as_ref()
            .expect("value")
            .measure
            .as_ref()
            .expect("measure map");
        assert!(measure.contains_key("usd"));
        assert!(measure.contains_key("eur"));
    }

    #[test]
    fn to_literal_roundtrips_number() {
        let literal = LiteralValue::number_from_decimal(Decimal::from(42));
        let rule_result = RuleResult::from_operation_result(
            dummy_evaluated_rule("answer", primitive_number_arc().as_ref()),
            &OperationResult::from_literal(literal.clone()),
            primitive_number_arc().as_ref(),
            None,
            Vec::new(),
        );
        assert_eq!(rule_result.to_literal(), literal);
    }

    #[test]
    fn to_literal_roundtrips_measure() {
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
        assert_eq!(rule_result.to_literal(), literal);
    }
}
