use crate::engine::Context;
use crate::parsing::ast::{
    self as ast, CalendarUnit, CommandArg, Constraint, EffectiveDate, FillRhs, LemmaData,
    LemmaRepository, LemmaRule, LemmaSpec, MetaValue, ParentType, PrimitiveKind,
    TypeConstraintCommand, Value,
};
use crate::parsing::source::Source;
use crate::planning::discovery;
use crate::planning::semantics::{
    self, calendar_decomposition, combine_decompositions, conversion_target_to_semantic,
    duration_decomposition, number_with_unit_to_value_kind, parser_value_to_value_kind,
    primitive_boolean, primitive_calendar, primitive_calendar_range, primitive_date,
    primitive_date_range, primitive_number, primitive_number_range, primitive_text, primitive_time,
    value_to_semantic, ArithmeticComputation, BaseQuantityVector, ComparisonComputation,
    DataDefinition, DataPath, Expression, ExpressionKind, LemmaType, LiteralValue, PathSegment,
    ReferenceTarget, RulePath, SemanticConversionTarget, TypeDefiningSpec, TypeExtends,
    TypeSpecification, ValueKind,
};
use crate::Error;
use ast::DataValue as ParsedDataValue;
use indexmap::IndexMap;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::Arc;

/// Data bindings map: maps a target data name path to the binding's value and source.
///
/// The key is the full path of **data names** from the root spec to the target data.
/// Spec set names are intentionally excluded from the key because spec ref bindings may change
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

    /// Build the data map: one entry per data (Value or Import), with defaults and coercion applied.
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
                DataDefinition::Import { spec: spec_arc, .. } => {
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
                    DataDefinition::Import {
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

    pub(crate) fn coerce_literal_to_schema_type(
        lit: &LiteralValue,
        schema_type: &LemmaType,
    ) -> Result<LiteralValue, String> {
        fn range_endpoint_schema_type(schema_type: &LemmaType) -> Option<LemmaType> {
            match &schema_type.specifications {
                TypeSpecification::NumberRange { .. } => {
                    Some(LemmaType::primitive(TypeSpecification::number()))
                }
                TypeSpecification::DateRange { .. } => {
                    Some(LemmaType::primitive(TypeSpecification::date()))
                }
                TypeSpecification::RatioRange { units, .. } => {
                    Some(LemmaType::primitive(TypeSpecification::Ratio {
                        minimum: None,
                        maximum: None,
                        decimals: None,
                        units: units.clone(),
                        help: String::new(),
                    }))
                }
                TypeSpecification::CalendarRange { .. } => {
                    Some(LemmaType::primitive(TypeSpecification::calendar()))
                }
                TypeSpecification::QuantityRange {
                    units,
                    decomposition,
                    canonical_unit,
                    ..
                } => Some(LemmaType::primitive(TypeSpecification::Quantity {
                    minimum: None,
                    maximum: None,
                    decimals: None,
                    units: units.clone(),
                    traits: Vec::new(),
                    decomposition: decomposition.clone(),
                    canonical_unit: canonical_unit.clone(),
                    help: String::new(),
                })),
                _ => None,
            }
        }

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
            | (TypeSpecification::Calendar { .. }, ValueKind::Calendar(_, _)) => {
                let mut out = lit.clone();
                out.lemma_type = schema_type.clone();
                Ok(out)
            }
            (TypeSpecification::Quantity { units, .. }, ValueKind::Quantity(_, unit_name, _)) => {
                if !units.iter().any(|u| u.name.eq_ignore_ascii_case(unit_name)) {
                    return Err(format!(
                        "value {} cannot be used as type {}: unknown unit '{}'",
                        lit,
                        schema_type.name(),
                        unit_name
                    ));
                }
                let mut out = lit.clone();
                out.lemma_type = schema_type.clone();
                Ok(out)
            }
            (TypeSpecification::Ratio { units, .. }, ValueKind::Ratio(_, unit_name)) => {
                if let Some(unit_name) = unit_name {
                    if !units.iter().any(|u| u.name.eq_ignore_ascii_case(unit_name)) {
                        return Err(format!(
                            "value {} cannot be used as type {}: unknown unit '{}'",
                            lit,
                            schema_type.name(),
                            unit_name
                        ));
                    }
                }
                let mut out = lit.clone();
                out.lemma_type = schema_type.clone();
                Ok(out)
            }
            (
                TypeSpecification::NumberRange { .. }
                | TypeSpecification::DateRange { .. }
                | TypeSpecification::RatioRange { .. }
                | TypeSpecification::CalendarRange { .. }
                | TypeSpecification::QuantityRange { .. },
                ValueKind::Range(left, right),
            ) => {
                let endpoint_schema_type =
                    range_endpoint_schema_type(schema_type).unwrap_or_else(|| {
                        unreachable!("BUG: range_endpoint_schema_type missing range schema arm")
                    });
                let coerced_left =
                    Self::coerce_literal_to_schema_type(left.as_ref(), &endpoint_schema_type)?;
                let coerced_right =
                    Self::coerce_literal_to_schema_type(right.as_ref(), &endpoint_schema_type)?;
                Ok(LiteralValue {
                    value: ValueKind::Range(Box::new(coerced_left), Box::new(coerced_right)),
                    lemma_type: schema_type.clone(),
                })
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
                let constraint_type_name = merged.name();
                match apply_constraints_to_spec(
                    &self.main_spec,
                    &constraint_type_name,
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
                    let constraint_type_name = merged.name();
                    match apply_constraints_to_spec(
                        &self.main_spec,
                        &constraint_type_name,
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
                let constraint_type_name = merged.name();
                match apply_constraints_to_spec(
                    &self.main_spec,
                    &constraint_type_name,
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

    /// Spec this rule belongs to (for type resolution during validation)
    pub spec_arc: Arc<LemmaSpec>,
}

type ResolvedTypesMap = Vec<(Arc<LemmaRepository>, Arc<LemmaSpec>, ResolvedSpecTypes)>;

struct GraphBuilder<'a> {
    data: IndexMap<DataPath, DataDefinition>,
    rules: BTreeMap<RulePath, RuleNode>,
    context: &'a Context,
    local_types: ResolvedTypesMap,
    errors: Vec<Error>,
    main_spec: Arc<LemmaSpec>,
    main_repository: Arc<ast::LemmaRepository>,
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
/// 1. matching base type spec (number / quantity / text / ratio / …) — see
///    [`LemmaType::has_same_base_type`]; and
/// 2. for quantity types, matching quantity family — see
///    [`LemmaType::same_quantity_family`]. Two quantities in different families
///    (e.g. `eur` vs `celsius`) share the `Quantity` discriminant but are not
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
    if lhs.is_quantity() && !lhs.same_quantity_family(target_type) {
        let lhs_family = lhs.quantity_family_name().expect(
            "BUG: declared quantity data must carry a family name; \
             anonymous quantity types only arise from runtime synthesis \
             and never appear as a reference's LHS-declared type",
        );
        let target_family = target_type.quantity_family_name().expect(
            "BUG: declared quantity data must carry a family name; \
             anonymous quantity types only arise from runtime synthesis \
             and never appear as a reference target's schema type",
        );
        return Some(format!(
            "Data reference '{}' quantity family mismatch: declared as '{}' (family '{}') but {} '{}' is '{}' (family '{}')",
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

/// Type name shown in `-> default` constraint errors (the declared type, not the data slot).
fn constraint_application_type_name(parent: &ParentType, data_name: &str) -> String {
    match parent {
        ParentType::Custom { name } => name.clone(),
        ParentType::Qualified { inner, .. } => constraint_application_type_name(inner, data_name),
        ParentType::Primitive { .. } => data_name.to_string(),
    }
}

/// Fold a list of definition-style constraints into a [`TypeSpecification`].
/// Used for both the GraphBuilder's regular TypeDeclaration path and the
/// post-build reference type-merging pass, so the underlying constraint
/// application logic stays in one place.
fn apply_constraints_to_spec(
    spec: &Arc<LemmaSpec>,
    type_name: &str,
    mut specs: TypeSpecification,
    constraints: &[Constraint],
    source: &crate::parsing::source::Source,
    declared_default: &mut Option<ValueKind>,
) -> Result<TypeSpecification, Vec<Error>> {
    let mut errors = Vec::new();
    let mut apply_one = |specs: TypeSpecification,
                         command: TypeConstraintCommand,
                         args: &[CommandArg],
                         declared_default: &mut Option<ValueKind>|
     -> TypeSpecification {
        let specs_clone = specs.clone();
        let mut default_before = declared_default.clone();
        match specs.apply_constraint(type_name, command, args, &mut default_before) {
            Ok(updated_specs) => {
                *declared_default = default_before;
                updated_specs
            }
            Err(e) => {
                errors.push(Error::validation_with_context(
                    format!("Failed to apply constraint '{}': {}", command, e),
                    Some(source.clone()),
                    None::<String>,
                    Some(Arc::clone(spec)),
                    None,
                ));
                specs_clone
            }
        }
    };

    let mut deferred: Vec<(TypeConstraintCommand, Vec<CommandArg>)> = Vec::new();
    for (command, args) in constraints {
        if matches!(
            command,
            TypeConstraintCommand::Unit | TypeConstraintCommand::Trait
        ) {
            specs = apply_one(specs, *command, args, declared_default);
        } else {
            deferred.push((*command, args.clone()));
        }
    }
    for (command, args) in deferred {
        specs = apply_one(specs, command, &args, declared_default);
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
        repository: &Arc<LemmaRepository>,
        main_spec: &Arc<LemmaSpec>,
        dag: &[(Arc<LemmaRepository>, Arc<LemmaSpec>)],
        effective: &EffectiveDate,
    ) -> Result<(Graph, ResolvedTypesMap), Vec<Error>> {
        let mut type_resolver = TypeResolver::new(context);

        let mut type_errors: Vec<Error> = Vec::new();
        for (repo, spec) in dag {
            type_errors.extend(type_resolver.register_all(repo, spec));
        }

        let (data, rules, graph_errors, local_types) = {
            let mut builder = GraphBuilder {
                data: IndexMap::new(),
                rules: BTreeMap::new(),
                context,
                local_types: Vec::new(),
                errors: Vec::new(),
                main_spec: Arc::clone(main_spec),
                main_repository: Arc::clone(repository),
            };

            builder.build_spec(
                main_spec,
                repository,
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

fn uses_import_surface_syntax(alias: &str, target_spec: &str) -> String {
    if alias == target_spec {
        format!("uses {alias}")
    } else {
        format!("uses {alias}: {target_spec}")
    }
}

fn is_uses_vs_data_clash(existing: &DataDefinition, incoming: &ParsedDataValue) -> bool {
    matches!(
        (existing, incoming),
        (
            DataDefinition::Import { .. },
            ParsedDataValue::Definition { .. }
        )
    ) || matches!(
        (existing, incoming),
        (
            DataDefinition::TypeDeclaration { .. } | DataDefinition::Value { .. },
            ParsedDataValue::Import(_)
        )
    )
}

fn qualified_type_name_from_definition(incoming: &ParsedDataValue) -> Option<&str> {
    let ParsedDataValue::Definition {
        base: Some(ParentType::Qualified { inner, .. }),
        ..
    } = incoming
    else {
        return None;
    };
    match inner.as_ref() {
        ParentType::Custom { name } => Some(name.as_str()),
        _ => None,
    }
}

fn uses_vs_data_duplicate_message(
    name: &str,
    existing: &DataDefinition,
    incoming: &ParsedDataValue,
) -> (String, Option<String>) {
    let (alias, target_spec) = match (existing, incoming) {
        (DataDefinition::Import { spec, .. }, ParsedDataValue::Definition { .. }) => {
            (name, spec.name.as_str())
        }
        (
            DataDefinition::TypeDeclaration { .. } | DataDefinition::Value { .. },
            ParsedDataValue::Import(spec_ref),
        ) => (name, spec_ref.name.as_str()),
        _ => unreachable!("uses_vs_data_duplicate_message requires a uses vs data clash"),
    };
    let uses_syntax = uses_import_surface_syntax(alias, target_spec);
    let import_alias = format!("{alias}_spec");
    let message = format!(
        "You used the name `{alias}` in both `{uses_syntax}` and `data {alias}`. A `uses` import and a `data` definition can't share the same name.",
    );
    let suggestion = match qualified_type_name_from_definition(incoming) {
        Some(type_name) => format!(
            "Try `uses {import_alias}: {target_spec}` and `data {alias}: {import_alias}.{type_name}`."
        ),
        _ => format!("Try `uses {import_alias}: {target_spec}` with a different name than `{alias}`."),
    };
    (message, Some(suggestion))
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
        consumer_spec: &Arc<LemmaSpec>,
        consumer_repository: &Arc<LemmaRepository>,
    ) -> Result<(Arc<LemmaRepository>, Arc<LemmaSpec>), Error> {
        discovery::resolve_spec_ref(
            self.context,
            spec_ref,
            consumer_repository,
            consumer_spec,
            effective,
            None,
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
        let binding_path_display = format!("{}", data.reference);

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
                ParsedDataValue::Import(sr) => sr,
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

            let walk_repository = discovery::lookup_owning_repository(self.context, &walk_spec)
                .unwrap_or_else(|| Arc::clone(&self.main_repository));
            walk_spec =
                match self.resolve_spec_ref(spec_ref, effective, &walk_spec, &walk_repository) {
                    Ok((_, arc)) => arc,
                    Err(e) => {
                        self.errors.push(e);
                        return None;
                    }
                };
        }

        if !walk_spec
            .data
            .iter()
            .any(|d| d.reference.segments.is_empty() && d.reference.name == data.reference.name)
        {
            self.errors.push(self.engine_error(
                format!(
                    "Data binding path '{}': data '{}' not found in spec '{}'",
                    binding_path_display, data.reference.name, walk_spec.name
                ),
                &data.source_location,
            ));
            return None;
        }

        // Build the binding key: current_segment_names ++ data.reference.segments ++ [data.reference.name]
        let mut binding_key: Vec<String> = current_segment_names.to_vec();
        binding_key.extend(data.reference.segments.iter().cloned());
        binding_key.push(data.reference.name.clone());

        let binding_value = match &data.value {
            ParsedDataValue::Fill(FillRhs::Literal(v)) => BindingValue::Literal(v.clone()),
            ParsedDataValue::Fill(FillRhs::Reference { target }) => {
                let resolved_target = self.resolve_reference_target_in_spec(
                    target,
                    &data.source_location,
                    parent_spec,
                    current_segment_names,
                    effective,
                )?;
                BindingValue::Reference {
                    target: resolved_target,
                    constraints: None,
                }
            }
            ParsedDataValue::Definition { value: Some(v), .. }
                if data.value.is_definition_literal_only() =>
            {
                BindingValue::Literal(v.clone())
            }
            ParsedDataValue::Import(_) => {
                unreachable!(
                    "BUG: build_data_bindings must reject Import bindings before calling resolve_data_binding"
                );
            }
            ParsedDataValue::Definition { .. } => {
                unreachable!(
                    "BUG: build_data_bindings must reject non-literal Definition bindings before calling resolve_data_binding"
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
            Arc::clone(containing_spec_arc),
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
    /// and collect into a DataBindings map. Rejects non-literal Definition binding values and
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
            let has_binding_lhs_segments = !data.reference.segments.is_empty();
            let is_local_fill = matches!(&data.value, ParsedDataValue::Fill(_));
            if !has_binding_lhs_segments && !is_local_fill {
                continue;
            }

            let binding_path_display = format!("{}", data.reference);

            // Reject spec reference as binding value — spec injection is not supported
            if matches!(&data.value, ParsedDataValue::Import(_)) {
                errors.push(self.engine_error(
                    format!(
                        "Data binding '{}' cannot override a spec reference — only literal values can be bound to nested data",
                        binding_path_display
                    ),
                    &data.source_location,
                ));
                continue;
            }

            // Reject non-literal Definition as binding value (explicit types / imports / constrained defs).
            if has_binding_lhs_segments {
                if let ParsedDataValue::Definition { .. } = &data.value {
                    if !data.value.is_definition_literal_only() {
                        errors.push(self.engine_error(
                            format!(
                                "Data binding '{}' must provide a literal value, not a data definition",
                                binding_path_display
                            ),
                            &data.source_location,
                        ));
                        continue;
                    }
                }
            }

            if let Some((binding_key, binding_value, source)) =
                self.resolve_data_binding(data, current_segment_names, spec_arc, effective)
            {
                if let Some((_, existing_source)) = bindings.get(&binding_key) {
                    errors.push(self.engine_error(
                        format!(
                            "Duplicate data binding for '{}' (previously bound at {}:{})",
                            binding_key.join("."),
                            existing_source.source_type,
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
        if let Some(existing) = self.data.get(&data_path) {
            let (message, suggestion) = if is_uses_vs_data_clash(existing, &data.value) {
                uses_vs_data_duplicate_message(&data_path.data, existing, &data.value)
            } else {
                (
                    format!(
                        "The name '{}' is already used for data in this spec.",
                        data_path.data
                    ),
                    None,
                )
            };
            self.errors.push(Error::validation_with_context(
                message,
                Some(data.source_location.clone()),
                suggestion,
                Some(Arc::clone(&self.main_spec)),
                None,
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
        //
        // When `data here: T -> …` and `fill here: ref` coexist, the `data` row
        // owns type/default; `fill` is applied in `materialize_local_fill_rows`.
        let binding_override: Option<(BindingValue, Source)> =
            data_bindings.get(&binding_key).and_then(|(v, s)| {
                if matches!(v, BindingValue::Reference { .. })
                    && Self::has_local_fill_reference_for_name(
                        current_spec_arc.as_ref(),
                        &data.reference.name,
                    )
                {
                    return None;
                }
                used_binding_keys.insert(binding_key.clone());
                Some((v.clone(), s.clone()))
            });

        let (original_schema_type, original_declared_default) = if matches!(
            &data.value,
            ParsedDataValue::Definition { .. }
        ) && data
            .value
            .definition_needs_type_resolution()
        {
            let resolved = self
                .local_types
                .iter()
                .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                .map(|(_, _, t)| t)
                .expect("BUG: no resolved types for spec during add_local_data");
            let lemma_type = resolved
                    .resolved
                    .get(&data.reference.name)
                    .expect("BUG: type not in ResolvedSpecTypes.resolved. TypeResolver should have registered it")
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
            ParsedDataValue::Definition { .. } if data.value.is_definition_literal_only() => {
                let ParsedDataValue::Definition {
                    value: Some(value), ..
                } = &data.value
                else {
                    unreachable!("BUG: literal-only Definition must carry value");
                };
                self.insert_literal_data(
                    data_path,
                    value,
                    original_schema_type,
                    effective_source,
                    current_spec_arc,
                );
            }
            ParsedDataValue::Definition { .. } => {
                let mut resolved_type = original_schema_type.unwrap_or_else(|| {
                    unreachable!(
                        "BUG: Definition without schema — TypeResolver should have registered it"
                    )
                });
                let mut declared_default = original_declared_default;

                let is_generic_quantity_range = matches!(
                    &resolved_type.specifications,
                    TypeSpecification::QuantityRange {
                        units,
                        decomposition,
                        canonical_unit,
                        ..
                    } if units.0.is_empty() && decomposition.is_empty() && canonical_unit.is_empty()
                );

                if is_generic_quantity_range {
                    if let Some(ValueKind::Range(left, right)) = &declared_default {
                        if let (
                            ValueKind::Quantity(_, left_unit, _),
                            ValueKind::Quantity(_, right_unit, _),
                        ) = (&left.value, &right.value)
                        {
                            let resolved = self
                                .local_types
                                .iter()
                                .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                                .map(|(_, _, t)| t)
                                .expect("BUG: no resolved types for spec during add_local_data");

                            let left_quantity_type = resolved.unit_index.get(left_unit);
                            let right_quantity_type = resolved.unit_index.get(right_unit);

                            match (left_quantity_type, right_quantity_type) {
                                (Some(left_quantity_type), Some(right_quantity_type))
                                    if left_quantity_type
                                        .same_quantity_family(right_quantity_type) =>
                                {
                                    let specialized_range_type =
                                        infer_range_type_from_endpoint_types(
                                            left_quantity_type,
                                            right_quantity_type,
                                        );
                                    let coerced_left = Graph::coerce_literal_to_schema_type(
                                        left.as_ref(),
                                        left_quantity_type,
                                    )
                                    .unwrap_or_else(|message| {
                                        unreachable!(
                                            "BUG: coercing quantity range default left endpoint failed: {}",
                                            message
                                        )
                                    });
                                    let coerced_right = Graph::coerce_literal_to_schema_type(
                                        right.as_ref(),
                                        right_quantity_type,
                                    )
                                    .unwrap_or_else(|message| {
                                        unreachable!(
                                            "BUG: coercing quantity range default right endpoint failed: {}",
                                            message
                                        )
                                    });
                                    let specialized_default = Graph::coerce_literal_to_schema_type(
                                        &LiteralValue {
                                            value: ValueKind::Range(
                                                Box::new(coerced_left),
                                                Box::new(coerced_right),
                                            ),
                                            lemma_type: specialized_range_type.clone(),
                                        },
                                        &specialized_range_type,
                                    )
                                    .unwrap_or_else(|message| {
                                        unreachable!(
                                            "BUG: specializing generic quantity range default failed: {}",
                                            message
                                        )
                                    });
                                    resolved_type = specialized_range_type;
                                    declared_default = Some(specialized_default.value);
                                }
                                _ => {
                                    self.errors.push(self.engine_error(
                                        format!(
                                            "Generic quantity range default must use units from one concrete local quantity family, got '{}' and '{}'",
                                            left_unit, right_unit
                                        ),
                                        &effective_source,
                                    ));
                                    return;
                                }
                            }
                        }
                    }
                }

                self.data.insert(
                    data_path,
                    DataDefinition::TypeDeclaration {
                        resolved_type,
                        declared_default,
                        source: effective_source,
                    },
                );
            }
            ParsedDataValue::Import(spec_ref) => {
                let consumer_repository =
                    discovery::lookup_owning_repository(self.context, current_spec_arc)
                        .unwrap_or_else(|| Arc::clone(&self.main_repository));
                let effective_spec_arc = match self.resolve_spec_ref(
                    spec_ref,
                    effective,
                    current_spec_arc,
                    &consumer_repository,
                ) {
                    Ok((_, arc)) => arc,
                    Err(e) => {
                        self.errors.push(e);
                        return;
                    }
                };

                self.data.insert(
                    data_path,
                    DataDefinition::Import {
                        spec: Arc::clone(&effective_spec_arc),
                        source: effective_source,
                    },
                );
            }
            ParsedDataValue::Fill(_) => {
                self.errors.push(self.engine_error(
                    "Internal planning error: a fill row reached add_data; fill rows must apply only through data_bindings"
                        .to_string(),
                    &effective_source,
                ));
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
        let semantic_value = if let Some(ref schema) = declared_schema_type {
            match parser_value_to_value_kind(value, &schema.specifications) {
                Ok(s) => s,
                Err(e) => {
                    self.errors.push(self.engine_error(e, &effective_source));
                    return;
                }
            }
        } else {
            match value {
                Value::NumberWithUnit(magnitude, unit) => {
                    let Some(lt) = self
                        .local_types
                        .iter()
                        .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                        .map(|(_, _, t)| t)
                        .and_then(|dt| dt.unit_index.get(unit))
                    else {
                        self.errors.push(self.engine_error(
                            format!("Unit '{}' is not in scope for this spec", unit),
                            &effective_source,
                        ));
                        return;
                    };
                    match number_with_unit_to_value_kind(*magnitude, unit, lt) {
                        Ok(s) => s,
                        Err(e) => {
                            self.errors.push(self.engine_error(e, &effective_source));
                            return;
                        }
                    }
                }
                _ => match value_to_semantic(value) {
                    Ok(s) => s,
                    Err(e) => {
                        self.errors.push(self.engine_error(e, &effective_source));
                        return;
                    }
                },
            }
        };
        let inferred_type = match value {
            Value::Text(_) => primitive_text().clone(),
            Value::Number(_) => primitive_number().clone(),
            Value::NumberWithUnit(_, unit) => {
                match self
                    .local_types
                    .iter()
                    .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                    .map(|(_, _, t)| t)
                    .and_then(|dt| dt.unit_index.get(unit))
                {
                    Some(lt) => lt.clone(),
                    None => {
                        self.errors.push(self.engine_error(
                            format!("Unit '{}' is not in scope for this spec", unit),
                            &effective_source,
                        ));
                        return;
                    }
                }
            }
            Value::Boolean(_) => primitive_boolean().clone(),
            Value::Date(_) => primitive_date().clone(),
            Value::Time(_) => primitive_time().clone(),
            Value::Calendar(_, _) => primitive_calendar().clone(),
            Value::Range(_, _) => match &semantic_value {
                ValueKind::Range(left, right) => {
                    LiteralValue::range(left.as_ref().clone(), right.as_ref().clone()).lemma_type
                }
                _ => unreachable!(
                    "BUG: semantic range literal conversion returned non-range value kind"
                ),
            },
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
        mut spec_context: Arc<LemmaSpec>,
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

            if let ParsedDataValue::Import(original_spec_ref) = &data_ref.value {
                let context_repository =
                    discovery::lookup_owning_repository(self.context, &spec_context)
                        .unwrap_or_else(|| Arc::clone(&self.main_repository));
                let arc = match self.resolve_spec_ref(
                    original_spec_ref,
                    effective,
                    &spec_context,
                    &context_repository,
                ) {
                    Ok((_, a)) => a,
                    Err(e) => {
                        self.errors.push(e);
                        return None;
                    }
                };
                spec_context = Arc::clone(&arc);

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

    fn has_local_fill_reference_for_name(spec: &LemmaSpec, name: &str) -> bool {
        spec.data.iter().any(|d| {
            d.reference.segments.is_empty()
                && d.reference.name == name
                && matches!(&d.value, ParsedDataValue::Fill(FillRhs::Reference { .. }))
        })
    }

    /// Insert graph entries for local `fill name: …` rows (empty LHS segments) that are not
    /// already present after the `add_data` pass (`data` + `fill` override or type-only `data`).
    fn materialize_local_fill_rows(
        &mut self,
        spec: &LemmaSpec,
        current_segments: &[PathSegment],
        effective_bindings: &DataBindings,
        spec_arc: &Arc<LemmaSpec>,
        used_binding_keys: &mut HashSet<Vec<String>>,
    ) {
        let current_segment_names: Vec<String> =
            current_segments.iter().map(|s| s.data.clone()).collect();

        for data in &spec.data {
            if !data.reference.segments.is_empty() {
                continue;
            }
            if !matches!(&data.value, ParsedDataValue::Fill(_)) {
                continue;
            }

            let data_path = DataPath {
                segments: current_segments.to_vec(),
                data: data.reference.name.clone(),
            };

            let binding_key: Vec<String> = current_segment_names
                .iter()
                .cloned()
                .chain(std::iter::once(data.reference.name.clone()))
                .collect();

            let Some((binding_value, binding_source)) = effective_bindings.get(&binding_key) else {
                self.errors.push(self.engine_error(
                    format!(
                        "Internal planning error: fill '{}' has no resolved binding",
                        data.reference.name
                    ),
                    &data.source_location,
                ));
                continue;
            };

            used_binding_keys.insert(binding_key);

            if let Some(DataDefinition::TypeDeclaration {
                resolved_type,
                declared_default,
                ..
            }) = self.data.get(&data_path)
            {
                let BindingValue::Reference {
                    target,
                    constraints,
                } = binding_value
                else {
                    continue;
                };
                if constraints.is_some() {
                    self.errors.push(self.engine_error(
                        format!(
                            "Constraint chains (`-> ...`) on `fill` are not allowed; use `data {}: … -> …` for constraints",
                            data.reference.name
                        ),
                        &data.source_location,
                    ));
                    continue;
                }
                let resolved_type = resolved_type.clone();
                let declared_default = declared_default.clone();
                self.data.insert(
                    data_path.clone(),
                    DataDefinition::Reference {
                        target: target.clone(),
                        resolved_type,
                        local_constraints: None,
                        local_default: declared_default,
                        source: binding_source.clone(),
                    },
                );
                continue;
            }

            if self.data.contains_key(&data_path) {
                continue;
            }

            self.add_data_from_binding(
                data_path,
                binding_value.clone(),
                binding_source.clone(),
                None,
                spec_arc,
            );
        }
    }

    fn build_spec(
        &mut self,
        spec_arc: &Arc<LemmaSpec>,
        spec_repository: &Arc<LemmaRepository>,
        current_segments: Vec<PathSegment>,
        data_bindings: DataBindings,
        effective: &EffectiveDate,
        type_resolver: &mut TypeResolver<'a>,
    ) -> Result<(), Vec<Error>> {
        let spec = spec_arc.as_ref();

        if current_segments.is_empty() {
            self.process_meta_fields(spec);
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

        if !self
            .local_types
            .iter()
            .any(|(_, s, _)| Arc::ptr_eq(s, spec_arc))
        {
            // Spec wasn't in the DAG (e.g. a sibling import failed during
            // DAG construction). The real error is already collected; skip
            // this spec to avoid resolving against unregistered types.
            if !type_resolver.is_registered(spec_arc) {
                return Ok(());
            }
            match type_resolver.resolve_and_validate(spec_arc, effective) {
                Ok(resolved_types) => {
                    self.local_types.push((
                        Arc::clone(spec_repository),
                        Arc::clone(spec_arc),
                        resolved_types,
                    ));
                }
                Err(es) => {
                    self.errors.extend(es);
                    return Ok(());
                }
            }
        }

        for data in &spec.data {
            if let ParsedDataValue::Definition {
                base: Some(ParentType::Qualified { spec_alias, .. }),
                ..
            } = &data.value
            {
                let from_ref = ast::SpecRef::same_repository(spec_alias.clone());
                match self.resolve_spec_ref(&from_ref, effective, spec_arc, spec_repository) {
                    Ok((source_repo, source_arc)) => {
                        if !self
                            .local_types
                            .iter()
                            .any(|(_, s, _)| Arc::ptr_eq(s, &source_arc))
                        {
                            match type_resolver.resolve_and_validate(&source_arc, effective) {
                                Ok(resolved_types) => {
                                    self.local_types.push((
                                        source_repo,
                                        source_arc,
                                        resolved_types,
                                    ));
                                }
                                Err(es) => self.errors.extend(es),
                            }
                        }
                    }
                    Err(e) => self.errors.push(e),
                }
            }
        }

        let mut effective_bindings = data_bindings.clone();
        effective_bindings.extend(this_spec_bindings.clone());

        // Step 4: Add local data using effective bindings (caller + this spec)
        let mut used_binding_keys: HashSet<Vec<String>> = HashSet::new();
        for data in &spec.data {
            if !data.reference.segments.is_empty() {
                continue; // Skip binding data (processed in step 2)
            }
            if matches!(&data.value, ParsedDataValue::Fill(_)) {
                continue; // Fill rows apply only through data_bindings
            }
            if matches!(&data.value, ParsedDataValue::Import(_)) {
                continue;
            }
            self.add_data(
                data,
                &current_segments,
                &effective_bindings,
                spec_arc,
                &mut used_binding_keys,
                effective,
            );
        }

        self.materialize_local_fill_rows(
            spec,
            &current_segments,
            &effective_bindings,
            spec_arc,
            &mut used_binding_keys,
        );

        for data in &spec.data {
            if !data.reference.segments.is_empty() {
                continue;
            }
            if let ParsedDataValue::Import(spec_ref) = &data.value {
                let nested_effective = spec_ref.at(effective);
                let (nested_repo, nested_arc) =
                    match self.resolve_spec_ref(spec_ref, effective, spec_arc, spec_repository) {
                        Ok(pair) => pair,
                        Err(e) => {
                            self.errors.push(e);
                            continue;
                        }
                    };
                self.add_data(
                    data,
                    &current_segments,
                    &effective_bindings,
                    spec_arc,
                    &mut used_binding_keys,
                    effective,
                );
                let mut nested_segments = current_segments.clone();
                nested_segments.push(PathSegment {
                    data: data.reference.name.clone(),
                    spec: nested_arc.name.clone(),
                });

                let nested_segment_names: Vec<String> =
                    nested_segments.iter().map(|s| s.data.clone()).collect();
                let mut combined_bindings = effective_bindings.clone();
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
                    &nested_repo,
                    nested_segments,
                    combined_bindings,
                    &nested_effective,
                    type_resolver,
                ) {
                    self.errors.extend(errs);
                }
            }
        }

        // Path fills (LHS with segments) must match a declared slot in a nested spec.
        let expected_key_len = current_segments.len() + 1;
        for data in &spec.data {
            if data.reference.segments.is_empty() {
                continue;
            }
            let mut binding_key: Vec<String> = current_segment_names.clone();
            binding_key.extend(data.reference.segments.iter().cloned());
            binding_key.push(data.reference.name.clone());
            if binding_key.len() != expected_key_len {
                continue;
            }
            if used_binding_keys.contains(&binding_key) {
                continue;
            }
            let Some((_, binding_source)) = effective_bindings.get(&binding_key) else {
                continue;
            };
            self.errors.push(self.engine_error(
                format!(
                    "No declared data matches fill or binding for '{}'",
                    binding_key.join(".")
                ),
                binding_source,
            ));
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
            spec_arc: Arc::clone(current_spec_arc),
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
                        Arc::clone(current_spec_arc),
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

                let resolved_spec_types = self
                    .local_types
                    .iter()
                    .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                    .map(|(_, _, t)| t);
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
                let semantic_value = match value {
                    Value::NumberWithUnit(magnitude, unit) => {
                        let Some(lt) = self
                            .local_types
                            .iter()
                            .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                            .map(|(_, _, t)| t)
                            .and_then(|dt| dt.unit_index.get(unit))
                        else {
                            self.errors.push(self.engine_error(
                                format!("Unit '{}' is not in scope for this spec", unit),
                                expr_src,
                            ));
                            return None;
                        };
                        match number_with_unit_to_value_kind(*magnitude, unit, lt) {
                            Ok(v) => v,
                            Err(e) => {
                                self.errors.push(self.engine_error(e, expr_src));
                                return None;
                            }
                        }
                    }
                    _ => match value_to_semantic(value) {
                        Ok(v) => v,
                        Err(e) => {
                            self.errors.push(self.engine_error(e, expr_src));
                            return None;
                        }
                    },
                };
                let lemma_type = match value {
                    Value::Text(_) => primitive_text().clone(),
                    Value::Number(_) => primitive_number().clone(),
                    Value::NumberWithUnit(_, unit) => {
                        match self
                            .local_types
                            .iter()
                            .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                            .map(|(_, _, t)| t)
                            .and_then(|dt| dt.unit_index.get(unit))
                        {
                            Some(lt) => lt.clone(),
                            None => {
                                self.errors.push(self.engine_error(
                                    format!("Unit '{}' is not in scope for this spec", unit),
                                    expr_src,
                                ));
                                return None;
                            }
                        }
                    }
                    Value::Boolean(_) => primitive_boolean().clone(),
                    Value::Date(_) => primitive_date().clone(),
                    Value::Time(_) => primitive_time().clone(),
                    Value::Calendar(_, _) => primitive_calendar().clone(),
                    Value::Range(_, _) => match &semantic_value {
                        ValueKind::Range(left, right) => {
                            LiteralValue::range(left.as_ref().clone(), right.as_ref().clone())
                                .lemma_type
                        }
                        _ => unreachable!(
                            "BUG: semantic range literal conversion returned non-range value kind"
                        ),
                    },
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

            ast::ExpressionKind::ResultIsVeto(operand) => {
                let converted = self.convert_expression_and_extract_dependencies(
                    operand,
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
                )?;
                Some(Expression {
                    kind: ExpressionKind::ResultIsVeto(Arc::new(converted)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::Now => Some(Expression {
                kind: ExpressionKind::Now,
                source_location: expr.source_location.clone(),
            }),

            ast::ExpressionKind::DateRelative(kind, date_expr) => {
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
                    kind: ExpressionKind::DateRelative(*kind, Arc::new(converted_date)),
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

            ast::ExpressionKind::RangeLiteral(left, right) => {
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
                    kind: ExpressionKind::RangeLiteral(Arc::new(l), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::PastFutureRange(kind, offset_expr) => {
                let converted_offset = self.convert_expression_and_extract_dependencies(
                    offset_expr,
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
                )?;
                Some(Expression {
                    kind: ExpressionKind::PastFutureRange(*kind, Arc::new(converted_offset)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::RangeContainment(value, range) => {
                let (converted_value, converted_range) = self.convert_binary_operands(
                    value,
                    range,
                    current_spec_arc,
                    data_map,
                    current_segments,
                    depends_on_rules,
                    rule_names,
                    effective,
                )?;
                Some(Expression {
                    kind: ExpressionKind::RangeContainment(
                        Arc::new(converted_value),
                        Arc::new(converted_range),
                    ),
                    source_location: expr.source_location.clone(),
                })
            }
        }
    }
}

/// Find resolved types for a spec by name. Since per-slice resolution registers
/// at most one version per spec name, this is a simple name match.
fn find_types_by_spec<'b>(
    types: &'b ResolvedTypesMap,
    spec_arc: &Arc<LemmaSpec>,
) -> Option<&'b ResolvedSpecTypes> {
    types
        .iter()
        .find(|(_, s, _)| Arc::ptr_eq(s, spec_arc))
        .map(|(_, _, t)| t)
}

fn find_duration_type_in_spec(
    resolved_types: &ResolvedTypesMap,
    spec_arc: &Arc<LemmaSpec>,
) -> Option<LemmaType> {
    let spec_types = find_types_by_spec(resolved_types, spec_arc)?;
    if let Some(named) = spec_types
        .resolved
        .values()
        .find(|lemma_type| lemma_type.is_duration_like_quantity())
    {
        return Some(named.clone());
    }
    if let Some(from_units) = spec_types
        .unit_index
        .values()
        .find(|lemma_type| lemma_type.is_duration_like_quantity())
    {
        return Some(from_units.clone());
    }
    for data in &spec_arc.data {
        let ParsedDataValue::Import(spec_ref) = &data.value else {
            continue;
        };
        let (_, _, imported_types) = resolved_types
            .iter()
            .find(|(_, s, _)| s.name == spec_ref.name)?;
        if let Some(duration_type) = imported_types
            .resolved
            .get("duration")
            .filter(|t| t.is_duration_like_quantity())
        {
            return Some(duration_type.clone());
        }
    }
    None
}

fn compute_arithmetic_result_type(
    left_type: LemmaType,
    op: &ArithmeticComputation,
    right_type: LemmaType,
) -> LemmaType {
    compute_arithmetic_result_type_recursive(left_type, op, right_type, false)
}

fn compute_arithmetic_result_type_recursive(
    left_type: LemmaType,
    op: &ArithmeticComputation,
    right_type: LemmaType,
    swapped: bool,
) -> LemmaType {
    match (&left_type.specifications, &right_type.specifications) {
        (TypeSpecification::Veto { .. }, _) | (_, TypeSpecification::Veto { .. }) => {
            LemmaType::veto_type()
        }
        (TypeSpecification::Undetermined, _) => LemmaType::undetermined_type(),

        (TypeSpecification::Date { .. }, TypeSpecification::Date { .. }) => {
            LemmaType::undetermined_type()
        }
        (TypeSpecification::Date { .. }, TypeSpecification::Time { .. }) => {
            LemmaType::anonymous_for_decomposition(duration_decomposition())
        }
        (TypeSpecification::Time { .. }, TypeSpecification::Time { .. }) => {
            LemmaType::anonymous_for_decomposition(duration_decomposition())
        }

        // Quantity pairs must fall through to operator-specific arms below.
        // The general equal-type guard must not short-circuit those.
        _ if left_type == right_type
            && !matches!(
                &left_type.specifications,
                TypeSpecification::Quantity { .. }
                    | TypeSpecification::QuantityRange { .. }
                    | TypeSpecification::NumberRange { .. }
                    | TypeSpecification::DateRange { .. }
                    | TypeSpecification::Calendar { .. }
                    | TypeSpecification::RatioRange { .. }
            ) =>
        {
            left_type
        }

        (TypeSpecification::Date { .. }, TypeSpecification::Calendar { .. }) => left_type,
        (TypeSpecification::Date { .. }, TypeSpecification::Quantity { .. })
            if right_type.is_duration_like_quantity() =>
        {
            left_type
        }
        (TypeSpecification::Time { .. }, TypeSpecification::Quantity { .. })
            if right_type.is_duration_like_quantity() =>
        {
            left_type
        }

        (TypeSpecification::Quantity { .. }, TypeSpecification::Ratio { .. }) => left_type,
        (TypeSpecification::Quantity { .. }, TypeSpecification::Number { .. }) => left_type,
        (
            TypeSpecification::Quantity {
                decomposition: l_decomp,
                ..
            },
            TypeSpecification::Calendar { .. },
        ) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                LemmaType::undetermined_type()
            }
            ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {
                let cal_decomp = calendar_decomposition();
                let combined = combine_decompositions(
                    l_decomp,
                    &cal_decomp,
                    matches!(op, ArithmeticComputation::Multiply),
                );
                if combined.is_empty() {
                    primitive_number().clone()
                } else {
                    LemmaType::anonymous_for_decomposition(combined)
                }
            }
            _ => primitive_number().clone(),
        },
        (
            TypeSpecification::Quantity {
                decomposition: l_decomp,
                ..
            },
            TypeSpecification::Quantity {
                decomposition: r_decomp,
                ..
            },
        ) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                if left_type.compatible_with_anonymous_quantity(&right_type)
                    || right_type.compatible_with_anonymous_quantity(&left_type)
                {
                    let left_decomp = left_type.quantity_type_decomposition();
                    let right_decomp = right_type.quantity_type_decomposition();
                    if !left_decomp.is_empty() && left_decomp == right_decomp {
                        if *left_decomp == duration_decomposition() {
                            LemmaType::anonymous_for_decomposition(duration_decomposition())
                        } else {
                            LemmaType::anonymous_for_decomposition(left_decomp.clone())
                        }
                    } else if left_type.is_duration_like_quantity()
                        && right_type.is_duration_like_quantity()
                    {
                        LemmaType::anonymous_for_decomposition(duration_decomposition())
                    } else {
                        left_type
                    }
                } else {
                    left_type
                }
            }
            ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {
                let combined = combine_decompositions(
                    l_decomp,
                    r_decomp,
                    matches!(op, ArithmeticComputation::Multiply),
                );
                if combined.is_empty() {
                    primitive_number().clone()
                } else {
                    LemmaType::anonymous_for_decomposition(combined)
                }
            }
            _ => primitive_number().clone(),
        },

        (
            TypeSpecification::Number { .. },
            TypeSpecification::Quantity {
                decomposition: r_decomp,
                ..
            },
        ) => {
            match op {
                ArithmeticComputation::Multiply => right_type,
                ArithmeticComputation::Divide => {
                    // Number / anonymous_Quantity → negate decomp.
                    // Number / named_Quantity (empty decomp in ValueKind, non-empty in TypeSpec)
                    //   → for anonymous LemmaType (no name), negate; for named type, return Number.
                    if right_type.is_anonymous_quantity() && !r_decomp.is_empty() {
                        let negated: BaseQuantityVector =
                            r_decomp.iter().map(|(k, &e)| (k.clone(), -e)).collect();
                        LemmaType::anonymous_for_decomposition(negated)
                    } else {
                        primitive_number().clone()
                    }
                }
                _ => primitive_number().clone(),
            }
        }

        (
            TypeSpecification::Calendar { .. },
            TypeSpecification::Quantity {
                decomposition: r_decomp,
                ..
            },
        ) => match op {
            ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {
                let cal_decomp = calendar_decomposition();
                let combined = combine_decompositions(
                    &cal_decomp,
                    r_decomp,
                    matches!(op, ArithmeticComputation::Multiply),
                );
                if combined.is_empty() {
                    primitive_number().clone()
                } else {
                    LemmaType::anonymous_for_decomposition(combined)
                }
            }
            _ => primitive_number().clone(),
        },
        (TypeSpecification::Calendar { .. }, TypeSpecification::Number { .. }) => left_type,
        (TypeSpecification::Calendar { .. }, TypeSpecification::Ratio { .. }) => left_type,
        (TypeSpecification::Calendar { .. }, TypeSpecification::Calendar { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                primitive_calendar().clone()
            }
            _ => primitive_number().clone(),
        },

        (TypeSpecification::Number { .. }, TypeSpecification::Calendar { .. }) => match op {
            ArithmeticComputation::Multiply => right_type,
            _ => primitive_number().clone(),
        },

        (TypeSpecification::Number { .. }, TypeSpecification::Ratio { .. }) => {
            primitive_number().clone()
        }
        (TypeSpecification::Number { .. }, TypeSpecification::Number { .. }) => {
            primitive_number().clone()
        }

        (TypeSpecification::Ratio { .. }, TypeSpecification::Ratio { .. }) => left_type,
        (TypeSpecification::DateRange { .. }, TypeSpecification::DateRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_span_type(&left_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::NumberRange { .. }, TypeSpecification::NumberRange { .. }) => {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_span_type(&left_type)
                }
                _ => LemmaType::undetermined_type(),
            }
        }
        (TypeSpecification::QuantityRange { .. }, TypeSpecification::QuantityRange { .. }) => {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
                    if range_matches_range_quantity(&left_type, &right_type) =>
                {
                    range_span_type(&left_type)
                }
                _ => LemmaType::undetermined_type(),
            }
        }
        (TypeSpecification::RatioRange { .. }, TypeSpecification::RatioRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_span_type(&left_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::CalendarRange { .. }, TypeSpecification::CalendarRange { .. }) => {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_span_type(&left_type)
                }
                _ => LemmaType::undetermined_type(),
            }
        }
        (TypeSpecification::DateRange { .. }, TypeSpecification::CalendarRange { .. })
        | (TypeSpecification::CalendarRange { .. }, TypeSpecification::DateRange { .. })
        | (TypeSpecification::Date { .. }, TypeSpecification::CalendarRange { .. })
        | (TypeSpecification::CalendarRange { .. }, TypeSpecification::Date { .. }) => {
            LemmaType::undetermined_type()
        }
        (TypeSpecification::DateRange { .. }, TypeSpecification::Calendar { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => left_type,
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::Calendar { .. }, TypeSpecification::DateRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => right_type,
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::CalendarRange { .. }, TypeSpecification::Calendar { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_quantity_type_for_operand(&left_type, &right_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::Calendar { .. }, TypeSpecification::CalendarRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_quantity_type_for_operand(&right_type, &left_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::NumberRange { .. }, TypeSpecification::Number { .. })
        | (TypeSpecification::RatioRange { .. }, TypeSpecification::Ratio { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_quantity_type_for_operand(&left_type, &right_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::QuantityRange { .. }, TypeSpecification::Quantity { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract
                if range_matches_quantity_type(&left_type, &right_type) =>
            {
                range_quantity_type_for_operand(&left_type, &right_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::Number { .. }, TypeSpecification::NumberRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_quantity_type_for_operand(&right_type, &left_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::Quantity { .. }, TypeSpecification::QuantityRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract
                if range_matches_quantity_type(&right_type, &left_type) =>
            {
                range_quantity_type_for_operand(&right_type, &left_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::Ratio { .. }, TypeSpecification::RatioRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_quantity_type_for_operand(&right_type, &left_type)
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::DateRange { .. }, TypeSpecification::Quantity { .. })
            if right_type.is_duration_like_quantity() =>
        {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_quantity_type_for_operand(&left_type, &right_type)
                }
                _ => LemmaType::undetermined_type(),
            }
        }
        (TypeSpecification::Quantity { .. }, TypeSpecification::DateRange { .. })
            if left_type.is_duration_like_quantity() =>
        {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_quantity_type_for_operand(&right_type, &left_type)
                }
                _ => LemmaType::undetermined_type(),
            }
        }
        (TypeSpecification::DateRange { .. }, TypeSpecification::Number { .. }) => match op {
            ArithmeticComputation::Multiply => range_span_type(&left_type),
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::Number { .. }, TypeSpecification::DateRange { .. }) => match op {
            ArithmeticComputation::Multiply => range_span_type(&right_type),
            _ => LemmaType::undetermined_type(),
        },
        (
            TypeSpecification::Quantity { decomposition, .. },
            TypeSpecification::DateRange { .. },
        ) => match (op, date_range_projection_axis(&left_type)) {
            (ArithmeticComputation::Multiply, Ok(DateRangeProjectionAxis::Duration)) => {
                let combined =
                    combine_decompositions(decomposition, &duration_decomposition(), true);
                if combined.is_empty() {
                    primitive_number().clone()
                } else {
                    LemmaType::anonymous_for_decomposition(combined)
                }
            }
            (ArithmeticComputation::Multiply, Ok(DateRangeProjectionAxis::Calendar)) => {
                let combined =
                    combine_decompositions(decomposition, &calendar_decomposition(), true);
                if combined.is_empty() {
                    primitive_number().clone()
                } else {
                    LemmaType::anonymous_for_decomposition(combined)
                }
            }
            _ => LemmaType::undetermined_type(),
        },
        (TypeSpecification::DateRange { .. }, TypeSpecification::Quantity { .. }) => {
            compute_arithmetic_result_type_recursive(right_type, op, left_type, true)
        }

        _ => {
            if swapped {
                LemmaType::undetermined_type()
            } else {
                compute_arithmetic_result_type_recursive(right_type, op, left_type, true)
            }
        }
    }
}

fn infer_range_type_from_endpoint_types(
    left_type: &LemmaType,
    right_type: &LemmaType,
) -> LemmaType {
    match (&left_type.specifications, &right_type.specifications) {
        (TypeSpecification::Date { .. }, TypeSpecification::Date { .. }) => {
            primitive_date_range().clone()
        }
        (TypeSpecification::Number { .. }, TypeSpecification::Number { .. }) => {
            primitive_number_range().clone()
        }
        (
            TypeSpecification::Quantity {
                units,
                decomposition,
                canonical_unit,
                ..
            },
            TypeSpecification::Quantity { .. },
        ) if left_type.same_quantity_family(right_type) => {
            let mut spec = TypeSpecification::quantity_range();
            if let TypeSpecification::QuantityRange {
                units: range_units,
                decomposition: range_decomposition,
                canonical_unit: range_canonical_unit,
                ..
            } = &mut spec
            {
                *range_units = units.clone();
                *range_decomposition = decomposition.clone();
                *range_canonical_unit = canonical_unit.clone();
            }
            LemmaType::primitive(spec)
        }
        (TypeSpecification::Ratio { units, .. }, TypeSpecification::Ratio { .. }) => {
            let mut spec = TypeSpecification::ratio_range();
            if let TypeSpecification::RatioRange {
                units: range_units, ..
            } = &mut spec
            {
                *range_units = units.clone();
            }
            LemmaType::primitive(spec)
        }
        (TypeSpecification::Calendar { .. }, TypeSpecification::Calendar { .. }) => {
            primitive_calendar_range().clone()
        }
        _ => LemmaType::undetermined_type(),
    }
}

fn range_span_type(range_type: &LemmaType) -> LemmaType {
    match &range_type.specifications {
        TypeSpecification::DateRange { .. } => {
            LemmaType::anonymous_for_decomposition(duration_decomposition())
        }
        TypeSpecification::NumberRange { .. } => primitive_number().clone(),
        TypeSpecification::QuantityRange {
            units,
            canonical_unit,
            ..
        } => LemmaType::primitive(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units: units.clone(),
            traits: Vec::new(),
            // Span magnitude uses the unit table; empty decomposition avoids
            // anonymous-intermediate rejection at rule boundaries.
            decomposition: BaseQuantityVector::new(),
            canonical_unit: canonical_unit.clone(),
            help: String::new(),
        }),
        TypeSpecification::RatioRange { units, .. } => {
            LemmaType::primitive(TypeSpecification::Ratio {
                minimum: None,
                maximum: None,
                decimals: None,
                units: units.clone(),
                help: String::new(),
            })
        }
        TypeSpecification::CalendarRange { .. } => primitive_calendar().clone(),
        _ => LemmaType::undetermined_type(),
    }
}

fn range_quantity_type_for_operand(range_type: &LemmaType, other_type: &LemmaType) -> LemmaType {
    let _ = other_type;
    if range_type.is_range() {
        range_type.clone()
    } else {
        range_span_type(range_type)
    }
}

fn range_matches_quantity_type(range_type: &LemmaType, measure_type: &LemmaType) -> bool {
    match &range_type.specifications {
        TypeSpecification::DateRange { .. } => {
            measure_type.is_duration_like() || measure_type.is_calendar()
        }
        TypeSpecification::NumberRange { .. } => measure_type.is_number(),
        TypeSpecification::QuantityRange { .. } => {
            measure_type.is_quantity() && quantity_range_matches_quantity(range_type, measure_type)
        }
        TypeSpecification::RatioRange { .. } => measure_type.is_ratio(),
        TypeSpecification::CalendarRange { .. } => measure_type.is_calendar(),
        _ => false,
    }
}

fn range_matches_range_quantity(left_range: &LemmaType, right_range: &LemmaType) -> bool {
    let right_measure_type = range_span_type(right_range);
    !right_measure_type.is_undetermined()
        && range_matches_quantity_type(left_range, &right_measure_type)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateRangeProjectionAxis {
    Duration,
    Calendar,
}

fn date_range_projection_axis(
    quantity_type: &LemmaType,
) -> Result<DateRangeProjectionAxis, String> {
    let quantity_decomposition = match &quantity_type.specifications {
        TypeSpecification::Quantity { decomposition, .. } => decomposition,
        _ => {
            return Err(format!(
                "Cannot project date range through non-quantity type {}.",
                quantity_type.name()
            ));
        }
    };

    let has_duration_axis = quantity_decomposition
        .get(semantics::DURATION_DIMENSION)
        .is_some_and(|exponent| *exponent != 0);
    let has_calendar_axis = quantity_decomposition
        .get(semantics::CALENDAR_DIMENSION)
        .is_some_and(|exponent| *exponent != 0);

    match (has_duration_axis, has_calendar_axis) {
        (true, false) => Ok(DateRangeProjectionAxis::Duration),
        (false, true) => Ok(DateRangeProjectionAxis::Calendar),
        (false, false) => Err(format!(
            "Cannot multiply {} by a date range because {} has no duration or calendar dimension.",
            quantity_type.name(),
            quantity_type.name()
        )),
        (true, true) => Err(format!(
            "Cannot multiply {} by a date range because {} has both duration and calendar dimensions.",
            quantity_type.name(),
            quantity_type.name()
        )),
    }
}

fn quantity_range_matches_quantity(range_type: &LemmaType, quantity_type: &LemmaType) -> bool {
    match (&range_type.specifications, &quantity_type.specifications) {
        (
            TypeSpecification::QuantityRange {
                units: range_units,
                decomposition: range_decomposition,
                canonical_unit: range_canonical_unit,
                ..
            },
            TypeSpecification::Quantity {
                units: quantity_units,
                decomposition: quantity_decomposition,
                canonical_unit: quantity_canonical_unit,
                ..
            },
        ) => {
            if range_units.0.is_empty()
                && range_decomposition.is_empty()
                && range_canonical_unit.is_empty()
            {
                true
            } else if quantity_decomposition.is_empty() {
                range_units == quantity_units
                    && range_canonical_unit.eq_ignore_ascii_case(quantity_canonical_unit)
            } else {
                range_units == quantity_units
                    && range_decomposition == quantity_decomposition
                    && range_canonical_unit.eq_ignore_ascii_case(quantity_canonical_unit)
            }
        }
        _ => false,
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
    spec_arc: &Arc<LemmaSpec>,
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
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            if left_type.vetoed() || right_type.vetoed() {
                return LemmaType::veto_type();
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            if !left_type.is_boolean() {
                return LemmaType::undetermined_type();
            }
            if right_type.is_boolean() {
                primitive_boolean().clone()
            } else {
                right_type
            }
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            if left_type.vetoed() || right_type.vetoed() {
                return LemmaType::veto_type();
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            if left_type.is_boolean() && right_type.is_boolean() {
                return primitive_boolean().clone();
            }
            if left_type == right_type {
                return left_type;
            }
            LemmaType::undetermined_type()
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            let operand_type = infer_expression_type(
                operand,
                graph,
                computed_rule_types,
                resolved_types,
                spec_arc,
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
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            if left_type.vetoed() || right_type.vetoed() {
                return LemmaType::veto_type();
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            primitive_boolean().clone()
        }

        ExpressionKind::Arithmetic(left, operator, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            let mut result =
                compute_arithmetic_result_type(left_type.clone(), operator, right_type.clone());
            if *operator == ArithmeticComputation::Subtract
                && left_type.is_time()
                && right_type.is_time()
                && result.is_anonymous_quantity()
                && *result.quantity_type_decomposition() == duration_decomposition()
            {
                if let Some(duration_type) = find_duration_type_in_spec(resolved_types, spec_arc) {
                    result = duration_type;
                }
            }
            result
        }

        ExpressionKind::UnitConversion(source_expression, target) => {
            let source_type = infer_expression_type(
                source_expression,
                graph,
                computed_rule_types,
                resolved_types,
                spec_arc,
            );
            if source_type.vetoed() {
                return LemmaType::veto_type();
            }
            if source_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            if source_type.is_range() {
                let span_type = range_span_type(&source_type);
                return match target {
                    SemanticConversionTarget::Number => primitive_number().clone(),
                    SemanticConversionTarget::Calendar(_) => primitive_calendar().clone(),
                    SemanticConversionTarget::QuantityUnit(unit_name) => {
                        find_types_by_spec(resolved_types, spec_arc)
                            .and_then(|dt| dt.unit_index.get(unit_name))
                            .cloned()
                            .unwrap_or(span_type)
                    }
                    SemanticConversionTarget::RatioUnit(unit_name) => {
                        find_types_by_spec(resolved_types, spec_arc)
                            .and_then(|dt| dt.unit_index.get(unit_name))
                            .cloned()
                            .unwrap_or(span_type)
                    }
                };
            }
            match target {
                SemanticConversionTarget::Number => primitive_number().clone(),
                SemanticConversionTarget::Calendar(_) => primitive_calendar().clone(),
                SemanticConversionTarget::QuantityUnit(unit_name) => {
                    if source_type.is_number()
                        || source_type.is_duration_like()
                        || source_type.is_date_range()
                        || source_type.is_anonymous_quantity()
                    {
                        find_types_by_spec(resolved_types, spec_arc)
                            .and_then(|dt| dt.unit_index.get(unit_name))
                            .cloned()
                            .unwrap_or_else(LemmaType::undetermined_type)
                    } else {
                        source_type
                    }
                }
                SemanticConversionTarget::RatioUnit(unit_name) => {
                    if source_type.is_number() {
                        find_types_by_spec(resolved_types, spec_arc)
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
                spec_arc,
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

        ExpressionKind::ResultIsVeto(operand) => {
            let _ = infer_expression_type(
                operand,
                graph,
                computed_rule_types,
                resolved_types,
                spec_arc,
            );
            primitive_boolean().clone()
        }

        ExpressionKind::Now => primitive_date().clone(),

        ExpressionKind::DateRelative(..)
        | ExpressionKind::DateCalendar(..)
        | ExpressionKind::RangeContainment(..) => primitive_boolean().clone(),

        ExpressionKind::RangeLiteral(left, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            if left_type.vetoed() || right_type.vetoed() {
                return LemmaType::veto_type();
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return LemmaType::undetermined_type();
            }
            infer_range_type_from_endpoint_types(&left_type, &right_type)
        }

        ExpressionKind::PastFutureRange(..) => primitive_date_range().clone(),
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
        DataDefinition::Import { .. } => LemmaType::undetermined_type(),
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

fn check_logical_or_operands(
    graph: &Graph,
    left_type: &LemmaType,
    right_type: &LemmaType,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if left_type.vetoed() || right_type.vetoed() {
        return Ok(());
    }
    if left_type.is_undetermined() || right_type.is_undetermined() {
        return Ok(());
    }
    if left_type.is_boolean() && right_type.is_boolean() {
        return check_logical_operands(graph, left_type, right_type, source);
    }
    if left_type == right_type {
        return Ok(());
    }
    Err(vec![engine_error_at_graph(
        graph,
        source,
        format!(
            "Logical OR requires matching types (unless-chain / De Morgan), got {:?} and {:?}",
            left_type, right_type
        ),
    )])
}

fn check_logical_and_operands(
    graph: &Graph,
    left_type: &LemmaType,
    right_type: &LemmaType,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if left_type.vetoed() || right_type.vetoed() {
        return Ok(());
    }
    if !left_type.is_boolean() {
        return Err(vec![engine_error_at_graph(
            graph,
            source,
            format!(
                "Logical AND requires boolean left operand, got {:?}",
                left_type
            ),
        )]);
    }
    if right_type.is_boolean() {
        return Ok(());
    }
    Ok(())
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

    if left_type.is_range() {
        if range_matches_quantity_type(left_type, right_type) {
            return Ok(());
        }
        return Err(vec![engine_error_at_graph(
            graph,
            source,
            format!("Cannot compare {:?} with {:?}", left_type, right_type),
        )]);
    }

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

    if left_type.is_quantity() && right_type.is_quantity() {
        if !left_type.same_quantity_family(right_type)
            && !left_type.compatible_with_anonymous_quantity(right_type)
        {
            return Err(vec![engine_error_at_graph(
                graph,
                source,
                format!(
                    "Cannot compare unrelated quantity types: {} and {}",
                    left_type.name(),
                    right_type.name()
                ),
            )]);
        }
        return Ok(());
    }

    if left_type.is_duration_like() && right_type.is_duration_like() {
        return Ok(());
    }
    if left_type.is_calendar() && right_type.is_calendar() {
        return Ok(());
    }
    if left_type.is_calendar() && right_type.is_number() {
        return Ok(());
    }
    if left_type.is_number() && right_type.is_calendar() {
        return Ok(());
    }

    Err(vec![engine_error_at_graph(
        graph,
        source,
        format!("Cannot compare {:?} with {:?}", left_type, right_type),
    )])
}

/// Literal zero on the right of `/` or `%` is rejected at planning time (runtime data divisors may Veto).
fn arithmetic_literal_zero_divisor_planning_errors(
    graph: &Graph,
    right: &Expression,
    operator: &ArithmeticComputation,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if !matches!(
        operator,
        ArithmeticComputation::Divide | ArithmeticComputation::Modulo
    ) {
        return Ok(());
    }

    if let ExpressionKind::Literal(literal) = &right.kind {
        if let ValueKind::Number(number) = &literal.value {
            if crate::computation::rational::rational_is_zero(number) {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!("Cannot apply '{}' with a zero divisor literal.", operator),
                )]);
            }
        }
    }

    Ok(())
}

fn arithmetic_power_exponent_planning_errors(
    graph: &Graph,
    _left: &Expression,
    right: &Expression,
    left_type: &LemmaType,
    _right_type: &LemmaType,
    operator: &ArithmeticComputation,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if *operator != ArithmeticComputation::Power {
        return Ok(());
    }
    // Quantity ^ non-integer-literal is rejected: fractional dimensions are undefined,
    // and variable exponents cannot be statically verified to be integers at plan time.
    if left_type.is_quantity() || left_type.is_duration_like() {
        let is_integer_literal = if let ExpressionKind::Literal(lit) = &right.kind {
            if let crate::planning::semantics::ValueKind::Number(n) = &lit.value {
                *n.denom() == 1
            } else {
                false
            }
        } else {
            false
        };
        if !is_integer_literal {
            return Err(vec![engine_error_at_graph(
                graph,
                source,
                "Cannot raise a quantity value to a fractional or variable exponent. Use a positive integer literal.".to_string(),
            )]);
        }
    }
    Ok(())
}

/// Discharges planning obligations for numeric arithmetic beyond type compatibility:
/// literal zero divisors and integer power exponents where required.
fn arithmetic_plan_time_exactness_planning_errors(
    graph: &Graph,
    left: &Expression,
    right: &Expression,
    left_type: &LemmaType,
    right_type: &LemmaType,
    operator: &ArithmeticComputation,
    source: &Source,
) -> Result<(), Vec<Error>> {
    if left_type.vetoed() || right_type.vetoed() {
        return Ok(());
    }
    if left_type.is_undetermined() || right_type.is_undetermined() {
        return Ok(());
    }

    let mut errors = Vec::new();
    let collect = |result: Result<(), Vec<Error>>, errors: &mut Vec<Error>| {
        if let Err(mut errs) = result {
            errors.append(&mut errs);
        }
    };

    collect(
        arithmetic_literal_zero_divisor_planning_errors(graph, right, operator, source),
        &mut errors,
    );
    collect(
        arithmetic_power_exponent_planning_errors(
            graph, left, right, left_type, right_type, operator, source,
        ),
        &mut errors,
    );

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
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

    if left_type.is_date() && right_type.is_date() && *operator == ArithmeticComputation::Subtract {
        return Err(vec![engine_error_at_graph(
            graph,
            source,
            "Cannot subtract dates. Use dateA...dateB to create a date range.".to_string(),
        )]);
    }

    if left_type.is_range() || right_type.is_range() {
        let range_measure_allowed = matches!(
            operator,
            ArithmeticComputation::Add | ArithmeticComputation::Subtract
        ) && ((left_type.is_range()
            && right_type.is_range()
            && range_matches_range_quantity(left_type, right_type))
            || (left_type.is_range()
                && !right_type.is_range()
                && range_matches_quantity_type(left_type, right_type))
            || (right_type.is_range()
                && !left_type.is_range()
                && range_matches_quantity_type(right_type, left_type)));
        let range_scalar_multiply_allowed = *operator == ArithmeticComputation::Multiply
            && ((left_type.is_range() && right_type.is_number())
                || (right_type.is_range() && left_type.is_number()));

        let quantity_date_range_allowed = if *operator == ArithmeticComputation::Multiply {
            if left_type.is_quantity() && right_type.is_date_range() {
                match date_range_projection_axis(left_type) {
                    Ok(_) => true,
                    Err(message) => {
                        return Err(vec![engine_error_at_graph(graph, source, message)]);
                    }
                }
            } else if left_type.is_date_range() && right_type.is_quantity() {
                match date_range_projection_axis(right_type) {
                    Ok(_) => true,
                    Err(message) => {
                        return Err(vec![engine_error_at_graph(graph, source, message)]);
                    }
                }
            } else {
                false
            }
        } else {
            false
        };

        if range_measure_allowed || range_scalar_multiply_allowed || quantity_date_range_allowed {
            return Ok(());
        }

        if (left_type.is_date_range()
            && right_type.is_quantity()
            && !right_type.is_duration_like_quantity())
            || (right_type.is_date_range()
                && left_type.is_quantity()
                && !left_type.is_duration_like_quantity())
        {
            return Err(vec![engine_error_at_graph(
                graph,
                source,
                format!(
                    "Cannot apply '{}' to a date range and an unrelated quantity.",
                    operator
                ),
            )]);
        }

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

    // Date/Time arithmetic is limited to supported temporal combinations.
    if left_type.is_date() || left_type.is_time() || right_type.is_date() || right_type.is_time() {
        let left_is_duration_like = left_type.is_duration_like();
        let right_is_duration_like = right_type.is_duration_like();
        let valid = matches!(
            (
                left_type.is_date(),
                left_type.is_time(),
                right_type.is_date(),
                right_type.is_time(),
                left_is_duration_like,
                right_is_duration_like,
                left_type.is_calendar(),
                right_type.is_calendar(),
                operator
            ),
            (
                true,
                _,
                _,
                true,
                _,
                _,
                _,
                _,
                ArithmeticComputation::Subtract
            ) | (
                _,
                true,
                _,
                true,
                _,
                _,
                _,
                _,
                ArithmeticComputation::Subtract
            ) | (
                true,
                _,
                _,
                _,
                _,
                true,
                _,
                _,
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
            ) | (
                true,
                _,
                _,
                _,
                _,
                _,
                _,
                true,
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
            ) | (_, _, true, _, true, _, _, _, ArithmeticComputation::Add)
                | (_, _, true, _, _, _, true, _, ArithmeticComputation::Add)
                | (
                    _,
                    true,
                    _,
                    _,
                    _,
                    true,
                    _,
                    _,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                )
                | (_, _, _, true, true, _, _, _, ArithmeticComputation::Add)
        );
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

    // Quantity/Quantity rules:
    //   +/- requires same quantity family (dimensionless addition is not meaningful otherwise).
    //   *   requires different quantity families (same-family quantity*quantity is rejected; use `as number`).
    //   /   is allowed for all families (same-family → scalar Number; cross-family → anonymous Quantity).
    //   %   and ^ on two Quantities are always rejected.
    if left_type.is_quantity() && right_type.is_quantity() {
        return match operator {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                if left_type.same_quantity_family(right_type)
                    || left_type.compatible_with_anonymous_quantity(right_type)
                {
                    Ok(())
                } else {
                    Err(vec![engine_error_at_graph(
                        graph,
                        source,
                        format!(
                            "Cannot {} unrelated quantity types: {} and {}.",
                            if matches!(operator, ArithmeticComputation::Add) {
                                "add"
                            } else {
                                "subtract"
                            },
                            left_type.name(),
                            right_type.name()
                        ),
                    )])
                }
            }
            ArithmeticComputation::Multiply => {
                if left_type.same_quantity_family(right_type) {
                    Err(vec![engine_error_at_graph(
                        graph,
                        source,
                        format!(
                            "Cannot multiply two '{}' quantity values of the same type. \
                             Convert operands first: 'value as number'.",
                            left_type.name()
                        ),
                    )])
                } else {
                    // Cross-family Quantity * Quantity → anonymous intermediate. Allowed.
                    Ok(())
                }
            }
            ArithmeticComputation::Divide => {
                // Quantity / Quantity (any family) → scalar Number or anonymous intermediate. Allowed.
                Ok(())
            }
            ArithmeticComputation::Modulo | ArithmeticComputation::Power => {
                Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!(
                        "Cannot apply '{}' to two quantity values ({} and {}).",
                        operator,
                        left_type.name(),
                        right_type.name()
                    ),
                )])
            }
        };
    }

    // Duration * Duration (and power/modulo) rejected for same reason.
    if left_type.is_duration_like() && right_type.is_duration_like() {
        return match operator {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => Ok(()),
            ArithmeticComputation::Divide => Ok(()),
            _ => Err(vec![engine_error_at_graph(
                graph,
                source,
                "Cannot multiply two duration values. Convert operands first: 'value as number'."
                    .to_string(),
            )]),
        };
    }

    if left_type.is_calendar() && right_type.is_calendar() {
        return match operator {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => Ok(()),
            ArithmeticComputation::Divide => Ok(()),
            _ => Err(vec![engine_error_at_graph(
                graph,
                source,
                "Cannot multiply two calendar values. Convert operands first: 'value as number'."
                    .to_string(),
            )]),
        };
    }

    if (left_type.is_duration_like() && right_type.is_calendar())
        || (left_type.is_calendar() && right_type.is_duration_like())
    {
        return Err(vec![engine_error_at_graph(
            graph,
            source,
            format!(
                "Cannot apply '{}' to {} and {}. Duration and calendar are unrelated types.",
                operator,
                left_type.name(),
                right_type.name()
            ),
        )]);
    }

    // Only Quantity, Number, Ratio, Duration, and Calendar can participate in arithmetic
    let left_valid = left_type.is_quantity()
        || left_type.is_number()
        || left_type.is_duration_like()
        || left_type.is_calendar()
        || left_type.is_ratio();
    let right_valid = right_type.is_quantity()
        || right_type.is_number()
        || right_type.is_duration_like()
        || right_type.is_calendar()
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
            pair(LemmaType::is_quantity, LemmaType::is_number)
                || pair(LemmaType::is_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_quantity, LemmaType::is_duration_like_quantity)
                || pair(LemmaType::is_quantity, LemmaType::is_calendar)
                || pair(LemmaType::is_duration_like_quantity, LemmaType::is_number)
                || pair(LemmaType::is_duration_like_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_calendar, LemmaType::is_number)
                || pair(LemmaType::is_calendar, LemmaType::is_ratio)
                || pair(LemmaType::is_number, LemmaType::is_ratio)
        }
        ArithmeticComputation::Divide => {
            pair(LemmaType::is_quantity, LemmaType::is_number)
                || pair(LemmaType::is_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_quantity, LemmaType::is_duration_like_quantity)
                || pair(LemmaType::is_quantity, LemmaType::is_calendar)
                || (left_type.is_duration_like() && right_type.is_number())
                || (left_type.is_duration_like() && right_type.is_ratio())
                || (left_type.is_calendar() && right_type.is_number())
                || (left_type.is_calendar() && right_type.is_ratio())
                || (left_type.is_number() && right_type.is_duration_like())
                || (left_type.is_number() && right_type.is_calendar())
                || pair(LemmaType::is_number, LemmaType::is_ratio)
        }
        ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
            pair(LemmaType::is_quantity, LemmaType::is_number)
                || pair(LemmaType::is_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_duration_like_quantity, LemmaType::is_number)
                || pair(LemmaType::is_duration_like_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_calendar, LemmaType::is_number)
                || pair(LemmaType::is_calendar, LemmaType::is_ratio)
                || pair(LemmaType::is_number, LemmaType::is_ratio)
        }
        ArithmeticComputation::Power => {
            // Exponent must be a dimensionless integer for quantity left (enforced separately
            // in arithmetic_power_exponent_planning_errors). Quantity ^ Ratio is rejected here.
            // number ^ (number | ratio) is allowed; exact rational result or runtime Veto.
            let left_ok = left_type.is_number()
                || left_type.is_quantity()
                || left_type.is_ratio()
                || left_type.is_duration_like();
            let right_ok = if left_type.is_quantity() || left_type.is_duration_like() {
                right_type.is_number()
            } else {
                right_type.is_number() || right_type.is_ratio()
            };
            left_ok && right_ok
        }
        ArithmeticComputation::Modulo => {
            // Quantity % Ratio is rejected: ratio is dimensionless-fractional, not a meaningful
            // modulus for a dimensioned value.
            if left_type.is_quantity() && right_type.is_ratio() {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!(
                        "Cannot apply modulo to {} with a ratio. Use a number divisor.",
                        left_type.name()
                    ),
                )]);
            }
            right_type.is_number() || right_type.is_ratio()
        }
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

fn check_range_span_unit_conversion(
    graph: &Graph,
    source_type: &LemmaType,
    target: &SemanticConversionTarget,
    resolved_types: &ResolvedTypesMap,
    source: &Source,
    spec_arc: &Arc<LemmaSpec>,
) -> Result<(), Vec<Error>> {
    match target {
        SemanticConversionTarget::Number => {
            if source_type.is_number_range() {
                Ok(())
            } else {
                Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!("Cannot convert {} to number.", source_type.name()),
                )])
            }
        }
        SemanticConversionTarget::QuantityUnit(unit_name) => {
            if source_type.is_calendar_range() {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!(
                        "Cannot convert {} to quantity unit '{}'.",
                        source_type.name(),
                        unit_name
                    ),
                )]);
            }
            let target_type = find_types_by_spec(resolved_types, spec_arc)
                .and_then(|dt| dt.unit_index.get(unit_name))
                .cloned();
            let target_type = match target_type {
                Some(lemma_type) => lemma_type,
                None => {
                    return Err(vec![engine_error_at_graph(
                        graph,
                        source,
                        format!(
                            "Unknown unit '{}': no quantity type in spec '{}' owns this unit.",
                            unit_name, spec_arc.name
                        ),
                    )]);
                }
            };
            if !target_type.is_duration_like_quantity() {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!(
                        "Cannot convert {} to quantity unit '{}'.",
                        source_type.name(),
                        unit_name
                    ),
                )]);
            }
            if source_type.is_number_range() {
                return Ok(());
            }
            if source_type.is_quantity_range() {
                if let TypeSpecification::QuantityRange {
                    decomposition,
                    units,
                    ..
                } = &source_type.specifications
                {
                    if *decomposition == duration_decomposition() {
                        return Ok(());
                    }
                    let all_duration_endpoints = !units.0.is_empty()
                        && units.0.iter().all(|unit| {
                            find_types_by_spec(resolved_types, spec_arc)
                                .and_then(|dt| dt.unit_index.get(&unit.name))
                                .is_some_and(|owner| owner.is_duration_like_quantity())
                        });
                    if all_duration_endpoints {
                        return Ok(());
                    }
                }
                return Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!(
                        "Cannot convert {} to quantity unit '{}'.",
                        source_type.name(),
                        unit_name
                    ),
                )]);
            }
            Err(vec![engine_error_at_graph(
                graph,
                source,
                format!(
                    "Cannot convert {} to quantity unit '{}'.",
                    source_type.name(),
                    unit_name
                ),
            )])
        }
        SemanticConversionTarget::RatioUnit(unit_name) => {
            if !source_type.is_ratio_range() {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!(
                        "Cannot convert {} to ratio unit '{}'.",
                        source_type.name(),
                        unit_name
                    ),
                )]);
            }
            let valid: Vec<&str> = match &source_type.specifications {
                TypeSpecification::RatioRange { units, .. } => {
                    units.iter().map(|u| u.name.as_str()).collect()
                }
                _ => unreachable!("BUG: is_ratio_range without RatioRange spec"),
            };
            if valid
                .iter()
                .any(|name| name.eq_ignore_ascii_case(unit_name))
            {
                Ok(())
            } else {
                Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!(
                        "Unknown unit '{}' for type {}. Valid units: {}",
                        unit_name,
                        source_type.name(),
                        valid.join(", ")
                    ),
                )])
            }
        }
        SemanticConversionTarget::Calendar(_) => Err(vec![engine_error_at_graph(
            graph,
            source,
            format!("Cannot convert {} to calendar.", source_type.name()),
        )]),
    }
}

fn check_unit_conversion_types(
    graph: &Graph,
    source_type: &LemmaType,
    target: &SemanticConversionTarget,
    resolved_types: &ResolvedTypesMap,
    source: &Source,
    spec_arc: &Arc<LemmaSpec>,
) -> Result<(), Vec<Error>> {
    if source_type.vetoed() {
        return Ok(());
    }
    match target {
        SemanticConversionTarget::Number => {
            if source_type.is_date_range() || source_type.is_number_range() {
                return Ok(());
            }
            if source_type.is_quantity_range()
                || source_type.is_ratio_range()
                || source_type.is_calendar_range()
            {
                return check_range_span_unit_conversion(
                    graph,
                    source_type,
                    target,
                    resolved_types,
                    source,
                    spec_arc,
                );
            }
            // Prohibit stripping anonymous compound intermediates to number directly.
            // Anonymous intermediates have unresolved dimensions that cannot be silently dropped;
            // the user must cast to a named typedef first (e.g., `as mps`) or use `as number`
            // only after all dimensions have cancelled.
            if source_type.is_anonymous_quantity() {
                let decomp = source_type.quantity_type_decomposition();
                if !decomp.is_empty() {
                    return Err(vec![engine_error_at_graph(
                        graph,
                        source,
                        format!(
                            "Cannot use 'as number' to strip an anonymous intermediate with unresolved \
                             dimensions {:?}. Cast to a named quantity typedef first (e.g., 'as <unit>'), \
                             or ensure all dimensions cancel before converting to number.",
                            decomp
                        ),
                    )]);
                }
            }
            if source_type.is_quantity()
                || source_type.is_number()
                || source_type.is_duration_like()
                || source_type.is_calendar()
                || source_type.is_ratio()
            {
                Ok(())
            } else {
                Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!("Cannot convert {} to number.", source_type.name()),
                )])
            }
        }
        SemanticConversionTarget::QuantityUnit(unit_name) => {
            if source_type.is_date_range() {
                let target_type = find_types_by_spec(resolved_types, spec_arc)
                    .and_then(|dt| dt.unit_index.get(unit_name))
                    .cloned();

                let target_type = match target_type {
                    Some(lemma_type) => lemma_type,
                    None => {
                        return Err(vec![engine_error_at_graph(
                            graph,
                            source,
                            format!(
                                "Unknown unit '{}': no quantity type in spec '{}' owns this unit.",
                                unit_name, spec_arc.name
                            ),
                        )]);
                    }
                };

                if !target_type.is_duration_like_quantity() {
                    return Err(vec![engine_error_at_graph(
                        graph,
                        source,
                        format!(
                            "Cannot convert date range to quantity unit '{}'.",
                            unit_name
                        ),
                    )]);
                }

                return Ok(());
            }

            if source_type.is_number_range()
                || source_type.is_quantity_range()
                || source_type.is_calendar_range()
            {
                return check_range_span_unit_conversion(
                    graph,
                    source_type,
                    target,
                    resolved_types,
                    source,
                    spec_arc,
                );
            }
            // Number can be cast to any quantity unit (interpreted as a value in that unit).
            if source_type.is_number() {
                return if find_types_by_spec(resolved_types, spec_arc)
                    .and_then(|dt| dt.unit_index.get(unit_name))
                    .is_some()
                {
                    Ok(())
                } else {
                    Err(vec![engine_error_at_graph(
                        graph,
                        source,
                        format!("Unknown unit '{}' in spec '{}'.", unit_name, spec_arc.name),
                    )])
                };
            }

            // Anonymous intermediate: verify decomposition compatibility with the target quantity,
            // then confirm the requested unit exists in that target quantity.
            if source_type.is_anonymous_quantity() {
                let target_type = find_types_by_spec(resolved_types, spec_arc)
                    .and_then(|dt| dt.unit_index.get(unit_name))
                    .cloned();

                let target_type = match target_type {
                    Some(lemma_type) => lemma_type,
                    None => {
                        return Err(vec![engine_error_at_graph(
                            graph,
                            source,
                            format!(
                                "Unknown unit '{}': no quantity type in spec '{}' owns this unit.",
                                unit_name, spec_arc.name
                            ),
                        )]);
                    }
                };

                let source_decomp = source_type.quantity_type_decomposition();
                let target_quantity_family = target_type
                    .quantity_family_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| target_type.name().to_string());

                let target_decomp = match &target_type.specifications {
                    TypeSpecification::Quantity { decomposition, .. } => decomposition.clone(),
                    _ => {
                        return Err(vec![engine_error_at_graph(
                            graph,
                            source,
                            format!("Unit '{}' does not belong to a quantity type.", unit_name),
                        )]);
                    }
                };

                if *source_decomp != target_decomp {
                    return Err(vec![engine_error_at_graph(
                        graph,
                        source,
                        format!(
                            "Cannot cast to '{}' (quantity '{}'): source dimensions {:?} do not \
                             match target dimensions {:?}. The intermediate result has a different \
                             physical quantity than the target type.",
                            unit_name, target_quantity_family, source_decomp, target_decomp
                        ),
                    )]);
                }

                return Ok(());
            }

            match source_type.validate_quantity_result_unit(unit_name) {
                Ok(()) => Ok(()),
                Err(message) => Err(vec![engine_error_at_graph(graph, source, message)]),
            }
        }
        SemanticConversionTarget::RatioUnit(unit_name) => {
            if source_type.is_ratio_range() {
                return check_range_span_unit_conversion(
                    graph,
                    source_type,
                    target,
                    resolved_types,
                    source,
                    spec_arc,
                );
            }
            let unit_check: Option<(bool, Vec<&str>)> = match &source_type.specifications {
                TypeSpecification::Ratio { units, .. } => {
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
                    if find_types_by_spec(resolved_types, spec_arc)
                        .and_then(|dt| dt.unit_index.get(unit_name))
                        .is_none()
                    {
                        Err(vec![engine_error_at_graph(
                            graph,
                            source,
                            format!("Unknown unit '{}' in spec '{}'.", unit_name, spec_arc.name),
                        )])
                    } else {
                        Ok(())
                    }
                }
                None => Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!(
                        "Cannot convert {} to ratio unit '{}'.",
                        source_type.name(),
                        unit_name
                    ),
                )]),
            }
        }
        SemanticConversionTarget::Calendar(_) => {
            if !source_type.is_calendar()
                && !source_type.is_number()
                && !source_type.is_date_range()
            {
                Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    format!("Cannot convert {} to calendar.", source_type.name()),
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
        DataDefinition::Import { .. } => Err(vec![engine_error_at_graph(
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
    spec_arc: &Arc<LemmaSpec>,
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
                check_expression(left, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );
            collect(
                check_expression(right, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let left_type =
                infer_expression_type(left, graph, inferred_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, inferred_types, resolved_types, spec_arc);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_logical_and_operands(graph, &left_type, &right_type, expr_source),
                &mut errors,
            );
        }

        ExpressionKind::LogicalOr(left, right) => {
            collect(
                check_expression(left, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );
            collect(
                check_expression(right, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let left_type =
                infer_expression_type(left, graph, inferred_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, inferred_types, resolved_types, spec_arc);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_logical_or_operands(graph, &left_type, &right_type, expr_source),
                &mut errors,
            );
        }

        ExpressionKind::LogicalNegation(operand, _) => {
            collect(
                check_expression(operand, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let operand_type =
                infer_expression_type(operand, graph, inferred_types, resolved_types, spec_arc);
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
                check_expression(left, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );
            collect(
                check_expression(right, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let left_type =
                infer_expression_type(left, graph, inferred_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, inferred_types, resolved_types, spec_arc);
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
                check_expression(left, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );
            collect(
                check_expression(right, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let left_type =
                infer_expression_type(left, graph, inferred_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, inferred_types, resolved_types, spec_arc);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");
            collect(
                check_arithmetic_types(graph, &left_type, &right_type, operator, expr_source),
                &mut errors,
            );
            collect(
                arithmetic_plan_time_exactness_planning_errors(
                    graph,
                    left,
                    right,
                    &left_type,
                    &right_type,
                    operator,
                    expr_source,
                ),
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
                    spec_arc,
                ),
                &mut errors,
            );

            let source_type = infer_expression_type(
                source_expression,
                graph,
                inferred_types,
                resolved_types,
                spec_arc,
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
                    spec_arc,
                ),
                &mut errors,
            );

            // For number sources with QuantityUnit/RatioUnit targets, check_unit_conversion_types
            // already validates the unit exists in the index. No additional check is needed here.
        }

        ExpressionKind::MathematicalComputation(_, operand) => {
            collect(
                check_expression(operand, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let operand_type =
                infer_expression_type(operand, graph, inferred_types, resolved_types, spec_arc);
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

        ExpressionKind::ResultIsVeto(operand) => {
            collect(
                check_expression(operand, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );
        }

        ExpressionKind::Now => {}

        ExpressionKind::DateRelative(_, date_expr) => {
            collect(
                check_expression(date_expr, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let date_type =
                infer_expression_type(date_expr, graph, inferred_types, resolved_types, spec_arc);
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
        }

        ExpressionKind::DateCalendar(_, _, date_expr) => {
            collect(
                check_expression(date_expr, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let date_type =
                infer_expression_type(date_expr, graph, inferred_types, resolved_types, spec_arc);
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

        ExpressionKind::RangeLiteral(left, right) => {
            collect(
                check_expression(left, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );
            collect(
                check_expression(right, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let left_type =
                infer_expression_type(left, graph, inferred_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, inferred_types, resolved_types, spec_arc);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");

            let inferred_range_type = infer_range_type_from_endpoint_types(&left_type, &right_type);
            if inferred_range_type.is_undetermined() {
                errors.push(engine_error_at_graph(
                    graph,
                    expr_source,
                    format!(
                        "Cannot create a range from {} and {}.",
                        left_type.name(),
                        right_type.name()
                    ),
                ));
            }
        }

        ExpressionKind::PastFutureRange(_, offset_expr) => {
            collect(
                check_expression(offset_expr, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let offset_type =
                infer_expression_type(offset_expr, graph, inferred_types, resolved_types, spec_arc);
            if !offset_type.is_duration_like() && !offset_type.is_calendar() {
                let expr_source = expression
                    .source_location
                    .as_ref()
                    .expect("BUG: expression missing source in check_expression");
                errors.push(engine_error_at_graph(
                    graph,
                    expr_source,
                    format!(
                        "Past/future range requires a duration or calendar expression, got type '{}'",
                        offset_type.name()
                    ),
                ));
            }
        }

        ExpressionKind::RangeContainment(value, range) => {
            collect(
                check_expression(value, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );
            collect(
                check_expression(range, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let value_type =
                infer_expression_type(value, graph, inferred_types, resolved_types, spec_arc);
            let range_type =
                infer_expression_type(range, graph, inferred_types, resolved_types, spec_arc);
            let expr_source = expression
                .source_location
                .as_ref()
                .expect("BUG: expression missing source in check_expression");

            if !range_type.is_range() {
                errors.push(engine_error_at_graph(
                    graph,
                    expr_source,
                    format!(
                        "Right side of 'in' must be a range, got type '{}'",
                        range_type.name()
                    ),
                ));
            } else {
                let compatible = (range_type.is_date_range() && value_type.is_date())
                    || (range_type.is_number_range() && value_type.is_number())
                    || (range_type.is_quantity_range()
                        && value_type.is_quantity()
                        && quantity_range_matches_quantity(&range_type, &value_type))
                    || (range_type.is_ratio_range() && value_type.is_ratio())
                    || (range_type.is_calendar_range() && value_type.is_calendar());
                if !compatible {
                    errors.push(engine_error_at_graph(
                        graph,
                        expr_source,
                        format!(
                            "Cannot test whether {} is in {}.",
                            value_type.name(),
                            range_type.name()
                        ),
                    ));
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
        let spec_arc = &rule_node.spec_arc;

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
                spec_arc,
            ),
            &mut errors,
        );
        let default_type = infer_expression_type(
            default_result,
            graph,
            inferred_types,
            resolved_types,
            spec_arc,
        );

        // Anonymous intermediates with unresolved dimensions are forbidden at rule boundaries.
        if default_type.is_anonymous_quantity() {
            let decomp = default_type.quantity_type_decomposition();
            if !decomp.is_empty() {
                let default_source = default_result
                    .source_location
                    .as_ref()
                    .expect("BUG: default branch result expression has no source location");
                errors.push(engine_error_at_graph(
                    graph,
                    default_source,
                    format!(
                        "Rule '{}' in spec '{}' returns an anonymous intermediate with unresolved \
                         dimensions {:?}. Cast the result with 'as <unit>' (e.g., 'as mps') \
                         or ensure all dimensions cancel.",
                        rule_path.rule, spec_arc.name, decomp
                    ),
                ));
            }
        }

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
                        spec_arc,
                    ),
                    &mut errors,
                );
                let condition_type = infer_expression_type(
                    condition_expression,
                    graph,
                    inferred_types,
                    resolved_types,
                    spec_arc,
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
                check_expression(result, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );
            let result_type =
                infer_expression_type(result, graph, inferred_types, resolved_types, spec_arc);

            // Anonymous intermediates with unresolved dimensions are forbidden at rule boundaries.
            if result_type.is_anonymous_quantity() {
                let decomp = result_type.quantity_type_decomposition();
                if !decomp.is_empty() {
                    let branch_source = result
                        .source_location
                        .as_ref()
                        .expect("BUG: unless branch result expression has no source location");
                    errors.push(engine_error_at_graph(
                        graph,
                        branch_source,
                        format!(
                            "Unless clause {} in rule '{}' (spec '{}') returns an anonymous \
                             intermediate with unresolved dimensions {:?}. Cast the result with \
                             'as <unit>' or ensure all dimensions cancel.",
                            branch_index, rule_path.rule, spec_arc.name, decomp
                        ),
                    ));
                }
            }

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
                            rule_source.source_type, rule_source.span.line, rule_source.span.col
                        )];

                        if let Some(loc) = &default_expr.source_location {
                            location_parts.push(format!(
                                "default branch at {}:{}:{}",
                                loc.source_type, loc.span.line, loc.span.col
                            ));
                        }
                        if let Some(loc) = &result.source_location {
                            location_parts.push(format!(
                                "unless clause {} at {}:{}:{}",
                                branch_index, loc.source_type, loc.span.line, loc.span.col
                            ));
                        }

                        errors.push(Error::validation_with_context(
                            format!("Type mismatch in rule '{}' in spec '{}' ({}): default branch returns {}, but unless clause {} returns {}. All branches must return the same primitive type.",
                            rule_path.rule,
                            spec_arc.name,
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
        let spec_arc = &rule_node.spec_arc;

        if branches.is_empty() {
            continue;
        }

        let (_, default_result) = &branches[0];
        let default_type = infer_expression_type(
            default_result,
            graph,
            &computed_types,
            resolved_types,
            spec_arc,
        );

        let mut non_veto_type: Option<LemmaType> = None;
        if !default_type.vetoed() && !default_type.is_undetermined() {
            non_veto_type = Some(default_type.clone());
        }

        for (_branch_index, (_condition, result)) in branches.iter().enumerate().skip(1) {
            let result_type =
                infer_expression_type(result, graph, &computed_types, resolved_types, spec_arc);
            if !result_type.vetoed() && !result_type.is_undetermined() && non_veto_type.is_none() {
                non_veto_type = Some(result_type.clone());
            }
        }

        let rule_type = non_veto_type.unwrap_or_else(LemmaType::veto_type);
        computed_types.insert(rule_path.clone(), rule_type);
    }

    computed_types
}

type UnitDecompLookup = HashMap<
    String,
    (
        String,
        BaseQuantityVector,
        crate::computation::rational::RationalInteger,
    ),
>;

fn declared_quantity_decomposition(type_name: &str, lemma_type: &LemmaType) -> BaseQuantityVector {
    match &lemma_type.specifications {
        TypeSpecification::Quantity { traits, .. }
            if traits.contains(&semantics::QuantityTrait::Duration) =>
        {
            duration_decomposition()
        }
        _ => {
            let dimension_key = lemma_type
                .quantity_family_name()
                .unwrap_or(type_name)
                .to_string();
            [(dimension_key, 1i32)].into_iter().collect()
        }
    }
}

fn sync_unit_index_from_resolved(
    resolved: &HashMap<String, LemmaType>,
    unit_index: &mut HashMap<String, LemmaType>,
) {
    let unit_index_updates: Vec<(String, LemmaType)> = unit_index
        .iter()
        .filter_map(|(unit_name, pre_decomp_type)| {
            let type_name = pre_decomp_type
                .name
                .as_deref()
                .or_else(|| pre_decomp_type.quantity_family_name())?;
            resolved
                .get(type_name)
                .or_else(|| {
                    pre_decomp_type
                        .quantity_family_name()
                        .and_then(|family| resolved.get(family))
                })
                .map(|post_decomp_type| (unit_name.clone(), post_decomp_type.clone()))
        })
        .collect();
    for (unit_name, post_decomp_type) in unit_index_updates {
        unit_index.insert(unit_name, post_decomp_type);
    }
}

/// `uses`-merged quantity rows in `unit_index` can still have empty `decomposition` until synced from
/// `resolved`. Compound unit resolution consults `unit_index` first; fill simple base quantitys here
/// before building [`UnitDecompLookup`].
fn repair_empty_simple_quantity_decomposition_in_unit_index(
    unit_index: &mut HashMap<String, LemmaType>,
) {
    for (_unit_key, lemma_type) in unit_index.iter_mut() {
        let base_decomp = {
            let TypeSpecification::Quantity {
                units,
                decomposition,
                ..
            } = &lemma_type.specifications
            else {
                continue;
            };
            if !decomposition.is_empty() {
                continue;
            }
            if units.is_empty() || units.iter().any(|u| !u.derived_quantity_factors.is_empty()) {
                continue;
            }
            let Some(ref type_name) = lemma_type.name else {
                continue;
            };
            if type_name.is_empty() {
                continue;
            }
            let candidate = declared_quantity_decomposition(type_name.as_str(), lemma_type);
            if candidate.is_empty() {
                continue;
            }
            Some(candidate)
        };
        let Some(base_decomp) = base_decomp else {
            continue;
        };
        let TypeSpecification::Quantity {
            units,
            decomposition,
            canonical_unit,
            ..
        } = &mut lemma_type.specifications
        else {
            continue;
        };
        let mut canonical = String::new();
        for unit in units.0.iter_mut() {
            unit.decomposition = base_decomp.clone();
            if unit.is_canonical_factor() && canonical.is_empty() {
                canonical = unit.name.clone();
            }
        }
        *decomposition = base_decomp;
        *canonical_unit = canonical;
    }
}

fn owning_quantity_type_name_for_unit(
    unit_name: &str,
    lookup: &UnitDecompLookup,
    unit_index: &HashMap<String, LemmaType>,
) -> Option<String> {
    if let Some((owning_quantity_name, _, _)) = lookup.get(unit_name) {
        return Some(owning_quantity_name.clone());
    }
    unit_index.get(unit_name).and_then(|lemma_type| {
        lemma_type
            .name
            .clone()
            .or_else(|| lemma_type.quantity_family_name().map(str::to_string))
    })
}

/// Order compound quantity types so every referenced unit from another compound type is resolved first.
fn sort_derived_quantity_types_for_resolution(
    spec_name: &str,
    derived_quantity_type_names: Vec<String>,
    resolved: &HashMap<String, LemmaType>,
    lookup: &UnitDecompLookup,
    unit_index: &HashMap<String, LemmaType>,
    source_for: &dyn Fn(&str) -> Option<Source>,
) -> Result<Vec<String>, Error> {
    let derived_quantity_type_count = derived_quantity_type_names.len();
    if derived_quantity_type_count == 0 {
        return Ok(derived_quantity_type_names);
    }

    let type_index: HashMap<&str, usize> = derived_quantity_type_names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();

    let mut dependency_sets: Vec<HashSet<usize>> =
        vec![HashSet::new(); derived_quantity_type_count];

    for (dependent_index, type_name) in derived_quantity_type_names.iter().enumerate() {
        let TypeSpecification::Quantity { units, .. } = &resolved[type_name].specifications else {
            continue;
        };
        for unit in units.iter() {
            for (factor_unit_name, _) in &unit.derived_quantity_factors {
                let Some(owning_quantity_name) =
                    owning_quantity_type_name_for_unit(factor_unit_name, lookup, unit_index)
                else {
                    continue;
                };
                let Some(dependency_index) = type_index.get(owning_quantity_name.as_str()).copied()
                else {
                    continue;
                };
                if dependency_index == dependent_index {
                    continue;
                }
                dependency_sets[dependent_index].insert(dependency_index);
            }
        }
    }

    let mut in_degree = vec![0usize; derived_quantity_type_count];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); derived_quantity_type_count];

    for (dependent_index, dependencies) in dependency_sets.iter().enumerate() {
        for &dependency_index in dependencies {
            in_degree[dependent_index] += 1;
            dependents[dependency_index].push(dependent_index);
        }
    }

    let mut queue: VecDeque<usize> = (0..derived_quantity_type_count)
        .filter(|&index| in_degree[index] == 0)
        .collect();
    let mut sorted_indices: Vec<usize> = Vec::with_capacity(derived_quantity_type_count);

    while let Some(index) = queue.pop_front() {
        sorted_indices.push(index);
        for &dependent_index in &dependents[index] {
            in_degree[dependent_index] -= 1;
            if in_degree[dependent_index] == 0 {
                queue.push_back(dependent_index);
            }
        }
    }

    if sorted_indices.len() != derived_quantity_type_count {
        let mut cycle_type_names: Vec<String> = (0..derived_quantity_type_count)
            .filter(|&index| in_degree[index] > 0)
            .map(|index| derived_quantity_type_names[index].clone())
            .collect();
        cycle_type_names.sort();
        return Err(Error::validation(
            format!(
                "In spec '{}': circular compound quantity type dependency among: {}",
                spec_name,
                cycle_type_names.join(", ")
            ),
            source_for(&cycle_type_names[0]),
            None::<String>,
        ));
    }

    Ok(sorted_indices
        .into_iter()
        .map(|index| derived_quantity_type_names[index].clone())
        .collect())
}

fn resolve_quantity_decompositions(
    spec_name: &str,
    resolved: &mut HashMap<String, LemmaType>,
    unit_index: &mut HashMap<String, LemmaType>,
    type_sources: &HashMap<String, Source>,
) -> Vec<Error> {
    let mut errors: Vec<Error> = Vec::new();

    let source_for = |type_name: &str| -> Option<Source> {
        type_sources
            .get(type_name)
            .or_else(|| type_sources.values().next())
            .cloned()
    };

    let base_type_names: Vec<String> = resolved
        .iter()
        .filter_map(|(name, lt)| {
            if let TypeSpecification::Quantity { units, .. } = &lt.specifications {
                if units.iter().all(|u| u.derived_quantity_factors.is_empty()) {
                    return Some(name.clone());
                }
            }
            None
        })
        .collect();

    for type_name in &base_type_names {
        let base_decomp = {
            let lemma_type = resolved.get(type_name).unwrap();
            declared_quantity_decomposition(type_name, lemma_type)
        };

        let lemma_type = resolved.get_mut(type_name).unwrap();
        let TypeSpecification::Quantity {
            units,
            decomposition,
            canonical_unit,
            ..
        } = &mut lemma_type.specifications
        else {
            continue;
        };

        let mut canonical = String::new();
        for unit in units.0.iter_mut() {
            unit.decomposition = base_decomp.clone();
            if unit.is_canonical_factor() && canonical.is_empty() {
                canonical = unit.name.clone();
            }
        }

        *decomposition = base_decomp;
        *canonical_unit = canonical;
    }

    repair_empty_simple_quantity_decomposition_in_unit_index(unit_index);

    let mut lookup = UnitDecompLookup::new();

    for (unit_name, lemma_type) in unit_index.iter() {
        if let TypeSpecification::Quantity {
            decomposition,
            units,
            ..
        } = &lemma_type.specifications
        {
            if !decomposition.is_empty() {
                let quantity_name = lemma_type.name.clone().unwrap_or_default();
                let factor = units
                    .iter()
                    .find(|u| &u.name == unit_name)
                    .map(|u| u.factor)
                    .unwrap_or_else(crate::computation::rational::rational_one);
                lookup.insert(
                    unit_name.clone(),
                    (quantity_name, decomposition.clone(), factor),
                );
            }
        }
    }

    for (type_name, lemma_type) in resolved.iter() {
        if let TypeSpecification::Quantity {
            units,
            decomposition,
            ..
        } = &lemma_type.specifications
        {
            if !decomposition.is_empty() {
                let is_defining_type = lemma_type
                    .quantity_family_name()
                    .map(|family| family == type_name.as_str())
                    .unwrap_or(false);
                if !is_defining_type {
                    continue;
                }
                for unit in units.iter() {
                    lookup.insert(
                        unit.name.clone(),
                        (type_name.clone(), decomposition.clone(), unit.factor),
                    );
                }
            }
        }
    }

    let derived_quantity_type_names_unsorted: Vec<String> = resolved
        .iter()
        .filter_map(|(name, lemma_type)| {
            if let TypeSpecification::Quantity { units, .. } = &lemma_type.specifications {
                if units
                    .iter()
                    .any(|unit| !unit.derived_quantity_factors.is_empty())
                {
                    return Some(name.clone());
                }
            }
            None
        })
        .collect();

    let derived_quantity_type_names = match sort_derived_quantity_types_for_resolution(
        spec_name,
        derived_quantity_type_names_unsorted,
        resolved,
        &lookup,
        unit_index,
        &|type_name| source_for(type_name),
    ) {
        Ok(sorted) => sorted,
        Err(error) => {
            errors.push(error);
            return errors;
        }
    };

    for type_name in &derived_quantity_type_names {
        let type_source = source_for(type_name);

        let units_snapshot = match &resolved[type_name].specifications {
            TypeSpecification::Quantity { units, .. } => units.clone(),
            _ => continue,
        };

        let mut resolved_type_decomp: Option<BaseQuantityVector> = None;
        let mut canonical = String::new();
        let mut unit_errors: Vec<Error> = Vec::new();
        let mut resolved_unit_factors: Vec<Option<crate::computation::rational::RationalInteger>> =
            vec![None; units_snapshot.len()];

        for (unit_idx, unit) in units_snapshot.iter().enumerate() {
            if unit.derived_quantity_factors.is_empty() {
                let simple_decomp =
                    declared_quantity_decomposition(type_name, &resolved[type_name]);

                if let Some(existing) = &resolved_type_decomp {
                    if existing != &simple_decomp {
                        unit_errors.push(Error::validation(
                            format!(
                                "In spec '{}': quantity type '{}' has inconsistent unit decompositions. \
                                 Unit '{}' is a simple unit (decomposition {{{}: 1}}) but other units \
                                 have decomposition {:?}.",
                                spec_name, type_name, unit.name, type_name, existing
                            ),
                            type_source.clone(),
                            None::<String>,
                        ));
                    }
                } else {
                    resolved_type_decomp = Some(simple_decomp);
                }

                resolved_unit_factors[unit_idx] = Some(unit.factor);

                if unit.is_canonical_factor() && canonical.is_empty() {
                    canonical = unit.name.clone();
                }
                continue;
            }

            match resolve_compound_unit(
                spec_name,
                type_name,
                &unit.name,
                unit.factor,
                &unit.derived_quantity_factors,
                &lookup,
                type_source.as_ref(),
            ) {
                Ok((unit_decomp, derived_factor)) => {
                    if let Some(existing) = &resolved_type_decomp {
                        if existing != &unit_decomp {
                            unit_errors.push(Error::validation(
                                format!(
                                    "In spec '{}': quantity type '{}' has inconsistent unit \
                                     decompositions. Unit '{}' resolved to {:?} but other units \
                                     resolved to {:?}. All units of a quantity must measure the same \
                                     physical quantity.",
                                    spec_name, type_name, unit.name, unit_decomp, existing
                                ),
                                type_source.clone(),
                                None::<String>,
                            ));
                        }
                    } else {
                        resolved_type_decomp = Some(unit_decomp);
                    }

                    resolved_unit_factors[unit_idx] = Some(derived_factor);

                    if derived_factor == crate::computation::rational::rational_one()
                        && canonical.is_empty()
                    {
                        canonical = unit.name.clone();
                    }
                }
                Err(err) => unit_errors.push(err),
            }
        }

        if !unit_errors.is_empty() {
            errors.extend(unit_errors);
            continue;
        }

        let type_decomp = match resolved_type_decomp {
            Some(d) => d,
            None => continue,
        };

        if canonical.is_empty() {
            use crate::computation::rational::{checked_div, rational_is_zero};

            let Some((normalizer_unit_index, normalizer_factor)) = resolved_unit_factors
                .iter()
                .enumerate()
                .find_map(|(unit_index, factor)| factor.map(|factor| (unit_index, factor)))
            else {
                errors.push(Error::validation(
                    format!(
                        "In spec '{}': quantity type '{}' has no unit with conversion factor 1. \
                         Exactly one unit must have factor 1.",
                        spec_name, type_name
                    ),
                    type_source.clone(),
                    None::<String>,
                ));
                continue;
            };

            if rational_is_zero(&normalizer_factor) {
                errors.push(Error::validation(
                    format!(
                        "In spec '{}': quantity type '{}' cannot normalize conversion factors because \
                         unit '{}' has a zero conversion factor.",
                        spec_name,
                        type_name,
                        units_snapshot.0[normalizer_unit_index].name
                    ),
                    type_source.clone(),
                    None::<String>,
                ));
                continue;
            }

            let mut normalization_failed = false;
            for (unit_index, resolved_factor) in resolved_unit_factors.iter_mut().enumerate() {
                let Some(factor) = resolved_factor.as_ref() else {
                    continue;
                };
                match checked_div(factor, &normalizer_factor) {
                    Ok(normalized_factor) => {
                        *resolved_factor = Some(normalized_factor);
                    }
                    Err(error) => {
                        normalization_failed = true;
                        errors.push(Error::validation(
                            format!(
                                "In spec '{}': quantity type '{}' overflowed while normalizing \
                                 conversion factor for unit '{}': {}",
                                spec_name, type_name, units_snapshot.0[unit_index].name, error
                            ),
                            type_source.clone(),
                            None::<String>,
                        ));
                    }
                }
            }

            if normalization_failed {
                continue;
            }

            canonical = units_snapshot.0[normalizer_unit_index].name.clone();
        }

        let lemma_type = resolved.get_mut(type_name).unwrap();
        let TypeSpecification::Quantity {
            units,
            decomposition,
            canonical_unit,
            ..
        } = &mut lemma_type.specifications
        else {
            continue;
        };

        for (unit_idx, unit) in units.0.iter_mut().enumerate() {
            unit.decomposition = type_decomp.clone();
            if let Some(factor) = resolved_unit_factors[unit_idx] {
                unit.factor = factor;
            }
        }
        *decomposition = type_decomp.clone();
        *canonical_unit = canonical;

        for unit in units.0.iter() {
            lookup.insert(
                unit.name.clone(),
                (type_name.clone(), type_decomp.clone(), unit.factor),
            );
        }
    }

    for type_name in &base_type_names {
        let lemma_type = resolved.get(type_name).unwrap();
        let TypeSpecification::Quantity {
            units,
            canonical_unit,
            ..
        } = &lemma_type.specifications
        else {
            continue;
        };

        if canonical_unit.is_empty() && !units.is_empty() {
            errors.push(Error::validation(
                format!(
                    "In spec '{}': quantity type '{}' has no unit with conversion factor 1. \
                     Exactly one unit must have factor 1.",
                    spec_name, type_name
                ),
                source_for(type_name),
                None::<String>,
            ));
        }
    }

    sync_unit_index_from_resolved(resolved, unit_index);
    repair_empty_simple_quantity_decomposition_in_unit_index(unit_index);

    errors
}

fn resolve_compound_unit(
    spec_name: &str,
    declaring_type_name: &str,
    unit_name: &str,
    prefix: crate::computation::rational::RationalInteger,
    factors: &[(String, i32)],
    lookup: &UnitDecompLookup,
    source: Option<&Source>,
) -> Result<
    (
        BaseQuantityVector,
        crate::computation::rational::RationalInteger,
    ),
    Error,
> {
    use crate::computation::rational::{checked_mul, checked_pow_i32};

    let mut result: BaseQuantityVector = BaseQuantityVector::new();
    let mut derived_factor = prefix;

    for (quantity_ref, exponent) in factors {
        if let Some(calendar_unit) = CalendarUnit::from_keyword(quantity_ref) {
            let calendar_factor = calendar_unit.canonical_factor();
            let calendar_decomp = calendar_decomposition();
            for (dim, &dim_exp) in &calendar_decomp {
                accumulate(&mut result, dim, dim_exp * exponent);
            }
            let calendar_rational = calendar_factor;
            let component_contribution =
                checked_pow_i32(&calendar_rational, *exponent).map_err(|error| {
                    overflow_to_validation_error(
                        spec_name,
                        unit_name,
                        declaring_type_name,
                        quantity_ref,
                        error,
                        source,
                    )
                })?;
            derived_factor =
                checked_mul(&derived_factor, &component_contribution).map_err(|error| {
                    overflow_to_validation_error(
                        spec_name,
                        unit_name,
                        declaring_type_name,
                        quantity_ref,
                        error,
                        source,
                    )
                })?;
            continue;
        }

        let (owning_quantity_name, owning_decomp, unit_factor) =
            lookup.get(quantity_ref.as_str()).ok_or_else(|| {
                Error::validation(
                    format!(
                        "In spec '{}': unit '{}' in quantity type '{}' references '{}' which is not a \
                         known unit of any in-scope quantity type. Add `uses <spec>` (or declare the \
                         owning quantity type in this spec) so its units are in scope.",
                        spec_name, unit_name, declaring_type_name, quantity_ref
                    ),
                    source.cloned(),
                    None::<String>,
                )
            })?;

        if owning_quantity_name == declaring_type_name {
            return Err(Error::validation(
                format!(
                    "In spec '{}': unit '{}' in quantity type '{}' references unit '{}' which \
                     belongs to the same quantity type. A quantity cannot reference its own units \
                     in a compound expression.",
                    spec_name, unit_name, declaring_type_name, quantity_ref
                ),
                source.cloned(),
                None::<String>,
            ));
        }

        if owning_decomp.is_empty() {
            return Err(Error::validation(
                format!(
                    "In spec '{}': unit '{}' in quantity type '{}' references '{}' whose owning \
                     quantity type '{}' does not yet have a resolved decomposition. Ensure base \
                     quantities are declared before derived quantities that depend on them.",
                    spec_name, unit_name, declaring_type_name, quantity_ref, owning_quantity_name
                ),
                source.cloned(),
                None::<String>,
            ));
        }

        for (dim, &dim_exp) in owning_decomp {
            accumulate(&mut result, dim, dim_exp * exponent);
        }

        let component_contribution = checked_pow_i32(unit_factor, *exponent).map_err(|error| {
            overflow_to_validation_error(
                spec_name,
                unit_name,
                declaring_type_name,
                quantity_ref,
                error,
                source,
            )
        })?;
        derived_factor =
            checked_mul(&derived_factor, &component_contribution).map_err(|error| {
                overflow_to_validation_error(
                    spec_name,
                    unit_name,
                    declaring_type_name,
                    quantity_ref,
                    error,
                    source,
                )
            })?;
    }

    Ok((result, derived_factor))
}

fn overflow_to_validation_error(
    spec_name: &str,
    unit_name: &str,
    declaring_type_name: &str,
    quantity_ref: &str,
    failure: crate::computation::rational::NumericFailure,
    source: Option<&Source>,
) -> Error {
    Error::validation(
        format!(
            "In spec '{}': unit '{}' in quantity type '{}' overflowed while combining '{}': {}",
            spec_name, unit_name, declaring_type_name, quantity_ref, failure
        ),
        source.cloned(),
        None::<String>,
    )
}

fn accumulate(result: &mut BaseQuantityVector, dim: &str, value: i32) {
    let entry = result.entry(dim.to_string()).or_insert(0);
    *entry += value;
    if *entry == 0 {
        result.remove(dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parsing::ast::{BooleanValue, Reference, Span, Value};

    fn test_source() -> Source {
        Source::new(
            crate::parsing::source::SourceType::Volatile,
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
        let repository = ctx.workspace();
        for s in all_specs {
            if let Err(e) = ctx.insert_spec(Arc::clone(&repository), Arc::new(s.clone())) {
                return Err(vec![e]);
            }
        }
        let effective = EffectiveDate::from_option(main_spec.effective_from().cloned());
        let main_spec_arc = ctx
            .spec_set(&repository, main_spec.name.as_str())
            .and_then(|ss| ss.get_exact(main_spec.effective_from()).cloned())
            .expect("main_spec must be in all_specs");
        let dag =
            discovery::build_dag_for_spec(&ctx, &main_spec_arc, &effective).map_err(
                |e| match e {
                    discovery::DagError::Cycle(es) | discovery::DagError::Other(es) => es,
                },
            )?;
        match Graph::build(&ctx, &repository, &main_spec_arc, &dag, &effective) {
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
            value: ParsedDataValue::Definition {
                base: None,
                constraints: None,
                value: Some(value),
            },
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
            value: ParsedDataValue::Definition {
                base: None,
                constraints: None,
                value: Some(Value::Number(2.into())),
            },
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
        assert!(errors.iter().any(|e| {
            let s = e.to_string();
            s.contains("already used") && s.contains("age")
        }));
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
            value: ParsedDataValue::Import(crate::parsing::ast::SpecRef::same_repository(
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
            .any(|e| e.to_string().contains("already used")));
        assert!(errors
            .iter()
            .any(|e| e.to_string().contains("Reference 'nonexistent' not found")));
    }

    #[test]
    fn test_type_registration_collects_multiple_errors() {
        use crate::parsing::ast::{DataValue, ParentType, PrimitiveKind, SpecRef};

        let type_source = Source::new(
            crate::parsing::source::SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let spec_a = create_test_spec("spec_a")
            .with_source_type(crate::parsing::source::SourceType::Volatile)
            .add_data(LemmaData {
                reference: Reference::local("dep".to_string()),
                value: DataValue::Import(SpecRef::same_repository("spec_b")),
                source_location: type_source.clone(),
            })
            .add_data(LemmaData {
                reference: Reference::local("money".to_string()),
                value: DataValue::Definition {
                    base: Some(ParentType::Primitive {
                        primitive: PrimitiveKind::Number,
                    }),
                    constraints: None,
                    value: None,
                },
                source_location: type_source.clone(),
            })
            .add_data(LemmaData {
                reference: Reference::local("money".to_string()),
                value: DataValue::Definition {
                    base: Some(ParentType::Primitive {
                        primitive: PrimitiveKind::Number,
                    }),
                    constraints: None,
                    value: None,
                },
                source_location: type_source,
            });

        let type_source_b = Source::new(
            crate::parsing::source::SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let spec_b = create_test_spec("spec_b")
            .with_source_type(crate::parsing::source::SourceType::Volatile)
            .add_data(LemmaData {
                reference: Reference::local("length".to_string()),
                value: DataValue::Definition {
                    base: Some(ParentType::Primitive {
                        primitive: PrimitiveKind::Number,
                    }),
                    constraints: None,
                    value: None,
                },
                source_location: type_source_b.clone(),
            })
            .add_data(LemmaData {
                reference: Reference::local("length".to_string()),
                value: DataValue::Definition {
                    base: Some(ParentType::Primitive {
                        primitive: PrimitiveKind::Number,
                    }),
                    constraints: None,
                    value: None,
                },
                source_location: type_source_b,
            });

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile.to_string(),
            "spec spec_a\nuses dep: spec_b\ndata money: number\ndata money: number".to_string(),
        );
        sources.insert(
            crate::parsing::source::SourceType::Volatile.to_string(),
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
uses m: myspec
rule result: m.x"#;
        let specs = crate::parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &crate::ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
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
uses m: nonexistent
rule result: m.x"#;
        let specs = crate::parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &crate::ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let consumer = specs.iter().find(|d| d.name == "consumer").unwrap();
        let result = build_graph(consumer, &specs);
        assert!(result.is_err(), "Should fail for non-existent spec");
    }

    // =================================================================
    // Self-reference: same spec body via uses (planning)
    // =================================================================

    #[test]
    fn import_alias_registered_in_graph() {
        let code = r#"
spec inner
data x: number -> default 1

spec outer
uses i: inner
rule r: i.x
"#;
        let specs = crate::parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &crate::ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let outer = specs.iter().find(|s| s.name == "outer").unwrap();
        let graph = build_graph(outer, &specs).expect("uses i: inner must plan");

        let alias_path = DataPath {
            segments: Vec::new(),
            data: "i".to_string(),
        };
        match graph.data().get(&alias_path) {
            Some(DataDefinition::Import { spec, .. }) => {
                assert_eq!(spec.name, "inner");
            }
            other => panic!(
                "alias path 'i' must be DataDefinition::Import, got {:?}",
                other
            ),
        }

        let nested_path = DataPath {
            segments: vec![PathSegment {
                data: "i".to_string(),
                spec: "inner".to_string(),
            }],
            data: "x".to_string(),
        };
        assert!(
            graph.data().contains_key(&nested_path),
            "nested data i.x must exist after nested build_spec"
        );
    }

    #[test]
    fn self_reference_is_error() {
        let code = "spec myspec\nuses m: myspec";
        let specs = crate::parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &crate::ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let result = build_graph(&specs[0], &specs);
        assert!(result.is_err(), "Self-reference should be an error");
        let errors = result.unwrap_err();
        let joined: String = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            joined.contains("cannot reference itself") && joined.contains("myspec"),
            "Error should name self-reference: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }
}

// ============================================================================
// Type resolution
// ============================================================================

/// Fully resolved types for a single spec.
/// After resolution, all imports are inlined — specs are independent.
#[derive(Debug, Clone)]
pub struct ResolvedSpecTypes {
    /// Resolved [`LemmaType`] for each **data type row name** declared in this spec (`data name: …`).
    /// Planning-only: includes quantity units and post-`resolve_quantity_decompositions` decomposition.
    pub resolved: HashMap<String, LemmaType>,

    /// Declared default per named type (e.g. `type rate: ratio -> default 0.5`).
    /// Only present for types that declared a `-> default ...` constraint anywhere
    /// in their extension chain; the inner-most `-> default` wins. Defaults live
    /// outside [`TypeSpecification`] so the type itself stays free of binding data.
    pub declared_defaults: HashMap<String, ValueKind>,

    /// Unit index: unit_name -> resolved type.
    /// Built during resolution — if unit appears in multiple types, resolution fails.
    pub unit_index: HashMap<String, LemmaType>,
}

/// Intermediate type definition extracted from [`DataValue::Definition`] data.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DataTypeDef {
    pub parent: ParentType,
    pub constraints: Option<Vec<Constraint>>,
    pub source: crate::parsing::source::Source,
    pub name: String,
    /// When the source row was `data N: <literal>` (no explicit parent type), the AST literal.
    pub bound_literal: Option<ast::Value>,
}

///
/// Named types are extracted from [`DataValue::Definition`] data and keyed by pointer
/// identity (`Arc::ptr_eq`) — no `Hash`/`Eq` on `LemmaSpec` required.
#[derive(Debug, Clone)]
pub(crate) struct TypeResolver<'a> {
    data_types: Vec<(Arc<LemmaSpec>, HashMap<String, DataTypeDef>)>,
    context: &'a Context,
    all_registered_specs: Vec<(Arc<LemmaRepository>, Arc<LemmaSpec>)>,
}

/// Infer primitive [`ParentType`] from a literal RHS (`data x: 3.14`).
fn inferred_parent_type_from_literal(value: &ast::Value) -> ParentType {
    match value {
        ast::Value::Number(_) => ParentType::Primitive {
            primitive: PrimitiveKind::Number,
        },
        ast::Value::Text(_) => ParentType::Primitive {
            primitive: PrimitiveKind::Text,
        },
        ast::Value::Boolean(_) => ParentType::Primitive {
            primitive: PrimitiveKind::Boolean,
        },
        ast::Value::Date(_) => ParentType::Primitive {
            primitive: PrimitiveKind::Date,
        },
        ast::Value::Time(_) => ParentType::Primitive {
            primitive: PrimitiveKind::Time,
        },
        ast::Value::NumberWithUnit(_, _) => ParentType::Primitive {
            primitive: PrimitiveKind::Quantity,
        },
        ast::Value::Calendar(_, _) => ParentType::Primitive {
            primitive: PrimitiveKind::Calendar,
        },
        ast::Value::Range(left, right) => {
            let primitive = match (left.as_ref(), right.as_ref()) {
                (ast::Value::Number(_), ast::Value::Number(_)) => PrimitiveKind::NumberRange,
                (ast::Value::Date(_), ast::Value::Date(_)) => PrimitiveKind::DateRange,
                (
                    ast::Value::NumberWithUnit(_, u1),
                    ast::Value::NumberWithUnit(_, u2),
                ) if u1 == u2 && matches!(u1.as_str(), "percent" | "permille") => {
                    PrimitiveKind::RatioRange
                }
                (ast::Value::NumberWithUnit(_, _), ast::Value::NumberWithUnit(_, _)) => {
                    PrimitiveKind::QuantityRange
                }
                (ast::Value::Calendar(_, _), ast::Value::Calendar(_, _)) => {
                    PrimitiveKind::CalendarRange
                }
                _ => unreachable!(
                    "BUG: inferred_parent_type_from_literal called on invalid range literal; planning must validate range endpoint types first"
                ),
            };
            ParentType::Primitive { primitive }
        }
    }
}

impl<'a> TypeResolver<'a> {
    pub fn new(context: &'a Context) -> Self {
        TypeResolver {
            data_types: Vec::new(),
            context,
            all_registered_specs: Vec::new(),
        }
    }

    pub fn is_registered(&self, spec: &Arc<LemmaSpec>) -> bool {
        self.all_registered_specs
            .iter()
            .any(|(_, s)| Arc::ptr_eq(s, spec))
    }

    /// Register all type-declaring data from a spec.
    pub fn register_all(
        &mut self,
        repository: &Arc<LemmaRepository>,
        spec: &Arc<LemmaSpec>,
    ) -> Vec<Error> {
        if !self
            .all_registered_specs
            .iter()
            .any(|(_, s)| Arc::ptr_eq(s, spec))
        {
            self.all_registered_specs
                .push((Arc::clone(repository), Arc::clone(spec)));
        }

        let mut errors = Vec::new();
        for data in &spec.data {
            match &data.value {
                ParsedDataValue::Definition {
                    base,
                    constraints,
                    value,
                } => {
                    if matches!(
                        (base.as_ref(), constraints.as_ref(), value.as_ref()),
                        (None, None, Some(Value::NumberWithUnit(_, _)),)
                    ) {
                        continue;
                    }
                    let name = &data.reference.name;
                    let parent = match (base.as_ref(), value.as_ref()) {
                        (Some(b), _) => b.clone(),
                        (None, Some(v)) => inferred_parent_type_from_literal(v),
                        (None, None) => {
                            errors.push(Error::validation_with_context(
                                format!(
                                    "Data '{name}' in spec '{}' must declare a type or a literal value",
                                    spec.name
                                ),
                                Some(data.source_location.clone()),
                                None::<String>,
                                Some(Arc::clone(spec)),
                                None,
                            ));
                            continue;
                        }
                    };
                    let ftd = DataTypeDef {
                        parent,
                        constraints: constraints.clone(),
                        source: data.source_location.clone(),
                        name: name.clone(),
                        bound_literal: value.clone(),
                    };
                    if let Err(e) = self.register_type(spec, ftd) {
                        errors.push(e);
                    }
                }
                ParsedDataValue::Fill(_) | ParsedDataValue::Import(_) => {}
            }
        }
        errors
    }

    /// Register a type from a data declaration.
    pub fn register_type(&mut self, spec: &Arc<LemmaSpec>, def: DataTypeDef) -> Result<(), Error> {
        let spec_types = if let Some(pos) = self
            .data_types
            .iter()
            .position(|(s, _)| Arc::ptr_eq(s, spec))
        {
            &mut self.data_types[pos].1
        } else {
            self.data_types.push((Arc::clone(spec), HashMap::new()));
            let last = self.data_types.len() - 1;
            &mut self.data_types[last].1
        };
        if spec_types.contains_key(&def.name) {
            return Err(Error::validation_with_context(
                format!(
                    "The name '{}' is already used for data in this spec.",
                    def.name
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
        let mut resolved_types = self.resolve_types_internal(spec, at)?;
        let mut errors = Vec::new();

        // Build the type-name → source map for precise error reporting.
        let type_sources: std::collections::HashMap<String, Source> = resolved_types
            .resolved
            .keys()
            .filter_map(|type_name| {
                self.data_types
                    .iter()
                    .find(|(s, _)| Arc::ptr_eq(s, spec))
                    .and_then(|(_, defs)| defs.get(type_name.as_str()))
                    .map(|ftd| (type_name.clone(), ftd.source.clone()))
            })
            .collect();

        // Run the decomposition pass to populate `BaseQuantityVector` on all Quantity types.
        // The pass also syncs `unit_index` with the post-decomp types as its final phase.
        let decomp_errors = resolve_quantity_decompositions(
            &spec.name,
            &mut resolved_types.resolved,
            &mut resolved_types.unit_index,
            &type_sources,
        );
        errors.extend(decomp_errors);

        for (type_name, lemma_type) in resolved_types.resolved.iter_mut() {
            let source = type_sources.get(type_name).cloned().unwrap_or_else(|| {
                unreachable!(
                    "BUG: resolved type '{}' has no corresponding DataTypeDef in spec '{}'",
                    type_name, spec.name
                )
            });
            if let Err(message) = semantics::finalize_quantity_unit_constraint_magnitudes(
                &mut lemma_type.specifications,
                resolved_types.declared_defaults.get(type_name),
                type_name,
            ) {
                errors.push(Error::validation_with_context(
                    format!(
                        "Type '{}' has invalid quantity unit constraints: {}",
                        type_name, message
                    ),
                    Some(source),
                    None::<String>,
                    Some(Arc::clone(spec)),
                    None,
                ));
            }
        }

        sync_unit_index_from_resolved(&resolved_types.resolved, &mut resolved_types.unit_index);

        for lemma_type in resolved_types.unit_index.values_mut() {
            let type_name = lemma_type
                .name
                .as_deref()
                .or_else(|| lemma_type.quantity_family_name())
                .map(str::to_string);
            let Some(type_name) = type_name else {
                continue;
            };
            if !lemma_type.is_quantity() {
                continue;
            }
            if let Err(message) = semantics::finalize_quantity_unit_constraint_magnitudes(
                &mut lemma_type.specifications,
                resolved_types.declared_defaults.get(type_name.as_str()),
                type_name.as_str(),
            ) {
                let source = type_sources
                    .get(type_name.as_str())
                    .cloned()
                    .or_else(|| {
                        self.data_types.iter().find_map(|(_, defs)| {
                            defs.get(type_name.as_str()).map(|def| def.source.clone())
                        })
                    })
                    .unwrap_or_else(|| {
                        unreachable!(
                            "BUG: quantity type '{}' in unit_index has no DataTypeDef source",
                            type_name
                        )
                    });
                errors.push(Error::validation_with_context(
                    format!(
                        "Type '{}' has invalid quantity unit constraints: {}",
                        type_name, message
                    ),
                    Some(source),
                    None::<String>,
                    Some(Arc::clone(spec)),
                    None,
                ));
            }
        }

        let mut validated_in_unit_index: HashSet<String> = HashSet::new();
        for lemma_type in resolved_types.unit_index.values() {
            let Some(type_name) = lemma_type.name.as_deref() else {
                continue;
            };
            if !lemma_type.is_quantity() || !validated_in_unit_index.insert(type_name.to_string()) {
                continue;
            }
            let source = type_sources
                .get(type_name)
                .cloned()
                .or_else(|| {
                    self.data_types
                        .iter()
                        .find_map(|(_, defs)| defs.get(type_name).map(|def| def.source.clone()))
                })
                .unwrap_or_else(|| {
                    unreachable!(
                        "BUG: quantity type '{}' in unit_index has no DataTypeDef source",
                        type_name
                    )
                });
            errors.extend(validate_type_specifications(
                &lemma_type.specifications,
                resolved_types.declared_defaults.get(type_name),
                type_name,
                &source,
                Some(Arc::clone(spec)),
            ));
        }

        for (type_name, lemma_type) in &resolved_types.resolved {
            let source = type_sources.get(type_name).cloned().unwrap_or_else(|| {
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
        let mut resolved = HashMap::new();
        let mut declared_defaults: HashMap<String, ValueKind> = HashMap::new();
        let mut visited: Vec<(Arc<LemmaSpec>, String)> = Vec::new();

        if let Some((_, spec_types)) = self.data_types.iter().find(|(s, _)| Arc::ptr_eq(s, spec)) {
            for type_name in spec_types.keys() {
                match self.resolve_type_internal(spec, type_name, &mut visited, at) {
                    Ok(Some((resolved_type, declared_default))) => {
                        resolved.insert(type_name.clone(), resolved_type);
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

        for (type_name, resolved_type) in &resolved {
            let data_type_def = self
                .data_types
                .iter()
                .find(|(s, _)| Arc::ptr_eq(s, spec))
                .and_then(|(_, defs)| defs.get(type_name.as_str()))
                .expect("BUG: type was resolved but not in registry");
            let e: Result<(), Error> = if resolved_type.is_quantity() {
                Self::add_quantity_units_to_index(
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

        for data_row in &spec.data {
            let ParsedDataValue::Import(spec_ref) = &data_row.value else {
                continue;
            };
            let (_, imported_spec) =
                match self.resolve_spec_for_import(spec, spec_ref, &data_row.source_location, at) {
                    Ok(x) => x,
                    Err(_) => {
                        // Import validation runs again when the graph resolves `uses`; do not fail
                        // `resolve_and_validate` here so other planning errors (bindings, fills, etc.)
                        // are still collected in the same load.
                        continue;
                    }
                };
            let Some((_, imported_type_map)) = self
                .data_types
                .iter()
                .find(|(s, _)| Arc::ptr_eq(s, &imported_spec))
            else {
                continue;
            };

            let mut import_visited: Vec<(Arc<LemmaSpec>, String)> = Vec::new();
            for (type_name, def) in imported_type_map.iter() {
                if matches!(def.parent, ParentType::Qualified { .. }) {
                    continue;
                }
                match self.resolve_type_internal(
                    &imported_spec,
                    type_name.as_str(),
                    &mut import_visited,
                    at,
                ) {
                    Ok(Some((resolved_type, _))) => {
                        if resolved_type.is_quantity() {
                            if let Err(e) = Self::add_quantity_units_to_index(
                                spec,
                                &mut unit_index_tmp,
                                &resolved_type,
                                def,
                            ) {
                                errors.push(e);
                            }
                        } else if resolved_type.is_ratio() {
                            if let Err(e) = Self::add_ratio_units_to_index(
                                spec,
                                &mut unit_index_tmp,
                                &resolved_type,
                                def,
                            ) {
                                errors.push(e);
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        // Skip merging units for this imported row; type resolution for the
                        // exported spec is validated when that spec is planned.
                    }
                }
                import_visited.clear();
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
            resolved,
            declared_defaults,
            unit_index,
        })
    }

    fn resolve_type_internal(
        &self,
        spec: &Arc<LemmaSpec>,
        name: &str,
        visited: &mut Vec<(Arc<LemmaSpec>, String)>,
        at: &EffectiveDate,
    ) -> Result<Option<(LemmaType, Option<ValueKind>)>, Vec<Error>> {
        if visited
            .iter()
            .any(|(s, n)| Arc::ptr_eq(s, spec) && n == name)
        {
            let source_location = self
                .data_types
                .iter()
                .find(|(s, _)| Arc::ptr_eq(s, spec))
                .and_then(|(_, dt)| dt.get(name))
                .map(|ftd| ftd.source.clone())
                .unwrap_or_else(|| {
                    unreachable!(
                        "BUG: circular dependency detected for type '{}::{}' but type definition not found in registry",
                        spec.name, name
                    )
                });
            return Err(vec![Error::validation_with_context(
                format!(
                    "Circular dependency detected in type resolution: {}::{}",
                    spec.name, name
                ),
                Some(source_location),
                None::<String>,
                Some(Arc::clone(spec)),
                None,
            )]);
        }
        visited.push((Arc::clone(spec), name.to_string()));

        let ftd = match self
            .data_types
            .iter()
            .find(|(s, _)| Arc::ptr_eq(s, spec))
            .and_then(|(_, dt)| dt.get(name))
        {
            Some(def) => def.clone(),
            None => {
                if let Some(pos) = visited
                    .iter()
                    .position(|(s, n)| Arc::ptr_eq(s, spec) && n == name)
                {
                    visited.remove(pos);
                }
                return Ok(None);
            }
        };

        let parent = ftd.parent.clone();
        let constraints = ftd.constraints.clone();

        let (parent_specs, parent_declared_default) = match self.resolve_parent(
            spec,
            &parent,
            visited,
            &ftd.source,
            at,
        ) {
            Ok(Some(pair)) => pair,
            Ok(None) => {
                if let Some(pos) = visited
                    .iter()
                    .position(|(s, n)| Arc::ptr_eq(s, spec) && n == name)
                {
                    visited.remove(pos);
                }
                return Err(vec![Error::validation_with_context(
                        format!("Unknown parent '{}' for data definition. Parent must be defined before use. Valid primitive types are: boolean, quantity, number, ratio, text, date, time, duration, percent", parent),
                        Some(ftd.source.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    )]);
            }
            Err(es) => {
                if let Some(pos) = visited
                    .iter()
                    .position(|(s, n)| Arc::ptr_eq(s, spec) && n == name)
                {
                    visited.remove(pos);
                }
                return Err(es);
            }
        };

        let mut declared_default = parent_declared_default;
        let final_specs = if let Some(constraints) = &constraints {
            let constraint_type_name = constraint_application_type_name(&parent, name);
            match apply_constraints_to_spec(
                spec,
                &constraint_type_name,
                parent_specs,
                constraints,
                &ftd.source,
                &mut declared_default,
            ) {
                Ok(specs) => specs,
                Err(errors) => {
                    if let Some(pos) = visited
                        .iter()
                        .position(|(s, n)| Arc::ptr_eq(s, spec) && n == name)
                    {
                        visited.remove(pos);
                    }
                    return Err(errors);
                }
            }
        } else {
            parent_specs
        };

        if let Some(pos) = visited
            .iter()
            .position(|(s, n)| Arc::ptr_eq(s, spec) && n == name)
        {
            visited.remove(pos);
        }

        let extends = {
            let parent_display = parent.to_string();
            let import_target: Option<Arc<LemmaSpec>> =
                if let ParentType::Qualified { spec_alias, .. } = &parent {
                    let spec_ref = ast::SpecRef::same_repository(spec_alias.clone());
                    match self.resolve_spec_for_import(spec, &spec_ref, &ftd.source, at) {
                        Ok((_, arc)) => Some(arc),
                        Err(e) => return Err(vec![e]),
                    }
                } else {
                    None
                };

            let lookup_for_family: Option<(Arc<LemmaSpec>, String)> = match &parent {
                ParentType::Primitive { .. } => None,
                ParentType::Custom { name } => Some((Arc::clone(spec), name.clone())),
                ParentType::Qualified { inner, .. } => {
                    let target = import_target.as_ref().expect(
                        "BUG: qualified parent missing resolved import target for family lookup",
                    );
                    match inner.as_ref() {
                        ParentType::Custom { name } => Some((Arc::clone(target), name.clone())),
                        ParentType::Primitive { .. } => None,
                        ParentType::Qualified { .. } => {
                            return Err(vec![Error::validation_with_context(
                                "Nested qualified parent types are invalid",
                                Some(ftd.source.clone()),
                                None::<String>,
                                Some(Arc::clone(spec)),
                                None,
                            )]);
                        }
                    }
                }
            };

            let family = match &lookup_for_family {
                None => name.to_string(),
                Some((r, pn)) => match self.resolve_type_internal(r, pn.as_str(), visited, at) {
                    Ok(Some((parent_type, _))) => parent_type
                        .quantity_family_name()
                        .map(String::from)
                        .unwrap_or_else(|| name.to_string()),
                    Ok(None) => name.to_string(),
                    Err(es) => return Err(es),
                },
            };

            let defining_spec = if let Some(ref arc) = import_target {
                TypeDefiningSpec::Import {
                    spec: Arc::clone(arc),
                }
            } else {
                TypeDefiningSpec::Local
            };

            TypeExtends::Custom {
                parent: parent_display,
                family,
                defining_spec,
            }
        };

        let declared_default = match &ftd.bound_literal {
            Some(lit) => match semantics::parser_value_to_value_kind(lit, &final_specs) {
                Ok(vk) => Some(vk),
                Err(message) => {
                    return Err(vec![Error::validation_with_context(
                        message,
                        Some(ftd.source.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    )]);
                }
            },
            None => declared_default,
        };

        Ok(Some((
            LemmaType {
                name: Some(name.to_string()),
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
        visited: &mut Vec<(Arc<LemmaSpec>, String)>,
        source: &crate::parsing::source::Source,
        at: &EffectiveDate,
    ) -> Result<Option<(TypeSpecification, Option<ValueKind>)>, Vec<Error>> {
        match parent {
            ParentType::Primitive { primitive: kind } => {
                Ok(Some((semantics::type_spec_for_primitive(*kind), None)))
            }
            ParentType::Custom { name } => {
                let parent_name = name.as_str();
                let result = self.resolve_type_internal(spec, parent_name, visited, at);
                match result {
                    Ok(Some((t, declared_default))) => {
                        Ok(Some((t.specifications, declared_default)))
                    }
                    Ok(None) => {
                        let type_exists = self
                            .data_types
                            .iter()
                            .find(|(s, _)| Arc::ptr_eq(s, spec))
                            .map(|(_, m)| m.contains_key(parent_name))
                            .unwrap_or(false);

                        if !type_exists {
                            if spec.data.iter().any(|d| {
                                d.reference.is_local()
                                    && d.reference.name == parent_name
                                    && matches!(&d.value, ParsedDataValue::Import(_))
                            }) {
                                return Err(vec![Error::validation_with_context(
                                    format!(
                                        "'{}' names a spec import alias, not a type: use `data x: {}.TypeName` after `uses`",
                                        parent_name, parent_name
                                    ),
                                    Some(source.clone()),
                                    None::<String>,
                                    Some(Arc::clone(spec)),
                                    None,
                                )]);
                            }
                            Err(vec![Error::validation_with_context(
                                format!("Unknown parent '{}' for data definition. Parent must be defined before use. Valid primitive types are: boolean, quantity, number, ratio, text, date, time, duration, percent", parent),
                                Some(source.clone()),
                                None::<String>,
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
            ParentType::Qualified { spec_alias, inner } => {
                let spec_ref = ast::SpecRef::same_repository(spec_alias.clone());
                let (_, target_arc) =
                    match self.resolve_spec_for_import(spec, &spec_ref, source, at) {
                        Ok(x) => x,
                        Err(e) => return Err(vec![e]),
                    };
                match inner.as_ref() {
                    ParentType::Primitive { primitive } => {
                        Ok(Some((semantics::type_spec_for_primitive(*primitive), None)))
                    }
                    ParentType::Custom { name } => {
                        let result =
                            self.resolve_type_internal(&target_arc, name.as_str(), visited, at);
                        match result {
                            Ok(Some((t, declared_default))) => {
                                Ok(Some((t.specifications, declared_default)))
                            }
                            Ok(None) => {
                                let type_exists = self
                                    .data_types
                                    .iter()
                                    .find(|(s, _)| Arc::ptr_eq(s, &target_arc))
                                    .map(|(_, m)| m.contains_key(name.as_str()))
                                    .unwrap_or(false);
                                if !type_exists {
                                    Err(vec![Error::validation_with_context(
                                        format!(
                                            "Type '{}' is not defined in spec '{}' (via import '{}')",
                                            name, target_arc.name, spec_alias
                                        ),
                                        Some(source.clone()),
                                        None::<String>,
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
                    ParentType::Qualified { .. } => Err(vec![Error::validation_with_context(
                        "Nested qualified parent types are invalid",
                        Some(source.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    )]),
                }
            }
        }
    }

    fn resolve_spec_for_import(
        &self,
        spec: &Arc<LemmaSpec>,
        from: &crate::parsing::ast::SpecRef,
        import_site: &crate::parsing::source::Source,
        at: &EffectiveDate,
    ) -> Result<(Arc<LemmaRepository>, Arc<LemmaSpec>), Error> {
        let consumer_repository = self
            .all_registered_specs
            .iter()
            .find(|(_, s)| Arc::ptr_eq(s, spec))
            .map(|(r, _)| Arc::clone(r))
            .unwrap_or_else(|| self.context.workspace());
        discovery::resolve_spec_ref(
            self.context,
            from,
            &consumer_repository,
            spec,
            at,
            Some(import_site.clone()),
        )
    }

    // =========================================================================
    // Static helpers (no &self)
    // =========================================================================

    fn add_quantity_units_to_index(
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

                if existing_type.is_quantity()
                    && (current_extends_existing || existing_extends_current)
                {
                    if current_extends_existing {
                        unit_index.insert(unit, (resolved_type.clone(), Some(defined_by.clone())));
                    }
                    continue;
                }

                if existing_type.same_quantity_family(resolved_type) {
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
                    if existing_def.is_none() {
                        unit_index.insert(
                            unit.clone(),
                            (resolved_type.clone(), Some(defined_by.clone())),
                        );
                        continue;
                    }
                    if existing_type.name() == resolved_type.name() {
                        continue;
                    }
                    let existing_name: String = existing_def
                        .as_ref()
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| existing_type.name());
                    return Err(Error::validation_with_context(
                        format!(
                            "Ambiguous unit '{}'. Defined in multiple ratio types: '{}' and '{}'",
                            unit, existing_name, defined_by.name
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
            TypeSpecification::Quantity { units, .. } => {
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
    use crate::computation::rational::RationalInteger;
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
        let repository = ctx.workspace();
        for s in specs {
            ctx.insert_spec(Arc::clone(&repository), Arc::clone(s))
                .unwrap();
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
        let specs = parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let spec_arcs: Vec<Arc<LemmaSpec>> = specs.iter().map(|s| Arc::new(s.clone())).collect();
        let (ctx, _) = test_context_and_effective(&spec_arcs);
        let repository = ctx.workspace();
        let mut resolver = TypeResolver::new(ctx);
        for spec_arc in &spec_arcs {
            resolver.register_all(&repository, spec_arc);
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
            PrimitiveKind::Quantity,
            PrimitiveKind::QuantityRange,
            PrimitiveKind::Number,
            PrimitiveKind::NumberRange,
            PrimitiveKind::Percent,
            PrimitiveKind::Ratio,
            PrimitiveKind::RatioRange,
            PrimitiveKind::Text,
            PrimitiveKind::Date,
            PrimitiveKind::DateRange,
            PrimitiveKind::Time,
            PrimitiveKind::Calendar,
            PrimitiveKind::CalendarRange,
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
        let mut resolver = TypeResolver::new(ctx);
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
            source: crate::parsing::source::Source::new(
                crate::parsing::source::SourceType::Volatile,
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "age".to_string(),
            bound_literal: None,
        };

        let result = resolver.register_type(&spec_arc, ftd);
        assert!(result.is_ok());
        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        assert!(resolved.resolved.contains_key("age"));
    }

    #[test]
    fn test_register_duplicate_type_fails() {
        let (dag, spec_arc) = dag_and_spec();
        let (ctx, _) = test_context_and_effective(&dag);
        let mut resolver = TypeResolver::new(ctx);
        let ftd = DataTypeDef {
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: None,
            source: crate::parsing::source::Source::new(
                crate::parsing::source::SourceType::Volatile,
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "money".to_string(),
            bound_literal: None,
        };
        resolver.register_type(&spec_arc, ftd.clone()).unwrap();
        let result = resolver.register_type(&spec_arc, ftd);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_custom_type_from_primitive() {
        let (dag, spec_arc) = dag_and_spec();
        let (ctx, _) = test_context_and_effective(&dag);
        let mut resolver = TypeResolver::new(ctx);
        let ftd = DataTypeDef {
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: None,
            source: crate::parsing::source::Source::new(
                crate::parsing::source::SourceType::Volatile,
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "money".to_string(),
            bound_literal: None,
        };

        resolver.register_type(&spec_arc, ftd).unwrap();
        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();

        assert!(resolved.resolved.contains_key("money"));
        let money_type = resolved.resolved.get("money").unwrap();
        assert_eq!(money_type.name, Some("money".to_string()));
    }

    #[test]
    fn test_child_quantity_type_keeps_declared_name_and_child_units() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data length: quantity
  -> unit meter 1
data road_length: length
  -> unit kilometer 1000"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();

        let road_length_type = resolved_types.resolved.get("road_length").unwrap();
        assert_eq!(road_length_type.name.as_deref(), Some("road_length"));

        match &road_length_type.specifications {
            TypeSpecification::Quantity { units, .. } => {
                assert!(units.iter().any(|unit| unit.name == "kilometer"));
            }
            _ => panic!("Expected Quantity type specifications"),
        }

        let kilometer_owner = resolved_types.unit_index.get("kilometer").unwrap();
        assert_eq!(kilometer_owner.name.as_deref(), Some("road_length"));
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
        let dice_type = resolved_types.resolved.get("dice").unwrap();

        match &dice_type.specifications {
            TypeSpecification::Number {
                minimum, maximum, ..
            } => {
                assert_eq!(*minimum, Some(RationalInteger::new(0, 1)));
                assert_eq!(*maximum, Some(RationalInteger::new(6, 1)));
            }
            _ => panic!("Expected Number type specifications"),
        }
    }

    #[test]
    fn test_type_definition_with_multiple_commands() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: quantity -> decimals 2 -> unit eur 1.0 -> unit usd 1.18"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let money_type = resolved_types.resolved.get("money").unwrap();

        match &money_type.specifications {
            TypeSpecification::Quantity {
                decimals, units, ..
            } => {
                assert_eq!(*decimals, Some(2));
                assert_eq!(units.len(), 2);
                assert!(units.iter().any(|u| u.name == "eur"));
                assert!(units.iter().any(|u| u.name == "usd"));
            }
            _ => panic!("Expected Quantity type specifications"),
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
        let price_type = resolved_types.resolved.get("price").unwrap();

        match &price_type.specifications {
            TypeSpecification::Number {
                decimals, minimum, ..
            } => {
                assert_eq!(*decimals, Some(2));
                assert_eq!(*minimum, Some(RationalInteger::new(0, 1)));
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
        let precise_type = resolved_types.resolved.get("precise_number").unwrap();

        match &precise_type.specifications {
            TypeSpecification::Number { decimals, .. } => {
                assert_eq!(*decimals, Some(4));
            }
            _ => panic!("Expected Number type with decimals 4"),
        }
    }

    #[test]
    fn test_quantity_type_decimals_only() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data weight: quantity -> unit kg 1 -> decimals 3"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let weight_type = resolved_types.resolved.get("weight").unwrap();

        match &weight_type.specifications {
            TypeSpecification::Quantity { decimals, .. } => {
                assert_eq!(*decimals, Some(3));
            }
            _ => panic!("Expected Quantity type with decimals 3"),
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
        let ratio_type = resolved_types.resolved.get("ratio_type").unwrap();

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
data percentage: ratio -> minimum 0% -> maximum 100% -> default 50%"#,
        );

        let resolved_types = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let percentage_type = resolved_types.resolved.get("percentage").unwrap();

        match &percentage_type.specifications {
            TypeSpecification::Ratio {
                minimum, maximum, ..
            } => {
                assert_eq!(
                    *minimum,
                    Some(RationalInteger::new(0, 1)),
                    "ratio type should have minimum 0"
                );
                assert_eq!(
                    *maximum,
                    Some(RationalInteger::new(1, 1)),
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
            ValueKind::Ratio(v, unit) => {
                assert_eq!(*v, RationalInteger::new(1, 2));
                assert_eq!(unit.as_deref(), Some("percent"));
            }
            other => panic!("expected Ratio declared default, got {:?}", other),
        }
    }

    #[test]
    fn test_quantity_extension_chain_same_family_units_allowed() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: quantity -> unit eur 1
data money2: money -> unit usd 1.24"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(
            result.is_ok(),
            "Quantity extension chain should resolve: {:?}",
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
            Some("money2"),
            "more derived type (money2) should own inherited eur"
        );
        assert_eq!(
            usd_type.name.as_deref(),
            Some("money2"),
            "usd defined on money2 should be owned by money2"
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
            error_msg.contains("Unknown parent") && error_msg.contains("nonexistent_type"),
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
            error_msg.contains("Unknown parent") && error_msg.contains("choice"),
            "Error should mention unknown type 'choice'. Got: {}",
            error_msg
        );
    }

    #[test]
    fn test_quantity_extension_overwrites_parent_units() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: quantity
  -> unit eur 1.00
  -> unit usd 0.84

data money2: money
  -> unit eur 1.20
  -> unit usd 1.21
  -> unit gbp 1.30"#,
        );

        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let money2 = resolved.resolved.get("money2").unwrap();
        match &money2.specifications {
            TypeSpecification::Quantity { units, .. } => {
                assert_eq!(units.len(), 3);
                let eur = units.iter().find(|u| u.name == "eur").unwrap();
                let usd = units.iter().find(|u| u.name == "usd").unwrap();
                let gbp = units.iter().find(|u| u.name == "gbp").unwrap();
                assert_eq!(
                    crate::commit_rational_to_decimal(&eur.factor).unwrap(),
                    Decimal::from_str_exact("1.20").unwrap()
                );
                assert_eq!(
                    crate::commit_rational_to_decimal(&usd.factor).unwrap(),
                    Decimal::from_str_exact("1.21").unwrap()
                );
                assert_eq!(
                    crate::commit_rational_to_decimal(&gbp.factor).unwrap(),
                    Decimal::from_str_exact("1.30").unwrap()
                );
            }
            other => panic!("Expected Quantity type specifications, got {:?}", other),
        }
    }

    #[test]
    fn test_spec_level_unit_ambiguity_errors_are_reported() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money_a: quantity
  -> unit eur 1.00
  -> unit usd 0.84

data money_b: quantity
  -> unit eur 1.00
  -> unit usd 1.20

data length_a: quantity
  -> unit meter 1.0

data length_b: quantity
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
    fn test_ratio_unit_cross_family_collision_errors() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data q: quantity
  -> unit foo 1

data r: ratio
  -> unit foo 100"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(
            result.is_err(),
            "quantity and ratio must not share a unit name"
        );
        let error_msg = result
            .unwrap_err()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            error_msg.contains("foo"),
            "expected cross-family collision on 'foo', got: {}",
            error_msg
        );
    }

    #[test]
    fn test_duplicate_ratio_unit_across_unrelated_types_errors() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data spread_a: ratio
  -> unit basis_points 10000

data spread_b: ratio
  -> unit basis_points 10000"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, &EffectiveDate::Origin);
        assert!(
            result.is_err(),
            "unrelated ratio types must not define the same unit name"
        );
        let error_msg = result
            .unwrap_err()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            error_msg.contains("spread_a") && error_msg.contains("spread_b"),
            "expected duplicate ratio unit between types, got: {}",
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
data money: quantity
  -> unit eur 1.00
  -> unit usd 0.84

data my_money: money
  -> unit gbp 1.30"#,
        );

        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let my_money_type = resolved.resolved.get("my_money").unwrap();

        match &my_money_type.specifications {
            TypeSpecification::Quantity { units, .. } => {
                assert_eq!(units.len(), 3);
                assert!(units.iter().any(|u| u.name == "eur"));
                assert!(units.iter().any(|u| u.name == "usd"));
                assert!(units.iter().any(|u| u.name == "gbp"));
            }
            other => panic!("Expected Quantity type specifications, got {:?}", other),
        }
    }

    #[test]
    fn test_value_copy_quantity_binding_overwrites_unit_factor() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data source_quantity: quantity
  -> unit usd 1.00

data z: source_quantity
  -> unit usd 0.84"#,
        );

        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let z = resolved.resolved.get("z").unwrap();
        match &z.specifications {
            TypeSpecification::Quantity { units, .. } => {
                assert_eq!(units.len(), 1);
                let usd = units.iter().find(|u| u.name == "usd").unwrap();
                assert_eq!(
                    crate::commit_rational_to_decimal(&usd.factor).unwrap(),
                    Decimal::from_str_exact("0.84").unwrap()
                );
            }
            other => panic!("Expected Quantity type specifications, got {:?}", other),
        }
    }

    #[test]
    fn test_duplicate_unit_in_same_type_last_wins() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: quantity
  -> unit eur 1.00
  -> unit eur 1.19"#,
        );

        let resolved = resolver
            .resolve_types_internal(&spec_arc, &EffectiveDate::Origin)
            .unwrap();
        let money = resolved.resolved.get("money").unwrap();
        match &money.specifications {
            TypeSpecification::Quantity { units, .. } => {
                assert_eq!(units.len(), 1);
                let eur = units.iter().find(|u| u.name == "eur").unwrap();
                assert_eq!(
                    crate::commit_rational_to_decimal(&eur.factor).unwrap(),
                    Decimal::from_str_exact("1.19").unwrap()
                );
            }
            other => panic!("Expected Quantity type specifications, got {:?}", other),
        }
    }
}

// ============================================================================
// Validation (formerly validation.rs)
// ============================================================================

/// Validate that TypeSpecification constraints are internally consistent.
///
/// Checks range, decimals, length, unit, and option constraints, and
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
        TypeSpecification::Quantity {
            minimum,
            maximum,
            decimals,
            units,
            ..
        } => {
            // Validate range consistency
            if let (Some(min), Some(max)) = (minimum, maximum) {
                match (
                    semantics::quantity_declared_bound_canonical(min, units, type_name, "minimum"),
                    semantics::quantity_declared_bound_canonical(max, units, type_name, "maximum"),
                ) {
                    (Ok(min_canonical), Ok(max_canonical)) => {
                        if min_canonical > max_canonical {
                            errors.push(Error::validation_with_context(
                                format!(
                                    "Type '{}' has invalid range: minimum {} {} is greater than maximum {} {}",
                                    type_name,
                                    min.0,
                                    min.1,
                                    max.0,
                                    max.1
                                ),
                                Some(source.clone()),
                                None::<String>,
                                spec_context.clone(),
                                None,
                            ));
                        }
                    }
                    (Err(message), _) | (_, Err(message)) => {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' has invalid quantity bound: {}",
                                type_name, message
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }

            if minimum.is_some() {
                for unit in units.iter() {
                    if unit.minimum.is_none() {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' has minimum bound but unit '{}' is missing per-unit minimum after planning",
                                type_name, unit.name
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
            if maximum.is_some() {
                for unit in units.iter() {
                    if unit.maximum.is_none() {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' has maximum bound but unit '{}' is missing per-unit maximum after planning",
                                type_name, unit.name
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
                }
            }
            if declared_default.is_some() {
                for unit in units.iter() {
                    if unit.default_magnitude.is_none() {
                        errors.push(Error::validation_with_context(
                            format!(
                                "Type '{}' has default but unit '{}' is missing per-unit default after planning",
                                type_name, unit.name
                            ),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    }
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

            if let Some(ValueKind::Quantity(_def_value, def_unit, _def_decomp)) = declared_default {
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
            }

            // Quantity types must have at least one unit (required for parsing and conversion)
            if units.is_empty() {
                errors.push(Error::validation_with_context(
                    format!(
                        "Type '{}' is a quantity type but has no units. Quantity types must define at least one unit (e.g. -> unit eur 1).",
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

                    if !unit.is_positive_factor() {
                        let factor = unit.factor.reduced();
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has unit '{}' with invalid value {}/{}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, factor.numer(), factor.denom()),
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

                    if *unit.value.numer() <= 0 {
                        let factor = unit.value.reduced();
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has unit '{}' with invalid value {}/{}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, factor.numer(), factor.denom()),
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

        TypeSpecification::NumberRange { .. }
        | TypeSpecification::DateRange { .. }
        | TypeSpecification::QuantityRange { .. }
        | TypeSpecification::RatioRange { .. }
        | TypeSpecification::CalendarRange { .. }
        | TypeSpecification::Boolean { .. }
        | TypeSpecification::Calendar { .. } => {
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

#[cfg(test)]
mod validation_tests {
    use super::*;
    use crate::computation::rational::RationalInteger;
    use crate::parsing::ast::{CommandArg, TypeConstraintCommand};
    use crate::planning::semantics::TypeSpecification;
    use rust_decimal::Decimal;

    fn test_source() -> Source {
        Source::new(
            crate::parsing::source::SourceType::Volatile,
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
        specs
            .apply_constraint("test", command, args, &mut default)
            .unwrap()
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
            minimum: Some(RationalInteger::new(10, 1)),
            maximum: None,
            decimals: None,
            help: String::new(),
        };
        let default = ValueKind::Number(RationalInteger::new(5, 1));

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
            maximum: Some(RationalInteger::new(100, 1)),
            decimals: None,
            help: String::new(),
        };
        let default = ValueKind::Number(RationalInteger::new(150, 1));

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
            minimum: Some(RationalInteger::new(0, 1)),
            maximum: Some(RationalInteger::new(100, 1)),
            decimals: None,
            help: String::new(),
        };
        let default = ValueKind::Number(RationalInteger::new(50, 1));

        let src = test_source();
        let errors = validate_type_specifications(&specs, Some(&default), "test", &src, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn text_minimum_command_is_rejected() {
        let specs = TypeSpecification::text();
        let res = specs.apply_constraint(
            "test",
            TypeConstraintCommand::Minimum,
            &[number_arg(5)],
            &mut None,
        );
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("Invalid command 'minimum' for text type"));
    }

    #[test]
    fn text_maximum_command_is_rejected() {
        let specs = TypeSpecification::text();
        let res = specs.apply_constraint(
            "test",
            TypeConstraintCommand::Maximum,
            &[number_arg(5)],
            &mut None,
        );
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
            minimum: Some(RationalInteger::new(2, 1)),
            maximum: Some(RationalInteger::new(1, 1)),
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
