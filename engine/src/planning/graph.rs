use crate::parsing::ast::{self as ast, LemmaDoc, LemmaFact, LemmaRule, TypeDef, Value};
use crate::parsing::source::Source;
use crate::planning::semantics::{
    conversion_target_to_semantic, parse_number_unit, primitive_boolean, primitive_date,
    primitive_duration, primitive_number, primitive_ratio, primitive_scale, primitive_text,
    primitive_time, value_to_semantic, ArithmeticComputation, Expression, ExpressionKind, FactData,
    FactPath, LemmaType, LiteralValue, PathSegment, RulePath, SemanticConversionTarget,
    TypeExtends, TypeSpecification, ValueKind,
};
use crate::planning::types::{ResolvedDocumentTypes, TypeRegistry};
use crate::planning::validation::{
    validate_document_interfaces, validate_type_specifications, RuleEntryForBindingCheck,
};
use crate::LemmaError;
use ast::FactValue as ParsedFactValue;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Fact bindings map: maps a target fact name path to the binding's value and source.
///
/// The key is the full path of **fact names** from the root document to the target fact.
/// Doc names are intentionally excluded from the key because doc ref bindings may change
/// which document a segment points to — matching by fact names only ensures bindings
/// are applied correctly regardless of doc ref bindings.
///
/// Example: `fact employee.salary = 7500` in the root doc produces key `["employee", "salary"]`.
type FactBindings = HashMap<Vec<String>, (ParsedFactValue, Source)>;

#[derive(Debug)]
pub(crate) struct Graph {
    facts: IndexMap<FactPath, FactData>,
    rules: IndexMap<RulePath, RuleNode>,
    sources: HashMap<String, String>,
    execution_order: Vec<RulePath>,
    /// Resolved types per document (from TypeRegistry during build). Used for unit lookups
    /// (e.g. result type of "number in usd") without re-resolving.
    pub(crate) resolved_types: HashMap<String, ResolvedDocumentTypes>,
}

impl Graph {
    pub(crate) fn facts(&self) -> &IndexMap<FactPath, FactData> {
        &self.facts
    }

    pub(crate) fn rules(&self) -> &IndexMap<RulePath, RuleNode> {
        &self.rules
    }

    pub(crate) fn rules_mut(&mut self) -> &mut IndexMap<RulePath, RuleNode> {
        &mut self.rules
    }

    pub(crate) fn sources(&self) -> &HashMap<String, String> {
        &self.sources
    }

    pub(crate) fn execution_order(&self) -> &[RulePath] {
        &self.execution_order
    }

    /// Build the fact map: one entry per fact (Value or DocumentRef), with defaults and coercion applied.
    pub(crate) fn build_facts(&self) -> HashMap<FactPath, FactData> {
        let mut schema: HashMap<FactPath, LemmaType> = HashMap::new();
        let mut values: HashMap<FactPath, LiteralValue> = HashMap::new();
        let mut doc_refs: HashMap<FactPath, String> = HashMap::new();
        let mut sources: HashMap<FactPath, Source> = HashMap::new();

        for (path, rfv) in self.facts.iter() {
            sources.insert(path.clone(), rfv.source().clone());
            match rfv {
                FactData::Value { value, .. } => {
                    values.insert(path.clone(), value.clone());
                    schema.insert(path.clone(), value.lemma_type.clone());
                }
                FactData::TypeDeclaration { resolved_type, .. } => {
                    schema.insert(path.clone(), resolved_type.clone());
                }
                FactData::DocumentRef { doc_name, .. } => {
                    doc_refs.insert(path.clone(), doc_name.clone());
                }
            }
        }

        for (path, schema_type) in &schema {
            if values.contains_key(path) {
                continue;
            }
            if let Some(default_value) = schema_type.create_default_value() {
                values.insert(path.clone(), default_value);
            }
        }

        for (path, value) in values.iter_mut() {
            let Some(schema_type) = schema.get(path).cloned() else {
                continue;
            };
            match Self::coerce_literal_to_schema_type(value, &schema_type) {
                Ok(coerced) => *value = coerced,
                Err(msg) => unreachable!("Fact {} incompatible: {}", path, msg),
            }
        }

        let mut facts = HashMap::new();
        for (path, source) in sources {
            if let Some(doc_name) = doc_refs.get(&path) {
                facts.insert(
                    path,
                    FactData::DocumentRef {
                        doc_name: doc_name.clone(),
                        source,
                    },
                );
            } else if let Some(value) = values.remove(&path) {
                facts.insert(path, FactData::Value { value, source });
            } else {
                let resolved_type = schema
                    .get(&path)
                    .cloned()
                    .expect("non-doc-ref fact has schema (value or type-only)");
                facts.insert(
                    path,
                    FactData::TypeDeclaration {
                        resolved_type,
                        source,
                    },
                );
            }
        }
        facts
    }

    fn coerce_literal_to_schema_type(
        lit: &LiteralValue,
        schema_type: &LemmaType,
    ) -> Result<LiteralValue, String> {
        if lit.lemma_type.specifications == schema_type.specifications {
            let mut out = lit.clone();
            out.lemma_type = schema_type.clone();
            return Ok(out);
        }
        match (&schema_type.specifications, &lit.value) {
            (TypeSpecification::Number { .. }, ValueKind::Number(_))
            | (TypeSpecification::Text { .. }, ValueKind::Text(_))
            | (TypeSpecification::Boolean { .. }, ValueKind::Boolean(_))
            | (TypeSpecification::Date { .. }, ValueKind::Date(_))
            | (TypeSpecification::Time { .. }, ValueKind::Time(_))
            | (TypeSpecification::Duration { .. }, ValueKind::Duration(_, _))
            | (TypeSpecification::Ratio { .. }, ValueKind::Ratio(_, _))
            | (TypeSpecification::Scale { .. }, ValueKind::Scale(_, _)) => {
                let mut out = lit.clone();
                out.lemma_type = schema_type.clone();
                Ok(out)
            }
            (TypeSpecification::Ratio { .. }, ValueKind::Number(n)) => {
                Ok(LiteralValue::ratio_with_type(*n, None, schema_type.clone()))
            }
            _ => Err(format!(
                "value {} cannot be used as type {}",
                lit,
                schema_type.name()
            )),
        }
    }

    /// Resolve a primitive type by name (helper function)
    fn resolve_primitive_type(name: &str) -> Option<TypeSpecification> {
        match name {
            "boolean" => Some(TypeSpecification::boolean()),
            "scale" => Some(TypeSpecification::scale()),
            "number" => Some(TypeSpecification::number()),
            "ratio" => Some(TypeSpecification::ratio()),
            "text" => Some(TypeSpecification::text()),
            "date" => Some(TypeSpecification::date()),
            "time" => Some(TypeSpecification::time()),
            "duration" => Some(TypeSpecification::duration()),
            "percent" => Some(TypeSpecification::ratio()),
            _ => None,
        }
    }

    fn topological_sort(&self) -> Result<Vec<RulePath>, Vec<LemmaError>> {
        let mut in_degree: HashMap<RulePath, usize> = HashMap::new();
        let mut dependents: HashMap<RulePath, Vec<RulePath>> = HashMap::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        for rule_path in self.rules.keys() {
            in_degree.insert(rule_path.clone(), 0);
            dependents.insert(rule_path.clone(), Vec::new());
        }

        for (rule_path, rule_node) in &self.rules {
            for dependency in &rule_node.depends_on_rules {
                if self.rules.contains_key(dependency) {
                    if let Some(degree) = in_degree.get_mut(rule_path) {
                        *degree += 1;
                    }
                    if let Some(deps) = dependents.get_mut(dependency) {
                        deps.push(rule_path.clone());
                    }
                }
            }
        }

        for (rule_path, degree) in &in_degree {
            if *degree == 0 {
                queue.push_back(rule_path.clone());
            }
        }

        while let Some(rule_path) = queue.pop_front() {
            result.push(rule_path.clone());

            if let Some(dependent_rules) = dependents.get(&rule_path) {
                for dependent in dependent_rules {
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent.clone());
                        }
                    }
                }
            }
        }

        if result.len() != self.rules.len() {
            let missing: Vec<RulePath> = self
                .rules
                .keys()
                .filter(|rule| !result.contains(rule))
                .cloned()
                .collect();
            let cycle: Vec<Source> = missing
                .iter()
                .filter_map(|rule| self.rules.get(rule).map(|n| n.source.clone()))
                .collect();

            let Some(first_source) = cycle.first() else {
                unreachable!(
                    "BUG: circular dependency detected but no sources could be collected ({} missing rules)",
                    missing.len()
                );
            };

            return Err(vec![LemmaError::circular_dependency(
                format!(
                    "Circular dependency detected. Rules involved: {}",
                    missing
                        .iter()
                        .map(|rule| rule.rule.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Some(first_source.clone()),
                cycle,
                None::<String>,
            )]);
        }

        Ok(result)
    }
}

#[derive(Debug)]
pub(crate) struct RuleNode {
    /// First branch has condition=None (default expression), subsequent branches are unless clauses.
    /// Resolved expressions (FactReference -> FactPath, RuleReference -> RulePath).
    pub branches: Vec<(Option<Expression>, Expression)>,
    pub source: Source,

    pub depends_on_rules: HashSet<RulePath>,

    /// Computed type of this rule's result (populated during validation)
    /// Every rule MUST have a type (Lemma is strictly typed)
    pub rule_type: LemmaType,
}

struct GraphBuilder<'a> {
    facts: IndexMap<FactPath, FactData>,
    rules: IndexMap<RulePath, RuleNode>,
    sources: HashMap<String, String>,
    all_docs: HashMap<String, &'a LemmaDoc>,
    resolved_types: HashMap<String, ResolvedDocumentTypes>,
    errors: Vec<LemmaError>,
}

/// Pre-built type state shared across multiple `Graph::build` calls.
///
/// Created once by [`Graph::prepare_types`] and reused for each document
/// being planned, avoiding redundant type registration and resolution.
#[derive(Clone)]
pub(crate) struct PreparedTypes {
    pub type_registry: TypeRegistry,
    pub resolved_types: HashMap<String, ResolvedDocumentTypes>,
}

impl Graph {
    /// Register all named types from all documents and resolve them.
    ///
    /// Returns the prepared type state plus any global type errors
    /// (unknown types, duplicate types, specification violations).
    ///
    /// Call this once and pass the result to [`Graph::build`] for each
    /// document being planned.
    pub(crate) fn prepare_types(
        all_docs: &[LemmaDoc],
        sources: &HashMap<String, String>,
    ) -> (PreparedTypes, Vec<LemmaError>) {
        let mut type_registry = TypeRegistry::new(sources.clone());
        let mut errors: Vec<LemmaError> = Vec::new();
        let mut resolved_types: HashMap<String, ResolvedDocumentTypes> = HashMap::new();

        // Register all named type definitions from every document.
        for doc in all_docs {
            for type_def in &doc.types {
                if let Err(e) = type_registry.register_type(&doc.name, type_def.clone()) {
                    errors.push(e);
                }
            }
        }

        // Pre-resolve named types for every document up-front.
        //
        // Graph construction and execution-plan building may need to resolve types "from" other
        // documents even if those documents are not reachable through document references.
        //
        // We only resolve *named* types here because inline type definitions are registered while
        // traversing facts during graph building and must be resolved afterwards per document.
        for doc in all_docs {
            match type_registry.resolve_named_types(&doc.name) {
                Ok(document_types) => {
                    // Validate type specifications for all resolved named types
                    for (type_name, lemma_type) in &document_types.named_types {
                        // Find the original TypeDef to get its real source location
                        let source = doc
                            .types
                            .iter()
                            .find(|td| match td {
                                ast::TypeDef::Regular { name, .. }
                                | ast::TypeDef::Import { name, .. } => name == type_name,
                                ast::TypeDef::Inline { .. } => false,
                            })
                            .map(|td| td.source_location().clone())
                            .unwrap_or_else(|| {
                                unreachable!(
                                    "BUG: resolved named type '{}' has no corresponding TypeDef in document '{}'",
                                    type_name, doc.name
                                )
                            });
                        let mut spec_errors = validate_type_specifications(
                            &lemma_type.specifications,
                            type_name,
                            &source,
                        );
                        errors.append(&mut spec_errors);
                    }
                    resolved_types.insert(doc.name.clone(), document_types);
                }
                Err(e) => errors.push(e),
            }
        }

        let prepared = PreparedTypes {
            type_registry,
            resolved_types,
        };
        (prepared, errors)
    }

