use crate::planning::semantics::{
    number_with_unit_to_value_kind, parse_value_from_string, parser_value_to_value_kind, LemmaType,
    LiteralValue, Source, TypeSpecification, ValueKind,
};
use crate::Error;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

/// Typed data value from a client (CLI/WASM). JSON parsing stays outside [`parse_data_value`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataValueInput {
    Convenience(String),
    Boolean(bool),
    QuantityMap(BTreeMap<String, String>),
    RatioMap(BTreeMap<String, String>),
}

impl DataValueInput {
    pub fn convenience(value: impl Into<String>) -> Self {
        Self::Convenience(value.into())
    }
}

pub fn parse_data_value(
    input: &DataValueInput,
    lemma_type: &Arc<LemmaType>,
    source: &Source,
) -> Result<LiteralValue, Error> {
    let to_err = |msg: String| Error::validation(msg, Some(source.clone()), None::<String>);
    let type_spec = &lemma_type.specifications;

    let kind = match (input, type_spec) {
        (DataValueInput::Convenience(s), _) => {
            let parsed = parse_value_from_string(s, type_spec, source)?;
            parser_value_to_value_kind(&parsed, type_spec).map_err(to_err)?
        }
        (DataValueInput::Boolean(b), TypeSpecification::Boolean { .. }) => ValueKind::Boolean(*b),
        (DataValueInput::Boolean(_), _) => {
            return Err(to_err(format!(
                "boolean input is only valid for boolean data, not {}",
                value_kind_tag_for_type(type_spec)
            )));
        }
        (DataValueInput::QuantityMap(map), TypeSpecification::Quantity { .. }) => {
            quantity_from_unit_map(map, lemma_type.as_ref()).map_err(to_err)?
        }
        (
            DataValueInput::QuantityMap(map) | DataValueInput::RatioMap(map),
            TypeSpecification::Ratio { .. },
        ) => ratio_from_unit_map(map, lemma_type.as_ref()).map_err(to_err)?,
        (DataValueInput::QuantityMap(_), _) => {
            return Err(to_err(format!(
                "quantity unit map is only valid for quantity data, not {}",
                value_kind_tag_for_type(type_spec)
            )));
        }
        (DataValueInput::RatioMap(_), _) => {
            return Err(to_err(format!(
                "ratio unit map is only valid for ratio data, not {}",
                value_kind_tag_for_type(type_spec)
            )));
        }
    };

    Ok(LiteralValue {
        value: kind,
        lemma_type: Arc::clone(lemma_type),
    })
}

