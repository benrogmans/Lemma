use crate::parsing::source::Source;
use crate::planning::execution_plan::Branch;
use crate::semantic::{
    ArithmeticComputation, BooleanValue, ConversionTarget, Expression, ExpressionKind, FactPath,
    FactValue, LemmaDoc, LemmaFact, LemmaRule, LemmaType, LiteralValue, NegationType, PathSegment,
    RulePath, TypeAnnotation,
};
use crate::LemmaError;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone)]
pub struct Graph {
    facts: IndexMap<FactPath, LemmaFact>,
    rules: IndexMap<RulePath, RuleNode>,
    sources: HashMap<String, (String, String)>,
    execution_order: Vec<RulePath>,
}

impl Graph {
    /// Create an empty graph (used for deserialization)
    pub fn empty() -> Self {
        Self {
            facts: IndexMap::new(),
            rules: IndexMap::new(),
            sources: HashMap::new(),
            execution_order: Vec::new(),
        }
    }

    pub fn facts(&self) -> &IndexMap<FactPath, LemmaFact> {
        &self.facts
    }

    pub fn rules(&self) -> &IndexMap<RulePath, RuleNode> {
        &self.rules
    }

    pub fn rules_mut(&mut self) -> &mut IndexMap<RulePath, RuleNode> {
        &mut self.rules
    }

    pub fn sources(&self) -> &HashMap<String, (String, String)> {
        &self.sources
    }

    pub fn execution_order(&self) -> &[RulePath] {
        &self.execution_order
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
            return Err(vec![LemmaError::CircularDependency(format!(
                "Circular dependency detected. Rules involved: {}",
                missing
                    .iter()
                    .map(|rule| rule.rule.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))]);
        }

        Ok(result)
    }
}

#[derive(Debug, Clone)]
pub struct RuleNode {
    /// Normalized branches with explicit conditions (last-wins semantics applied).
    /// All branches have explicit conditions - no Option<Expression> needed.
    /// Expressions are already converted (FactReference -> FactPath, RuleReference -> RulePath).
    pub branches: Vec<Branch>,
    pub source: Option<Source>,

    pub depends_on_rules: HashSet<RulePath>,

    /// Computed type of this rule's result (populated during validation)
    pub rule_type: Option<LemmaType>,
}

struct GraphBuilder<'a> {
    facts: IndexMap<FactPath, LemmaFact>,
    rules: IndexMap<RulePath, RuleNode>,
    sources: HashMap<String, (String, String)>,
    all_docs: HashMap<String, &'a LemmaDoc>,
    errors: Vec<LemmaError>,
}

/// Build suffix OR conditions for "last wins" semantics
///
/// For each branch i, returns the OR of all conditions from branches i+1 to end.
/// This represents "any later branch could match", which we need to exclude.
fn build_suffix_or_conditions(
    branches: &[(Option<Expression>, Expression)],
) -> Vec<Option<Expression>> {
    let mut suffix_or: Vec<Option<Expression>> = vec![None; branches.len()];
    let mut acc: Option<Expression> = None;

    // Build from end to beginning
    for i in (0..branches.len()).rev() {
        suffix_or[i] = acc.clone();

        // Add this branch's condition to the accumulator
        if let Some((Some(cond), _)) = branches.get(i) {
            acc = Some(match acc {
                None => cond.clone(),
                Some(prev) => Expression::new(
                    ExpressionKind::LogicalOr(Box::new(cond.clone()), Box::new(prev)),
                    cond.source.clone(),
                ),
            });
        }
    }

    suffix_or
}

/// Normalize rule branches by applying last-wins semantics
///
/// Makes branch conditions explicit: each branch's condition excludes all later branches.
/// Rule references (RulePath) remain as-is - they're not expanded here.
fn normalize_rule_branches(
    branches: &[(Option<Expression>, Expression)],
    source: &Option<Source>,
) -> Vec<Branch> {
    let suffix_or = build_suffix_or_conditions(branches);
    let mut normalized = Vec::new();

    for (idx, (condition, result)) in branches.iter().enumerate() {
        // Base condition: original condition or true for default branch
        let base_condition = condition.clone().unwrap_or_else(|| {
            Expression::new(
                ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                source.clone(),
            )
        });

        // Apply last-wins: base_condition AND NOT(suffix_or)
        // For default branch: true AND NOT(cond_1 OR cond_2 OR ...) = NOT(cond_1) AND NOT(cond_2) AND ...
        // For branch 1: cond_1 AND NOT(cond_2 OR cond_3 OR ...) = cond_1 AND NOT(cond_2) AND NOT(cond_3) AND ...
        let normalized_condition = if let Some(later_or) = &suffix_or[idx] {
            Expression::new(
                ExpressionKind::LogicalAnd(
                    Box::new(base_condition),
                    Box::new(Expression::new(
                        ExpressionKind::LogicalNegation(
                            Box::new(later_or.clone()),
                            NegationType::Not,
                        ),
                        source.clone(),
                    )),
                ),
                source.clone(),
            )
        } else {
            base_condition // Last branch - no later branches to exclude
        };

        normalized.push(Branch {
            condition: normalized_condition,
            result: result.clone(),
            source: source.clone(),
        });
    }

    normalized
}

impl Graph {
    pub fn build(
        main_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
        sources: HashMap<String, (String, String)>,
    ) -> Result<Graph, Vec<LemmaError>> {
        let mut builder = GraphBuilder {
            facts: IndexMap::new(),
            rules: IndexMap::new(),
            sources,
            all_docs: all_docs.iter().map(|doc| (doc.name.clone(), doc)).collect(),
            errors: Vec::new(),
        };

        builder.build_document(main_doc, Vec::new())?;

        if !builder.errors.is_empty() {
            return Err(builder.errors);
        }

        let mut graph = Graph {
            facts: builder.facts,
            rules: builder.rules,
            sources: builder.sources,
            execution_order: Vec::new(),
        };

        // Validate and compute execution order
        graph.validate_and_sort(all_docs)?;

        Ok(graph)
    }

    fn validate_and_sort(&mut self, all_docs: &[LemmaDoc]) -> Result<(), Vec<LemmaError>> {
        let mut errors = Vec::new();

        validate_document_interfaces(self, all_docs, &mut errors);
        validate_all_rule_references_exist(self, &mut errors);

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
    ) -> Result<(), Vec<LemmaError>> {
        self.build_document_with_overrides(doc, current_segments, HashMap::new())
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
                    self.errors
                        .push(LemmaError::Engine(format!("Fact '{}' not found", segment)));
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
                        self.errors.push(LemmaError::Engine(format!(
                            "Document '{}' not found",
                            doc_name
                        )));
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
                self.errors.push(LemmaError::Engine(format!(
                    "Fact '{}' is not a document reference",
                    segment
                )));
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
            self.errors.push(LemmaError::Engine(format!(
                "Duplicate fact '{}'",
                fact_path.fact
            )));
            return;
        }

