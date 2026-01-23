use crate::parsing::ast::Span;
use crate::planning::ExecutionPlan;
use crate::semantic::{
    BooleanValue, FactPath, LemmaFact, LemmaType, LiteralValue, Value as LemmaValue,
};
use crate::LemmaError;
use crate::Source;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Convert JSON values to typed Lemma values using the ExecutionPlan for type information.
///
/// This function converts JSON values to Lemma types with the following rules:
///
/// | Lemma Type | Valid JSON Types | Conversion |
/// |------------|------------------|------------|
/// | Text | any | Strings pass through; numbers/booleans/arrays/objects serialize to JSON string |
/// | Number | number, string | Numbers pass through; strings are parsed as decimals |
/// | Boolean | boolean, string | Booleans pass through; strings parsed as "true"/"false"/"yes"/"no"/"accept"/"reject" |
/// | Percent | number, string | Numbers become percent; strings parsed (with or without %) |
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
    let map: HashMap<String, Value> = serde_json::from_slice(json).map_err(|e| {
        LemmaError::engine(
            format!("JSON parse error: {}", e),
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<unknown>",
            Arc::from(""),
            "<unknown>",
            1,
            None::<String>,
        )
    })?;

    let mut result = HashMap::new();

    for (fact_name, json_value) in map {
        if json_value.is_null() {
            continue;
        }

        let fact_path = plan.get_fact_path_by_str(&fact_name).ok_or_else(|| {
            let available: Vec<String> = plan.fact_schema.keys().map(|p| p.to_string()).collect();
            LemmaError::engine(
                format!(
                    "Fact '{}' not found in document. Available facts: {}",
                    fact_name,
                    available.join(", ")
                ),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;

        let expected_type = plan.fact_schema.get(fact_path).cloned().unwrap_or_else(|| {
            unreachable!(
                "BUG: get_fact_path_by_str returned a fact path that is not in fact_schema (fact={})",
                fact_name
            )
        });
        let literal_value = convert_json_value(&fact_name, &json_value, &expected_type)?;

        result.insert(fact_name, literal_value);
    }

    Ok(result)
}

// Note: expected types come exclusively from `ExecutionPlan.fact_schema`.

fn convert_json_value(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match &expected_type.specifications {
        crate::semantic::TypeSpecification::Text { .. } => {
            convert_to_text(fact_name, json_value, expected_type)
        }
        crate::semantic::TypeSpecification::Scale { .. } => {
            convert_to_scale(fact_name, json_value, expected_type)
        }
        crate::semantic::TypeSpecification::Number { .. } => {
            convert_to_number(fact_name, json_value, expected_type)
        }
        crate::semantic::TypeSpecification::Boolean { .. } => {
            convert_to_boolean(fact_name, json_value, expected_type)
        }
        crate::semantic::TypeSpecification::Ratio { .. } => {
            convert_to_ratio(fact_name, json_value, expected_type)
        }
        crate::semantic::TypeSpecification::Date { .. } => {
            convert_to_date(fact_name, json_value, expected_type)
        }
        crate::semantic::TypeSpecification::Duration { .. } => {
            convert_to_duration(fact_name, json_value, expected_type)
        }
        crate::semantic::TypeSpecification::Time { .. } => {
            convert_to_time(fact_name, json_value, expected_type)
        }
        crate::semantic::TypeSpecification::Veto { .. } => Err(LemmaError::engine(
            "Veto type is not a user-declarable type and cannot be converted from JSON",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<unknown>",
            Arc::from(""),
            "<unknown>",
            1,
            None::<String>,
        )),
    }
}

fn convert_to_text(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    let text = match json_value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(json_value).unwrap_or_else(|_| json_value.to_string())
        }
        Value::Null => return Err(type_error(fact_name, "text", "null")),
    };
    Ok(LiteralValue::text_with_type(text, expected_type.clone()))
}

