//! Planning module for Lemma documents
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with facts and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topologically sorted, ready for evaluation)
//! - Validates document structure and references

pub mod content_hash;
pub mod execution_plan;
pub mod graph;
pub mod semantics;
pub mod slice_interface;
pub mod temporal;
pub mod types;
pub mod validation;
pub use execution_plan::{Branch, DocumentSchema, ExecutableRule, ExecutionPlan};
pub use semantics::{
    negated_comparison, ArithmeticComputation, ComparisonComputation, Expression, ExpressionKind,
    Fact, FactData, FactPath, FactValue, LemmaType, LiteralValue, LogicalComputation,
    MathematicalComputation, NegationType, PathSegment, RulePath, Source, Span, TypeExtends,
    ValueKind, VetoExpression,
};
pub use types::TypeResolver;

use crate::engine::Context;
use crate::parsing::ast::LemmaDoc;
use crate::Error;
use std::collections::HashMap;
use std::sync::Arc;

/// Result of planning a single document: the document, its execution plans (if any), and errors produced while planning it.
#[derive(Debug, Clone)]
pub struct DocPlanningResult {
    /// The document we were planning (the one this result is for).
    pub document: Arc<LemmaDoc>,
    /// Execution plans for that document (one per temporal interval; empty if planning failed).
    pub plans: Vec<ExecutionPlan>,
    /// All planning errors produced while planning this document.
    pub errors: Vec<Error>,
    /// Content hash of this document (hash pin, 8 lowercase hex chars).
    pub hash_pin: String,
}

/// Result of running plan() across the context: per-document results and global errors (e.g. temporal coverage).
#[derive(Debug, Clone)]
pub struct PlanningResult {
    /// One result per document we attempted to plan.
    pub per_document: Vec<DocPlanningResult>,
    /// Errors not tied to a single document (e.g. from validate_temporal_coverage).
    pub global_errors: Vec<Error>,
}

