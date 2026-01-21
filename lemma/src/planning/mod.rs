//! Planning module for Lemma documents
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with facts and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topologically sorted, ready for evaluation)
//! - Validates document structure and references

pub mod execution_plan;
pub mod graph;
pub mod types;
pub mod validation;

pub use execution_plan::{Branch, ExecutableRule, ExecutionPlan};
pub use types::TypeRegistry;

use crate::semantic::LemmaDoc;
use crate::LemmaError;
use std::collections::HashMap;

/// Builds an execution plan from Lemma documents.
///
/// The `sources` parameter maps source IDs (filenames) to their source code,
/// needed for extracting original expression text in proofs.
pub fn plan(
    main_doc: &LemmaDoc,
    all_docs: &[LemmaDoc],
    sources: HashMap<String, String>,
) -> Result<ExecutionPlan, Vec<LemmaError>> {
    validate_all_documents(all_docs)?;

    let graph = graph::Graph::build(main_doc, all_docs, sources)?;
    let execution_plan = execution_plan::build_execution_plan(&graph, &main_doc.name);
    let value_errors = execution_plan::validate_literal_facts_against_types(&execution_plan);
    if !value_errors.is_empty() {
        return Err(value_errors);
    }
    Ok(execution_plan)
}

/// Validate all documents
fn validate_all_documents(all_docs: &[LemmaDoc]) -> Result<(), Vec<LemmaError>> {
    let mut errors = Vec::new();

    // Pass all_docs to validate_types so cross-document type imports can resolve
    for doc in all_docs {
        if let Err(doc_errors) = validation::validate_types(doc, Some(all_docs)) {
            errors.extend(doc_errors);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod internal_tests {
    use super::plan;
    use crate::semantic::{FactPath, PathSegment};
    use crate::{parse, ResourceLimits};
    use std::collections::HashMap;

    #[test]
    fn test_basic_validation() {
        let input = r#"doc person
fact name = "John"
fact age = 25
rule is_adult = age >= 18"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        for doc in &docs {
            let result = plan(doc, &docs, sources.clone());
            assert!(
                result.is_ok(),
                "Basic validation should pass: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn test_duplicate_facts() {
        let input = r#"doc person
fact name = "John"
fact name = "Jane""#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan(&docs[0], &docs, sources);

        assert!(
            result.is_err(),
            "Duplicate facts should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("Duplicate fact"),
            "Error should mention duplicate fact: {}",
            error_string
        );
        assert!(error_string.contains("name"));
    }

    #[test]
    fn test_duplicate_rules() {
        let input = r#"doc person
fact age = 25
rule is_adult = age >= 18
rule is_adult = age >= 21"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan(&docs[0], &docs, sources);

        assert!(
            result.is_err(),
            "Duplicate rules should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("Duplicate rule"),
            "Error should mention duplicate rule: {}",
            error_string
        );
        assert!(error_string.contains("is_adult"));
    }

    #[test]
    fn test_circular_dependency() {
        let input = r#"doc test
rule a = b?
rule b = a?"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan(&docs[0], &docs, sources);

        assert!(
            result.is_err(),
            "Circular dependency should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(error_string.contains("Circular dependency") || error_string.contains("circular"));
    }

    #[test]
    fn test_reference_type_errors() {
        let input = r#"doc test
fact age = 25
rule is_adult = age >= 18
rule test1 = age?
rule test2 = is_adult"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan(&docs[0], &docs, sources);

        assert!(
            result.is_err(),
            "Reference type errors should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("is a rule, not a fact") || error_string.contains("Reference"),
            "Error should mention reference issue: {}",
            error_string
        );
    }

    #[test]
    fn test_multiple_documents() {
        let input = r#"doc person
fact name = "John"
fact age = 25

doc company
fact name = "Acme Corp"
fact employee = doc person"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan(&docs[0], &docs, sources);

        assert!(
            result.is_ok(),
            "Multiple documents should validate successfully: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_invalid_document_reference() {
        let input = r#"doc person
fact name = "John"
fact contract = doc nonexistent"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan(&docs[0], &docs, sources);

        assert!(
            result.is_err(),
            "Invalid document reference should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("not found") || error_string.contains("Document"),
            "Error should mention document reference issue: {}",
            error_string
        );
        assert!(error_string.contains("nonexistent"));
    }

    #[test]
    fn test_fact_override_with_custom_type_resolves_in_correct_document_context() {
        // This is a planning-level test: ensure fact overrides resolve custom types correctly
        // when the type is defined in a different document than the override.
        //
        // doc one:
        //   type money = number
        //   fact x = [money]
        // doc two:
        //   fact one = doc one
        //   fact one.x = 7
        //   rule getx = one.x
        let code = r#"
doc one
type money = number
fact x = [money]

doc two
fact one = doc one
fact one.x = 7
rule getx = one.x
"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc_two = docs.iter().find(|d| d.name == "two").unwrap();

        let execution_plan = plan(doc_two, &docs, HashMap::new()).expect("planning should succeed");

        // Verify that one.x has type 'money' (resolved from doc one)
        let one_x_path = FactPath {
            segments: vec![PathSegment {
                fact: "one".to_string(),
                doc: "one".to_string(),
            }],
            fact: "x".to_string(),
        };

        let one_x_type = execution_plan
            .fact_schema
            .get(&one_x_path)
            .expect("one.x should have a resolved type");

        assert_eq!(
            one_x_type.name(),
            "money",
            "one.x should have type 'money', got: {}",
            one_x_type.name()
        );
        assert!(one_x_type.is_number(), "money should be number-based");
    }
}
