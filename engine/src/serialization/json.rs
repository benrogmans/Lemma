use crate::planning::semantics::{FactData, FactPath};
use crate::LemmaError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Parse JSON to string values for use with ExecutionPlan::with_values().
///
/// - `null` values are skipped
/// - All other values are converted to their string representation
pub fn from_json(json: &[u8]) -> Result<HashMap<String, String>, LemmaError> {
    let map: HashMap<String, Value> = serde_json::from_slice(json).map_err(|e| {
        LemmaError::engine(format!("JSON parse error: {}", e), None, None::<String>)
    })?;

    Ok(map
        .into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k, json_value_to_string(&v)))
        .collect())
}

fn json_value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
        Value::Null => String::new(),
    }
}

/// Serializes HashMap<FactPath, FactData> as array of [FactPath, FactData] tuples.
pub fn serialize_resolved_fact_value_map<S>(
    map: &HashMap<FactPath, FactData>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let entries: Vec<(&FactPath, &FactData)> = map.iter().collect();
    entries.serialize(serializer)
}

/// Deserializes from array of [FactPath, FactData] tuples.
pub fn deserialize_resolved_fact_value_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<FactPath, FactData>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries: Vec<(FactPath, FactData)> = Vec::deserialize(deserializer)?;
    Ok(entries.into_iter().collect())
}

/// Serializes HashSet<FactPath> as array of FactPath structures.
pub fn serialize_fact_path_set<S>(set: &HashSet<FactPath>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let items: Vec<&FactPath> = set.iter().collect();
    items.serialize(serializer)
}

/// Deserializes array of FactPath structures to HashSet<FactPath>.
pub fn deserialize_fact_path_set<'de, D>(deserializer: D) -> Result<HashSet<FactPath>, D::Error>
where
    D: Deserializer<'de>,
{
    let items: Vec<FactPath> = Vec::deserialize(deserializer)?;
    Ok(items.into_iter().collect())
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
}