/// Build execution plans for one or more Lemma documents.
///
/// Context is immutable — types are resolved transiently and never stored in
/// Context. The flow:
/// 1. TypeResolver registers + resolves named types → HashMap
/// 2. Per-document Graph::build augments the HashMap with inline types
/// 3. ExecutionPlan is built from the graph (types baked into facts/rules)
///
/// Returns a PlanningResult: per-document results (document, plans, errors) and global errors.
/// When displaying errors, iterate per_document and for each with non-empty errors output "In document 'X':" then each error.
pub fn plan(context: &Context, sources: HashMap<String, String>) -> PlanningResult {
    let mut global_errors: Vec<Error> = Vec::new();
    global_errors.extend(temporal::validate_temporal_coverage(context));

    let mut type_resolver = TypeResolver::new();
    let all_docs: Vec<_> = context.iter().collect();
    for doc_arc in &all_docs {
        global_errors.extend(type_resolver.register_all(doc_arc));
    }
    let (mut resolved_types, type_errors) = type_resolver.resolve(all_docs.clone());
    global_errors.extend(type_errors);

    let mut per_document: Vec<DocPlanningResult> = Vec::new();

    if !global_errors.is_empty() {
        return PlanningResult {
            per_document,
            global_errors,
        };
    }

    // Compute content hashes for all docs (own content only for now).
    // TODO: bottom-up transitive hashing once dep resolution order is settled.
    let doc_hashes: graph::DocContentHashes = all_docs
        .iter()
        .map(|d| (graph::doc_hash_key(d), content_hash::hash_doc(d, &[])))
        .collect();

    for doc_arc in &all_docs {
        let slices = temporal::compute_temporal_slices(doc_arc, context);
        let mut doc_plans: Vec<ExecutionPlan> = Vec::new();
        let mut doc_errors: Vec<Error> = Vec::new();
        let mut slice_resolved_types: Vec<HashMap<Arc<LemmaDoc>, types::ResolvedDocumentTypes>> =
            Vec::new();

        for slice in &slices {
            match graph::Graph::build(
                doc_arc,
                context,
                sources.clone(),
                &type_resolver,
                &resolved_types,
                slice.from.clone(),
                &doc_hashes,
            ) {
                Ok((graph, doc_types)) => {
                    for (arc, types) in &doc_types {
                        resolved_types.insert(Arc::clone(arc), types.clone());
                    }
                    let execution_plan = execution_plan::build_execution_plan(
                        &graph,
                        doc_arc.name.as_str(),
                        slice.from.clone(),
                        slice.to.clone(),
                    );
                    let value_errors =
                        execution_plan::validate_literal_facts_against_types(&execution_plan);
                    if value_errors.is_empty() {
                        doc_plans.push(execution_plan);
                    } else {
                        doc_errors.extend(value_errors);
                    }
                    slice_resolved_types.push(doc_types);
                }
                Err(doc_errors_from_build) => {
                    doc_errors.extend(doc_errors_from_build);
                }
            }
        }

        if doc_errors.is_empty() && doc_plans.len() > 1 {
            doc_errors.extend(slice_interface::validate_slice_interfaces(
                &doc_arc.name,
                &doc_plans,
                &slice_resolved_types,
            ));
        }

        let hash = doc_hashes
            .get(&graph::doc_hash_key(doc_arc))
            .cloned()
            .unwrap_or_default();

        per_document.push(DocPlanningResult {
            document: Arc::clone(doc_arc),
            plans: doc_plans,
            errors: doc_errors,
            hash_pin: hash,
        });
    }

    PlanningResult {
        per_document,
        global_errors,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod internal_tests {
    use super::plan;
    use crate::engine::Context;
    use crate::parsing::ast::{FactValue, LemmaDoc, LemmaFact, Reference, Span};
    use crate::parsing::source::Source;
    use crate::planning::execution_plan::ExecutionPlan;
    use crate::planning::semantics::{FactPath, PathSegment};
    use crate::{parse, Error, ResourceLimits};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Test helper: plan a single document and return its execution plan.
    fn plan_single(
        main_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
        sources: HashMap<String, String>,
    ) -> Result<ExecutionPlan, Vec<Error>> {
        let mut ctx = Context::new();
        for doc in all_docs {
            if let Err(e) = ctx.insert_doc(Arc::new(doc.clone())) {
                return Err(vec![e]);
            }
        }
        let main_doc_arc = ctx
            .get_doc_effective_from(main_doc.name.as_str(), main_doc.effective_from())
            .expect("main_doc must be in all_docs");
        let result = plan(&ctx, sources);
        let all_errors: Vec<Error> = result
            .global_errors
            .into_iter()
            .chain(
                result
                    .per_document
                    .iter()
                    .flat_map(|r| r.errors.clone())
                    .collect::<Vec<_>>(),
            )
            .collect();
        if !all_errors.is_empty() {
            return Err(all_errors);
        }
        match result
            .per_document
            .into_iter()
            .find(|r| Arc::ptr_eq(&r.document, &main_doc_arc))
        {
            Some(doc_result) if !doc_result.plans.is_empty() => {
                let mut plans = doc_result.plans;
                Ok(plans.remove(0))
            }
            _ => Err(vec![Error::validation(
                format!(
                    "No execution plan produced for document '{}'",
                    main_doc.name
                ),
                Some(crate::planning::semantics::Source::new(
                    "<test>",
                    crate::planning::semantics::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    main_doc.name.clone(),
                    std::sync::Arc::from("doc test\nfact x: 1"),
                )),
                None::<String>,
            )]),
        }
    }

    #[test]
    fn test_basic_validation() {
        let input = r#"doc person
fact name: "John"
fact age: 25
rule is_adult: age >= 18"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        for doc in &docs {
            let result = plan_single(doc, &docs, sources.clone());
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
fact name: "John"
fact name: "Jane""#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&docs[0], &docs, sources);

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
fact age: 25
rule is_adult: age >= 18
rule is_adult: age >= 21"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&docs[0], &docs, sources);

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
rule a: b
rule b: a"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&docs[0], &docs, sources);

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
    fn test_unified_references_work() {
        let input = r#"doc test
fact age: 25
rule is_adult: age >= 18
rule test1: age
rule test2: is_adult"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&docs[0], &docs, sources);

        assert!(
            result.is_ok(),
            "Unified references should work: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_multiple_documents() {
        let input = r#"doc person
fact name: "John"
fact age: 25

doc company
fact name: "Acme Corp"
fact employee: doc person"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&docs[0], &docs, sources);

        assert!(
            result.is_ok(),
            "Multiple documents should validate successfully: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_invalid_document_reference() {
        let input = r#"doc person
fact name: "John"
fact contract: doc nonexistent"#;

        let docs = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&docs[0], &docs, sources);

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
            error_string.contains("not found")
                || error_string.contains("Document")
                || (error_string.contains("nonexistent") && error_string.contains("depends")),
            "Error should mention document reference issue: {}",
            error_string
        );
        assert!(error_string.contains("nonexistent"));
    }

    #[test]
    fn test_type_declaration_empty_base_returns_lemma_error() {
        let mut doc = LemmaDoc::new("test".to_string());
        let source = Source::new(
            "test.lemma",
            Span {
                start: 0,
                end: 10,
                line: 1,
                col: 0,
            },
            "test",
            Arc::from("fact x: []"),
        );
        doc.facts.push(LemmaFact::new(
            Reference {
                segments: vec![],
                name: "x".to_string(),
            },
            FactValue::TypeDeclaration {
                base: String::new(),
                constraints: None,
                from: None,
            },
            source,
        ));

        let docs = vec![doc.clone()];
        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), "doc test\nfact x: []".to_string());

        let result = plan_single(&doc, &docs, sources);
        assert!(
            result.is_err(),
            "TypeDeclaration with empty base should fail planning"
        );
        let errors = result.unwrap_err();
        let combined = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            combined.contains("TypeDeclaration base cannot be empty"),
            "Error should mention empty base; got: {}",
            combined
        );
    }

    #[test]
    fn test_fact_binding_with_custom_type_resolves_in_correct_document_context() {
        // This is a planning-level test: ensure fact bindings resolve custom types correctly
        // when the type is defined in a different document than the binding.
        //
        // doc one:
        //   type money: number
        //   fact x: [money]
        // doc two:
        //   fact one: doc one
        //   fact one.x: 7
        //   rule getx: one.x
        let code = r#"
doc one
type money: number
fact x: [money]

doc two
fact one: doc one
fact one.x: 7
rule getx: one.x
"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc_two = docs.iter().find(|d| d.name == "two").unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());
        let execution_plan = plan_single(doc_two, &docs, sources).expect("planning should succeed");

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
    fn test_plan_with_registry_style_doc_names() {
        let source = r#"doc user/workspace/somedoc
fact quantity: 10

doc user/workspace/example
fact inventory: doc @user/workspace/somedoc
rule total_quantity: inventory.quantity"#;

        let docs = parse(source, "registry_bundle.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(docs.len(), 2);

        let example_doc = docs
            .iter()
            .find(|d| d.name == "user/workspace/example")
            .expect("should find user/workspace/example");

        let mut sources = HashMap::new();
        sources.insert("registry_bundle.lemma".to_string(), source.to_string());

        let result = plan_single(example_doc, &docs, sources);
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
fact helper: doc nonexistent_doc
fact price: 10
rule total: helper.value + price"#;

        let docs = parse(source, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

        let result = plan_single(&docs[0], &docs, sources);
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

        // Must also report the document reference error (not just the type error)
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
fact ext: doc also_missing
rule val: ext.some_fact"#;

        let docs = parse(source, "test.lemma", &ResourceLimits::default()).unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

        let result = plan_single(&docs[0], &docs, sources);
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