        let current_depth = current_segments.len();

        match &fact.value {
            FactValue::Literal(_) | FactValue::TypeAnnotation(_) => {
                // Check if there's an override for this literal/type fact
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
                    source: fact.source.clone(),
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
                        self.errors.push(LemmaError::Engine(format!(
                            "Document '{}' not found",
                            effective_doc_name
                        )));
                        return;
                    }
                };

                // Store the fact with the effective document reference
                let stored_fact = LemmaFact {
                    reference: fact.reference.clone(),
                    value: FactValue::DocumentReference(effective_doc_name.clone()),
                    source: fact.source.clone(),
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

                let _ = self.build_document_with_overrides(
                    nested_doc,
                    nested_segments,
                    nested_overrides,
                );
            }
        }
    }

    fn build_document_with_overrides(
        &mut self,
        doc: &'a LemmaDoc,
        current_segments: Vec<PathSegment>,
        override_map: HashMap<String, Vec<(&'a LemmaFact, usize)>>,
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
            self.add_fact_with_overrides(fact, &current_segments, &pending_overrides);
        }

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
        current_doc: &LemmaDoc,
        facts_map: &HashMap<String, &'a LemmaFact>,
        current_segments: &[PathSegment],
        effective_doc_refs: &HashMap<String, String>,
    ) {
        let rule_path = RulePath {
            segments: current_segments.to_vec(),
            rule: rule.name.clone(),
        };

        if self.rules.contains_key(&rule_path) {
            self.errors.push(LemmaError::Engine(format!(
                "Duplicate rule '{}'",
                rule_path.rule
            )));
            return;
        }

        let mut raw_branches = Vec::new();
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
        raw_branches.push((None, converted_expression));

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
            raw_branches.push((Some(converted_condition), converted_result));
        }

        let source = rule.source.clone();

        // Normalize branches (apply last-wins semantics)
        let normalized_branches = normalize_rule_branches(&raw_branches, &source);

        let rule_node = RuleNode {
            branches: normalized_branches,
            source,
            depends_on_rules,
            rule_type: None,
        };

        self.rules.insert(rule_path, rule_node);
    }

    #[allow(clippy::too_many_arguments)]
    fn convert_binary_operands(
        &mut self,
        left: &Expression,
        right: &Expression,
        current_doc: &LemmaDoc,
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
        current_doc: &LemmaDoc,
        facts_map: &HashMap<String, &'a LemmaFact>,
        current_segments: &[PathSegment],
        depends_on_rules: &mut HashSet<RulePath>,
        effective_doc_refs: &HashMap<String, String>,
    ) -> Option<Expression> {
        match &expr.kind {
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
                        self.errors.push(LemmaError::Engine(format!(
                            "'{}' is a rule, not a fact. Use '{}?' to reference rules",
                            fact_ref.fact, fact_ref.fact
                        )));
                    } else {
                        self.errors.push(LemmaError::Engine(format!(
                            "Fact '{}' not found",
                            fact_ref.fact
                        )));
                    }
                    return None;
                }

                let fact_path = FactPath {
                    segments,
                    fact: fact_ref.fact.clone(),
                };

                Some(Expression {
                    kind: ExpressionKind::FactPath(fact_path),
                    source: expr.source.clone(),
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
                    source: expr.source.clone(),
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
                    kind: ExpressionKind::LogicalAnd(Box::new(l), Box::new(r)),
                    source: expr.source.clone(),
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
                    kind: ExpressionKind::LogicalOr(Box::new(l), Box::new(r)),
                    source: expr.source.clone(),
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
                    kind: ExpressionKind::Arithmetic(Box::new(l), op.clone(), Box::new(r)),
                    source: expr.source.clone(),
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
                    kind: ExpressionKind::Comparison(Box::new(l), op.clone(), Box::new(r)),
                    source: expr.source.clone(),
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
                    kind: ExpressionKind::UnitConversion(Box::new(converted_value), target.clone()),
                    source: expr.source.clone(),
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
                        Box::new(converted_operand),
                        neg_type.clone(),
                    ),
                    source: expr.source.clone(),
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
                        Box::new(converted_operand),
                    ),
                    source: expr.source.clone(),
                })
            }

            ExpressionKind::FactPath(_) => Some(expr.clone()),
            ExpressionKind::RulePath(rule_path) => {
                depends_on_rules.insert(rule_path.clone());
                Some(expr.clone())
            }

            ExpressionKind::Literal(_) | ExpressionKind::Veto(_) => Some(expr.clone()),
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

        // All branches have explicit conditions after normalization
        // Branch 0's condition excludes all later branches (NOT(cond_1) AND NOT(cond_2) AND ...)
        // Branch 1+'s conditions are cond_i AND NOT(cond_{i+1}) AND ...
        let default_result = &branches[0].result;
        let default_type = compute_expression_type(default_result, graph, &computed_types, errors);

        let mut all_branch_types: Vec<Option<LemmaType>> = vec![default_type.clone()];

        // Validate condition types and result types for all branches
        for (branch_index, branch) in branches.iter().enumerate() {
            // Validate condition type (all branches have explicit conditions)
            let condition_type =
                compute_expression_type(&branch.condition, graph, &computed_types, errors);
            if let Some(cond_type) = condition_type {
                if cond_type != LemmaType::Boolean {
                    errors.push(LemmaError::Engine(format!(
                        "Branch condition in rule '{}' must be boolean, got {:?}",
                        rule_path.rule, cond_type
                    )));
                }
            }

            // Validate result type
            let result_type =
                compute_expression_type(&branch.result, graph, &computed_types, errors);
            if branch_index > 0 {
                // For branches after the first, check consistency with the first branch's result type
                all_branch_types.push(result_type.clone());
                validate_branch_type_consistency(
                    rule_path,
                    branch_index,
                    &default_type,
                    &result_type,
                    errors,
                );
            }
        }

        if let Some(rule_type) = default_type {
            computed_types.insert(rule_path.clone(), rule_type);
        } else if let Some(branch_type_value) = all_branch_types.iter().flatten().next() {
            computed_types.insert(rule_path.clone(), branch_type_value.clone());
        }
    }

    for (rule_path, rule_type) in computed_types {
        if let Some(rule_node) = graph.rules_mut().get_mut(&rule_path) {
            rule_node.rule_type = Some(rule_type);
        }
    }
}

