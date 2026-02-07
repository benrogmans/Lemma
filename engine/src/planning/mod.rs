//! Planning module for Lemma documents
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with facts and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topologically sorted, ready for evaluation)
//! - Validates document structure and references

pub mod execution_plan;
pub mod graph;
pub mod semantics;
pub mod types;
pub mod validation;

pub use execution_plan::{Branch, ExecutableRule, ExecutionPlan};
pub use semantics::{
    ArithmeticComputation, ComparisonComputation, Expression, ExpressionKind, Fact, FactData,
    FactPath, FactValue, LemmaType, LiteralValue, LogicalComputation, MathematicalComputation,
    NegationType, PathSegment, RulePath, Source, Span, TypeExtends, ValueKind, VetoExpression,
};
pub use types::TypeRegistry;

use crate::parsing::ast::LemmaDoc;
use crate::LemmaError;
use std::collections::HashMap;
use std::sync::Arc;

/// Builds an execution plan from Lemma documents.
///
/// The `sources` parameter maps source IDs (filenames) to their source code,
/// needed for extracting original expression text in proofs.
pub fn plan(
    main_doc: &LemmaDoc,
    all_docs: &[LemmaDoc],
    sources: HashMap<String, String>,
) -> Result<ExecutionPlan, Vec<LemmaError>> {
    // Collect pre-graph validation errors.  Duplicate document names are fatal
    // (they cause HashMap key conflicts), so we return immediately for those.
    // Other validation errors (e.g. structural type issues) are collected and
    // merged with graph-build errors so the caller sees as many diagnostics as
    // possible in a single pass.
    let validation_errors = match validate_all_documents(all_docs, &sources) {
        Ok(()) => Vec::new(),
        Err(errs) => {
            // If any error is a duplicate-document-name error, return immediately —
            // the graph builder relies on unique document names.
            let has_fatal = errs
                .iter()
                .any(|e| e.message().contains("Duplicate document name"));
            if has_fatal {
                return Err(errs);
            }
            errs
        }
    };

    let graph = match graph::Graph::build(main_doc, all_docs, sources) {
        Ok(graph) => {
            if !validation_errors.is_empty() {
                return Err(validation_errors);
            }
            graph
        }
        Err(mut build_errors) => {
            // Merge pre-validation errors in front so they appear first.
            let mut all_errors = validation_errors;
            all_errors.append(&mut build_errors);
            return Err(all_errors);
        }
    };

    let execution_plan = execution_plan::build_execution_plan(&graph, &main_doc.name);
    let value_errors = execution_plan::validate_literal_facts_against_types(&execution_plan);
    if !value_errors.is_empty() {
        return Err(value_errors);
    }
    Ok(execution_plan)
}

