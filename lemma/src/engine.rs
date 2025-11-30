use crate::evaluation::Evaluator;
use crate::planning::plan;
use crate::{parse, LemmaDoc, LemmaError, LemmaResult, ResourceLimits, Response};
use std::collections::{HashMap, HashSet};

/// Engine for evaluating Lemma rules
///
/// Pure Rust implementation that evaluates Lemma docs directly from the AST.
/// Uses pre-built execution plans that are self-contained and ready for evaluation.
pub struct Engine {
    execution_plans: HashMap<String, crate::planning::ExecutionPlan>,
    documents: HashMap<String, LemmaDoc>,
    sources: HashMap<String, String>,
    evaluator: Evaluator,
    limits: ResourceLimits,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            execution_plans: HashMap::new(),
            documents: HashMap::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits: ResourceLimits::default(),
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an engine with custom resource limits
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            execution_plans: HashMap::new(),
            documents: HashMap::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits,
        }
    }

    pub fn add_lemma_code(&mut self, lemma_code: &str, source: &str) -> LemmaResult<()> {
        let new_docs = parse(lemma_code, Some(source.to_owned()), &self.limits)?;

        for doc in &new_docs {
            let source_id = doc.source.clone().unwrap_or_else(|| doc.name.clone());
            self.sources.insert(source_id, lemma_code.to_owned());
            self.documents.insert(doc.name.clone(), doc.clone());
        }

        // Collect all documents (existing + new)
        let all_docs: Vec<LemmaDoc> = self.documents.values().cloned().collect();

        // Build execution plans for all new documents
        for doc in &new_docs {
            let execution_plan = plan(doc, &all_docs, self.sources.clone()).map_err(|errs| {
                if errs.is_empty() {
                    LemmaError::Engine(format!(
                        "Failed to build execution plan for document: {}",
                        doc.name
                    ))
                } else {
                    errs.into_iter().next().unwrap_or_else(|| {
                        LemmaError::Engine(format!(
                            "Failed to build execution plan for document: {}",
                            doc.name
                        ))
                    })
                }
            })?;

            self.execution_plans
                .insert(doc.name.clone(), execution_plan);
        }

        Ok(())
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

    pub fn get_document_facts(&self, doc_name: &str) -> Vec<&crate::LemmaFact> {
        if let Some(doc) = self.documents.get(doc_name) {
            doc.facts.iter().collect()
        } else {
            Vec::new()
        }
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
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        let values = crate::serialization::from_json(json, base_plan)?;

        self.evaluate_strict(doc_name, rule_names, values)
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
    /// Values are provided as name -> value string pairs (e.g., "type" -> "latte").
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn evaluate(
        &self,
        doc_name: &str,
        rule_names: Vec<String>,
        values: HashMap<String, String>,
    ) -> LemmaResult<Response> {
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        let plan = base_plan.clone().with_values(values, &self.limits)?;

        self.evaluate_plan(plan, rule_names)
    }

    /// Evaluate rules in a document with typed values for facts.
    ///
    /// This is the strict API that accepts pre-typed LiteralValue values.
    /// Use this for programmatic APIs, protobuf, msgpack, FFI, and other
    /// strongly-typed interfaces where values are already parsed.
    ///
    /// If `rule_names` is empty, evaluates all rules.
    /// Otherwise, only returns results for the specified rules (dependencies still computed).
    ///
    /// Values are provided as name -> LiteralValue pairs (e.g., "age" -> Number(25)).
    pub fn evaluate_strict(
        &self,
        doc_name: &str,
        rule_names: Vec<String>,
        values: HashMap<String, crate::LiteralValue>,
    ) -> LemmaResult<Response> {
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        let plan = base_plan.clone().with_typed_values(values, &self.limits)?;

        self.evaluate_plan(plan, rule_names)
    }

    /// Invert a rule to find input domains that produce a desired outcome with JSON values.
    ///
    /// This is a convenience method that accepts JSON directly and converts it
    /// to typed values using the document's fact type declarations.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Concrete domain constraints for each free variable
    /// - `shape`: The symbolic representation of the solution space
    /// - `free_variables`: Facts that are not fully determined
    /// - `is_fully_constrained`: Whether all facts have concrete values
    ///
    /// Values are provided as JSON bytes (e.g., `b"{\"quantity\": 5, \"is_member\": true}"`).
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn invert_json(
        &self,
        doc_name: &str,
        rule_name: &str,
        target: crate::Target,
        json: &[u8],
    ) -> LemmaResult<crate::InversionResponse> {
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        let values = crate::serialization::from_json(json, base_plan)?;

        self.invert_strict(doc_name, rule_name, target, values)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// This is the user-friendly API that accepts raw string values and parses them
    /// to the appropriate types based on the document's fact type declarations.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Concrete domain constraints for each free variable
    /// - `shape`: The symbolic representation of the solution space
    /// - `free_variables`: Facts that are not fully determined
    /// - `is_fully_constrained`: Whether all facts have concrete values
    ///
    /// Values are provided as name -> value string pairs (e.g., "quantity" -> "5").
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn invert(
        &self,
        doc_name: &str,
        rule_name: &str,
        target: crate::Target,
        values: HashMap<String, String>,
    ) -> LemmaResult<crate::InversionResponse> {
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        // Resolve value keys to FactPaths for inversion
        let provided_facts: HashSet<crate::FactPath> = values
            .keys()
            .filter_map(|k| base_plan.get_fact_by_path_str(k).map(|(fp, _)| fp.clone()))
            .collect();

        let plan = base_plan.clone().with_values(values, &self.limits)?;

        self.invert_plan(plan, rule_name, target, provided_facts)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// This is the strict API that accepts pre-typed LiteralValue values.
    /// Use this for programmatic APIs, protobuf, msgpack, FFI, and other
    /// strongly-typed interfaces where values are already parsed.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Concrete domain constraints for each free variable
    /// - `shape`: The symbolic representation of the solution space
    /// - `free_variables`: Facts that are not fully determined
    /// - `is_fully_constrained`: Whether all facts have concrete values
    ///
    /// Values are provided as name -> LiteralValue pairs (e.g., "quantity" -> Number(5)).
    pub fn invert_strict(
        &self,
        doc_name: &str,
        rule_name: &str,
        target: crate::Target,
        values: HashMap<String, crate::LiteralValue>,
    ) -> LemmaResult<crate::InversionResponse> {
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        // Resolve value keys to FactPaths for inversion
        let provided_facts: HashSet<crate::FactPath> = values
            .keys()
            .filter_map(|k| base_plan.get_fact_by_path_str(k).map(|(fp, _)| fp.clone()))
            .collect();

        let plan = base_plan.clone().with_typed_values(values, &self.limits)?;

        self.invert_plan(plan, rule_name, target, provided_facts)
    }

    fn evaluate_plan(
        &self,
        plan: crate::planning::ExecutionPlan,
        rule_names: Vec<String>,
    ) -> LemmaResult<Response> {
        let mut response = self.evaluator.evaluate(&plan)?;

        if !rule_names.is_empty() {
            response.filter_rules(&rule_names);
        }

        Ok(response)
    }

    fn invert_plan(
        &self,
        plan: crate::planning::ExecutionPlan,
        rule_name: &str,
        target: crate::Target,
        provided_facts: HashSet<crate::FactPath>,
    ) -> LemmaResult<crate::InversionResponse> {
        let shape = crate::inversion::invert(rule_name, target, &plan, &provided_facts)?;
        let solutions = crate::inversion::shape_to_domains(&shape)?;
        Ok(crate::InversionResponse::new(shape, solutions))
    }
}
