use crate::planning::ExecutionPlan;
use crate::semantic::{BooleanValue, FactPath, LemmaFact, LemmaType, LiteralValue};
use crate::LemmaError;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Convert JSON values to typed Lemma values using the ExecutionPlan for type information.
///
/// This function converts JSON values to Lemma types with the following rules:
///
/// | Lemma Type | Valid JSON Types | Conversion |
/// |------------|------------------|------------|
/// | Text | any | Strings pass through; numbers/booleans/arrays/objects serialize to JSON string |
/// | Number | number, string | Numbers pass through; strings are parsed as decimals |
/// | Boolean | boolean, string | Booleans pass through; strings parsed as "true"/"false"/"yes"/"no"/"accept"/"reject" |
/// | Percentage | number, string | Numbers become percentage; strings parsed (with or without %) |
/// | Date | string | ISO format "2024-01-15" or "2024-01-15T14:30:00Z" |
/// | Regex | string | Pattern string, optionally wrapped in /slashes/ |
/// | Unit types | string | Format: "100 kilogram", "5 meter", etc. |
///
/// Special handling:
/// - `null` values are skipped (treated as if the fact was not provided)
/// - Unknown facts return an error
/// - Unparseable values return an error with a descriptive message
pub fn from_json(
    json: &[u8],
    plan: &ExecutionPlan,
) -> Result<HashMap<String, LiteralValue>, LemmaError> {
    let map: HashMap<String, Value> = serde_json::from_slice(json)
        .map_err(|e| LemmaError::Engine(format!("JSON parse error: {}", e)))?;

    let mut result = HashMap::new();

    for (fact_name, json_value) in map {
        if json_value.is_null() {
            continue;
        }

        let (_, fact) = plan.get_fact_by_path_str(&fact_name).ok_or_else(|| {
            let available: Vec<String> = plan.facts.keys().map(|p| p.to_string()).collect();
            LemmaError::Engine(format!(
                "Fact '{}' not found in document. Available facts: {}",
                fact_name,
                available.join(", ")
            ))
        })?;

        let expected_type = get_expected_type(fact)?;
        let literal_value = convert_json_value(&fact_name, &json_value, &expected_type)?;

        result.insert(fact_name, literal_value);
    }

    Ok(result)
}

fn get_expected_type(fact: &crate::semantic::LemmaFact) -> Result<LemmaType, LemmaError> {
    match &fact.value {
        crate::semantic::FactValue::Literal(lit) => Ok(lit.to_type()),
        crate::semantic::FactValue::TypeAnnotation(crate::semantic::TypeAnnotation::LemmaType(
            t,
        )) => Ok(t.clone()),
        crate::semantic::FactValue::DocumentReference(_) => Err(LemmaError::Engine(
            "Cannot provide a value for a document reference fact".to_string(),
        )),
    }
}

fn convert_json_value(
    fact_name: &str,
    json_value: &Value,
    expected_type: &LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match expected_type {
        LemmaType::Text => convert_to_text(fact_name, json_value),
        LemmaType::Number => convert_to_number(fact_name, json_value),
        LemmaType::Boolean => convert_to_boolean(fact_name, json_value),
        LemmaType::Percentage => convert_to_percentage(fact_name, json_value),
        LemmaType::Date => convert_to_date(fact_name, json_value),
        LemmaType::Regex => convert_to_regex(fact_name, json_value),
        LemmaType::Mass
        | LemmaType::Length
        | LemmaType::Volume
        | LemmaType::Duration
        | LemmaType::Temperature
        | LemmaType::Power
        | LemmaType::Energy
        | LemmaType::Force
        | LemmaType::Pressure
        | LemmaType::Frequency
        | LemmaType::Data => convert_to_unit(fact_name, json_value, expected_type),
    }
}

fn convert_to_text(_fact_name: &str, json_value: &Value) -> Result<LiteralValue, LemmaError> {
    let text = match json_value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(json_value).unwrap_or_else(|_| json_value.to_string())
        }
        Value::Null => unreachable!("null values are filtered before conversion"),
    };
    Ok(LiteralValue::Text(text))
}

