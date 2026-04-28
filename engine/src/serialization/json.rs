use crate::planning::semantics::{DataDefinition, DataPath, LiteralValue, ValueKind};
use crate::Error;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::HashMap;

/// Parse JSON to string values for use with ExecutionPlan::with_values().
///
/// - `null` values are skipped
/// - All other values are converted to their string representation
pub fn from_json(json: &[u8]) -> Result<HashMap<String, String>, Error> {
    let map: HashMap<String, Value> = serde_json::from_slice(json)
        .map_err(|e| Error::validation(format!("JSON parse error: {}", e), None, None::<String>))?;

    Ok(data_values_from_map(map))
}

/// Same string coercion as [`from_json`], for maps already parsed as JSON values (e.g. WASM).
pub fn data_values_from_map(map: HashMap<String, Value>) -> HashMap<String, String> {
    map.into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k, json_value_to_string(&v)))
        .collect()
}

fn json_value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value)
            .expect("BUG: serde_json::to_string failed on a serde_json::Value"),
        Value::Null => unreachable!(
            "null JSON values are filtered in data_values_from_map before json_value_to_string"
        ),
    }
}

// -----------------------------------------------------------------------------
// Output: Lemma values → JSON (for evaluation responses)
// -----------------------------------------------------------------------------

/// Convert a Lemma literal value to a JSON value and optional unit string.
///
/// Used when serializing evaluation results (e.g. CLI `run --output json`, HTTP API).
/// Returns `(value, unit)` where `unit` is present for scale and duration.
pub fn literal_value_to_json(v: &LiteralValue) -> (Value, Option<String>) {
    match &v.value {
        ValueKind::Boolean(b) => (Value::Bool(*b), None),
        ValueKind::Number(n) => (decimal_to_json(n), None),
        ValueKind::Scale(n, unit) => (decimal_to_json(n), Some(unit.clone())),
        ValueKind::Ratio(r, _) => (decimal_to_json(r), None),
        ValueKind::Duration(n, unit) => (decimal_to_json(n), Some(unit.to_string())),
        ValueKind::Text(_) | ValueKind::Date(_) | ValueKind::Time(_) => {
            (Value::String(v.display_value()), None)
        }
    }
}

/// Convert a decimal to a JSON number when in range; otherwise serialize as string.
///
/// Avoids panics for decimals outside i64 (integer case) or f64 (fractional case).
fn decimal_to_json(d: &Decimal) -> Value {
    if d.fract().is_zero() {
        match i64::try_from(d.trunc()) {
            Ok(n) => Value::Number(n.into()),
            Err(_) => Value::String(d.to_string()),
        }
    } else {
        let s = d.to_string();
        let Ok(f) = s.parse::<f64>() else {
            return Value::String(s);
        };
        match serde_json::Number::from_f64(f) {
            Some(n) => Value::Number(n),
            None => Value::String(s),
        }
    }
}

// -----------------------------------------------------------------------------
// Serde helpers for DataPath / DataDefinition
// -----------------------------------------------------------------------------

/// Serializes IndexMap<DataPath, DataDefinition> as array of [DataPath, DataDefinition] tuples.
pub fn serialize_resolved_data_value_map<S>(
    map: &IndexMap<DataPath, DataDefinition>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let entries: Vec<(&DataPath, &DataDefinition)> = map.iter().collect();
    entries.serialize(serializer)
}

/// Deserializes from array of [DataPath, DataDefinition] tuples, preserving order.
pub fn deserialize_resolved_data_value_map<'de, D>(
    deserializer: D,
) -> Result<IndexMap<DataPath, DataDefinition>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries: Vec<(DataPath, DataDefinition)> = Vec::deserialize(deserializer)?;
    Ok(entries.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_string_to_string() {
        let json = br#"{"name": "Alice"}"#;
        let result = from_json(json).unwrap();
        assert_eq!(result.get("name"), Some(&"Alice".to_string()));
    }

    #[test]
    fn test_json_number_to_string() {
        let json = br#"{"name": 42}"#;
        let result = from_json(json).unwrap();
        assert_eq!(result.get("name"), Some(&"42".to_string()));
    }

    #[test]
    fn test_json_boolean_to_string() {
        let json = br#"{"name": true}"#;
        let result = from_json(json).unwrap();
        assert_eq!(result.get("name"), Some(&"true".to_string()));
    }

    #[test]
    fn test_json_array_to_string() {
        let json = br#"{"data": [1, 2, 3]}"#;
        let result = from_json(json).unwrap();
        assert_eq!(result.get("data"), Some(&"[1,2,3]".to_string()));
    }

    #[test]
    fn test_json_object_to_string() {
        let json = br#"{"config": {"key": "value"}}"#;
        let result = from_json(json).unwrap();
        assert_eq!(
            result.get("config"),
            Some(&"{\"key\":\"value\"}".to_string())
        );
    }

    #[test]
    fn test_null_value_skipped() {
        let json = br#"{"name": null, "age": 30}"#;
        let result = from_json(json).unwrap();
        assert_eq!(result.len(), 1);
        assert!(!result.contains_key("name"));
        assert_eq!(result.get("age"), Some(&"30".to_string()));
    }

    #[test]
    fn test_all_null_values() {
        let json = br#"{"name": null}"#;
        let result = from_json(json).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_mixed_valid_types() {
        let json = br#"{"name": "Test", "count": 5, "active": true, "discount": 21}"#;
        let result = from_json(json).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result.get("name"), Some(&"Test".to_string()));
        assert_eq!(result.get("count"), Some(&"5".to_string()));
        assert_eq!(result.get("active"), Some(&"true".to_string()));
        assert_eq!(result.get("discount"), Some(&"21".to_string()));
    }

    #[test]
    fn test_invalid_json_syntax() {
        let json = br#"{"name": }"#;
        let result = from_json(json);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("JSON parse error"));
    }

    // --- literal_value_to_json / decimal_to_json ---

    #[test]
    fn test_literal_value_to_json_number() {
        use crate::planning::semantics::LiteralValue;
        use std::str::FromStr;
        let v = LiteralValue::number(rust_decimal::Decimal::from_str("42").unwrap());
        let (val, unit) = literal_value_to_json(&v);
        assert!(val.is_number());
        assert_eq!(val.as_i64(), Some(42));
        assert!(unit.is_none());
    }

    #[test]
    fn test_literal_value_to_json_scale() {
        use crate::planning::semantics::{primitive_scale, LiteralValue};
        use std::str::FromStr;
        let v = LiteralValue::scale_with_type(
            rust_decimal::Decimal::from_str("99.50").unwrap(),
            "eur".to_string(),
            primitive_scale().clone(),
        );
        let (val, unit) = literal_value_to_json(&v);
        assert!(val.is_number());
        assert_eq!(unit.as_deref(), Some("eur"));
    }

    #[test]
    fn test_literal_value_to_json_boolean() {
        use crate::planning::semantics::LiteralValue;
        let (val, unit) = literal_value_to_json(&LiteralValue::from_bool(true));
        assert_eq!(val.as_bool(), Some(true));
        assert!(unit.is_none());
    }

    #[test]
    fn test_decimal_to_json_out_of_i64_fallback() {
        use crate::planning::semantics::LiteralValue;
        use std::str::FromStr;
        // One more than i64::MAX; fits in Decimal but not i64
        let huge = rust_decimal::Decimal::from_str("9223372036854775808").unwrap();
        let v = LiteralValue::number(huge);
        let (val, _) = literal_value_to_json(&v);
        assert!(val.is_string());
        assert_eq!(val.as_str(), Some("9223372036854775808"));
    }
}