/// Validate all documents before building the graph.
///
/// This checks for duplicate document names (which would silently overwrite each other
/// in HashMap-based lookups) and validates types in each document.
fn validate_all_documents(
    all_docs: &[LemmaDoc],
    sources: &HashMap<String, String>,
) -> Result<(), Vec<LemmaError>> {
    let mut errors = Vec::new();

    // Detect duplicate document names. Two documents with the same name would silently
    // overwrite each other in the HashMap used by Graph::build. This must be a fatal error.
    let mut seen_document_names: HashMap<&str, &LemmaDoc> = HashMap::new();
    for doc in all_docs {
        if let Some(earlier_doc) = seen_document_names.get(doc.name.as_str()) {
            let attribute = doc.attribute.as_deref().unwrap_or(&doc.name);
            let source_text: Arc<str> = sources
                .get(attribute)
                .map(|text| Arc::from(text.as_str()))
                .unwrap_or_else(|| Arc::from(""));
            let earlier_attribute = earlier_doc
                .attribute
                .as_deref()
                .unwrap_or(&earlier_doc.name);
            errors.push(LemmaError::semantic(
                format!(
                    "Duplicate document name '{}' (previously declared in '{}')",
                    doc.name, earlier_attribute
                ),
                Source::new(
                    attribute,
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: doc.start_line,
                        col: 0,
                    },
                    doc.name.clone(),
                ),
                source_text,
                None::<String>,
            ));
        } else {
            seen_document_names.insert(&doc.name, doc);
        }
    }

    // Return duplicate-name errors immediately — no point validating types if names collide.
    if !errors.is_empty() {
        return Err(errors);
    }

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
    use crate::planning::semantics::{FactPath, PathSegment};
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
    fn test_fact_binding_with_custom_type_resolves_in_correct_document_context() {
        // This is a planning-level test: ensure fact bindings resolve custom types correctly
        // when the type is defined in a different document than the binding.
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
            .facts
            .get(&one_x_path)
            .and_then(|d| d.schema_type())
            .expect("one.x should have a resolved type");

        assert_eq!(
            one_x_type.name(),
            "money",
            "one.x should have type 'money', got: {}",
            one_x_type.name()
        );
        assert!(one_x_type.is_number(), "money should be number-based");
    }

    #[test]
    fn test_duplicate_document_names_are_rejected() {
        let source_a = r#"doc pricing
fact base_price = 100"#;
        let source_b = r#"doc pricing
fact base_price = 200"#;

        let docs_a = parse(source_a, "file_a.lemma", &ResourceLimits::default()).unwrap();
        let docs_b = parse(source_b, "file_b.lemma", &ResourceLimits::default()).unwrap();

        let all_docs: Vec<_> = docs_a.into_iter().chain(docs_b).collect();
        let mut sources = HashMap::new();
        sources.insert("file_a.lemma".to_string(), source_a.to_string());
        sources.insert("file_b.lemma".to_string(), source_b.to_string());

        let result = plan(&all_docs[0], &all_docs, sources);

        assert!(
            result.is_err(),
            "Duplicate document names should cause a validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("Duplicate document name"),
            "Error should mention duplicate document name: {}",
            error_string
        );
        assert!(
            error_string.contains("pricing"),
            "Error should mention the duplicate name 'pricing': {}",
            error_string
        );
    }

    #[test]
    fn test_plan_with_registry_style_doc_names() {
        let source = r#"doc user/workspace/somedoc
fact quantity = 10

doc user/workspace/example
fact inventory = doc @user/workspace/somedoc
rule total_quantity = inventory.quantity"#;

        let docs = parse(source, "registry_bundle.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(docs.len(), 2);

        let example_doc = docs
            .iter()
            .find(|d| d.name == "user/workspace/example")
            .expect("should find user/workspace/example");

        let mut sources = HashMap::new();
        sources.insert("registry_bundle.lemma".to_string(), source.to_string());

        let result = plan(example_doc, &docs, sources);
        assert!(
            result.is_ok(),
            "Planning with @... document names should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_multiple_independent_errors_are_all_reported() {
        // A document referencing a non-existing type import AND a non-existing
        // document should report errors for BOTH, not just stop at the first.
        let source = r#"doc demo
type money from nonexistent_type_source
fact helper = doc nonexistent_doc
fact price = 10
rule total = helper.value + price"#;

        let docs = parse(source, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

        let result = plan(&docs[0], &docs, sources);
        assert!(result.is_err(), "Planning should fail with multiple errors");

        let errors = result.unwrap_err();
        let all_messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let combined = all_messages.join("\n");

        // Must report the type resolution error (shows up as "Unknown type: 'money'")
        assert!(
            combined.contains("Unknown type") && combined.contains("money"),
            "Should report type import error for 'money'. Got:\n{}",
            combined
        );

        // Must ALSO report the document reference error — this is the bug we fixed:
        // previously only the type error was reported and the doc reference error
        // was swallowed by the early return.
        assert!(
            combined.contains("nonexistent_doc"),
            "Should report doc reference error for 'nonexistent_doc'. Got:\n{}",
            combined
        );

        // Should have at least 2 distinct kinds of errors (type + doc ref)
        assert!(
            errors.len() >= 2,
            "Expected at least 2 errors, got {}: {}",
            errors.len(),
            combined
        );
    }

    #[test]
    fn test_type_error_does_not_suppress_cross_doc_fact_error() {
        // When a type import fails, errors about cross-document fact references
        // (e.g. ext.some_fact where ext is a doc ref to a non-existing doc)
        // must still be reported.
        let source = r#"doc demo
type currency from missing_doc
fact ext = doc also_missing
rule val = ext.some_fact"#;

        let docs = parse(source, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

        let result = plan(&docs[0], &docs, sources);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        let combined: String = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        // The type error about 'currency' should be reported
        assert!(
            combined.contains("currency"),
            "Should report type error about 'currency'. Got:\n{}",
            combined
        );

        // The document reference error about 'also_missing' should ALSO be reported
        assert!(
            combined.contains("also_missing"),
            "Should report error about 'also_missing'. Got:\n{}",
            combined
        );
    }
}