fn convert_to_number(fact_name: &str, json_value: &Value) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::Number(n) => {
            let decimal = json_number_to_decimal(fact_name, n)?;
            Ok(LiteralValue::Number(decimal))
        }
        Value::String(s) => {
            let clean = s.trim().replace(['_', ','], "");
            let decimal = Decimal::from_str_exact(&clean).map_err(|_| {
                LemmaError::Engine(format!(
                    "Invalid number string for fact '{}': '{}' is not a valid decimal",
                    fact_name, s
                ))
            })?;
            Ok(LiteralValue::Number(decimal))
        }
        Value::Null => unreachable!("null values are filtered before conversion"),
        Value::Bool(_) => Err(type_error(fact_name, "number", "boolean")),
        Value::Array(_) => Err(type_error(fact_name, "number", "array")),
        Value::Object(_) => Err(type_error(fact_name, "number", "object")),
    }
}

fn convert_to_boolean(fact_name: &str, json_value: &Value) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::Bool(b) => {
            let boolean_value = if *b {
                BooleanValue::True
            } else {
                BooleanValue::False
            };
            Ok(LiteralValue::Boolean(boolean_value))
        }
        Value::String(s) => {
            let boolean_value: BooleanValue = s.parse().map_err(|_| {
                LemmaError::Engine(format!(
                    "Invalid boolean string for fact '{}': '{}'. Expected one of: true, false, yes, no, accept, reject",
                    fact_name, s
                ))
            })?;
            Ok(LiteralValue::Boolean(boolean_value))
        }
        Value::Null => unreachable!("null values are filtered before conversion"),
        Value::Number(_) => Err(type_error(fact_name, "boolean", "number")),
        Value::Array(_) => Err(type_error(fact_name, "boolean", "array")),
        Value::Object(_) => Err(type_error(fact_name, "boolean", "object")),
    }
}

fn convert_to_percentage(fact_name: &str, json_value: &Value) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::Number(n) => {
            let decimal = json_number_to_decimal(fact_name, n)?;
            Ok(LiteralValue::Percentage(decimal))
        }
        Value::String(s) => {
            let trimmed = s.trim();
            let number_part = if let Some(stripped) = trimmed.strip_suffix('%') {
                stripped.trim()
            } else if trimmed.to_lowercase().ends_with("percent") {
                let without_suffix = &trimmed[..trimmed.len() - 7];
                without_suffix.trim()
            } else {
                trimmed
            };

            let clean_number = number_part.replace(['_', ','], "");
            let decimal = Decimal::from_str_exact(&clean_number).map_err(|_| {
                LemmaError::Engine(format!(
                    "Invalid percentage string for fact '{}': '{}' is not a valid number",
                    fact_name, s
                ))
            })?;
            Ok(LiteralValue::Percentage(decimal))
        }
        Value::Null => unreachable!("null values are filtered before conversion"),
        Value::Bool(_) => Err(type_error(fact_name, "percentage", "boolean")),
        Value::Array(_) => Err(type_error(fact_name, "percentage", "array")),
        Value::Object(_) => Err(type_error(fact_name, "percentage", "object")),
    }
}

fn convert_to_date(fact_name: &str, json_value: &Value) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::String(s) => LemmaType::Date.parse_value(s).map_err(|e| {
            LemmaError::Engine(format!("Invalid date for fact '{}': {}", fact_name, e))
        }),
        Value::Null => unreachable!("null values are filtered before conversion"),
        Value::Bool(_) => Err(type_error(fact_name, "date", "boolean")),
        Value::Number(_) => Err(type_error(fact_name, "date", "number")),
        Value::Array(_) => Err(type_error(fact_name, "date", "array")),
        Value::Object(_) => Err(type_error(fact_name, "date", "object")),
    }
}

