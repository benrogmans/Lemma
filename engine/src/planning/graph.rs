use crate::engine::Context;
use crate::parsing::ast::{
    self as ast, Constraint, EffectiveDate, LemmaData, LemmaRule, LemmaSpec, MetaValue, ParentType,
    Value,
};
use crate::parsing::source::Source;
use crate::planning::discovery;
use crate::planning::semantics::{
    self, conversion_target_to_semantic, primitive_boolean, primitive_date, primitive_duration,
    primitive_number, primitive_ratio, primitive_text, primitive_time, value_to_semantic,
    ArithmeticComputation, ComparisonComputation, DataDefinition, DataPath, Expression,
    ExpressionKind, LemmaType, LiteralValue, PathSegment, ReferenceTarget, RulePath,
    SemanticConversionTarget, TypeDefiningSpec, TypeExtends, TypeSpecification, ValueKind,
};
use crate::Error;
use ast::DataValue as ParsedDataValue;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::Arc;

/// Data bindings map: maps a target data name path to the binding's value and source.
///
/// The key is the full path of **data names** from the root spec to the target data.
/// Spec names are intentionally excluded from the key because spec ref bindings may change
/// which spec a segment points to — matching by data names only ensures bindings
/// are applied correctly regardless of spec ref bindings.
///
/// Example: `data employee.salary: 7500` in the root spec produces key `["employee", "salary"]`.
type DataBindings = HashMap<Vec<String>, (BindingValue, Source)>;

/// Binding value stored in [`DataBindings`]. Only two forms are valid for a
/// cross-spec binding: a literal value, or a reference to another data or rule.
///
/// References on the binding's right-hand side (e.g. `data license.other: law.other`)
/// are resolved at binding collection time against the spec in which the binding
/// itself was written (not the nested target spec). The resolved [`ReferenceTarget`]
/// is carried through so the nested spec's planning does not need the outer
/// spec's scope to interpret the reference.
#[derive(Debug, Clone)]
pub(crate) enum BindingValue {
    /// Literal RHS (parsed as a `Value`). Applied as a plain value to the bound data.
    Literal(ast::Value),
    /// Reference RHS pre-resolved to a concrete reference target.
    Reference {
        target: ReferenceTarget,
        constraints: Option<Vec<Constraint>>,
    },
}

#[derive(Debug)]
pub(crate) struct Graph {
    /// Root spec being planned (for error spec_context).
    main_spec: Arc<LemmaSpec>,
    data: IndexMap<DataPath, DataDefinition>,
    rules: BTreeMap<RulePath, RuleNode>,
    execution_order: Vec<RulePath>,
    /// Order in which references must be resolved so each reference's target
    /// (when it too is a reference) is already computed. References targeting
    /// non-reference data have no ordering constraints amongst themselves and
    /// appear in the order they are discovered.
    reference_evaluation_order: Vec<DataPath>,
}

impl Graph {
    pub(crate) fn data(&self) -> &IndexMap<DataPath, DataDefinition> {
        &self.data
    }

    pub(crate) fn rules(&self) -> &BTreeMap<RulePath, RuleNode> {
        &self.rules
    }

    pub(crate) fn rules_mut(&mut self) -> &mut BTreeMap<RulePath, RuleNode> {
        &mut self.rules
    }

    pub(crate) fn execution_order(&self) -> &[RulePath] {
        &self.execution_order
    }

    pub(crate) fn reference_evaluation_order(&self) -> &[DataPath] {
        &self.reference_evaluation_order
    }

    pub(crate) fn main_spec(&self) -> &Arc<LemmaSpec> {
        &self.main_spec
    }

