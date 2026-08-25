use crate::computation::{OperationResult, VetoType};
use crate::planning::execution_plan::{validate_value_against_type, ExecutionPlan};
use crate::planning::semantics::{
    number_with_unit_to_value_kind, parse_value_from_string, parser_value_to_value_kind,
    DataDefinition, DataPath, LemmaType, LiteralValue, Source, TypeSpecification, ValueKind,
};
use crate::Error;
use crate::ResourceLimits;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

/// Typed data value from a client (CLI/WASM). JSON parsing stays outside [`parse_data_value`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDataValue {
    /// Raw string, parsed against the target type's specification by [`parse_value_from_string`].
    String(String),
    Boolean(bool),
    MeasureMap(BTreeMap<String, String>),
    RatioMap(BTreeMap<String, String>),
}

impl RunDataValue {
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Self::String(s) => s.trim().is_empty(),
            Self::MeasureMap(map) | Self::RatioMap(map) => map.is_empty(),
            Self::Boolean(_) => false,
        }
    }
}

/// Parse one JSON value into a [`RunDataValue`] (SDK / MCP / WASM wire shape).
pub fn run_data_value_from_json_value(value: serde_json::Value) -> Result<RunDataValue, String> {
    match value {
        serde_json::Value::String(s) => Ok(RunDataValue::String(s)),
        serde_json::Value::Bool(b) => Ok(RunDataValue::Boolean(b)),
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                Ok(RunDataValue::String(n.to_string()))
            } else {
                Err("decimal values must be passed as strings to preserve exactness".to_string())
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                return Err("data value object must not be empty".to_string());
            }
            if obj.len() == 2 && obj.contains_key("value") && obj.contains_key("unit") {
                return Err(
                    "the {value, unit} object shape is not supported; use a unit map like {\"eur\": \"84\"}"
                        .to_string(),
                );
            }
            if obj.values().all(|v| v.is_string()) {
                let map: BTreeMap<String, String> = obj
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            v.as_str()
                                .expect("BUG: object values checked as strings")
                                .to_string(),
                        )
                    })
                    .collect();
                return Ok(RunDataValue::MeasureMap(map));
            }
            Err("data value object must be a unit map with string magnitudes".to_string())
        }
        serde_json::Value::Null => Err("data value must not be null".to_string()),
        serde_json::Value::Array(_) => Err("data value must not be an array".to_string()),
    }
}

/// Convert SDK/MCP/WASM `data` object to string map for [`crate::Engine::run`].
///
/// Single-entry unit maps become convenience strings (`"84 eur"`). Multi-key maps
/// are rejected until `Engine::run` accepts typed [`RunDataValue`] directly.
pub fn parse_run_data_object(
    data: &Option<serde_json::Value>,
) -> Result<HashMap<String, String>, String> {
    let Some(value) = data else {
        return Ok(HashMap::new());
    };
    if value.is_null() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, serde_json::Value> = serde_json::from_value(value.clone())
        .map_err(|e| format!("data must be a plain object: {e}"))?;
    map.into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| {
            let input = run_data_value_from_json_value(v)?;
            match input {
                RunDataValue::String(s) => Ok((k, s)),
                RunDataValue::Boolean(b) => Ok((k, b.to_string())),
                RunDataValue::MeasureMap(m) | RunDataValue::RatioMap(m) => {
                    if m.len() == 1 {
                        let (unit, mag) = m.into_iter().next().expect("BUG: single entry map");
                        Ok((k, format!("{mag} {unit}")))
                    } else {
                        Err(format!(
                            "data value '{k}' must be a convenience string for run"
                        ))
                    }
                }
            }
        })
        .collect()
}

/// Resolve SDK/MCP/WASM `rules` (string or string array) for [`crate::Engine::run`].
pub fn resolve_run_rules(rules: &Option<serde_json::Value>) -> Result<Option<Vec<String>>, String> {
    let Some(value) = rules else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    if let Some(s) = value.as_str() {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err("rules must not be empty".to_string());
        }
        return Ok(Some(vec![trimmed.to_string()]));
    }
    if let Some(arr) = value.as_array() {
        if arr.is_empty() {
            return Err("rules must not be empty".to_string());
        }
        let names: Vec<String> = arr
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| "rules must be an array of strings".to_string())
            })
            .collect::<Result<_, _>>()?;
        return Ok(Some(names));
    }
    Err("rules must be a string or array of strings".to_string())
}

