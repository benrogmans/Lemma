use crate::{LemmaDoc, LemmaError, LemmaType};
use serde_json::Value;
use std::collections::HashMap;

/// Serialize a JSON value to Lemma syntax based on expected type
fn serialize_value(value: &Value, fact_type: &LemmaType) -> Result<String, LemmaError> {
    match fact_type {
        LemmaType::Text => match value {
            Value::String(s) => Ok(format!("\"{}\"", s)),
            _ => Err(LemmaError::Engine(format!(
                "Expected string for Text, got {:?}",
                value
            ))),
        },
        LemmaType::Number => match value {
            Value::Number(n) => Ok(n.to_string()),
            Value::String(s) => s
                .trim()
                .parse::<f64>()
                .map(|_| s.trim().to_string())
                .map_err(|_| LemmaError::Engine(format!("Invalid number string: '{}'", s))),
            _ => Err(LemmaError::Engine(format!(
                "Expected number or string for Number, got {:?}",
                value
            ))),
        },
        LemmaType::Percentage => match value {
            Value::Number(n) => {
                let decimal = n.as_f64().ok_or_else(|| {
                    LemmaError::Engine(format!("Invalid number for percentage: {:?}", n))
                })?;
                Ok(format!("{}%", decimal * 100.0))
            }
            Value::String(s) => Ok(s.clone()),
            _ => Err(LemmaError::Engine(format!(
                "Expected number or string for Percentage, got {:?}",
                value
            ))),
        },
        LemmaType::Boolean => match value {
            Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
            Value::String(s) => Ok(s.clone()),
            _ => Err(LemmaError::Engine(format!(
                "Expected boolean or string for Boolean, got {:?}",
                value
            ))),
        },
        LemmaType::Date => match value {
            Value::String(s) => Ok(s.clone()),
            _ => Err(LemmaError::Engine(format!(
                "Expected string for Date, got {:?}",
                value
            ))),
        },
        LemmaType::Regex => match value {
            Value::String(s) => {
                if s.starts_with('/') && s.ends_with('/') {
                    Ok(s.clone())
                } else {
                    Ok(format!("/{}/", s))
                }
            }
            _ => Err(LemmaError::Engine(format!(
                "Expected string for Regex, got {:?}",
                value
            ))),
        },
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
        | LemmaType::Data
        | LemmaType::Money => match value {
            Value::String(s) => Ok(s.clone()),
            _ => Err(LemmaError::Engine(format!(
                "Expected string with value and unit for {:?} (e.g., \"100 kilogram\"), got {:?}",
                fact_type, value
            ))),
        },
    }
}

/// Convert JSON fact overrides to Lemma syntax strings
///
/// Expected JSON formats:
/// - Text: "string value"
/// - Number: 123 or 45.67 as number, or "123" as string (parsed and passed through)
/// - Percentage: 0.21 as number (becomes "21%"), or "21%" as string (passed through)
/// - Boolean: true/false as boolean (becomes "true"/"false"), or "yes"/"no"/"accept"/etc as string (passed through)
/// - Date: "2024-01-15" or "2024-01-15T14:30:00Z"
/// - Regex: "pattern" or "/pattern/" (passed through as string)
/// - Unit types: "100 kilogram" (Lemma syntax as string)
///
/// Example:
/// ```json
/// {
///   "name": "John",
///   "rate": 0.21,
///   "active": true,
///   "weight": "75 kilogram",
///   "start_date": "2024-01-15"
/// }
/// ```
pub fn to_lemma_syntax(
    json: &[u8],
    doc: &LemmaDoc,
    all_docs: &HashMap<String, LemmaDoc>,
) -> Result<Vec<String>, crate::LemmaError> {
    let map: HashMap<String, Value> = serde_json::from_slice(json)
        .map_err(|e| crate::LemmaError::Engine(format!("JSON parse error: {}", e)))?;

    let mut lemma_strings = Vec::new();

    for (name, value) in map {
        let fact_type = super::find_fact_type(&name, doc, all_docs)?;
        let lemma_value = serialize_value(&value, &fact_type)?;
        lemma_strings.push(format!("{}={}", name, lemma_value));
    }

    Ok(lemma_strings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, LemmaResult};

    #[test]
    fn test_percentage_as_number() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact discount = 10%
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        // Number 0.9 (decimal) should become "90%"
        let json = r#"{"discount": 0.9}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "discount=90%");
        Ok(())
    }

    #[test]
    fn test_percentage_as_string_with_percent() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact discount = 10%
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        // String "90%" should become "90%"
        let json = r#"{"discount": "90%"}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "discount=90%");
        Ok(())
    }

    #[test]
    fn test_percentage_as_string_without_percent() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact discount = 10%
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        // String "90" should be passed through as-is
        let json = r#"{"discount": "90"}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "discount=90");
        Ok(())
    }

    #[test]
    fn test_text_with_quotes() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact name = "Alice"
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        let json = r#"{"name": "Bob"}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], r#"name="Bob""#);
        Ok(())
    }

    #[test]
    fn test_number_as_string() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact age = 30
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        let json = r#"{"age": "42"}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "age=42");
        Ok(())
    }

    #[test]
    fn test_unit_as_string() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact price = 100 USD
            fact weight = 50 kilogram
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        let json = r#"{"price": "200 USD", "weight": "75 kilogram"}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 2);
        assert!(result.contains(&"price=200 USD".to_string()));
        assert!(result.contains(&"weight=75 kilogram".to_string()));
        Ok(())
    }

    #[test]
    fn test_boolean_values() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact active = false
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        let json = r#"{"active": true}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "active=true");
        Ok(())
    }

    #[test]
    fn test_boolean_as_string() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact status = yes
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        let json = r#"{"status": "no"}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "status=no");
        Ok(())
    }

    #[test]
    fn test_date_as_string() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact start_date = 2024-01-01
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        let json = r#"{"start_date": "2024-12-25"}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "start_date=2024-12-25");
        Ok(())
    }

    #[test]
    fn test_mixed_types() -> LemmaResult<()> {
        let mut engine = Engine::new();
        engine.add_lemma_code(
            r#"
            doc test
            fact name = "Alice"
            fact age = 30
            fact discount = 10%
            fact active = true
            fact price = 100 USD
            "#,
            "test.lemma",
        )?;

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        let json = r#"{
            "name": "Bob",
            "age": 25,
            "discount": 0.15,
            "active": false,
            "price": "200 USD"
        }"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs)?;

        assert_eq!(result.len(), 5);
        assert!(result.contains(&r#"name="Bob""#.to_string()));
        assert!(result.contains(&"age=25".to_string()));
        assert!(result.contains(&"discount=15%".to_string()));
        assert!(result.contains(&"active=false".to_string()));
        assert!(result.contains(&"price=200 USD".to_string()));
        Ok(())
    }

    #[test]
    fn test_type_mismatch_error() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
            doc test
            fact age = 30
            "#,
                "test.lemma",
            )
            .unwrap();

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        // String for number type should error
        let json = r#"{"age": "not a number"}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs);

        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_fact_error() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
            doc test
            fact age = 30
            "#,
                "test.lemma",
            )
            .unwrap();

        let doc = engine.get_document("test").unwrap();
        let all_docs = engine.get_all_documents();

        // Unknown fact should error
        let json = r#"{"unknown_fact": 42}"#;
        let result = to_lemma_syntax(json.as_bytes(), doc, all_docs);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
