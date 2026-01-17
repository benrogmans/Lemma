//! Semantic validation for Lemma documents
//!
//! Validates document structure and type declarations
//! to catch errors early with clear messages.

use crate::parsing::ast::Span;
use crate::semantic::{DateTimeValue, FactValue, LemmaDoc, TimeValue, TypeSpecification};
use crate::LemmaError;
use rust_decimal::Decimal;
use std::cmp::Ordering;
use std::sync::Arc;

/// Validate all types in a document
///
/// If `all_docs` is provided, all types from all documents are registered first,
/// allowing cross-document type imports to resolve correctly.
pub fn validate_types(
    document: &LemmaDoc,
    all_docs: Option<&[LemmaDoc]>,
) -> Result<(), Vec<LemmaError>> {
    use crate::planning::types::TypeRegistry;

    let mut errors = Vec::new();

    // Validate type declarations in facts (inline type definitions)
    for fact in &document.facts {
        if let FactValue::TypeDeclaration {
            base,
            overrides: _,
            from: _,
        } = &fact.value
        {
            // Basic validation - check that base is not empty
            if base.is_empty() {
                errors.push(LemmaError::engine(
                    "TypeDeclaration base cannot be empty",
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
        }
    }

    // Create type registry and register all types from all documents first
    // This allows cross-document type imports to resolve correctly
    let mut type_registry = TypeRegistry::new();
    if let Some(all_docs) = all_docs {
        for doc in all_docs {
            for type_def in &doc.types {
                if let Err(e) = type_registry.register_type(&doc.name, type_def.clone()) {
                    errors.push(e);
                }
            }
        }
    } else {
        // Fallback: only register types from current document
        for type_def in &document.types {
            if let Err(e) = type_registry.register_type(&document.name, type_def.clone()) {
                errors.push(e);
            }
        }
    }

    // Validate type definitions by resolving them and checking specifications
    // Resolve only named types (anonymous types are registered during graph building, not validation)
    match type_registry.resolve_named_types(&document.name) {
        Ok(resolved_types) => {
            // Validate each named type's specifications
            for (type_name, lemma_type) in &resolved_types.named_types {
                let mut spec_errors =
                    validate_type_specifications(&lemma_type.specifications, type_name);
                errors.append(&mut spec_errors);
            }
        }
        Err(e) => {
            errors.push(e);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate that TypeSpecification constraints are internally consistent
///
/// This checks:
/// - minimum <= maximum (for types that support ranges)
/// - default values satisfy all constraints
/// - length constraints are consistent (for Text)
/// - precision/decimals are within valid ranges
///
/// Returns a vector of errors (empty if valid)
pub(crate) fn validate_type_specifications(
    specs: &TypeSpecification,
    type_name: &str,
) -> Vec<LemmaError> {
    let mut errors = Vec::new();

    match specs {
        TypeSpecification::Scale {
            minimum,
            maximum,
            decimals,
            precision,
            default,
            units,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
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
            }

            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
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
            }

            // Validate precision is positive if set
            if let Some(prec) = precision {
                if *prec <= Decimal::ZERO {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid precision: {}. Must be positive",
                            type_name, prec
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
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
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
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
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
                }
            }

            // Validate units (if present)
            // Scale types can have units - validate them
            if !units.is_empty() {
                let mut seen_names: Vec<String> = Vec::new();
                for unit in units {
                    // Validate unit name is not empty
                    if unit.name.trim().is_empty() {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' has a unit with empty name. Unit names cannot be empty.",
                                type_name
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

                    // Validate unit names are unique within the type (case-insensitive)
                    let lower_name = unit.name.to_lowercase();
                    if seen_names
                        .iter()
                        .any(|seen| seen.to_lowercase() == lower_name)
                    {
                        errors.push(LemmaError::engine(
                            format!("Type '{}' has duplicate unit name '{}' (case-insensitive). Unit names must be unique within a type.", type_name, unit.name),
                            Span { start: 0, end: 0, line: 1, col: 0 },
                            "<unknown>",
                            Arc::from(""),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    // Validate unit values are positive (conversion factors relative to type base of 1)
                    if unit.value <= Decimal::ZERO {
                        errors.push(LemmaError::engine(
                            format!("Type '{}' has unit '{}' with invalid value {}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value),
                            Span { start: 0, end: 0, line: 1, col: 0 },
                            "<unknown>",
                            Arc::from(""),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                }
            }
        }
        TypeSpecification::Number {
            minimum,
            maximum,
            decimals,
            precision,
            default,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
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
            }

            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
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
            }

            // Validate precision is positive if set
            if let Some(prec) = precision {
                if *prec <= Decimal::ZERO {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid precision: {}. Must be positive",
                            type_name, prec
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
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
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
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
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
                }
            }
            // Note: Number types are dimensionless and cannot have units (validated in apply_override)
        }

        TypeSpecification::Ratio {
            minimum,
            maximum,
            default,
            units,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
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
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
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
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
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
                }
            }

            // Validate units (if present)
            // Types can have zero units (e.g., type ratio = number -> ratio) - this is valid
            // Only validate if units are defined
            if !units.is_empty() {
                let mut seen_names: Vec<String> = Vec::new();
                for unit in units {
                    // Validate unit name is not empty
                    if unit.name.trim().is_empty() {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' has a unit with empty name. Unit names cannot be empty.",
                                type_name
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

                    // Validate unit names are unique within the type (case-insensitive)
                    let lower_name = unit.name.to_lowercase();
                    if seen_names
                        .iter()
                        .any(|seen| seen.to_lowercase() == lower_name)
                    {
                        errors.push(LemmaError::engine(
                            format!("Type '{}' has duplicate unit name '{}' (case-insensitive). Unit names must be unique within a type.", type_name, unit.name),
                            Span { start: 0, end: 0, line: 1, col: 0 },
                            "<unknown>",
                            Arc::from(""),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    // Validate unit values are positive (conversion factors relative to type base of 1)
                    if unit.value <= Decimal::ZERO {
                        errors.push(LemmaError::engine(
                            format!("Type '{}' has unit '{}' with invalid value {}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value),
                            Span { start: 0, end: 0, line: 1, col: 0 },
                            "<unknown>",
                            Arc::from(""),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                }
            }
        }

        TypeSpecification::Text {
            minimum,
            maximum,
            length,
            options,
            default,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(LemmaError::engine(
                        format!("Type '{}' has invalid range: minimum length {} is greater than maximum length {}", type_name, min, max),
                        Span { start: 0, end: 0, line: 1, col: 0 },
                        "<unknown>",
                        Arc::from(""),
                        "<unknown>",
                        1,
                        None::<String>,
                    ));
                }
            }

            // Validate length consistency
            if let Some(len) = length {
                if let Some(min) = minimum {
                    if *len < *min {
                        errors.push(LemmaError::engine(
                            format!("Type '{}' has inconsistent length constraint: length {} is less than minimum {}", type_name, len, min),
                            Span { start: 0, end: 0, line: 1, col: 0 },
                            "<unknown>",
                            Arc::from(""),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *len > *max {
                        errors.push(LemmaError::engine(
                            format!("Type '{}' has inconsistent length constraint: length {} is greater than maximum {}", type_name, len, max),
                            Span { start: 0, end: 0, line: 1, col: 0 },
                            "<unknown>",
                            Arc::from(""),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                }
            }

            // Validate default value constraints
            if let Some(def) = default {
                let def_len = def.len();

                if let Some(min) = minimum {
                    if def_len < *min {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default value length {} is less than minimum {}",
                                type_name, def_len, min
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
                }
                if let Some(max) = maximum {
                    if def_len > *max {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default value length {} is greater than maximum {}",
                                type_name, def_len, max
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
                }
                if let Some(len) = length {
                    if def_len != *len {
                        errors.push(LemmaError::engine(
                            format!("Type '{}' default value length {} does not match required length {}", type_name, def_len, len),
                            Span { start: 0, end: 0, line: 1, col: 0 },
                            "<unknown>",
                            Arc::from(""),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                }
                if !options.is_empty() && !options.contains(def) {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' default value '{}' is not in allowed options: {:?}",
                            type_name, def, options
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
            }
        }

        TypeSpecification::Date {
            minimum,
            maximum,
            default,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if compare_date_values(min, max) == Ordering::Greater {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid date range: minimum {} is after maximum {}",
                            type_name, min, max
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
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if compare_date_values(def, min) == Ordering::Less {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default date {} is before minimum {}",
                                type_name, def, min
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
                }
                if let Some(max) = maximum {
                    if compare_date_values(def, max) == Ordering::Greater {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default date {} is after maximum {}",
                                type_name, def, max
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
                }
            }
        }

        TypeSpecification::Time {
            minimum,
            maximum,
            default,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if compare_time_values(min, max) == Ordering::Greater {
                    errors.push(LemmaError::engine(
                        format!(
                            "Type '{}' has invalid time range: minimum {} is after maximum {}",
                            type_name, min, max
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
            }

            // Validate default value constraints
            if let Some(def) = default {
                if let Some(min) = minimum {
                    if compare_time_values(def, min) == Ordering::Less {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default time {} is before minimum {}",
                                type_name, def, min
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
                }
                if let Some(max) = maximum {
                    if compare_time_values(def, max) == Ordering::Greater {
                        errors.push(LemmaError::engine(
                            format!(
                                "Type '{}' default time {} is after maximum {}",
                                type_name, def, max
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
                }
            }
        }

        TypeSpecification::Boolean { .. } | TypeSpecification::Duration { .. } => {
            // No constraint validation needed for these types
        }
        TypeSpecification::Veto { .. } => {
            // Veto is not a user-declarable type, so validation should not be called on it
            // But if it is, there's nothing to validate
        }
    }

    errors
}

/// Compare two DateTimeValue instances for ordering
fn compare_date_values(left: &DateTimeValue, right: &DateTimeValue) -> Ordering {
    // Compare by year, month, day, hour, minute, second
    left.year
        .cmp(&right.year)
        .then_with(|| left.month.cmp(&right.month))
        .then_with(|| left.day.cmp(&right.day))
        .then_with(|| left.hour.cmp(&right.hour))
        .then_with(|| left.minute.cmp(&right.minute))
        .then_with(|| left.second.cmp(&right.second))
}

/// Compare two TimeValue instances for ordering
fn compare_time_values(left: &TimeValue, right: &TimeValue) -> Ordering {
    // Compare by hour, minute, second
    left.hour
        .cmp(&right.hour)
        .then_with(|| left.minute.cmp(&right.minute))
        .then_with(|| left.second.cmp(&right.second))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::{FactReference, FactValue, LemmaFact, LiteralValue, TypeSpecification};
    use rust_decimal::Decimal;

    fn make_doc(name: &str) -> LemmaDoc {
        LemmaDoc::new(name.to_string())
    }

    fn make_fact(name: &str) -> LemmaFact {
        LemmaFact {
            reference: FactReference::local(name.to_string()),
            value: FactValue::Literal(LiteralValue::number(Decimal::from(1))),
            source_location: None,
        }
    }

    #[test]
    fn validate_basic_document() {
        let mut doc = make_doc("test");
        doc.facts.push(make_fact("age"));

        let result = validate_types(&doc, None);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_number_minimum_greater_than_maximum() {
        let mut specs = TypeSpecification::number();
        specs = specs
            .apply_override("minimum", &["100".to_string()])
            .unwrap();
        specs = specs
            .apply_override("maximum", &["50".to_string()])
            .unwrap();

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum 100 is greater than maximum 50"));
    }

    #[test]
    fn validate_number_valid_range() {
        let mut specs = TypeSpecification::number();
        specs = specs.apply_override("minimum", &["0".to_string()]).unwrap();
        specs = specs
            .apply_override("maximum", &["100".to_string()])
            .unwrap();

        let errors = validate_type_specifications(&specs, "test");
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_number_default_below_minimum() {
        let specs = TypeSpecification::Number {
            minimum: Some(Decimal::from(10)),
            maximum: None,
            decimals: None,
            precision: None,
            help: None,
            default: Some(Decimal::from(5)),
        };

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("default value 5 is less than minimum 10"));
    }

    #[test]
    fn validate_number_default_above_maximum() {
        let specs = TypeSpecification::Number {
            minimum: None,
            maximum: Some(Decimal::from(100)),
            decimals: None,
            precision: None,
            help: None,
            default: Some(Decimal::from(150)),
        };

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("default value 150 is greater than maximum 100"));
    }

    #[test]
    fn validate_number_default_valid() {
        let specs = TypeSpecification::Number {
            minimum: Some(Decimal::from(0)),
            maximum: Some(Decimal::from(100)),
            decimals: None,
            precision: None,
            help: None,
            default: Some(Decimal::from(50)),
        };

        let errors = validate_type_specifications(&specs, "test");
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_text_minimum_greater_than_maximum() {
        let mut specs = TypeSpecification::text();
        specs = specs
            .apply_override("minimum", &["100".to_string()])
            .unwrap();
        specs = specs
            .apply_override("maximum", &["50".to_string()])
            .unwrap();

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum length 100 is greater than maximum length 50"));
    }

    #[test]
    fn validate_text_length_inconsistent_with_minimum() {
        let mut specs = TypeSpecification::text();
        specs = specs
            .apply_override("minimum", &["10".to_string()])
            .unwrap();
        specs = specs.apply_override("length", &["5".to_string()]).unwrap();

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("length 5 is less than minimum 10"));
    }

    #[test]
    fn validate_text_default_not_in_options() {
        let specs = TypeSpecification::Text {
            minimum: None,
            maximum: None,
            length: None,
            options: vec!["red".to_string(), "blue".to_string()],
            help: None,
            default: Some("green".to_string()),
        };

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("default value 'green' is not in allowed options"));
    }

    #[test]
    fn validate_text_default_valid_in_options() {
        let specs = TypeSpecification::Text {
            minimum: None,
            maximum: None,
            length: None,
            options: vec!["red".to_string(), "blue".to_string()],
            help: None,
            default: Some("red".to_string()),
        };

        let errors = validate_type_specifications(&specs, "test");
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_ratio_minimum_greater_than_maximum() {
        let specs = TypeSpecification::Ratio {
            minimum: Some(Decimal::from(2)),
            maximum: Some(Decimal::from(1)),
            units: vec![],
            help: None,
            default: None,
        };

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum 2 is greater than maximum 1"));
    }

    #[test]
    fn validate_date_minimum_after_maximum() {
        let mut specs = TypeSpecification::date();
        specs = specs
            .apply_override("minimum", &["2024-12-31".to_string()])
            .unwrap();
        specs = specs
            .apply_override("maximum", &["2024-01-01".to_string()])
            .unwrap();

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].to_string().contains("minimum")
                && errors[0].to_string().contains("is after maximum")
        );
    }

    #[test]
    fn validate_date_valid_range() {
        let mut specs = TypeSpecification::date();
        specs = specs
            .apply_override("minimum", &["2024-01-01".to_string()])
            .unwrap();
        specs = specs
            .apply_override("maximum", &["2024-12-31".to_string()])
            .unwrap();

        let errors = validate_type_specifications(&specs, "test");
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_time_minimum_after_maximum() {
        let mut specs = TypeSpecification::time();
        specs = specs
            .apply_override("minimum", &["23:00:00".to_string()])
            .unwrap();
        specs = specs
            .apply_override("maximum", &["10:00:00".to_string()])
            .unwrap();

        let errors = validate_type_specifications(&specs, "test");
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].to_string().contains("minimum")
                && errors[0].to_string().contains("is after maximum")
        );
    }

    #[test]
    fn validate_type_definition_with_invalid_constraints() {
        use crate::semantic::TypeDef;

        let type_def = TypeDef::Regular {
            name: "invalid_money".to_string(),
            parent: "number".to_string(),
            overrides: Some(vec![
                ("minimum".to_string(), vec!["100".to_string()]),
                ("maximum".to_string(), vec!["50".to_string()]),
            ]),
        };

        let mut doc = make_doc("test");
        doc.types.push(type_def);

        let result = validate_types(&doc, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e
            .to_string()
            .contains("minimum 100 is greater than maximum 50")));
    }
}
