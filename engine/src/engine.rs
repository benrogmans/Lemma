use crate::evaluation::Evaluator;
use crate::parsing::ast::LemmaDoc;
use crate::registry::Registry;
use crate::{parse, LemmaError, LemmaResult, ResourceLimits, Response};
use std::collections::HashMap;
use std::sync::Arc;

/// Engine for evaluating Lemma rules
///
/// Pure Rust implementation that evaluates Lemma docs directly from the AST.
/// Uses pre-built execution plans that are self-contained and ready for evaluation.
///
/// An optional Registry can be configured to resolve external `@...` references.
/// When a Registry is set, `add_lemma_files` will automatically resolve `@...`
/// references by fetching source text from the Registry, parsing it, and including
/// the resulting Lemma docs in the document set before planning.
pub struct Engine {
    execution_plans: HashMap<String, crate::planning::ExecutionPlan>,
    documents: HashMap<String, LemmaDoc>,
    sources: HashMap<String, String>,
    evaluator: Evaluator,
    limits: ResourceLimits,
    registry: Option<Arc<dyn Registry>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            execution_plans: HashMap::new(),
            documents: HashMap::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits: ResourceLimits::default(),
            registry: Self::default_registry(),
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the default registry based on enabled features.
    ///
    /// When the `registry` feature is enabled, the default registry is `LemmaBase`,
    /// which resolves `@...` references by fetching Lemma source from LemmaBase.com.
    ///
    /// When the `registry` feature is disabled, no registry is configured and
    /// `@...` references will fail during resolution.
    fn default_registry() -> Option<Arc<dyn Registry>> {
        #[cfg(feature = "registry")]
        {
            Some(Arc::new(crate::registry::LemmaBase::new()))
        }
        #[cfg(not(feature = "registry"))]
        {
            None
        }
    }

