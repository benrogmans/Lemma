use lemma::DataValueInput;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// Parse `application/x-www-form-urlencoded` fields into data inputs (all convenience strings).
pub fn form_urlencoded_to_data_values(
    body: &[u8],
) -> Result<HashMap<String, DataValueInput>, String> {
    let fields: HashMap<String, String> =
        serde_urlencoded::from_bytes(body).map_err(|e| format!("invalid form body: {e}"))?;
    Ok(fields
        .into_iter()
        .map(|(k, v)| (k, DataValueInput::convenience(v)))
        .collect())
}

/// Convert one JSON value to [`DataValueInput`]. Rejects unsupported shapes.
pub fn json_value_to_data_input(value: Value) -> Result<DataValueInput, String> {
    match value {
        Value::String(s) => Ok(DataValueInput::Convenience(s)),
        Value::Bool(b) => Ok(DataValueInput::Boolean(b)),
        Value::Number(n) => Ok(DataValueInput::Convenience(n.to_string())),
        Value::Object(obj) => {
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
                return Ok(DataValueInput::QuantityMap(map));
            }
            Err("data value object must be a unit map with string magnitudes".to_string())
        }
        Value::Null => Err("data value must not be null".to_string()),
        Value::Array(_) => Err("data value must not be an array".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn json_string_preserved() {
        let input = json_value_to_data_input(Value::String("Alice".to_string())).unwrap();
        assert_eq!(input, DataValueInput::Convenience("Alice".to_string()));
    }

    #[test]
    fn json_unit_map_parsed() {
        let mut map = serde_json::Map::new();
        map.insert("eur_per_hour".to_string(), Value::String("85".to_string()));
        let input = json_value_to_data_input(Value::Object(map)).unwrap();
        match input {
            DataValueInput::QuantityMap(m) => {
                assert_eq!(m.get("eur_per_hour"), Some(&"85".to_string()));
            }
            other => panic!("expected quantity map, got {:?}", other),
        }
    }

    #[test]
    fn value_unit_object_shape_rejected() {
        let mut map = serde_json::Map::new();
        map.insert("value".to_string(), Value::String("5".to_string()));
        map.insert("unit".to_string(), Value::String("usd".to_string()));
        let err = json_value_to_data_input(Value::Object(map)).unwrap_err();
        assert!(err.contains("{value, unit}"));
    }

    #[test]
    fn array_rejected() {
        let err =
            json_value_to_data_input(Value::Array(vec![Value::String("x".into())])).unwrap_err();
        assert!(err.contains("array"));
    }

    #[test]
    fn form_urlencoded_parsed_as_convenience() {
        let map = form_urlencoded_to_data_values(b"code=AD&qty=3").unwrap();
        assert_eq!(
            map.get("code"),
            Some(&DataValueInput::Convenience("AD".to_string()))
        );
        assert_eq!(
            map.get("qty"),
            Some(&DataValueInput::Convenience("3".to_string()))
        );
    }

    #[test]
    fn form_urlencoded_decodes_plus_and_percent() {
        let map = form_urlencoded_to_data_values(b"name=hello+world&city=S%C3%A3o+Paulo").unwrap();
        assert_eq!(
            map.get("name"),
            Some(&DataValueInput::Convenience("hello world".to_string()))
        );
        assert_eq!(
            map.get("city"),
            Some(&DataValueInput::Convenience("São Paulo".to_string()))
        );
    }

    #[test]
    fn object_roundtrip_via_server_shape() {
        let body: HashMap<String, Value> = serde_json::from_str(r#"{"age":"30"}"#).unwrap();
        let converted: HashMap<String, DataValueInput> = body
            .into_iter()
            .map(|(k, v)| json_value_to_data_input(v).map(|input| (k, input)))
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            converted.get("age"),
            Some(&DataValueInput::Convenience("30".to_string()))
        );
    }
}