    /// Build the dependency graph for a single document using pre-built types.
    ///
    /// The `prepared` types are cloned internally because `build_document`
    /// registers inline type definitions and re-resolves types per document.
    ///
    /// Only reports per-document errors (doc references, inline types, rule
    /// validation). Global type errors are returned separately by
    /// [`prepare_types`](Self::prepare_types).
    pub(crate) fn build(
        main_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
        sources: HashMap<String, String>,
        prepared: &PreparedTypes,
    ) -> Result<Graph, Vec<LemmaError>> {
        let mut type_registry = prepared.type_registry.clone();

        let mut builder = GraphBuilder {
            facts: IndexMap::new(),
            rules: IndexMap::new(),
            sources,
            all_docs: all_docs.iter().map(|doc| (doc.name.clone(), doc)).collect(),
            resolved_types: prepared.resolved_types.clone(),
            errors: Vec::new(),
        };

        // Do NOT return early here — continue with build_document even when
        // type resolution produced errors.  build_document handles missing
        // resolved types gracefully (push error & skip the affected fact)
        // and this lets us collect *all* errors (doc references, unit
        // lookups, cross-doc fact resolution, …) in a single pass rather
        // than forcing the user to fix type errors before any other problems
        // are reported.

        builder.build_document(main_doc, Vec::new(), HashMap::new(), &mut type_registry)?;

        let graph_errors = builder.errors;

        let mut graph = Graph {
            facts: builder.facts,
            rules: builder.rules,
            sources: builder.sources,
            execution_order: Vec::new(),
            resolved_types: builder.resolved_types,
        };

        // Always run validation, even when graph building produced errors.
        // This lets us collect type errors alongside structural errors so the
        // user sees *all* problems in a single pass (e.g. missing `?` on a
        // rule reference AND a type mismatch in a different rule).
        let validation_errors = match graph.validate(all_docs) {
            Ok(()) => Vec::new(),
            Err(errors) => errors,
        };

        let mut all_errors = graph_errors;
        all_errors.extend(validation_errors);

        if all_errors.is_empty() {
            Ok(graph)
        } else {
            Err(all_errors)
        }
    }

    fn validate(&mut self, all_docs: &[LemmaDoc]) -> Result<(), Vec<LemmaError>> {
        let mut errors = Vec::new();

        // Structural checks (no type info needed)
        if let Err(structural_errors) = check_all_rule_references_exist(self) {
            errors.extend(structural_errors);
        }
        if let Err(collision_errors) = check_fact_and_rule_name_collisions(self) {
            errors.extend(collision_errors);
        }

        let execution_order = match self.topological_sort() {
            Ok(order) => order,
            Err(circular_errors) => {
                errors.extend(circular_errors);
                return Err(errors);
            }
        };

        // Continue to type inference and type checking even when structural
        // checks found errors.  This lets us report structural errors (e.g.
        // missing rule reference) alongside type errors (e.g. branch type
        // mismatch) in a single pass.

        // Phase 1: Infer types (pure, no errors)
        let inferred_types = infer_rule_types(self, &execution_order);

        // Phase 2: Check types (pure, returns Result)
        if let Err(type_errors) = check_rule_types(self, &execution_order, &inferred_types) {
            errors.extend(type_errors);
        }

        // Document interface validation uses inferred types (not yet applied to graph)
        let referenced_rules = compute_referenced_rules_by_path(self);
        let doc_ref_facts: Vec<(FactPath, String, Source)> = self
            .facts()
            .iter()
            .filter_map(|(path, fact)| {
                fact.doc_ref()
                    .map(|doc_name| (path.clone(), doc_name.to_string(), fact.source().clone()))
            })
            .collect();
        let rule_entries: IndexMap<RulePath, RuleEntryForBindingCheck> = self
            .rules()
            .iter()
            .map(|(path, node)| {
                let rule_type = inferred_types
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| node.rule_type.clone());
                (
                    path.clone(),
                    RuleEntryForBindingCheck {
                        rule_type,
                        depends_on_rules: node.depends_on_rules.clone(),
                        branches: node.branches.clone(),
                    },
                )
            })
            .collect();
        if let Err(interface_errors) = validate_document_interfaces(
            &referenced_rules,
            &doc_ref_facts,
            &rule_entries,
            all_docs,
            self.sources(),
        ) {
            errors.extend(interface_errors);
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        // Phase 3: Apply (only on full success)
        apply_inferred_types(self, inferred_types);
        self.execution_order = execution_order;
        Ok(())
    }
}

impl<'a> GraphBuilder<'a> {
    fn engine_error(&self, message: impl Into<String>, source: &Source) -> LemmaError {
        LemmaError::engine(message.into(), Some(source.clone()), None::<String>)
    }

    /// Resolve a TypeDeclaration ParsedFactValue into a LemmaType
    fn resolve_type_declaration(
        &self,
        type_decl: &ParsedFactValue,
        decl_source: &Source,
        context_doc: &str,
    ) -> Result<LemmaType, Vec<LemmaError>> {
        let ParsedFactValue::TypeDeclaration {
            base,
            constraints,
            from,
        } = type_decl
        else {
            unreachable!(
                "BUG: resolve_type_declaration called with non-TypeDeclaration ParsedFactValue"
            );
        };

        if base.is_empty() {
            return Err(vec![
                self.engine_error("TypeDeclaration base cannot be empty", decl_source)
            ]);
        }

        // Get resolved types for the source document.
        // If 'from' is specified, resolve from that document; otherwise use context_doc.
        // DocRef.name is already the clean name (@ stripped by parser).
        let source_doc = from
            .as_ref()
            .map(|r| r.name.as_str())
            .unwrap_or(context_doc);

        // Try to resolve as a primitive type first (number, boolean, etc.)
        let (base_lemma_type, extends) = if let Some(specs) = Graph::resolve_primitive_type(base) {
            // Primitive type
            (LemmaType::primitive(specs), TypeExtends::Primitive)
        } else {
            // Custom type - look up in resolved types
            let document_types = self.resolved_types.get(source_doc).ok_or_else(|| {
                vec![self.engine_error(
                    format!("Resolved types not found for document '{}'", source_doc),
                    decl_source,
                )]
            })?;

            let base_type = document_types
                .named_types
                .get(base)
                .ok_or_else(|| {
                    vec![self.engine_error(
                        format!("Unknown type: '{}'. Type must be defined before use.", base),
                        decl_source,
                    )]
                })?
                .clone();
            let family = base_type
                .scale_family_name()
                .map(String::from)
                .unwrap_or_else(|| base.clone());
            let extends = TypeExtends::Custom {
                parent: base.to_string(),
                family,
            };
            (base_type, extends)
        };

        // Apply inline constraints if any
        let mut errors = Vec::new();
        let mut specs = base_lemma_type.specifications;
        if let Some(ref constraints_vec) = constraints {
            for (command, args) in constraints_vec {
                match specs.clone().apply_constraint(command, args) {
                    Ok(updated) => specs = updated,
                    Err(e) => errors.push(self.engine_error(
                        format!("Invalid command '{}' for type '{}': {}", command, base, e),
                        decl_source,
                    )),
                }
            }
            errors.extend(validate_type_specifications(&specs, base, decl_source));
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(LemmaType::new(base.clone(), specs, extends))
    }

    /// Validate a fact binding path by walking through document references.
    ///
    /// Returns the binding key (full path as fact names from root) and validates
    /// that each segment in the path is a document reference. The binding key uses
    /// fact names only (no doc names) so that doc ref bindings don't cause mismatches.
    fn resolve_fact_binding(
        &self,
        fact: &LemmaFact,
        current_segment_names: &[String],
        effective_doc_refs: &HashMap<String, String>,
    ) -> Result<(Vec<String>, ParsedFactValue, Source), Vec<LemmaError>> {
        let fact_source = &fact.source_location;
        let binding_path_display = format!(
            "{}.{}",
            fact.reference.segments.join("."),
            fact.reference.fact
        );

        let mut current_doc_name: Option<String> = None;

        for (index, segment) in fact.reference.segments.iter().enumerate() {
            let doc_name = if index == 0 {
                match effective_doc_refs.get(segment) {
                    Some(name) => name.clone(),
                    None => {
                        return Err(vec![self.engine_error(
                            format!(
                                "Invalid fact binding path '{}': '{}' is not a document reference",
                                binding_path_display, segment
                            ),
                            fact_source,
                        )]);
                    }
                }
            } else {
                let prev_doc_name = current_doc_name.as_ref().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: current_doc_name should be set after first segment in resolve_fact_binding"
                    )
                });
                let prev_doc = match self.all_docs.get(prev_doc_name.as_str()) {
                    Some(d) => d,
                    None => {
                        return Err(vec![self.engine_error(
                            format!(
                                "Invalid fact binding path '{}': document '{}' not found",
                                binding_path_display, prev_doc_name
                            ),
                            fact_source,
                        )]);
                    }
                };

                let seg_fact = prev_doc
                    .facts
                    .iter()
                    .find(|f| f.reference.segments.is_empty() && f.reference.fact == *segment);

                match seg_fact {
                    Some(f) => match &f.value {
                        ParsedFactValue::DocumentReference(doc_ref) => doc_ref.name.clone(),
                        _ => {
                            return Err(vec![self.engine_error(
                                format!(
                                    "Invalid fact binding path '{}': '{}' in document '{}' is not a document reference",
                                    binding_path_display, segment, prev_doc_name
                                ),
                                fact_source,
                            )]);
                        }
                    },
                    None => {
                        return Err(vec![self.engine_error(
                            format!(
                                "Invalid fact binding path '{}': fact '{}' not found in document '{}'",
                                binding_path_display, segment, prev_doc_name
                            ),
                            fact_source,
                        )]);
                    }
                }
            };