pub fn parse_data_value(
    input: &RunDataValue,
    lemma_type: &Arc<LemmaType>,
    source: &Source,
) -> Result<LiteralValue, Error> {
    let to_err = |msg: String| Error::validation(msg, Some(source.clone()), None::<String>);
    let type_spec = &lemma_type.specifications;

    let kind = match (input, type_spec) {
        (RunDataValue::String(s), _) => {
            let parsed = parse_value_from_string(s, type_spec, source)?;
            parser_value_to_value_kind(&parsed, type_spec).map_err(to_err)?
        }
        (RunDataValue::Boolean(b), TypeSpecification::Boolean { .. }) => ValueKind::Boolean(*b),
        (RunDataValue::Boolean(_), _) => {
            return Err(to_err(format!(
                "boolean input is only valid for boolean data, not {}",
                type_spec
            )));
        }
        (RunDataValue::MeasureMap(map), TypeSpecification::Measure { .. }) => {
            measure_from_unit_map(map, lemma_type.as_ref()).map_err(to_err)?
        }
        (
            RunDataValue::MeasureMap(map) | RunDataValue::RatioMap(map),
            TypeSpecification::Ratio { .. },
        ) => ratio_from_unit_map(map, lemma_type.as_ref()).map_err(to_err)?,
        (RunDataValue::MeasureMap(_), _) => {
            return Err(to_err(format!(
                "measure unit map is only valid for measure data, not {}",
                type_spec
            )));
        }
        (RunDataValue::RatioMap(_), _) => {
            return Err(to_err(format!(
                "ratio unit map is only valid for ratio data, not {}",
                type_spec
            )));
        }
    };

    Ok(LiteralValue {
        value: kind,
        lemma_type: Arc::clone(lemma_type),
    })
}

fn measure_from_unit_map(
    map: &BTreeMap<String, String>,
    lemma_type: &LemmaType,
) -> Result<ValueKind, String> {
    if map.is_empty() {
        return Err("measure input map must contain at least one unit key".to_string());
    }
    if lemma_type
        .measure_unit_names()
        .is_none_or(|names| names.is_empty())
    {
        unreachable!("BUG: measure type has no units at data input");
    }

    let mut kinds: Vec<ValueKind> = Vec::with_capacity(map.len());
    for (unit_name, mag_str) in map {
        let magnitude = Decimal::from_str(mag_str.trim())
            .map_err(|error| format!("invalid decimal '{mag_str}': {error}"))?;
        kinds.push(number_with_unit_to_value_kind(
            magnitude, unit_name, lemma_type,
        )?);
    }

    let first = kinds.first().expect("BUG: map non-empty");
    let ValueKind::Measure(first_magnitude, first_signature) = first else {
        return Err("expected measure value".to_string());
    };
    if first_signature.len() != 1 || first_signature[0].1 != 1 {
        return Err(
            "measure map produced a compound signature; use a convenience string instead"
                .to_string(),
        );
    }
    for kind in kinds.iter().skip(1) {
        let ValueKind::Measure(magnitude, signature) = kind else {
            return Err("expected measure value".to_string());
        };
        if signature.len() != 1 || signature[0].1 != 1 {
            return Err(
                "measure map produced a compound signature; use a convenience string instead"
                    .to_string(),
            );
        }
        if magnitude != first_magnitude {
            return Err(
                "measure unit map values disagree when converted to a common basis".to_string(),
            );
        }
    }
    Ok(first.clone())
}

fn ratio_from_unit_map(
    map: &BTreeMap<String, String>,
    lemma_type: &LemmaType,
) -> Result<ValueKind, String> {
    if map.is_empty() {
        return Err("ratio input map must contain at least one unit key".to_string());
    }
    match &lemma_type.specifications {
        TypeSpecification::Ratio { units, .. } if !units.is_empty() => {}
        _ => unreachable!("BUG: ratio type has no units at data input"),
    }

    let mut kinds: Vec<ValueKind> = Vec::with_capacity(map.len());
    for (unit_name, mag_str) in map {
        let magnitude = Decimal::from_str(mag_str.trim())
            .map_err(|error| format!("invalid decimal '{mag_str}': {error}"))?;
        kinds.push(number_with_unit_to_value_kind(
            magnitude, unit_name, lemma_type,
        )?);
    }

    let first = kinds.first().expect("BUG: map non-empty");
    let ValueKind::Ratio(first_canonical, first_unit) = first else {
        return Err("expected ratio value".to_string());
    };
    for kind in kinds.iter().skip(1) {
        let ValueKind::Ratio(canonical, _) = kind else {
            return Err("expected ratio value".to_string());
        };
        if canonical != first_canonical {
            return Err(
                "ratio unit map values disagree when converted to a common basis".to_string(),
            );
        }
    }
    Ok(ValueKind::Ratio(
        first_canonical.clone(),
        first_unit.clone(),
    ))
}

