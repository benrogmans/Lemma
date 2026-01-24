use crate::parsing::ast::Span;
use crate::planning::ExecutionPlan;
use crate::semantic::{FactPath, LemmaFact, LemmaType, LiteralValue};
use crate::LemmaError;
use crate::Source;
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Parse JSON to string values for use with ExecutionPlan::with_values().
///
/// - `null` values are skipped
/// - All other values are converted to their string representation
pub fn from_json(json: &[u8], plan: &ExecutionPlan) -> Result<HashMap<String, String>, LemmaError> {
    let map: HashMap<String, Value> = serde_json::from_slice(json).map_err(|e| {
        LemmaError::engine(
            format!("JSON parse error: {}", e),
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<input>",
            Arc::from(""),
            &plan.doc_name,
            1,
            None::<String>,
        )
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

/// Custom serializer for HashMap<FactPath, LemmaFact>
///
/// JSON object keys must be strings, so FactPath keys are serialized as strings
/// using their Display implementation (e.g., "age" or "employee.salary").
pub fn serialize_fact_path_map<S>(
    map: &HashMap<FactPath, LemmaFact>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;
    let mut map_serializer = serializer.serialize_map(Some(map.len()))?;
    for (key, value) in map {
        map_serializer.serialize_entry(&key.to_string(), value)?;
    }
    map_serializer.end()
}

/// Custom deserializer for HashMap<FactPath, LemmaFact>
///
/// Deserializes string keys back to FactPath using FactPath::from_path().
pub fn deserialize_fact_path_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<FactPath, LemmaFact>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, LemmaFact> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (key_str, value) in map {
        let path_parts: Vec<String> = key_str.split('.').map(|s| s.to_string()).collect();
        let fact_path = FactPath::from_path(path_parts);
        result.insert(fact_path, value);
    }
    Ok(result)
}

/// Custom serializer for HashMap<FactPath, LemmaType>
///
/// JSON object keys must be strings, so FactPath keys are serialized as strings
/// using their Display implementation (e.g., "age" or "employee.salary").
pub fn serialize_fact_type_map<S>(
    map: &HashMap<FactPath, LemmaType>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;
    let mut map_serializer = serializer.serialize_map(Some(map.len()))?;
    for (key, value) in map {
        map_serializer.serialize_entry(&key.to_string(), value)?;
    }
    map_serializer.end()
}

/// Custom deserializer for HashMap<FactPath, LemmaType>
///
/// Deserializes string keys back to FactPath using FactPath::from_path().
pub fn deserialize_fact_type_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<FactPath, LemmaType>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, LemmaType> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (key_str, value) in map {
        let path_parts: Vec<String> = key_str.split('.').map(|s| s.to_string()).collect();
        let fact_path = FactPath::from_path(path_parts);
        result.insert(fact_path, value);
    }
    Ok(result)
}

/// Custom serializer for HashMap<FactPath, LiteralValue>
///
/// JSON object keys must be strings, so FactPath keys are serialized as strings
/// using their Display implementation (e.g., "age" or "employee.salary").
pub fn serialize_fact_value_map<S>(
    map: &HashMap<FactPath, LiteralValue>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;
    let mut map_serializer = serializer.serialize_map(Some(map.len()))?;
    for (key, value) in map {
        map_serializer.serialize_entry(&key.to_string(), value)?;
    }
    map_serializer.end()
}

/// Custom deserializer for HashMap<FactPath, LiteralValue>
///
/// Deserializes string keys back to FactPath using FactPath::from_path().
pub fn deserialize_fact_value_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<FactPath, LiteralValue>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, LiteralValue> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (key_str, value) in map {
        let path_parts: Vec<String> = key_str.split('.').map(|s| s.to_string()).collect();
        let fact_path = FactPath::from_path(path_parts);
        result.insert(fact_path, value);
    }
    Ok(result)
}

/// Custom serializer for HashMap<FactPath, String> (document references)
pub fn serialize_fact_doc_ref_map<S>(
    map: &HashMap<FactPath, String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;
    let mut map_serializer = serializer.serialize_map(Some(map.len()))?;
    for (key, value) in map {
        map_serializer.serialize_entry(&key.to_string(), value)?;
    }
    map_serializer.end()
}