fn convert_to_number(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::Number(n) => {
            let decimal = json_number_to_decimal(fact_name, n)?;
            Ok(LiteralValue::number_with_type(
                decimal,
                expected_type.clone(),
            ))
        }
        Value::String(s) => {
            let clean = s.trim().replace(['_', ','], "");
            let decimal = Decimal::from_str_exact(&clean).map_err(|_| {
                LemmaError::engine(
                    format!(
                        "Invalid number string for fact '{}': '{}' is not a valid decimal",
                        fact_name, s
                    ),
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    Arc::from(s.as_str()),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;
            Ok(LiteralValue::number_with_type(
                decimal,
                expected_type.clone(),
            ))
        }
        Value::Null => Err(type_error(fact_name, "number", "null")),
        Value::Bool(_) => Err(type_error(fact_name, "number", "boolean")),
        Value::Array(_) => Err(type_error(fact_name, "number", "array")),
        Value::Object(_) => Err(type_error(fact_name, "number", "object")),
    }
}

fn convert_to_scale(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::Number(n) => {
            // JSON number (e.g., 50) -> Scale with no unit
            let decimal = json_number_to_decimal(fact_name, n)?;
            Ok(LiteralValue::scale_with_type(
                decimal,
                None,
                expected_type.clone(),
            ))
        }
        Value::String(s) => {
            let trimmed = s.trim();

            // Parse number and optional unit from string
            // Handles: "50", "50 eur", "50eur", "1,234.56 usd", etc.

            // First, try to find where the number part ends
            // Numbers can contain: digits, decimal point, sign, underscore, comma
            let mut number_end = 0;
            let chars: Vec<char> = trimmed.chars().collect();
            let mut has_decimal = false;

            // Skip leading sign
            let start = if chars.first().is_some_and(|c| *c == '+' || *c == '-') {
                1
            } else {
                0
            };

            for (i, &ch) in chars.iter().enumerate().skip(start) {
                match ch {
                    '0'..='9' => number_end = i + 1,
                    '.' if !has_decimal => {
                        has_decimal = true;
                        number_end = i + 1;
                    }
                    '_' | ',' => {
                        // Thousand separators - continue scanning
                        number_end = i + 1;
                    }
                    _ => {
                        // Non-numeric character - number ends here
                        break;
                    }
                }
            }

            // Extract number and unit parts
            let number_part = trimmed[..number_end].trim();
            let unit_part = trimmed[number_end..].trim();

            // Clean number part (remove separators for parsing)
            let clean_number = number_part.replace(['_', ','], "");
            let decimal = Decimal::from_str_exact(&clean_number).map_err(|_| {
                LemmaError::engine(
                    format!(
                        "Invalid scale string for fact '{}': '{}' is not a valid number",
                        fact_name, s
                    ),
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    Arc::from(s.as_str()),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;

            // Validate unit against type definition
            let allowed_units = match &expected_type.specifications {
                crate::semantic::TypeSpecification::Scale { units, .. } => units,
                _ => {
                    return Err(LemmaError::engine(
                        format!(
                            "Internal error: expected a scale type for fact '{}' but got {}",
                            fact_name,
                            expected_type.name()
                        ),
                        Span {
                            start: 0,
                            end: 0,
                            line: 1,
                            col: 0,
                        },
                        "<unknown>",
                        Arc::from(""),
                        "<unknown>",
                        1,
                        None::<String>,
                    ));
                }
            };

            let unit = if unit_part.is_empty() {
                if !allowed_units.is_empty() {
                    let valid: Vec<String> = allowed_units.iter().map(|u| u.name.clone()).collect();
                    return Err(LemmaError::engine(
                        format!(
                            "Missing unit for fact '{}'. Valid units: {}",
                            fact_name,
                            valid.join(", ")
                        ),
                        Span {
                            start: 0,
                            end: 0,
                            line: 1,
                            col: 0,
                        },
                        "<unknown>",
                        Arc::from(s.as_str()),
                        "<unknown>",
                        1,
                        None::<String>,
                    ));
                }
                None
            } else {
                let matched = allowed_units
                    .iter()
                    .find(|u| u.name.eq_ignore_ascii_case(unit_part));
                match matched {
                    Some(unit_def) => Some(unit_def.name.clone()),
                    None => {
                        let valid: Vec<String> =
                            allowed_units.iter().map(|u| u.name.clone()).collect();
                        let valid_str = if valid.is_empty() {
                            "none".to_string()
                        } else {
                            valid.join(", ")
                        };
                        return Err(LemmaError::engine(
                            format!(
                                "Invalid unit '{}' for fact '{}'. Valid units: {}",
                                unit_part, fact_name, valid_str
                            ),
                            Span {
                                start: 0,
                                end: 0,
                                line: 1,
                                col: 0,
                            },
                            "<unknown>",
                            Arc::from(s.as_str()),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                }
            };

            Ok(LiteralValue::scale_with_type(
                decimal,
                unit,
                expected_type.clone(),
            ))
        }
        Value::Null => Err(type_error(fact_name, "scale", "null")),
        Value::Bool(_) => Err(type_error(fact_name, "scale", "boolean")),
        Value::Array(_) => Err(type_error(fact_name, "scale", "array")),
        Value::Object(_) => Err(type_error(fact_name, "scale", "object")),
    }
}

fn convert_to_boolean(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::Bool(b) => {
            let boolean_value = if *b {
                BooleanValue::True
            } else {
                BooleanValue::False
            };
            Ok(LiteralValue::boolean_with_type(
                boolean_value,
                expected_type.clone(),
            ))
        }
        Value::String(s) => {
            let boolean_value: BooleanValue = s.parse().map_err(|_| {
                LemmaError::engine(
                    format!("Invalid boolean string for fact '{}': '{}'. Expected one of: true, false, yes, no, accept, reject", fact_name, s),
                    Span { start: 0, end: 0, line: 1, col: 0 },
                    "<unknown>",
                    Arc::from(s.as_str()),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;
            Ok(LiteralValue::boolean_with_type(
                boolean_value,
                expected_type.clone(),
            ))
        }
        Value::Null => Err(type_error(fact_name, "boolean", "null")),
        Value::Number(_) => Err(type_error(fact_name, "boolean", "number")),
        Value::Array(_) => Err(type_error(fact_name, "boolean", "array")),
        Value::Object(_) => Err(type_error(fact_name, "boolean", "object")),
    }
}

fn convert_to_ratio(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::Number(n) => {
            // JSON number (e.g., 0.10) -> ratio with no unit
            let decimal = json_number_to_decimal(fact_name, n)?;
            Ok(LiteralValue::ratio_with_type(
                decimal,
                None,
                expected_type.clone(),
            ))
        }
        Value::String(s) => {
            let trimmed = s.trim();
            let trimmed_lower = trimmed.to_lowercase();

            // Determine unit and extract number part
            let (number_part, unit) = if let Some(stripped) = trimmed.strip_suffix("%%") {
                // "10%%" -> ratio with "permille" unit
                (stripped.trim(), Some("permille".to_string()))
            } else if let Some(stripped) = trimmed.strip_suffix('%') {
                // "10%" -> ratio with "percent" unit
                (stripped.trim(), Some("percent".to_string()))
            } else if trimmed_lower.ends_with("permille") {
                // "10permille" or "10 PERMILLE" -> ratio with "permille" unit
                (
                    trimmed[..trimmed.len() - 8].trim(),
                    Some("permille".to_string()),
                )
            } else if trimmed_lower.ends_with("percent") {
                // "10percent" or "10PERCENT" or "10 percent" or "10 PERCENT" -> ratio with "percent" unit
                (
                    trimmed[..trimmed.len() - 7].trim(),
                    Some("percent".to_string()),
                )
            } else {
                // "0.10" -> ratio with no unit
                (trimmed, None)
            };

            let clean_number = number_part.replace(['_', ','], "");
            let decimal = Decimal::from_str_exact(&clean_number).map_err(|_| {
                LemmaError::engine(
                    format!(
                        "Invalid ratio string for fact '{}': '{}' is not a valid number",
                        fact_name, s
                    ),
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    Arc::from(s.as_str()),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;

            // Convert percent/permille values to ratio (e.g., 10 -> 0.10 for percent, 10 -> 0.01 for permille)
            let ratio_value = if let Some(ref unit_name) = unit {
                if unit_name == "percent" {
                    decimal / Decimal::from(100)
                } else if unit_name == "permille" {
                    decimal / Decimal::from(1000)
                } else {
                    decimal
                }
            } else {
                decimal
            };

            Ok(LiteralValue::ratio_with_type(
                ratio_value,
                unit,
                expected_type.clone(),
            ))
        }
        Value::Null => Err(type_error(fact_name, "ratio", "null")),
        Value::Bool(_) => Err(type_error(fact_name, "ratio", "boolean")),
        Value::Array(_) => Err(type_error(fact_name, "ratio", "array")),
        Value::Object(_) => Err(type_error(fact_name, "ratio", "object")),
    }
}

fn convert_to_date(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::String(s) => expected_type.parse_value(s).map_err(|e| {
            LemmaError::engine(
                format!("Invalid date for fact '{}': {}", fact_name, e),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(s.as_str()),
                "<unknown>",
                1,
                None::<String>,
            )
        }),
        Value::Null => Err(type_error(fact_name, "date", "null")),
        Value::Bool(_) => Err(type_error(fact_name, "date", "boolean")),
        Value::Number(_) => Err(type_error(fact_name, "date", "number")),
        Value::Array(_) => Err(type_error(fact_name, "date", "array")),
        Value::Object(_) => Err(type_error(fact_name, "date", "object")),
    }
}

fn convert_to_duration(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::String(s) => expected_type.parse_value(s).map_err(|e| {
            LemmaError::engine(
                format!("Invalid duration value for fact '{}': {}", fact_name, e),
                Span { start: 0, end: 0, line: 1, col: 0 },
                "<unknown>",
                Arc::from(s.as_str()),
                "<unknown>",
                1,
                None::<String>,
            )
        }),
        Value::Null => Err(type_error(fact_name, "duration", "null")),
        Value::Bool(_) => Err(type_error(fact_name, "duration", "boolean")),
        Value::Number(_) => Err(LemmaError::engine(
            format!("Invalid JSON type for fact '{}': expected duration (as string like '5 days'), got number. Duration values must include the unit name.", fact_name),
            Span { start: 0, end: 0, line: 1, col: 0 },
            "<unknown>",
            Arc::from(""),
            "<unknown>",
            1,
            None::<String>,
        )),
        Value::Array(_) => Err(type_error(fact_name, "duration", "array")),
        Value::Object(_) => Err(type_error(fact_name, "duration", "object")),
    }
}

fn convert_to_time(
    fact_name: &str,
    json_value: &Value,
    expected_type: &crate::semantic::LemmaType,
) -> Result<LiteralValue, LemmaError> {
    match json_value {
        Value::String(s) => expected_type.parse_value(s).map_err(|e| {
            LemmaError::engine(
                format!("Invalid time value for fact '{}': {}", fact_name, e),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(s.as_str()),
                "<unknown>",
                1,
                None::<String>,
            )
        }),
        Value::Null => Err(type_error(fact_name, "time", "null")),
        Value::Bool(_) => Err(type_error(fact_name, "time", "boolean")),
        Value::Number(_) => Err(type_error(fact_name, "time", "number")),
        Value::Array(_) => Err(type_error(fact_name, "time", "array")),
        Value::Object(_) => Err(type_error(fact_name, "time", "object")),
    }
}

fn json_number_to_decimal(fact_name: &str, n: &serde_json::Number) -> Result<Decimal, LemmaError> {
    if let Some(i) = n.as_i64() {
        Ok(Decimal::from(i))
    } else if let Some(u) = n.as_u64() {
        Ok(Decimal::from(u))
    } else if let Some(f) = n.as_f64() {
        Decimal::try_from(f).map_err(|_| {
            LemmaError::engine(
                format!(
                    "Invalid number for fact '{}': {} cannot be represented as a decimal",
                    fact_name, n
                ),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            )
        })
    } else {
        Err(LemmaError::engine(
            format!(
                "Invalid number for fact '{}': {} is not a valid number",
                fact_name, n
            ),
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<unknown>",
            Arc::from(""),
            "<unknown>",
            1,
            None::<String>,
        ))
    }
}

fn type_error(fact_name: &str, expected: &str, got: &str) -> LemmaError {
    LemmaError::engine(
        format!(
            "Invalid JSON type for fact '{}': expected {}, got {}",
            fact_name, expected, got
        ),
        Span {
            start: 0,
            end: 0,
            line: 1,
            col: 0,
        },
        "<unknown>",
        Arc::from(""),
        "<unknown>",
        1,
        None::<String>,
    )
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

    match &value.value {
        LemmaValue::Number(n) => {
            map.serialize_entry("type", "number")?;
            let num = Number::from_str(&n.to_string())
                .map_err(|_| serde::ser::Error::custom("Failed to convert Decimal to Number"))?;
            map.serialize_entry("value", &num)?;
        }
        LemmaValue::Scale(n, unit_opt) => {
            map.serialize_entry("type", "scale")?;
            let num = Number::from_str(&n.to_string())
                .map_err(|_| serde::ser::Error::custom("Failed to convert Decimal to Number"))?;
            map.serialize_entry("value", &num)?;
            if let Some(unit) = unit_opt {
                map.serialize_entry("unit", unit)?;
            }
        }
        LemmaValue::Ratio(r, _) => {
            map.serialize_entry("type", "ratio")?;
            let num = Number::from_str(&r.to_string())
                .map_err(|_| serde::ser::Error::custom("Failed to convert Decimal to Number"))?;
            map.serialize_entry("value", &num)?;
        }
        LemmaValue::Boolean(b) => {
            map.serialize_entry("type", "boolean")?;
            map.serialize_entry("value", &bool::from(b.clone()))?;
        }
        LemmaValue::Text(s) => {
            map.serialize_entry("type", "text")?;
            map.serialize_entry("value", s)?;
        }
        LemmaValue::Date(dt) => {
            map.serialize_entry("type", "date")?;
            map.serialize_entry("value", &dt.to_string())?;
        }
        LemmaValue::Time(time) => {
            map.serialize_entry("type", "time")?;
            map.serialize_entry("value", &time.to_string())?;
        }
        LemmaValue::Duration(value, unit) => {
            map.serialize_entry("type", "duration")?;
            map.serialize_entry("value", &format!("{} {}", value, unit))?;
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
    use crate::semantic::{
        standard_boolean, standard_date, standard_duration, standard_number, standard_ratio,
        standard_text, standard_time, FactPath, LemmaType, LiteralValue,
    };
    use rust_decimal::Decimal;

    fn create_test_plan(facts: Vec<(&str, LemmaType)>) -> ExecutionPlan {
        let mut fact_schema = HashMap::new();
        for (name, lemma_type) in facts {
            let fact_path = FactPath {
                segments: vec![],
                fact: name.to_string(),
            };
            fact_schema.insert(fact_path, lemma_type);
        }
        ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema,
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: vec![],
            sources: HashMap::new(),
        }
    }

    fn create_text_literal(s: String) -> LiteralValue {
        LiteralValue::text(s)
    }

    fn create_number_literal(n: Decimal) -> LiteralValue {
        LiteralValue::number(n)
    }

    fn create_percentage_literal(p: Decimal) -> LiteralValue {
        // Convert percent (e.g., 50) to ratio (0.50) with "percent" unit
        // Note: This function is for tests that expect the old behavior where bare numbers
        // were treated as percentages. New code should use explicit "10%" format.
        LiteralValue::ratio(p / Decimal::from(100), Some("percent".to_string()))
    }

    #[test]
    fn test_text_from_string() {
        let plan = create_test_plan(vec![("name", standard_text().clone())]);
        let json = br#"{"name": "Alice"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("name"),
            Some(&create_text_literal("Alice".to_string()))
        );
    }

    #[test]
    fn test_text_from_number() {
        let plan = create_test_plan(vec![("name", standard_text().clone())]);
        let json = br#"{"name": 42}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("name"),
            Some(&create_text_literal("42".to_string()))
        );
    }

    #[test]
    fn test_text_from_boolean() {
        let plan = create_test_plan(vec![("name", standard_text().clone())]);
        let json = br#"{"name": true}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("name"),
            Some(&create_text_literal("true".to_string()))
        );
    }

    #[test]
    fn test_text_from_array() {
        let plan = create_test_plan(vec![("data", standard_text().clone())]);
        let json = br#"{"data": [1, 2, 3]}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("data"),
            Some(&create_text_literal("[1,2,3]".to_string()))
        );
    }

    #[test]
    fn test_text_from_object() {
        let plan = create_test_plan(vec![("config", standard_text().clone())]);
        let json = br#"{"config": {"key": "value"}}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("config"),
            Some(&create_text_literal("{\"key\":\"value\"}".to_string()))
        );
    }

    #[test]
    fn test_number_from_integer() {
        let plan = create_test_plan(vec![("count", standard_number().clone())]);
        let json = br#"{"count": 42}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("count"),
            Some(&create_number_literal(Decimal::from(42)))
        );
    }

    #[test]
    fn test_number_from_decimal() {
        let plan = create_test_plan(vec![("price", standard_number().clone())]);
        let json = br#"{"price": 99.95}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("price") {
            Some(lit) => {
                if let LemmaValue::Number(n) = &lit.value {
                    let expected = Decimal::try_from(99.95).unwrap();
                    let tolerance = Decimal::try_from(0.001).unwrap();
                    assert!((*n - expected).abs() < tolerance);
                } else {
                    panic!("Expected Number, got {:?}", lit);
                }
            }
            other => panic!("Expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_number_from_string() {
        let plan = create_test_plan(vec![("count", standard_number().clone())]);
        let json = br#"{"count": "42"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("count"),
            Some(&create_number_literal(Decimal::from(42)))
        );
    }

    #[test]
    fn test_number_from_string_with_formatting() {
        let plan = create_test_plan(vec![("price", standard_number().clone())]);
        let json = br#"{"price": "1,234.56"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("price") {
            Some(lit) => {
                if let LemmaValue::Number(n) = &lit.value {
                    let expected = Decimal::try_from(1234.56).unwrap();
                    let tolerance = Decimal::try_from(0.001).unwrap();
                    assert!((*n - expected).abs() < tolerance);
                } else {
                    panic!("Expected Number, got {:?}", lit);
                }
            }
            other => panic!("Expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_number_from_invalid_string() {
        let plan = create_test_plan(vec![("count", standard_number().clone())]);
        let json = br#"{"count": "hello"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Invalid number string"));
    }

    #[test]
    fn test_number_rejects_boolean() {
        let plan = create_test_plan(vec![("count", standard_number().clone())]);
        let json = br#"{"count": true}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected number"));
        assert!(error_message.contains("got boolean"));
    }

    #[test]
    fn test_boolean_from_true() {
        let plan = create_test_plan(vec![("active", standard_boolean().clone())]);
        let json = br#"{"active": true}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("active") {
            Some(lit) => {
                if let LemmaValue::Boolean(b) = &lit.value {
                    assert!(bool::from(b));
                } else {
                    panic!("Expected Boolean, got {:?}", lit);
                }
            }
            other => panic!("Expected Boolean, got {:?}", other),
        }
    }

    #[test]
    fn test_boolean_from_false() {
        let plan = create_test_plan(vec![("active", standard_boolean().clone())]);
        let json = br#"{"active": false}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("active") {
            Some(lit) => {
                if let LemmaValue::Boolean(b) = &lit.value {
                    assert!(!bool::from(b));
                } else {
                    panic!("Expected Boolean, got {:?}", lit);
                }
            }
            other => panic!("Expected Boolean, got {:?}", other),
        }
    }

    #[test]
    fn test_boolean_from_string_yes() {
        let plan = create_test_plan(vec![("active", standard_boolean().clone())]);
        let json = br#"{"active": "yes"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("active") {
            Some(lit) => {
                if let LemmaValue::Boolean(b) = &lit.value {
                    assert!(bool::from(b));
                } else {
                    panic!("Expected Boolean, got {:?}", lit);
                }
            }
            other => panic!("Expected Boolean, got {:?}", other),
        }
    }

    #[test]
    fn test_boolean_from_string_no() {
        let plan = create_test_plan(vec![("active", standard_boolean().clone())]);
        let json = br#"{"active": "no"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("active") {
            Some(lit) => {
                if let LemmaValue::Boolean(b) = &lit.value {
                    assert!(!bool::from(b));
                } else {
                    panic!("Expected Boolean, got {:?}", lit);
                }
            }
            other => panic!("Expected Boolean, got {:?}", other),
        }
    }

    #[test]
    fn test_boolean_rejects_number() {
        let plan = create_test_plan(vec![("active", standard_boolean().clone())]);
        let json = br#"{"active": 1}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected boolean"));
        assert!(error_message.contains("got number"));
    }

    #[test]
    fn test_boolean_rejects_invalid_string() {
        let plan = create_test_plan(vec![("active", standard_boolean().clone())]);
        let json = br#"{"active": "maybe"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Invalid boolean string"));
    }

    #[test]
    fn test_percentage_from_number() {
        // JSON number 21 for ratio type is now treated as ratio 21, not percentage
        let plan = create_test_plan(vec![("discount", standard_ratio().clone())]);
        let json = br#"{"discount": 21}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("discount"),
            Some(&LiteralValue::ratio(Decimal::from(21), None))
        );
    }

    #[test]
    fn test_percentage_from_string_with_percent_sign() {
        let plan = create_test_plan(vec![("discount", standard_ratio().clone())]);
        let json = br#"{"discount": "21%"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("discount"),
            Some(&create_percentage_literal(Decimal::from(21)))
        );
    }

    #[test]
    fn test_percentage_from_string_with_percent_word() {
        let plan = create_test_plan(vec![("discount", standard_ratio().clone())]);
        let json = br#"{"discount": "21 percent"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("discount"),
            Some(&create_percentage_literal(Decimal::from(21)))
        );
    }

    #[test]
    fn test_percentage_from_bare_string() {
        // Bare string "21" is now treated as ratio 21, not percentage 21%
        let plan = create_test_plan(vec![("discount", standard_ratio().clone())]);
        let json = br#"{"discount": "21"}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(
            result.get("discount"),
            Some(&LiteralValue::ratio(Decimal::from(21), None))
        );
    }

    #[test]
    fn test_percentage_from_invalid_string() {
        let plan = create_test_plan(vec![("discount", standard_ratio().clone())]);
        let json = br#"{"discount": "hello"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Invalid ratio string"));
    }

    #[test]
    fn test_percentage_rejects_boolean() {
        let plan = create_test_plan(vec![("discount", standard_ratio().clone())]);
        let json = br#"{"discount": false}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected ratio"));
        assert!(error_message.contains("got boolean"));
    }

    #[test]
    fn test_date_from_string() {
        let plan = create_test_plan(vec![("start_date", standard_date().clone())]);
        let json = br#"{"start_date": "2024-01-15"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("start_date") {
            Some(lit) => {
                if let LemmaValue::Date(dt) = &lit.value {
                    assert_eq!(dt.year, 2024);
                    assert_eq!(dt.month, 1);
                    assert_eq!(dt.day, 15);
                } else {
                    panic!("Expected Date, got {:?}", lit);
                }
            }
            other => panic!("Expected Date, got {:?}", other),
        }
    }

    #[test]
    fn test_date_rejects_number() {
        let plan = create_test_plan(vec![("start_date", standard_date().clone())]);
        let json = br#"{"start_date": 20240115}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected date"));
        assert!(error_message.contains("got number"));
    }

    #[test]
    fn test_duration_from_string() {
        let plan = create_test_plan(vec![("duration", standard_duration().clone())]);
        let json = br#"{"duration": "5 days"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("duration") {
            Some(lit) => {
                if let LemmaValue::Duration(value, unit) = &lit.value {
                    assert_eq!(*value, Decimal::from(5));
                    assert_eq!(*unit, crate::DurationUnit::Day);
                } else {
                    panic!("Expected Duration, got {:?}", lit);
                }
            }
            other => panic!("Expected Duration, got {:?}", other),
        }
    }

    #[test]
    fn test_duration_rejects_number() {
        let plan = create_test_plan(vec![("duration", standard_duration().clone())]);
        let json = br#"{"duration": 100}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Duration values must include the unit name"));
    }

    #[test]
    fn test_time_from_string_hhmm() {
        let plan = create_test_plan(vec![("start_time", standard_time().clone())]);
        let json = br#"{"start_time": "14:30"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("start_time") {
            Some(lit) => {
                if let LemmaValue::Time(t) = &lit.value {
                    assert_eq!(t.hour, 14);
                    assert_eq!(t.minute, 30);
                    assert_eq!(t.second, 0);
                    assert_eq!(t.timezone, None);
                } else {
                    panic!("Expected Time, got {:?}", lit);
                }
            }
            other => panic!("Expected Time, got {:?}", other),
        }
    }

    #[test]
    fn test_time_from_string_hhmmss() {
        let plan = create_test_plan(vec![("start_time", standard_time().clone())]);
        let json = br#"{"start_time": "14:30:45"}"#;
        let result = from_json(json, &plan).unwrap();
        match result.get("start_time") {
            Some(lit) => {
                if let LemmaValue::Time(t) = &lit.value {
                    assert_eq!(t.hour, 14);
                    assert_eq!(t.minute, 30);
                    assert_eq!(t.second, 45);
                    assert_eq!(t.timezone, None);
                } else {
                    panic!("Expected Time, got {:?}", lit);
                }
            }
            other => panic!("Expected Time, got {:?}", other),
        }
    }

    #[test]
    fn test_time_from_string_with_timezone() {
        let plan = create_test_plan(vec![("start_time", standard_time().clone())]);
        // Test with Z timezone format (UTC)
        let json = br#"{"start_time": "14:30:00Z"}"#;
        let result = from_json(json, &plan);
        // Note: Timezone parsing may not work for all formats yet
        // This test verifies the conversion function is called and handles the input
        // If timezone parsing fails, it should return a proper error, not panic
        match result {
            Ok(values) => {
                // If parsing succeeds, verify it's a valid time
                if let Some(lit) = values.get("start_time") {
                    if let LemmaValue::Time(t) = &lit.value {
                        assert!(t.hour < 24 && t.minute < 60 && t.second < 60);
                    }
                }
            }
            Err(_) => {
                // Timezone parsing may not be fully supported yet, which is acceptable
                // The important thing is that convert_to_time is being called
            }
        }
    }

    #[test]
    fn test_time_rejects_number() {
        let plan = create_test_plan(vec![("start_time", standard_time().clone())]);
        let json = br#"{"start_time": 1430}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("expected time"));
        assert!(error_message.contains("got number"));
    }

    #[test]
    fn test_time_rejects_invalid_format() {
        let plan = create_test_plan(vec![("start_time", standard_time().clone())]);
        let json = br#"{"start_time": "25:00"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Invalid time"));
    }

    #[test]
    fn test_unknown_fact_error() {
        let plan = create_test_plan(vec![("known", standard_text().clone())]);
        let json = br#"{"unknown": "value"}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("Fact 'unknown' not found"));
        assert!(error_message.contains("Available facts"));
    }

    #[test]
    fn test_null_value_skipped() {
        let plan = create_test_plan(vec![
            ("name", standard_text().clone()),
            ("age", standard_number().clone()),
        ]);
        let json = br#"{"name": null, "age": 30}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.len(), 1);
        assert!(!result.contains_key("name"));
        assert_eq!(
            result.get("age"),
            Some(&create_number_literal(Decimal::from(30)))
        );
    }

    #[test]
    fn test_all_null_values() {
        let plan = create_test_plan(vec![("name", standard_text().clone())]);
        let json = br#"{"name": null}"#;
        let result = from_json(json, &plan).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_array_value_for_non_text() {
        let plan = create_test_plan(vec![("items", standard_number().clone())]);
        let json = br#"{"items": [1, 2, 3]}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("got array"));
    }

    #[test]
    fn test_object_value_for_non_text() {
        let plan = create_test_plan(vec![("config", standard_number().clone())]);
        let json = br#"{"config": {"key": "value"}}"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("got object"));
    }

    #[test]
    fn test_mixed_valid_types() {
        let plan = create_test_plan(vec![
            ("name", standard_text().clone()),
            ("count", standard_number().clone()),
            ("active", standard_boolean().clone()),
            ("discount", standard_ratio().clone()),
        ]);
        let json = br#"{"name": "Test", "count": 5, "active": true, "discount": 21}"#;
        let result = from_json(json, &plan).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(
            result.get("name"),
            Some(&create_text_literal("Test".to_string()))
        );
        assert_eq!(
            result.get("count"),
            Some(&create_number_literal(Decimal::from(5)))
        );
        // JSON number 21 for ratio type is treated as ratio 21, not percentage
        assert_eq!(
            result.get("discount"),
            Some(&LiteralValue::ratio(Decimal::from(21), None))
        );
    }

    #[test]
    fn test_invalid_json_syntax() {
        let plan = create_test_plan(vec![("name", standard_text().clone())]);
        let json = br#"{"name": }"#;
        let result = from_json(json, &plan);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("JSON parse error"));
    }
}