/// User-provided data values resolved against a plan's type declarations.
///
/// Lightweight and cheap to construct — no plan cloning required. The
/// [`ExecutionPlan`] stays immutable; callers pass `(&ExecutionPlan, &RunData)`
/// to evaluation and show paths.
#[derive(Debug, Clone, Default)]
pub struct RunData {
    /// Caller bindings: successful literals or Veto (bad override) per Data.
    pub bindings: HashMap<DataPath, OperationResult>,
    /// Input keys that did not match any plan Data (including Import aliases).
    pub ignored_unknown: Vec<String>,
}

impl RunData {
    /// Parse and validate caller-supplied values against the plan's data declarations.
    ///
    /// Unknown keys and Import aliases are ignored (recorded in [`Self::ignored_unknown`]).
    /// Parse, constraint, options, decimals, and input-oversize failures bind that Data
    /// as [`OperationResult::Veto`]; evaluation still runs. Duplicate canonical keys Error.
    pub fn resolve(
        plan: &ExecutionPlan,
        raw_values: HashMap<String, RunDataValue>,
        limits: &ResourceLimits,
    ) -> Result<Self, Error> {
        let mut run_data = Self::default();
        let mut seen_canonical = HashSet::with_capacity(raw_values.len());
        let mut by_input_key: HashMap<String, &DataPath> = HashMap::with_capacity(plan.data.len());
        for path in plan.data.keys() {
            by_input_key.insert(path.input_key(), path);
        }

        for (name, raw_value) in raw_values {
            let canonical = crate::parsing::ast::ascii_lowercase_logical_name(name.clone());
            if !seen_canonical.insert(canonical.clone()) {
                return Err(Error::request(
                    format!("Duplicate data key '{canonical}'"),
                    Some("Data keys are case-insensitive; remove the duplicate"),
                ));
            }

            let Some(data_path) = by_input_key.get(canonical.as_str()) else {
                run_data.ignored_unknown.push(name);
                continue;
            };
            let data_path = (*data_path).clone();

            let data_definition = plan
                .data
                .get(&data_path)
                .expect("BUG: data_path was just resolved from plan.data, must exist");

            let data_source = data_definition.source().clone();
            let type_arc = match data_definition {
                DataDefinition::TypeDeclaration { resolved_type, .. }
                | DataDefinition::Reference { resolved_type, .. } => Arc::clone(resolved_type),
                DataDefinition::Value { value, .. } => Arc::clone(&value.lemma_type),
                DataDefinition::Import { .. } => {
                    run_data.ignored_unknown.push(name);
                    continue;
                }
            };

            let input_key = data_path.input_key();

            if raw_value.is_empty() && type_arc.empty_runtime_input_vetoes() {
                run_data.bindings.insert(
                    data_path,
                    OperationResult::Veto(VetoType::computation(
                        type_arc.data_veto_message(&input_key, "cannot be empty."),
                    )),
                );
                continue;
            }

            let literal_value = match parse_data_value(&raw_value, &type_arc, &data_source) {
                Ok(value) => value,
                Err(error) => {
                    run_data.bindings.insert(
                        data_path,
                        OperationResult::Veto(VetoType::computation(
                            type_arc.data_veto_message(&input_key, error.message()),
                        )),
                    );
                    continue;
                }
            };

            let size = literal_value.byte_size();
            if size > limits.max_data_value_bytes {
                run_data.bindings.insert(
                    data_path,
                    OperationResult::Veto(VetoType::computation(
                        type_arc.data_veto_message(&input_key, "exceeds the size limit."),
                    )),
                );
                continue;
            }

            if let Err(message) = validate_value_against_type(
                type_arc.as_ref(),
                &literal_value,
                plan.expression_unit_index(),
            ) {
                run_data.bindings.insert(
                    data_path,
                    OperationResult::Veto(VetoType::computation(
                        type_arc.data_veto_message(&input_key, &message),
                    )),
                );
                continue;
            }

            run_data
                .bindings
                .insert(data_path, OperationResult::from_literal(literal_value));
        }

        Ok(run_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::{decimal_to_rational, rational_new, rational_one};
    use crate::planning::semantics::{
        primitive_number_arc, MeasureUnit, MeasureUnits, RatioUnit, RatioUnits, TypeExtends,
    };

    fn dummy_source() -> Source {
        Source::new(
            crate::parsing::source::SourceType::Volatile,
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 1,
            },
        )
    }

    fn mass_measure_type() -> Arc<LemmaType> {
        Arc::new(LemmaType::new(
            "Mass".to_string(),
            TypeSpecification::Measure {
                minimum: None,
                maximum: None,
                decimals: None,
                units: MeasureUnits::from(vec![
                    MeasureUnit {
                        name: "kilogram".to_string(),
                        factor: rational_one(),
                        derived_measure_factors: Vec::new(),
                        decomposition: crate::literals::BaseMeasureVector::new(),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                    MeasureUnit {
                        name: "gram".to_string(),
                        factor: decimal_to_rational(Decimal::new(1, 3)).expect("factor"),
                        derived_measure_factors: Vec::new(),
                        decomposition: crate::literals::BaseMeasureVector::new(),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                ]),
                traits: Vec::new(),
                decomposition: None,
                help: String::new(),
            },
            TypeExtends::Primitive,
        ))
    }

    fn ratio_with_percent_type() -> Arc<LemmaType> {
        Arc::new(LemmaType::new(
            "Rate".to_string(),
            TypeSpecification::Ratio {
                minimum: None,
                maximum: None,
                decimals: None,
                units: RatioUnits::from(vec![
                    RatioUnit {
                        name: "percent".to_string(),
                        value: decimal_to_rational(Decimal::new(100, 0)).expect("factor"),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                    RatioUnit {
                        name: "fraction".to_string(),
                        value: rational_one(),
                        minimum: None,
                        maximum: None,
                        suggestion_magnitude: None,
                    },
                ]),
                help: String::new(),
            },
            TypeExtends::Primitive,
        ))
    }

    #[test]
    fn string_input_parsed_against_type() {
        let ty = primitive_number_arc();
        let lit =
            parse_data_value(&RunDataValue::String("42".to_string()), ty, &dummy_source()).unwrap();
        assert!(matches!(lit.value, ValueKind::Number(_)));
    }

    #[test]
    fn measure_map_agreeing_units_canonicalize() {
        let ty = mass_measure_type();
        let mut map = BTreeMap::new();
        map.insert("kilogram".to_string(), "2".to_string());
        map.insert("gram".to_string(), "2000".to_string());
        let lit = parse_data_value(&RunDataValue::MeasureMap(map), &ty, &dummy_source()).unwrap();
        let ValueKind::Measure(magnitude, signature) = &lit.value else {
            panic!("expected measure");
        };
        assert_eq!(magnitude, &rational_new(2, 1));
        assert_eq!(signature.len(), 1);
        assert_eq!(signature[0].1, 1);
    }

    #[test]
    fn measure_map_disagreeing_units_rejected() {
        let ty = mass_measure_type();
        let mut map = BTreeMap::new();
        map.insert("kilogram".to_string(), "2".to_string());
        map.insert("gram".to_string(), "3000".to_string());
        let err =
            parse_data_value(&RunDataValue::MeasureMap(map), &ty, &dummy_source()).unwrap_err();
        assert!(err.message().contains("disagree"));
    }

    #[test]
    fn ratio_map_percent_and_fraction_agree() {
        let ty = ratio_with_percent_type();
        let mut map = BTreeMap::new();
        map.insert("percent".to_string(), "10".to_string());
        map.insert("fraction".to_string(), "0.1".to_string());
        let lit = parse_data_value(&RunDataValue::RatioMap(map), &ty, &dummy_source()).unwrap();
        let ValueKind::Ratio(canonical, unit) = &lit.value else {
            panic!("expected ratio");
        };
        assert_eq!(
            *canonical,
            decimal_to_rational(Decimal::new(1, 1)).expect("canonical")
        );
        assert!(unit.is_some());
    }
}