/// Custom deserializer for HashMap<FactPath, String> (document references)
pub fn deserialize_fact_doc_ref_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<FactPath, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, String> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (key_str, value) in map {
        let path_parts: Vec<String> = key_str.split('.').map(|s| s.to_string()).collect();
        let fact_path = FactPath::from_path(path_parts);
        result.insert(fact_path, value);
    }
    Ok(result)
}

/// Custom serializer for HashMap<FactPath, Source> (fact sources)
pub fn serialize_fact_source_map<S>(
    map: &HashMap<FactPath, Source>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;
    let mut map_serializer = serializer.serialize_map(Some(map.len()))?;
    for (key, value) in map {
        map_serializer.serialize_entry(&key.to_string(), value)?;
    }
    map_serializer.end()
}

/// Custom deserializer for HashMap<FactPath, Source> (fact sources)
pub fn deserialize_fact_source_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<FactPath, Source>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, Source> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (key_str, value) in map {
        let path_parts: Vec<String> = key_str.split('.').map(|s| s.to_string()).collect();
        let fact_path = FactPath::from_path(path_parts);
        result.insert(fact_path, value);
    }
    Ok(result)
}

/// Custom serializer for HashSet<FactPath>
///
/// Serializes as a JSON array of strings using FactPath's Display implementation.
pub fn serialize_fact_path_set<S>(set: &HashSet<FactPath>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(set.len()))?;
    for item in set {
        seq.serialize_element(&item.to_string())?;
    }
    seq.end()
}

/// Custom deserializer for HashSet<FactPath>
///
/// Deserializes array of strings back to FactPath using FactPath::from_path().
pub fn deserialize_fact_path_set<'de, D>(deserializer: D) -> Result<HashSet<FactPath>, D::Error>
where
    D: Deserializer<'de>,
{
    let vec: Vec<String> = Vec::deserialize(deserializer)?;
    let mut result = HashSet::new();
    for key_str in vec {
        let path_parts: Vec<String> = key_str.split('.').map(|s| s.to_string()).collect();
        let fact_path = FactPath::from_path(path_parts);
        result.insert(fact_path);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create_test_plan() -> ExecutionPlan {
        ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: HashMap::new(),
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: vec![],
            sources: HashMap::from([("<test>".to_string(), "".to_string())]),
        }
    }

    #[test]
    fn test_json_string_to_string() {
        let plan = create_test_plan();
        let json = br#"{"name": "Alice"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.get("name"), Some(&"Alice".to_string()));
    }

    #[test]
    fn test_json_number_to_string() {
        let plan = create_test_plan();
        let json = br#"{"name": 42}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.get("name"), Some(&"42".to_string()));
    }

    #[test]
    fn test_json_boolean_to_string() {
        let plan = create_test_plan();
        let json = br#"{"name": true}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.get("name"), Some(&"true".to_string()));
    }

    #[test]
    fn test_json_array_to_string() {
        let plan = create_test_plan();
        let json = br#"{"data": [1, 2, 3]}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.get("data"), Some(&"[1,2,3]".to_string()));
    }

    #[test]
    fn test_json_object_to_string() {
        let plan = create_test_plan();
        let json = br#"{"config": {"key": "value"}}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("config"),
            Some(&"{\"key\":\"value\"}".to_string())
        );
    }

    #[test]
    fn test_null_value_skipped() {
        let plan = create_test_plan();
        let json = br#"{"name": null, "age": 30}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.len(), 1);
        assert!(!result.contains_key("name"));
        assert_eq!(result.get("age"), Some(&"30".to_string()));
    }

    #[test]
    fn test_all_null_values() {
        let plan = create_test_plan();
        let json = br#"{"name": null}"#;
        let result = from_json(json, &plan).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_mixed_valid_types() {
        let plan = create_test_plan();
        let json = br#"{"name": "Test", "count": 5, "active": true, "discount": 21}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result.get("name"), Some(&"Test".to_string()));
        assert_eq!(result.get("count"), Some(&"5".to_string()));
        assert_eq!(result.get("active"), Some(&"true".to_string()));
        assert_eq!(result.get("discount"), Some(&"21".to_string()));
    }

    #[test]
    fn test_invalid_json_syntax() {
        let plan = create_test_plan();
        let json = br#"{"name": }"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("JSON parse error"));
    }
}