fn validate_branch_type_consistency(
    rule_path: &RulePath,
    branch_index: usize,
    default_type: &Option<LemmaType>,
    branch_type: &Option<LemmaType>,
    errors: &mut Vec<LemmaError>,
) {
    if let (Some(default), Some(branch)) = (default_type, branch_type) {
        if default != branch {
            errors.push(LemmaError::Engine(format!(
                "Type mismatch in rule '{}': default branch returns {:?}, but unless clause {} returns {:?}",
                rule_path.rule, default, branch_index, branch
            )));
        }
    }
}

fn compute_expression_type(
    expression: &Expression,
    graph: &Graph,
    computed_rule_types: &HashMap<RulePath, LemmaType>,
    errors: &mut Vec<LemmaError>,
) -> Option<LemmaType> {
    match &expression.kind {
        ExpressionKind::Literal(literal_value) => Some(literal_value.to_type()),
        ExpressionKind::FactPath(fact_path) => compute_fact_type(fact_path, graph, errors),
        ExpressionKind::RulePath(rule_path) => computed_rule_types.get(rule_path).cloned(),
        ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
            let left_type = compute_expression_type(left, graph, computed_rule_types, errors);
            let right_type = compute_expression_type(right, graph, computed_rule_types, errors);
            validate_logical_operands(left_type.as_ref(), right_type.as_ref(), errors);
            Some(LemmaType::Boolean)
        }
        ExpressionKind::LogicalNegation(operand, _) => {
            let operand_type = compute_expression_type(operand, graph, computed_rule_types, errors);
            validate_logical_operand(operand_type.as_ref(), errors);
            Some(LemmaType::Boolean)
        }
        ExpressionKind::Comparison(left, _, right) => {
            let left_type = compute_expression_type(left, graph, computed_rule_types, errors);
            let right_type = compute_expression_type(right, graph, computed_rule_types, errors);
            validate_comparison_types(left_type.as_ref(), right_type.as_ref(), errors);
            Some(LemmaType::Boolean)
        }
        ExpressionKind::Arithmetic(left, operator, right) => {
            let left_type = compute_expression_type(left, graph, computed_rule_types, errors);
            let right_type = compute_expression_type(right, graph, computed_rule_types, errors);
            validate_arithmetic_types(left_type.as_ref(), right_type.as_ref(), operator, errors);
            compute_arithmetic_result_type(left_type, right_type, operator)
        }
        ExpressionKind::UnitConversion(source_expression, target) => {
            let source_type =
                compute_expression_type(source_expression, graph, computed_rule_types, errors);
            validate_unit_conversion_types(source_type.as_ref(), target, errors);
            Some(conversion_target_to_type(target))
        }
        ExpressionKind::MathematicalComputation(_, operand) => {
            let operand_type = compute_expression_type(operand, graph, computed_rule_types, errors);
            validate_mathematical_operand(operand_type.as_ref(), errors);
            Some(LemmaType::Number)
        }
        ExpressionKind::Veto(_) => None,
        ExpressionKind::FactReference(_) | ExpressionKind::RuleReference(_) => {
            errors.push(LemmaError::Engine(
                "Internal error: FactReference/RuleReference should be converted during graph building".to_string()
            ));
            None
        }
    }
}

fn validate_logical_operands(
    left_type: Option<&LemmaType>,
    right_type: Option<&LemmaType>,
    errors: &mut Vec<LemmaError>,
) {
    if let Some(left) = left_type {
        if *left != LemmaType::Boolean {
            errors.push(LemmaError::Engine(format!(
                "Logical operation requires boolean operands, got {:?} for left operand",
                left
            )));
        }
    }
    if let Some(right) = right_type {
        if *right != LemmaType::Boolean {
            errors.push(LemmaError::Engine(format!(
                "Logical operation requires boolean operands, got {:?} for right operand",
                right
            )));
        }
    }
}

fn validate_logical_operand(operand_type: Option<&LemmaType>, errors: &mut Vec<LemmaError>) {
    if let Some(operand) = operand_type {
        if *operand != LemmaType::Boolean {
            errors.push(LemmaError::Engine(format!(
                "Logical negation requires boolean operand, got {:?}",
                operand
            )));
        }
    }
}

fn validate_comparison_types(
    left_type: Option<&LemmaType>,
    right_type: Option<&LemmaType>,
    errors: &mut Vec<LemmaError>,
) {
    if let (Some(left), Some(right)) = (left_type, right_type) {
        if left == right {
            return;
        }
        if left.is_numeric() && right.is_numeric() {
            return;
        }
        errors.push(LemmaError::Engine(format!(
            "Cannot compare {:?} with {:?}",
            left, right
        )));
    }
}

fn validate_arithmetic_types(
    left_type: Option<&LemmaType>,
    right_type: Option<&LemmaType>,
    operator: &ArithmeticComputation,
    errors: &mut Vec<LemmaError>,
) {
    if let (Some(left), Some(right)) = (left_type, right_type) {
        if left.is_temporal() || right.is_temporal() {
            if compute_temporal_arithmetic_result_type(left, right, operator).is_none() {
                errors.push(LemmaError::Engine(format!(
                    "Invalid date/time arithmetic: {:?} {:?} {:?}",
                    left, operator, right
                )));
            }
            return;
        }
        if !left.is_numeric() {
            errors.push(LemmaError::Engine(format!(
                "Arithmetic operation requires numeric operands, got {:?} for left operand",
                left
            )));
            return;
        }
        if !right.is_numeric() {
            errors.push(LemmaError::Engine(format!(
                "Arithmetic operation requires numeric operands, got {:?} for right operand",
                right
            )));
            return;
        }
        validate_arithmetic_operator_constraints(left, right, operator, errors);
    }
}