fn quantity_from_unit_map(
    map: &BTreeMap<String, String>,
    lemma_type: &LemmaType,
) -> Result<ValueKind, String> {
    if map.is_empty() {
        return Err("quantity input map must contain at least one unit key".to_string());
    }
    if lemma_type
        .quantity_unit_names()
        .is_none_or(|names| names.is_empty())
    {
        unreachable!("BUG: quantity type has no units at data input");
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
    let ValueKind::Quantity(first_magnitude, first_signature) = first else {
        return Err("expected quantity value".to_string());
    };
    if first_signature.len() != 1 || first_signature[0].1 != 1 {
        return Err(
            "quantity map produced a compound signature; use a convenience string instead"
                .to_string(),
        );
    }
    for kind in kinds.iter().skip(1) {
        let ValueKind::Quantity(magnitude, signature) = kind else {
            return Err("expected quantity value".to_string());
        };
        if signature.len() != 1 || signature[0].1 != 1 {
            return Err(
                "quantity map produced a compound signature; use a convenience string instead"
                    .to_string(),
            );
        }
        if magnitude != first_magnitude {
            return Err(
                "quantity unit map values disagree when converted to a common basis".to_string(),
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
    Ok(ValueKind::Ratio(*first_canonical, first_unit.clone()))
}

fn value_kind_tag_for_type(spec: &TypeSpecification) -> &'static str {
    match spec {
        TypeSpecification::Boolean { .. } => "boolean",
        TypeSpecification::Quantity { .. } => "quantity",
        TypeSpecification::Number { .. } => "number",
        TypeSpecification::NumberRange { .. }
        | TypeSpecification::QuantityRange { .. }
        | TypeSpecification::DateRange { .. }
        | TypeSpecification::TimeRange { .. }
        | TypeSpecification::RatioRange { .. } => "range",
        TypeSpecification::Ratio { .. } => "ratio",
        TypeSpecification::Text { .. } => "text",
        TypeSpecification::Date { .. } => "date",
        TypeSpecification::Time { .. } => "time",
        TypeSpecification::Veto { .. } => "veto",
        TypeSpecification::Undetermined => "undetermined",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::{decimal_to_rational, rational_one};
    use crate::planning::semantics::{
        primitive_number_arc, QuantityUnit, QuantityUnits, RatioUnit, RatioUnits, TypeExtends,
    };

    fn dummy_source() -> Source {
        Source::new(
            crate::parsing::source::SourceType::Volatile,
            crate::planning::semantics::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 1,
            },
        )
    }

    fn mass_quantity_type() -> Arc<LemmaType> {
        Arc::new(LemmaType::new(
            "Mass".to_string(),
            TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: QuantityUnits::from(vec![
                    QuantityUnit {
                        name: "kilogram".to_string(),
                        factor: rational_one(),
                        derived_quantity_factors: Vec::new(),
                        decomposition: crate::literals::BaseQuantityVector::new(),
                        minimum: None,
                        maximum: None,
                        default_magnitude: None,
                    },
                    QuantityUnit {
                        name: "gram".to_string(),
                        factor: decimal_to_rational(Decimal::new(1, 3)).expect("factor"),
                        derived_quantity_factors: Vec::new(),
                        decomposition: crate::literals::BaseQuantityVector::new(),
                        minimum: None,
                        maximum: None,
                        default_magnitude: None,
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
                        default_magnitude: None,
                    },
                    RatioUnit {
                        name: "fraction".to_string(),
                        value: rational_one(),
                        minimum: None,
                        maximum: None,
                        default_magnitude: None,
                    },
                ]),
                help: String::new(),
            },
            TypeExtends::Primitive,
        ))
    }

    #[test]
    fn convenience_string_still_works() {
        let ty = primitive_number_arc();
        let lit = parse_data_value(
            &DataValueInput::Convenience("42".to_string()),
            ty,
            &dummy_source(),
        )
        .unwrap();
        assert!(matches!(lit.value, ValueKind::Number(_)));
    }

    #[test]
    fn quantity_map_agreeing_units_canonicalize() {
        let ty = mass_quantity_type();
        let mut map = BTreeMap::new();
        map.insert("kilogram".to_string(), "2".to_string());
        map.insert("gram".to_string(), "2000".to_string());
        let lit =
            parse_data_value(&DataValueInput::QuantityMap(map), &ty, &dummy_source()).unwrap();
        let ValueKind::Quantity(magnitude, signature) = &lit.value else {
            panic!("expected quantity");
        };
        assert_eq!(*magnitude, rational_one() + rational_one());
        assert_eq!(signature.len(), 1);
        assert_eq!(signature[0].1, 1);
    }

    #[test]
    fn quantity_map_disagreeing_units_rejected() {
        let ty = mass_quantity_type();
        let mut map = BTreeMap::new();
        map.insert("kilogram".to_string(), "2".to_string());
        map.insert("gram".to_string(), "3000".to_string());
        let err =
            parse_data_value(&DataValueInput::QuantityMap(map), &ty, &dummy_source()).unwrap_err();
        assert!(err.message().contains("disagree"));
    }

    #[test]
    fn ratio_map_percent_and_fraction_agree() {
        let ty = ratio_with_percent_type();
        let mut map = BTreeMap::new();
        map.insert("percent".to_string(), "10".to_string());
        map.insert("fraction".to_string(), "0.1".to_string());
        let lit = parse_data_value(&DataValueInput::RatioMap(map), &ty, &dummy_source()).unwrap();
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