fn convert_to_regex(fact_name: &str, json_value: &Value) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::String(s) => LemmaType::Regex.parse_value(s).map_err(|e| {
            LemmaError::Engine(format!("Invalid regex for fact '{}': {}", fact_name, e))
        }),
        Value::Null => unreachable!("null values are filtered before conversion"),
        Value::Bool(_) => Err(type_error(fact_name, "regex", "boolean")),
        Value::Number(_) => Err(type_error(fact_name, "regex", "number")),
        Value::Array(_) => Err(type_error(fact_name, "regex", "array")),
        Value::Object(_) => Err(type_error(fact_name, "regex", "object")),
    }
}

fn convert_to_unit(
    fact_name: &str,
    json_value: &Value,
    expected_type: &LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::String(s) => expected_type.parse_value(s).map_err(|e| {
            LemmaError::Engine(format!(
                "Invalid {} value for fact '{}': {}",
                expected_type, fact_name, e
            ))
        }),
        Value::Null => unreachable!("null values are filtered before conversion"),
        Value::Bool(_) => Err(type_error(fact_name, &expected_type.to_string(), "boolean")),
        Value::Number(_) => Err(LemmaError::Engine(format!(
            "Invalid JSON type for fact '{}': expected {} (as string like '100 kilogram'), got number. Unit values must include the unit name.",
            fact_name, expected_type
        ))),
        Value::Array(_) => Err(type_error(fact_name, &expected_type.to_string(), "array")),
        Value::Object(_) => Err(type_error(fact_name, &expected_type.to_string(), "object")),
    }
}

fn json_number_to_decimal(fact_name: &str, n: &serde_json::Number) -> Result<Decimal, LemmaError> {
    if let Some(i) = n.as_i64() {
        Ok(Decimal::from(i))
    } else if let Some(u) = n.as_u64() {
        Ok(Decimal::from(u))
    } else if let Some(f) = n.as_f64() {
        Decimal::try_from(f).map_err(|_| {
            LemmaError::Engine(format!(
                "Invalid number for fact '{}': {} cannot be represented as a decimal",
                fact_name, n
            ))
        })
    } else {
        Err(LemmaError::Engine(format!(
            "Invalid number for fact '{}': {} is not a valid number",
            fact_name, n
        )))
    }
}

fn type_error(fact_name: &str, expected: &str, got: &str) -> LemmaError {
    LemmaError::Engine(format!(
        "Invalid JSON type for fact '{}': expected {}, got {}",
        fact_name, expected, got
    ))
}

// Custom JSON serializers for Response types