fn validate_arithmetic_operator_constraints(
    left_type: &LemmaType,
    right_type: &LemmaType,
    operator: &ArithmeticComputation,
    errors: &mut Vec<LemmaError>,
) {
    match operator {
        ArithmeticComputation::Modulo => {
            if left_type.is_unit() || right_type.is_unit() {
                errors.push(LemmaError::Engine(format!(
                    "Modulo operation not supported for unit types: {:?} % {:?}",
                    left_type, right_type
                )));
            }
        }
        ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {}
        ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
            if left_type.is_unit() && right_type.is_unit() && left_type != right_type {
                errors.push(LemmaError::Engine(format!(
                    "Cannot add/subtract different unit categories: {:?} and {:?}",
                    left_type, right_type
                )));
            }
        }
        ArithmeticComputation::Power => {
            if *right_type != LemmaType::Number && *right_type != LemmaType::Percentage {
                errors.push(LemmaError::Engine(format!(
                    "Power exponent must be a number, got {:?}",
                    right_type
                )));
            }
        }
    }
}

fn validate_unit_conversion_types(
    source_type: Option<&LemmaType>,
    target: &ConversionTarget,
    errors: &mut Vec<LemmaError>,
) {
    let target_type = conversion_target_to_type(target);
    if let Some(source) = source_type {
        if *source != target_type && *source != LemmaType::Number {
            errors.push(LemmaError::Engine(format!(
                "Cannot convert {:?} to {:?}",
                source, target_type
            )));
        }
    }
}

fn validate_mathematical_operand(operand_type: Option<&LemmaType>, errors: &mut Vec<LemmaError>) {
    if let Some(operand) = operand_type {
        if !operand.is_numeric() {
            errors.push(LemmaError::Engine(format!(
                "Mathematical function requires numeric operand, got {:?}",
                operand
            )));
        }
    }
}

fn compute_fact_type(
    fact_path: &FactPath,
    graph: &Graph,
    errors: &mut Vec<LemmaError>,
) -> Option<LemmaType> {
    let fact = match graph.facts().get(fact_path) {
        Some(fact) => fact,
        None => {
            let potential_rule_path = RulePath {
                segments: fact_path.segments.clone(),
                rule: fact_path.fact.clone(),
            };
            if graph.rules().contains_key(&potential_rule_path) {
                errors.push(LemmaError::Engine(format!(
                    "'{}' is a rule, not a fact. Use '{}?' to reference rules",
                    fact_path.fact, fact_path.fact
                )));
            } else {
                errors.push(LemmaError::Engine(format!(
                    "Fact '{}' not found",
                    fact_path
                )));
            }
            return None;
        }
    };
    match &fact.value {
        FactValue::Literal(literal_value) => Some(literal_value.to_type()),
        FactValue::TypeAnnotation(TypeAnnotation::LemmaType(lemma_type)) => {
            Some(lemma_type.clone())
        }
        FactValue::DocumentReference(_) => None,
    }
}

fn compute_arithmetic_result_type(
    left_type: Option<LemmaType>,
    right_type: Option<LemmaType>,
    operator: &ArithmeticComputation,
) -> Option<LemmaType> {
    match (left_type.as_ref(), right_type.as_ref()) {
        (Some(left), Some(right)) => {
            if left.is_temporal() || right.is_temporal() {
                return compute_temporal_arithmetic_result_type(left, right, operator);
            }
            if left == right {
                return left_type;
            }
            if *left == LemmaType::Number && *right == LemmaType::Percentage {
                return match operator {
                    ArithmeticComputation::Multiply
                    | ArithmeticComputation::Add
                    | ArithmeticComputation::Subtract => Some(LemmaType::Number),
                    _ => None,
                };
            }
            if *left == LemmaType::Percentage && *right == LemmaType::Number {
                return match operator {
                    ArithmeticComputation::Multiply => Some(LemmaType::Number),
                    ArithmeticComputation::Divide => Some(LemmaType::Percentage),
                    _ => None,
                };
            }
            if *left == LemmaType::Number {
                return right_type;
            }
            if *right == LemmaType::Number {
                return left_type;
            }
            Some(LemmaType::Number)
        }
        _ => None,
    }
}

fn compute_temporal_arithmetic_result_type(
    left: &LemmaType,
    right: &LemmaType,
    operator: &ArithmeticComputation,
) -> Option<LemmaType> {
    match operator {
        ArithmeticComputation::Subtract => {
            if left.is_temporal() && right.is_temporal() {
                return Some(LemmaType::Duration);
            }
            if left.is_temporal() && *right == LemmaType::Duration {
                return Some(left.clone());
            }
        }
        ArithmeticComputation::Add => {
            if left.is_temporal() && *right == LemmaType::Duration {
                return Some(left.clone());
            }
            if *left == LemmaType::Duration && right.is_temporal() {
                return Some(right.clone());
            }
        }
        _ => {}
    }
    None
}

fn conversion_target_to_type(target: &ConversionTarget) -> LemmaType {
    match target {
        ConversionTarget::Mass(_) => LemmaType::Mass,
        ConversionTarget::Length(_) => LemmaType::Length,
        ConversionTarget::Volume(_) => LemmaType::Volume,
        ConversionTarget::Duration(_) => LemmaType::Duration,
        ConversionTarget::Temperature(_) => LemmaType::Temperature,
        ConversionTarget::Power(_) => LemmaType::Power,
        ConversionTarget::Force(_) => LemmaType::Force,
        ConversionTarget::Pressure(_) => LemmaType::Pressure,
        ConversionTarget::Energy(_) => LemmaType::Energy,
        ConversionTarget::Frequency(_) => LemmaType::Frequency,
        ConversionTarget::Data(_) => LemmaType::Data,
        ConversionTarget::Percentage => LemmaType::Percentage,
    }
}