            current_doc_name = Some(doc_name);
        }

        // Build the binding key: current_segment_names ++ fact.reference.segments ++ [fact.reference.fact]
        let mut binding_key: Vec<String> = current_segment_names.to_vec();
        binding_key.extend(fact.reference.segments.iter().cloned());
        binding_key.push(fact.reference.fact.clone());

        Ok((
            binding_key,
            fact.value.clone(),
            fact.source_location.clone(),
        ))
    }

    /// Build the fact bindings declared in a document.
    ///
    /// For each cross-document fact (reference.segments is non-empty), validate the path
    /// and collect into a FactBindings map. Rejects TypeDeclaration binding values and
    /// duplicate bindings targeting the same path.
    fn build_fact_bindings(
        &self,
        doc: &LemmaDoc,
        current_segment_names: &[String],
        effective_doc_refs: &HashMap<String, String>,
    ) -> Result<FactBindings, Vec<LemmaError>> {
        let mut bindings: FactBindings = HashMap::new();
        let mut errors: Vec<LemmaError> = Vec::new();

        for fact in &doc.facts {
            if fact.reference.segments.is_empty() {
                continue; // Local facts are not bindings
            }

            // Reject TypeDeclaration as binding value (single enforcement point)
            if matches!(&fact.value, ParsedFactValue::TypeDeclaration { .. }) {
                let binding_path_display = format!(
                    "{}.{}",
                    fact.reference.segments.join("."),
                    fact.reference.fact
                );
                errors.push(self.engine_error(
                    format!(
                        "Fact binding '{}' must provide a literal value or document reference, not a type declaration",
                        binding_path_display
                    ),
                    &fact.source_location,
                ));
                continue;
            }

            match self.resolve_fact_binding(fact, current_segment_names, effective_doc_refs) {
                Ok((binding_key, fact_value, source)) => {
                    if let Some((_, existing_source)) = bindings.get(&binding_key) {
                        errors.push(self.engine_error(
                            format!(
                                "Duplicate fact binding for '{}' (previously bound at {}:{})",
                                binding_key.join("."),
                                existing_source.attribute,
                                existing_source.span.line
                            ),
                            &fact.source_location,
                        ));
                    } else {
                        bindings.insert(binding_key, (fact_value, source));
                    }
                }
                Err(mut resolve_errors) => {
                    errors.append(&mut resolve_errors);
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(bindings)
    }

    /// Add a single local fact to the graph.
    ///
    /// Determines the effective value by checking `fact_bindings` for an entry at
    /// the fact's path. If a binding exists, uses the bound value; otherwise uses
    /// the fact's own value. Reports an error on duplicate facts.
    #[allow(clippy::too_many_arguments)]
    fn add_fact(
        &mut self,
        fact: &'a LemmaFact,
        current_segments: &[PathSegment],
        fact_bindings: &FactBindings,
        effective_doc_refs: &HashMap<String, String>,
        current_doc: &'a LemmaDoc,
        used_binding_keys: &mut HashSet<Vec<String>>,
    ) {
        let fact_path = FactPath {
            segments: current_segments.to_vec(),
            fact: fact.reference.fact.clone(),
        };

        // Check for duplicates
        if self.facts.contains_key(&fact_path) {
            self.errors.push(self.engine_error(
                format!("Duplicate fact '{}'", fact_path.fact),
                &fact.source_location,
            ));
            return;
        }

        // Build the binding key for this fact: segment fact names + fact name
        let binding_key: Vec<String> = current_segments
            .iter()
            .map(|s| s.fact.clone())
            .chain(std::iter::once(fact.reference.fact.clone()))
            .collect();

        // Determine the effective value: use the binding if present, else the fact's own value
        let (effective_value, effective_source) = match fact_bindings.get(&binding_key) {
            Some((bound_value, bound_source)) => {
                used_binding_keys.insert(binding_key);
                (bound_value.clone(), bound_source.clone())
            }
            None => (fact.value.clone(), fact.source_location.clone()),
        };

        // Resolve the schema type from the original fact (if it's a TypeDeclaration).
        // This is needed when a binding provides a literal value for a TypeDeclaration fact:
        // the schema type from the declaration should be preserved.
        let original_schema_type = if matches!(&fact.value, ParsedFactValue::TypeDeclaration { .. })
        {
            match self.resolve_type_declaration(
                &fact.value,
                &fact.source_location,
                current_doc.name.as_str(),
            ) {
                Ok(t) => Some(t),
                Err(errs) => {
                    self.errors.extend(errs);
                    return;
                }
            }
        } else {
            None
        };

        match &effective_value {
            ParsedFactValue::Literal(value) => {
                let semantic_value = match value_to_semantic(value) {
                    Ok(s) => s,
                    Err(e) => {
                        self.errors.push(self.engine_error(e, &effective_source));
                        return;
                    }
                };
                let inferred_type = match value {
                    Value::Text(_) => primitive_text().clone(),
                    Value::Number(_) => primitive_number().clone(),
                    Value::Scale(_, unit) => self
                        .resolved_types
                        .get(&current_doc.name)
                        .and_then(|dt| dt.unit_index.get(unit))
                        .cloned()
                        .unwrap_or_else(|| primitive_scale().clone()),
                    Value::Boolean(_) => primitive_boolean().clone(),
                    Value::Date(_) => primitive_date().clone(),
                    Value::Time(_) => primitive_time().clone(),
                    Value::Duration(_, _) => primitive_duration().clone(),
                    Value::Ratio(_, _) => primitive_ratio().clone(),
                };
                // Use original schema type if the fact was declared as a TypeDeclaration;
                // otherwise use the inferred type from the literal.
                let schema_type = original_schema_type.unwrap_or(inferred_type);
                let literal_value = LiteralValue {
                    value: semantic_value,
                    lemma_type: schema_type,
                };
                self.facts.insert(
                    fact_path,
                    FactData::Value {
                        value: literal_value,
                        source: effective_source,
                    },
                );
            }
            ParsedFactValue::TypeDeclaration { .. } => {
                // If no binding overrides the value, store as TypeDeclaration (needs runtime value).
                let resolved_type = original_schema_type.unwrap_or_else(|| {
                    match self.resolve_type_declaration(
                        &effective_value,
                        &effective_source,
                        current_doc.name.as_str(),
                    ) {
                        Ok(t) => t,
                        Err(_) => {
                            // Error already pushed if original_schema_type failed;
                            // this path is for when the effective value IS a TypeDeclaration
                            // (no binding, or binding is also a TypeDeclaration — which should
                            // have been rejected by build_fact_bindings)
                            unreachable!(
                                "BUG: TypeDeclaration effective value without original_schema_type"
                            )
                        }
                    }
                });

                self.facts.insert(
                    fact_path,
                    FactData::TypeDeclaration {
                        resolved_type,
                        source: effective_source,
                    },
                );
            }
            ParsedFactValue::DocumentReference(_) => {
                // Use effective_doc_refs for the actual doc name (accounts for bound doc refs).
                // DocRef.name is already clean (@ stripped by parser).
                let effective_doc_name = effective_doc_refs
                    .get(&fact.reference.fact)
                    .cloned()
                    .unwrap_or_else(|| {
                        if let ParsedFactValue::DocumentReference(doc_ref) = &effective_value {
                            doc_ref.name.clone()
                        } else {
                            unreachable!(
                                "BUG: effective_value is DocumentReference but pattern didn't match"
                            )
                        }
                    });

                if !self.all_docs.contains_key(effective_doc_name.as_str()) {
                    self.errors.push(self.engine_error(
                        format!("Document '{}' not found", effective_doc_name),
                        &effective_source,
                    ));
                    return;
                }

                self.facts.insert(
                    fact_path,
                    FactData::DocumentRef {
                        doc_name: effective_doc_name,
                        source: effective_source,
                    },
                );
            }
        }
    }

    fn resolve_path_segments(
        &mut self,
        segments: &[String],
        reference_source: &Source,
        mut current_facts_map: HashMap<String, &'a LemmaFact>,
        mut path_segments: Vec<PathSegment>,
        effective_doc_refs: &HashMap<String, String>,
    ) -> Option<Vec<PathSegment>> {
        for (index, segment) in segments.iter().enumerate() {
            let fact_ref =
                match current_facts_map.get(segment) {
                    Some(f) => f,
                    None => {
                        self.errors.push(self.engine_error(
                            format!("Fact '{}' not found", segment),
                            reference_source,
                        ));
                        return None;
                    }
                };

            if let ParsedFactValue::DocumentReference(original_doc_ref) = &fact_ref.value {
                // Only use effective_doc_refs for the FIRST segment.
                // Subsequent segments use the actual document references from traversed documents.
                // DocRef.name is already the clean name (@ stripped by parser).
                let doc_name = if index == 0 {
                    effective_doc_refs
                        .get(segment)
                        .map(|s| s.as_str())
                        .unwrap_or(&original_doc_ref.name)
                } else {
                    &original_doc_ref.name
                };

                let next_doc = match self.all_docs.get(doc_name) {
                    Some(d) => d,
                    None => {
                        self.errors.push(self.engine_error(
                            format!("Document '{}' not found", doc_name),
                            reference_source,
                        ));
                        return None;
                    }
                };
                path_segments.push(PathSegment {
                    fact: segment.clone(),
                    doc: doc_name.to_string(),
                });
                current_facts_map = next_doc
                    .facts
                    .iter()
                    .map(|f| (f.reference.fact.clone(), f))
                    .collect();
            } else {
                self.errors.push(self.engine_error(
                    format!("Fact '{}' is not a document reference", segment),
                    reference_source,
                ));
                return None;
            }
        }
        Some(path_segments)
    }

    fn build_document(
        &mut self,
        doc: &'a LemmaDoc,
        current_segments: Vec<PathSegment>,
        fact_bindings: FactBindings,
        type_registry: &mut TypeRegistry,
    ) -> Result<(), Vec<LemmaError>> {
        // Step 1: Initial effective_doc_refs from this document's local facts.
        // DocRef.name is already the clean name (@ stripped by the parser).
        let mut effective_doc_refs: HashMap<String, String> = HashMap::new();
        for fact in doc.facts.iter() {
            if fact.reference.segments.is_empty() {
                if let ParsedFactValue::DocumentReference(doc_ref) = &fact.value {
                    effective_doc_refs.insert(fact.reference.fact.clone(), doc_ref.name.clone());
                }
            }
        }

        // Step 1b: Update effective_doc_refs with caller's doc ref bindings.
        // If the caller bound a local DocumentReference fact to a different document, use that.
        let current_segment_names: Vec<String> =
            current_segments.iter().map(|s| s.fact.clone()).collect();
        for fact in doc.facts.iter() {
            if fact.reference.segments.is_empty() {
                if let ParsedFactValue::DocumentReference(_) = &fact.value {
                    let mut binding_key = current_segment_names.clone();
                    binding_key.push(fact.reference.fact.clone());
                    if let Some((ParsedFactValue::DocumentReference(bound_doc_ref), _)) =
                        fact_bindings.get(&binding_key)
                    {
                        effective_doc_refs
                            .insert(fact.reference.fact.clone(), bound_doc_ref.name.clone());
                    }
                }
            }
        }

        // Step 2: Build fact bindings declared in this document (for passing to referenced docs)
        let this_doc_bindings =
            match self.build_fact_bindings(doc, &current_segment_names, &effective_doc_refs) {
                Ok(bindings) => bindings,
                Err(errors) => {
                    self.errors.extend(errors);
                    HashMap::new() // Continue with empty bindings to collect more errors
                }
            };

        // Build facts_map for rule resolution and other lookups
        let facts_map: HashMap<String, &LemmaFact> = doc
            .facts
            .iter()
            .map(|fact| (fact.reference.fact.clone(), fact))
            .collect();

        // Register inline type definitions from this document's facts (no insert yet).
        // Only top-level TypeDeclaration facts with 'from' or 'constraints' are inline type defs.
        if current_segments.is_empty() {
            for fact in &doc.facts {
                if !fact.reference.segments.is_empty() {
                    continue;
                }
                if let ParsedFactValue::TypeDeclaration {
                    base,
                    constraints: inline_constraints,
                    from,
                } = &fact.value
                {
                    if base.is_empty() {
                        self.errors.push(self.engine_error(
                            "TypeDeclaration base cannot be empty",
                            &fact.source_location,
                        ));
                        continue;
                    }
                    let is_inline_type_definition = from.is_some() || inline_constraints.is_some();
                    if is_inline_type_definition {
                        let source_location = fact.source_location.clone();
                        let inline_type_def = TypeDef::Inline {
                            source_location,
                            parent: base.clone(),
                            constraints: inline_constraints.clone(),
                            fact_ref: fact.reference.clone(),
                            from: from.clone(),
                        };
                        if let Err(e) = type_registry.register_type(&doc.name, inline_type_def) {
                            self.errors.push(e);
                        }
                    }
                }
            }
        }

        // Resolve inline types only — named types were already resolved by prepare_types.
        // Take the existing ResolvedDocumentTypes (from prepare_types) as the base,
        // so we never re-resolve named types and never produce duplicate errors.
        //
        // If prepare_types failed for this document, there is no entry in resolved_types.
        // That failure was already reported — skip inline resolution entirely.
        if let Some(existing_types) = self.resolved_types.remove(&doc.name) {
            match type_registry.resolve_inline_types(&doc.name, existing_types) {
                Ok(document_types) => {
                    for (fact_ref, lemma_type) in &document_types.inline_type_definitions {
                        let type_name = format!("{} (inline)", fact_ref.fact);
                        let fact = doc
                            .facts
                            .iter()
                            .find(|f| &f.reference == fact_ref)
                            .unwrap_or_else(|| {
                                unreachable!(
                                    "BUG: inline type definition for '{}' has no corresponding fact in document '{}'",
                                    fact_ref.fact, doc.name
                                )
                            });
                        let source = &fact.source_location;
                        let mut spec_errors = validate_type_specifications(
                            &lemma_type.specifications,
                            &type_name,
                            source,
                        );
                        self.errors.append(&mut spec_errors);
                    }
                    self.resolved_types.insert(doc.name.clone(), document_types);
                }
                Err(e) => {
                    self.errors.push(e);
                }
            }
        }

        // Step 4: Add local facts using caller's fact_bindings
        let mut used_binding_keys: HashSet<Vec<String>> = HashSet::new();
        for fact in &doc.facts {
            if !fact.reference.segments.is_empty() {
                continue; // Skip binding facts (processed in step 2)
            }
            self.add_fact(
                fact,
                &current_segments,
                &fact_bindings,
                &effective_doc_refs,
                doc,
                &mut used_binding_keys,
            );
        }

        // Rebuild effective_doc_refs from the graph so bound doc refs are reflected for rule resolution
        for (path, rfv) in &self.facts {
            if path.segments.len() != current_segments.len() {
                continue;
            }
            if !path
                .segments
                .iter()
                .zip(current_segments.iter())
                .all(|(a, b)| a.fact == b.fact && a.doc == b.doc)
            {
                continue;
            }
            if let FactData::DocumentRef { doc_name, .. } = rfv {
                effective_doc_refs.insert(path.fact.clone(), doc_name.clone());
            }
        }

        // Step 5: Recurse into document references
        for fact in &doc.facts {
            if !fact.reference.segments.is_empty() {
                continue;
            }
            if let ParsedFactValue::DocumentReference(_) = &fact.value {
                let doc_name = match effective_doc_refs.get(&fact.reference.fact) {
                    Some(name) => name.clone(),
                    None => continue, // Not a doc ref after all
                };
                let nested_doc = match self.all_docs.get(doc_name.as_str()) {
                    Some(d) => *d,
                    None => continue, // Error already reported in add_fact
                };
                let mut nested_segments = current_segments.clone();
                nested_segments.push(PathSegment {
                    fact: fact.reference.fact.clone(),
                    doc: doc_name.clone(),
                });

                // Combine this doc's bindings with pass-through bindings from the caller
                // that target facts deeper than this document
                let nested_segment_names: Vec<String> =
                    nested_segments.iter().map(|s| s.fact.clone()).collect();
                let mut combined_bindings = this_doc_bindings.clone();
                for (key, value_and_source) in &fact_bindings {
                    if key.len() > nested_segment_names.len()
                        && key[..nested_segment_names.len()] == nested_segment_names[..]
                        && !combined_bindings.contains_key(key)
                    {
                        combined_bindings.insert(key.clone(), value_and_source.clone());
                    }
                }

                if let Err(errs) = self.build_document(
                    nested_doc,
                    nested_segments,
                    combined_bindings,
                    type_registry,
                ) {
                    self.errors.extend(errs);
                }
            }
        }

        // Check for unused fact bindings that targeted this document's facts
        // Only check bindings at exactly this depth (deeper bindings are passed through)
        let expected_key_len = current_segments.len() + 1;
        for (binding_key, (_, binding_source)) in &fact_bindings {
            if binding_key.len() == expected_key_len
                && binding_key[..current_segments.len()]
                    .iter()
                    .zip(current_segments.iter())
                    .all(|(a, b)| a == &b.fact)
                && !used_binding_keys.contains(binding_key)
            {
                self.errors.push(self.engine_error(
                    format!(
                        "Fact binding targets a fact that does not exist in the referenced document: '{}'",
                        binding_key.join(".")
                    ),
                    binding_source,
                ));
            }
        }

        // Process all rules
        for rule in &doc.rules {
            self.add_rule(
                rule,
                doc,
                &facts_map,
                &current_segments,
                &effective_doc_refs,
            );
        }

        Ok(())
    }

    fn add_rule(
        &mut self,
        rule: &LemmaRule,
        current_doc: &'a LemmaDoc,
        facts_map: &HashMap<String, &'a LemmaFact>,
        current_segments: &[PathSegment],
        effective_doc_refs: &HashMap<String, String>,
    ) {
        let rule_path = RulePath {
            segments: current_segments.to_vec(),
            rule: rule.name.clone(),
        };

        if self.rules.contains_key(&rule_path) {
            let rule_source = &rule.source_location;
            self.errors.push(
                self.engine_error(format!("Duplicate rule '{}'", rule_path.rule), rule_source),
            );
            return;
        }

        let mut branches = Vec::new();
        let mut depends_on_rules = HashSet::new();

        let converted_expression = match self.convert_expression_and_extract_dependencies(
            &rule.expression,
            current_doc,
            facts_map,
            current_segments,
            &mut depends_on_rules,
            effective_doc_refs,
        ) {
            Some(expr) => expr,
            None => return,
        };
        branches.push((None, converted_expression));

        for unless_clause in &rule.unless_clauses {
            let converted_condition = match self.convert_expression_and_extract_dependencies(
                &unless_clause.condition,
                current_doc,
                facts_map,
                current_segments,
                &mut depends_on_rules,
                effective_doc_refs,
            ) {
                Some(expr) => expr,
                None => return,
            };
            let converted_result = match self.convert_expression_and_extract_dependencies(
                &unless_clause.result,
                current_doc,
                facts_map,
                current_segments,
                &mut depends_on_rules,
                effective_doc_refs,
            ) {
                Some(expr) => expr,
                None => return,
            };
            branches.push((Some(converted_condition), converted_result));
        }

        let rule_node = RuleNode {
            branches,
            source: rule.source_location.clone(),
            depends_on_rules,
            rule_type: LemmaType::veto_type(), // Initialized to veto_type; actual type computed in compute_all_rule_types during validation
        };

        self.rules.insert(rule_path, rule_node);
    }

    /// Converts left and right expressions and accumulates rule dependencies.
    /// Same `current_segments`, `depends_on_rules`, and `effective_doc_refs` semantics as [`convert_expression_and_extract_dependencies`](Self::convert_expression_and_extract_dependencies).
    #[allow(clippy::too_many_arguments)]
    fn convert_binary_operands(
        &mut self,
        left: &ast::Expression,
        right: &ast::Expression,
        current_doc: &'a LemmaDoc,
        facts_map: &HashMap<String, &'a LemmaFact>,
        current_segments: &[PathSegment],
        depends_on_rules: &mut HashSet<RulePath>,
        effective_doc_refs: &HashMap<String, String>,
    ) -> Option<(Expression, Expression)> {
        let converted_left = self.convert_expression_and_extract_dependencies(
            left,
            current_doc,
            facts_map,
            current_segments,
            depends_on_rules,
            effective_doc_refs,
        )?;
        let converted_right = self.convert_expression_and_extract_dependencies(
            right,
            current_doc,
            facts_map,
            current_segments,
            depends_on_rules,
            effective_doc_refs,
        )?;
        Some((converted_left, converted_right))
    }

    /// Converts an AST expression into a resolved expression and records any rule references.
    ///
    /// ## Parameters
    ///
    /// - **current_segments**: Path from the root document to the document we're currently converting in. Each segment is a (fact name, doc name) pair. Used to build full [`FactPath`]s and [`RulePath`]s when resolving references like `nested_doc.fact` or `nested_doc.rule?`.
    /// - **depends_on_rules**: Accumulator for the rule we're converting: every [`RulePath`] that this expression references (e.g. via `other_rule?` or `doc_ref.rule?`) is inserted here. Later used for topological ordering and cycle detection.
    /// - **effective_doc_refs**: For the current document, maps **fact name → doc name** for facts that are document references. E.g. `fact x = doc foo` gives `"x" → "foo"`. Includes bindings (e.g. `fact base.x = doc bar`). Used by [`resolve_path_segments`](Self::resolve_path_segments) when resolving the first segment of a path like `x.some_rule?`.
    fn convert_expression_and_extract_dependencies(
        &mut self,
        expr: &ast::Expression,
        current_doc: &'a LemmaDoc,
        facts_map: &HashMap<String, &'a LemmaFact>,
        current_segments: &[PathSegment],
        depends_on_rules: &mut HashSet<RulePath>,
        effective_doc_refs: &HashMap<String, String>,
    ) -> Option<Expression> {
        let expr_src = expr
            .source_location
            .as_ref()
            .expect("BUG: AST expression missing source location");
        match &expr.kind {
            ast::ExpressionKind::FactReference(r) => {
                let expr_source = expr_src;
                let segments = self.resolve_path_segments(
                    &r.segments,
                    expr_source,
                    facts_map.clone(),
                    current_segments.to_vec(),
                    effective_doc_refs,
                )?;

                if r.segments.is_empty() && !facts_map.contains_key(&r.fact) {
                    let is_rule = current_doc.rules.iter().any(|rule| rule.name == r.fact);
                    if is_rule {
                        self.errors.push(self.engine_error(
                            format!(
                                "'{}' is a rule, not a fact. Use '{}?' to reference rules",
                                r.fact, r.fact
                            ),
                            expr_source,
                        ));
                    } else {
                        self.errors.push(
                            self.engine_error(format!("Fact '{}' not found", r.fact), expr_source),
                        );
                    }
                    return None;
                }

                let fact_path = FactPath {
                    segments,
                    fact: r.fact.clone(),
                };

                Some(Expression {
                    kind: ExpressionKind::FactPath(fact_path),
                    source_location: expr.source_location.clone(),
                })
            }
            ast::ExpressionKind::UnresolvedUnitLiteral(_number, unit_name) => {
                let expr_source = expr_src;

                let Some(document_types) = self.resolved_types.get(&current_doc.name) else {
                    self.errors.push(self.engine_error(
                        format!(
                            "Cannot resolve unit '{}': types were not resolved for document '{}'",
                            unit_name, current_doc.name
                        ),
                        expr_source,
                    ));
                    return None;
                };

                let lemma_type = match document_types.unit_index.get(unit_name) {
                    Some(lemma_type) => lemma_type.clone(),
                    None => {
                        self.errors.push(self.engine_error(
                            format!(
                                "Unknown unit '{}' in document '{}'",
                                unit_name, current_doc.name
                            ),
                            expr_source,
                        ));
                        return None;
                    }
                };

                let source_text = self.sources.get(&expr_src.attribute).unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing sources entry for attribute '{}' (doc '{}')",
                        expr_src.attribute, current_doc.name
                    )
                });
                let literal_str = match expr_src.extract_text(source_text) {
                    Some(s) => s,
                    None => {
                        self.errors.push(self.engine_error(
                            "Could not extract source text for literal".to_string(),
                            expr_source,
                        ));
                        return None;
                    }
                };

                let value = match parse_number_unit(&literal_str, &lemma_type.specifications) {
                    Ok(v) => v,
                    Err(e) => {
                        self.errors.push(self.engine_error(e, expr_source));
                        return None;
                    }
                };

                let literal_value = match value {
                    Value::Scale(n, u) => LiteralValue::scale_with_type(n, u, lemma_type),
                    Value::Ratio(r, u) => LiteralValue::ratio_with_type(r, u, lemma_type),
                    _ => unreachable!(
                        "parse_number_unit only returns Scale or Ratio for unit_index types"
                    ),
                };
                Some(Expression {
                    kind: ExpressionKind::Literal(Box::new(literal_value)),
                    source_location: expr.source_location.clone(),
                })
            }
            ast::ExpressionKind::RuleReference(rule_ref) => {
                let expr_source = expr_src;
                let segments = self.resolve_path_segments(
                    &rule_ref.segments,
                    expr_source,
                    facts_map.clone(),
                    current_segments.to_vec(),
                    effective_doc_refs,
                )?;

                let rule_path = RulePath {
                    segments,
                    rule: rule_ref.rule.clone(),
                };

                depends_on_rules.insert(rule_path.clone());

                Some(Expression {
                    kind: ExpressionKind::RulePath(rule_path),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::LogicalAnd(left, right) => {
                let (l, r) = self.convert_binary_operands(
                    left,
                    right,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )?;
                Some(Expression {
                    kind: ExpressionKind::LogicalAnd(Arc::new(l), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::LogicalOr(left, right) => {
                let (l, r) = self.convert_binary_operands(
                    left,
                    right,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )?;
                Some(Expression {
                    kind: ExpressionKind::LogicalOr(Arc::new(l), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::Arithmetic(left, op, right) => {
                let (l, r) = self.convert_binary_operands(
                    left,
                    right,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )?;
                Some(Expression {
                    kind: ExpressionKind::Arithmetic(Arc::new(l), op.clone(), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::Comparison(left, op, right) => {
                let (l, r) = self.convert_binary_operands(
                    left,
                    right,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )?;
                Some(Expression {
                    kind: ExpressionKind::Comparison(Arc::new(l), op.clone(), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::UnitConversion(value, target) => {
                let converted_value = self.convert_expression_and_extract_dependencies(
                    value,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )?;

                let unit_index = self
                    .resolved_types
                    .get(&current_doc.name)
                    .map(|dt| &dt.unit_index);
                let semantic_target = match conversion_target_to_semantic(target, unit_index) {
                    Ok(t) => t,
                    Err(msg) => {
                        let full_msg = unit_index
                            .map(|idx| {
                                let valid: Vec<&str> = idx.keys().map(String::as_str).collect();
                                format!("{} Valid units: {}", msg, valid.join(", "))
                            })
                            .unwrap_or(msg);
                        self.errors.push(LemmaError::semantic(
                            full_msg,
                            expr.source_location.clone(),
                            None::<String>,
                        ));
                        return None;
                    }
                };

                Some(Expression {
                    kind: ExpressionKind::UnitConversion(
                        Arc::new(converted_value),
                        semantic_target,
                    ),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::LogicalNegation(operand, neg_type) => {
                let converted_operand = self.convert_expression_and_extract_dependencies(
                    operand,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )?;
                Some(Expression {
                    kind: ExpressionKind::LogicalNegation(
                        Arc::new(converted_operand),
                        neg_type.clone(),
                    ),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::MathematicalComputation(op, operand) => {
                let converted_operand = self.convert_expression_and_extract_dependencies(
                    operand,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )?;
                Some(Expression {
                    kind: ExpressionKind::MathematicalComputation(
                        op.clone(),
                        Arc::new(converted_operand),
                    ),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::Literal(value) => {
                // Convert AST Value to semantic ValueKind
                let semantic_value = match value_to_semantic(value) {
                    Ok(v) => v,
                    Err(e) => {
                        self.errors.push(self.engine_error(e, expr_src));
                        return None;
                    }
                };
                // Create LiteralValue with inferred type from the Value
                let lemma_type = match value {
                    Value::Text(_) => primitive_text().clone(),
                    Value::Number(_) => primitive_number().clone(),
                    Value::Scale(_, unit) => self
                        .resolved_types
                        .get(&current_doc.name)
                        .and_then(|dt| dt.unit_index.get(unit))
                        .cloned()
                        .unwrap_or_else(|| primitive_scale().clone()),
                    Value::Boolean(_) => primitive_boolean().clone(),
                    Value::Date(_) => primitive_date().clone(),
                    Value::Time(_) => primitive_time().clone(),
                    Value::Duration(_, _) => primitive_duration().clone(),
                    Value::Ratio(_, _) => primitive_ratio().clone(),
                };
                let literal_value = LiteralValue {
                    value: semantic_value,
                    lemma_type,
                };
                Some(Expression {
                    kind: ExpressionKind::Literal(Box::new(literal_value)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::Veto(veto_expression) => Some(Expression {
                kind: ExpressionKind::Veto(veto_expression.clone()),
                source_location: expr.source_location.clone(),
            }),
        }
    }
}

fn compute_arithmetic_result_type(left_type: LemmaType, right_type: LemmaType) -> LemmaType {
    compute_arithmetic_result_type_recursive(left_type, right_type, false)
}

fn compute_arithmetic_result_type_recursive(
    left_type: LemmaType,
    right_type: LemmaType,
    swapped: bool,
) -> LemmaType {
    match (&left_type.specifications, &right_type.specifications) {
        (TypeSpecification::Error, _) => LemmaType::error_type(),

        (TypeSpecification::Date { .. }, TypeSpecification::Date { .. }) => {
            primitive_duration().clone()
        }
        (TypeSpecification::Date { .. }, TypeSpecification::Time { .. }) => {
            primitive_duration().clone()
        }
        (TypeSpecification::Time { .. }, TypeSpecification::Time { .. }) => {
            primitive_duration().clone()
        }

        _ if left_type == right_type => left_type,

        (TypeSpecification::Date { .. }, TypeSpecification::Duration { .. }) => left_type,
        (TypeSpecification::Time { .. }, TypeSpecification::Duration { .. }) => left_type,

        (TypeSpecification::Scale { .. }, TypeSpecification::Ratio { .. }) => left_type,
        (TypeSpecification::Scale { .. }, TypeSpecification::Number { .. }) => left_type,
        (TypeSpecification::Scale { .. }, TypeSpecification::Duration { .. }) => {
            primitive_number().clone()
        }
        (TypeSpecification::Scale { .. }, TypeSpecification::Scale { .. }) => left_type,

        (TypeSpecification::Duration { .. }, TypeSpecification::Number { .. }) => left_type,
        (TypeSpecification::Duration { .. }, TypeSpecification::Ratio { .. }) => left_type,
        (TypeSpecification::Duration { .. }, TypeSpecification::Duration { .. }) => {
            primitive_duration().clone()
        }

        (TypeSpecification::Number { .. }, TypeSpecification::Ratio { .. }) => {
            primitive_number().clone()
        }
        (TypeSpecification::Number { .. }, TypeSpecification::Number { .. }) => {
            primitive_number().clone()
        }

        (TypeSpecification::Ratio { .. }, TypeSpecification::Ratio { .. }) => left_type,

        _ => {
            if swapped {
                LemmaType::error_type()
            } else {
                compute_arithmetic_result_type_recursive(right_type, left_type, true)
            }
        }
    }
}

// =============================================================================
// Phase 1: Pure type inference (no validation, no error collection)
// =============================================================================

/// Infer the type of an expression without performing any validation.
/// Returns `LemmaType::error_type()` when a type cannot be determined (e.g. unknown fact).
/// This function is pure: it takes `&Graph` and returns a `LemmaType` with no side effects.
fn infer_expression_type(
    expression: &Expression,
    graph: &Graph,
    computed_rule_types: &HashMap<RulePath, LemmaType>,
) -> LemmaType {
    match &expression.kind {
        ExpressionKind::Literal(literal_value) => literal_value.as_ref().get_type().clone(),

        ExpressionKind::FactPath(fact_path) => infer_fact_type(fact_path, graph),

        ExpressionKind::RulePath(rule_path) => computed_rule_types
            .get(rule_path)
            .cloned()
            .unwrap_or_else(LemmaType::error_type),

        ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
            let left_type = infer_expression_type(left, graph, computed_rule_types);
            let right_type = infer_expression_type(right, graph, computed_rule_types);
            if left_type.is_error() || right_type.is_error() {
                return LemmaType::error_type();
            }
            primitive_boolean().clone()
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let operand_type = infer_expression_type(operand, graph, computed_rule_types);
            if operand_type.is_error() {
                return LemmaType::error_type();
            }
            primitive_boolean().clone()
        }

        ExpressionKind::Comparison(left, _op, right) => {
            let left_type = infer_expression_type(left, graph, computed_rule_types);
            let right_type = infer_expression_type(right, graph, computed_rule_types);
            if left_type.is_error() || right_type.is_error() {
                return LemmaType::error_type();
            }
            primitive_boolean().clone()
        }

        ExpressionKind::Arithmetic(left, _operator, right) => {
            let left_type = infer_expression_type(left, graph, computed_rule_types);
            let right_type = infer_expression_type(right, graph, computed_rule_types);
            compute_arithmetic_result_type(left_type, right_type)
        }

        ExpressionKind::UnitConversion(source_expression, target) => {
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in infer_expression_type");
            let source_type = infer_expression_type(source_expression, graph, computed_rule_types);
            if source_type.is_error() {
                return LemmaType::error_type();
            }
            match target {
                SemanticConversionTarget::Duration(_) => primitive_duration().clone(),
                SemanticConversionTarget::ScaleUnit(unit_name) => {
                    if source_type.is_number() {
                        let doc_name = &expr_source.doc_name;
                        graph
                            .resolved_types
                            .get(doc_name)
                            .and_then(|dt| dt.unit_index.get(unit_name).cloned())
                            .unwrap_or_else(LemmaType::error_type)
                    } else {
                        source_type
                    }
                }
                SemanticConversionTarget::RatioUnit(unit_name) => {
                    if source_type.is_number() {
                        let doc_name = &expr_source.doc_name;
                        graph
                            .resolved_types
                            .get(doc_name)
                            .and_then(|dt| dt.unit_index.get(unit_name).cloned())
                            .unwrap_or_else(LemmaType::error_type)
                    } else {
                        source_type
                    }
                }
            }
        }

        ExpressionKind::MathematicalComputation(_, operand) => {
            let operand_type = infer_expression_type(operand, graph, computed_rule_types);
            if operand_type.is_error() {
                return LemmaType::error_type();
            }
            primitive_number().clone()
        }

        ExpressionKind::Veto(_) => LemmaType::veto_type(),
    }
}

/// Infer the type of a fact reference without producing errors.
/// Returns `LemmaType::error_type()` when the fact cannot be found or is a document reference.
fn infer_fact_type(fact_path: &FactPath, graph: &Graph) -> LemmaType {
    let entry = match graph.facts().get(fact_path) {
        Some(e) => e,
        None => return LemmaType::error_type(),
    };
    match entry {
        FactData::Value { value, .. } => value.lemma_type.clone(),
        FactData::TypeDeclaration { resolved_type, .. } => resolved_type.clone(),
        FactData::DocumentRef { .. } => LemmaType::error_type(),
    }
}

// =============================================================================
// Phase 2: Pure type checking (validation only, no mutation, returns Result)
// =============================================================================

/// Construct a LemmaError::engine with source context.
fn engine_error_at(_graph: &Graph, source: &Source, message: impl Into<String>) -> LemmaError {
    LemmaError::engine(message.into(), Some(source.clone()), None::<String>)
}

/// Construct a LemmaError::semantic with source context.
fn semantic_error_at(_graph: &Graph, source: &Source, message: impl Into<String>) -> LemmaError {
    LemmaError::semantic(message.into(), Some(source.clone()), None::<String>)
}

/// Check that both operands of a logical operation (and/or) are boolean.
fn check_logical_operands(
    left_type: &LemmaType,
    right_type: &LemmaType,
    graph: &Graph,
    source: &Source,
) -> Result<(), Vec<LemmaError>> {
    let mut errors = Vec::new();
    if !left_type.is_boolean() {
        errors.push(engine_error_at(
            graph,
            source,
            format!(
                "Logical operation requires boolean operands, got {:?} for left operand",
                left_type
            ),
        ));
    }
    if !right_type.is_boolean() {
        errors.push(engine_error_at(
            graph,
            source,
            format!(
                "Logical operation requires boolean operands, got {:?} for right operand",
                right_type
            ),
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Check that the operand of a logical negation is boolean.
fn check_logical_operand(
    operand_type: &LemmaType,
    graph: &Graph,
    source: &Source,
) -> Result<(), Vec<LemmaError>> {
    if !operand_type.is_boolean() {
        Err(vec![engine_error_at(
            graph,
            source,
            format!(
                "Logical negation requires boolean operand, got {:?}",
                operand_type
            ),
        )])
    } else {
        Ok(())
    }
}

/// Check that a comparison operation has compatible operand types.
fn check_comparison_types(
    left_type: &LemmaType,
    op: &crate::ComparisonComputation,
    right_type: &LemmaType,
    graph: &Graph,
    source: &Source,
) -> Result<(), Vec<LemmaError>> {
    let is_equality_only = matches!(
        op,
        crate::ComparisonComputation::Equal
            | crate::ComparisonComputation::NotEqual
            | crate::ComparisonComputation::Is
            | crate::ComparisonComputation::IsNot
    );

    if left_type.is_boolean() && right_type.is_boolean() {
        if !is_equality_only {
            return Err(vec![engine_error_at(
                graph,
                source,
                format!("Can only use == and != with booleans (got {})", op),
            )]);
        }
        return Ok(());
    }

    if left_type.is_text() && right_type.is_text() {
        if !is_equality_only {
            return Err(vec![engine_error_at(
                graph,
                source,
                format!("Can only use == and != with text (got {})", op),
            )]);
        }
        return Ok(());
    }

    if left_type.is_number() && right_type.is_number() {
        return Ok(());
    }

    if left_type.is_ratio() && right_type.is_ratio() {
        return Ok(());
    }

    if left_type.is_date() && right_type.is_date() {
        return Ok(());
    }

    if left_type.is_time() && right_type.is_time() {
        return Ok(());
    }

    if left_type.is_scale() && right_type.is_scale() {
        if !left_type.same_scale_family(right_type) {
            return Err(vec![engine_error_at(
                graph,
                source,
                format!(
                    "Cannot compare different scale types: {} and {}",
                    left_type.name(),
                    right_type.name()
                ),
            )]);
        }
        return Ok(());
    }

    if left_type.is_duration() && right_type.is_duration() {
        return Ok(());
    }
    if left_type.is_duration() && right_type.is_number() {
        return Ok(());
    }
    if left_type.is_number() && right_type.is_duration() {
        return Ok(());
    }

    Err(vec![engine_error_at(
        graph,
        source,
        format!("Cannot compare {:?} with {:?}", left_type, right_type),
    )])
}

/// Check that an arithmetic operation has compatible operand types and operator constraints.
/// This function folds in the operator constraint checking (previously `validate_arithmetic_operator_constraints`).
fn check_arithmetic_types(
    left_type: &LemmaType,
    right_type: &LemmaType,
    operator: &ArithmeticComputation,
    graph: &Graph,
    source: &Source,
) -> Result<(), Vec<LemmaError>> {
    // Date/Time: only Add and Subtract with Duration (or Date/Time - Date/Time)
    if left_type.is_date() || left_type.is_time() || right_type.is_date() || right_type.is_time() {
        let both_temporal = (left_type.is_date() || left_type.is_time())
            && (right_type.is_date() || right_type.is_time());
        let one_is_duration = left_type.is_duration() || right_type.is_duration();
        let valid = matches!(
            operator,
            ArithmeticComputation::Add | ArithmeticComputation::Subtract
        ) && (both_temporal || one_is_duration);
        if !valid {
            return Err(vec![engine_error_at(
                graph,
                source,
                format!(
                    "Cannot apply '{}' to {} and {}.",
                    operator,
                    left_type.name(),
                    right_type.name()
                ),
            )]);
        }
        return Ok(());
    }

    // Different scale families: reject all operators
    if left_type.is_scale() && right_type.is_scale() && !left_type.same_scale_family(right_type) {
        return Err(vec![engine_error_at(
            graph,
            source,
            format!(
                "Cannot {} different scale types: {} and {}. Operations between different scale types produce ambiguous result units.",
                match operator {
                    ArithmeticComputation::Add => "add",
                    ArithmeticComputation::Subtract => "subtract",
                    ArithmeticComputation::Multiply => "multiply",
                    ArithmeticComputation::Divide => "divide",
                    ArithmeticComputation::Modulo => "modulo",
                    ArithmeticComputation::Power => "power",
                },
                left_type.name(),
                right_type.name()
            ),
        )]);
    }

    // Only Scale, Number, Ratio, and Duration can participate in arithmetic
    let left_valid = left_type.is_scale()
        || left_type.is_number()
        || left_type.is_duration()
        || left_type.is_ratio();
    let right_valid = right_type.is_scale()
        || right_type.is_number()
        || right_type.is_duration()
        || right_type.is_ratio();

    if !left_valid || !right_valid {
        return Err(vec![engine_error_at(
            graph,
            source,
            format!(
                "Cannot apply '{}' to {} and {}.",
                operator,
                left_type.name(),
                right_type.name()
            ),
        )]);
    }

    // Operator-specific constraints (same base type is always allowed)
    if left_type.has_same_base_type(right_type) {
        return Ok(());
    }

    let pair = |a: fn(&LemmaType) -> bool, b: fn(&LemmaType) -> bool| {
        (a(left_type) && b(right_type)) || (b(left_type) && a(right_type))
    };

    let allowed = match operator {
        ArithmeticComputation::Multiply => {
            pair(LemmaType::is_scale, LemmaType::is_number)
                || pair(LemmaType::is_scale, LemmaType::is_ratio)
                || pair(LemmaType::is_scale, LemmaType::is_duration)
                || pair(LemmaType::is_duration, LemmaType::is_number)
                || pair(LemmaType::is_duration, LemmaType::is_ratio)
                || pair(LemmaType::is_number, LemmaType::is_ratio)
        }
        ArithmeticComputation::Divide => {
            pair(LemmaType::is_scale, LemmaType::is_number)
                || pair(LemmaType::is_scale, LemmaType::is_ratio)
                || pair(LemmaType::is_scale, LemmaType::is_duration)
                || (left_type.is_duration() && right_type.is_number())
                || (left_type.is_duration() && right_type.is_ratio())
                || pair(LemmaType::is_number, LemmaType::is_ratio)
        }
        ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
            pair(LemmaType::is_scale, LemmaType::is_number)
                || pair(LemmaType::is_scale, LemmaType::is_ratio)
                || pair(LemmaType::is_duration, LemmaType::is_number)
                || pair(LemmaType::is_duration, LemmaType::is_ratio)
                || pair(LemmaType::is_number, LemmaType::is_ratio)
        }
        ArithmeticComputation::Power => {
            (left_type.is_number()
                || left_type.is_scale()
                || left_type.is_ratio()
                || left_type.is_duration())
                && (right_type.is_number() || right_type.is_ratio())
        }
        ArithmeticComputation::Modulo => right_type.is_number() || right_type.is_ratio(),
    };

    if !allowed {
        return Err(vec![engine_error_at(
            graph,
            source,
            format!(
                "Cannot apply '{}' to {} and {}.",
                operator,
                left_type.name(),
                right_type.name(),
            ),
        )]);
    }

    Ok(())
}

/// Check that a unit conversion has a compatible source type.
fn check_unit_conversion_types(
    source_type: &LemmaType,
    target: &SemanticConversionTarget,
    graph: &Graph,
    source: &Source,
) -> Result<(), Vec<LemmaError>> {
    match target {
        SemanticConversionTarget::ScaleUnit(unit_name)
        | SemanticConversionTarget::RatioUnit(unit_name) => {
            let unit_check: Option<(bool, Vec<&str>)> = match (&source_type.specifications, target)
            {
                (
                    TypeSpecification::Scale { units, .. },
                    SemanticConversionTarget::ScaleUnit(_),
                ) => {
                    let valid: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
                    let found = units.iter().any(|u| u.name.eq_ignore_ascii_case(unit_name));
                    Some((found, valid))
                }
                (
                    TypeSpecification::Ratio { units, .. },
                    SemanticConversionTarget::RatioUnit(_),
                ) => {
                    let valid: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
                    let found = units.iter().any(|u| u.name.eq_ignore_ascii_case(unit_name));
                    Some((found, valid))
                }
                _ => None,
            };

            match unit_check {
                Some((true, _)) => Ok(()),
                Some((false, valid)) => Err(vec![engine_error_at(
                    graph,
                    source,
                    format!(
                        "Unknown unit '{}' for type {}. Valid units: {}",
                        unit_name,
                        source_type.name(),
                        valid.join(", ")
                    ),
                )]),
                None if source_type.is_number() => {
                    if graph
                        .resolved_types
                        .get(&source.doc_name)
                        .and_then(|dt| dt.unit_index.get(unit_name))
                        .is_none()
                    {
                        Err(vec![engine_error_at(
                            graph,
                            source,
                            format!(
                                "Unknown unit '{}' in document '{}'.",
                                unit_name, source.doc_name
                            ),
                        )])
                    } else {
                        Ok(())
                    }
                }
                None => Err(vec![engine_error_at(
                    graph,
                    source,
                    format!(
                        "Cannot convert {} to unit '{}'.",
                        source_type.name(),
                        unit_name
                    ),
                )]),
            }
        }
        SemanticConversionTarget::Duration(_) => {
            if !source_type.is_duration() && !source_type.is_numeric() {
                Err(vec![engine_error_at(
                    graph,
                    source,
                    format!("Cannot convert {} to duration.", source_type.name()),
                )])
            } else {
                Ok(())
            }
        }
    }
}

/// Check that the operand of a mathematical function (sqrt, sin, etc.) is numeric.
fn check_mathematical_operand(
    operand_type: &LemmaType,
    graph: &Graph,
    source: &Source,
) -> Result<(), Vec<LemmaError>> {
    if !operand_type.is_scale() && !operand_type.is_number() {
        Err(vec![engine_error_at(
            graph,
            source,
            format!(
                "Mathematical function requires numeric operand (scale or number), got {:?}",
                operand_type
            ),
        )])
    } else {
        Ok(())
    }
}

/// Check that all rule references in the graph point to existing rules.
fn check_all_rule_references_exist(graph: &Graph) -> Result<(), Vec<LemmaError>> {
    let mut errors = Vec::new();
    let existing_rules: HashSet<&RulePath> = graph.rules().keys().collect();
    for (rule_path, rule_node) in graph.rules() {
        for dependency in &rule_node.depends_on_rules {
            if !existing_rules.contains(dependency) {
                errors.push(engine_error_at(
                    graph,
                    &rule_node.source,
                    format!(
                        "Rule '{}' references non-existent rule '{}'",
                        rule_path.rule, dependency.rule
                    ),
                ));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Check that no fact and rule share the same name in the same document.
fn check_fact_and_rule_name_collisions(graph: &Graph) -> Result<(), Vec<LemmaError>> {
    let mut errors = Vec::new();
    for rule_path in graph.rules().keys() {
        let fact_path = FactPath::new(rule_path.segments.clone(), rule_path.rule.clone());
        if graph.facts().contains_key(&fact_path) {
            let rule_node = graph.rules().get(rule_path).unwrap_or_else(|| {
                unreachable!(
                    "BUG: rule '{}' missing from graph while validating name collisions",
                    rule_path.rule
                )
            });
            errors.push(engine_error_at(
                graph,
                &rule_node.source,
                format!(
                    "Name collision: '{}' is defined as both a fact and a rule",
                    fact_path
                ),
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Check that a fact reference is valid (exists and is not a bare document reference).
/// Also reports when a rule reference is missing the `?` suffix.
fn check_fact_reference(
    fact_path: &FactPath,
    graph: &Graph,
    fact_source: &Source,
) -> Result<(), Vec<LemmaError>> {
    let entry = match graph.facts().get(fact_path) {
        Some(e) => e,
        None => {
            let maybe_rule_path = RulePath {
                segments: fact_path.segments.clone(),
                rule: fact_path.fact.clone(),
            };

            if graph.rules().contains_key(&maybe_rule_path) {
                return Err(vec![semantic_error_at(
                    graph,
                    fact_source,
                    format!(
                        "Rule reference '{}' must use '?' (did you mean '{}?')",
                        fact_path, fact_path
                    ),
                )]);
            } else {
                return Err(vec![semantic_error_at(
                    graph,
                    fact_source,
                    format!("Unknown fact reference '{}'", fact_path),
                )]);
            }
        }
    };
    match entry {
        FactData::Value { .. } | FactData::TypeDeclaration { .. } => Ok(()),
        FactData::DocumentRef { .. } => Err(vec![engine_error_at(
            graph,
            entry.source(),
            format!(
                "Cannot compute type for document reference fact '{}'",
                fact_path
            ),
        )]),
    }
}

/// Check a single expression for type errors, given precomputed inferred types.
/// Recursively checks sub-expressions. Skips validation when either operand is `Error`
/// (the root cause is reported by `check_fact_reference` or similar).
fn check_expression(
    expression: &Expression,
    graph: &Graph,
    inferred_types: &HashMap<RulePath, LemmaType>,
) -> Result<(), Vec<LemmaError>> {
    let mut errors = Vec::new();

    let collect = |result: Result<(), Vec<LemmaError>>, errors: &mut Vec<LemmaError>| {
        if let Err(errs) = result {
            errors.extend(errs);
        }
    };

    match &expression.kind {
        ExpressionKind::Literal(_) => {}

        ExpressionKind::FactPath(fact_path) => {
            let fact_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_fact_reference(fact_path, graph, fact_source),
                &mut errors,
            );
        }

        ExpressionKind::RulePath(_) => {}

        ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
            collect(check_expression(left, graph, inferred_types), &mut errors);
            collect(check_expression(right, graph, inferred_types), &mut errors);

            let left_type = infer_expression_type(left, graph, inferred_types);
            let right_type = infer_expression_type(right, graph, inferred_types);
            if !left_type.is_error() && !right_type.is_error() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                collect(
                    check_logical_operands(&left_type, &right_type, graph, expr_source),
                    &mut errors,
                );
            }
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            collect(
                check_expression(operand, graph, inferred_types),
                &mut errors,
            );

            let operand_type = infer_expression_type(operand, graph, inferred_types);
            if !operand_type.is_error() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                collect(
                    check_logical_operand(&operand_type, graph, expr_source),
                    &mut errors,
                );
            }
        }

        ExpressionKind::Comparison(left, op, right) => {
            collect(check_expression(left, graph, inferred_types), &mut errors);
            collect(check_expression(right, graph, inferred_types), &mut errors);

            let left_type = infer_expression_type(left, graph, inferred_types);
            let right_type = infer_expression_type(right, graph, inferred_types);
            if !left_type.is_error() && !right_type.is_error() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                collect(
                    check_comparison_types(&left_type, op, &right_type, graph, expr_source),
                    &mut errors,
                );
            }
        }

        ExpressionKind::Arithmetic(left, operator, right) => {
            collect(check_expression(left, graph, inferred_types), &mut errors);
            collect(check_expression(right, graph, inferred_types), &mut errors);

            let left_type = infer_expression_type(left, graph, inferred_types);
            let right_type = infer_expression_type(right, graph, inferred_types);
            if !left_type.is_error() && !right_type.is_error() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                collect(
                    check_arithmetic_types(&left_type, &right_type, operator, graph, expr_source),
                    &mut errors,
                );
            }
        }

        ExpressionKind::UnitConversion(source_expression, target) => {
            collect(
                check_expression(source_expression, graph, inferred_types),
                &mut errors,
            );

            let source_type = infer_expression_type(source_expression, graph, inferred_types);
            if !source_type.is_error() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                collect(
                    check_unit_conversion_types(&source_type, target, graph, expr_source),
                    &mut errors,
                );

                // Check that unit can be resolved when source is a plain number
                if source_type.is_number() {
                    match target {
                        SemanticConversionTarget::ScaleUnit(unit_name)
                        | SemanticConversionTarget::RatioUnit(unit_name) => {
                            if graph
                                .resolved_types
                                .get(&expr_source.doc_name)
                                .and_then(|dt| dt.unit_index.get(unit_name))
                                .is_none()
                            {
                                errors.push(engine_error_at(
                                    graph,
                                    expr_source,
                                    format!(
                                        "Cannot resolve unit '{}' for document '{}' (types may not have been resolved)",
                                        unit_name,
                                        expr_source.doc_name
                                    ),
                                ));
                            }
                        }
                        SemanticConversionTarget::Duration(_) => {}
                    }
                }
            }
        }

        ExpressionKind::MathematicalComputation(_, operand) => {
            collect(
                check_expression(operand, graph, inferred_types),
                &mut errors,
            );

            let operand_type = infer_expression_type(operand, graph, inferred_types);
            if !operand_type.is_error() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                collect(
                    check_mathematical_operand(&operand_type, graph, expr_source),
                    &mut errors,
                );
            }
        }

        ExpressionKind::Veto(_) => {}
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Check all rule types in topological order, given precomputed inferred types.
/// Validates:
/// - Branch type consistency (all non-Veto branches must return the same primitive type)
/// - Condition types (unless clause conditions must be boolean)
/// - All sub-expressions via `check_expression`
fn check_rule_types(
    graph: &Graph,
    execution_order: &[RulePath],
    inferred_types: &HashMap<RulePath, LemmaType>,
) -> Result<(), Vec<LemmaError>> {
    let mut errors = Vec::new();

    let collect = |result: Result<(), Vec<LemmaError>>, errors: &mut Vec<LemmaError>| {
        if let Err(errs) = result {
            errors.extend(errs);
        }
    };

    for rule_path in execution_order {
        let branches = {
            let rule_node = match graph.rules().get(rule_path) {
                Some(node) => node,
                None => continue,
            };
            rule_node.branches.clone()
        };

        if branches.is_empty() {
            continue;
        }

        let (_, default_result) = &branches[0];
        collect(
            check_expression(default_result, graph, inferred_types),
            &mut errors,
        );
        let default_type = infer_expression_type(default_result, graph, inferred_types);

        let mut non_veto_type: Option<LemmaType> = None;
        if !default_type.is_veto() && !default_type.is_error() {
            non_veto_type = Some(default_type.clone());
        }

        for (branch_index, (condition, result)) in branches.iter().enumerate().skip(1) {
            if let Some(condition_expression) = condition {
                collect(
                    check_expression(condition_expression, graph, inferred_types),
                    &mut errors,
                );
                let condition_type =
                    infer_expression_type(condition_expression, graph, inferred_types);
                if !condition_type.is_boolean() && !condition_type.is_error() {
                    let condition_source = condition_expression
                        .source_location
                        .as_ref()
                        .expect("BUG: condition expression missing source in check_rule_types");
                    errors.push(engine_error_at(
                        graph,
                        condition_source,
                        format!(
                            "Unless clause condition in rule '{}' must be boolean, got {:?}",
                            rule_path.rule, condition_type
                        ),
                    ));
                }
            }

            collect(check_expression(result, graph, inferred_types), &mut errors);
            let result_type = infer_expression_type(result, graph, inferred_types);

            if !result_type.is_veto() && !result_type.is_error() {
                if non_veto_type.is_none() {
                    non_veto_type = Some(result_type.clone());
                } else if let Some(ref existing_type) = non_veto_type {
                    if !existing_type.has_same_base_type(&result_type) {
                        let Some(rule_node) = graph.rules().get(rule_path) else {
                            unreachable!(
                                "BUG: rule type validation referenced missing rule '{}'",
                                rule_path.rule
                            );
                        };
                        let rule_source = &rule_node.source;
                        let default_expr = &branches[0].1;

                        let mut location_parts = vec![format!(
                            "{}:{}:{}",
                            rule_source.attribute, rule_source.span.line, rule_source.span.col
                        )];

                        if let Some(loc) = &default_expr.source_location {
                            location_parts.push(format!(
                                "default branch at {}:{}:{}",
                                loc.attribute, loc.span.line, loc.span.col
                            ));
                        }
                        if let Some(loc) = &result.source_location {
                            location_parts.push(format!(
                                "unless clause {} at {}:{}:{}",
                                branch_index, loc.attribute, loc.span.line, loc.span.col
                            ));
                        }

                        errors.push(LemmaError::semantic(
                            format!("Type mismatch in rule '{}' in document '{}' ({}): default branch returns {}, but unless clause {} returns {}. All branches must return the same primitive type.",
                            rule_path.rule,
                            rule_source.doc_name,
                            location_parts.join(", "),
                            existing_type.name(),
                            branch_index,
                            result_type.name()),
                            Some(rule_source.clone()),
                            None::<String>,
                        ));
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// =============================================================================
// Phase 3: Apply inferred types to the graph (the only mutation point)
// =============================================================================

/// Write inferred types into the graph's rule nodes.
/// This is the only function that mutates the graph during the validation pipeline.
/// It must only be called after all checks pass (no errors).
fn apply_inferred_types(graph: &mut Graph, inferred_types: HashMap<RulePath, LemmaType>) {
    for (rule_path, rule_type) in inferred_types {
        if let Some(rule_node) = graph.rules_mut().get_mut(&rule_path) {
            rule_node.rule_type = rule_type;
        }
    }
}

/// Infer the types of all rules in topological order without performing any validation.
/// Returns a map from rule path to its inferred type.
/// This function is pure: it takes `&Graph` and returns data with no side effects.
fn infer_rule_types(graph: &Graph, execution_order: &[RulePath]) -> HashMap<RulePath, LemmaType> {
    let mut computed_types: HashMap<RulePath, LemmaType> = HashMap::new();

    for rule_path in execution_order {
        let branches = {
            let rule_node = match graph.rules().get(rule_path) {
                Some(node) => node,
                None => continue,
            };
            rule_node.branches.clone()
        };

        if branches.is_empty() {
            continue;
        }

        let (_, default_result) = &branches[0];
        let default_type = infer_expression_type(default_result, graph, &computed_types);

        let mut non_veto_type: Option<LemmaType> = None;
        if !default_type.is_veto() && !default_type.is_error() {
            non_veto_type = Some(default_type.clone());
        }

        for (_branch_index, (condition, result)) in branches.iter().enumerate().skip(1) {
            if let Some(condition_expression) = condition {
                let _condition_type =
                    infer_expression_type(condition_expression, graph, &computed_types);
            }

            let result_type = infer_expression_type(result, graph, &computed_types);
            if !result_type.is_veto() && !result_type.is_error() && non_veto_type.is_none() {
                non_veto_type = Some(result_type.clone());
            }
        }

        let rule_type = non_veto_type.unwrap_or_else(LemmaType::veto_type);
        computed_types.insert(rule_path.clone(), rule_type);
    }

    computed_types
}

fn compute_referenced_rules_by_path(graph: &Graph) -> HashMap<Vec<String>, HashSet<String>> {
    let mut referenced_rules: HashMap<Vec<String>, HashSet<String>> = HashMap::new();
    for rule_node in graph.rules().values() {
        for rule_dependency in &rule_node.depends_on_rules {
            if !rule_dependency.segments.is_empty() {
                let path: Vec<String> = rule_dependency
                    .segments
                    .iter()
                    .map(|segment| segment.fact.clone())
                    .collect();
                referenced_rules
                    .entry(path)
                    .or_default()
                    .insert(rule_dependency.rule.clone());
            }
        }
    }
    referenced_rules
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parsing::ast::{BooleanValue, FactReference, RuleReference, Span, Value};

    fn test_source() -> Source {
        Source::new(
            "test.lemma",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "test",
            Arc::from("doc test\nfact x = 1\nrule result = x"),
        )
    }

    fn test_sources() -> HashMap<String, String> {
        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), "doc test\n".to_string());
        sources
    }

    /// Test helper: prepare types and build graph in one call.
    fn build_graph(
        main_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
        sources: HashMap<String, String>,
    ) -> Result<Graph, Vec<LemmaError>> {
        let (prepared, type_errors) = Graph::prepare_types(all_docs, &sources);
        match Graph::build(main_doc, all_docs, sources, &prepared) {
            Ok(graph) => {
                if type_errors.is_empty() {
                    Ok(graph)
                } else {
                    Err(type_errors)
                }
            }
            Err(mut doc_errors) => {
                let mut all_errors = type_errors;
                all_errors.append(&mut doc_errors);
                Err(all_errors)
            }
        }
    }

    fn create_test_doc(name: &str) -> LemmaDoc {
        LemmaDoc::new(name.to_string())
    }

    fn create_literal_fact(name: &str, value: Value) -> LemmaFact {
        LemmaFact {
            reference: FactReference {
                segments: Vec::new(),
                fact: name.to_string(),
            },
            value: ParsedFactValue::Literal(value),
            source_location: test_source(),
        }
    }

    fn create_literal_expr(value: Value) -> ast::Expression {
        ast::Expression {
            kind: ast::ExpressionKind::Literal(value),
            source_location: Some(test_source()),
        }
    }

    #[test]
    fn test_build_simple_graph() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));
        doc = doc.add_fact(create_literal_fact("name", Value::Text("John".to_string())));

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_ok(), "Should build graph successfully");

        let graph = result.unwrap();
        assert_eq!(graph.facts().len(), 2);
        assert_eq!(graph.rules().len(), 0);
    }

    #[test]
    fn test_build_graph_with_rule() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));

        let age_expr = ast::Expression {
            kind: ast::ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "age".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule = LemmaRule {
            name: "is_adult".to_string(),
            expression: age_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        doc = doc.add_rule(rule);

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_ok(), "Should build graph successfully");

        let graph = result.unwrap();
        assert_eq!(graph.facts().len(), 1);
        assert_eq!(graph.rules().len(), 1);
    }

    #[test]
    fn should_reject_fact_binding_into_non_document_fact() {
        // Higher-standard language rule:
        // if `x` is a literal (not a document reference), `x.y = ...` must be rejected.
        //
        // This is currently expected to FAIL until graph building enforces it consistently.
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact("x", Value::Number(1.into())));

        // Bind x.y, but x is not a document reference.
        doc = doc.add_fact(LemmaFact {
            reference: FactReference::from_path(vec!["x".to_string(), "y".to_string()]),
            value: ParsedFactValue::Literal(Value::Number(2.into())),
            source_location: test_source(),
        });

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(
            result.is_err(),
            "Overriding x.y must fail when x is not a document reference"
        );
    }

    #[test]
    fn should_reject_fact_and_rule_name_collision() {
        // Higher-standard language rule: fact and rule names should not collide.
        // It's ambiguous for humans and leads to confusing error messages.
        //
        // This is currently expected to FAIL until the language enforces it.
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact("x", Value::Number(1.into())));
        doc = doc.add_rule(LemmaRule {
            name: "x".to_string(),
            expression: create_literal_expr(Value::Number(2.into())),
            unless_clauses: Vec::new(),
            source_location: test_source(),
        });

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(
            result.is_err(),
            "Fact and rule name collisions should be rejected"
        );
    }

    #[test]
    fn test_duplicate_fact() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));
        doc = doc.add_fact(create_literal_fact(
            "age",
            Value::Number(rust_decimal::Decimal::from(30)),
        ));

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_err(), "Should detect duplicate fact");

        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Duplicate fact") && e.to_string().contains("age")));
    }

    #[test]
    fn test_duplicate_rule() {
        let mut doc = create_test_doc("test");

        let rule1 = LemmaRule {
            name: "test_rule".to_string(),
            expression: create_literal_expr(Value::Boolean(BooleanValue::True)),
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        let rule2 = LemmaRule {
            name: "test_rule".to_string(),
            expression: create_literal_expr(Value::Boolean(BooleanValue::False)),
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };

        doc = doc.add_rule(rule1);
        doc = doc.add_rule(rule2);

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_err(), "Should detect duplicate rule");

        let errors = result.unwrap_err();
        assert!(errors.iter().any(
            |e| e.to_string().contains("Duplicate rule") && e.to_string().contains("test_rule")
        ));
    }

    #[test]
    fn test_missing_fact_reference() {
        let mut doc = create_test_doc("test");

        let missing_fact_expr = ast::Expression {
            kind: ast::ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "nonexistent".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: missing_fact_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        doc = doc.add_rule(rule);

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_err(), "Should detect missing fact");

        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Fact 'nonexistent' not found")));
    }

    #[test]
    fn test_missing_document_reference() {
        let mut doc = create_test_doc("test");

        let fact = LemmaFact {
            reference: FactReference {
                segments: Vec::new(),
                fact: "contract".to_string(),
            },
            value: ParsedFactValue::DocumentReference(crate::DocRef::local("nonexistent")),
            source_location: test_source(),
        };
        doc = doc.add_fact(fact);

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_err(), "Should detect missing document");

        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Document 'nonexistent' not found")));
    }

    #[test]
    fn test_fact_reference_conversion() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));

        let age_expr = ast::Expression {
            kind: ast::ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "age".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: age_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        doc = doc.add_rule(rule);

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_ok(), "Should build graph successfully");

        let graph = result.unwrap();
        let rule_node = graph.rules().values().next().unwrap();

        assert!(matches!(
            rule_node.branches[0].1.kind,
            ExpressionKind::FactPath(_)
        ));
    }

    #[test]
    fn test_rule_reference_conversion() {
        let mut doc = create_test_doc("test");

        let rule1_expr = ast::Expression {
            kind: ast::ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "age".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule1 = LemmaRule {
            name: "rule1".to_string(),
            expression: rule1_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        doc = doc.add_rule(rule1);

        let rule2_expr = ast::Expression {
            kind: ast::ExpressionKind::RuleReference(RuleReference {
                segments: Vec::new(),
                rule: "rule1".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule2 = LemmaRule {
            name: "rule2".to_string(),
            expression: rule2_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        doc = doc.add_rule(rule2);

        doc = doc.add_fact(create_literal_fact(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_ok(), "Should build graph successfully");

        let graph = result.unwrap();
        let rule2_node = graph
            .rules()
            .get(&RulePath {
                segments: Vec::new(),
                rule: "rule2".to_string(),
            })
            .unwrap();

        assert_eq!(rule2_node.depends_on_rules.len(), 1);
        assert!(matches!(
            rule2_node.branches[0].1.kind,
            ExpressionKind::RulePath(_)
        ));
    }

    #[test]
    fn test_collect_multiple_errors() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));
        doc = doc.add_fact(create_literal_fact(
            "age",
            Value::Number(rust_decimal::Decimal::from(30)),
        ));

        let missing_fact_expr = ast::Expression {
            kind: ast::ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "nonexistent".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: missing_fact_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        doc = doc.add_rule(rule);

        let result = build_graph(&doc, &[doc.clone()], test_sources());
        assert!(result.is_err(), "Should collect multiple errors");

        let errors = result.unwrap_err();
        assert!(errors.len() >= 2, "Should have at least 2 errors");
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Duplicate fact")));
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Fact 'nonexistent' not found")));
    }

    #[test]
    fn test_type_registration_collects_multiple_errors() {
        use crate::parsing::ast::TypeDef;

        let type_source = Source::new(
            "a.lemma",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "doc_a",
            Arc::from("doc test\nfact x = 1\nrule result = x"),
        );
        let doc_a = create_test_doc("doc_a")
            .with_attribute("a.lemma".to_string())
            .add_type(TypeDef::Regular {
                source_location: type_source.clone(),
                name: "money".to_string(),
                parent: "number".to_string(),
                constraints: None,
            })
            .add_type(TypeDef::Regular {
                source_location: type_source,
                name: "money".to_string(),
                parent: "number".to_string(),
                constraints: None,
            });

        let type_source_b = Source::new(
            "b.lemma",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "doc_b",
            Arc::from("doc test\nfact x = 1\nrule result = x"),
        );
        let doc_b = create_test_doc("doc_b")
            .with_attribute("b.lemma".to_string())
            .add_type(TypeDef::Regular {
                source_location: type_source_b.clone(),
                name: "length".to_string(),
                parent: "number".to_string(),
                constraints: None,
            })
            .add_type(TypeDef::Regular {
                source_location: type_source_b,
                name: "length".to_string(),
                parent: "number".to_string(),
                constraints: None,
            });

        let mut sources = HashMap::new();
        sources.insert(
            "a.lemma".to_string(),
            "doc doc_a\ntype money = number\ntype money = number".to_string(),
        );
        sources.insert(
            "b.lemma".to_string(),
            "doc doc_b\ntype length = number\ntype length = number".to_string(),
        );

        let result = build_graph(&doc_a, &[doc_a.clone(), doc_b.clone()], sources);
        assert!(result.is_err(), "Should fail with duplicate type errors");
        let errors = result.unwrap_err();
        assert!(
            errors.len() >= 2,
            "Should collect duplicate type error from each document, got {}",
            errors.len()
        );
        assert!(
            errors
                .iter()
                .any(|e| e.to_string().contains("Type 'money' is already defined")),
            "Should report duplicate 'money' in doc_a: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
        assert!(
            errors
                .iter()
                .any(|e| e.to_string().contains("Type 'length' is already defined")),
            "Should report duplicate 'length' in doc_b: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }
}