/// Custom serializer for LiteralValue that outputs type and value
pub fn serialize_literal_value<S>(value: &LiteralValue, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::ser::Serializer,
{
    use serde::ser::SerializeMap;
    use serde_json::Number;
    use std::str::FromStr;

    let mut map = serializer.serialize_map(Some(2))?;

    match value {
        LiteralValue::Number(n) => {
            map.serialize_entry("type", "number")?;
            let num = Number::from_str(&n.to_string())
                .map_err(|_| serde::ser::Error::custom("Failed to convert Decimal to Number"))?;
            map.serialize_entry("value", &num)?;
        }
        LiteralValue::Percentage(p) => {
            map.serialize_entry("type", "percentage")?;
            let num = Number::from_str(&p.to_string())
                .map_err(|_| serde::ser::Error::custom("Failed to convert Decimal to Number"))?;
            map.serialize_entry("value", &num)?;
        }
        LiteralValue::Boolean(b) => {
            map.serialize_entry("type", "boolean")?;
            map.serialize_entry("value", &bool::from(b.clone()))?;
        }
        LiteralValue::Text(s) => {
            map.serialize_entry("type", "text")?;
            map.serialize_entry("value", s)?;
        }
        LiteralValue::Date(dt) => {
            map.serialize_entry("type", "date")?;
            map.serialize_entry("value", &dt.to_string())?;
        }
        LiteralValue::Time(time) => {
            map.serialize_entry("type", "time")?;
            map.serialize_entry("value", &time.to_string())?;
        }
        LiteralValue::Unit(unit) => {
            map.serialize_entry("type", "unit")?;
            map.serialize_entry("value", &unit.to_string())?;
        }
        LiteralValue::Regex(s) => {
            map.serialize_entry("type", "regex")?;
            map.serialize_entry("value", s)?;
        }
    }

    map.end()
}

/// Custom serializer for OperationResult
pub fn serialize_operation_result<S>(
    result: &crate::evaluation::operations::OperationResult,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::ser::Serializer,
{
    use crate::evaluation::operations::OperationResult;
    use serde::ser::SerializeMap;

    match result {
        OperationResult::Value(lit_val) => {
            // Just serialize the literal value directly
            serialize_literal_value(lit_val, serializer)
        }
        OperationResult::Veto(msg) => {
            let mut map = serializer.serialize_map(Some(if msg.is_some() { 2 } else { 1 }))?;
            map.serialize_entry("type", "veto")?;
            if let Some(m) = msg {
                map.serialize_entry("message", m)?;
            }
            map.end()
        }
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
    use crate::semantic::{
        FactPath, FactReference, FactValue, LemmaFact, LemmaType, LiteralValue, TypeAnnotation,
    };
    use rust_decimal::Decimal;

    fn create_test_plan(facts: Vec<(&str, LemmaType)>) -> ExecutionPlan {
        let mut fact_map = HashMap::new();
        for (name, lemma_type) in facts {
            let fact_path = FactPath {
                segments: vec![],
                fact: name.to_string(),
            };
            let fact = LemmaFact {
                reference: FactReference {
                    segments: vec![],
                    fact: name.to_string(),
                },
                value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(lemma_type)),
                source_location: None,
            };
            fact_map.insert(fact_path, fact);
        }
        ExecutionPlan {
            doc_name: "test".to_string(),
            facts: fact_map,
            rules: vec![],
            sources: HashMap::new(),
        }
    }

    #[test]
    fn test_text_from_string() {
        let plan = create_test_plan(vec![("name", LemmaType::Text)]);
        let json = br#"{"name": "Alice"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("name"),
            Some(&LiteralValue::Text("Alice".to_string()))
        );
    }

    #[test]
    fn test_text_from_number() {
        let plan = create_test_plan(vec![("name", LemmaType::Text)]);
        let json = br#"{"name": 42}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("name"),
            Some(&LiteralValue::Text("42".to_string()))
        );
    }

    #[test]
    fn test_text_from_boolean() {
        let plan = create_test_plan(vec![("name", LemmaType::Text)]);
        let json = br#"{"name": true}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("name"),
            Some(&LiteralValue::Text("true".to_string()))
        );
    }

    #[test]
    fn test_text_from_array() {
        let plan = create_test_plan(vec![("data", LemmaType::Text)]);
        let json = br#"{"data": [1, 2, 3]}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("data"),
            Some(&LiteralValue::Text("[1,2,3]".to_string()))
        );
    }

    #[test]
    fn test_text_from_object() {
        let plan = create_test_plan(vec![("config", LemmaType::Text)]);
        let json = br#"{"config": {"key": "value"}}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("config"),
            Some(&LiteralValue::Text("{\"key\":\"value\"}".to_string()))
        );
    }

    #[test]
    fn test_number_from_integer() {
        let plan = create_test_plan(vec![("count", LemmaType::Number)]);
        let json = br#"{"count": 42}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("count"),
            Some(&LiteralValue::Number(Decimal::from(42)))
        );
    }

    #[test]
    fn test_number_from_decimal() {
        let plan = create_test_plan(vec![("price", LemmaType::Number)]);
        let json = br#"{"price": 99.95}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("price") {
            Some(LiteralValue::Number(n)) => {
                let expected = Decimal::try_from(99.95).unwrap();
                let tolerance = Decimal::try_from(0.001).unwrap();
                assert!((*n - expected).abs() < tolerance);
            }
            other => panic!("Expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_number_from_string() {
        let plan = create_test_plan(vec![("count", LemmaType::Number)]);
        let json = br#"{"count": "42"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("count"),
            Some(&LiteralValue::Number(Decimal::from(42)))
        );
    }

    #[test]
    fn test_number_from_string_with_formatting() {
        let plan = create_test_plan(vec![("price", LemmaType::Number)]);
        let json = br#"{"price": "1,234.56"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("price") {
            Some(LiteralValue::Number(n)) => {
                let expected = Decimal::try_from(1234.56).unwrap();
                let tolerance = Decimal::try_from(0.001).unwrap();
                assert!((*n - expected).abs() < tolerance);
            }
            other => panic!("Expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_number_from_invalid_string() {
        let plan = create_test_plan(vec![("count", LemmaType::Number)]);
        let json = br#"{"count": "hello"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Invalid number string"));
    }

    #[test]
    fn test_number_rejects_boolean() {
        let plan = create_test_plan(vec![("count", LemmaType::Number)]);
        let json = br#"{"count": true}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected number"));
        assert!(error_message.contains("got boolean"));
    }

    #[test]
    fn test_boolean_from_true() {
        let plan = create_test_plan(vec![("active", LemmaType::Boolean)]);
        let json = br#"{"active": true}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("active") {
            Some(LiteralValue::Boolean(b)) => assert!(bool::from(b)),
            other => panic!("Expected Boolean, got {:?}", other),
        }
    }

    #[test]
    fn test_boolean_from_false() {
        let plan = create_test_plan(vec![("active", LemmaType::Boolean)]);
        let json = br#"{"active": false}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("active") {
            Some(LiteralValue::Boolean(b)) => assert!(!bool::from(b)),
            other => panic!("Expected Boolean, got {:?}", other),
        }
    }

    #[test]
    fn test_boolean_from_string_yes() {
        let plan = create_test_plan(vec![("active", LemmaType::Boolean)]);
        let json = br#"{"active": "yes"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("active") {
            Some(LiteralValue::Boolean(b)) => assert!(bool::from(b)),
            other => panic!("Expected Boolean, got {:?}", other),
        }
    }

    #[test]
    fn test_boolean_from_string_no() {
        let plan = create_test_plan(vec![("active", LemmaType::Boolean)]);
        let json = br#"{"active": "no"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("active") {
            Some(LiteralValue::Boolean(b)) => assert!(!bool::from(b)),
            other => panic!("Expected Boolean, got {:?}", other),
        }
    }

    #[test]
    fn test_boolean_rejects_number() {
        let plan = create_test_plan(vec![("active", LemmaType::Boolean)]);
        let json = br#"{"active": 1}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected boolean"));
        assert!(error_message.contains("got number"));
    }

    #[test]
    fn test_boolean_rejects_invalid_string() {
        let plan = create_test_plan(vec![("active", LemmaType::Boolean)]);
        let json = br#"{"active": "maybe"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Invalid boolean string"));
    }

    #[test]
    fn test_percentage_from_number() {
        let plan = create_test_plan(vec![("discount", LemmaType::Percentage)]);
        let json = br#"{"discount": 21}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("discount"),
            Some(&LiteralValue::Percentage(Decimal::from(21)))
        );
    }

    #[test]
    fn test_percentage_from_string_with_percent_sign() {
        let plan = create_test_plan(vec![("discount", LemmaType::Percentage)]);
        let json = br#"{"discount": "21%"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("discount"),
            Some(&LiteralValue::Percentage(Decimal::from(21)))
        );
    }

    #[test]
    fn test_percentage_from_string_with_percent_word() {
        let plan = create_test_plan(vec![("discount", LemmaType::Percentage)]);
        let json = br#"{"discount": "21 percent"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("discount"),
            Some(&LiteralValue::Percentage(Decimal::from(21)))
        );
    }

    #[test]
    fn test_percentage_from_bare_string() {
        let plan = create_test_plan(vec![("discount", LemmaType::Percentage)]);
        let json = br#"{"discount": "21"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("discount"),
            Some(&LiteralValue::Percentage(Decimal::from(21)))
        );
    }

    #[test]
    fn test_percentage_from_invalid_string() {
        let plan = create_test_plan(vec![("discount", LemmaType::Percentage)]);
        let json = br#"{"discount": "hello"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Invalid percentage string"));
    }

    #[test]
    fn test_percentage_rejects_boolean() {
        let plan = create_test_plan(vec![("discount", LemmaType::Percentage)]);
        let json = br#"{"discount": false}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected percentage"));
        assert!(error_message.contains("got boolean"));
    }

    #[test]
    fn test_date_from_string() {
        let plan = create_test_plan(vec![("start_date", LemmaType::Date)]);
        let json = br#"{"start_date": "2024-01-15"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("start_date") {
            Some(LiteralValue::Date(dt)) => {
                assert_eq!(dt.year, 2024);
                assert_eq!(dt.month, 1);
                assert_eq!(dt.day, 15);
            }
            other => panic!("Expected Date, got {:?}", other),
        }
    }

    #[test]
    fn test_date_rejects_number() {
        let plan = create_test_plan(vec![("start_date", LemmaType::Date)]);
        let json = br#"{"start_date": 20240115}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected date"));
        assert!(error_message.contains("got number"));
    }

    #[test]
    fn test_mass_from_string() {
        let plan = create_test_plan(vec![("weight", LemmaType::Mass)]);
        let json = br#"{"weight": "100 kilogram"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("weight") {
            Some(LiteralValue::Unit(unit)) => {
                assert_eq!(unit.value(), Decimal::from(100));
            }
            other => panic!("Expected Unit, got {:?}", other),
        }
    }

    #[test]
    fn test_mass_rejects_number() {
        let plan = create_test_plan(vec![("weight", LemmaType::Mass)]);
        let json = br#"{"weight": 100}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Unit values must include the unit name"));
    }

    #[test]
    fn test_unknown_fact_error() {
        let plan = create_test_plan(vec![("known", LemmaType::Text)]);
        let json = br#"{"unknown": "value"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Fact 'unknown' not found"));
        assert!(error_message.contains("Available facts"));
    }

    #[test]
    fn test_null_value_skipped() {
        let plan = create_test_plan(vec![("name", LemmaType::Text), ("age", LemmaType::Number)]);
        let json = br#"{"name": null, "age": 30}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.len(), 1);
        assert!(!result.contains_key("name"));
        assert_eq!(
            result.get("age"),
            Some(&LiteralValue::Number(Decimal::from(30)))
        );
    }

    #[test]
    fn test_all_null_values() {
        let plan = create_test_plan(vec![("name", LemmaType::Text)]);
        let json = br#"{"name": null}"#;
        let result = from_json(json, &plan).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_array_value_for_non_text() {
        let plan = create_test_plan(vec![("items", LemmaType::Number)]);
        let json = br#"{"items": [1, 2, 3]}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("got array"));
    }

    #[test]
    fn test_object_value_for_non_text() {
        let plan = create_test_plan(vec![("config", LemmaType::Number)]);
        let json = br#"{"config": {"key": "value"}}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("got object"));
    }

    #[test]
    fn test_mixed_valid_types() {
        let plan = create_test_plan(vec![
            ("name", LemmaType::Text),
            ("count", LemmaType::Number),
            ("active", LemmaType::Boolean),
            ("discount", LemmaType::Percentage),
        ]);
        let json = br#"{"name": "Test", "count": 5, "active": true, "discount": 21}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(
            result.get("name"),
            Some(&LiteralValue::Text("Test".to_string()))
        );
        assert_eq!(
            result.get("count"),
            Some(&LiteralValue::Number(Decimal::from(5)))
        );
        assert_eq!(
            result.get("discount"),
            Some(&LiteralValue::Percentage(Decimal::from(21)))
        );
    }

    #[test]
    fn test_invalid_json_syntax() {
        let plan = create_test_plan(vec![("name", LemmaType::Text)]);
        let json = br#"{"name": }"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("JSON parse error"));
    }
}
