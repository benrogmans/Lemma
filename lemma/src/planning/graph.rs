use crate::parsing::ast::Span;
use crate::parsing::source::Source;
use crate::planning::types::{ResolvedDocumentTypes, TypeRegistry};
use crate::planning::validation::validate_type_specifications;
use crate::semantic::{
    standard_boolean, standard_duration, standard_number, standard_ratio, ArithmeticComputation,
    ConversionTarget, Expression, ExpressionKind, FactPath, FactReference, FactValue, LemmaDoc,
    LemmaFact, LemmaRule, LemmaType, LiteralValue, PathSegment, RulePath, TypeDef,
    TypeSpecification,
};
use crate::LemmaError;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct Graph {
    facts: IndexMap<FactPath, LemmaFact>,
    rules: IndexMap<RulePath, RuleNode>,
    sources: HashMap<String, String>,
    execution_order: Vec<RulePath>,
    all_docs: HashMap<String, LemmaDoc>, // Store all_docs for document traversal and context determination
    resolved_types: HashMap<String, ResolvedDocumentTypes>,
}

impl Graph {
    pub(crate) fn facts(&self) -> &IndexMap<FactPath, LemmaFact> {
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

    pub(crate) fn all_docs(&self) -> &HashMap<String, LemmaDoc> {
        &self.all_docs
    }

    pub(crate) fn resolved_types(&self) -> &HashMap<String, ResolvedDocumentTypes> {
        &self.resolved_types
    }

    /// Resolve a standard type by name (helper function)
    fn resolve_standard_type(name: &str) -> Option<TypeSpecification> {
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

    /// Resolve a TypeDeclaration to a LemmaType
    ///
    /// This resolves both type references (e.g., [money]) and inline type definitions
    /// (e.g., [money -> minimal 100] or [number -> minimal 100]) to their final LemmaType.
    ///
    /// # Arguments
    /// * `type_decl` - The TypeDeclaration to resolve
    /// * `context_doc` - The document context where this type is being used
    pub(crate) fn resolve_type_declaration(
        &self,
        type_decl: &FactValue,
        context_doc: &str,
    ) -> Result<LemmaType, LemmaError> {
        let FactValue::TypeDeclaration {
            base,
            overrides,
            from,
        } = type_decl
        else {
            return Err(LemmaError::engine(
                "Expected TypeDeclaration",
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                context_doc,
                1,
                None::<String>,
            ));
        };

        // Get resolved types for the source document
        // If 'from' is specified, resolve from that document; otherwise use context_doc
        let source_doc = from.as_deref().unwrap_or(context_doc);

        // Try to resolve as a standard type first (number, boolean, etc.)
        let base_lemma_type = if let Some(specs) = Self::resolve_standard_type(base) {
            // Standard type - create LemmaType without name
            LemmaType::without_name(specs)
        } else {
            // Custom type - look up in resolved types
            let document_types = self.resolved_types.get(source_doc).ok_or_else(|| {
                LemmaError::engine(
                    format!("Resolved types not found for document '{}'", source_doc),
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    Arc::from(""),
                    context_doc,
                    1,
                    None::<String>,
                )
            })?;

            document_types
                .named_types
                .get(base)
                .ok_or_else(|| {
                    LemmaError::engine(
                        format!("Unknown type: '{}'. Type must be defined before use.", base),
                        Span {
                            start: 0,
                            end: 0,
                            line: 1,
                            col: 0,
                        },
                        "<unknown>",
                        Arc::from(""),
                        context_doc,
                        1,
                        None::<String>,
                    )
                })?
                .clone()
        };

        // Apply inline overrides if any
        let mut specs = base_lemma_type.specifications;
        if let Some(ref overrides_vec) = overrides {
            for (command, args) in overrides_vec {
                specs = specs.apply_override(command, args).map_err(|e| {
                    LemmaError::engine(
                        format!("Invalid command '{}' for type '{}': {}", command, base, e),
                        Span {
                            start: 0,
                            end: 0,
                            line: 1,
                            col: 0,
                        },
                        "<unknown>",
                        Arc::from(""),
                        context_doc,
                        1,
                        None::<String>,
                    )
                })?;
            }
        }

        // Create final LemmaType
        // For standard types, use without_name(); for custom types, preserve the name
        let lemma_type = if let Some(name) = base_lemma_type.name {
            LemmaType::new(name, specs)
        } else {
            LemmaType::without_name(specs)
        };

        Ok(lemma_type)
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
            return Err(vec![LemmaError::circular_dependency(
                format!(
                    "Circular dependency detected. Rules involved: {}",
                    missing
                        .iter()
                        .map(|rule| rule.rule.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                std::sync::Arc::from(""),
                "<unknown>",
                1,
                vec![],
                None::<String>,
            )]);
        }

        Ok(result)
    }
}

#[derive(Debug)]
pub(crate) struct RuleNode {
    /// First branch has condition=None (default expression), subsequent branches are unless clauses.
    /// Expressions are already converted (Reference -> FactPath, RuleReference -> RulePath).
    pub branches: Vec<(Option<Expression>, Expression)>,
    pub source: Source,

    pub depends_on_rules: HashSet<RulePath>,

    /// Computed type of this rule's result (populated during validation)
    /// Every rule MUST have a type (Lemma is strictly typed)
    pub rule_type: LemmaType,
}

struct GraphBuilder<'a> {
    facts: IndexMap<FactPath, LemmaFact>,
    rules: IndexMap<RulePath, RuleNode>,
    sources: HashMap<String, String>,
    all_docs: HashMap<String, &'a LemmaDoc>,
    resolved_types: HashMap<String, ResolvedDocumentTypes>,
    errors: Vec<LemmaError>,
}

impl Graph {
    pub(crate) fn build(
        main_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
        sources: HashMap<String, String>,
    ) -> Result<Graph, Vec<LemmaError>> {
        // Create and populate TypeRegistry
        let mut type_registry = TypeRegistry::new();
        for doc in all_docs {
            for type_def in &doc.types {
                if let Err(e) = type_registry.register_type(&doc.name, type_def.clone()) {
                    return Err(vec![e]);
                }
            }
        }

        let mut builder = GraphBuilder {
            facts: IndexMap::new(),
            rules: IndexMap::new(),
            sources,
            all_docs: all_docs.iter().map(|doc| (doc.name.clone(), doc)).collect(),
            resolved_types: HashMap::new(),
            errors: Vec::new(),
        };

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
                        let mut spec_errors =
                            validate_type_specifications(&lemma_type.specifications, type_name);
                        builder.errors.append(&mut spec_errors);
                    }
                    builder
                        .resolved_types
                        .insert(doc.name.clone(), document_types);
                }
                Err(e) => builder.errors.push(e),
            }
        }
        if !builder.errors.is_empty() {
            return Err(builder.errors);
        }

        builder.build_document(main_doc, Vec::new(), &mut type_registry)?;

        if !builder.errors.is_empty() {
            return Err(builder.errors);
        }

        let mut graph = Graph {
            facts: builder.facts,
            rules: builder.rules,
            sources: builder.sources,
            execution_order: Vec::new(),
            all_docs: all_docs
                .iter()
                .map(|doc| (doc.name.clone(), doc.clone()))
                .collect(),
            resolved_types: builder.resolved_types,
        };

        // Validate and compute execution order
        graph.validate(all_docs)?;

        Ok(graph)
    }

    fn validate(&mut self, all_docs: &[LemmaDoc]) -> Result<(), Vec<LemmaError>> {
        let mut errors = Vec::new();

        validate_document_interfaces(self, all_docs, &mut errors);
        validate_all_rule_references_exist(self, &mut errors);
        validate_fact_override_paths_target_document_facts(self, &mut errors);
        validate_fact_and_rule_name_collisions(self, &mut errors);

        let execution_order = match self.topological_sort() {
            Ok(order) => order,
            Err(circular_errors) => {
                errors.extend(circular_errors);
                Vec::new()
            }
        };

        if errors.is_empty() {
            compute_all_rule_types(self, &execution_order, &mut errors);
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        self.execution_order = execution_order;
        Ok(())
    }
}