    /// Create an engine with custom resource limits.
    ///
    /// Uses the default registry (LemmaBase when the `registry` feature is enabled).
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            execution_plans: HashMap::new(),
            documents: HashMap::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits,
            registry: Self::default_registry(),
        }
    }

    /// Configure a Registry for resolving external `@...` references.
    ///
    /// When set, `add_lemma_files` will resolve `@...` references automatically
    /// by fetching source text from the Registry before planning.
    pub fn with_registry(mut self, registry: Arc<dyn Registry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Add Lemma source files and (when a registry is configured) resolve any `@...` references.
    ///
    /// - Resolves registry references **once** for all documents
    /// - Validates and resolves types **once** across all documents
    /// - Collects **all** errors across all files (parse, registry, planning) instead of aborting on the first
    ///
    /// `files` maps source identifiers (e.g. file paths) to source code.
    /// For a single file, pass a one-entry `HashMap`.
    pub async fn add_lemma_files(
        &mut self,
        files: HashMap<String, String>,
    ) -> Result<(), Vec<LemmaError>> {
        let mut errors: Vec<LemmaError> = Vec::new();
        let mut all_new_docs: Vec<LemmaDoc> = Vec::new();

        // 1. Parse all files, collect parse errors and detect duplicate document names.
        //    Duplicates are checked against both existing documents (from prior calls)
        //    and documents parsed earlier in this same call.
        for (source_id, code) in &files {
            match parse(code, source_id, &self.limits) {
                Ok(new_docs) => {
                    let source_text: Arc<str> = Arc::from(code.as_str());
                    for doc in new_docs {
                        let doc_id = doc.full_id();
                        let attribute = doc.attribute.clone().unwrap_or_else(|| doc_id.clone());

                        if let Some(existing) = self.documents.get(&doc_id) {
                            let earlier_attr =
                                existing.attribute.as_deref().unwrap_or(&existing.name);
                            errors.push(LemmaError::semantic(
                                format!(
                                    "Duplicate document name '{}' (previously declared in '{}')",
                                    doc_id, earlier_attr
                                ),
                                Some(crate::Source::new(
                                    &attribute,
                                    crate::parsing::ast::Span {
                                        start: 0,
                                        end: 0,
                                        line: doc.start_line,
                                        col: 0,
                                    },
                                    &doc_id,
                                    source_text.clone(),
                                )),
                                None::<String>,
                            ));
                        } else {
                            self.sources.insert(attribute, code.clone());
                            self.documents.insert(doc_id, doc.clone());
                        }

                        all_new_docs.push(doc);
                    }
                }
                Err(e) => errors.push(e),
            }
        }

        // 2. Resolve registry references once for all documents
        if let Some(registry) = &self.registry {
            let docs_to_resolve: Vec<LemmaDoc> = self.documents.values().cloned().collect();
            match crate::registry::resolve_registry_references(
                docs_to_resolve,
                &mut self.sources,
                registry.as_ref(),
                &self.limits,
            )
            .await
            {
                Ok(resolved_docs) => {
                    self.documents.clear();
                    for doc in resolved_docs {
                        self.documents.insert(doc.full_id(), doc);
                    }
                }
                Err(e) => match e {
                    LemmaError::MultipleErrors(inner) => errors.extend(inner),
                    other => errors.push(other),
                },
            }
        }

        // 3. Plan all new documents at once (validates and resolves types once)
        let docs_to_plan: Vec<&LemmaDoc> = all_new_docs.iter().collect();
        let all_docs: Vec<LemmaDoc> = self.documents.values().cloned().collect();
        let (plans, plan_errors) =
            crate::planning::plan(&docs_to_plan, &all_docs, self.sources.clone());
        self.execution_plans.extend(plans);
        errors.extend(plan_errors);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn remove_document(&mut self, doc_name: &str) {
        self.execution_plans.remove(doc_name);
        self.documents.remove(doc_name);
    }

    pub fn list_documents(&self) -> Vec<String> {
        self.documents.keys().cloned().collect()
    }

    pub fn get_document(&self, doc_name: &str) -> Option<&LemmaDoc> {
        self.documents.get(doc_name)
    }

    /// Get the execution plan for a document.
    ///
    /// The execution plan contains the resolved fact schema, default values,
    /// and topologically sorted rules ready for evaluation.
    pub fn get_execution_plan(&self, doc_name: &str) -> Option<&crate::planning::ExecutionPlan> {
        self.execution_plans.get(doc_name)
    }

    pub fn get_document_rules(&self, doc_name: &str) -> Vec<&crate::LemmaRule> {
        if let Some(doc) = self.documents.get(doc_name) {
            doc.rules.iter().collect()
        } else {
            Vec::new()
        }
    }

    /// Evaluate rules in a document with JSON values for facts.
    ///
    /// This is a convenience method that accepts JSON directly and converts it
    /// to typed values using the document's fact type declarations.
    ///
    /// If `rule_names` is empty, evaluates all rules.
    /// Otherwise, only returns results for the specified rules (dependencies still computed).
    ///
    /// Values are provided as JSON bytes (e.g., `b"{\"quantity\": 5, \"is_member\": true}"`).
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn evaluate_json(
        &self,
        doc_name: &str,
        rule_names: Vec<String>,
        json: &[u8],
    ) -> LemmaResult<Response> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                None,
                None::<String>,
            )
        })?;

        let values = crate::serialization::from_json(json)?;
        let plan = base_plan.clone().with_fact_values(values, &self.limits)?;

        self.evaluate_plan(plan, rule_names)
    }

    /// Evaluate rules in a document with string values for facts.
    ///
    /// This is the user-friendly API that accepts raw string values and parses them
    /// to the appropriate types based on the document's fact type declarations.
    /// Use this for CLI, HTTP APIs, and other user-facing interfaces.
    ///
    /// If `rule_names` is empty, evaluates all rules.
    /// Otherwise, only returns results for the specified rules (dependencies still computed).
    ///
    /// Fact values are provided as name -> value string pairs (e.g., "type" -> "latte").
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn evaluate(
        &self,
        doc_name: &str,
        rule_names: Vec<String>,
        fact_values: HashMap<String, String>,
    ) -> LemmaResult<Response> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                None,
                None::<String>,
            )
        })?;

        let plan = base_plan
            .clone()
            .with_fact_values(fact_values, &self.limits)?;

        self.evaluate_plan(plan, rule_names)
    }

    /// Invert a rule to find input domains that produce a desired outcome with JSON values.
    ///
    /// Values are provided as JSON bytes (e.g., `b"{\"quantity\": 5, \"is_member\": true}"`).
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn invert_json(
        &self,
        doc_name: &str,
        rule_name: &str,
        target: crate::inversion::Target,
        json: &[u8],
    ) -> LemmaResult<crate::InversionResponse> {
        let values = crate::serialization::from_json(json)?;
        self.invert(doc_name, rule_name, target, values)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// Values are provided as name -> value string pairs (e.g., "quantity" -> "5").
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn invert(
        &self,
        doc_name: &str,
        rule_name: &str,
        target: crate::inversion::Target,
        values: HashMap<String, String>,
    ) -> LemmaResult<crate::InversionResponse> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                None,
                None::<String>,
            )
        })?;

        let plan = base_plan.clone().with_fact_values(values, &self.limits)?;
        let provided_facts: std::collections::HashSet<_> = plan
            .facts
            .iter()
            .filter(|(_, d)| d.value().is_some())
            .map(|(p, _)| p.clone())
            .collect();

        crate::inversion::invert(rule_name, target, &plan, &provided_facts)
    }

    fn evaluate_plan(
        &self,
        plan: crate::planning::ExecutionPlan,
        rule_names: Vec<String>,
    ) -> LemmaResult<Response> {
        let mut response = self.evaluator.evaluate(&plan);

        if !rule_names.is_empty() {
            response.filter_rules(&rule_names);
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn add_lemma_code_blocking(engine: &mut Engine, code: &str, source: &str) -> LemmaResult<()> {
        let files: HashMap<String, String> =
            std::iter::once((source.to_string(), code.to_string())).collect();
        tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(engine.add_lemma_files(files))
            .map_err(|errs| match errs.len() {
                0 => unreachable!("add_lemma_files returned Err with empty error list"),
                1 => errs.into_iter().next().unwrap(),
                _ => LemmaError::MultipleErrors(errs),
            })
    }

    #[test]
    fn test_evaluate_document_all_rules() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc test
        fact x = 10
        fact y = 5
        rule sum = x + y
        rule product = x * y
    "#,
            "test.lemma",
        )
        .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(response.results.len(), 2);

        let sum_result = response
            .results
            .values()
            .find(|r| r.rule.name == "sum")
            .unwrap();
        assert_eq!(
            sum_result.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("15").unwrap()
            )))
        );

        let product_result = response
            .results
            .values()
            .find(|r| r.rule.name == "product")
            .unwrap();
        assert_eq!(
            product_result.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("50").unwrap()
            )))
        );
    }

    #[test]
    fn test_evaluate_empty_facts() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc test
        fact price = 100
        rule total = price * 2
    "#,
            "test.lemma",
        )
        .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("200").unwrap()
            )))
        );
    }

    #[test]
    fn test_evaluate_boolean_rule() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc test
        fact age = 25
        rule is_adult = age >= 18
    "#,
            "test.lemma",
        )
        .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::from_bool(true)))
        );
    }

    #[test]
    fn test_evaluate_with_unless_clause() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc test
        fact quantity = 15
        rule discount = 0
          unless quantity >= 10 then 10
    "#,
            "test.lemma",
        )
        .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("10").unwrap()
            )))
        );
    }

    #[test]
    fn test_document_not_found() {
        let engine = Engine::new();
        let result = engine.evaluate("nonexistent", vec![], HashMap::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_multiple_documents() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc doc1
        fact x = 10
        rule result = x * 2
    "#,
            "doc1.lemma",
        )
        .unwrap();

        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc doc2
        fact y = 5
        rule result = y * 3
    "#,
            "doc2.lemma",
        )
        .unwrap();

        let response1 = engine.evaluate("doc1", vec![], HashMap::new()).unwrap();
        assert_eq!(
            response1.results[0].result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("20").unwrap()
            )))
        );

        let response2 = engine.evaluate("doc2", vec![], HashMap::new()).unwrap();
        assert_eq!(
            response2.results[0].result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("15").unwrap()
            )))
        );
    }

    #[test]
    fn test_runtime_error_mapping() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc test
        fact numerator = 10
        fact denominator = 0
        rule division = numerator / denominator
    "#,
            "test.lemma",
        )
        .unwrap();

        let result = engine.evaluate("test", vec![], HashMap::new());
        // Division by zero returns a Veto (not an error)
        assert!(result.is_ok(), "Evaluation should succeed");
        let response = result.unwrap();
        let division_result = response
            .results
            .values()
            .find(|r| r.rule.name == "division");
        assert!(
            division_result.is_some(),
            "Should have division rule result"
        );
        match &division_result.unwrap().result {
            crate::OperationResult::Veto(message) => {
                assert!(
                    message
                        .as_ref()
                        .map(|m| m.contains("Division by zero"))
                        .unwrap_or(false),
                    "Veto message should mention division by zero: {:?}",
                    message
                );
            }
            other => panic!("Expected Veto for division by zero, got {:?}", other),
        }
    }

    #[test]
    fn test_rules_sorted_by_source_order() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc test
        fact a = 1
        fact b = 2
        rule z = a + b
        rule y = a * b
        rule x = a - b
    "#,
            "test.lemma",
        )
        .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(response.results.len(), 3);

        // Verify source positions increase (z < y < x)
        let z_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "z")
            .unwrap()
            .rule
            .source_location
            .span
            .start;
        let y_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "y")
            .unwrap()
            .rule
            .source_location
            .span
            .start;
        let x_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "x")
            .unwrap()
            .rule
            .source_location
            .span
            .start;

        assert!(z_pos < y_pos);
        assert!(y_pos < x_pos);
    }

    #[test]
    fn test_rule_filtering_evaluates_dependencies() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        doc test
        fact base = 100
        rule subtotal = base * 2
        rule tax = subtotal? * 10%
        rule total = subtotal? + tax?
    "#,
            "test.lemma",
        )
        .unwrap();

        // Request only 'total', but it depends on 'subtotal' and 'tax'
        let response = engine
            .evaluate("test", vec!["total".to_string()], HashMap::new())
            .unwrap();

        // Only 'total' should be in results
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results.keys().next().unwrap(), "total");

        // But the value should be correct (dependencies were computed)
        let total = response.results.values().next().unwrap();
        assert_eq!(
            total.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("220").unwrap()
            )))
        );
    }

    // -------------------------------------------------------------------
    // Registry integration tests
    // -------------------------------------------------------------------

    use crate::registry::{RegistryBundle, RegistryError};

    /// Minimal test registry for engine-level tests.
    struct EngineTestRegistry {
        bundles: std::collections::HashMap<String, RegistryBundle>,
    }

    impl EngineTestRegistry {
        fn new() -> Self {
            Self {
                bundles: std::collections::HashMap::new(),
            }
        }

        fn add(&mut self, identifier: &str, source: &str) {
            self.bundles.insert(
                identifier.to_string(),
                RegistryBundle {
                    lemma_source: source.to_string(),
                    attribute: format!("@{}", identifier),
                },
            );
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    impl Registry for EngineTestRegistry {
        async fn resolve_doc(
            &self,
            name: &str,
            _version: Option<&str>,
        ) -> Result<RegistryBundle, RegistryError> {
            self.bundles.get(name).cloned().ok_or(RegistryError {
                message: format!("not found: {}", name),
                kind: crate::registry::RegistryErrorKind::NotFound,
            })
        }

        async fn resolve_type(
            &self,
            name: &str,
            _version: Option<&str>,
        ) -> Result<RegistryBundle, RegistryError> {
            self.bundles.get(name).cloned().ok_or(RegistryError {
                message: format!("not found: {}", name),
                kind: crate::registry::RegistryErrorKind::NotFound,
            })
        }

        fn url_for_id(&self, name: &str, _version: Option<&str>) -> Option<String> {
            Some(format!("https://test/{}", name))
        }
    }

    /// Build an engine with no registry (regardless of feature flags).
    fn engine_without_registry() -> Engine {
        Engine {
            execution_plans: HashMap::new(),
            documents: HashMap::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits: ResourceLimits::default(),
            registry: None,
        }
    }

    #[test]
    fn add_lemma_files_with_registry_resolves_and_evaluates_external_doc() {
        let mut registry = EngineTestRegistry::new();
        registry.add(
            "org/project/helper",
            "doc org/project/helper\nfact quantity = 42",
        );

        let mut engine = engine_without_registry().with_registry(Arc::new(registry));

        add_lemma_code_blocking(
            &mut engine,
            r#"doc main_doc
fact external = doc @org/project/helper
rule value = external.quantity"#,
            "main.lemma",
        )
        .expect("add_lemma_files should succeed with registry resolving the external doc");

        let response = engine
            .evaluate("main_doc", vec![], HashMap::new())
            .expect("evaluate should succeed");

        let value_result = response
            .results
            .get("value")
            .expect("rule 'value' should exist");
        assert_eq!(
            value_result.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("42").unwrap()
            )))
        );
    }

    #[test]
    fn add_lemma_files_without_registry_and_no_external_refs_works() {
        let mut engine = engine_without_registry();

        add_lemma_code_blocking(
            &mut engine,
            r#"doc local_only
fact price = 100
rule doubled = price * 2"#,
            "local.lemma",
        )
        .expect(
            "add_lemma_files should succeed without registry when there are no @... references",
        );

        let response = engine
            .evaluate("local_only", vec![], HashMap::new())
            .expect("evaluate should succeed");

        assert!(response.results.contains_key("doubled"));
    }

    #[test]
    fn add_lemma_files_without_registry_and_external_ref_fails() {
        let mut engine = engine_without_registry();

        let result = add_lemma_code_blocking(
            &mut engine,
            r#"doc main_doc
fact external = doc @org/project/missing
rule value = external.quantity"#,
            "main.lemma",
        );

        assert!(
            result.is_err(),
            "Should fail when @... reference exists but no registry is configured"
        );
    }

    #[test]
    fn add_lemma_files_with_registry_error_propagates_as_registry_error() {
        // Empty registry — every lookup returns "not found"
        let registry = EngineTestRegistry::new();

        let mut engine = engine_without_registry().with_registry(Arc::new(registry));

        let result = add_lemma_code_blocking(
            &mut engine,
            r#"doc main_doc
fact external = doc @org/project/missing
rule value = external.quantity"#,
            "main.lemma",
        );

        assert!(
            result.is_err(),
            "Should fail when registry cannot resolve the @... reference"
        );
        let error = result.unwrap_err();
        let registry_err = match &error {
            LemmaError::Registry { .. } => &error,
            LemmaError::MultipleErrors(inner) => inner
                .iter()
                .find(|e| matches!(e, LemmaError::Registry { .. }))
                .expect("MultipleErrors should contain at least one Registry error"),
            other => panic!(
                "Expected LemmaError::Registry or MultipleErrors, got: {}",
                other
            ),
        };
        match registry_err {
            LemmaError::Registry {
                identifier, kind, ..
            } => {
                assert_eq!(identifier, "org/project/missing");
                assert_eq!(*kind, crate::registry::RegistryErrorKind::NotFound);
            }
            _ => unreachable!(),
        }
        // The Display output should also mention the identifier and kind.
        let error_message = error.to_string();
        assert!(
            error_message.contains("org/project/missing"),
            "Error should mention the unresolved identifier: {}",
            error_message
        );
        assert!(
            error_message.contains("not found"),
            "Error should mention the error kind: {}",
            error_message
        );
    }

    #[test]
    fn with_registry_replaces_default_registry() {
        let mut registry = EngineTestRegistry::new();
        registry.add("custom/doc", "doc custom/doc\nfact x = 99");

        let mut engine = Engine::new().with_registry(Arc::new(registry));

        add_lemma_code_blocking(
            &mut engine,
            r#"doc main_doc
fact ext = doc @custom/doc
rule val = ext.x"#,
            "main.lemma",
        )
        .expect("with_registry should replace the default registry");

        let response = engine
            .evaluate("main_doc", vec![], HashMap::new())
            .expect("evaluate should succeed");

        let val_result = response
            .results
            .get("val")
            .expect("rule 'val' should exist");
        assert_eq!(
            val_result.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("99").unwrap()
            )))
        );
    }

    #[test]
    fn add_lemma_files_returns_all_errors_not_just_first() {
        // When a document has multiple independent errors (type import from
        // non-existing doc AND doc reference to non-existing doc), the Engine
        // should surface all of them, not just the first one.
        let mut engine = engine_without_registry();

        let result = add_lemma_code_blocking(
            &mut engine,
            r#"doc demo
type money from nonexistent_type_source
fact helper = doc nonexistent_doc
fact price = 10
rule total = helper.value + price"#,
            "test.lemma",
        );

        assert!(result.is_err(), "Should fail with multiple errors");
        let error = result.unwrap_err();
        let error_message = error.to_string();

        // The type resolution error should be present
        assert!(
            error_message.contains("money"),
            "Should mention type error about 'money'. Got:\n{}",
            error_message
        );

        // The doc reference error should ALSO be present (not swallowed)
        assert!(
            error_message.contains("nonexistent_doc"),
            "Should mention doc reference error about 'nonexistent_doc'. Got:\n{}",
            error_message
        );

        // The error should be a MultipleErrors variant since there are 2+ errors
        assert!(
            matches!(error, LemmaError::MultipleErrors(_)),
            "Expected MultipleErrors, got: {}",
            error_message
        );
    }

    // ── Default value type validation ────────────────────────────────
    // Planning must reject default values that don't match the type.
    // These tests cover both primitives and named types (which the parser
    // can't validate because it doesn't resolve type names).

    #[test]
    fn planning_rejects_invalid_number_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "doc t\nfact x = [number -> default \"10 $$\"]\nrule r = x",
            "t.lemma",
        );
        assert!(
            result.is_err(),
            "must reject non-numeric default on number type"
        );
    }

    #[test]
    fn planning_rejects_text_literal_as_number_default() {
        // The parser produces CommandArg::Text("10") for `default "10"`.
        // Planning now checks the CommandArg variant: a Text literal is
        // rejected where a Number literal is required, even though the
        // string content "10" could be parsed as a valid Decimal.
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "doc t\nfact x = [number -> default \"10\"]\nrule r = x",
            "t.lemma",
        );
        assert!(
            result.is_err(),
            "must reject text literal \"10\" as default for number type"
        );
    }

    #[test]
    fn planning_rejects_invalid_boolean_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "doc t\nfact x = [boolean -> default \"maybe\"]\nrule r = x",
            "t.lemma",
        );
        assert!(
            result.is_err(),
            "must reject non-boolean default on boolean type"
        );
    }

    #[test]
    fn planning_rejects_invalid_named_type_default() {
        // Named type: the parser can't validate this, only planning can.
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "doc t\ntype custom = number -> minimum 0\nfact x = [custom -> default \"abc\"]\nrule r = x",
            "t.lemma",
        );
        assert!(
            result.is_err(),
            "must reject non-numeric default on named number type"
        );
    }

    #[test]
    fn planning_accepts_valid_number_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "doc t\nfact x = [number -> default 10]\nrule r = x",
            "t.lemma",
        );
        assert!(result.is_ok(), "must accept valid number default");
    }

    #[test]
    fn planning_accepts_valid_boolean_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "doc t\nfact x = [boolean -> default true]\nrule r = x",
            "t.lemma",
        );
        assert!(result.is_ok(), "must accept valid boolean default");
    }

    #[test]
    fn planning_accepts_valid_text_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "doc t\nfact x = [text -> default \"hello\"]\nrule r = x",
            "t.lemma",
        );
        assert!(result.is_ok(), "must accept valid text default");
    }
}