    /// Build the data map: one entry per data (Value or SpecRef), with defaults and coercion applied.
    /// Preserves definition order from the source spec.
    pub(crate) fn build_data(&self) -> IndexMap<DataPath, DataDefinition> {
        struct PendingReference {
            target: ReferenceTarget,
            resolved_type: LemmaType,
            local_constraints: Option<Vec<Constraint>>,
            local_default: Option<ValueKind>,
        }

        let mut schema: HashMap<DataPath, LemmaType> = HashMap::new();
        let mut declared_defaults: HashMap<DataPath, ValueKind> = HashMap::new();
        let mut values: HashMap<DataPath, LiteralValue> = HashMap::new();
        let mut spec_arcs: HashMap<DataPath, Arc<LemmaSpec>> = HashMap::new();
        let mut references: HashMap<DataPath, PendingReference> = HashMap::new();

        for (path, rfv) in self.data.iter() {
            match rfv {
                DataDefinition::Value { value, .. } => {
                    values.insert(path.clone(), value.clone());
                    schema.insert(path.clone(), value.lemma_type.clone());
                }
                DataDefinition::TypeDeclaration {
                    resolved_type,
                    declared_default,
                    ..
                } => {
                    schema.insert(path.clone(), resolved_type.clone());
                    if let Some(dv) = declared_default {
                        declared_defaults.insert(path.clone(), dv.clone());
                    }
                }
                DataDefinition::SpecRef { spec: spec_arc, .. } => {
                    spec_arcs.insert(path.clone(), Arc::clone(spec_arc));
                }
                DataDefinition::Reference {
                    target,
                    resolved_type,
                    local_constraints,
                    local_default,
                    ..
                } => {
                    schema.insert(path.clone(), resolved_type.clone());
                    references.insert(
                        path.clone(),
                        PendingReference {
                            target: target.clone(),
                            resolved_type: resolved_type.clone(),
                            local_constraints: local_constraints.clone(),
                            local_default: local_default.clone(),
                        },
                    );
                }
            }
        }

        for (path, schema_type) in &schema {
            if values.contains_key(path) {
                continue;
            }
            if references.contains_key(path) {
                continue;
            }
            if let Some(declared) = declared_defaults.get(path) {
                values.insert(
                    path.clone(),
                    LiteralValue {
                        value: declared.clone(),
                        lemma_type: schema_type.clone(),
                    },
                );
            }
        }

        for (path, value) in values.iter_mut() {
            let Some(schema_type) = schema.get(path).cloned() else {
                continue;
            };
            match Self::coerce_literal_to_schema_type(value, &schema_type) {
                Ok(coerced) => *value = coerced,
                Err(msg) => unreachable!("Data {} incompatible: {}", path, msg),
            }
        }

        let mut data = IndexMap::new();
        for (path, rfv) in &self.data {
            let source = rfv.source().clone();
            if let Some(spec_arc) = spec_arcs.remove(path) {
                data.insert(
                    path.clone(),
                    DataDefinition::SpecRef {
                        spec: spec_arc,
                        source,
                    },
                );
            } else if let Some(pending) = references.remove(path) {
                data.insert(
                    path.clone(),
                    DataDefinition::Reference {
                        target: pending.target,
                        resolved_type: pending.resolved_type,
                        local_constraints: pending.local_constraints,
                        local_default: pending.local_default,
                        source,
                    },
                );
            } else if let Some(value) = values.remove(path) {
                data.insert(path.clone(), DataDefinition::Value { value, source });
            } else {
                let resolved_type = schema
                    .get(path)
                    .cloned()
                    .expect("non-spec-ref data has schema (value, reference, or type-only)");
                let declared_default = declared_defaults.remove(path);
                data.insert(
                    path.clone(),
                    DataDefinition::TypeDeclaration {
                        resolved_type,
                        declared_default,
                        source,
                    },
                );
            }
        }
        data
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

    /// Resolve each data-target [`DataDefinition::Reference`]'s provisional
    /// `resolved_type` into its final merged form by combining:
    ///   1. the target data's declared schema type,
    ///   2. any local `-> ...` constraints attached to the reference itself,
    ///   3. the LHS-declared type of the referencing data (when present; only
    ///      possible in a binding whose bound data has its own type
    ///      declaration in the nested spec).
    ///
    /// Rule-target references are skipped here — they are resolved later in
    /// [`Self::resolve_rule_reference_types`] using the inferred rule
    /// type, which is only available after [`infer_rule_types`] has run.
    fn resolve_data_reference_types(&mut self) -> Result<(), Vec<Error>> {
        let mut errors: Vec<Error> = Vec::new();
        let mut updates: Vec<(DataPath, LemmaType, Option<ValueKind>)> = Vec::new();

        for (reference_path, entry) in &self.data {
            let DataDefinition::Reference {
                target,
                resolved_type: provisional,
                local_constraints,
                source,
                ..
            } = entry
            else {
                continue;
            };

            let target_data_path = match target {
                ReferenceTarget::Data(path) => path,
                ReferenceTarget::Rule(_) => continue,
            };

            let Some(target_entry) = self.data.get(target_data_path) else {
                errors.push(reference_error(
                    &self.main_spec,
                    source,
                    format!(
                        "Data reference '{}' target '{}' does not exist",
                        reference_path, target_data_path
                    ),
                ));
                continue;
            };

            let Some(target_type) = target_entry.schema_type().cloned() else {
                errors.push(reference_error(
                    &self.main_spec,
                    source,
                    format!(
                        "Data reference '{}' target '{}' is a spec reference and cannot carry a value",
                        reference_path, target_data_path
                    ),
                ));
                continue;
            };

            let lhs_declared_type: Option<&LemmaType> = if provisional.is_undetermined() {
                None
            } else {
                Some(provisional)
            };

            if let Some(lhs) = lhs_declared_type {
                if let Some(msg) = reference_kind_mismatch_message(
                    lhs,
                    &target_type,
                    reference_path,
                    target_data_path,
                    "target",
                ) {
                    errors.push(reference_error(&self.main_spec, source, msg));
                    continue;
                }
            }

            // Merge: prefer LHS-declared spec when present so child-declared
            // constraints (e.g. `maximum 5` from a binding's parent type
            // chain) are enforced on the copied value at run time. Without
            // a LHS-declared type, fall back to the target's spec.
            let mut merged = match lhs_declared_type {
                Some(lhs) => lhs.clone(),
                None => target_type.clone(),
            };
            let mut captured_default: Option<ValueKind> = None;
            if let Some(constraints) = local_constraints {
                match apply_constraints_to_spec(
                    &self.main_spec,
                    merged.specifications.clone(),
                    constraints,
                    source,
                    &mut captured_default,
                ) {
                    Ok(specs) => merged.specifications = specs,
                    Err(errs) => {
                        errors.extend(errs);
                        continue;
                    }
                }
            }

            updates.push((reference_path.clone(), merged, captured_default));
        }

        for (path, new_type, new_default) in updates {
            if let Some(DataDefinition::Reference {
                resolved_type,
                local_default,
                ..
            }) = self.data.get_mut(&path)
            {
                *resolved_type = new_type;
                if new_default.is_some() {
                    *local_default = new_default;
                }
            } else {
                unreachable!("BUG: reference path disappeared between collect and update phases");
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Resolve each rule-target [`DataDefinition::Reference`]'s `resolved_type`
    /// from the inferred type of the target rule. Applies the same LHS-vs-target
    /// kind compatibility check and local `-> ...` constraint merge that
    /// [`Self::resolve_data_reference_types`] applies to data-target references.
    ///
    /// Must run AFTER [`infer_rule_types`] so each target rule's inferred type
    /// is available, and BEFORE [`check_rule_types`] so consumers see the
    /// merged reference type during validation.
    fn resolve_rule_reference_types(
        &mut self,
        computed_rule_types: &HashMap<RulePath, LemmaType>,
    ) -> Result<(), Vec<Error>> {
        let mut errors: Vec<Error> = Vec::new();
        let mut updates: Vec<(DataPath, LemmaType, Option<ValueKind>)> = Vec::new();

        for (reference_path, entry) in &self.data {
            let DataDefinition::Reference {
                target,
                resolved_type: provisional,
                local_constraints,
                source,
                ..
            } = entry
            else {
                continue;
            };

            let target_rule_path = match target {
                ReferenceTarget::Rule(path) => path,
                ReferenceTarget::Data(_) => continue,
            };

            let Some(target_type) = computed_rule_types.get(target_rule_path) else {
                errors.push(reference_error(
                    &self.main_spec,
                    source,
                    format!(
                        "Data reference '{}' target rule '{}' does not exist",
                        reference_path, target_rule_path
                    ),
                ));
                continue;
            };

            // A target rule whose inferred type is `veto` carries no concrete
            // schema kind, so a LHS declared type cannot be checked against
            // it at planning time. The runtime veto propagation in the
            // evaluator will surface the rule's veto reason directly.
            if target_type.vetoed() || target_type.is_undetermined() {
                let mut merged = target_type.clone();
                let mut captured_default: Option<ValueKind> = None;
                if let Some(constraints) = local_constraints {
                    match apply_constraints_to_spec(
                        &self.main_spec,
                        merged.specifications.clone(),
                        constraints,
                        source,
                        &mut captured_default,
                    ) {
                        Ok(specs) => merged.specifications = specs,
                        Err(errs) => {
                            errors.extend(errs);
                            continue;
                        }
                    }
                }
                updates.push((reference_path.clone(), merged, captured_default));
                continue;
            }

            let lhs_declared_type: Option<&LemmaType> = if provisional.is_undetermined() {
                None
            } else {
                Some(provisional)
            };

            if let Some(lhs) = lhs_declared_type {
                if let Some(msg) = reference_kind_mismatch_message(
                    lhs,
                    target_type,
                    reference_path,
                    target_rule_path,
                    "target rule",
                ) {
                    errors.push(reference_error(&self.main_spec, source, msg));
                    continue;
                }
            }

            // Prefer LHS-declared spec when present (see data-target merge
            // for rationale).
            let mut merged = match lhs_declared_type {
                Some(lhs) => lhs.clone(),
                None => target_type.clone(),
            };
            let mut captured_default: Option<ValueKind> = None;
            if let Some(constraints) = local_constraints {
                match apply_constraints_to_spec(
                    &self.main_spec,
                    merged.specifications.clone(),
                    constraints,
                    source,
                    &mut captured_default,
                ) {
                    Ok(specs) => merged.specifications = specs,
                    Err(errs) => {
                        errors.extend(errs);
                        continue;
                    }
                }
            }

            updates.push((reference_path.clone(), merged, captured_default));
        }

        for (path, new_type, new_default) in updates {
            if let Some(DataDefinition::Reference {
                resolved_type,
                local_default,
                ..
            }) = self.data.get_mut(&path)
            {
                *resolved_type = new_type;
                if new_default.is_some() {
                    *local_default = new_default;
                }
            } else {
                unreachable!(
                    "BUG: rule-target reference path disappeared between collect and update phases"
                );
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Add a `depends_on_rules` edge from every rule that reads a rule-target
    /// reference data path to the reference's target rule. This ensures the
    /// target rule is evaluated before the consumer (so the lazy reference
    /// resolver in the evaluator finds the result), and lets the topological
    /// sort detect cycles that flow through reference paths.
    ///
    /// Walks data-target reference chains so that a path `y: m.x` where
    /// `m.x: r` is a rule-target reference, still adds a dep edge from any
    /// consumer of `y` to `r`.
    fn add_rule_reference_dependency_edges(&mut self) {
        let reference_to_rule: HashMap<DataPath, RulePath> =
            self.transitive_reference_to_rule_map();

        if reference_to_rule.is_empty() {
            return;
        }

        let mut updates: Vec<(RulePath, RulePath)> = Vec::new();
        for (rule_path, rule_node) in &self.rules {
            let mut found: BTreeSet<RulePath> = BTreeSet::new();
            for (cond, result) in &rule_node.branches {
                if let Some(c) = cond {
                    collect_rule_reference_dependencies(c, &reference_to_rule, &mut found);
                }
                collect_rule_reference_dependencies(result, &reference_to_rule, &mut found);
            }
            for target in found {
                updates.push((rule_path.clone(), target));
            }
        }

        for (rule_path, target) in updates {
            if let Some(node) = self.rules.get_mut(&rule_path) {
                node.depends_on_rules.insert(target);
            }
        }
    }

    /// For each [`DataDefinition::Reference`] in `self.data`, follow the
    /// `Reference::Data` chain and record the eventual `Reference::Rule`
    /// target (if any). Includes direct rule-target references. Cycles
    /// among data-target references are not possible here because
    /// `compute_reference_evaluation_order` already rejected them; we still
    /// guard with a visited set as defense-in-depth.
    fn transitive_reference_to_rule_map(&self) -> HashMap<DataPath, RulePath> {
        let mut out: HashMap<DataPath, RulePath> = HashMap::new();
        for (path, def) in &self.data {
            if !matches!(def, DataDefinition::Reference { .. }) {
                continue;
            }
            let mut visited: HashSet<DataPath> = HashSet::new();
            let mut cursor: DataPath = path.clone();
            loop {
                if !visited.insert(cursor.clone()) {
                    break;
                }
                let Some(DataDefinition::Reference { target, .. }) = self.data.get(&cursor) else {
                    break;
                };
                match target {
                    ReferenceTarget::Data(next) => cursor = next.clone(),
                    ReferenceTarget::Rule(rule_path) => {
                        out.insert(path.clone(), rule_path.clone());
                        break;
                    }
                }
            }
        }
        out
    }

    /// Compute an order in which data-target references can be evaluated at
    /// runtime so each reference's target (when itself a reference) has been
    /// evaluated first. Rule-target references are intentionally excluded —
    /// they are resolved lazily on first read in the evaluator from the
    /// already-evaluated target rule's result. Cycles among data-target
    /// references are reported as planning errors.
    fn compute_reference_evaluation_order(&self) -> Result<Vec<DataPath>, Vec<Error>> {
        let reference_paths: Vec<DataPath> = self
            .data
            .iter()
            .filter_map(|(p, d)| match d {
                DataDefinition::Reference {
                    target: ReferenceTarget::Data(_),
                    ..
                } => Some(p.clone()),
                _ => None,
            })
            .collect();

        if reference_paths.is_empty() {
            return Ok(Vec::new());
        }

        let reference_set: BTreeSet<DataPath> = reference_paths.iter().cloned().collect();
        let mut in_degree: BTreeMap<DataPath, usize> = BTreeMap::new();
        let mut dependents: BTreeMap<DataPath, Vec<DataPath>> = BTreeMap::new();
        for p in &reference_paths {
            in_degree.insert(p.clone(), 0);
            dependents.insert(p.clone(), Vec::new());
        }

        for p in &reference_paths {
            let Some(DataDefinition::Reference { target, .. }) = self.data.get(p) else {
                unreachable!("BUG: reference entry lost between collect and walk");
            };
            if let ReferenceTarget::Data(target_path) = target {
                if reference_set.contains(target_path) {
                    *in_degree
                        .get_mut(p)
                        .expect("BUG: reference missing in_degree") += 1;
                    dependents
                        .get_mut(target_path)
                        .expect("BUG: reference missing dependents list")
                        .push(p.clone());
                }
            }
        }

        let mut queue: VecDeque<DataPath> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(p, _)| p.clone())
            .collect();

        let mut result: Vec<DataPath> = Vec::new();
        while let Some(path) = queue.pop_front() {
            result.push(path.clone());
            if let Some(deps) = dependents.get(&path) {
                for dependent in deps.clone() {
                    let degree = in_degree
                        .get_mut(&dependent)
                        .expect("BUG: reference dependent missing in_degree");
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent);
                    }
                }
            }
        }

        if result.len() != reference_paths.len() {
            let cycle_members: Vec<DataPath> = reference_paths
                .iter()
                .filter(|p| !result.contains(p))
                .cloned()
                .collect();
            let cycle_display: String = cycle_members
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let errors: Vec<Error> = cycle_members
                .iter()
                .filter_map(|p| {
                    self.data.get(p).map(|entry| {
                        reference_error(
                            &self.main_spec,
                            entry.source(),
                            format!("Circular data reference ({})", cycle_display),
                        )
                    })
                })
                .collect();
            return Err(errors);
        }

        Ok(result)
    }

    fn topological_sort(&self) -> Result<Vec<RulePath>, Vec<Error>> {
        let mut in_degree: BTreeMap<RulePath, usize> = BTreeMap::new();
        let mut dependents: BTreeMap<RulePath, Vec<RulePath>> = BTreeMap::new();
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

            if cycle.is_empty() {
                unreachable!(
                    "BUG: circular dependency detected but no sources could be collected ({} missing rules)",
                    missing.len()
                );
            }
            let rules_involved: String = missing
                .iter()
                .map(|rp| rp.rule.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let message = format!("Circular dependency (rules: {})", rules_involved);
            let errors: Vec<Error> = cycle
                .into_iter()
                .map(|source| {
                    Error::validation_with_context(
                        message.clone(),
                        Some(source),
                        None::<String>,
                        Some(Arc::clone(&self.main_spec)),
                        None,
                    )
                })
                .collect();
            return Err(errors);
        }

        Ok(result)
    }
}

#[derive(Debug)]
pub(crate) struct RuleNode {
    /// First branch has condition=None (default expression), subsequent branches are unless clauses.
    /// Resolved expressions (Reference -> DataPath or RulePath).
    pub branches: Vec<(Option<Expression>, Expression)>,
    pub source: Source,

    pub depends_on_rules: BTreeSet<RulePath>,

    /// Computed type of this rule's result (populated during validation)
    /// Every rule MUST have a type (Lemma is strictly typed)
    pub rule_type: LemmaType,

    /// Name of the spec this rule belongs to (for type resolution during validation)
    pub spec_name: String,
}

type ResolvedTypesMap = HashMap<Arc<LemmaSpec>, ResolvedSpecTypes>;

struct GraphBuilder<'a> {
    data: IndexMap<DataPath, DataDefinition>,
    rules: BTreeMap<RulePath, RuleNode>,
    context: &'a Context,
    local_types: ResolvedTypesMap,
    errors: Vec<Error>,
    main_spec: Arc<LemmaSpec>,
}

fn reference_error(main_spec: &Arc<LemmaSpec>, source: &Source, message: String) -> Error {
    Error::validation_with_context(
        message,
        Some(source.clone()),
        None::<String>,
        Some(Arc::clone(main_spec)),
        None,
    )
}

/// Decide whether an LHS-declared reference type and the resolved target type
/// share a compatible kind. Returns `None` when they do; returns `Some(msg)`
/// describing the mismatch otherwise.
///
/// "Same kind" requires:
/// 1. matching base type spec (number / scale / text / ratio / …) — see
///    [`LemmaType::has_same_base_type`]; and
/// 2. for scale types, matching scale family — see
///    [`LemmaType::same_scale_family`]. Two scales in different families
///    (e.g. `eur` vs `celsius`) share the `Scale` discriminant but are not
///    interchangeable values; copying one into the other would silently
///    propagate a wrong-domain quantity.
///
/// `target_kind_label` distinguishes the two callers ("target" for data
/// references, "target rule" for rule references) so the message reads
/// naturally.
fn reference_kind_mismatch_message<P: fmt::Display>(
    lhs: &LemmaType,
    target_type: &LemmaType,
    reference_path: &DataPath,
    target_path: &P,
    target_kind_label: &str,
) -> Option<String> {
    if !lhs.has_same_base_type(target_type) {
        return Some(format!(
            "Data reference '{}' type mismatch: declared as '{}' but {} '{}' is '{}'",
            reference_path,
            lhs.name(),
            target_kind_label,
            target_path,
            target_type.name(),
        ));
    }
    if lhs.is_scale() && !lhs.same_scale_family(target_type) {
        let lhs_family = lhs.scale_family_name().expect(
            "BUG: declared scale data must carry a family name; \
             anonymous scale types only arise from runtime synthesis \
             and never appear as a reference's LHS-declared type",
        );
        let target_family = target_type.scale_family_name().expect(
            "BUG: declared scale data must carry a family name; \
             anonymous scale types only arise from runtime synthesis \
             and never appear as a reference target's schema type",
        );
        return Some(format!(
            "Data reference '{}' scale family mismatch: declared as '{}' (family '{}') but {} '{}' is '{}' (family '{}')",
            reference_path,
            lhs.name(),
            lhs_family,
            target_kind_label,
            target_path,
            target_type.name(),
            target_family,
        ));
    }
    None
}

/// Fold a list of typedef-style constraints into a [`TypeSpecification`].
/// Used for both the GraphBuilder's regular TypeDeclaration path and the
/// post-build reference type-merging pass, so the underlying constraint
/// application logic stays in one place.
fn apply_constraints_to_spec(
    spec: &Arc<LemmaSpec>,
    mut specs: TypeSpecification,
    constraints: &[Constraint],
    source: &crate::Source,
    declared_default: &mut Option<ValueKind>,
) -> Result<TypeSpecification, Vec<Error>> {
    let mut errors = Vec::new();
    for (command, args) in constraints {
        let specs_clone = specs.clone();
        let mut default_before = declared_default.clone();
        match specs.apply_constraint(*command, args, &mut default_before) {
            Ok(updated_specs) => {
                specs = updated_specs;
                *declared_default = default_before;
            }
            Err(e) => {
                errors.push(Error::validation_with_context(
                    format!("Failed to apply constraint '{}': {}", command, e),
                    Some(source.clone()),
                    None::<String>,
                    Some(Arc::clone(spec)),
                    None,
                ));
                specs = specs_clone;
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(specs)
}

impl Graph {
    /// Build the dependency graph for a single spec within a pre-resolved DAG slice.
    pub(crate) fn build(
        context: &Context,
        main_spec: &Arc<LemmaSpec>,
        dag: &[Arc<LemmaSpec>],
        effective: &EffectiveDate,
    ) -> Result<(Graph, ResolvedTypesMap), Vec<Error>> {
        let mut type_resolver = TypeResolver::new(context, dag);

        let mut type_errors: Vec<Error> = Vec::new();
        for spec in dag {
            type_errors.extend(type_resolver.register_all(spec));
        }

        let (data, rules, graph_errors, local_types) = {
            let mut builder = GraphBuilder {
                data: IndexMap::new(),
                rules: BTreeMap::new(),
                context,
                local_types: HashMap::new(),
                errors: Vec::new(),
                main_spec: Arc::clone(main_spec),
            };

            builder.build_spec(
                main_spec,
                Vec::new(),
                HashMap::new(),
                effective,
                &mut type_resolver,
            )?;

            (
                builder.data,
                builder.rules,
                builder.errors,
                builder.local_types,
            )
        };

        let mut graph = Graph {
            data,
            rules,
            execution_order: Vec::new(),
            reference_evaluation_order: Vec::new(),
            main_spec: Arc::clone(main_spec),
        };

        let validation_errors = match graph.validate(&local_types) {
            Ok(()) => Vec::new(),
            Err(errors) => errors,
        };

        let mut all_errors = type_errors;
        all_errors.extend(graph_errors);
        all_errors.extend(validation_errors);

        if all_errors.is_empty() {
            Ok((graph, local_types))
        } else {
            Err(all_errors)
        }
    }

    fn validate(&mut self, resolved_types: &ResolvedTypesMap) -> Result<(), Vec<Error>> {
        let mut errors = Vec::new();

        // Structural checks (no type info needed)
        if let Err(structural_errors) = check_all_rule_references_exist(self) {
            errors.extend(structural_errors);
        }
        if let Err(collision_errors) = check_data_and_rule_name_collisions(self) {
            errors.extend(collision_errors);
        }

        // Phase 1: Resolve data-target reference types now that all data
        // definitions (across all specs) are populated. Rule-target references
        // are resolved in Phase 4 once the target rule's type is inferred.
        if let Err(reference_errors) = self.resolve_data_reference_types() {
            errors.extend(reference_errors);
        }

        // Compute the data-target reference evaluation (copy) order. Rule-target
        // references are resolved lazily at evaluation time — they do not
        // participate in the prepop copy loop.
        let reference_order = match self.compute_reference_evaluation_order() {
            Ok(order) => order,
            Err(circular_errors) => {
                errors.extend(circular_errors);
                return Err(errors);
            }
        };

        // Phase 2: Inject rule-rule dependency edges for rule-target references.
        // A rule R that reads a data path D where D is `Reference(target: rule T)`
        // must be evaluated AFTER T so the lazy resolver can read T's result.
        // This must happen before topological_sort so cycles through reference
        // paths are detected.
        self.add_rule_reference_dependency_edges();

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

        // Phase 3: Infer types (pure, no errors). Looks through rule-target
        // references by consulting `computed_rule_types` for the target rule.
        let inferred_types = infer_rule_types(self, &execution_order, resolved_types);

        // Phase 4: Now that target rule types are known, materialize each
        // rule-target reference's `resolved_type` (LHS check + target type +
        // local constraints), so check_rule_types and downstream consumers
        // see a real type on the reference path.
        if let Err(rule_reference_errors) = self.resolve_rule_reference_types(&inferred_types) {
            errors.extend(rule_reference_errors);
        }

        // Phase 5: Check types (pure, returns Result)
        if let Err(type_errors) =
            check_rule_types(self, &execution_order, &inferred_types, resolved_types)
        {
            errors.extend(type_errors);
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        // Phase 6: Apply (only on full success)
        apply_inferred_types(self, inferred_types);
        self.execution_order = execution_order;
        self.reference_evaluation_order = reference_order;
        Ok(())
    }
}

impl<'a> GraphBuilder<'a> {
    fn engine_error(&self, message: impl Into<String>, source: &Source) -> Error {
        Error::validation_with_context(
            message.into(),
            Some(source.clone()),
            None::<String>,
            Some(Arc::clone(&self.main_spec)),
            None,
        )
    }

    fn process_meta_fields(&mut self, spec: &LemmaSpec) {
        let mut seen = HashSet::new();
        for field in &spec.meta_fields {
            // Validate built-in keys
            if field.key == "title" && !matches!(field.value, MetaValue::Literal(Value::Text(_))) {
                self.errors.push(self.engine_error(
                    "Meta 'title' must be a text literal",
                    &field.source_location,
                ));
            }

            if !seen.insert(field.key.clone()) {
                self.errors.push(self.engine_error(
                    format!("Duplicate meta key '{}'", field.key),
                    &field.source_location,
                ));
            }
        }
    }

    fn resolve_spec_ref(
        &self,
        spec_ref: &ast::SpecRef,
        effective: &EffectiveDate,
    ) -> Result<Arc<LemmaSpec>, Error> {
        discovery::resolve_spec_ref(
            self.context,
            spec_ref,
            effective,
            &self.main_spec.name,
            None,
            Some(Arc::clone(&self.main_spec)),
        )
    }

    /// Validate a data binding path by walking through spec references, and
    /// convert the binding's right-hand side into a [`BindingValue`] that the
    /// nested spec can interpret without access to the outer spec.
    ///
    /// The binding key (full path as data names from root) uses data names only
    /// (no spec names) so that spec ref bindings don't cause mismatches.
    fn resolve_data_binding(
        &mut self,
        data: &LemmaData,
        current_segment_names: &[String],
        parent_spec: &Arc<LemmaSpec>,
        effective: &EffectiveDate,
    ) -> Option<(Vec<String>, BindingValue, Source)> {
        let binding_path_display = format!(
            "{}.{}",
            data.reference.segments.join("."),
            data.reference.name
        );

        let mut walk_spec = Arc::clone(parent_spec);

        for segment in &data.reference.segments {
            let Some(seg_data) = walk_spec
                .data
                .iter()
                .find(|f| f.reference.segments.is_empty() && f.reference.name == *segment)
            else {
                self.errors.push(self.engine_error(
                    format!(
                        "Data binding path '{}': data '{}' not found in spec '{}'",
                        binding_path_display, segment, walk_spec.name
                    ),
                    &data.source_location,
                ));
                return None;
            };

            let spec_ref = match &seg_data.value {
                ParsedDataValue::SpecReference(sr) => sr,
                _ => {
                    self.errors.push(self.engine_error(
                        format!(
                            "Data binding path '{}': '{}' in spec '{}' is not a spec reference",
                            binding_path_display, segment, walk_spec.name
                        ),
                        &data.source_location,
                    ));
                    return None;
                }
            };

            walk_spec = match self.resolve_spec_ref(spec_ref, effective) {
                Ok(arc) => arc,
                Err(e) => {
                    self.errors.push(e);
                    return None;
                }
            };
        }

        // Build the binding key: current_segment_names ++ data.reference.segments ++ [data.reference.name]
        let mut binding_key: Vec<String> = current_segment_names.to_vec();
        binding_key.extend(data.reference.segments.iter().cloned());
        binding_key.push(data.reference.name.clone());

        let binding_value = match &data.value {
            ParsedDataValue::Literal(v) => BindingValue::Literal(v.clone()),
            ParsedDataValue::Reference {
                target,
                constraints,
            } => {
                let resolved_target = self.resolve_reference_target_in_spec(
                    target,
                    &data.source_location,
                    parent_spec,
                    current_segment_names,
                    effective,
                )?;
                BindingValue::Reference {
                    target: resolved_target,
                    constraints: constraints.clone(),
                }
            }
            ParsedDataValue::TypeDeclaration { .. } | ParsedDataValue::SpecReference(_) => {
                unreachable!(
                    "BUG: build_data_bindings must reject TypeDeclaration/SpecReference bindings before calling resolve_data_binding"
                );
            }
        };

        Some((binding_key, binding_value, data.source_location.clone()))
    }

    /// Resolve a parsed [`ast::Reference`] appearing on the RHS of a `data x: ref`
    /// assignment against the scope of `containing_spec_arc`. Returns an
    /// [`ReferenceTarget`] pointing at a data path or rule path. Errors push into
    /// `self.errors`; this function returns `None` on failure (and does not
    /// return a proper `Result` because it mirrors `resolve_path_segments`'s
    /// side-effecting convention so the two can compose cleanly).
    fn resolve_reference_target_in_spec(
        &mut self,
        reference: &ast::Reference,
        reference_source: &Source,
        containing_spec_arc: &Arc<LemmaSpec>,
        containing_segments_names: &[String],
        effective: &EffectiveDate,
    ) -> Option<ReferenceTarget> {
        let containing_data_map: HashMap<String, LemmaData> = containing_spec_arc
            .data
            .iter()
            .filter(|d| d.reference.is_local())
            .map(|d| (d.reference.name.clone(), d.clone()))
            .collect();

        let containing_rule_names: HashSet<&str> = containing_spec_arc
            .rules
            .iter()
            .map(|r| r.name.as_str())
            .collect();

        let containing_segments: Vec<PathSegment> = containing_segments_names
            .iter()
            .map(|name| PathSegment {
                data: name.clone(),
                spec: containing_spec_arc.name.clone(),
            })
            .collect();

        if reference.segments.is_empty() {
            let is_data = containing_data_map.contains_key(&reference.name);
            let is_rule = containing_rule_names.contains(reference.name.as_str());
            if is_data && is_rule {
                self.errors.push(self.engine_error(
                    format!(
                        "Reference target '{}' is ambiguous: both a data and a rule in spec '{}'",
                        reference.name, containing_spec_arc.name
                    ),
                    reference_source,
                ));
                return None;
            }
            if is_data {
                return Some(ReferenceTarget::Data(DataPath {
                    segments: containing_segments,
                    data: reference.name.clone(),
                }));
            }
            if is_rule {
                return Some(ReferenceTarget::Rule(RulePath {
                    segments: containing_segments,
                    rule: reference.name.clone(),
                }));
            }
            self.errors.push(self.engine_error(
                format!(
                    "Reference target '{}' not found in spec '{}'",
                    reference.name, containing_spec_arc.name
                ),
                reference_source,
            ));
            return None;
        }

        let (resolved_segments, target_spec_arc) = self.resolve_path_segments(
            &reference.segments,
            reference_source,
            containing_data_map,
            containing_segments,
            effective,
        )?;

        let target_data_names: HashSet<&str> = target_spec_arc
            .data
            .iter()
            .filter(|d| d.reference.is_local())
            .map(|d| d.reference.name.as_str())
            .collect();
        let target_rule_names: HashSet<&str> = target_spec_arc
            .rules
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        let is_data = target_data_names.contains(reference.name.as_str());
        let is_rule = target_rule_names.contains(reference.name.as_str());

        if is_data && is_rule {
            self.errors.push(self.engine_error(
                format!(
                    "Reference target '{}' is ambiguous: both a data and a rule in spec '{}'",
                    reference.name, target_spec_arc.name
                ),
                reference_source,
            ));
            return None;
        }
        if is_data {
            return Some(ReferenceTarget::Data(DataPath {
                segments: resolved_segments,
                data: reference.name.clone(),
            }));
        }
        if is_rule {
            return Some(ReferenceTarget::Rule(RulePath {
                segments: resolved_segments,
                rule: reference.name.clone(),
            }));
        }

        self.errors.push(self.engine_error(
            format!(
                "Reference target '{}' not found in spec '{}'",
                reference.name, target_spec_arc.name
            ),
            reference_source,
        ));
        None
    }

    /// Build the data bindings declared in a spec.
    ///
    /// For each cross-spec data (reference.segments is non-empty), validate the path
    /// and collect into a DataBindings map. Rejects TypeDeclaration binding values and
    /// duplicate bindings targeting the same path.
    fn build_data_bindings(
        &mut self,
        spec: &LemmaSpec,
        current_segment_names: &[String],
        spec_arc: &Arc<LemmaSpec>,
        effective: &EffectiveDate,
    ) -> Result<DataBindings, Vec<Error>> {
        let mut bindings: DataBindings = HashMap::new();
        let mut errors: Vec<Error> = Vec::new();

        for data in &spec.data {
            if data.reference.segments.is_empty() {
                continue; // Local data are not bindings
            }

            let binding_path_display = format!(
                "{}.{}",
                data.reference.segments.join("."),
                data.reference.name
            );

            // Reject spec reference as binding value — spec injection is not supported
            if matches!(&data.value, ParsedDataValue::SpecReference { .. }) {
                errors.push(self.engine_error(
                    format!(
                        "Data binding '{}' cannot override a spec reference — only literal values can be bound to nested data",
                        binding_path_display
                    ),
                    &data.source_location,
                ));
                continue;
            }

            // Reject TypeDeclaration as binding value
            if matches!(&data.value, ParsedDataValue::TypeDeclaration { .. }) {
                errors.push(self.engine_error(
                    format!(
                        "Data binding '{}' must provide a literal value, not a type declaration",
                        binding_path_display
                    ),
                    &data.source_location,
                ));
                continue;
            }

            if let Some((binding_key, binding_value, source)) =
                self.resolve_data_binding(data, current_segment_names, spec_arc, effective)
            {
                if let Some((_, existing_source)) = bindings.get(&binding_key) {
                    errors.push(self.engine_error(
                        format!(
                            "Duplicate data binding for '{}' (previously bound at {}:{})",
                            binding_key.join("."),
                            existing_source.attribute,
                            existing_source.span.line
                        ),
                        &data.source_location,
                    ));
                } else {
                    bindings.insert(binding_key, (binding_value, source));
                }
            }
            // resolve_data_binding failures are pushed into self.errors already.
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(bindings)
    }

    /// Add a single local data to the graph.
    ///
    /// Determines the effective value by checking `data_bindings` for an entry at
    /// the data's path. If a binding exists, uses the bound value; otherwise uses
    /// the data's own value. Reports an error on duplicate data.
    #[allow(clippy::too_many_arguments)]
    fn add_data(
        &mut self,
        data: &LemmaData,
        current_segments: &[PathSegment],
        data_bindings: &DataBindings,
        current_spec_arc: &Arc<LemmaSpec>,
        used_binding_keys: &mut HashSet<Vec<String>>,
        effective: &EffectiveDate,
    ) {
        let data_path = DataPath {
            segments: current_segments.to_vec(),
            data: data.reference.name.clone(),
        };

        // Check for duplicates
        if self.data.contains_key(&data_path) {
            self.errors.push(self.engine_error(
                format!("Duplicate data '{}'", data_path.data),
                &data.source_location,
            ));
            return;
        }

        // Build the binding key for this data: segment data names + data name
        let binding_key: Vec<String> = current_segments
            .iter()
            .map(|s| s.data.clone())
            .chain(std::iter::once(data.reference.name.clone()))
            .collect();

        // A binding (if any) overrides the data's own RHS. We track the binding
        // separately from the data's own value because `BindingValue` (resolved)
        // and `ParsedDataValue` (raw AST) are different types.
        let binding_override: Option<(BindingValue, Source)> =
            data_bindings.get(&binding_key).map(|(v, s)| {
                used_binding_keys.insert(binding_key.clone());
                (v.clone(), s.clone())
            });

        let (original_schema_type, original_declared_default) =
            if matches!(&data.value, ParsedDataValue::TypeDeclaration { .. }) {
                let resolved = self
                    .local_types
                    .get(current_spec_arc)
                    .expect("BUG: no resolved types for spec during add_local_data");
                let lemma_type = resolved
                    .named_types
                    .get(&data.reference.name)
                    .expect("BUG: type not in named_types — TypeResolver should have registered it")
                    .clone();
                let declared = resolved
                    .declared_defaults
                    .get(&data.reference.name)
                    .cloned();
                (Some(lemma_type), declared)
            } else {
                (None, None)
            };

        if let Some((binding_value, binding_source)) = binding_override {
            self.add_data_from_binding(
                data_path,
                binding_value,
                binding_source,
                original_schema_type,
                current_spec_arc,
            );
            return;
        }

        let effective_source = data.source_location.clone();

        match &data.value {
            ParsedDataValue::Literal(value) => {
                self.insert_literal_data(
                    data_path,
                    value,
                    original_schema_type,
                    effective_source,
                    current_spec_arc,
                );
            }
            ParsedDataValue::TypeDeclaration { .. } => {
                let resolved_type = original_schema_type.unwrap_or_else(|| {
                    unreachable!(
                        "BUG: TypeDeclaration effective value without original_schema_type"
                    )
                });

                self.data.insert(
                    data_path,
                    DataDefinition::TypeDeclaration {
                        resolved_type,
                        declared_default: original_declared_default,
                        source: effective_source,
                    },
                );
            }
            ParsedDataValue::SpecReference(spec_ref) => {
                let effective_spec_arc = match self.resolve_spec_ref(spec_ref, effective) {
                    Ok(arc) => arc,
                    Err(e) => {
                        self.errors.push(e);
                        return;
                    }
                };

                self.data.insert(
                    data_path,
                    DataDefinition::SpecRef {
                        spec: Arc::clone(&effective_spec_arc),
                        source: effective_source,
                    },
                );
            }
            ParsedDataValue::Reference {
                target,
                constraints,
            } => {
                let current_segment_names: Vec<String> =
                    current_segments.iter().map(|s| s.data.clone()).collect();
                let Some(resolved_target) = self.resolve_reference_target_in_spec(
                    target,
                    &effective_source,
                    current_spec_arc,
                    &current_segment_names,
                    effective,
                ) else {
                    return;
                };
                // Reference type is resolved in a later pass once all data+rule
                // types are known. Use LHS declared type if present, otherwise
                // a placeholder that must be filled before validation.
                let provisional_type = original_schema_type
                    .clone()
                    .unwrap_or_else(LemmaType::undetermined_type);
                self.data.insert(
                    data_path,
                    DataDefinition::Reference {
                        target: resolved_target,
                        resolved_type: provisional_type,
                        local_constraints: constraints.clone(),
                        local_default: None,
                        source: effective_source,
                    },
                );
            }
        }
    }

    /// Inserts a literal-value data definition using the given literal.
    /// Shared between the literal path of `add_data` and the literal path of
    /// a binding-provided value (bindings can only be literals or references).
    fn insert_literal_data(
        &mut self,
        data_path: DataPath,
        value: &ast::Value,
        declared_schema_type: Option<LemmaType>,
        effective_source: Source,
        current_spec_arc: &Arc<LemmaSpec>,
    ) {
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
            Value::Scale(_, unit) => {
                match self
                    .local_types
                    .get(current_spec_arc)
                    .and_then(|dt| dt.unit_index.get(unit))
                {
                    Some(lt) => lt.clone(),
                    None => {
                        self.errors.push(self.engine_error(
                            format!("Scale literal uses unknown unit '{}' for this spec", unit),
                            &effective_source,
                        ));
                        return;
                    }
                }
            }
            Value::Boolean(_) => primitive_boolean().clone(),
            Value::Date(_) => primitive_date().clone(),
            Value::Time(_) => primitive_time().clone(),
            Value::Duration(_, _) => primitive_duration().clone(),
            Value::Ratio(_, _) => primitive_ratio().clone(),
        };
        let schema_type = declared_schema_type.unwrap_or(inferred_type);
        let literal_value = LiteralValue {
            value: semantic_value,
            lemma_type: schema_type,
        };
        self.data.insert(
            data_path,
            DataDefinition::Value {
                value: literal_value,
                source: effective_source,
            },
        );
    }

    /// Apply a binding override to insert the bound data's definition.
    /// Bindings are pre-resolved — literal values or reference targets.
    fn add_data_from_binding(
        &mut self,
        data_path: DataPath,
        binding_value: BindingValue,
        binding_source: Source,
        declared_schema_type: Option<LemmaType>,
        current_spec_arc: &Arc<LemmaSpec>,
    ) {
        match binding_value {
            BindingValue::Literal(value) => {
                self.insert_literal_data(
                    data_path,
                    &value,
                    declared_schema_type,
                    binding_source,
                    current_spec_arc,
                );
            }
            BindingValue::Reference {
                target,
                constraints,
            } => {
                let provisional_type =
                    declared_schema_type.unwrap_or_else(LemmaType::undetermined_type);
                self.data.insert(
                    data_path,
                    DataDefinition::Reference {
                        target,
                        resolved_type: provisional_type,
                        local_constraints: constraints,
                        local_default: None,
                        source: binding_source,
                    },
                );
            }
        }
    }

    /// Returns (path_segments, last_resolved_spec_arc) on success.
    fn resolve_path_segments(
        &mut self,
        segments: &[String],
        reference_source: &Source,
        mut current_data_map: HashMap<String, LemmaData>,
        mut path_segments: Vec<PathSegment>,
        effective: &EffectiveDate,
    ) -> Option<(Vec<PathSegment>, Arc<LemmaSpec>)> {
        let mut last_arc: Option<Arc<LemmaSpec>> = None;

        for segment in segments.iter() {
            let data_ref =
                match current_data_map.get(segment) {
                    Some(f) => f,
                    None => {
                        self.errors.push(self.engine_error(
                            format!("Data '{}' not found", segment),
                            reference_source,
                        ));
                        return None;
                    }
                };

            if let ParsedDataValue::SpecReference(original_spec_ref) = &data_ref.value {
                let arc = match self.resolve_spec_ref(original_spec_ref, effective) {
                    Ok(a) => a,
                    Err(e) => {
                        self.errors.push(e);
                        return None;
                    }
                };

                path_segments.push(PathSegment {
                    data: segment.clone(),
                    spec: arc.name.clone(),
                });
                current_data_map = arc
                    .data
                    .iter()
                    .map(|f| (f.reference.name.clone(), f.clone()))
                    .collect();
                last_arc = Some(arc);
            } else {
                self.errors.push(self.engine_error(
                    format!("Data '{}' is not a spec reference", segment),
                    reference_source,
                ));
                return None;
            }
        }

        let final_arc = last_arc.unwrap_or_else(|| {
            unreachable!(
                "BUG: resolve_path_segments called with empty segments should not reach here"
            )
        });
        Some((path_segments, final_arc))
    }

    fn build_spec(
        &mut self,
        spec_arc: &Arc<LemmaSpec>,
        current_segments: Vec<PathSegment>,
        data_bindings: DataBindings,
        effective: &EffectiveDate,
        type_resolver: &mut TypeResolver<'a>,
    ) -> Result<(), Vec<Error>> {
        let spec = spec_arc.as_ref();

        if current_segments.is_empty() {
            self.process_meta_fields(spec);
        }

        // Step 0: Cross-version self-reference check.
        // A spec must not reference any version of itself (same base name).
        for data in spec.data.iter() {
            if let ParsedDataValue::SpecReference(spec_ref) = &data.value {
                if spec_ref.name == spec.name {
                    self.errors.push(self.engine_error(
                        format!(
                            "spec '{}' cannot reference '{}' (same base name)",
                            spec.name, spec_ref
                        ),
                        &data.source_location,
                    ));
                }
            }
        }
        let current_segment_names: Vec<String> =
            current_segments.iter().map(|s| s.data.clone()).collect();

        // Step 2: Build data bindings declared in this spec (for passing to referenced specs)
        let this_spec_bindings =
            match self.build_data_bindings(spec, &current_segment_names, spec_arc, effective) {
                Ok(bindings) => bindings,
                Err(errors) => {
                    self.errors.extend(errors);
                    HashMap::new()
                }
            };

        // Build data_map for rule resolution and other lookups
        let data_map: HashMap<String, &LemmaData> = spec
            .data
            .iter()
            .map(|data| (data.reference.name.clone(), data))
            .collect();

        if !self.local_types.contains_key(spec_arc) {
            match type_resolver.resolve_and_validate(spec_arc, effective) {
                Ok(resolved_types) => {
                    self.local_types
                        .insert(Arc::clone(spec_arc), resolved_types);
                }
                Err(es) => {
                    self.errors.extend(es);
                    return Ok(());
                }
            }
        }

        for data in &spec.data {
            if let ParsedDataValue::TypeDeclaration {
                from: Some(from_ref),
                ..
            } = &data.value
            {
                match self.resolve_spec_ref(from_ref, effective) {
                    Ok(source_arc) => {
                        if let std::collections::hash_map::Entry::Vacant(e) =
                            self.local_types.entry(source_arc)
                        {
                            match type_resolver.resolve_and_validate(e.key(), effective) {
                                Ok(resolved_types) => {
                                    e.insert(resolved_types);
                                }
                                Err(es) => self.errors.extend(es),
                            }
                        }
                    }
                    Err(e) => self.errors.push(e),
                }
            }
        }

        // Step 4: Add local data using caller's data_bindings
        let mut used_binding_keys: HashSet<Vec<String>> = HashSet::new();
        for data in &spec.data {
            if !data.reference.segments.is_empty() {
                continue; // Skip binding data (processed in step 2)
            }
            if let ParsedDataValue::SpecReference(spec_ref) = &data.value {
                if spec_ref.name == spec.name {
                    continue; // Self-reference — error already reported in step 0
                }
            }
            self.add_data(
                data,
                &current_segments,
                &data_bindings,
                spec_arc,
                &mut used_binding_keys,
                effective,
            );
        }

        for data in &spec.data {
            if !data.reference.segments.is_empty() {
                continue;
            }
            if let ParsedDataValue::SpecReference(spec_ref) = &data.value {
                if spec_ref.name == spec.name {
                    continue; // Self-reference — error already reported in step 0
                }
                let nested_effective = spec_ref.at(effective);
                let nested_arc = match self.resolve_spec_ref(spec_ref, effective) {
                    Ok(arc) => arc,
                    Err(e) => {
                        self.errors.push(e);
                        continue;
                    }
                };
                let mut nested_segments = current_segments.clone();
                nested_segments.push(PathSegment {
                    data: data.reference.name.clone(),
                    spec: nested_arc.name.clone(),
                });

                let nested_segment_names: Vec<String> =
                    nested_segments.iter().map(|s| s.data.clone()).collect();
                let mut combined_bindings = this_spec_bindings.clone();
                for (key, value_and_source) in &data_bindings {
                    if key.len() > nested_segment_names.len()
                        && key[..nested_segment_names.len()] == nested_segment_names[..]
                        && !combined_bindings.contains_key(key)
                    {
                        combined_bindings.insert(key.clone(), value_and_source.clone());
                    }
                }

                if let Err(errs) = self.build_spec(
                    &nested_arc,
                    nested_segments,
                    combined_bindings,
                    &nested_effective,
                    type_resolver,
                ) {
                    self.errors.extend(errs);
                }
            }
        }

        // Check for unused data bindings that targeted this spec's data
        // Only check bindings at exactly this depth (deeper bindings are passed through)
        let expected_key_len = current_segments.len() + 1;
        for (binding_key, (_, binding_source)) in &data_bindings {
            if binding_key.len() == expected_key_len
                && binding_key[..current_segments.len()]
                    .iter()
                    .zip(current_segments.iter())
                    .all(|(a, b)| a == &b.data)
                && !used_binding_keys.contains(binding_key)
            {
                self.errors.push(self.engine_error(
                    format!(
                        "Data binding targets a data that does not exist in the referenced spec: '{}'",
                        binding_key.join(".")
                    ),
                    binding_source,
                ));
            }
        }

        let rule_names: HashSet<&str> = spec.rules.iter().map(|r| r.name.as_str()).collect();
        for rule in &spec.rules {
            self.add_rule(
                rule,
                spec_arc,
                &data_map,
                &current_segments,
                &rule_names,
                effective,
            );
        }

        Ok(())
    }

    fn add_rule(
        &mut self,
        rule: &LemmaRule,
        current_spec_arc: &Arc<LemmaSpec>,
        data_map: &HashMap<String, &LemmaData>,
        current_segments: &[PathSegment],
        rule_names: &HashSet<&str>,
        effective: &EffectiveDate,
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
        let mut depends_on_rules = BTreeSet::new();

        let converted_expression = match self.convert_expression_and_extract_dependencies(
            &rule.expression,
            current_spec_arc,
            data_map,
            current_segments,
            &mut depends_on_rules,
            rule_names,
            effective,
        ) {
            Some(expr) => expr,
            None => return,
        };
        branches.push((None, converted_expression));

        for unless_clause in &rule.unless_clauses {
            let converted_condition = match self.convert_expression_and_extract_dependencies(
                &unless_clause.condition,
                current_spec_arc,
                data_map,
                current_segments,
                &mut depends_on_rules,
                rule_names,
                effective,
            ) {
                Some(expr) => expr,
                None => return,
            };
            let converted_result = match self.convert_expression_and_extract_dependencies(
                &unless_clause.result,
                current_spec_arc,
                data_map,
                current_segments,
                &mut depends_on_rules,
                rule_names,
                effective,
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
            rule_type: LemmaType::veto_type(),
            spec_name: current_spec_arc.name.clone(),
        };

        self.rules.insert(rule_path, rule_node);
    }

    /// Converts left and right expressions and accumulates rule dependencies.
    #[allow(clippy::too_many_arguments)]
    fn convert_binary_operands(
        &mut self,
        left: &ast::Expression,
        right: &ast::Expression,
        current_spec_arc: &Arc<LemmaSpec>,
        data_map: &HashMap<String, &LemmaData>,
        current_segments: &[PathSegment],
        depends_on_rules: &mut BTreeSet<RulePath>,
        rule_names: &HashSet<&str>,
        effective: &EffectiveDate,
    ) -> Option<(Expression, Expression)> {
        let converted_left = self.convert_expression_and_extract_dependencies(
            left,
            current_spec_arc,
            data_map,
            current_segments,
            depends_on_rules,
            rule_names,
            effective,
        )?;
        let converted_right = self.convert_expression_and_extract_dependencies(
            right,
            current_spec_arc,
            data_map,
            current_segments,
            depends_on_rules,
            rule_names,
            effective,
        )?;
        Some((converted_left, converted_right))
    }

    /// Converts an AST expression into a resolved expression and records any rule references.
    #[allow(clippy::too_many_arguments)]
    fn convert_expression_and_extract_dependencies(
        &mut self,
        expr: &ast::Expression,
        current_spec_arc: &Arc<LemmaSpec>,
        data_map: &HashMap<String, &LemmaData>,
        current_segments: &[PathSegment],
        depends_on_rules: &mut BTreeSet<RulePath>,
        rule_names: &HashSet<&str>,
        effective: &EffectiveDate,
    ) -> Option<Expression> {
        let expr_src = expr
            .source_location
            .as_ref()
            .expect("BUG: AST expression missing source location");
        match &expr.kind {
            ast::ExpressionKind::Reference(r) => {
                let expr_source = expr_src;
                let (segments, target_arc_opt) = if r.segments.is_empty() {
                    (current_segments.to_vec(), None)
                } else {
                    let data_map_owned: HashMap<String, LemmaData> = data_map
                        .iter()
                        .map(|(k, v)| (k.clone(), (*v).clone()))
                        .collect();
                    let (segs, arc) = self.resolve_path_segments(
                        &r.segments,
                        expr_source,
                        data_map_owned,
                        current_segments.to_vec(),
                        effective,
                    )?;
                    (segs, Some(arc))
                };

                let (is_data, is_rule, target_spec_name_opt) = match &target_arc_opt {
                    None => {
                        let is_data = data_map.contains_key(&r.name);
                        let is_rule = rule_names.contains(r.name.as_str());
                        (is_data, is_rule, None)
                    }
                    Some(target_arc) => {
                        let target_spec = target_arc.as_ref();
                        let target_data_names: HashSet<&str> = target_spec
                            .data
                            .iter()
                            .filter(|f| f.reference.is_local())
                            .map(|f| f.reference.name.as_str())
                            .collect();
                        let target_rule_names: HashSet<&str> =
                            target_spec.rules.iter().map(|r| r.name.as_str()).collect();
                        let is_data = target_data_names.contains(r.name.as_str());
                        let is_rule = target_rule_names.contains(r.name.as_str());
                        (is_data, is_rule, Some(target_spec.name.as_str()))
                    }
                };

                if is_data && is_rule {
                    self.errors.push(self.engine_error(
                        format!("'{}' is both a data and a rule", r.name),
                        expr_source,
                    ));
                    return None;
                }
                if is_data {
                    let data_path = DataPath {
                        segments,
                        data: r.name.clone(),
                    };
                    return Some(Expression {
                        kind: ExpressionKind::DataPath(data_path),
                        source_location: expr.source_location.clone(),
                    });
                }
                if is_rule {
                    let rule_path = RulePath {
                        segments,
                        rule: r.name.clone(),
                    };
                    depends_on_rules.insert(rule_path.clone());
                    return Some(Expression {
                        kind: ExpressionKind::RulePath(rule_path),
                        source_location: expr.source_location.clone(),
                    });
                }
                let msg = match target_spec_name_opt {
                    Some(s) => format!("Reference '{}' not found in spec '{}'", r.name, s),
                    None => format!("Reference '{}' not found", r.name),
                };
                self.errors.push(self.engine_error(msg, expr_source));
                None
            }

            ast::ExpressionKind::LogicalAnd(left, right) => {
                let (l, r) = self.convert_binary_operands(
                    left,
                    right,
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
                )?;
                Some(Expression {
                    kind: ExpressionKind::LogicalAnd(Arc::new(l), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::Arithmetic(left, op, right) => {
                let (l, r) = self.convert_binary_operands(
                    left,
                    right,
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
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
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
                )?;
                Some(Expression {
                    kind: ExpressionKind::Comparison(Arc::new(l), op.clone(), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::UnitConversion(value, target) => {
                let converted_value = self.convert_expression_and_extract_dependencies(
                    value,
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
                )?;

                let resolved_spec_types = self.local_types.get(current_spec_arc);
                let unit_index = resolved_spec_types.map(|dt| &dt.unit_index);
                let semantic_target = match conversion_target_to_semantic(target, unit_index) {
                    Ok(t) => t,
                    Err(msg) => {
                        // When there is no unit index (e.g. primitive context), surface the
                        // conversion error without a "valid units" list.
                        let full_msg = unit_index
                            .map(|idx| {
                                let valid: Vec<&str> = idx.keys().map(String::as_str).collect();
                                format!("{} Valid units: {}", msg, valid.join(", "))
                            })
                            .unwrap_or(msg);
                        self.errors.push(Error::validation_with_context(
                            full_msg,
                            expr.source_location.clone(),
                            None::<String>,
                            Some(Arc::clone(&self.main_spec)),
                            None,
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
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
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
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
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
                    Value::Scale(_, unit) => {
                        match self
                            .local_types
                            .get(current_spec_arc)
                            .and_then(|dt| dt.unit_index.get(unit))
                        {
                            Some(lt) => lt.clone(),
                            None => {
                                self.errors.push(self.engine_error(
                                    format!(
                                        "Scale literal uses unknown unit '{}' for this spec",
                                        unit
                                    ),
                                    expr_src,
                                ));
                                return None;
                            }
                        }
                    }
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

            ast::ExpressionKind::UnresolvedUnitLiteral(value, unit) => {
                if let Some(lt) = self
                    .local_types
                    .get(current_spec_arc)
                    .and_then(|dt| dt.unit_index.get(unit))
                {
                    let semantic_value = ValueKind::Scale(*value, unit.clone());
                    let literal_value = LiteralValue {
                        value: semantic_value,
                        lemma_type: lt.clone(),
                    };
                    Some(Expression {
                        kind: ExpressionKind::Literal(Box::new(literal_value)),
                        source_location: expr.source_location.clone(),
                    })
                } else {
                    self.errors
                        .push(self.engine_error(format!("Unknown unit '{}'", unit), expr_src));
                    None
                }
            }

            ast::ExpressionKind::Now => Some(Expression {
                kind: ExpressionKind::Now,
                source_location: expr.source_location.clone(),
            }),

            ast::ExpressionKind::DateRelative(kind, date_expr, tolerance) => {
                let converted_date = self.convert_expression_and_extract_dependencies(
                    date_expr,
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
                )?;
                let converted_tolerance = match tolerance {
                    Some(tol) => Some(Arc::new(self.convert_expression_and_extract_dependencies(
                        tol,
                        current_spec_arc,
                        data_map,
                        current_segments,
                        depends_on_rules,
                        rule_names,
                        effective,
                    )?)),
                    None => None,
                };
                Some(Expression {
                    kind: ExpressionKind::DateRelative(
                        *kind,
                        Arc::new(converted_date),
                        converted_tolerance,
                    ),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::DateCalendar(kind, unit, date_expr) => {
                let converted_date = self.convert_expression_and_extract_dependencies(
                    date_expr,
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
                )?;
                Some(Expression {
                    kind: ExpressionKind::DateCalendar(*kind, *unit, Arc::new(converted_date)),
                    source_location: expr.source_location.clone(),
                })
            }
        }
    }
}

/// Find resolved types for a spec by name. Since per-slice resolution registers
/// at most one version per spec name, this is a simple name match.
fn find_types_by_name<'b>(
    types: &'b ResolvedTypesMap,
    name: &str,
) -> Option<&'b ResolvedSpecTypes> {
    types
        .iter()
        .find(|(spec, _)| spec.name == name)
        .map(|(_, t)| t)
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
        (TypeSpecification::Veto { .. }, _) | (_, TypeSpecification::Veto { .. }) => {
            LemmaType::veto_type()
        }
        (TypeSpecification::Undetermined, _) => LemmaType::undetermined_type(),

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
                LemmaType::undetermined_type()
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
/// Returns `LemmaType::undetermined_type()` when a type cannot be determined (e.g. unknown data).
fn infer_expression_type(
    expression: &Expression,
    graph: &Graph,
    computed_rule_types: &HashMap<RulePath, LemmaType>,
    resolved_types: &ResolvedTypesMap,
    spec_name: &str,
) -> LemmaType {
    match &expression.kind {
        ExpressionKind::Literal(literal_value) => literal_value.as_ref().get_type().clone(),

        ExpressionKind::DataPath(data_path) => {
            infer_data_type(data_path, graph, computed_rule_types)
        }

        ExpressionKind::RulePath(rule_path) => computed_rule_types
            .get(rule_path)
            .cloned()
            .unwrap_or_else(LemmaType::undetermined_type),

        ExpressionKind::LogicalAnd(left, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_name);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_name);
            if left_type.vetoed() || right_type.vetoed() {
                return LemmaType::veto_type();
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            primitive_boolean().clone()
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let operand_type = infer_expression_type(
                operand,
                graph,
                computed_rule_types,
                resolved_types,
                spec_name,
            );
            if operand_type.vetoed() {
                return LemmaType::veto_type();
            }
            if operand_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            primitive_boolean().clone()
        }

        ExpressionKind::Comparison(left, _op, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_name);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_name);
            if left_type.vetoed() || right_type.vetoed() {
                return LemmaType::veto_type();
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            primitive_boolean().clone()
        }

        ExpressionKind::Arithmetic(left, _operator, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_name);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_name);
            compute_arithmetic_result_type(left_type, right_type)
        }

        ExpressionKind::UnitConversion(source_expression, target) => {
            let source_type = infer_expression_type(
                source_expression,
                graph,
                computed_rule_types,
                resolved_types,
                spec_name,
            );
            if source_type.vetoed() {
                return LemmaType::veto_type();
            }
            if source_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            match target {
                SemanticConversionTarget::Duration(_) => primitive_duration().clone(),
                SemanticConversionTarget::ScaleUnit(unit_name) => {
                    if source_type.is_number() {
                        find_types_by_name(resolved_types, spec_name)
                            .and_then(|dt| dt.unit_index.get(unit_name))
                            .cloned()
                            .unwrap_or_else(LemmaType::undetermined_type)
                    } else {
                        source_type
                    }
                }
                SemanticConversionTarget::RatioUnit(unit_name) => {
                    if source_type.is_number() {
                        find_types_by_name(resolved_types, spec_name)
                            .and_then(|dt| dt.unit_index.get(unit_name))
                            .cloned()
                            .unwrap_or_else(LemmaType::undetermined_type)
                    } else {
                        source_type
                    }
                }
            }
        }

        ExpressionKind::MathematicalComputation(_, operand) => {
            let operand_type = infer_expression_type(
                operand,
                graph,
                computed_rule_types,
                resolved_types,
                spec_name,
            );
            if operand_type.vetoed() {
                return LemmaType::veto_type();
            }
            if operand_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            primitive_number().clone()
        }

        ExpressionKind::Veto(_) => LemmaType::veto_type(),

        ExpressionKind::Now => primitive_date().clone(),

        ExpressionKind::DateRelative(..) | ExpressionKind::DateCalendar(..) => {
            primitive_boolean().clone()
        }
    }
}

/// Infer the type of a data reference without producing errors.
/// Returns `LemmaType::undetermined_type()` when the data cannot be found or is a spec reference.
///
/// For rule-target references the reference's stored `resolved_type` is still
/// the LHS-only placeholder (or fully `undetermined`) at the time
/// [`infer_rule_types`] runs — that field is filled by
/// [`Graph::resolve_rule_reference_types`] AFTER this pass. We therefore
/// look the target rule's inferred type up in `computed_rule_types`.
fn infer_data_type(
    data_path: &DataPath,
    graph: &Graph,
    computed_rule_types: &HashMap<RulePath, LemmaType>,
) -> LemmaType {
    let entry = match graph.data().get(data_path) {
        Some(e) => e,
        None => return LemmaType::undetermined_type(),
    };
    match entry {
        DataDefinition::Value { value, .. } => value.lemma_type.clone(),
        DataDefinition::TypeDeclaration { resolved_type, .. } => resolved_type.clone(),
        DataDefinition::Reference {
            target: ReferenceTarget::Rule(target_rule),
            resolved_type,
            ..
        } => {
            if !resolved_type.is_undetermined() {
                resolved_type.clone()
            } else {
                computed_rule_types
                    .get(target_rule)
                    .cloned()
                    .unwrap_or_else(LemmaType::undetermined_type)
            }
        }
        DataDefinition::Reference { resolved_type, .. } => resolved_type.clone(),
        DataDefinition::SpecRef { .. } => LemmaType::undetermined_type(),
    }
}

/// Walk an expression tree, find every `DataPath` that resolves to a
/// rule-target reference in `reference_to_rule`, and accumulate the reference's
/// target rule into `out`. Used by
/// [`Graph::add_rule_reference_dependency_edges`] to inject rule-rule
/// dependency edges so `topological_sort` orders the target rule before any
/// consumer of the reference data path.
fn collect_rule_reference_dependencies(
    expression: &Expression,
    reference_to_rule: &HashMap<DataPath, RulePath>,
    out: &mut BTreeSet<RulePath>,
) {
    let mut paths: HashSet<DataPath> = HashSet::new();
    expression.kind.collect_data_paths(&mut paths);
    for path in paths {
        if let Some(target_rule) = reference_to_rule.get(&path) {
            out.insert(target_rule.clone());
        }
    }
}

// =============================================================================
// Phase 2: Pure type checking (validation only, no mutation, returns Result)
// =============================================================================

fn engine_error_at_graph(graph: &Graph, source: &Source, message: impl Into<String>) -> Error {
    Error::validation_with_context(
        message.into(),
        Some(source.clone()),
        None::<String>,
        Some(Arc::clone(&graph.main_spec)),
        None,
    )
}

fn check_logical_operands(
    graph: &Graph,
    left_type: &LemmaType,
    right_type: &LemmaType,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if left_type.vetoed() || right_type.vetoed() {
        return Ok(());
    }
    let mut errors = Vec::new();
    if !left_type.is_boolean() {
        errors.push(engine_error_at_graph(
            graph,
            source,
            format!(
                "Logical operation requires boolean operands, got {:?} for left operand",
                left_type
            ),
        ));
    }
    if !right_type.is_boolean() {
        errors.push(engine_error_at_graph(
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

fn check_logical_operand(
    graph: &Graph,
    operand_type: &LemmaType,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if operand_type.vetoed() {
        return Ok(());
    }
    if !operand_type.is_boolean() {
        Err(vec![engine_error_at_graph(
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

fn check_comparison_types(
    graph: &Graph,
    left_type: &LemmaType,
    op: &ComparisonComputation,
    right_type: &LemmaType,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if left_type.vetoed() || right_type.vetoed() {
        return Ok(());
    }
    let is_equality_only = matches!(op, ComparisonComputation::Is | ComparisonComputation::IsNot);

    if left_type.is_boolean() && right_type.is_boolean() {
        if !is_equality_only {
            return Err(vec![engine_error_at_graph(
                graph,
                source,
                format!("Can only use 'is' and 'is not' with booleans (got {})", op),
            )]);
        }
        return Ok(());
    }

    if left_type.is_text() && right_type.is_text() {
        if !is_equality_only {
            return Err(vec![engine_error_at_graph(
                graph,
                source,
                format!("Can only use 'is' and 'is not' with text (got {})", op),
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
            return Err(vec![engine_error_at_graph(
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

    Err(vec![engine_error_at_graph(
        graph,
        source,
        format!("Cannot compare {:?} with {:?}", left_type, right_type),
    )])
}

fn check_arithmetic_types(
    graph: &Graph,
    left_type: &LemmaType,
    right_type: &LemmaType,
    operator: &ArithmeticComputation,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if left_type.vetoed() || right_type.vetoed() {
        return Ok(());
    }
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
            return Err(vec![engine_error_at_graph(
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
        return Err(vec![engine_error_at_graph(
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
        return Err(vec![engine_error_at_graph(
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
        return Err(vec![engine_error_at_graph(
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

fn check_unit_conversion_types(
    graph: &Graph,
    source_type: &LemmaType,
    target: &SemanticConversionTarget,
    resolved_types: &ResolvedTypesMap,
    source: &Source,
    spec_name: &str,
) -> Result<(), Vec<Error>> {
    if source_type.vetoed() {
        return Ok(());
    }
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
                Some((false, valid)) => Err(vec![engine_error_at_graph(
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
                    if find_types_by_name(resolved_types, spec_name)
                        .and_then(|dt| dt.unit_index.get(unit_name))
                        .is_none()
                    {
                        Err(vec![engine_error_at_graph(
                            graph,
                            source,
                            format!("Unknown unit '{}' in spec '{}'.", unit_name, spec_name),
                        )])
                    } else {
                        Ok(())
                    }
                }
                None => Err(vec![engine_error_at_graph(
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
                Err(vec![engine_error_at_graph(
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

fn check_mathematical_operand(
    graph: &Graph,
    operand_type: &LemmaType,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if operand_type.vetoed() {
        return Ok(());
    }
    if !operand_type.is_number() {
        Err(vec![engine_error_at_graph(
            graph,
            source,
            format!(
                "Mathematical function requires number operand, got {:?}",
                operand_type
            ),
        )])
    } else {
        Ok(())
    }
}

/// Check that all rule references in the graph point to existing rules.
fn check_all_rule_references_exist(graph: &Graph) -> Result<(), Vec<Error>> {
    let mut errors = Vec::new();
    let existing_rules: HashSet<&RulePath> = graph.rules().keys().collect();
    for (rule_path, rule_node) in graph.rules() {
        for dependency in &rule_node.depends_on_rules {
            if !existing_rules.contains(dependency) {
                errors.push(engine_error_at_graph(
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

/// Check that no data and rule share the same name in the same spec.
fn check_data_and_rule_name_collisions(graph: &Graph) -> Result<(), Vec<Error>> {
    let mut errors = Vec::new();
    for rule_path in graph.rules().keys() {
        let data_path = DataPath::new(rule_path.segments.clone(), rule_path.rule.clone());
        if graph.data().contains_key(&data_path) {
            let rule_node = graph.rules().get(rule_path).unwrap_or_else(|| {
                unreachable!(
                    "BUG: rule '{}' missing from graph while validating name collisions",
                    rule_path.rule
                )
            });
            errors.push(engine_error_at_graph(
                graph,
                &rule_node.source,
                format!(
                    "Name collision: '{}' is defined as both a data and a rule",
                    data_path
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

/// Check that a data reference is valid (exists and is not a bare spec reference).
fn check_data_reference(
    data_path: &DataPath,
    graph: &Graph,
    data_source: &Source,
) -> Result<(), Vec<Error>> {
    let entry = match graph.data().get(data_path) {
        Some(e) => e,
        None => {
            return Err(vec![engine_error_at_graph(
                graph,
                data_source,
                format!("Unknown data reference '{}'", data_path),
            )]);
        }
    };
    match entry {
        DataDefinition::Value { .. }
        | DataDefinition::TypeDeclaration { .. }
        | DataDefinition::Reference { .. } => Ok(()),
        DataDefinition::SpecRef { .. } => Err(vec![engine_error_at_graph(
            graph,
            entry.source(),
            format!(
                "Cannot compute type for spec reference data '{}'",
                data_path
            ),
        )]),
    }
}

/// Check a single expression for type errors, given precomputed inferred types.
/// Recursively checks sub-expressions. Skips validation when either operand is `Error`
/// (the root cause is reported by `check_data_reference` or similar).
fn check_expression(
    expression: &Expression,
    graph: &Graph,
    inferred_types: &HashMap<RulePath, LemmaType>,
    resolved_types: &ResolvedTypesMap,
    spec_name: &str,
) -> Result<(), Vec<Error>> {
    let mut errors = Vec::new();

    let collect = |result: Result<(), Vec<Error>>, errors: &mut Vec<Error>| {
        if let Err(errs) = result {
            errors.extend(errs);
        }
    };

    match &expression.kind {
        ExpressionKind::Literal(_) => {}

        ExpressionKind::DataPath(data_path) => {
            let data_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_data_reference(data_path, graph, data_source),
                &mut errors,
            );
        }

        ExpressionKind::RulePath(_) => {}

        ExpressionKind::LogicalAnd(left, right) => {
            collect(
                check_expression(left, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );
            collect(
                check_expression(right, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );

            let left_type =
                infer_expression_type(left, graph, inferred_types, resolved_types, spec_name);
            let right_type =
                infer_expression_type(right, graph, inferred_types, resolved_types, spec_name);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_logical_operands(graph, &left_type, &right_type, expr_source),
                &mut errors,
            );
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            collect(
                check_expression(operand, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );

            let operand_type =
                infer_expression_type(operand, graph, inferred_types, resolved_types, spec_name);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_logical_operand(graph, &operand_type, expr_source),
                &mut errors,
            );
        }

        ExpressionKind::Comparison(left, op, right) => {
            collect(
                check_expression(left, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );
            collect(
                check_expression(right, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );

            let left_type =
                infer_expression_type(left, graph, inferred_types, resolved_types, spec_name);
            let right_type =
                infer_expression_type(right, graph, inferred_types, resolved_types, spec_name);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_comparison_types(graph, &left_type, op, &right_type, expr_source),
                &mut errors,
            );
        }

        ExpressionKind::Arithmetic(left, operator, right) => {
            collect(
                check_expression(left, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );
            collect(
                check_expression(right, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );

            let left_type =
                infer_expression_type(left, graph, inferred_types, resolved_types, spec_name);
            let right_type =
                infer_expression_type(right, graph, inferred_types, resolved_types, spec_name);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_arithmetic_types(graph, &left_type, &right_type, operator, expr_source),
                &mut errors,
            );
        }

        ExpressionKind::UnitConversion(source_expression, target) => {
            collect(
                check_expression(
                    source_expression,
                    graph,
                    inferred_types,
                    resolved_types,
                    spec_name,
                ),
                &mut errors,
            );

            let source_type = infer_expression_type(
                source_expression,
                graph,
                inferred_types,
                resolved_types,
                spec_name,
            );
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_unit_conversion_types(
                    graph,
                    &source_type,
                    target,
                    resolved_types,
                    expr_source,
                    spec_name,
                ),
                &mut errors,
            );

            if source_type.is_number() {
                match target {
                    SemanticConversionTarget::ScaleUnit(unit_name)
                    | SemanticConversionTarget::RatioUnit(unit_name) => {
                        if find_types_by_name(resolved_types, spec_name)
                            .and_then(|dt| dt.unit_index.get(unit_name))
                            .is_none()
                        {
                            errors.push(engine_error_at_graph(
                                graph,
                                expr_source,
                                format!(
                                    "Cannot resolve unit '{}' for spec '{}' (types may not have been resolved)",
                                    unit_name,
                                    spec_name
                                ),
                            ));
                        }
                    }
                    SemanticConversionTarget::Duration(_) => {}
                }
            }
        }

        ExpressionKind::MathematicalComputation(_, operand) => {
            collect(
                check_expression(operand, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );

            let operand_type =
                infer_expression_type(operand, graph, inferred_types, resolved_types, spec_name);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_mathematical_operand(graph, &operand_type, expr_source),
                &mut errors,
            );
        }

        ExpressionKind::Veto(_) => {}

        ExpressionKind::Now => {}

        ExpressionKind::DateRelative(_, date_expr, tolerance) => {
            collect(
                check_expression(date_expr, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );

            let date_type =
                infer_expression_type(date_expr, graph, inferred_types, resolved_types, spec_name);
            if !date_type.is_date() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                errors.push(engine_error_at_graph(
                    graph,
                    expr_source,
                    format!(
                        "Date sugar 'in past/future' requires a date expression, got type '{}'",
                        date_type
                    ),
                ));
            }

            if let Some(tol) = tolerance {
                collect(
                    check_expression(tol, graph, inferred_types, resolved_types, spec_name),
                    &mut errors,
                );

                let tol_type =
                    infer_expression_type(tol, graph, inferred_types, resolved_types, spec_name);
                if !tol_type.is_duration() {
                    let expr_source = expression
                        .source_location
                        .as_ref()
                        .expect("BUG: expression missing source in check_expression");
                    errors.push(engine_error_at_graph(
                        graph,
                        expr_source,
                        format!(
                            "Tolerance in date sugar must be a duration, got type '{}'",
                            tol_type
                        ),
                    ));
                }
            }
        }

        ExpressionKind::DateCalendar(_, _, date_expr) => {
            collect(
                check_expression(date_expr, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );

            let date_type =
                infer_expression_type(date_expr, graph, inferred_types, resolved_types, spec_name);
            if !date_type.is_date() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                errors.push(engine_error_at_graph(
                    graph,
                    expr_source,
                    format!(
                        "Calendar sugar requires a date expression, got type '{}'",
                        date_type
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

/// Check all rule types in topological order, given precomputed inferred types.
/// Validates:
/// - Branch type consistency (all non-Veto branches must return the same primitive type)
/// - Condition types (unless clause conditions must be boolean)
/// - All sub-expressions via `check_expression`
fn check_rule_types(
    graph: &Graph,
    execution_order: &[RulePath],
    inferred_types: &HashMap<RulePath, LemmaType>,
    resolved_types: &ResolvedTypesMap,
) -> Result<(), Vec<Error>> {
    let mut errors = Vec::new();

    let collect = |result: Result<(), Vec<Error>>, errors: &mut Vec<Error>| {
        if let Err(errs) = result {
            errors.extend(errs);
        }
    };

    for rule_path in execution_order {
        let rule_node = match graph.rules().get(rule_path) {
            Some(node) => node,
            None => continue,
        };
        let branches = &rule_node.branches;
        let spec_name = rule_node.spec_name.as_str();

        if branches.is_empty() {
            continue;
        }

        let (_, default_result) = &branches[0];
        collect(
            check_expression(
                default_result,
                graph,
                inferred_types,
                resolved_types,
                spec_name,
            ),
            &mut errors,
        );
        let default_type = infer_expression_type(
            default_result,
            graph,
            inferred_types,
            resolved_types,
            spec_name,
        );

        let mut non_veto_type: Option<LemmaType> = None;
        if !default_type.vetoed() && !default_type.is_undetermined() {
            non_veto_type = Some(default_type.clone());
        }

        for (branch_index, (condition, result)) in branches.iter().enumerate().skip(1) {
            if let Some(condition_expression) = condition {
                collect(
                    check_expression(
                        condition_expression,
                        graph,
                        inferred_types,
                        resolved_types,
                        spec_name,
                    ),
                    &mut errors,
                );
                let condition_type = infer_expression_type(
                    condition_expression,
                    graph,
                    inferred_types,
                    resolved_types,
                    spec_name,
                );
                if !condition_type.is_boolean() && !condition_type.is_undetermined() {
                    let condition_source = condition_expression
                        .source_location
                        .as_ref()
                        .expect("BUG: condition expression missing source in check_rule_types");
                    errors.push(engine_error_at_graph(
                        graph,
                        condition_source,
                        format!(
                            "Unless clause condition in rule '{}' must be boolean, got {:?}",
                            rule_path.rule, condition_type
                        ),
                    ));
                }
            }

            collect(
                check_expression(result, graph, inferred_types, resolved_types, spec_name),
                &mut errors,
            );
            let result_type =
                infer_expression_type(result, graph, inferred_types, resolved_types, spec_name);

            if !result_type.vetoed() && !result_type.is_undetermined() {
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

                        errors.push(Error::validation_with_context(
                            format!("Type mismatch in rule '{}' in spec '{}' ({}): default branch returns {}, but unless clause {} returns {}. All branches must return the same primitive type.",
                            rule_path.rule,
                            spec_name,
                            location_parts.join(", "),
                            existing_type.name(),
                            branch_index,
                            result_type.name()),
                            Some(rule_source.clone()),
                            None::<String>,
                            Some(Arc::clone(&graph.main_spec)),
                            None,
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
fn infer_rule_types(
    graph: &Graph,
    execution_order: &[RulePath],
    resolved_types: &ResolvedTypesMap,
) -> HashMap<RulePath, LemmaType> {
    let mut computed_types: HashMap<RulePath, LemmaType> = HashMap::new();

    for rule_path in execution_order {
        let rule_node = match graph.rules().get(rule_path) {
            Some(node) => node,
            None => continue,
        };
        let branches = &rule_node.branches;
        let spec_name = rule_node.spec_name.as_str();

        if branches.is_empty() {
            continue;
        }

        let (_, default_result) = &branches[0];
        let default_type = infer_expression_type(
            default_result,
            graph,
            &computed_types,
            resolved_types,
            spec_name,
        );

        let mut non_veto_type: Option<LemmaType> = None;
        if !default_type.vetoed() && !default_type.is_undetermined() {
            non_veto_type = Some(default_type.clone());
        }

        for (_branch_index, (condition, result)) in branches.iter().enumerate().skip(1) {
            if let Some(condition_expression) = condition {
                let _condition_type = infer_expression_type(
                    condition_expression,
                    graph,
                    &computed_types,
                    resolved_types,
                    spec_name,
                );
            }

            let result_type =
                infer_expression_type(result, graph, &computed_types, resolved_types, spec_name);
            if !result_type.vetoed() && !result_type.is_undetermined() && non_veto_type.is_none() {
                non_veto_type = Some(result_type.clone());
            }
        }

        let rule_type = non_veto_type.unwrap_or_else(LemmaType::veto_type);
        computed_types.insert(rule_path.clone(), rule_type);
    }

    computed_types
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parsing::ast::{BooleanValue, Reference, Span, Value};

    fn test_source() -> Source {
        Source::new(
            "test.lemma",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        )
    }

    fn build_graph(main_spec: &LemmaSpec, all_specs: &[LemmaSpec]) -> Result<Graph, Vec<Error>> {
        use crate::engine::Context;
        use crate::planning::discovery;

        let mut ctx = Context::new();
        for s in all_specs {
            if let Err(e) = ctx.insert_spec(Arc::new(s.clone()), s.from_registry) {
                return Err(vec![e]);
            }
        }
        let effective = EffectiveDate::from_option(main_spec.effective_from().cloned());
        let main_spec_arc = ctx
            .spec_sets()
            .get(main_spec.name.as_str())
            .and_then(|ss| ss.get_exact(main_spec.effective_from()).cloned())
            .expect("main_spec must be in all_specs");
        let dag =
            discovery::build_dag_for_spec(&ctx, &main_spec_arc, &effective).map_err(
                |e| match e {
                    discovery::DagError::Cycle(es) | discovery::DagError::Other(es) => es,
                },
            )?;
        match Graph::build(&ctx, &main_spec_arc, &dag, &effective) {
            Ok((graph, _types)) => Ok(graph),
            Err(errors) => Err(errors),
        }
    }

    fn create_test_spec(name: &str) -> LemmaSpec {
        LemmaSpec::new(name.to_string())
    }

    fn create_literal_data(name: &str, value: Value) -> LemmaData {
        LemmaData {
            reference: Reference {
                segments: Vec::new(),
                name: name.to_string(),
            },
            value: ParsedDataValue::Literal(value),
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
    fn should_reject_data_binding_into_non_spec_data() {
        // Higher-standard language rule:
        // if `x` is a literal (not a spec reference), `x.y = ...` must be rejected.
        //
        // This is currently expected to FAIL until graph building enforces it consistently.
        let mut spec = create_test_spec("test");
        spec = spec.add_data(create_literal_data("x", Value::Number(1.into())));

        // Bind x.y, but x is not a spec reference.
        spec = spec.add_data(LemmaData {
            reference: Reference::from_path(vec!["x".to_string(), "y".to_string()]),
            value: ParsedDataValue::Literal(Value::Number(2.into())),
            source_location: test_source(),
        });

        let result = build_graph(&spec, &[spec.clone()]);
        assert!(
            result.is_err(),
            "Overriding x.y must fail when x is not a spec reference"
        );
    }

    #[test]
    fn should_reject_data_and_rule_name_collision() {
        // Higher-standard language rule: data and rule names should not collide.
        // It's ambiguous for humans and leads to confusing error messages.
        //
        // This is currently expected to FAIL until the language enforces it.
        let mut spec = create_test_spec("test");
        spec = spec.add_data(create_literal_data("x", Value::Number(1.into())));
        spec = spec.add_rule(LemmaRule {
            name: "x".to_string(),
            expression: create_literal_expr(Value::Number(2.into())),
            unless_clauses: Vec::new(),
            source_location: test_source(),
        });

        let result = build_graph(&spec, &[spec.clone()]);
        assert!(
            result.is_err(),
            "Data and rule name collisions should be rejected"
        );
    }

    #[test]
    fn test_duplicate_data() {
        let mut spec = create_test_spec("test");
        spec = spec.add_data(create_literal_data(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));
        spec = spec.add_data(create_literal_data(
            "age",
            Value::Number(rust_decimal::Decimal::from(30)),
        ));

        let result = build_graph(&spec, &[spec.clone()]);
        assert!(result.is_err(), "Should detect duplicate data");

        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Duplicate data") && e.to_string().contains("age")));
    }

    #[test]
    fn test_duplicate_rule() {
        let mut spec = create_test_spec("test");

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

        spec = spec.add_rule(rule1);
        spec = spec.add_rule(rule2);

        let result = build_graph(&spec, &[spec.clone()]);
        assert!(result.is_err(), "Should detect duplicate rule");

        let errors = result.unwrap_err();
        assert!(errors.iter().any(
            |e| e.to_string().contains("Duplicate rule") && e.to_string().contains("test_rule")
        ));
    }

    #[test]
    fn test_missing_data_reference() {
        let mut spec = create_test_spec("test");

        let missing_data_expr = ast::Expression {
            kind: ast::ExpressionKind::Reference(Reference {
                segments: Vec::new(),
                name: "nonexistent".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: missing_data_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        spec = spec.add_rule(rule);

        let result = build_graph(&spec, &[spec.clone()]);
        assert!(result.is_err(), "Should detect missing data");

        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Reference 'nonexistent' not found")));
    }

    #[test]
    fn test_missing_spec_reference() {
        let mut spec = create_test_spec("test");

        let data = LemmaData {
            reference: Reference {
                segments: Vec::new(),
                name: "contract".to_string(),
            },
            value: ParsedDataValue::SpecReference(crate::parsing::ast::SpecRef::local(
                "nonexistent",
            )),
            source_location: test_source(),
        };
        spec = spec.add_data(data);

        let result = build_graph(&spec, &[spec.clone()]);
        assert!(result.is_err(), "Should detect missing spec");

        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.to_string().contains("nonexistent")),
            "Error should mention nonexistent spec: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_data_reference_conversion() {
        let mut spec = create_test_spec("test");
        spec = spec.add_data(create_literal_data(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));

        let age_expr = ast::Expression {
            kind: ast::ExpressionKind::Reference(Reference {
                segments: Vec::new(),
                name: "age".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: age_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        spec = spec.add_rule(rule);

        let result = build_graph(&spec, &[spec.clone()]);
        assert!(result.is_ok(), "Should build graph successfully");

        let graph = result.unwrap();
        let rule_node = graph.rules().values().next().unwrap();

        assert!(matches!(
            rule_node.branches[0].1.kind,
            ExpressionKind::DataPath(_)
        ));
    }

    #[test]
    fn test_rule_reference_conversion() {
        let mut spec = create_test_spec("test");

        let rule1_expr = ast::Expression {
            kind: ast::ExpressionKind::Reference(Reference {
                segments: Vec::new(),
                name: "age".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule1 = LemmaRule {
            name: "rule1".to_string(),
            expression: rule1_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        spec = spec.add_rule(rule1);

        let rule2_expr = ast::Expression {
            kind: ast::ExpressionKind::Reference(Reference {
                segments: Vec::new(),
                name: "rule1".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule2 = LemmaRule {
            name: "rule2".to_string(),
            expression: rule2_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        spec = spec.add_rule(rule2);

        spec = spec.add_data(create_literal_data(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));

        let result = build_graph(&spec, &[spec.clone()]);
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
        let mut spec = create_test_spec("test");
        spec = spec.add_data(create_literal_data(
            "age",
            Value::Number(rust_decimal::Decimal::from(25)),
        ));
        spec = spec.add_data(create_literal_data(
            "age",
            Value::Number(rust_decimal::Decimal::from(30)),
        ));

        let missing_data_expr = ast::Expression {
            kind: ast::ExpressionKind::Reference(Reference {
                segments: Vec::new(),
                name: "nonexistent".to_string(),
            }),
            source_location: Some(test_source()),
        };

        let rule = LemmaRule {
            name: "test_rule".to_string(),
            expression: missing_data_expr,
            unless_clauses: Vec::new(),
            source_location: test_source(),
        };
        spec = spec.add_rule(rule);

        let result = build_graph(&spec, &[spec.clone()]);
        assert!(result.is_err(), "Should collect multiple errors");

        let errors = result.unwrap_err();
        assert!(errors.len() >= 2, "Should have at least 2 errors");
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Duplicate data")));
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Reference 'nonexistent' not found")));
    }

    #[test]
    fn test_type_registration_collects_multiple_errors() {
        use crate::parsing::ast::{DataValue, ParentType, PrimitiveKind, SpecRef};

        let type_source = Source::new(
            "a.lemma",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let spec_a = create_test_spec("spec_a")
            .with_attribute("a.lemma".to_string())
            .add_data(LemmaData {
                reference: Reference::local("dep".to_string()),
                value: DataValue::SpecReference(SpecRef::local("spec_b")),
                source_location: type_source.clone(),
            })
            .add_data(LemmaData {
                reference: Reference::local("money".to_string()),
                value: DataValue::TypeDeclaration {
                    base: ParentType::Primitive {
                        primitive: PrimitiveKind::Number,
                    },
                    constraints: None,
                    from: None,
                },
                source_location: type_source.clone(),
            })
            .add_data(LemmaData {
                reference: Reference::local("money".to_string()),
                value: DataValue::TypeDeclaration {
                    base: ParentType::Primitive {
                        primitive: PrimitiveKind::Number,
                    },
                    constraints: None,
                    from: None,
                },
                source_location: type_source,
            });

        let type_source_b = Source::new(
            "b.lemma",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let spec_b = create_test_spec("spec_b")
            .with_attribute("b.lemma".to_string())
            .add_data(LemmaData {
                reference: Reference::local("length".to_string()),
                value: DataValue::TypeDeclaration {
                    base: ParentType::Primitive {
                        primitive: PrimitiveKind::Number,
                    },
                    constraints: None,
                    from: None,
                },
                source_location: type_source_b.clone(),
            })
            .add_data(LemmaData {
                reference: Reference::local("length".to_string()),
                value: DataValue::TypeDeclaration {
                    base: ParentType::Primitive {
                        primitive: PrimitiveKind::Number,
                    },
                    constraints: None,
                    from: None,
                },
                source_location: type_source_b,
            });

        let mut sources = HashMap::new();
        sources.insert(
            "a.lemma".to_string(),
            "spec spec_a\nwith dep: spec_b\ndata money: number\ndata money: number".to_string(),
        );
        sources.insert(
            "b.lemma".to_string(),
            "spec spec_b\ndata length: number\ndata length: number".to_string(),
        );

        let result = build_graph(&spec_a, &[spec_a.clone(), spec_b.clone()]);
        assert!(
            result.is_err(),
            "Should fail with duplicate type/data errors"
        );
    }

    // =================================================================
    // Versioned spec identifiers: latest-resolution (section 6.3)
    // =================================================================

    #[test]
    fn spec_ref_resolves_to_single_spec_by_name() {
        let code = r#"spec myspec
data x: 10

spec consumer
with m: myspec
rule result: m.x"#;
        let specs = crate::parse(code, "test.lemma", &crate::ResourceLimits::default())
            .unwrap()
            .specs;
        let consumer = specs.iter().find(|d| d.name == "consumer").unwrap();

        let graph = build_graph(consumer, &specs).unwrap();
        let data_path = DataPath {
            segments: vec![PathSegment {
                data: "m".to_string(),
                spec: "myspec".to_string(),
            }],
            data: "x".to_string(),
        };
        assert!(
            graph.data.contains_key(&data_path),
            "Ref should resolve to myspec. Data: {:?}",
            graph.data.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn spec_ref_to_nonexistent_spec_is_error() {
        let code = r#"spec myspec
data x: 10

spec consumer
with m: nonexistent
rule result: m.x"#;
        let specs = crate::parse(code, "test.lemma", &crate::ResourceLimits::default())
            .unwrap()
            .specs;
        let consumer = specs.iter().find(|d| d.name == "consumer").unwrap();
        let result = build_graph(consumer, &specs);
        assert!(result.is_err(), "Should fail for non-existent spec");
    }

    // =================================================================
    // Versioned spec identifiers: self-reference check (section 6.4)
    // =================================================================

    #[test]
    fn self_reference_is_error() {
        let code = "spec myspec\nwith m: myspec";
        let specs = crate::parse(code, "test.lemma", &crate::ResourceLimits::default())
            .unwrap()
            .specs;
        let result = build_graph(&specs[0], &specs);
        assert!(result.is_err(), "Self-reference should be an error");
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| {
                let s = e.to_string();
                s.contains("cycle") || s.contains("myspec")
            }),
            "Error should mention cycle or self-referencing spec: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }
}

// ============================================================================
// Type resolution (formerly types.rs)
// ============================================================================

/// Fully resolved types for a single spec.
/// After resolution, all imports are inlined — specs are independent.
#[derive(Debug, Clone)]
pub struct ResolvedSpecTypes {
    /// Named types: type_name -> fully resolved type
    pub named_types: HashMap<String, LemmaType>,

    /// Declared default per named type (e.g. `type rate: ratio -> default 0.5`).
    /// Only present for types that declared a `-> default ...` constraint anywhere
    /// in their extension chain; the inner-most `-> default` wins. Defaults live
    /// outside [`TypeSpecification`] so the type itself stays free of binding data.
    pub declared_defaults: HashMap<String, ValueKind>,

    /// Unit index: unit_name -> resolved type.
    /// Built during resolution — if unit appears in multiple types, resolution fails.
    pub unit_index: HashMap<String, LemmaType>,
}

/// Intermediate type definition extracted from `DataValue::TypeDeclaration` data.
/// Replaces the deleted `TypeDef` AST enum for type resolution purposes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DataTypeDef {
    pub parent: ParentType,
    pub constraints: Option<Vec<Constraint>>,
    pub from: Option<ast::SpecRef>,
    pub source: crate::Source,
    pub name: String,
}

/// Resolved spec for a parent type reference (same-spec or cross-spec import).
#[derive(Debug, Clone)]
pub(crate) struct ResolvedParentSpec {
    pub spec: Arc<LemmaSpec>,
}

/// Per-slice type resolver. Constructed for each `Graph::build` call.
///
/// Types are extracted from `DataValue::TypeDeclaration` data and keyed by `Arc<LemmaSpec>`.
/// The resolver handles cycle detection and accumulates constraints through the inheritance chain.
#[derive(Debug, Clone)]
pub(crate) struct TypeResolver<'a> {
    data_types: HashMap<Arc<LemmaSpec>, HashMap<String, DataTypeDef>>,
    context: &'a Context,
    all_registered_specs: Vec<Arc<LemmaSpec>>,
}

impl<'a> TypeResolver<'a> {
    pub fn new(context: &'a Context, _dag: &'a [Arc<LemmaSpec>]) -> Self {
        TypeResolver {
            data_types: HashMap::new(),
            context,
            all_registered_specs: Vec::new(),
        }
    }

    /// Register all type-declaring data from a spec.
    pub fn register_all(&mut self, spec: &Arc<LemmaSpec>) -> Vec<Error> {
        if !self
            .all_registered_specs
            .iter()
            .any(|s| Arc::ptr_eq(s, spec))
        {
            self.all_registered_specs.push(Arc::clone(spec));
        }

        let mut errors = Vec::new();
        for data in &spec.data {
            if let ParsedDataValue::TypeDeclaration {
                base,
                constraints,
                from,
            } = &data.value
            {
                let name = &data.reference.name;
                let ftd = DataTypeDef {
                    parent: base.clone(),
                    constraints: constraints.clone(),
                    from: from.clone(),
                    source: data.source_location.clone(),
                    name: name.clone(),
                };
                if let Err(e) = self.register_type(spec, ftd) {
                    errors.push(e);
                }
            }
        }
        errors
    }

    /// Register a type from a data declaration.
    pub fn register_type(&mut self, spec: &Arc<LemmaSpec>, def: DataTypeDef) -> Result<(), Error> {
        if !self
            .all_registered_specs
            .iter()
            .any(|s| Arc::ptr_eq(s, spec))
        {
            self.all_registered_specs.push(Arc::clone(spec));
        }

        let spec_types = self.data_types.entry(Arc::clone(spec)).or_default();
        if spec_types.contains_key(&def.name) {
            return Err(Error::validation_with_context(
                format!(
                    "Type '{}' is already defined in spec '{}'",
                    def.name, spec.name
                ),
                Some(def.source.clone()),
                None::<String>,
                Some(Arc::clone(spec)),
                None,
            ));
        }
        spec_types.insert(def.name.clone(), def);
        Ok(())
    }

    /// Resolve types for a single spec and validate their specifications.
    /// `at` is the planning instant for this spec (nested qualified refs use their pin).
    pub fn resolve_and_validate(
        &self,
        spec: &Arc<LemmaSpec>,
        at: &EffectiveDate,
    ) -> Result<ResolvedSpecTypes, Vec<Error>> {
        let resolved_types = self.resolve_types_internal(spec, at)?;
        let mut errors = Vec::new();

        for (type_name, lemma_type) in &resolved_types.named_types {
            let source = self
                .data_types
                .get(spec)
                .and_then(|defs| defs.get(type_name))
                .map(|ftd| ftd.source.clone())
                .unwrap_or_else(|| {
                    unreachable!(
                        "BUG: resolved type '{}' has no corresponding DataTypeDef in spec '{}'",
                        type_name, spec.name
                    )
                });
            let mut spec_errors = validate_type_specifications(
                &lemma_type.specifications,
                resolved_types.declared_defaults.get(type_name),
                type_name,
                &source,
                Some(Arc::clone(spec)),
            );
            errors.append(&mut spec_errors);
        }

        if errors.is_empty() {
            Ok(resolved_types)
        } else {
            Err(errors)
        }
    }

    // =========================================================================
    // Private resolution methods
    // =========================================================================

    fn resolve_types_internal(
        &self,
        spec: &Arc<LemmaSpec>,
        at: &EffectiveDate,
    ) -> Result<ResolvedSpecTypes, Vec<Error>> {
        let mut named_types = HashMap::new();
        let mut declared_defaults: HashMap<String, ValueKind> = HashMap::new();
        let mut visited = HashSet::new();

        if let Some(spec_types) = self.data_types.get(spec) {
            for type_name in spec_types.keys() {
                match self.resolve_type_internal(spec, type_name, &mut visited, at) {
                    Ok(Some((resolved_type, declared_default))) => {
                        named_types.insert(type_name.clone(), resolved_type);
                        if let Some(dv) = declared_default {
                            declared_defaults.insert(type_name.clone(), dv);
                        }
                    }
                    Ok(None) => {
                        unreachable!(
                            "BUG: registered type '{}' could not be resolved (spec='{}')",
                            type_name, spec.name
                        );
                    }
                    Err(es) => return Err(es),
                }
                visited.clear();
            }
        }

        // Build unit_index with DataTypeDef for conflict detection, then strip to LemmaType.
        let mut unit_index_tmp: HashMap<String, (LemmaType, Option<DataTypeDef>)> = HashMap::new();
        let mut errors = Vec::new();

        let prim_ratio = semantics::primitive_ratio();
        for unit in Self::extract_units_from_type(&prim_ratio.specifications) {
            unit_index_tmp.insert(unit, (prim_ratio.clone(), None));
        }

        for (type_name, resolved_type) in &named_types {
            let data_type_def = self
                .data_types
                .get(spec)
                .and_then(|defs| defs.get(type_name.as_str()))
                .expect("BUG: type was resolved but not in registry");
            let e: Result<(), Error> = if resolved_type.is_scale() {
                Self::add_scale_units_to_index(
                    spec,
                    &mut unit_index_tmp,
                    resolved_type,
                    data_type_def,
                )
            } else if resolved_type.is_ratio() {
                Self::add_ratio_units_to_index(
                    spec,
                    &mut unit_index_tmp,
                    resolved_type,
                    data_type_def,
                )
            } else {
                Ok(())
            };
            if let Err(e) = e {
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let unit_index = unit_index_tmp
            .into_iter()
            .map(|(k, (lt, _))| (k, lt))
            .collect();

        Ok(ResolvedSpecTypes {
            named_types,
            declared_defaults,
            unit_index,
        })
    }

    fn resolve_type_internal(
        &self,
        spec: &Arc<LemmaSpec>,
        name: &str,
        visited: &mut HashSet<String>,
        at: &EffectiveDate,
    ) -> Result<Option<(LemmaType, Option<ValueKind>)>, Vec<Error>> {
        let key = format!("{}::{}", spec.name, name);
        if visited.contains(&key) {
            let source_location = self
                .data_types
                .get(spec)
                .and_then(|dt| dt.get(name))
                .map(|ftd| ftd.source.clone())
                .unwrap_or_else(|| {
                    unreachable!(
                        "BUG: circular dependency detected for type '{}::{}' but type definition not found in registry",
                        spec.name, name
                    )
                });
            return Err(vec![Error::validation_with_context(
                format!("Circular dependency detected in type resolution: {}", key),
                Some(source_location),
                None::<String>,
                Some(Arc::clone(spec)),
                None,
            )]);
        }
        visited.insert(key.clone());

        let ftd = match self.data_types.get(spec).and_then(|dt| dt.get(name)) {
            Some(def) => def.clone(),
            None => {
                visited.remove(&key);
                return Ok(None);
            }
        };

        let parent = ftd.parent.clone();
        let from = ftd.from.clone();
        let constraints = ftd.constraints.clone();

        let (parent_specs, parent_declared_default) = match self.resolve_parent(
            spec,
            &parent,
            &from,
            visited,
            &ftd.source,
            at,
        ) {
            Ok(Some(pair)) => pair,
            Ok(None) => {
                visited.remove(&key);
                return Err(vec![Error::validation_with_context(
                        format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                        Some(ftd.source.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    )]);
            }
            Err(es) => {
                visited.remove(&key);
                return Err(es);
            }
        };

        let mut declared_default = parent_declared_default;
        let final_specs = if let Some(constraints) = &constraints {
            match apply_constraints_to_spec(
                spec,
                parent_specs,
                constraints,
                &ftd.source,
                &mut declared_default,
            ) {
                Ok(specs) => specs,
                Err(errors) => {
                    visited.remove(&key);
                    return Err(errors);
                }
            }
        } else {
            parent_specs
        };

        visited.remove(&key);

        let extends = {
            let parent_name = parent.to_string();
            let parent_spec = match self.get_spec_arc_for_parent(spec, &from, &ftd.source, at) {
                Ok(x) => x,
                Err(e) => return Err(vec![e]),
            };
            let family = match &parent_spec {
                Some(r) => match self.resolve_type_internal(&r.spec, &parent_name, visited, at) {
                    Ok(Some((parent_type, _))) => parent_type
                        .scale_family_name()
                        .map(String::from)
                        .unwrap_or_else(|| name.to_string()),
                    Ok(None) => name.to_string(),
                    Err(es) => return Err(es),
                },
                None => name.to_string(),
            };
            let defining_spec = if from.is_some() {
                match &parent_spec {
                    Some(r) => TypeDefiningSpec::Import {
                        spec: Arc::clone(&r.spec),
                    },
                    None => unreachable!(
                        "BUG: from.is_some() but get_spec_arc_for_parent returned Ok(None)"
                    ),
                }
            } else {
                TypeDefiningSpec::Local
            };
            TypeExtends::Custom {
                parent: parent_name,
                family,
                defining_spec,
            }
        };

        Ok(Some((
            LemmaType {
                name: Some(parent.to_string()),
                specifications: final_specs,
                extends,
            },
            declared_default,
        )))
    }

    fn resolve_parent(
        &self,
        spec: &Arc<LemmaSpec>,
        parent: &ParentType,
        from: &Option<crate::parsing::ast::SpecRef>,
        visited: &mut HashSet<String>,
        source: &crate::Source,
        at: &EffectiveDate,
    ) -> Result<Option<(TypeSpecification, Option<ValueKind>)>, Vec<Error>> {
        if let ParentType::Primitive { primitive: kind } = parent {
            return Ok(Some((semantics::type_spec_for_primitive(*kind), None)));
        }

        let parent_name = match parent {
            ParentType::Custom { name } => name.as_str(),
            ParentType::Primitive { .. } => unreachable!("already returned above"),
        };

        let parent_spec = match self.get_spec_arc_for_parent(spec, from, source, at) {
            Ok(x) => x,
            Err(e) => return Err(vec![e]),
        };
        let result = match &parent_spec {
            Some(r) => self.resolve_type_internal(&r.spec, parent_name, visited, at),
            None => Ok(None),
        };
        match result {
            Ok(Some((t, declared_default))) => Ok(Some((t.specifications, declared_default))),
            Ok(None) => {
                let type_exists = parent_spec
                    .as_ref()
                    .and_then(|r| self.data_types.get(&r.spec))
                    .map(|spec_types| spec_types.contains_key(parent_name))
                    .unwrap_or(false);

                if !type_exists {
                    if from.is_none()
                        && spec.data.iter().any(|d| {
                            d.reference.is_local()
                                && d.reference.name == parent_name
                                && matches!(&d.value, ParsedDataValue::SpecReference(_))
                        })
                    {
                        return Err(vec![Error::validation_with_context(
                            format!(
                                "'{}' is a spec reference and cannot carry a value: a spec reference is not a type and cannot be referenced from a data declaration",
                                parent_name
                            ),
                            Some(source.clone()),
                            Some(format!(
                                "To reference data inside the spec, use a dotted path like '{}.<data_name>'",
                                parent_name
                            )),
                            Some(Arc::clone(spec)),
                            None,
                        )]);
                    }
                    let suggestion = from.as_ref().filter(|r| r.from_registry).map(|r| {
                        format!(
                            "Run `lemma get` or `lemma get {}` to fetch this dependency.",
                            r.name
                        )
                    });
                    Err(vec![Error::validation_with_context(
                        format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                        Some(source.clone()),
                        suggestion,
                        Some(Arc::clone(spec)),
                        None,
                    )])
                } else {
                    Ok(None)
                }
            }
            Err(es) => Err(es),
        }
    }

    fn get_spec_arc_for_parent(
        &self,
        spec: &Arc<LemmaSpec>,
        from: &Option<crate::parsing::ast::SpecRef>,
        import_site: &crate::Source,
        at: &EffectiveDate,
    ) -> Result<Option<ResolvedParentSpec>, Error> {
        match from {
            Some(from_ref) => self
                .resolve_spec_for_import(spec, from_ref, import_site, at)
                .map(|arc| Some(ResolvedParentSpec { spec: arc })),
            None => Ok(Some(ResolvedParentSpec {
                spec: Arc::clone(spec),
            })),
        }
    }

    fn resolve_spec_for_import(
        &self,
        spec: &Arc<LemmaSpec>,
        from: &crate::parsing::ast::SpecRef,
        import_site: &crate::Source,
        at: &EffectiveDate,
    ) -> Result<Arc<LemmaSpec>, Error> {
        discovery::resolve_spec_ref(
            self.context,
            from,
            at,
            &spec.name,
            Some(import_site.clone()),
            Some(Arc::clone(spec)),
        )
    }

    // =========================================================================
    // Static helpers (no &self)
    // =========================================================================

    fn add_scale_units_to_index(
        spec: &Arc<LemmaSpec>,
        unit_index: &mut HashMap<String, (LemmaType, Option<DataTypeDef>)>,
        resolved_type: &LemmaType,
        defined_by: &DataTypeDef,
    ) -> Result<(), Error> {
        let units = Self::extract_units_from_type(&resolved_type.specifications);
        for unit in units {
            if let Some((existing_type, existing_def)) = unit_index.get(&unit) {
                let same_type = existing_def.as_ref() == Some(defined_by);

                if same_type {
                    return Err(Error::validation_with_context(
                        format!(
                            "Unit '{}' is defined more than once in type '{}'",
                            unit, defined_by.name
                        ),
                        Some(defined_by.source.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    ));
                }

                let existing_name: String = existing_def
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| existing_type.name());
                let current_extends_existing = resolved_type
                    .extends
                    .parent_name()
                    .map(|p| p == existing_name.as_str())
                    .unwrap_or(false);
                let existing_extends_current = existing_type
                    .extends
                    .parent_name()
                    .map(|p| p == defined_by.name.as_str())
                    .unwrap_or(false);

                if existing_type.is_scale()
                    && (current_extends_existing || existing_extends_current)
                {
                    if current_extends_existing {
                        unit_index.insert(unit, (resolved_type.clone(), Some(defined_by.clone())));
                    }
                    continue;
                }

                if existing_type.same_scale_family(resolved_type) {
                    continue;
                }

                return Err(Error::validation_with_context(
                    format!(
                        "Ambiguous unit '{}'. Defined in multiple types: '{}' and '{}'",
                        unit, existing_name, defined_by.name
                    ),
                    Some(defined_by.source.clone()),
                    None::<String>,
                    Some(Arc::clone(spec)),
                    None,
                ));
            }
            unit_index.insert(unit, (resolved_type.clone(), Some(defined_by.clone())));
        }
        Ok(())
    }

    fn add_ratio_units_to_index(
        spec: &Arc<LemmaSpec>,
        unit_index: &mut HashMap<String, (LemmaType, Option<DataTypeDef>)>,
        resolved_type: &LemmaType,
        defined_by: &DataTypeDef,
    ) -> Result<(), Error> {
        let units = Self::extract_units_from_type(&resolved_type.specifications);
        for unit in units {
            if let Some((existing_type, existing_def)) = unit_index.get(&unit) {
                if existing_type.is_ratio() {
                    continue;
                }
                let existing_name: String = existing_def
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| existing_type.name());
                return Err(Error::validation_with_context(
                    format!(
                        "Ambiguous unit '{}'. Defined in multiple types: '{}' and '{}'",
                        unit, existing_name, defined_by.name
                    ),
                    Some(defined_by.source.clone()),
                    None::<String>,
                    Some(Arc::clone(spec)),
                    None,
                ));
            }
            unit_index.insert(unit, (resolved_type.clone(), Some(defined_by.clone())));
        }
        Ok(())
    }

    fn extract_units_from_type(specs: &TypeSpecification) -> Vec<String> {
        match specs {
            TypeSpecification::Scale { units, .. } => {
                units.iter().map(|unit| unit.name.clone()).collect()
            }
            TypeSpecification::Ratio { units, .. } => {
                units.iter().map(|unit| unit.name.clone()).collect()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod type_resolution_tests {
    use super::*;
    use crate::parse;
    use crate::parsing::ast::{
        CommandArg, LemmaSpec, ParentType, PrimitiveKind, TypeConstraintCommand,
    };
    use crate::ResourceLimits;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    fn test_context_and_effective(
        specs: &[Arc<LemmaSpec>],
    ) -> (&'static Context, &'static EffectiveDate) {
        use crate::engine::Context;
        let mut ctx = Context::new();
        for s in specs {
            ctx.insert_spec(Arc::clone(s), s.from_registry).unwrap();
        }
        let ctx = Box::leak(Box::new(ctx));
        let eff = Box::leak(Box::new(EffectiveDate::Origin));
        (ctx, eff)
    }

    fn dag_and_spec() -> (Vec<Arc<LemmaSpec>>, Arc<LemmaSpec>) {
        let spec = LemmaSpec::new("test_spec".to_string());
        let arc = Arc::new(spec);
        let dag = vec![Arc::clone(&arc)];
        (dag, arc)
    }

    fn resolver_for_code(code: &str) -> (TypeResolver<'static>, Vec<Arc<LemmaSpec>>) {
        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec_arcs: Vec<Arc<LemmaSpec>> = specs.iter().map(|s| Arc::new(s.clone())).collect();
        let dag: Vec<Arc<LemmaSpec>> = spec_arcs.iter().map(Arc::clone).collect();
        let dag = Box::leak(Box::new(dag));
        let (ctx, _) = test_context_and_effective(&spec_arcs);
        let mut resolver = TypeResolver::new(ctx, dag);
        for spec_arc in &spec_arcs {
            resolver.register_all(spec_arc);
        }
        (resolver, spec_arcs)
    }

    fn resolver_single_spec(code: &str) -> (TypeResolver<'static>, Arc<LemmaSpec>) {
        let (resolver, spec_arcs) = resolver_for_code(code);
        let spec_arc = spec_arcs.into_iter().next().expect("at least one spec");
        (resolver, spec_arc)
    }

    #[test]
    fn test_type_spec_for_primitive_covers_all_variants() {
        use crate::parsing::ast::PrimitiveKind;
        use crate::planning::semantics::type_spec_for_primitive;

        for kind in [
            PrimitiveKind::Boolean,
            PrimitiveKind::Scale,
            PrimitiveKind::Number,
            PrimitiveKind::Percent,
            PrimitiveKind::Ratio,
            PrimitiveKind::Text,
            PrimitiveKind::Date,
            PrimitiveKind::Time,
            PrimitiveKind::Duration,
        ] {
            let spec = type_spec_for_primitive(kind);
            assert!(
                !matches!(
                    spec,
                    crate::planning::semantics::TypeSpecification::Undetermined
                ),
                "type_spec_for_primitive({:?}) returned Undetermined",
                kind
            );
        }
    }

    #[test]
    fn test_register_data_type_def() {
        let (dag, spec_arc) = dag_and_spec();
        let (ctx, _) = test_context_and_effective(&dag);
        let mut resolver = TypeResolver::new(ctx, &dag);
        let ftd = DataTypeDef {
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: Some(vec![
                (
                    TypeConstraintCommand::Minimum,
                    vec![CommandArg::Literal(crate::literals::Value::Number(
                        Decimal::ZERO,
                    ))],
                ),
                (
                    TypeConstraintCommand::Maximum,
                    vec![CommandArg::Literal(crate::literals::Value::Number(
                        Decimal::from(150),
                    ))],
                ),
            ]),
            from: None,
            source: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "age".to_string(),
        };

        let result = resolver.register_type(&spec_arc, ftd);
        assert!(result.is_ok());
        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        assert!(resolved.named_types.contains_key("age"));
    }

    #[test]
    fn test_register_duplicate_type_fails() {
        let (dag, spec_arc) = dag_and_spec();
        let (ctx, _) = test_context_and_effective(&dag);
        let mut resolver = TypeResolver::new(ctx, &dag);
        let ftd = DataTypeDef {
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: None,
            from: None,
            source: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "money".to_string(),
        };

        resolver.register_type(&spec_arc, ftd.clone()).unwrap();
        let result = resolver.register_type(&spec_arc, ftd);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_custom_type_from_primitive() {
        let (dag, spec_arc) = dag_and_spec();
        let (ctx, _) = test_context_and_effective(&dag);
        let mut resolver = TypeResolver::new(ctx, &dag);
        let ftd = DataTypeDef {
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: None,
            from: None,
            source: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "money".to_string(),
        };

        resolver.register_type(&spec_arc, ftd).unwrap();
        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();

        assert!(resolved.named_types.contains_key("money"));
        let money_type = resolved.named_types.get("money").unwrap();
        assert_eq!(money_type.name, Some("number".to_string()));
    }

    #[test]
    fn test_type_definition_resolution() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data dice: number -> minimum 0 -> maximum 6"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let dice_type = resolved_types.named_types.get("dice").unwrap();

        match &dice_type.specifications {
            TypeSpecification::Number {
                minimum, maximum, ..
            } => {
                assert_eq!(*minimum, Some(Decimal::from(0)));
                assert_eq!(*maximum, Some(Decimal::from(6)));
            }
            _ => panic!("Expected Number type specifications"),
        }
    }

    #[test]
    fn test_type_definition_with_multiple_commands() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: scale -> decimals 2 -> unit eur 1.0 -> unit usd 1.18"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let money_type = resolved_types.named_types.get("money").unwrap();

        match &money_type.specifications {
            TypeSpecification::Scale {
                decimals, units, ..
            } => {
                assert_eq!(*decimals, Some(2));
                assert_eq!(units.len(), 2);
                assert!(units.iter().any(|u| u.name == "eur"));
                assert!(units.iter().any(|u| u.name == "usd"));
            }
            _ => panic!("Expected Scale type specifications"),
        }
    }

    #[test]
    fn test_number_type_with_decimals() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data price: number -> decimals 2 -> minimum 0"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let price_type = resolved_types.named_types.get("price").unwrap();

        match &price_type.specifications {
            TypeSpecification::Number {
                decimals, minimum, ..
            } => {
                assert_eq!(*decimals, Some(2));
                assert_eq!(*minimum, Some(Decimal::from(0)));
            }
            _ => panic!("Expected Number type specifications with decimals"),
        }
    }

    #[test]
    fn test_number_type_decimals_only() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data precise_number: number -> decimals 4"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let precise_type = resolved_types.named_types.get("precise_number").unwrap();

        match &precise_type.specifications {
            TypeSpecification::Number { decimals, .. } => {
                assert_eq!(*decimals, Some(4));
            }
            _ => panic!("Expected Number type with decimals 4"),
        }
    }

    #[test]
    fn test_scale_type_decimals_only() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data weight: scale -> unit kg 1 -> decimals 3"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let weight_type = resolved_types.named_types.get("weight").unwrap();

        match &weight_type.specifications {
            TypeSpecification::Scale { decimals, .. } => {
                assert_eq!(*decimals, Some(3));
            }
            _ => panic!("Expected Scale type with decimals 3"),
        }
    }

    #[test]
    fn test_ratio_type_accepts_optional_decimals_command() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data ratio_type: ratio -> decimals 2"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let ratio_type = resolved_types.named_types.get("ratio_type").unwrap();

        match &ratio_type.specifications {
            TypeSpecification::Ratio { decimals, .. } => {
                assert_eq!(
                    *decimals,
                    Some(2),
                    "ratio type should accept decimals command"
                );
            }
            _ => panic!("Expected Ratio type with decimals 2"),
        }
    }

    #[test]
    fn test_ratio_type_with_default_command() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data percentage: ratio -> minimum 0 -> maximum 1 -> default 0.5"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let percentage_type = resolved_types.named_types.get("percentage").unwrap();

        match &percentage_type.specifications {
            TypeSpecification::Ratio {
                minimum, maximum, ..
            } => {
                assert_eq!(
                    *minimum,
                    Some(Decimal::from(0)),
                    "ratio type should have minimum 0"
                );
                assert_eq!(
                    *maximum,
                    Some(Decimal::from(1)),
                    "ratio type should have maximum 1"
                );
            }
            _ => panic!("Expected Ratio type with minimum and maximum"),
        }

        let declared = resolved_types
            .declared_defaults
            .get("percentage")
            .expect("declared default must be tracked for percentage");
        match declared {
            ValueKind::Ratio(v, _) => assert_eq!(*v, Decimal::from_i128_with_scale(5, 1)),
            other => panic!("expected Ratio declared default, got {:?}", other),
        }
    }

    #[test]
    fn test_scale_extension_chain_same_family_units_allowed() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: scale -> unit eur 1
data money2: money -> unit usd 1.24"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(
            result.is_ok(),
            "Scale extension chain should resolve: {:?}",
            result.err()
        );

        let resolved = result.unwrap();
        assert!(
            resolved.unit_index.contains_key("eur"),
            "eur should be in unit_index"
        );
        assert!(
            resolved.unit_index.contains_key("usd"),
            "usd should be in unit_index"
        );
        let eur_type = resolved.unit_index.get("eur").unwrap();
        let usd_type = resolved.unit_index.get("usd").unwrap();
        assert_eq!(
            eur_type.name.as_deref(),
            Some("money"),
            "more derived type (money2) should own eur; its parent name is 'money'"
        );
        assert_eq!(
            usd_type.name.as_deref(),
            Some("money"),
            "usd defined on money2 whose parent is 'money'"
        );
    }

    #[test]
    fn test_invalid_parent_type_in_named_type_should_error() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data invalid: nonexistent_type -> minimum 0"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(result.is_err(), "Should reject invalid parent type");

        let errs = result.unwrap_err();
        assert!(!errs.is_empty(), "expected at least one error");
        let error_msg = errs[0].to_string();
        assert!(
            error_msg.contains("Unknown type") && error_msg.contains("nonexistent_type"),
            "Error should mention unknown type. Got: {}",
            error_msg
        );
    }

    #[test]
    fn test_invalid_primitive_type_name_should_error() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data invalid: choice -> option "a""#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(result.is_err(), "Should reject invalid type base 'choice'");

        let errs = result.unwrap_err();
        assert!(!errs.is_empty(), "expected at least one error");
        let error_msg = errs[0].to_string();
        assert!(
            error_msg.contains("Unknown type") && error_msg.contains("choice"),
            "Error should mention unknown type 'choice'. Got: {}",
            error_msg
        );
    }

    #[test]
    fn test_unit_constraint_validation_errors_are_reported() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: scale
  -> unit eur 1.00
  -> unit usd 1.19

data money2: money
  -> unit eur 1.20
  -> unit usd 1.21
  -> unit gbp 1.30"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(
            result.is_err(),
            "Expected unit constraint conflicts to error"
        );

        let errs = result.unwrap_err();
        assert!(!errs.is_empty(), "expected at least one error");
        let error_msg = errs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            error_msg.contains("eur") || error_msg.contains("usd"),
            "Error should mention the conflicting units. Got: {}",
            error_msg
        );
    }

    #[test]
    fn test_spec_level_unit_ambiguity_errors_are_reported() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money_a: scale
  -> unit eur 1.00
  -> unit usd 1.19

data money_b: scale
  -> unit eur 1.00
  -> unit usd 1.20

data length_a: scale
  -> unit meter 1.0

data length_b: scale
  -> unit meter 1.0"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(
            result.is_err(),
            "Expected ambiguous unit definitions to error"
        );

        let errs = result.unwrap_err();
        assert!(!errs.is_empty(), "expected at least one error");
        let error_msg = errs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            error_msg.contains("eur") || error_msg.contains("usd") || error_msg.contains("meter"),
            "Error should mention at least one ambiguous unit. Got: {}",
            error_msg
        );
    }

    #[test]
    fn test_number_type_cannot_have_units() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data price: number
  -> unit eur 1.00"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(result.is_err(), "Number types must reject unit commands");

        let errs = result.unwrap_err();
        assert!(!errs.is_empty(), "expected at least one error");
        let error_msg = errs[0].to_string();
        assert!(
            error_msg.contains("unit") && error_msg.contains("number"),
            "Error should mention units are invalid on number. Got: {}",
            error_msg
        );
    }

    #[test]
    fn test_extending_type_inherits_units() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: scale
  -> unit eur 1.00
  -> unit usd 1.19

data my_money: money
  -> unit gbp 1.30"#,
        );

        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let my_money_type = resolved.named_types.get("my_money").unwrap();

        match &my_money_type.specifications {
            TypeSpecification::Scale { units, .. } => {
                assert_eq!(units.len(), 3);
                assert!(units.iter().any(|u| u.name == "eur"));
                assert!(units.iter().any(|u| u.name == "usd"));
                assert!(units.iter().any(|u| u.name == "gbp"));
            }
            other => panic!("Expected Scale type specifications, got {:?}", other),
        }
    }

    #[test]
    fn test_duplicate_unit_in_same_type_is_rejected() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: scale
  -> unit eur 1.00
  -> unit eur 1.19"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(
            result.is_err(),
            "Duplicate units within a type should error"
        );

        let errs = result.unwrap_err();
        assert!(!errs.is_empty(), "expected at least one error");
        let error_msg = errs[0].to_string();
        assert!(
            error_msg.contains("Duplicate unit")
                || error_msg.contains("duplicate")
                || error_msg.contains("already exists")
                || error_msg.contains("eur"),
            "Error should mention duplicate unit issue. Got: {}",
            error_msg
        );
    }
}

// ============================================================================
// Validation (formerly validation.rs)
// ============================================================================

/// Validate that TypeSpecification constraints are internally consistent.
///
/// Checks range, decimals/precision, length, unit, and option constraints, and
/// validates the `declared_default` (when present) against those constraints.
/// The default lives outside the type specification (on the data binding or
/// typedef entry); callers thread it in explicitly so this function can verify
/// consistency without owning the value.
///
/// Returns a vector of errors (empty if valid).
pub fn validate_type_specifications(
    specs: &TypeSpecification,
    declared_default: Option<&ValueKind>,
    type_name: &str,
    source: &Source,
    spec_context: Option<Arc<LemmaSpec>>,
) -> Vec<Error> {
    let mut errors = Vec::new();

    match specs {
        TypeSpecification::Scale {
            minimum,
            maximum,
            decimals,
            precision,
            units,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate precision is positive if set
            if let Some(prec) = precision {
                if *prec <= Decimal::ZERO {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid precision: {}. Must be positive",
                            type_name, prec
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            if let Some(ValueKind::Scale(def_value, def_unit)) = declared_default {
                if !units.iter().any(|u| u.name == *def_unit) {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' default unit '{}' is not a valid unit. Valid units: {}",
                            type_name,
                            def_unit,
                            units
                                .iter()
                                .map(|u| u.name.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
                if let Some(min) = minimum {
                    if *def_value < *min {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} {} is less than minimum {}",
                                type_name, def_value, def_unit, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def_value > *max {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} {} is greater than maximum {}",
                                type_name, def_value, def_unit, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }

            // Scale types must have at least one unit (required for parsing and conversion)
            if units.is_empty() {
                errors.push(Error::validation_with_context(
                    format!(
                        "Type '{}' is a scale type but has no units. Scale types must define at least one unit (e.g. -> unit eur 1).",
                        type_name
                    ),
                    Some(source.clone()),
                    None::<String>,
                    spec_context.clone(),
                    None,
                ));
            }

            // Validate units (if present)
            if !units.is_empty() {
                let mut seen_names: Vec<String> = Vec::new();
                for unit in units.iter() {
                    // Validate unit name is not empty
                    if unit.name.trim().is_empty() {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' has a unit with empty name. Unit names cannot be empty.",
                                type_name
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }

                    // Validate unit names are unique within the type (case-insensitive)
                    let lower_name = unit.name.to_lowercase();
                    if seen_names
                        .iter()
                        .any(|seen| seen.to_lowercase() == lower_name)
                    {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has duplicate unit name '{}' (case-insensitive). Unit names must be unique within a type.", type_name, unit.name),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    // Validate unit values are positive (conversion factors relative to type base of 1)
                    if unit.value <= Decimal::ZERO {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has unit '{}' with invalid value {}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
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
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate precision is positive if set
            if let Some(prec) = precision {
                if *prec <= Decimal::ZERO {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid precision: {}. Must be positive",
                            type_name, prec
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            if let Some(ValueKind::Number(def)) = declared_default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
            // Note: Number types are dimensionless and cannot have units (validated in apply_constraint)
        }

        TypeSpecification::Ratio {
            minimum,
            maximum,
            decimals,
            units,
            ..
        } => {
            // Validate decimals range (0-28 is rust_decimal limit)
            if let Some(d) = decimals {
                if *d > 28 {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid decimals value: {}. Must be between 0 and 28",
                            type_name, d
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                if min > max {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid range: minimum {} is greater than maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            if let Some(ValueKind::Ratio(def, _)) = declared_default {
                if let Some(min) = minimum {
                    if *def < *min {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} is less than minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if *def > *max {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default value {} is greater than maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }

            // Validate units (if present)
            // Types can have zero units (e.g., type ratio: number -> ratio) - this is valid
            // Only validate if units are defined
            if !units.is_empty() {
                let mut seen_names: Vec<String> = Vec::new();
                for unit in units.iter() {
                    // Validate unit name is not empty
                    if unit.name.trim().is_empty() {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' has a unit with empty name. Unit names cannot be empty.",
                                type_name
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }

                    // Validate unit names are unique within the type (case-insensitive)
                    let lower_name = unit.name.to_lowercase();
                    if seen_names
                        .iter()
                        .any(|seen| seen.to_lowercase() == lower_name)
                    {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has duplicate unit name '{}' (case-insensitive). Unit names must be unique within a type.", type_name, unit.name),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    // Validate unit values are positive (conversion factors relative to type base of 1)
                    if unit.value <= Decimal::ZERO {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has unit '{}' with invalid value {}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
        }

        TypeSpecification::Text {
            length, options, ..
        } => {
            if let Some(ValueKind::Text(def)) = declared_default {
                let def_len = def.len();

                if let Some(len) = length {
                    if def_len != *len {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' default value length {} does not match required length {}", type_name, def_len, len),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if !options.is_empty() && !options.contains(def) {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' default value '{}' is not in allowed options: {:?}",
                            type_name, def, options
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }
        }

        TypeSpecification::Date {
            minimum,
            maximum,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                let min_sem = semantics::date_time_to_semantic(min);
                let max_sem = semantics::date_time_to_semantic(max);
                if semantics::compare_semantic_dates(&min_sem, &max_sem) == Ordering::Greater {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid date range: minimum {} is after maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            if let Some(ValueKind::Date(def)) = declared_default {
                if let Some(min) = minimum {
                    let min_sem = semantics::date_time_to_semantic(min);
                    if semantics::compare_semantic_dates(def, &min_sem) == Ordering::Less {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default date {} is before minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    let max_sem = semantics::date_time_to_semantic(max);
                    if semantics::compare_semantic_dates(def, &max_sem) == Ordering::Greater {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default date {} is after maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
        }

        TypeSpecification::Time {
            minimum,
            maximum,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                let min_sem = semantics::time_to_semantic(min);
                let max_sem = semantics::time_to_semantic(max);
                if semantics::compare_semantic_times(&min_sem, &max_sem) == Ordering::Greater {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Type '{}' has invalid time range: minimum {} is after maximum {}",
                            type_name, min, max
                        ),
                        Some(source.clone()),
                        None::<String>,
                        spec_context.clone(),
                        None,
                    ));
                }
            }

            if let Some(ValueKind::Time(def)) = declared_default {
                if let Some(min) = minimum {
                    let min_sem = semantics::time_to_semantic(min);
                    if semantics::compare_semantic_times(def, &min_sem) == Ordering::Less {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default time {} is before minimum {}",
                                type_name, def, min
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
                if let Some(max) = maximum {
                    let max_sem = semantics::time_to_semantic(max);
                    if semantics::compare_semantic_times(def, &max_sem) == Ordering::Greater {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' default time {} is after maximum {}",
                                type_name, def, max
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
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
        TypeSpecification::Undetermined => unreachable!(
            "BUG: validate_type_specification_constraints called with Undetermined sentinel type; this type exists only during type inference"
        ),
    }

    errors
}

/// Validate that a registry spec (`from_registry == true`) does not contain
/// bare (non-`@`) references. The registry is responsible for rewriting all
/// spec references to use `@`-prefixed names before serving the bundle.
///
/// Returns a list of bare reference names found, empty if valid.
pub fn collect_bare_registry_refs(spec: &LemmaSpec) -> Vec<String> {
    if !spec.from_registry {
        return Vec::new();
    }
    let mut bare: Vec<String> = Vec::new();
    for data in &spec.data {
        match &data.value {
            ParsedDataValue::SpecReference(r) if !r.from_registry => {
                bare.push(r.name.clone());
            }
            ParsedDataValue::TypeDeclaration { from: Some(r), .. } if !r.from_registry => {
                bare.push(r.name.clone());
            }
            _ => {}
        }
    }
    bare
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use crate::parsing::ast::{CommandArg, TypeConstraintCommand};
    use crate::planning::semantics::TypeSpecification;
    use rust_decimal::Decimal;

    fn test_source() -> Source {
        Source::new(
            "<test>",
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        )
    }

    fn apply(
        specs: TypeSpecification,
        command: TypeConstraintCommand,
        args: &[CommandArg],
    ) -> TypeSpecification {
        let mut default = None;
        specs.apply_constraint(command, args, &mut default).unwrap()
    }

    fn number_arg(n: i64) -> CommandArg {
        CommandArg::Literal(crate::literals::Value::Number(Decimal::from(n)))
    }

    fn date_arg(s: &str) -> CommandArg {
        let dt = s.parse::<crate::literals::DateTimeValue>().expect("date");
        CommandArg::Literal(crate::literals::Value::Date(dt))
    }

    fn time_arg(s: &str) -> CommandArg {
        let t = s.parse::<crate::literals::TimeValue>().expect("time");
        CommandArg::Literal(crate::literals::Value::Time(t))
    }

    #[test]
    fn validate_number_minimum_greater_than_maximum() {
        let mut specs = TypeSpecification::number();
        specs = apply(specs, TypeConstraintCommand::Minimum, &[number_arg(100)]);
        specs = apply(specs, TypeConstraintCommand::Maximum, &[number_arg(50)]);

        let src = test_source();
        let errors = validate_type_specifications(&specs, None, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum 100 is greater than maximum 50"));
    }

    #[test]
    fn validate_number_default_below_minimum() {
        let specs = TypeSpecification::Number {
            minimum: Some(Decimal::from(10)),
            maximum: None,
            decimals: None,
            precision: None,
            help: String::new(),
        };
        let default = ValueKind::Number(Decimal::from(5));

        let src = test_source();
        let errors = validate_type_specifications(&specs, Some(&default), "test", &src, None);
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
            help: String::new(),
        };
        let default = ValueKind::Number(Decimal::from(150));

        let src = test_source();
        let errors = validate_type_specifications(&specs, Some(&default), "test", &src, None);
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
            help: String::new(),
        };
        let default = ValueKind::Number(Decimal::from(50));

        let src = test_source();
        let errors = validate_type_specifications(&specs, Some(&default), "test", &src, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn text_minimum_command_is_rejected() {
        let specs = TypeSpecification::text();
        let res =
            specs.apply_constraint(TypeConstraintCommand::Minimum, &[number_arg(5)], &mut None);
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("Invalid command 'minimum' for text type"));
    }

    #[test]
    fn text_maximum_command_is_rejected() {
        let specs = TypeSpecification::text();
        let res =
            specs.apply_constraint(TypeConstraintCommand::Maximum, &[number_arg(5)], &mut None);
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("Invalid command 'maximum' for text type"));
    }

    #[test]
    fn validate_text_default_not_in_options() {
        let specs = TypeSpecification::Text {
            length: None,
            options: vec!["red".to_string(), "blue".to_string()],
            help: String::new(),
        };
        let default = ValueKind::Text("green".to_string());

        let src = test_source();
        let errors = validate_type_specifications(&specs, Some(&default), "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("default value 'green' is not in allowed options"));
    }

    #[test]
    fn validate_ratio_minimum_greater_than_maximum() {
        let specs = TypeSpecification::Ratio {
            minimum: Some(Decimal::from(2)),
            maximum: Some(Decimal::from(1)),
            decimals: None,
            units: crate::planning::semantics::RatioUnits::new(),
            help: String::new(),
        };

        let src = test_source();
        let errors = validate_type_specifications(&specs, None, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0]
            .to_string()
            .contains("minimum 2 is greater than maximum 1"));
    }

    #[test]
    fn validate_date_minimum_after_maximum() {
        let mut specs = TypeSpecification::date();
        specs = apply(
            specs,
            TypeConstraintCommand::Minimum,
            &[date_arg("2024-12-31")],
        );
        specs = apply(
            specs,
            TypeConstraintCommand::Maximum,
            &[date_arg("2024-01-01")],
        );

        let src = test_source();
        let errors = validate_type_specifications(&specs, None, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].to_string().contains("minimum")
                && errors[0].to_string().contains("is after maximum")
        );
    }

    #[test]
    fn validate_date_valid_range() {
        let mut specs = TypeSpecification::date();
        specs = apply(
            specs,
            TypeConstraintCommand::Minimum,
            &[date_arg("2024-01-01")],
        );
        specs = apply(
            specs,
            TypeConstraintCommand::Maximum,
            &[date_arg("2024-12-31")],
        );

        let src = test_source();
        let errors = validate_type_specifications(&specs, None, "test", &src, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_time_minimum_after_maximum() {
        let mut specs = TypeSpecification::time();
        specs = apply(
            specs,
            TypeConstraintCommand::Minimum,
            &[time_arg("23:00:00")],
        );
        specs = apply(
            specs,
            TypeConstraintCommand::Maximum,
            &[time_arg("10:00:00")],
        );

        let src = test_source();
        let errors = validate_type_specifications(&specs, None, "test", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].to_string().contains("minimum")
                && errors[0].to_string().contains("is after maximum")
        );
    }
}