impl<'a> GraphBuilder<'a> {
    fn build_document(
        &mut self,
        doc: &'a LemmaDoc,
        current_segments: Vec<PathSegment>,
        type_registry: &mut TypeRegistry,
    ) -> Result<(), Vec<LemmaError>> {
        self.build_document_with_overrides(doc, current_segments, HashMap::new(), type_registry)
    }

    fn resolve_path_segments_with_overrides(
        &mut self,
        segments: &[String],
        mut current_facts_map: HashMap<String, &'a LemmaFact>,
        mut path_segments: Vec<PathSegment>,
        effective_doc_refs: &HashMap<String, String>,
    ) -> Option<Vec<PathSegment>> {
        for (index, segment) in segments.iter().enumerate() {
            let fact_ref = match current_facts_map.get(segment) {
                Some(f) => f,
                None => {
                    self.errors.push(LemmaError::engine(
                        format!("Fact '{}' not found", segment),
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
                    return None;
                }
            };

            if let FactValue::DocumentReference(original_doc_name) = &fact_ref.value {
                // Only use effective_doc_refs for the FIRST segment
                // Subsequent segments use the actual document references from traversed documents
                let doc_name = if index == 0 {
                    effective_doc_refs.get(segment).unwrap_or(original_doc_name)
                } else {
                    original_doc_name
                };

                let next_doc = match self.all_docs.get(doc_name) {
                    Some(d) => d,
                    None => {
                        self.errors.push(LemmaError::engine(
                            format!("Document '{}' not found", doc_name),
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
                        return None;
                    }
                };
                path_segments.push(PathSegment {
                    fact: segment.clone(),
                    doc: doc_name.clone(),
                });
                current_facts_map = next_doc
                    .facts
                    .iter()
                    .map(|f| (f.reference.fact.clone(), f))
                    .collect();
            } else {
                self.errors.push(LemmaError::engine(
                    format!("Fact '{}' is not a document reference", segment),
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
                return None;
            }
        }
        Some(path_segments)
    }

    fn add_fact_with_overrides(
        &mut self,
        fact: &'a LemmaFact,
        current_segments: &[PathSegment],
        pending_overrides: &HashMap<String, Vec<(&'a LemmaFact, usize)>>,
        current_doc: &'a LemmaDoc,
        type_registry: &mut TypeRegistry,
    ) {
        // Skip override facts - they are applied when the original fact is processed
        // The override's value will be used instead of the original fact's value
        // Don't build nested documents here - that happens when the base fact is processed
        if !fact.reference.segments.is_empty() {
            return;
        }

        let fact_path = FactPath {
            segments: current_segments.to_vec(),
            fact: fact.reference.fact.clone(),
        };

        // Check for duplicates
        if self.facts.contains_key(&fact_path) {
            self.errors.push(LemmaError::engine(
                format!("Duplicate fact '{}'", fact_path.fact),
                fact.source_location
                    .as_ref()
                    .map(|s| s.span.clone())
                    .unwrap_or(Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    }),
                fact.source_location
                    .as_ref()
                    .map(|s| s.attribute.as_str())
                    .unwrap_or("<unknown>"),
                fact.source_location
                    .as_ref()
                    .map(|s| Arc::from(s.doc_name.as_str()))
                    .unwrap_or_else(|| Arc::from("")),
                fact.source_location
                    .as_ref()
                    .map(|s| s.doc_name.as_str())
                    .unwrap_or("<unknown>"),
                1,
                None::<String>,
            ));
            return;
        }

        let current_depth = current_segments.len();

        match &fact.value {
            FactValue::Literal(_) => {
                // Check if there's an override for this literal fact
                let effective_value = if let Some(overrides) =
                    pending_overrides.get(&fact.reference.fact)
                {
                    // An override applies when we've traversed all its segments from the entry point
                    // entry_depth + segments.len() == current_depth
                    if let Some((override_fact, _)) = overrides.iter().find(|(o, entry_depth)| {
                        *entry_depth + o.reference.segments.len() == current_depth
                            && o.reference.fact == fact.reference.fact
                    }) {
                        override_fact.value.clone()
                    } else {
                        fact.value.clone()
                    }
                } else {
                    fact.value.clone()
                };

                let stored_fact = LemmaFact {
                    reference: fact.reference.clone(),
                    value: effective_value,
                    source_location: fact.source_location.clone(),
                };
                self.facts.insert(fact_path, stored_fact);
            }
            FactValue::TypeDeclaration {
                base,
                overrides: inline_overrides,
                from,
            } => {
                // Only register as inline type definition if we have 'from' OR 'overrides'
                // If both are None, it's just a direct type reference [coffee], not an inline type definition
                let is_inline_type_definition = from.is_some() || inline_overrides.is_some();

                // Only register inline type definitions when processing the document directly,
                // not when processing it as a nested reference. This prevents duplicate registrations
                // and ensures literal overrides don't trigger type definition registration.
                if is_inline_type_definition && current_segments.is_empty() {
                    // Register inline type definition in TypeRegistry
                    // Create a TypeDef for this inline type definition
                    let inline_type_def = TypeDef::Inline {
                        parent: base.clone(),
                        overrides: inline_overrides.clone(),
                        fact_ref: fact.reference.clone(),
                        from: from.clone(),
                    };

                    // Register in the current document
                    let doc_name = current_doc.name.clone();

                    // Register the inline type definition
                    if let Err(e) = type_registry.register_type(&doc_name, inline_type_def) {
                        self.errors.push(e);
                    }
                }

                // Check if there's an override for this type fact
                let effective_value = if let Some(overrides) =
                    pending_overrides.get(&fact.reference.fact)
                {
                    // An override applies when we've traversed all its segments from the entry point
                    // entry_depth + segments.len() == current_depth
                    if let Some((override_fact, _)) = overrides.iter().find(|(o, entry_depth)| {
                        *entry_depth + o.reference.segments.len() == current_depth
                            && o.reference.fact == fact.reference.fact
                    }) {
                        override_fact.value.clone()
                    } else {
                        fact.value.clone()
                    }
                } else {
                    fact.value.clone()
                };

                let stored_fact = LemmaFact {
                    reference: fact.reference.clone(),
                    value: effective_value,
                    source_location: fact.source_location.clone(),
                };
                self.facts.insert(fact_path, stored_fact);
            }
            FactValue::DocumentReference(doc_name) => {
                // Check if there's an override for this document reference
                let effective_doc_name = if let Some(overrides) =
                    pending_overrides.get(&fact.reference.fact)
                {
                    // An override applies when we've traversed all its segments from the entry point
                    if let Some((override_fact, _)) = overrides.iter().find(|(o, entry_depth)| {
                        *entry_depth + o.reference.segments.len() == current_depth
                            && o.reference.fact == fact.reference.fact
                    }) {
                        if let FactValue::DocumentReference(override_doc) = &override_fact.value {
                            override_doc.clone()
                        } else {
                            doc_name.clone()
                        }
                    } else {
                        doc_name.clone()
                    }
                } else {
                    doc_name.clone()
                };

                let nested_doc = match self.all_docs.get(&effective_doc_name) {
                    Some(d) => d,
                    None => {
                        self.errors.push(LemmaError::engine(
                            format!("Document '{}' not found", effective_doc_name),
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
                        return;
                    }
                };

                // Store the fact with the effective document reference
                let stored_fact = LemmaFact {
                    reference: fact.reference.clone(),
                    value: FactValue::DocumentReference(effective_doc_name.clone()),
                    source_location: fact.source_location.clone(),
                };
                self.facts.insert(fact_path.clone(), stored_fact);

                // Collect overrides for the nested document
                // Each override is (fact, entry_depth) where entry_depth is when it was added
                // Key by the next segment or fact name
                let nested_overrides: HashMap<String, Vec<(&LemmaFact, usize)>> = pending_overrides
                    .get(&fact.reference.fact)
                    .map(|overrides| {
                        let mut nested: HashMap<String, Vec<(&LemmaFact, usize)>> = HashMap::new();
                        for (o, entry_depth) in overrides {
                            // Calculate how many segments we've traversed from entry point
                            let traversed = current_depth - entry_depth;
                            let next_index = traversed + 1;
                            let key = if o.reference.segments.len() > next_index {
                                o.reference.segments[next_index].clone()
                            } else {
                                o.reference.fact.clone()
                            };
                            nested.entry(key).or_default().push((*o, *entry_depth));
                        }
                        nested
                    })
                    .unwrap_or_default();

                // Build nested document with the effective document
                let mut nested_segments = current_segments.to_vec();
                nested_segments.push(PathSegment {
                    fact: fact.reference.fact.clone(),
                    doc: effective_doc_name.clone(),
                });

                if let Err(errs) = self.build_document_with_overrides(
                    nested_doc,
                    nested_segments,
                    nested_overrides,
                    type_registry,
                ) {
                    self.errors.extend(errs);
                }
            }
        }
    }

    fn build_document_with_overrides(
        &mut self,
        doc: &'a LemmaDoc,
        current_segments: Vec<PathSegment>,
        override_map: HashMap<String, Vec<(&'a LemmaFact, usize)>>,
        type_registry: &mut TypeRegistry,
    ) -> Result<(), Vec<LemmaError>> {
        // Merge overrides with additional pending overrides from this document
        // New overrides from this doc get entry_depth = current_segments.len()
        let current_depth = current_segments.len();
        let mut pending_overrides = override_map;
        for fact in &doc.facts {
            if !fact.reference.segments.is_empty() {
                let first_segment = &fact.reference.segments[0];
                pending_overrides
                    .entry(first_segment.clone())
                    .or_default()
                    .push((fact, current_depth));
            }
        }

        // Build effective_facts_map with overridden values
        // Key: fact name, Value: effective document name (for document references)
        let mut effective_doc_refs: HashMap<String, String> = HashMap::new();
        for fact in doc.facts.iter() {
            if fact.reference.segments.is_empty() {
                if let FactValue::DocumentReference(doc_name) = &fact.value {
                    // Check if there's an override for this fact
                    // Override applies when entry_depth + segments.len() == current_depth
                    let effective_doc = if let Some(overrides) =
                        pending_overrides.get(&fact.reference.fact)
                    {
                        if let Some((override_fact, _)) =
                            overrides.iter().find(|(o, entry_depth)| {
                                *entry_depth + o.reference.segments.len() == current_depth
                                    && o.reference.fact == fact.reference.fact
                            })
                        {
                            if let FactValue::DocumentReference(override_doc) = &override_fact.value
                            {
                                override_doc.clone()
                            } else {
                                doc_name.clone()
                            }
                        } else {
                            doc_name.clone()
                        }
                    } else {
                        doc_name.clone()
                    };
                    effective_doc_refs.insert(fact.reference.fact.clone(), effective_doc);
                }
            }
        }

        // Original facts_map for basic lookups
        let facts_map: HashMap<String, &LemmaFact> = doc
            .facts
            .iter()
            .map(|fact| (fact.reference.fact.clone(), fact))
            .collect();

        for fact in &doc.facts {
            self.add_fact_with_overrides(
                fact,
                &current_segments,
                &pending_overrides,
                doc,
                type_registry,
            );
        }

        // Resolve types for this document after all facts are registered
        match type_registry.resolve_types(&doc.name) {
            Ok(document_types) => {
                // Validate type specifications for inline type definitions
                for (fact_ref, lemma_type) in &document_types.inline_type_definitions {
                    let type_name = format!("{} (inline)", fact_ref.fact);
                    let mut spec_errors =
                        validate_type_specifications(&lemma_type.specifications, &type_name);
                    self.errors.append(&mut spec_errors);
                }
                // Always overwrite: inline type definitions may have been registered while processing facts.
                self.resolved_types.insert(doc.name.clone(), document_types);
            }
            Err(e) => {
                self.errors.push(e);
                return Err(self.errors.clone());
            }
        }

        // Process all rules (now has access to resolved types)
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
            self.errors.push(LemmaError::engine(
                format!("Duplicate rule '{}'", rule_path.rule),
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
            source: rule.source_location.clone().unwrap_or_else(|| {
                Source::new(
                    "",
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 0,
                        col: 0,
                    },
                    "",
                )
            }),
            depends_on_rules,
            rule_type: LemmaType::veto_type(), // Initialized to veto_type; actual type computed in compute_all_rule_types during validation
        };

        self.rules.insert(rule_path, rule_node);
    }

    #[allow(clippy::too_many_arguments)]
    fn convert_binary_operands(
        &mut self,
        left: &Expression,
        right: &Expression,
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

    fn convert_expression_and_extract_dependencies(
        &mut self,
        expr: &Expression,
        current_doc: &'a LemmaDoc,
        facts_map: &HashMap<String, &'a LemmaFact>,
        current_segments: &[PathSegment],
        depends_on_rules: &mut HashSet<RulePath>,
        effective_doc_refs: &HashMap<String, String>,
    ) -> Option<Expression> {
        match &expr.kind {
            ExpressionKind::Reference(r) => {
                // Convert Reference to FactReference and recurse
                let fact_ref_expr = Expression {
                    kind: ExpressionKind::FactReference(r.to_fact_reference()),
                    source_location: expr.source_location.clone(),
                };
                self.convert_expression_and_extract_dependencies(
                    &fact_ref_expr,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )
            }
            ExpressionKind::UnresolvedUnitLiteral(number, unit_name) => {
                // Get resolved types for current document from self.resolved_types
                // Types must be resolved by this point (after facts, before rules)
                // Even empty documents get resolved types (with empty maps) - so get() should never fail
                let document_types = self.resolved_types.get(&current_doc.name).unwrap_or_else(|| {
                    unreachable!(
                        "Internal error: resolved types not found for document '{}' - types should have been resolved before processing rules (even empty documents have resolved types with empty maps)",
                        current_doc.name
                    )
                });

                // Lookup unit in unit_index
                let lemma_type = match document_types.unit_index.get(unit_name) {
                    Some(lemma_type) => lemma_type.clone(),
                    None => {
                        self.errors.push(LemmaError::engine(
                            format!(
                                "Unknown unit '{}' in document '{}'",
                                unit_name, current_doc.name
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
                        return None;
                    }
                };

                match &lemma_type.specifications {
                    TypeSpecification::Scale { units, .. } => {
                        if units
                            .iter()
                            .all(|unit| !unit.name.eq_ignore_ascii_case(unit_name))
                        {
                            unreachable!(
                                "Internal error: unit_index returned type '{}' that doesn't have unit '{}'",
                                lemma_type.name.as_ref().unwrap_or(&"<inline>".to_string()),
                                unit_name
                            );
                        }

                        let literal_value = LiteralValue::scale_with_type(
                            *number,
                            Some(unit_name.clone()), // Store the unit name with the value
                            lemma_type.clone(),
                        );
                        Some(Expression {
                            kind: ExpressionKind::Literal(literal_value),
                            source_location: expr.source_location.clone(),
                        })
                    }
                    TypeSpecification::Ratio { units, .. } => {
                        if units
                            .iter()
                            .all(|unit| !unit.name.eq_ignore_ascii_case(unit_name))
                        {
                            unreachable!(
                                "Internal error: unit_index returned type '{}' that doesn't have unit '{}'",
                                lemma_type.name.as_ref().unwrap_or(&"<inline>".to_string()),
                                unit_name
                            );
                        }

                        let literal_value = LiteralValue::ratio_with_type(
                            *number,
                            Some(unit_name.clone()), // Store the unit name with the value
                            lemma_type.clone(),
                        );
                        Some(Expression {
                            kind: ExpressionKind::Literal(literal_value),
                            source_location: expr.source_location.clone(),
                        })
                    }
                    _ => {
                        unreachable!(
                            "Internal error: unit_index returned non-Number/Ratio type '{}' for unit '{}'",
                            lemma_type.name.as_ref().unwrap_or(&"<inline>".to_string()),
                            unit_name
                        );
                    }
                }
            }
            ExpressionKind::FactReference(fact_ref) => {
                let segments = self.resolve_path_segments_with_overrides(
                    &fact_ref.segments,
                    facts_map.clone(),
                    current_segments.to_vec(),
                    effective_doc_refs,
                )?;

                // Validate that the referenced fact exists
                // For local facts (no segments), check current facts_map
                // For cross-document facts, the path segments validation already happened
                if fact_ref.segments.is_empty() && !facts_map.contains_key(&fact_ref.fact) {
                    // Check if this is actually a rule name - provide helpful error message
                    let is_rule = current_doc.rules.iter().any(|r| r.name == fact_ref.fact);
                    if is_rule {
                        self.errors.push(LemmaError::engine(
                            format!(
                                "'{}' is a rule, not a fact. Use '{}?' to reference rules",
                                fact_ref.fact, fact_ref.fact
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
                    } else {
                        self.errors.push(LemmaError::engine(
                            format!("Fact '{}' not found", fact_ref.fact),
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
                    return None;
                }

                let fact_path = FactPath {
                    segments,
                    fact: fact_ref.fact.clone(),
                };

                Some(Expression {
                    kind: ExpressionKind::FactPath(fact_path),
                    source_location: expr.source_location.clone(),
                })
            }

            ExpressionKind::RuleReference(rule_ref) => {
                let segments = self.resolve_path_segments_with_overrides(
                    &rule_ref.segments,
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

            ExpressionKind::LogicalAnd(left, right) => {
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

            ExpressionKind::LogicalOr(left, right) => {
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

            ExpressionKind::Arithmetic(left, op, right) => {
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

            ExpressionKind::Comparison(left, op, right) => {
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

            ExpressionKind::UnitConversion(value, target) => {
                let converted_value = self.convert_expression_and_extract_dependencies(
                    value,
                    current_doc,
                    facts_map,
                    current_segments,
                    depends_on_rules,
                    effective_doc_refs,
                )?;

                Some(Expression {
                    kind: ExpressionKind::UnitConversion(Arc::new(converted_value), target.clone()),
                    source_location: expr.source_location.clone(),
                })
            }

            ExpressionKind::LogicalNegation(operand, neg_type) => {
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

            ExpressionKind::MathematicalComputation(op, operand) => {
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

            ExpressionKind::FactPath(_) => Some(expr.clone()),
            ExpressionKind::RulePath(rule_path) => {
                depends_on_rules.insert(rule_path.clone());
                Some(expr.clone())
            }

            ExpressionKind::Literal(_) => Some(expr.clone()),

            ExpressionKind::Veto(_) => Some(expr.clone()),
        }
    }
}

fn compute_all_rule_types(
    graph: &mut Graph,
    execution_order: &[RulePath],
    errors: &mut Vec<LemmaError>,
) {
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
        let default_type = compute_expression_type(default_result, graph, &computed_types, errors);

        // Collect all non-Veto types from branches
        // Veto is a runtime exception, not a type that should affect the rule's type
        // If a branch returns Veto, it's handled at runtime, but the rule type is the non-Veto type
        let mut non_veto_type: Option<LemmaType> = None;
        if !default_type.is_veto() {
            non_veto_type = Some(default_type.clone());
        }

        for (branch_index, (condition, result)) in branches.iter().enumerate().skip(1) {
            if let Some(condition_expression) = condition {
                let condition_type =
                    compute_expression_type(condition_expression, graph, &computed_types, errors);
                if !condition_type.is_boolean() {
                    errors.push(LemmaError::engine(
                        format!(
                            "Unless clause condition in rule '{}' must be boolean, got {:?}",
                            rule_path.rule, condition_type
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

            let result_type = compute_expression_type(result, graph, &computed_types, errors);
            if !result_type.is_veto() {
                // If we haven't seen a non-Veto type yet, store it
                // All non-Veto branches must have the same standard type (enforced by validate_branch_type_consistency)
                if non_veto_type.is_none() {
                    non_veto_type = Some(result_type.clone());
                } else if let Some(ref existing_type) = non_veto_type {
                    // Check that this branch has the same standard type as the first non-veto type
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
                            format!("Type mismatch in rule '{}' in document '{}' ({}): default branch returns {}, but unless clause {} returns {}. All branches must return the same standard type.",
                            rule_path.rule,
                            rule_source.doc_name,
                            location_parts.join(", "),
                            existing_type.name(),
                            branch_index,
                            result_type.name()),
                            rule_source.span.clone(),
                            rule_source.attribute.clone(),
                            std::sync::Arc::from(""),
                            rule_source.doc_name.clone(),
                            1,
                            None::<String>,
                        ));
                    }
                }
            }

            if !default_type.has_same_base_type(&result_type)
                && !default_type.is_veto()
                && !result_type.is_veto()
            {
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
                    format!("Type mismatch in rule '{}' in document '{}' ({}): default branch returns {}, but unless clause {} returns {}. All branches must return the same standard type.",
                    rule_path.rule,
                    rule_source.doc_name,
                    location_parts.join(", "),
                    default_type.name(),
                    branch_index,
                    result_type.name()),
                    rule_source.span.clone(),
                    rule_source.attribute.clone(),
                    std::sync::Arc::from(""),
                    rule_source.doc_name.clone(),
                    1,
                    None::<String>,
                ));
            }
        }

        // Every rule MUST have a type (Lemma is strictly typed)
        // If all branches return Veto, the rule type is Veto
        // Otherwise, use the first non-Veto type (typically the default branch)
        // All non-Veto branches must have the same type (enforced by validate_branch_type_consistency)
        let rule_type = non_veto_type.unwrap_or_else(LemmaType::veto_type);
        computed_types.insert(rule_path.clone(), rule_type);
    }

    for (rule_path, rule_type) in computed_types {
        if let Some(rule_node) = graph.rules_mut().get_mut(&rule_path) {
            rule_node.rule_type = rule_type;
        }
    }
}

fn compute_expression_type(
    expression: &Expression,
    graph: &Graph,
    computed_rule_types: &HashMap<RulePath, LemmaType>,
    errors: &mut Vec<LemmaError>,
) -> LemmaType {
    match &expression.kind {
        ExpressionKind::Literal(literal_value) => literal_value.get_type().clone(),
        ExpressionKind::FactPath(fact_path) => {
            compute_fact_type(fact_path, graph, computed_rule_types, errors)
        }
        ExpressionKind::RulePath(rule_path) => computed_rule_types
            .get(rule_path)
            .cloned()
            .unwrap_or_else(|| {
                unreachable!(
                    "BUG: Rule '{}' referenced before its type was computed (topological ordering)",
                    rule_path.rule
                )
            }),
        ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
            let left_type = compute_expression_type(left, graph, computed_rule_types, errors);
            let right_type = compute_expression_type(right, graph, computed_rule_types, errors);
            validate_logical_operands(&left_type, &right_type, errors);
            standard_boolean().clone()
        }
        ExpressionKind::LogicalNegation(operand, _) => {
            let operand_type = compute_expression_type(operand, graph, computed_rule_types, errors);
            validate_logical_operand(&operand_type, errors);
            standard_boolean().clone()
        }
        ExpressionKind::Comparison(left, _, right) => {
            let left_type = compute_expression_type(left, graph, computed_rule_types, errors);
            let right_type = compute_expression_type(right, graph, computed_rule_types, errors);
            validate_comparison_types(&left_type, &right_type, errors);
            standard_boolean().clone()
        }
        ExpressionKind::Arithmetic(left, operator, right) => {
            let left_type = compute_expression_type(left, graph, computed_rule_types, errors);
            let right_type = compute_expression_type(right, graph, computed_rule_types, errors);
            validate_arithmetic_types(&left_type, &right_type, operator, errors);
            compute_arithmetic_result_type(left_type, right_type, operator)
        }
        ExpressionKind::UnitConversion(source_expression, target) => {
            let source_type =
                compute_expression_type(source_expression, graph, computed_rule_types, errors);
            validate_unit_conversion_types(&source_type, target, errors);
            conversion_target_to_type(target)
        }
        ExpressionKind::MathematicalComputation(_, operand) => {
            let operand_type = compute_expression_type(operand, graph, computed_rule_types, errors);
            validate_mathematical_operand(&operand_type, errors);
            standard_number().clone()
        }
        ExpressionKind::Veto(_) => LemmaType::veto_type(),
        ExpressionKind::Reference(_)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::RuleReference(_) => {
            unreachable!("Internal error: Reference/FactReference/RuleReference should be converted during graph building");
        }
        ExpressionKind::UnresolvedUnitLiteral(_, _) => {
            unreachable!(
                "UnresolvedUnitLiteral found during type computation - this is a bug: unresolved units should be resolved during graph building in convert_expression_and_extract_dependencies"
            );
        }
    }
}

fn validate_logical_operands(
    left_type: &LemmaType,
    right_type: &LemmaType,
    errors: &mut Vec<LemmaError>,
) {
    if !left_type.is_boolean() {
        errors.push(LemmaError::engine(
            format!(
                "Logical operation requires boolean operands, got {:?} for left operand",
                left_type
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
    if !right_type.is_boolean() {
        errors.push(LemmaError::engine(
            format!(
                "Logical operation requires boolean operands, got {:?} for right operand",
                right_type
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

fn validate_logical_operand(operand_type: &LemmaType, errors: &mut Vec<LemmaError>) {
    if !operand_type.is_boolean() {
        errors.push(LemmaError::engine(
            format!(
                "Logical negation requires boolean operand, got {:?}",
                operand_type
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

fn validate_comparison_types(
    left_type: &LemmaType,
    right_type: &LemmaType,
    errors: &mut Vec<LemmaError>,
) {
    if left_type == right_type {
        return;
    }

    // CRITICAL: If both operands are different Scale types, reject ALL comparisons
    if left_type.is_scale() && right_type.is_scale() && left_type.name != right_type.name {
        errors.push(LemmaError::engine(
            format!("Cannot compare different scale types: {} and {}. Operations between different scale types produce ambiguous result units.", left_type.name(), right_type.name()),
            Span { start: 0, end: 0, line: 1, col: 0 },
            "<unknown>",
            Arc::from(""),
            "<unknown>",
            1,
            None::<String>,
        ));
        return;
    }

    // Allow comparison between compatible numeric types (Scale, Number, Ratio, Duration)
    // Scale and Number are both numeric and can be compared with each other
    // But different Scale types are rejected above
    if (left_type.is_scale()
        || left_type.is_number()
        || left_type.is_duration()
        || left_type.is_ratio())
        && (right_type.is_scale()
            || right_type.is_number()
            || right_type.is_duration()
            || right_type.is_ratio())
    {
        return;
    }
    // Allow comparison between text types (including inline type definitions with their base type)
    // Inline type definitions extending a base text type are comparable with that base type
    // Options validation happens at runtime, not during type checking
    if left_type.is_text() && right_type.is_text() {
        return;
    }
    errors.push(LemmaError::engine(
        format!("Cannot compare {:?} with {:?}", left_type, right_type),
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

fn validate_arithmetic_types(
    left_type: &LemmaType,
    right_type: &LemmaType,
    operator: &ArithmeticComputation,
    errors: &mut Vec<LemmaError>,
) {
    // Check for temporal arithmetic (Date/Time)
    if left_type.is_date() || left_type.is_time() || right_type.is_date() || right_type.is_time() {
        // Validate temporal arithmetic is supported
        // compute_temporal_arithmetic_result_type will return a fallback if unsupported
        // but we check here to provide a better error message
        let result = compute_temporal_arithmetic_result_type(left_type, right_type, operator);
        // If result is duration but operator is not Subtract/Add, it's invalid
        if result.is_duration()
            && !matches!(
                operator,
                ArithmeticComputation::Subtract | ArithmeticComputation::Add
            )
        {
            errors.push(LemmaError::engine(
                format!(
                    "Invalid date/time arithmetic: {:?} {:?} {:?}",
                    left_type, operator, right_type
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
        return;
    }

    // CRITICAL: If both operands are different Scale types, reject ALL arithmetic operations
    if left_type.is_scale() && right_type.is_scale() && left_type.name != right_type.name {
        errors.push(LemmaError::engine(
            format!("Cannot {} different scale types: {} and {}. Operations between different scale types produce ambiguous result units.",
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
            Span { start: 0, end: 0, line: 1, col: 0 },
            "<unknown>",
            Arc::from(""),
            "<unknown>",
            1,
            None::<String>,
        ));
        return;
    }

    // Check for valid arithmetic type combinations
    // Scale, Number, Ratio, and Duration can participate in arithmetic
    // but with specific constraints handled in validate_arithmetic_operator_constraints
    let left_valid = left_type.is_scale()
        || left_type.is_number()
        || left_type.is_duration()
        || left_type.is_ratio();
    let right_valid = right_type.is_scale()
        || right_type.is_number()
        || right_type.is_duration()
        || right_type.is_ratio();

    if !left_valid {
        errors.push(LemmaError::engine(
            format!(
                "Arithmetic operation requires numeric operands, got {:?} for left operand",
                left_type
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
        return;
    }
    if !right_valid {
        errors.push(LemmaError::engine(
            format!(
                "Arithmetic operation requires numeric operands, got {:?} for right operand",
                right_type
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
        return;
    }

    validate_arithmetic_operator_constraints(left_type, right_type, operator, errors);
}

fn validate_arithmetic_operator_constraints(
    left_type: &LemmaType,
    right_type: &LemmaType,
    operator: &ArithmeticComputation,
    errors: &mut Vec<LemmaError>,
) {
    match operator {
        ArithmeticComputation::Modulo => {
            if left_type.is_duration() || right_type.is_duration() {
                errors.push(LemmaError::engine(
                    format!(
                        "Modulo operation not supported for duration types: {:?} % {:?}",
                        left_type, right_type
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
            } else if !right_type.is_number() {
                // Modulo: dividend % divisor
                // Dividend can be Scale or Number (custom or standard)
                // Divisor must be Number (dimensionless, not Scale)
                // Allow: Scale % Number → result is Scale
                // Allow: Number % Number → result is Number
                // Error: Scale % Scale (divisor must be dimensionless)
                // Error: Number % Scale (divisor must be dimensionless)
                errors.push(LemmaError::engine(
                    format!(
                        "Modulo divisor must be a dimensionless number (not a scale type), got {}",
                        right_type.name()
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
            // If right is Number, allow it (left can be Scale or Number)
        }
        ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {
            // Multiply/Divide: Different Scale types are already rejected in validate_arithmetic_types
            // At this point, if both are Scale, they must be the same Scale type

            // - Same standard type: allowed (Number * Number, Scale * Scale, Ratio * Ratio, etc.)
            // - Scale * Number, Number * Scale: allowed
            // - Scale * Ratio, Ratio * Scale: allowed
            // - Number * Ratio, Ratio * Number: allowed
            // - Duration * Number: allowed (Multiply only)
            // - Number * Duration: allowed (Multiply only)
            // - Duration / Number: allowed (Divide only)
            // - Number / Duration: NOT allowed

            if !left_type.has_same_base_type(right_type) {
                // Check if Scale * Number or Number * Scale (allowed)
                let is_scale_number = (left_type.is_scale() && right_type.is_number())
                    || (left_type.is_number() && right_type.is_scale());

                // Check if Scale * Ratio or Ratio * Scale (allowed)
                let is_scale_ratio = (left_type.is_scale() && right_type.is_ratio())
                    || (left_type.is_ratio() && right_type.is_scale());

                // Check if Number * Ratio or Ratio * Number (allowed)
                let is_number_ratio = (left_type.is_number() && right_type.is_ratio())
                    || (left_type.is_ratio() && right_type.is_number());

                // Check Duration combinations
                let is_duration_number = (left_type.is_duration() && right_type.is_number())
                    || (left_type.is_number() && right_type.is_duration());

                if is_duration_number {
                    // Duration * Number or Number * Duration: only Multiply is allowed
                    // Duration / Number: only Divide is allowed (when Duration is left)
                    // Number / Duration: NOT allowed
                    if matches!(operator, ArithmeticComputation::Divide)
                        && left_type.is_number()
                        && right_type.is_duration()
                    {
                        errors.push(LemmaError::engine(
                            "Cannot divide number by duration. Duration can only be multiplied by number or divided by number.".to_string(),
                            Span { start: 0, end: 0, line: 1, col: 0 },
                            "<unknown>",
                            Arc::from(""),
                            "<unknown>",
                            1,
                            None::<String>,
                        ));
                    }
                    // Otherwise, Duration * Number or Number * Duration (Multiply) or Duration / Number (Divide) are allowed
                } else if !is_scale_number && !is_scale_ratio && !is_number_ratio {
                    // Not the special case - types are incompatible
                    errors.push(LemmaError::engine(
                        format!(
                            "Cannot apply '{}' to values with different types: {} and {}. '*'/'/' require the same standard type, scale * number (or number * scale), scale * ratio (or ratio * scale), number * ratio (or ratio * number), or duration * number (or number * duration) for multiply, or duration / number for divide.",
                            operator,
                            left_type.name(),
                            right_type.name()
                        ),
                        Span { start: 0, end: 0, line: 1, col: 0 },
                        "<unknown>",
                        Arc::from(""),
                        "<unknown>",
                        1,
                        None::<String>,
                    ));
                }
            } else {
                // Types have the same standard type - always allowed (even with different constraints)
            }
        }
        ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
            // Different Scale types are already rejected in validate_arithmetic_types
            // At this point, if both are Scale, they must be the same Scale type

            // - Same standard type: allowed (Number + Number, Scale + Scale, etc.) - even with different constraints
            // - Scale + Number: allowed (result is Scale)
            // - Number + Scale: allowed (result is Scale)
            // - Number + Ratio: allowed (result is Number with ratio semantics)
            // - Scale + Ratio: allowed (result is Scale with ratio semantics)
            if !left_type.has_same_base_type(right_type) {
                // Check if Scale + Number or Number + Scale (allowed)
                let is_scale_number = (left_type.is_scale() && right_type.is_number())
                    || (left_type.is_number() && right_type.is_scale());

                // Check if Scale op Ratio or Ratio op Scale (allowed)
                let is_scale_ratio = (left_type.is_scale() && right_type.is_ratio())
                    || (left_type.is_ratio() && right_type.is_scale());

                // Check if Number op Ratio or Ratio op Number (allowed with ratio semantics)
                let is_number_ratio = (left_type.is_number() && right_type.is_ratio())
                    || (left_type.is_ratio() && right_type.is_number());

                if !is_scale_number && !is_scale_ratio && !is_number_ratio {
                    // Not the special case - types are incompatible
                    errors.push(LemmaError::engine(
                        format!(
                            "Cannot apply '{}' to values with different types: {} and {}. '+'/'-' require the same standard type, scale + number (or number + scale), scale + ratio (or ratio + scale), or number + ratio (or ratio + number).",
                            operator,
                            left_type.name(),
                            right_type.name()
                        ),
                        Span { start: 0, end: 0, line: 1, col: 0 },
                        "<unknown>",
                        Arc::from(""),
                        "<unknown>",
                        1,
                        None::<String>,
                        ));
                }
            } else {
                // Types have the same standard type - always allowed (even with different constraints)
            }
        }
        ArithmeticComputation::Power => {
            // Power: base ^ exponent
            // Base can be Scale or Number (custom or standard)
            // Exponent must be Number or Ratio (dimensionless, not Scale)
            // Allow: Scale ^ Number → result is Scale
            // Allow: Number ^ Number → result is Number
            // Error: Scale ^ Scale (exponent must be dimensionless)
            // Error: Number ^ Scale (exponent must be dimensionless)
            if !right_type.is_number() && !right_type.is_ratio() {
                errors.push(LemmaError::engine(
                    format!(
                        "Power exponent must be a dimensionless number (not a scale type), got {}",
                        right_type.name()
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
            // If right is Number or Ratio, allow it (left can be Scale or Number)
        }
    }
}

fn validate_unit_conversion_types(
    source_type: &LemmaType,
    target: &ConversionTarget,
    errors: &mut Vec<LemmaError>,
) {
    let target_type = conversion_target_to_type(target);
    // Allow conversion from Scale/Number to compatible numeric types
    // Scale and Number are both numeric and can be converted to each other
    if source_type.specifications != target_type.specifications
        && !source_type.is_scale()
        && !source_type.is_number()
    {
        errors.push(LemmaError::engine(
            format!("Cannot convert {:?} to {:?}", source_type, target_type),
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

fn validate_mathematical_operand(operand_type: &LemmaType, errors: &mut Vec<LemmaError>) {
    // Mathematical functions work on Scale and Number (not Ratio or Duration)
    // Both Scale and Number are numeric types suitable for mathematical operations
    if !operand_type.is_scale() && !operand_type.is_number() {
        errors.push(LemmaError::engine(
            format!(
                "Mathematical function requires numeric operand (scale or number), got {:?}",
                operand_type
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

fn compute_fact_type(
    fact_path: &FactPath,
    graph: &Graph,
    _computed_rule_types: &HashMap<RulePath, LemmaType>,
    errors: &mut Vec<LemmaError>,
) -> LemmaType {
    let fact = match graph.facts().get(fact_path) {
        Some(fact) => fact,
        None => {
            let potential_rule_path = RulePath {
                segments: fact_path.segments.clone(),
                rule: fact_path.fact.clone(),
            };
            if graph.rules().contains_key(&potential_rule_path) {
                errors.push(LemmaError::engine(
                    format!(
                        "'{}' is a rule, not a fact. Use '{}?' to reference rules",
                        fact_path.fact, fact_path.fact
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
            } else {
                errors.push(LemmaError::engine(
                    format!("Fact '{}' not found", fact_path),
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
            return LemmaType::veto_type();
        }
    };
    match &fact.value {
        FactValue::Literal(literal_value) => literal_value.get_type().clone(),
        FactValue::TypeDeclaration { .. } => {
            // Use TypeRegistry to determine document context and resolve type
            let fact_ref = FactReference {
                segments: fact_path.segments.iter().map(|s| s.fact.clone()).collect(),
                fact: fact_path.fact.clone(),
            };

            // For inline type definitions, check if they exist in resolved_types
            // Inline type definitions are already fully resolved during type resolution, so just use them directly
            for (_doc_name, document_types) in graph.resolved_types.iter() {
                if let Some(resolved_type) = document_types.inline_type_definitions.get(&fact_ref) {
                    // Inline type definition already resolved - return it directly
                    return resolved_type.clone();
                }
            }

            // Find which document this fact belongs to
            // Use the document from the first segment (set during graph building)
            // This is more reliable than searching, especially for nested facts
            let context_doc: &str = if let Some(first_segment) = fact_path.segments.first() {
                // Use the document from the segment - this is set during graph building
                &first_segment.doc
            } else {
                // Top-level fact - try to find it by searching documents
                let fact_ref_segments: Vec<String> = vec![];
                let mut found_doc: Option<&str> = None;
                for (doc_name, doc) in graph.all_docs() {
                    for orig_fact in &doc.facts {
                        if orig_fact.reference.segments == fact_ref_segments
                            && orig_fact.reference.fact == fact_path.fact
                        {
                            found_doc = Some(doc_name);
                            break;
                        }
                    }
                    if found_doc.is_some() {
                        break;
                    }
                }
                // If not found by searching, use the document from the fact's source_location
                // This is reliable since facts are always added from a specific document
                if let Some(doc) = found_doc.or_else(|| {
                    fact.source_location
                        .as_ref()
                        .map(|src| src.doc_name.as_str())
                }) {
                    doc
                } else {
                    // This should not happen - all facts should have document context
                    // But if it does, return an error rather than panicking
                    errors.push(LemmaError::engine(
                        format!("Cannot determine document context for fact '{}'", fact_path),
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
                    return LemmaType::veto_type();
                }
            };

            // Use Graph::resolve_type_declaration which uses TypeRegistry
            // For direct type references [coffee] (from=None, overrides=None), this looks up the named type directly
            match graph.resolve_type_declaration(&fact.value, context_doc) {
                Ok(lemma_type) => {
                    // For direct type references, we should get the actual named type back
                    lemma_type
                }
                Err(e) => {
                    errors.push(e);
                    LemmaType::veto_type()
                }
            }
        }
        FactValue::DocumentReference(_) => {
            errors.push(LemmaError::engine(
                format!(
                    "Cannot compute type for document reference fact '{}'",
                    fact_path
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
            LemmaType::veto_type()
        }
    }
}

fn compute_arithmetic_result_type(
    left_type: LemmaType,
    right_type: LemmaType,
    operator: &ArithmeticComputation,
) -> LemmaType {
    let left = &left_type;
    let right = &right_type;

    if left.is_date() || left.is_time() || right.is_date() || right.is_time() {
        return compute_temporal_arithmetic_result_type(left, right, operator);
    }
    if left == right {
        return left_type;
    }

    // Handle Scale + Number or Number + Scale: result is Scale (Scale has units, Number doesn't)
    if left.is_scale() && right.is_number() {
        return left_type; // Scale + Number → Scale
    }
    if left.is_number() && right.is_scale() {
        return right_type; // Number + Scale → Scale
    }

    // Handle Ratio operations
    // Ratio op Number or Number op Ratio → Number
    if left.is_ratio() && right.is_number() {
        return standard_number().clone(); // Ratio op Number → Number
    }
    if left.is_number() && right.is_ratio() {
        return standard_number().clone(); // Number op Ratio → Number
    }
    // Ratio op Ratio → Ratio
    if left.is_ratio() && right.is_ratio() {
        return left_type; // Ratio op Ratio → Ratio (preserve Ratio type)
    }
    // Ratio op Scale or Scale op Ratio → Scale
    if left.is_ratio() && right.is_scale() {
        return right_type; // Ratio op Scale → Scale
    }
    if left.is_scale() && right.is_ratio() {
        return left_type; // Scale op Ratio → Scale
    }

    // Handle standard (no name) + custom (has name) case: result is the custom type
    // This handles: STANDARD_SCALE + custom_scale, STANDARD_NUMBER + custom_scale, etc.
    // ORDER DOES NOT MATTER for Add/Subtract/Multiply/Divide - both orders return the custom type
    // For Power/Modulo, validation ensures correct order (custom op standard)
    let one_is_standard_one_is_custom = left_type.name.is_none() != right_type.name.is_none();

    if one_is_standard_one_is_custom {
        // One is standard, one is custom → result is the custom type (order-independent)
        // Return whichever operand is the custom type (has a name)
        if left_type.name.is_some() {
            return left_type;
        } else {
            return right_type;
        }
    }

    // Both are numeric types, check if we can preserve custom type
    // If we reach here, validation should have ensured types are compatible
    if left.name.is_some() && right.name.is_some() {
        // Both are custom types
        // Different Scale types are already rejected in validate_arithmetic_types
        // But different custom Number types with same base are allowed
        // Return the left type (result type is left operand for same base operations)
        return left_type;
    }

    // Both are standard types (both name.is_none()) - determine result type
    // Scale op Scale (same type) → Scale
    // Number op Number → Number
    // Scale op Number → Scale (handled above)
    // Number op Scale → Scale (handled above)
    if left.is_scale() && right.is_scale() {
        // Both are Scale - they must be the same type (validation ensures this)
        return left_type;
    }
    if left.is_number() && right.is_number() {
        // Both are Number
        return standard_number().clone();
    }

    // Fallback (should not reach here if validation is correct)
    standard_number().clone()
}

fn compute_temporal_arithmetic_result_type(
    left: &LemmaType,
    right: &LemmaType,
    operator: &ArithmeticComputation,
) -> LemmaType {
    match operator {
        ArithmeticComputation::Subtract => {
            // Date - Date → Duration (supported)
            if left.is_date() && right.is_date() {
                return standard_duration().clone();
            }
            // Time - Time → Duration (supported)
            if left.is_time() && right.is_time() {
                return standard_duration().clone();
            }
            // Date - Time → Duration (supported: datetime - time = duration)
            if left.is_date() && right.is_time() {
                return standard_duration().clone();
            }
            // Time - Date → Duration (supported: time - datetime = duration)
            if left.is_time() && right.is_date() {
                return standard_duration().clone();
            }
            // Date - Duration → Date (supported)
            if left.is_date() && right.is_duration() {
                return left.clone();
            }
            // Time - Duration → Time (supported)
            if left.is_time() && right.is_duration() {
                return left.clone();
            }
        }
        ArithmeticComputation::Add => {
            // Date + Duration → Date (supported)
            if left.is_date() && right.is_duration() {
                return left.clone();
            }
            // Time + Duration → Time (supported)
            if left.is_time() && right.is_duration() {
                return left.clone();
            }
            // Duration + Date → Date (supported)
            if left.is_duration() && right.is_date() {
                return right.clone();
            }
            // Duration + Time → Time (supported)
            if left.is_duration() && right.is_time() {
                return right.clone();
            }
        }
        _ => {}
    }
    // Unsupported temporal arithmetic - validation should have caught this
    // Return fallback type (validation will fail due to errors vector)
    standard_duration().clone()
}

fn conversion_target_to_type(target: &ConversionTarget) -> LemmaType {
    match target {
        ConversionTarget::Duration(_) => standard_duration().clone(),
        ConversionTarget::Percentage => standard_ratio().clone(),
    }
}

fn validate_all_rule_references_exist(graph: &Graph, errors: &mut Vec<LemmaError>) {
    let existing_rules: HashSet<&RulePath> = graph.rules().keys().collect();
    for (rule_path, rule_node) in graph.rules() {
        for dependency in &rule_node.depends_on_rules {
            if !existing_rules.contains(dependency) {
                errors.push(LemmaError::engine(
                    format!(
                        "Rule '{}' references non-existent rule '{}'",
                        rule_path.rule, dependency.rule
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

fn validate_fact_override_paths_target_document_facts(graph: &Graph, errors: &mut Vec<LemmaError>) {
    // For any fact path like `a.b.c`, each segment (`a`, `a.b`, ...) must be a document reference.
    for (fact_path, _fact) in graph.facts() {
        if fact_path.segments.is_empty() {
            continue;
        }

        for i in 0..fact_path.segments.len() {
            let seg = &fact_path.segments[i];
            let prefix_segments: Vec<PathSegment> = fact_path.segments[..i].to_vec();
            let seg_fact_path = FactPath::new(prefix_segments, seg.fact.clone());

            match graph.facts().get(&seg_fact_path) {
                Some(seg_fact) => match &seg_fact.value {
                    FactValue::DocumentReference(_) => {}
                    _ => errors.push(LemmaError::engine(
                        format!(
                            "Invalid fact override path '{}': '{}' is not a document reference",
                            fact_path, seg_fact_path
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
                    )),
                },
                None => errors.push(LemmaError::engine(
                    format!(
                        "Invalid fact override path '{}': missing document reference '{}'",
                        fact_path, seg_fact_path
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
                )),
            }
        }
    }

    // Also validate *syntactic override facts* from source documents.
    //
    // GraphBuilder intentionally skips registering override facts (facts with reference.segments),
    // so they are not present in `graph.facts()`. However, they are still part of the language and
    // must be validated. We validate by traversing document references through the source docs:
    // for an override `x.y.z = ...`, each segment (`x`, then `y`) must be a document reference in
    // the document reached after traversing the previous segment.
    for doc in graph.all_docs.values() {
        for fact in &doc.facts {
            if fact.reference.segments.is_empty() {
                continue;
            }

            let mut current_doc_name = doc.name.clone();
            let mut prefix: Vec<String> = Vec::new();
            let mut path_valid = true;

            for seg in &fact.reference.segments {
                prefix.push(seg.clone());

                let current_doc = match graph.all_docs.get(&current_doc_name) {
                    Some(d) => d,
                    None => {
                        errors.push(LemmaError::engine(
                            format!(
                                "Invalid fact override path '{}.{}': document '{}' not found",
                                prefix.join("."),
                                fact.reference.fact,
                                current_doc_name
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
                        path_valid = false;
                        break;
                    }
                };

                let Some(seg_fact) = current_doc
                    .facts
                    .iter()
                    .find(|f| f.reference.segments.is_empty() && f.reference.fact == *seg)
                else {
                    errors.push(LemmaError::engine(
                        format!(
                            "Invalid fact override path '{}.{}': missing document reference '{}'",
                            prefix.join("."),
                            fact.reference.fact,
                            prefix.join(".")
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
                    path_valid = false;
                    break;
                };

                match &seg_fact.value {
                    FactValue::DocumentReference(next_doc) => {
                        current_doc_name = next_doc.clone();
                    }
                    _ => {
                        errors.push(LemmaError::engine(
                            format!(
                                "Invalid fact override path '{}.{}': '{}' is not a document reference",
                                prefix.join("."),
                                fact.reference.fact,
                                prefix.join(".")
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
                        path_valid = false;
                        break;
                    }
                }
            }

            // If path traversal succeeded, validate that we're not overriding a typed fact with a type definition
            if path_valid {
                if let Some(target_doc) = graph.all_docs.get(&current_doc_name) {
                    if let Some(original_fact) = target_doc.facts.iter().find(|f| {
                        f.reference.segments.is_empty() && f.reference.fact == fact.reference.fact
                    }) {
                        // Check if both original and override are type declarations
                        if matches!(&original_fact.value, FactValue::TypeDeclaration { .. })
                            && matches!(&fact.value, FactValue::TypeDeclaration { .. })
                        {
                            let override_path = if fact.reference.segments.is_empty() {
                                fact.reference.fact.clone()
                            } else {
                                format!(
                                    "{}.{}",
                                    fact.reference.segments.join("."),
                                    fact.reference.fact
                                )
                            };
                            errors.push(LemmaError::engine(
                                format!(
                                    "Cannot override typed fact '{}' with type definition. Use a concrete value instead.",
                                    override_path
                                ),
                                fact.source_location
                                    .as_ref()
                                    .map(|s| s.span.clone())
                                    .unwrap_or(Span {
                                        start: 0,
                                        end: 0,
                                        line: 1,
                                        col: 0,
                                    }),
                                fact.source_location
                                    .as_ref()
                                    .map(|s| s.attribute.as_str())
                                    .unwrap_or("<unknown>"),
                                fact.source_location
                                    .as_ref()
                                    .map(|s| Arc::from(s.doc_name.as_str()))
                                    .unwrap_or_else(|| Arc::from("")),
                                fact.source_location
                                    .as_ref()
                                    .map(|s| s.doc_name.as_str())
                                    .unwrap_or("<unknown>"),
                                1,
                                None::<String>,
                            ));
                        }
                    }
                }
            }
        }
    }
}

fn validate_fact_and_rule_name_collisions(graph: &Graph, errors: &mut Vec<LemmaError>) {
    // Disallow fact/rule name collision in the same namespace (same traversal segments).
    for rule_path in graph.rules().keys() {
        let fact_path = FactPath::new(rule_path.segments.clone(), rule_path.rule.clone());
        if graph.facts().contains_key(&fact_path) {
            errors.push(LemmaError::engine(
                format!(
                    "Name collision: '{}' is defined as both a fact and a rule",
                    fact_path
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

fn validate_document_interfaces(
    graph: &Graph,
    all_docs: &[LemmaDoc],
    errors: &mut Vec<LemmaError>,
) {
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
    for (fact_path, fact) in graph.facts() {
        if let FactValue::DocumentReference(doc_name) = &fact.value {
            let mut full_path: Vec<String> = fact_path
                .segments
                .iter()
                .map(|segment| segment.fact.clone())
                .collect();
            full_path.push(fact_path.fact.clone());
            if let Some(required_rules) = referenced_rules.get(&full_path) {
                let doc = match all_docs.iter().find(|document| document.name == *doc_name) {
                    Some(document) => document,
                    None => continue,
                };
                let doc_rule_names: HashSet<String> =
                    doc.rules.iter().map(|rule| rule.name.clone()).collect();
                for required_rule in required_rules {
                    if !doc_rule_names.contains(required_rule) {
                        errors.push(LemmaError::engine(
                            format!(
                                "Document '{}' referenced by '{}' is missing required rule '{}'",
                                doc_name, fact_path, required_rule
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::semantic::{FactReference, LiteralValue, RuleReference};

    fn create_test_doc(name: &str) -> LemmaDoc {
        LemmaDoc::new(name.to_string())
    }

    fn create_literal_fact(name: &str, value: LiteralValue) -> LemmaFact {
        LemmaFact {
            reference: FactReference {
                segments: Vec::new(),
                fact: name.to_string(),
            },
            value: FactValue::Literal(value),
            source_location: None,
        }
    }

    fn create_literal_expr(value: LiteralValue) -> Expression {
        Expression {
            kind: ExpressionKind::Literal(value),
            source_location: None,
        }
    }

    #[test]
    fn test_build_simple_graph() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact(
            "age",
            LiteralValue::number(rust_decimal::Decimal::from(25)),
        ));
        doc = doc.add_fact(create_literal_fact(
            "name",
            LiteralValue::text("John".to_string()),
        ));

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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
            LiteralValue::number(rust_decimal::Decimal::from(25)),
        ));

        let age_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "age".to_string(),
            }),
            source_location: None,
        };

        let rule = LemmaRule {
            name: "is_adult".to_string(),
            expression: age_expr,
            unless_clauses: Vec::new(),
            source_location: None,
        };
        doc = doc.add_rule(rule);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
        assert!(result.is_ok(), "Should build graph successfully");

        let graph = result.unwrap();
        assert_eq!(graph.facts().len(), 1);
        assert_eq!(graph.rules().len(), 1);
    }

    #[test]
    fn should_reject_fact_override_into_non_document_fact() {
        // Higher-standard language rule:
        // if `x` is a literal (not a document reference), `x.y = ...` must be rejected.
        //
        // This is currently expected to FAIL until graph building enforces it consistently.
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact("x", LiteralValue::number(1)));

        // Override x.y, but x is not a document reference.
        doc = doc.add_fact(LemmaFact {
            reference: FactReference::from_path(vec!["x".to_string(), "y".to_string()]),
            value: FactValue::Literal(LiteralValue::number(2)),
            source_location: None,
        });

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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
        doc = doc.add_fact(create_literal_fact("x", LiteralValue::number(1)));
        doc = doc.add_rule(LemmaRule {
            name: "x".to_string(),
            expression: create_literal_expr(LiteralValue::number(2)),
            unless_clauses: Vec::new(),
            source_location: None,
        });

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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
            LiteralValue::number(rust_decimal::Decimal::from(25)),
        ));
        doc = doc.add_fact(create_literal_fact(
            "age",
            LiteralValue::number(rust_decimal::Decimal::from(30)),
        ));

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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
            expression: create_literal_expr(LiteralValue::boolean(true.into())),
            unless_clauses: Vec::new(),
            source_location: None,
        };
        let rule2 = LemmaRule {
            name: "test_rule".to_string(),
            expression: create_literal_expr(LiteralValue::boolean(false.into())),
            unless_clauses: Vec::new(),
            source_location: None,
        };

        doc = doc.add_rule(rule1);
        doc = doc.add_rule(rule2);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
        assert!(result.is_err(), "Should detect duplicate rule");

        let errors = result.unwrap_err();
        assert!(errors.iter().any(
            |e| e.to_string().contains("Duplicate rule") && e.to_string().contains("test_rule")
        ));
    }

    #[test]
    fn test_missing_fact_reference() {
        let mut doc = create_test_doc("test");

        let missing_fact_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "nonexistent".to_string(),
            }),
            source_location: None,
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: missing_fact_expr,
            unless_clauses: Vec::new(),
            source_location: None,
        };
        doc = doc.add_rule(rule);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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
            value: FactValue::DocumentReference("nonexistent".to_string()),
            source_location: None,
        };
        doc = doc.add_fact(fact);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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
            LiteralValue::number(rust_decimal::Decimal::from(25)),
        ));

        let age_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "age".to_string(),
            }),
            source_location: None,
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: age_expr,
            unless_clauses: Vec::new(),
            source_location: None,
        };
        doc = doc.add_rule(rule);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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

        let rule1_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "age".to_string(),
            }),
            source_location: None,
        };

        let rule1 = LemmaRule {
            name: "rule1".to_string(),
            expression: rule1_expr,
            unless_clauses: Vec::new(),
            source_location: None,
        };
        doc = doc.add_rule(rule1);

        let rule2_expr = Expression {
            kind: ExpressionKind::RuleReference(RuleReference {
                segments: Vec::new(),
                rule: "rule1".to_string(),
            }),
            source_location: None,
        };

        let rule2 = LemmaRule {
            name: "rule2".to_string(),
            expression: rule2_expr,
            unless_clauses: Vec::new(),
            source_location: None,
        };
        doc = doc.add_rule(rule2);

        doc = doc.add_fact(create_literal_fact(
            "age",
            LiteralValue::number(rust_decimal::Decimal::from(25)),
        ));

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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
            LiteralValue::number(rust_decimal::Decimal::from(25)),
        ));
        doc = doc.add_fact(create_literal_fact(
            "age",
            LiteralValue::number(rust_decimal::Decimal::from(30)),
        ));

        let missing_fact_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "nonexistent".to_string(),
            }),
            source_location: None,
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: missing_fact_expr,
            unless_clauses: Vec::new(),
            source_location: None,
        };
        doc = doc.add_rule(rule);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
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
}
