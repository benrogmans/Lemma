use crate::planning::semantics::{DataDefinition, DataPath};
use crate::Error;
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::HashMap;

/// Parse JSON object to data values for [`ExecutionPlan::set_data_values`](crate::planning::ExecutionPlan::set_data_values).
///
/// - `null` values are skipped
/// - JSON strings and numbers use convenience Lemma literal syntax
/// - structured objects use the same serialization shape as evaluation output
pub fn from_json(json: &[u8]) -> Result<HashMap<String, Value>, Error> {
    let map: HashMap<String, Value> = serde_json::from_slice(json)
        .map_err(|e| Error::validation(format!("JSON parse error: {}", e), None, None::<String>))?;

    Ok(data_values_from_map(map))
}

/// Filter nulls from a parsed JSON object (e.g. WASM `run` data).
pub fn data_values_from_map(map: HashMap<String, Value>) -> HashMap<String, Value> {
    map.into_iter().filter(|(_, v)| !v.is_null()).collect()
}

/// Wrap convenience string maps (CLI forms, `Engine::run` call sites).
pub fn data_values_from_strings(map: HashMap<String, String>) -> HashMap<String, Value> {
    map.into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect()
}

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
    fn test_json_string_preserved() {
        let json = br#"{"name": "Alice"}"#;
        let result = from_json(json).unwrap();
        assert_eq!(
            result.get("name"),
            Some(&Value::String("Alice".to_string()))
        );
    }

    #[test]
    fn test_json_number_preserved_in_map() {
        let json = br#"{"count": 42}"#;
        let result = from_json(json).unwrap();
        assert!(result.get("count").unwrap().is_number());
    }

    #[test]
    fn test_null_value_skipped() {
        let json = br#"{"name": null, "age": "30"}"#;
        let result = from_json(json).unwrap();
        assert_eq!(result.len(), 1);
        assert!(!result.contains_key("name"));
        assert_eq!(result.get("age"), Some(&Value::String("30".to_string())));
    }

    #[test]
    fn test_invalid_json_syntax() {
        let json = br#"{"name": }"#;
        let result = from_json(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("JSON parse error"));
    }
}