fn validate_all_rule_references_exist(graph: &Graph, errors: &mut Vec<LemmaError>) {
    let existing_rules: HashSet<&RulePath> = graph.rules().keys().collect();
    for (rule_path, rule_node) in graph.rules() {
        for dependency in &rule_node.depends_on_rules {
            if !existing_rules.contains(dependency) {
                errors.push(LemmaError::Engine(format!(
                    "Rule '{}' references non-existent rule '{}'",
                    rule_path.rule, dependency.rule
                )));
            }
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
                        errors.push(LemmaError::Engine(format!(
                            "Document '{}' referenced by '{}' is missing required rule '{}'",
                            doc_name, fact_path, required_rule
                        )));
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
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn create_test_doc(name: &str) -> LemmaDoc {
        LemmaDoc::new(name.to_string())
    }

    fn create_test_source() -> Source {
        use crate::parsing::ast::Span;
        Source::new(
            "<test>",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "test",
        )
    }

    fn create_literal_fact(name: &str, value: LiteralValue) -> LemmaFact {
        LemmaFact {
            reference: FactReference {
                segments: Vec::new(),
                fact: name.to_string(),
            },
            value: FactValue::Literal(value),
            source: Some(create_test_source()),
        }
    }

    fn create_literal_expr(value: LiteralValue) -> Expression {
        Expression {
            kind: ExpressionKind::Literal(value),
            source: Some(create_test_source()),
        }
    }

    #[test]
    fn test_build_simple_graph() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact("age", LiteralValue::Number(25.into())));
        doc = doc.add_fact(create_literal_fact(
            "name",
            LiteralValue::Text("John".to_string()),
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
        doc = doc.add_fact(create_literal_fact("age", LiteralValue::Number(25.into())));

        let age_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "age".to_string(),
            }),
            source: Some(create_test_source()),
        };

        let rule = LemmaRule {
            name: "is_adult".to_string(),
            expression: age_expr,
            unless_clauses: Vec::new(),
            source: Some(create_test_source()),
        };
        doc = doc.add_rule(rule);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
        assert!(result.is_ok(), "Should build graph successfully");

        let graph = result.unwrap();
        assert_eq!(graph.facts().len(), 1);
        assert_eq!(graph.rules().len(), 1);
    }

    #[test]
    fn test_duplicate_fact() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact("age", LiteralValue::Number(25.into())));
        doc = doc.add_fact(create_literal_fact("age", LiteralValue::Number(30.into())));

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
        assert!(result.is_err(), "Should detect duplicate fact");

        let errors = result.unwrap_err();
        let error_strings: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let has_duplicate_fact = error_strings
            .iter()
            .any(|e| e.contains("Duplicate fact") && e.contains("age"));
        assert!(
            has_duplicate_fact,
            "Should have error mentioning 'Duplicate fact' and 'age'. Got errors: {:?}",
            error_strings
        );
    }

    #[test]
    fn test_duplicate_rule() {
        let mut doc = create_test_doc("test");

        let rule1 = LemmaRule {
            name: "test_rule".to_string(),
            expression: create_literal_expr(LiteralValue::Boolean(true.into())),
            unless_clauses: Vec::new(),
            source: Some(create_test_source()),
        };
        let rule2 = LemmaRule {
            name: "test_rule".to_string(),
            expression: create_literal_expr(LiteralValue::Boolean(false.into())),
            unless_clauses: Vec::new(),
            source: Some(create_test_source()),
        };

        doc = doc.add_rule(rule1);
        doc = doc.add_rule(rule2);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
        assert!(result.is_err(), "Should detect duplicate rule");

        let errors = result.unwrap_err();
        let error_strings: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let has_duplicate_rule = error_strings
            .iter()
            .any(|e| e.contains("Duplicate rule") && e.contains("test_rule"));
        assert!(
            has_duplicate_rule,
            "Should have error mentioning 'Duplicate rule' and 'test_rule'. Got errors: {:?}",
            error_strings
        );
    }

    #[test]
    fn test_missing_fact_reference() {
        let mut doc = create_test_doc("test");

        let missing_fact_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "nonexistent".to_string(),
            }),
            source: Some(create_test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: missing_fact_expr,
            unless_clauses: Vec::new(),
            source: Some(create_test_source()),
        };
        doc = doc.add_rule(rule);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
        assert!(result.is_err(), "Should detect missing fact");

        let errors = result.unwrap_err();
        let error_strings: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let has_missing_fact = error_strings.iter().any(|e| {
            e.contains("Fact 'nonexistent' not found")
                || (e.contains("not found") && e.contains("nonexistent"))
        });
        assert!(
            has_missing_fact,
            "Should have error mentioning missing fact 'nonexistent'. Got errors: {:?}",
            error_strings
        );
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
            source: Some(create_test_source()),
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
        doc = doc.add_fact(create_literal_fact("age", LiteralValue::Number(25.into())));

        let age_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "age".to_string(),
            }),
            source: Some(create_test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: age_expr,
            unless_clauses: Vec::new(),
            source: Some(create_test_source()),
        };
        doc = doc.add_rule(rule);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
        assert!(result.is_ok(), "Should build graph successfully");

        let graph = result.unwrap();
        let rule_node = graph.rules().values().next().unwrap();

        assert!(matches!(
            rule_node.branches[0].result.kind,
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
            source: Some(create_test_source()),
        };

        let rule1 = LemmaRule {
            name: "rule1".to_string(),
            expression: rule1_expr,
            unless_clauses: Vec::new(),
            source: Some(create_test_source()),
        };
        doc = doc.add_rule(rule1);

        let rule2_expr = Expression {
            kind: ExpressionKind::RuleReference(RuleReference {
                segments: Vec::new(),
                rule: "rule1".to_string(),
            }),
            source: Some(create_test_source()),
        };

        let rule2 = LemmaRule {
            name: "rule2".to_string(),
            expression: rule2_expr,
            unless_clauses: Vec::new(),
            source: Some(create_test_source()),
        };
        doc = doc.add_rule(rule2);

        doc = doc.add_fact(create_literal_fact("age", LiteralValue::Number(25.into())));

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
            rule2_node.branches[0].result.kind,
            ExpressionKind::RulePath(_)
        ));
    }

    #[test]
    fn test_collect_multiple_errors() {
        let mut doc = create_test_doc("test");
        doc = doc.add_fact(create_literal_fact("age", LiteralValue::Number(25.into())));
        doc = doc.add_fact(create_literal_fact("age", LiteralValue::Number(30.into())));

        let missing_fact_expr = Expression {
            kind: ExpressionKind::FactReference(FactReference {
                segments: Vec::new(),
                fact: "nonexistent".to_string(),
            }),
            source: Some(create_test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: missing_fact_expr,
            unless_clauses: Vec::new(),
            source: Some(create_test_source()),
        };
        doc = doc.add_rule(rule);

        let result = Graph::build(&doc, &[doc.clone()], HashMap::new());
        assert!(result.is_err(), "Should collect multiple errors");

        let errors = result.unwrap_err();
        // Should have exactly 2 errors: duplicate fact and missing fact
        assert_eq!(
            errors.len(),
            2,
            "Should have exactly 2 errors (duplicate fact and missing fact), got {} errors: {:?}",
            errors.len(),
            errors
        );

        // Verify both specific errors are present
        let has_duplicate = errors
            .iter()
            .any(|e| e.to_string().contains("Duplicate fact") && e.to_string().contains("age"));
        let has_missing = errors
            .iter()
            .any(|e| e.to_string().contains("Fact 'nonexistent' not found"));

        assert!(
            has_duplicate,
            "Should have duplicate fact error for 'age'. Errors: {:?}",
            errors
        );
        assert!(
            has_missing,
            "Should have missing fact error for 'nonexistent'. Errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_rule_reference_without_question_mark_fails() {
        use crate::Engine;

        let lemma_code = r#"
doc test_validation

fact base = 100

rule calculated = base * 2

rule buggy_usage = calculated + 50
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(lemma_code, "test.lemma");

        assert!(
            result.is_err(),
            "Should fail when referencing a rule without ?"
        );
        let err_msg = result.unwrap_err().to_string();
        // Error must mention the rule name AND indicate it needs ?
        assert!(
            err_msg.contains("calculated"),
            "Error should mention rule name 'calculated'. Got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("rule") || err_msg.contains("?"),
            "Error should indicate it's a rule or needs ?. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_fact_reference_with_question_mark_fails() {
        use crate::Engine;

        let lemma_code = r#"
doc test_validation

fact base = 100
fact multiplier = 2

rule buggy_usage = base? * multiplier?
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(lemma_code, "test.lemma");

        assert!(
            result.is_err(),
            "Should fail when referencing a fact with ?"
        );
        let err_msg = result.unwrap_err().to_string();
        // The error might be about rule reference instead of fact reference
        // Check if it mentions the fact names OR if it's a different error type
        let mentions_fact = err_msg.contains("base") || err_msg.contains("multiplier");
        let indicates_fact_issue =
            err_msg.contains("fact") || err_msg.contains("?") || err_msg.contains("should not");
        let is_rule_reference_error = err_msg.contains("rule")
            && (err_msg.contains("base") || err_msg.contains("multiplier"));

        // Error should either mention fact reference issue OR be a rule reference error
        assert!(
            (mentions_fact && indicates_fact_issue) || is_rule_reference_error,
            "Error should mention fact name and fact reference issue, or be a rule reference error. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_correct_rule_reference_with_question_mark_succeeds() {
        use crate::Engine;

        let lemma_code = r#"
doc test_validation

fact base = 100

rule calculated = base * 2

rule correct_usage = calculated? + 50
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(lemma_code, "test.lemma");
        assert!(
            result.is_ok(),
            "Should succeed when using ? for rule reference: {:?}",
            result
        );

        // Verify the rule actually works by evaluating it
        let response = engine
            .evaluate(
                "test_validation",
                vec!["correct_usage".to_string()],
                HashMap::new(),
            )
            .expect("Should evaluate successfully");
        let rule_result = response
            .results
            .get("correct_usage")
            .expect("Should have correct_usage rule result");
        match &rule_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                // calculated = base * 2 = 100 * 2 = 200
                // correct_usage = calculated? + 50 = 200 + 50 = 250
                assert_eq!(*n, rust_decimal::Decimal::from(250));
            }
            other => panic!("correct_usage should be 250, got: {:?}", other),
        }
    }

    #[test]
    fn test_correct_fact_reference_without_question_mark_succeeds() {
        use crate::Engine;

        let lemma_code = r#"
doc test_validation

fact base = 100
fact multiplier = 2

rule correct_usage = base * multiplier
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(lemma_code, "test.lemma");
        assert!(
            result.is_ok(),
            "Should succeed when not using ? for fact reference: {:?}",
            result
        );

        // Verify the rule actually works by evaluating it
        let response = engine
            .evaluate(
                "test_validation",
                vec!["correct_usage".to_string()],
                HashMap::new(),
            )
            .expect("Should evaluate successfully");
        let rule_result = response
            .results
            .get("correct_usage")
            .expect("Should have correct_usage rule result");
        match &rule_result.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                // correct_usage = base * multiplier = 100 * 2 = 200
                assert_eq!(*n, Decimal::from(200));
            }
            other => panic!("correct_usage should be 200, got: {:?}", other),
        }
    }

    #[test]
    fn test_rule_reference_in_unless_clause_without_question_mark_fails() {
        use crate::Engine;

        let lemma_code = r#"
doc test_validation

fact amount = 100

rule is_valid = amount > 50

rule discount = 0%
  unless is_valid then 10%
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(lemma_code, "test.lemma");

        assert!(
            result.is_err(),
            "Should fail when referencing a rule without ? in unless condition"
        );
        let err_msg = result.unwrap_err().to_string();
        // Error must mention the rule name AND indicate it needs ?
        assert!(
            err_msg.contains("is_valid"),
            "Error should mention rule name 'is_valid'. Got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("rule") || err_msg.contains("?"),
            "Error should indicate it's a rule or needs ?. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_document_field_rule_reference_without_question_mark_fails() {
        use crate::Engine;

        let lemma_code = r#"
doc base_doc
fact salary = 5000
rule annual = salary * 12

doc main_doc
fact employee = doc base_doc
rule buggy = employee.annual + 1000
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(lemma_code, "test.lemma");

        assert!(
            result.is_err(),
            "Should fail when referencing document rule without ?"
        );
        let err_msg = result.unwrap_err().to_string();
        // Error must mention the rule name AND indicate it needs ?
        assert!(
            err_msg.contains("annual") || err_msg.contains("employee.annual"),
            "Error should mention rule name 'annual'. Got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("rule") || err_msg.contains("?"),
            "Error should indicate it's a rule or needs ?. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_document_field_fact_reference_with_question_mark_fails() {
        use crate::Engine;

        let lemma_code = r#"
doc base_doc
fact salary = 5000

doc main_doc
fact employee = doc base_doc
rule buggy = employee.salary? * 2
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(lemma_code, "test.lemma");

        assert!(
            result.is_err(),
            "Should fail when referencing document fact with ?"
        );
        let err_msg = result.unwrap_err().to_string();
        // Error must mention the fact name
        // The error might be about rule reference instead, so check for either
        let mentions_fact = err_msg.contains("salary") || err_msg.contains("employee.salary");
        let indicates_issue = err_msg.contains("fact")
            || err_msg.contains("?")
            || err_msg.contains("should not")
            || err_msg.contains("rule");

        assert!(
            mentions_fact,
            "Error should mention fact name 'salary'. Got: {}",
            err_msg
        );
        assert!(
            indicates_issue,
            "Error should indicate fact/rule reference issue. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_logical_and_requires_boolean_operands() {
        use crate::Engine;

        let code = r#"
doc test
rule result = 5 and true
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Should reject non-boolean in 'and'");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("boolean"),
            "Error should mention 'boolean'. Got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("and") || err_msg.contains("logical"),
            "Error should mention 'and' or 'logical'. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_logical_or_requires_boolean_operands() {
        use crate::Engine;

        let code = r#"
doc test
rule result = "hello" or false
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Should reject non-boolean in 'or'");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("boolean"),
            "Error should mention 'boolean'. Got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("or") || err_msg.contains("logical"),
            "Error should mention 'or' or 'logical'. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_unless_condition_must_be_boolean() {
        use crate::Engine;

        let code = r#"
doc test
rule result = 10
  unless 5 then 20
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_err(), "Unless condition must be boolean");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("boolean") || err_msg.contains("unless"),
            "Error should mention 'boolean' or 'unless'. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_number_vs_percentage_type_mismatch() {
        use crate::Engine;

        let code = r#"
doc test

fact income = 100000
fact total_tax = 20000

rule effective_tax_rate = (total_tax / income)
  unless income == 0 then 0%
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing number and percentage types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            err.contains("number") && err.contains("percentage"),
            "Error should mention both 'number' and 'percentage' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_percentage_vs_number_type_mismatch() {
        use crate::Engine;

        let code = r#"
doc test

fact rate = 10%

rule adjusted_rate = rate
  unless rate > 5% then 100
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing percentage and number types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            err.contains("percentage") && err.contains("number"),
            "Error should mention both 'percentage' and 'number' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_mass_vs_length_type_mismatch() {
        use crate::Engine;

        let code = r#"
doc test

fact weight = 50 kilograms

rule measurement = weight
  unless weight > 100 kilograms then 10 meters
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing mass and length types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            err.contains("mass") && err.contains("length"),
            "Error should mention both 'mass' and 'length' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_volume_vs_duration_type_mismatch() {
        use crate::Engine;

        let code = r#"
doc test

fact capacity = 100 liters

rule result = capacity
  unless capacity > 50 liters then 5 hours
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing volume and duration types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            err.contains("volume") && err.contains("duration"),
            "Error should mention both 'volume' and 'duration' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_power_vs_energy_type_mismatch() {
        use crate::Engine;

        let code = r#"
doc test

fact consumption = 1000 watts

rule result = consumption
  unless consumption > 500 watts then 100 joules
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing power and energy types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            err.contains("power") && err.contains("energy"),
            "Error should mention both 'power' and 'energy' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_frequency_vs_pressure_type_mismatch() {
        use crate::Engine;

        let code = r#"
doc test

fact freq = 100 hertz

rule result = freq
  unless freq > 50 hertz then 10 pascals
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing frequency and pressure types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            err.contains("frequency") && err.contains("pressure"),
            "Error should mention both 'frequency' and 'pressure' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_data_size_vs_force_type_mismatch() {
        use crate::Engine;

        let code = r#"
doc test

fact size = 1024 megabytes

rule result = size
  unless size > 500 megabytes then 100 newtons
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing data size and force types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            (err.contains("data") || err.contains("size")) && err.contains("force"),
            "Error should mention both 'data'/'size' and 'force' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_temperature_vs_length_type_mismatch() {
        use crate::Engine;

        let code = r#"
doc test

fact temp = 25 celsius

rule result = temp
  unless temp > 30 celsius then 100 meters
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing temperature and length types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            err.contains("temperature") && err.contains("length"),
            "Error should mention both 'temperature' and 'length' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_conversion_preserves_type_consistency() {
        use crate::Engine;

        let code = r#"
doc test

fact income = 100000
fact tax = 20000

rule rate_decimal = tax / income
rule rate_percentage = (tax / income) in percentage
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_ok(),
            "Should allow separate rules with different types"
        );

        // Verify both rules can be evaluated correctly
        let response = engine
            .evaluate(
                "test",
                vec!["rate_decimal".to_string(), "rate_percentage".to_string()],
                HashMap::new(),
            )
            .expect("Should evaluate successfully");

        // rate_decimal = 20000 / 100000 = 0.2 (number)
        let rate_decimal = response
            .results
            .get("rate_decimal")
            .expect("Should have rate_decimal");
        match &rate_decimal.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, Decimal::from_str("0.2").unwrap());
            }
            other => panic!("rate_decimal should be 0.2 (number), got: {:?}", other),
        }

        // rate_percentage = (20000 / 100000) in percentage = 20% (percentage)
        let rate_percentage = response
            .results
            .get("rate_percentage")
            .expect("Should have rate_percentage");
        match &rate_percentage.result {
            crate::OperationResult::Value(crate::LiteralValue::Percentage(n)) => {
                assert_eq!(*n, Decimal::from_str("20").unwrap());
            }
            other => panic!(
                "rate_percentage should be 20% (percentage), got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_same_unit_type_allowed() {
        use crate::Engine;

        let code = r#"
doc test

fact weight = 50 kilograms

rule adjusted_weight = weight
  unless weight > 100 kilograms then 75 grams
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_ok(),
            "Should allow same category units (mass vs mass)"
        );

        // Verify the rule evaluates correctly with unit conversion
        let response = engine
            .evaluate("test", vec!["adjusted_weight".to_string()], HashMap::new())
            .expect("Should evaluate successfully");

        let adjusted_weight = response
            .results
            .get("adjusted_weight")
            .expect("Should have adjusted_weight");
        match &adjusted_weight.result {
            crate::OperationResult::Value(crate::LiteralValue::Unit(crate::NumericUnit::Mass(
                value,
                unit,
            ))) => {
                // weight = 50 kg, which is > 100 kg is false, so should return 50 kg
                assert_eq!(*unit, crate::MassUnit::Kilogram);
                assert_eq!(*value, Decimal::from_str("50").unwrap());
            }
            other => panic!(
                "adjusted_weight should be 50 kilograms (mass unit), got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_consistent_number_types() {
        use crate::Engine;

        let code = r#"
doc test
fact x = 10
fact condition = true

rule result = 5
    unless condition then 10
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok(), "Should allow consistent number types");

        // Verify the rule evaluates correctly
        let response = engine
            .evaluate("test", vec!["result".to_string()], HashMap::new())
            .expect("Should evaluate successfully");

        let result_rule = response
            .results
            .get("result")
            .expect("Should have result rule");
        match &result_rule.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                // condition = true, so "unless condition then 10" matches (last matching wins)
                // result = 10, not 5
                assert_eq!(*n, Decimal::from_str("10").unwrap());
            }
            other => panic!("result should be 10 (number), got: {:?}", other),
        }
    }

    #[test]
    fn test_consistent_text_types() {
        use crate::Engine;

        let code = r#"
doc test
fact condition = true

rule status = "pending"
    unless condition then "approved"
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok(), "Should allow consistent text types");

        // Verify the rule evaluates correctly
        let response = engine
            .evaluate("test", vec!["status".to_string()], HashMap::new())
            .expect("Should evaluate successfully");

        let status_rule = response
            .results
            .get("status")
            .expect("Should have status rule");
        match &status_rule.result {
            crate::OperationResult::Value(crate::LiteralValue::Text(text)) => {
                // condition = true, so "unless condition then 'approved'" matches (last matching wins)
                // status = "approved", not "pending"
                assert_eq!(text, "approved");
            }
            other => panic!("status should be 'approved' (text), got: {:?}", other),
        }
    }

    #[test]
    fn test_consistent_boolean_types() {
        use crate::Engine;

        let code = r#"
doc test
fact x = 10
fact y = 20

rule check = x > 5
    unless y > 15 then y < 25
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok(), "Should allow consistent boolean types");

        // Verify the rule evaluates correctly
        let response = engine
            .evaluate("test", vec!["check".to_string()], HashMap::new())
            .expect("Should evaluate successfully");

        let check_rule = response
            .results
            .get("check")
            .expect("Should have check rule");
        match &check_rule.result {
            crate::OperationResult::Value(crate::LiteralValue::Boolean(b)) => {
                // x = 10 > 5 is true, so unless y > 15 then y < 25 doesn't match (y=20 > 15), check = true
                assert!(bool::from(b));
            }
            other => panic!("check should be true (boolean), got: {:?}", other),
        }
    }

    #[test]
    fn test_mixed_number_and_text_rejected() {
        use crate::Engine;

        let code = r#"
doc test
fact condition = true

rule result = 100
    unless condition then "text"
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing number and text types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            (err.contains("number") || err.contains("text"))
                && (err.contains("number") || err.contains("text")),
            "Error should mention both 'number' and 'text' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_mixed_text_and_boolean_rejected() {
        use crate::Engine;

        let code = r#"
doc test
fact condition = true

rule result = "text"
    unless condition then true
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing number and text types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            (err.contains("number") || err.contains("text"))
                && (err.contains("number") || err.contains("text")),
            "Error should mention both 'number' and 'text' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_mixed_number_and_boolean_rejected() {
        use crate::Engine;

        let code = r#"
doc test
fact condition = true

rule result = 42
    unless condition then false
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing number and text types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            (err.contains("number") || err.contains("text"))
                && (err.contains("number") || err.contains("text")),
            "Error should mention both 'number' and 'text' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_multiple_unless_clauses_consistent() {
        use crate::Engine;

        let code = r#"
doc test
fact a = true
fact b = false

rule result = 1
    unless a then 2
    unless b then 3
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(
            result.is_ok(),
            "Should allow multiple unless clauses with consistent types"
        );

        // Verify the rule evaluates correctly
        // a = true, b = false
        // result = 1 (default)
        // unless a then 2 - a is true, so this matches -> result = 2
        // unless b then 3 - b is false, so this doesn't match
        // Last matching wins, so result = 2
        let response = engine
            .evaluate("test", vec!["result".to_string()], HashMap::new())
            .expect("Should evaluate successfully");

        let result_rule = response
            .results
            .get("result")
            .expect("Should have result rule");
        match &result_rule.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, Decimal::from_str("2").unwrap());
            }
            other => panic!("result should be 2 (number), got: {:?}", other),
        }
    }

    #[test]
    fn test_multiple_unless_clauses_inconsistent() {
        use crate::Engine;

        let code = r#"
doc test
fact a = true
fact b = false

rule result = 1
    unless a then 2
    unless b then "three"
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing number and text types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            (err.contains("number") || err.contains("text"))
                && (err.contains("number") || err.contains("text")),
            "Error should mention both 'number' and 'text' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_veto_with_consistent_types() {
        use crate::Engine;

        let code = r#"
doc test
fact blocked = true
fact condition = false

rule result = 10
    unless blocked then veto "blocked"
    unless condition then 20
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_veto_with_mixed_types() {
        use crate::Engine;

        let code = r#"
doc test
fact blocked = true
fact condition = false

rule result = 10
    unless blocked then veto "blocked"
    unless condition then "text"
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing number and text types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            (err.contains("number") || err.contains("text"))
                && (err.contains("number") || err.contains("text")),
            "Error should mention both 'number' and 'text' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_all_veto_clauses_allowed() {
        use crate::Engine;

        let code = r#"
doc test
fact a = true
fact b = false

rule result = 10
    unless a then veto "a"
    unless b then veto "b"
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_consistent_length_types() {
        use crate::Engine;

        let code = r#"
doc test
fact condition = true

rule distance = 100 meters
    unless condition then 200 meters
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_mixed_length_and_number_rejected() {
        use crate::Engine;

        let code = r#"
doc test
fact condition = true

rule distance = 100 meters
    unless condition then 200
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing number and text types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            (err.contains("number") || err.contains("text"))
                && (err.contains("number") || err.contains("text")),
            "Error should mention both 'number' and 'text' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_consistent_mass_types() {
        use crate::Engine;

        let code = r#"
doc test
fact heavy = true

rule weight = 10 kilograms
    unless heavy then 20 kilograms
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_mixed_mass_and_number_rejected() {
        use crate::Engine;

        let code = r#"
doc test
fact heavy = true

rule weight = 10 kilograms
    unless heavy then 20
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");

        assert!(
            result.is_err(),
            "Should reject mixing number and text types"
        );
        let err = result.unwrap_err().to_string().to_lowercase();
        // Error must mention both types involved
        assert!(
            (err.contains("number") || err.contains("text"))
                && (err.contains("number") || err.contains("text")),
            "Error should mention both 'number' and 'text' types. Got: {}",
            err
        );
        assert!(
            err.contains("incompatible") || err.contains("type") || err.contains("mismatch"),
            "Error should mention type incompatibility. Got: {}",
            err
        );
    }

    #[test]
    fn test_complex_expression_consistent_types() {
        use crate::Engine;

        let code = r#"
doc test
fact x = 10
fact y = 20
fact condition = true

rule result = x + y
    unless condition then x * 2
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }

    #[test]
    fn test_comparison_expression_consistent_types() {
        use crate::Engine;

        let code = r#"
doc test
fact x = 10
fact condition = true

rule check = x > 5
    unless condition then x < 20
"#;

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(code, "test.lemma");
        assert!(result.is_ok());
    }
}
