use crate::engine::Context;
use crate::literals::{QuantityUnit, QuantityUnits};
use crate::parsing::ast::{
    self as ast, CommandArg, Constraint, EffectiveDate, LemmaData, LemmaRepository, LemmaRule,
    LemmaSpec, MetaValue, ParentType, PrimitiveKind, TypeConstraintCommand, Value, WithRhs,
};
use crate::parsing::source::Source;
use crate::planning::discovery;
use crate::planning::semantics::{
    self, calendar_decomposition, canonicalize_signature, combine_decompositions,
    conversion_target_to_semantic, duration_decomposition, materialize_raw_default,
    number_with_unit_to_value_kind, parser_value_to_value_kind, primitive_boolean_arc,
    primitive_date_arc, primitive_date_range_arc, primitive_number_arc, primitive_text_arc,
    primitive_time_arc, range_type_specification_from_endpoints, value_kind_matches_spec,
    value_to_semantic, ArithmeticComputation, BaseQuantityVector, ComparisonComputation,
    DataDefinition, DataPath, Expression, ExpressionKind, LemmaType, LiteralValue, PathSegment,
    RawDefault, ReferenceTarget, RulePath, SemanticConversionTarget, TypeDefiningSpec, TypeExtends,
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
    pub(crate) fn build_data(
        &self,
        resolved_by_type_name: &HashMap<String, Arc<LemmaType>>,
    ) -> IndexMap<DataPath, DataDefinition> {
        struct PendingReference {
            target: ReferenceTarget,
            resolved_type: Arc<LemmaType>,
            local_constraints: Option<Vec<Constraint>>,
            local_default: Option<ValueKind>,
        }

        let mut schema: HashMap<DataPath, Arc<LemmaType>> = HashMap::new();
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
                    schema.insert(path.clone(), Arc::clone(resolved_type));
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
                    schema.insert(path.clone(), Arc::clone(resolved_type));
                    references.insert(
                        path.clone(),
                        PendingReference {
                            target: target.clone(),
                            resolved_type: Arc::clone(resolved_type),
                            local_constraints: local_constraints.clone(),
                            local_default: local_default.clone(),
                        },
                    );
                }
            }
        }

        for (path, value) in values.iter_mut() {
            if let Some(type_name) = value.lemma_type.name.as_deref() {
                if let Some(resolved) = resolved_by_type_name.get(type_name) {
                    semantics::refresh_quantity_literal_canonical_magnitude(
                        value,
                        resolved.as_ref(),
                    );
                }
            }
            let Some(schema_type) = schema.get(path) else {
                continue;
            };
            match Self::coerce_literal_to_schema_type(value, schema_type) {
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
        schema_type: &Arc<LemmaType>,
    ) -> Result<LiteralValue, String> {
        fn range_endpoint_schema_type(schema_type: &LemmaType) -> Option<Arc<LemmaType>> {
            match &schema_type.specifications {
                TypeSpecification::NumberRange { .. } => {
                    Some(semantics::primitive_number_arc().clone())
                }
                TypeSpecification::DateRange { .. } => {
                    Some(semantics::primitive_date_arc().clone())
                }
                TypeSpecification::TimeRange { .. } => {
                    Some(semantics::primitive_time_arc().clone())
                }
                TypeSpecification::RatioRange { units, .. } => {
                    Some(Arc::new(LemmaType::primitive(TypeSpecification::Ratio {
                        minimum: None,
                        maximum: None,
                        decimals: None,
                        units: units.clone(),
                        help: String::new(),
                    })))
                }
                TypeSpecification::QuantityRange {
                    units,
                    decomposition,
                    ..
                } => Some(Arc::new(LemmaType::primitive(
                    TypeSpecification::Quantity {
                        minimum: None,
                        maximum: None,
                        decimals: None,
                        units: units.clone(),
                        traits: Vec::new(),
                        decomposition: decomposition.clone(),
                        help: String::new(),
                    },
                ))),
                _ => None,
            }
        }

        let schema_ref = schema_type.as_ref();
        if lit.lemma_type.specifications == schema_ref.specifications {
            if !value_kind_matches_spec(&lit.value, &schema_ref.specifications) {
                panic!(
                    "BUG: LiteralValue value kind {:?} inconsistent with lemma_type {:?}",
                    lit.value, lit.lemma_type.specifications
                );
            }
            if let ValueKind::Quantity(_, signature) = &lit.value {
                let unit_name = signature
                    .first()
                    .map(|(name, _)| name.as_str())
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        format!(
                            "value {} cannot be used as type {}: quantity literal has empty unit name",
                            lit,
                            schema_ref.name()
                        )
                    })?;
                if let TypeSpecification::Quantity { units, .. } = &schema_ref.specifications {
                    if !units.iter().any(|u| u.name == unit_name) {
                        return Err(format!(
                            "value {} cannot be used as type {}: unknown unit '{}'",
                            lit,
                            schema_ref.name(),
                            unit_name
                        ));
                    }
                }
            }
            let mut out = lit.clone();
            out.lemma_type = Arc::clone(schema_type);
            return Ok(out);
        }
        match (&schema_ref.specifications, &lit.value) {
            (TypeSpecification::Number { .. }, ValueKind::Number(_))
            | (TypeSpecification::Text { .. }, ValueKind::Text(_))
            | (TypeSpecification::Boolean { .. }, ValueKind::Boolean(_))
            | (TypeSpecification::Date { .. }, ValueKind::Date(_))
            | (TypeSpecification::Time { .. }, ValueKind::Time(_)) => {
                let mut out = lit.clone();
                out.lemma_type = Arc::clone(schema_type);
                Ok(out)
            }
            (TypeSpecification::Quantity { units, .. }, ValueKind::Quantity(_, signature)) => {
                let unit_name = signature
                    .first()
                    .map(|(name, _)| name.as_str())
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        format!(
                            "value {} cannot be used as type {}: quantity literal has empty unit name",
                            lit,
                            schema_ref.name()
                        )
                    })?;
                if !units.iter().any(|u| u.name == unit_name) {
                    return Err(format!(
                        "value {} cannot be used as type {}: unknown unit '{}'",
                        lit,
                        schema_ref.name(),
                        unit_name
                    ));
                }
                let mut out = lit.clone();
                out.lemma_type = Arc::clone(schema_type);
                Ok(out)
            }
            (TypeSpecification::Ratio { units, .. }, ValueKind::Ratio(_, unit_name)) => {
                if let Some(unit_name) = unit_name {
                    if !units.iter().any(|u| u.name == *unit_name) {
                        return Err(format!(
                            "value {} cannot be used as type {}: unknown unit '{}'",
                            lit,
                            schema_ref.name(),
                            unit_name
                        ));
                    }
                }
                let mut out = lit.clone();
                out.lemma_type = Arc::clone(schema_type);
                Ok(out)
            }
            (
                TypeSpecification::NumberRange { .. }
                | TypeSpecification::DateRange { .. }
                | TypeSpecification::TimeRange { .. }
                | TypeSpecification::RatioRange { .. }
                | TypeSpecification::QuantityRange { .. },
                ValueKind::Range(left, right),
            ) => {
                let endpoint_schema_type =
                    range_endpoint_schema_type(schema_ref).unwrap_or_else(|| {
                        unreachable!("BUG: range_endpoint_schema_type missing range schema arm")
                    });
                let coerced_left =
                    Self::coerce_literal_to_schema_type(left.as_ref(), &endpoint_schema_type)?;
                let coerced_right =
                    Self::coerce_literal_to_schema_type(right.as_ref(), &endpoint_schema_type)?;
                Ok(LiteralValue {
                    value: ValueKind::Range(Box::new(coerced_left), Box::new(coerced_right)),
                    lemma_type: Arc::clone(schema_type),
                })
            }
            (TypeSpecification::Ratio { .. }, ValueKind::Number(n)) => Ok(
                LiteralValue::ratio_with_type(n.clone(), None, Arc::clone(schema_type)),
            ),
            _ => Err(format!(
                "value {} cannot be used as type {}",
                lit,
                schema_ref.name()
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
    /// `ordered_reference_paths` is the dependency order from
    /// [`Self::compute_reference_evaluation_order`] (targets before
    /// dependents). Each resolution is applied immediately so a reference
    /// whose target is itself a reference reads the target's final resolved
    /// type — batched application would leave chains `Undetermined`.
    ///
    /// Rule-target references are not in the order — they are resolved later
    /// in [`Self::resolve_rule_reference_types`] using the inferred rule
    /// type, which is only available after [`infer_rule_types`] has run.
    fn resolve_data_reference_types(
        &mut self,
        ordered_reference_paths: &[DataPath],
    ) -> Result<(), Vec<Error>> {
        let mut errors: Vec<Error> = Vec::new();

        for reference_path in ordered_reference_paths {
            let (target_data_path, provisional, local_constraints, source) =
                match self.data.get(reference_path) {
                    Some(DataDefinition::Reference {
                        target: ReferenceTarget::Data(path),
                        resolved_type,
                        local_constraints,
                        source,
                        ..
                    }) => (
                        path.clone(),
                        Arc::clone(resolved_type),
                        local_constraints.clone(),
                        source.clone(),
                    ),
                    _ => unreachable!(
                        "BUG: reference evaluation order must contain only data-target references"
                    ),
                };

            let Some(target_entry) = self.data.get(&target_data_path) else {
                errors.push(reference_error(
                    &self.main_spec,
                    &source,
                    format!(
                        "Data reference '{}' target '{}' does not exist",
                        reference_path, target_data_path
                    ),
                ));
                continue;
            };

            let target_type_arc = match target_entry {
                DataDefinition::TypeDeclaration { resolved_type, .. }
                | DataDefinition::Reference { resolved_type, .. } => Arc::clone(resolved_type),
                DataDefinition::Value { value, .. } => Arc::clone(&value.lemma_type),
                DataDefinition::Import { .. } => {
                    errors.push(reference_error(
                        &self.main_spec,
                        &source,
                        format!(
                            "Data reference '{}' target '{}' is a spec reference and cannot carry a value",
                            reference_path, target_data_path
                        ),
                    ));
                    continue;
                }
            };

            let lhs_declared_type: Option<&LemmaType> = if provisional.is_undetermined() {
                None
            } else {
                Some(provisional.as_ref())
            };

            if let Some(lhs) = lhs_declared_type {
                if let Some(msg) = reference_kind_mismatch_message(
                    lhs,
                    target_type_arc.as_ref(),
                    reference_path,
                    &target_data_path,
                    "target",
                ) {
                    errors.push(reference_error(&self.main_spec, &source, msg));
                    continue;
                }
            }

            // Merge: prefer LHS-declared spec when present so child-declared
            // constraints (e.g. `maximum 5` from a binding's parent type
            // chain) are enforced on the copied value at run time. Without
            // a LHS-declared type, fall back to the target's spec.
            let mut merged = match lhs_declared_type {
                Some(lhs) => lhs.clone(),
                None => target_type_arc.as_ref().clone(),
            };
            let mut raw_default: Option<RawDefault> = None;
            if let Some(constraints) = &local_constraints {
                let constraint_type_name = merged.name();
                match apply_constraints_to_spec(
                    &self.main_spec,
                    &constraint_type_name,
                    merged.specifications.clone(),
                    constraints,
                    &source,
                    &mut raw_default,
                ) {
                    Ok(specs) => merged.specifications = specs,
                    Err(errs) => {
                        errors.extend(errs);
                        continue;
                    }
                }
            }
            let captured_default = match raw_default {
                None => None,
                Some(raw) => {
                    match materialize_raw_default(raw, &merged.specifications, &merged.name()) {
                        Ok(vk) => Some(vk),
                        Err(message) => {
                            errors.push(reference_error(&self.main_spec, &source, message));
                            continue;
                        }
                    }
                }
            };

            // Apply immediately: later references in the order may target
            // this one and must read the final merged type.
            if let Some(DataDefinition::Reference {
                resolved_type,
                local_default,
                ..
            }) = self.data.get_mut(reference_path)
            {
                *resolved_type = Arc::new(merged);
                if captured_default.is_some() {
                    *local_default = captured_default;
                }
            } else {
                unreachable!("BUG: reference path disappeared during type resolution");
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
        computed_rule_types: &HashMap<RulePath, Arc<LemmaType>>,
    ) -> Result<(), Vec<Error>> {
        let mut errors: Vec<Error> = Vec::new();
        let mut updates: Vec<(DataPath, Arc<LemmaType>, Option<ValueKind>)> = Vec::new();

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
                let mut merged = target_type.as_ref().clone();
                let mut raw_default: Option<RawDefault> = None;
                if let Some(constraints) = local_constraints {
                    let constraint_type_name = merged.name();
                    match apply_constraints_to_spec(
                        &self.main_spec,
                        &constraint_type_name,
                        merged.specifications.clone(),
                        constraints,
                        source,
                        &mut raw_default,
                    ) {
                        Ok(specs) => merged.specifications = specs,
                        Err(errs) => {
                            errors.extend(errs);
                            continue;
                        }
                    }
                }
                let captured_default = match raw_default {
                    None => None,
                    Some(raw) => {
                        match materialize_raw_default(raw, &merged.specifications, &merged.name()) {
                            Ok(vk) => Some(vk),
                            Err(message) => {
                                errors.push(reference_error(&self.main_spec, source, message));
                                continue;
                            }
                        }
                    }
                };
                updates.push((reference_path.clone(), Arc::new(merged), captured_default));
                continue;
            }

            let lhs_declared_type: Option<&LemmaType> = if provisional.is_undetermined() {
                None
            } else {
                Some(provisional.as_ref())
            };

            if let Some(lhs) = lhs_declared_type {
                if let Some(msg) = reference_kind_mismatch_message(
                    lhs,
                    target_type.as_ref(),
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
                None => target_type.as_ref().clone(),
            };
            let mut raw_default: Option<RawDefault> = None;
            if let Some(constraints) = local_constraints {
                let constraint_type_name = merged.name();
                match apply_constraints_to_spec(
                    &self.main_spec,
                    &constraint_type_name,
                    merged.specifications.clone(),
                    constraints,
                    source,
                    &mut raw_default,
                ) {
                    Ok(specs) => merged.specifications = specs,
                    Err(errs) => {
                        errors.extend(errs);
                        continue;
                    }
                }
            }
            let captured_default = match raw_default {
                None => None,
                Some(raw) => {
                    match materialize_raw_default(raw, &merged.specifications, &merged.name()) {
                        Ok(vk) => Some(vk),
                        Err(message) => {
                            errors.push(reference_error(&self.main_spec, source, message));
                            continue;
                        }
                    }
                }
            };

            updates.push((reference_path.clone(), Arc::new(merged), captured_default));
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
    pub rule_type: Arc<LemmaType>,

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
    /// Degraded planning: dependency discovery failed and the DAG was
    /// reduced to the root spec, so unregistered nested specs are expected
    /// (their discovery errors were already reported).
    dependency_discovery_failed: bool,
}

struct RuleExpressionConversion<'a> {
    spec: &'a Arc<LemmaSpec>,
    data_map: &'a HashMap<String, &'a LemmaData>,
    segments: &'a [PathSegment],
    rule_names: &'a HashSet<&'a str>,
    effective: &'a EffectiveDate,
    depends_on_rules: &'a mut BTreeSet<RulePath>,
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
        ParentType::Ranged { inner, .. } => constraint_application_type_name(inner, data_name),
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
    declared_default: &mut Option<RawDefault>,
) -> Result<TypeSpecification, Vec<Error>> {
    let mut errors = Vec::new();

    let mut seen_unit_names: std::collections::HashSet<String> = Default::default();
    for (command, args) in constraints.iter() {
        if *command == TypeConstraintCommand::Unit {
            if let Some(CommandArg::Label(name)) = args.first() {
                if !seen_unit_names.insert(name.clone()) {
                    errors.push(Error::validation_with_context(
                        format!(
                            "Duplicate unit '{}': each unit name may appear at most once per type definition",
                            name
                        ),
                        Some(source.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    ));
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut apply_one = |specs: TypeSpecification,
                         command: TypeConstraintCommand,
                         args: &[CommandArg],
                         declared_default: &mut Option<RawDefault>|
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
    ///
    /// `dependency_discovery_failed` marks degraded planning: discovery
    /// already reported errors and the DAG was reduced to the root spec, so
    /// nested specs are expected to be unregistered and their subtrees are
    /// skipped quietly. Outside degraded planning, reaching an unregistered
    /// spec is an engine bug and crashes.
    pub(crate) fn build(
        context: &Context,
        repository: &Arc<LemmaRepository>,
        main_spec: &Arc<LemmaSpec>,
        dag: &[(Arc<LemmaRepository>, Arc<LemmaSpec>)],
        effective: &EffectiveDate,
        dependency_discovery_failed: bool,
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
                dependency_discovery_failed,
            };

            builder.build_spec(
                main_spec,
                repository,
                Vec::new(),
                HashMap::new(),
                effective,
                &type_resolver,
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

        // Compute the data-target reference evaluation (copy) order first: it
        // is both the type-resolution order for Phase 1 (targets before
        // dependents, so reference→reference chains resolve) and the runtime
        // prepop copy order. Rule-target references are resolved lazily at
        // evaluation time — they do not participate in either.
        let reference_order = match self.compute_reference_evaluation_order() {
            Ok(order) => order,
            Err(circular_errors) => {
                errors.extend(circular_errors);
                return Err(errors);
            }
        };

        // Phase 1: Resolve data-target reference types now that all data
        // definitions (across all specs) are populated. Rule-target references
        // are resolved in Phase 4 once the target rule's type is inferred.
        if let Err(reference_errors) = self.resolve_data_reference_types(&reference_order) {
            errors.extend(reference_errors);
        }

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
            ParsedDataValue::With(WithRhs::Literal(v)) => BindingValue::Literal(v.clone()),
            ParsedDataValue::With(WithRhs::Reference { target }) => {
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
            if data.reference.segments.is_empty() {
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
        let binding_override: Option<(BindingValue, Source)> =
            data_bindings.get(&binding_key).map(|(v, s)| {
                used_binding_keys.insert(binding_key.clone());
                (v.clone(), s.clone())
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
            let lemma_type = Arc::clone(
                resolved
                    .resolved
                    .get(&data.reference.name)
                    .expect(
                        "BUG: type not in ResolvedSpecTypes.resolved. TypeResolver should have registered it",
                    ),
            );
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
                        ..
                    } if units.0.is_empty() && decomposition.is_none()
                );
                let is_calendar_quantity = matches!(
                    &resolved_type.specifications,
                    TypeSpecification::Quantity { traits, .. }
                        if traits.contains(&semantics::QuantityTrait::Calendar)
                );

                if is_generic_quantity_range || is_calendar_quantity {
                    if let Some(ValueKind::Range(left, right)) = &declared_default {
                        if let (
                            ValueKind::Quantity(_, left_sig),
                            ValueKind::Quantity(_, right_sig),
                        ) = (&left.value, &right.value)
                        {
                            let left_unit = left_sig.first().map(|(n, _)| n.as_str()).unwrap_or("");
                            let right_unit =
                                right_sig.first().map(|(n, _)| n.as_str()).unwrap_or("");
                            let resolved = self
                                .local_types
                                .iter()
                                .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                                .map(|(_, _, t)| t)
                                .expect("BUG: no resolved types for spec during add_local_data");

                            let left_quantity_type = resolved.unit_index.get(left_unit).cloned();
                            let right_quantity_type = resolved.unit_index.get(right_unit).cloned();

                            match (&left_quantity_type, &right_quantity_type) {
                                (Some(left_quantity_type), Some(right_quantity_type))
                                    if left_quantity_type
                                        .as_ref()
                                        .same_quantity_family(right_quantity_type.as_ref()) =>
                                {
                                    let specialized_range_type =
                                        infer_range_type_from_endpoint_types(
                                            left_quantity_type.as_ref(),
                                            right_quantity_type.as_ref(),
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
                                            lemma_type: Arc::clone(&specialized_range_type),
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
            ParsedDataValue::With(_) => {
                unreachable!(
                    "BUG: local with is rejected at parse; binding with rows must not reach add_data"
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
        declared_schema_type: Option<Arc<LemmaType>>,
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
                        .map(Arc::as_ref)
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
                Value::Range(left, right) => match (left.as_ref(), right.as_ref()) {
                    (
                        Value::NumberWithUnit(left_mag, unit),
                        Value::NumberWithUnit(right_mag, right_unit),
                    ) if unit == right_unit => {
                        let Some(lt) = self
                            .local_types
                            .iter()
                            .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                            .map(|(_, _, t)| t)
                            .and_then(|dt| dt.unit_index.get(unit))
                            .map(Arc::clone)
                        else {
                            self.errors.push(self.engine_error(
                                format!("Unit '{}' is not in scope for this spec", unit),
                                &effective_source,
                            ));
                            return;
                        };
                        let left_kind =
                            match number_with_unit_to_value_kind(*left_mag, unit, lt.as_ref()) {
                                Ok(v) => v,
                                Err(e) => {
                                    self.errors.push(self.engine_error(e, &effective_source));
                                    return;
                                }
                            };
                        let right_kind =
                            match number_with_unit_to_value_kind(*right_mag, unit, lt.as_ref()) {
                                Ok(v) => v,
                                Err(e) => {
                                    self.errors.push(self.engine_error(e, &effective_source));
                                    return;
                                }
                            };
                        ValueKind::Range(
                            Box::new(LiteralValue {
                                value: left_kind,
                                lemma_type: Arc::clone(&lt),
                            }),
                            Box::new(LiteralValue {
                                value: right_kind,
                                lemma_type: lt,
                            }),
                        )
                    }
                    _ => match value_to_semantic(value) {
                        Ok(s) => s,
                        Err(e) => {
                            self.errors.push(self.engine_error(e, &effective_source));
                            return;
                        }
                    },
                },
                _ => match value_to_semantic(value) {
                    Ok(s) => s,
                    Err(e) => {
                        self.errors.push(self.engine_error(e, &effective_source));
                        return;
                    }
                },
            }
        };
        let inferred_type: Arc<LemmaType> = match value {
            Value::Text(_) => primitive_text_arc().clone(),
            Value::Number(_) => primitive_number_arc().clone(),
            Value::NumberWithUnit(_, unit) => {
                match self
                    .local_types
                    .iter()
                    .find(|(_, s, _)| Arc::ptr_eq(s, current_spec_arc))
                    .map(|(_, _, t)| t)
                    .and_then(|dt| dt.unit_index.get(unit))
                {
                    Some(lt) => Arc::clone(lt),
                    None => {
                        self.errors.push(self.engine_error(
                            format!("Unit '{}' is not in scope for this spec", unit),
                            &effective_source,
                        ));
                        return;
                    }
                }
            }
            Value::Boolean(_) => primitive_boolean_arc().clone(),
            Value::Date(_) => primitive_date_arc().clone(),
            Value::Time(_) => primitive_time_arc().clone(),
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
        declared_schema_type: Option<Arc<LemmaType>>,
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
                let provisional_type = declared_schema_type
                    .unwrap_or_else(|| Arc::new(LemmaType::undetermined_type()));
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

    /// Ensure `spec` and every qualified-import target spec (transitively) are resolved into
    /// [`Self::local_types`] before the consumer reads cross-spec parent types.
    fn ensure_spec_types_resolved(
        &mut self,
        spec_arc: &Arc<LemmaSpec>,
        spec_repository: &Arc<LemmaRepository>,
        effective: &EffectiveDate,
        type_resolver: &TypeResolver<'a>,
    ) -> bool {
        if self
            .local_types
            .iter()
            .any(|(_, spec, _)| Arc::ptr_eq(spec, spec_arc))
        {
            return true;
        }
        if !type_resolver.is_registered(spec_arc) {
            if self.dependency_discovery_failed {
                // Degraded planning: discovery already reported why this
                // spec's dependencies could not be resolved, and the DAG was
                // reduced to the root spec. Skip the unregistered subtree
                // quietly — the planning result already carries the errors.
                return false;
            }
            panic!(
                "BUG: spec '{}' reachable from spec '{}' was not registered during dependency discovery",
                spec_arc.name, self.main_spec.name
            );
        }
        if type_resolver.registration_failed(spec_arc) {
            // The registration errors were already collected by
            // `register_all`; the spec cannot be built without its types.
            return false;
        }
        for data in &spec_arc.data {
            match &data.value {
                ParsedDataValue::Import(spec_ref) => {
                    let import_effective = spec_ref.at(effective);
                    match self.resolve_spec_ref(
                        spec_ref,
                        &import_effective,
                        spec_arc,
                        spec_repository,
                    ) {
                        Ok((source_repo, source_arc)) => {
                            self.ensure_spec_types_resolved(
                                &source_arc,
                                &source_repo,
                                &import_effective,
                                type_resolver,
                            );
                        }
                        Err(error) => self.errors.push(error),
                    }
                }
                ParsedDataValue::Definition {
                    base: Some(ParentType::Qualified { spec_alias, .. }),
                    ..
                } => {
                    match self.resolve_spec_ref(
                        &ast::SpecRef::same_repository(spec_alias.clone()),
                        effective,
                        spec_arc,
                        spec_repository,
                    ) {
                        Ok((source_repo, source_arc)) => {
                            self.ensure_spec_types_resolved(
                                &source_arc,
                                &source_repo,
                                effective,
                                type_resolver,
                            );
                        }
                        Err(error) => self.errors.push(error),
                    }
                }
                _ => {}
            }
        }
        match type_resolver.resolve_and_validate(spec_arc, effective, &self.local_types) {
            Ok(resolved_types) => {
                self.local_types.push((
                    Arc::clone(spec_repository),
                    Arc::clone(spec_arc),
                    resolved_types,
                ));
                true
            }
            Err(errors) => {
                self.errors.extend(errors);
                false
            }
        }
    }

    fn build_spec(
        &mut self,
        spec_arc: &Arc<LemmaSpec>,
        spec_repository: &Arc<LemmaRepository>,
        current_segments: Vec<PathSegment>,
        data_bindings: DataBindings,
        effective: &EffectiveDate,
        type_resolver: &TypeResolver<'a>,
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
            && !self.ensure_spec_types_resolved(spec_arc, spec_repository, effective, type_resolver)
        {
            return Ok(());
        }

        let mut effective_bindings = data_bindings.clone();
        effective_bindings.extend(this_spec_bindings.clone());

        // Step 4: Add local data using effective bindings (caller + this spec)
        let mut used_binding_keys: HashSet<Vec<String>> = HashSet::new();
        for data in &spec.data {
            if !data.reference.segments.is_empty() {
                continue; // Skip binding data (processed in step 2)
            }
            if matches!(&data.value, ParsedDataValue::With(_)) {
                continue; // With rows apply only through data_bindings
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
                    "No declared data matches with or binding for '{}'",
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
        let mut convert_ctx = RuleExpressionConversion {
            spec: current_spec_arc,
            data_map,
            segments: current_segments,
            rule_names,
            effective,
            depends_on_rules: &mut depends_on_rules,
        };

        let converted_expression = match self
            .convert_expression_and_extract_dependencies(&rule.expression, &mut convert_ctx)
        {
            Some(expr) => expr,
            None => return,
        };
        branches.push((None, converted_expression));

        for unless_clause in &rule.unless_clauses {
            let converted_condition = match self.convert_expression_and_extract_dependencies(
                &unless_clause.condition,
                &mut convert_ctx,
            ) {
                Some(expr) => expr,
                None => return,
            };
            let converted_result = match self.convert_expression_and_extract_dependencies(
                &unless_clause.result,
                &mut convert_ctx,
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
            rule_type: Arc::new(LemmaType::veto_type()),
            spec_arc: Arc::clone(current_spec_arc),
        };

        self.rules.insert(rule_path, rule_node);
    }

    /// Converts left and right expressions and accumulates rule dependencies.
    fn convert_binary_operands(
        &mut self,
        left: &ast::Expression,
        right: &ast::Expression,
        ctx: &mut RuleExpressionConversion<'_>,
    ) -> Option<(Expression, Expression)> {
        let converted_left = self.convert_expression_and_extract_dependencies(left, ctx)?;
        let converted_right = self.convert_expression_and_extract_dependencies(right, ctx)?;
        Some((converted_left, converted_right))
    }

    /// Converts an AST expression into a resolved expression and records any rule references.
    fn convert_expression_and_extract_dependencies(
        &mut self,
        expr: &ast::Expression,
        ctx: &mut RuleExpressionConversion<'_>,
    ) -> Option<Expression> {
        let expr_src = expr
            .source_location
            .as_ref()
            .expect("BUG: AST expression missing source location");
        match &expr.kind {
            ast::ExpressionKind::Reference(r) => {
                let expr_source = expr_src;
                let (segments, target_arc_opt) = if r.segments.is_empty() {
                    (ctx.segments.to_vec(), None)
                } else {
                    let data_map_owned: HashMap<String, LemmaData> = ctx
                        .data_map
                        .iter()
                        .map(|(k, v)| (k.clone(), (*v).clone()))
                        .collect();
                    let (segs, arc) = self.resolve_path_segments(
                        &r.segments,
                        expr_source,
                        data_map_owned,
                        ctx.segments.to_vec(),
                        Arc::clone(ctx.spec),
                        ctx.effective,
                    )?;
                    (segs, Some(arc))
                };

                let (is_data, is_rule, target_spec_name_opt) = match &target_arc_opt {
                    None => {
                        let is_data = ctx.data_map.contains_key(&r.name);
                        let is_rule = ctx.rule_names.contains(r.name.as_str());
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
                    ctx.depends_on_rules.insert(rule_path.clone());
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
                let (l, r) = self.convert_binary_operands(left, right, ctx)?;
                Some(Expression {
                    kind: ExpressionKind::LogicalAnd(Arc::new(l), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::Arithmetic(left, op, right) => {
                let (l, r) = self.convert_binary_operands(left, right, ctx)?;
                Some(Expression {
                    kind: ExpressionKind::Arithmetic(Arc::new(l), op.clone(), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::Comparison(left, op, right) => {
                let (l, r) = self.convert_binary_operands(left, right, ctx)?;
                Some(Expression {
                    kind: ExpressionKind::Comparison(Arc::new(l), op.clone(), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::UnitConversion(value, target) => {
                let converted_value =
                    self.convert_expression_and_extract_dependencies(value, ctx)?;

                let resolved_spec_types = self
                    .local_types
                    .iter()
                    .find(|(_, s, _)| Arc::ptr_eq(s, ctx.spec))
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
                let converted_operand =
                    self.convert_expression_and_extract_dependencies(operand, ctx)?;
                Some(Expression {
                    kind: ExpressionKind::LogicalNegation(
                        Arc::new(converted_operand),
                        neg_type.clone(),
                    ),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::MathematicalComputation(op, operand) => {
                let converted_operand =
                    self.convert_expression_and_extract_dependencies(operand, ctx)?;
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
                            .find(|(_, s, _)| Arc::ptr_eq(s, ctx.spec))
                            .map(|(_, _, t)| t)
                            .and_then(|dt| dt.unit_index.get(unit))
                            .map(Arc::as_ref)
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
                    Value::Range(left, right) => match (left.as_ref(), right.as_ref()) {
                        (
                            Value::NumberWithUnit(left_mag, unit),
                            Value::NumberWithUnit(right_mag, right_unit),
                        ) if unit == right_unit => {
                            let Some(lt) = self
                                .local_types
                                .iter()
                                .find(|(_, s, _)| Arc::ptr_eq(s, ctx.spec))
                                .map(|(_, _, t)| t)
                                .and_then(|dt| dt.unit_index.get(unit))
                                .map(Arc::clone)
                            else {
                                self.errors.push(self.engine_error(
                                    format!("Unit '{}' is not in scope for this spec", unit),
                                    expr_src,
                                ));
                                return None;
                            };
                            let left_kind = match number_with_unit_to_value_kind(
                                *left_mag,
                                unit,
                                lt.as_ref(),
                            ) {
                                Ok(v) => v,
                                Err(e) => {
                                    self.errors.push(self.engine_error(e, expr_src));
                                    return None;
                                }
                            };
                            let right_kind =
                                match number_with_unit_to_value_kind(*right_mag, unit, lt.as_ref())
                                {
                                    Ok(v) => v,
                                    Err(e) => {
                                        self.errors.push(self.engine_error(e, expr_src));
                                        return None;
                                    }
                                };
                            ValueKind::Range(
                                Box::new(LiteralValue {
                                    value: left_kind,
                                    lemma_type: Arc::clone(&lt),
                                }),
                                Box::new(LiteralValue {
                                    value: right_kind,
                                    lemma_type: lt,
                                }),
                            )
                        }
                        _ => match value_to_semantic(value) {
                            Ok(v) => v,
                            Err(e) => {
                                self.errors.push(self.engine_error(e, expr_src));
                                return None;
                            }
                        },
                    },
                    _ => match value_to_semantic(value) {
                        Ok(v) => v,
                        Err(e) => {
                            self.errors.push(self.engine_error(e, expr_src));
                            return None;
                        }
                    },
                };
                let lemma_type: Arc<LemmaType> = match value {
                    Value::Text(_) => primitive_text_arc().clone(),
                    Value::Number(_) => primitive_number_arc().clone(),
                    Value::NumberWithUnit(_, unit) => {
                        match self
                            .local_types
                            .iter()
                            .find(|(_, s, _)| Arc::ptr_eq(s, ctx.spec))
                            .map(|(_, _, t)| t)
                            .and_then(|dt| dt.unit_index.get(unit))
                        {
                            Some(lt) => Arc::clone(lt),
                            None => {
                                self.errors.push(self.engine_error(
                                    format!("Unit '{}' is not in scope for this spec", unit),
                                    expr_src,
                                ));
                                return None;
                            }
                        }
                    }
                    Value::Boolean(_) => primitive_boolean_arc().clone(),
                    Value::Date(_) => primitive_date_arc().clone(),
                    Value::Time(_) => primitive_time_arc().clone(),
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
                let converted = self.convert_expression_and_extract_dependencies(operand, ctx)?;
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
                let converted_date =
                    self.convert_expression_and_extract_dependencies(date_expr, ctx)?;
                Some(Expression {
                    kind: ExpressionKind::DateRelative(*kind, Arc::new(converted_date)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::DateCalendar(kind, unit, date_expr) => {
                let converted_date =
                    self.convert_expression_and_extract_dependencies(date_expr, ctx)?;
                Some(Expression {
                    kind: ExpressionKind::DateCalendar(*kind, *unit, Arc::new(converted_date)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::RangeLiteral(left, right) => {
                let (l, r) = self.convert_binary_operands(left, right, ctx)?;
                Some(Expression {
                    kind: ExpressionKind::RangeLiteral(Arc::new(l), Arc::new(r)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::PastFutureRange(kind, offset_expr) => {
                let converted_offset =
                    self.convert_expression_and_extract_dependencies(offset_expr, ctx)?;
                Some(Expression {
                    kind: ExpressionKind::PastFutureRange(*kind, Arc::new(converted_offset)),
                    source_location: expr.source_location.clone(),
                })
            }

            ast::ExpressionKind::RangeContainment(value, range) => {
                let (converted_value, converted_range) =
                    self.convert_binary_operands(value, range, ctx)?;
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

fn find_type_by_name_in_scope(
    resolved_types: &ResolvedTypesMap,
    spec_arc: &Arc<LemmaSpec>,
    name: &str,
) -> Option<Arc<LemmaType>> {
    if let Some(spec_types) = find_types_by_spec(resolved_types, spec_arc) {
        if let Some(t) = spec_types
            .resolved
            .get(name)
            .or_else(|| spec_types.unit_index.get(name))
        {
            return Some(Arc::clone(t));
        }
    }
    for data in &spec_arc.data {
        let ParsedDataValue::Import(spec_ref) = &data.value else {
            continue;
        };
        let Some((_, _, imported_types)) = resolved_types
            .iter()
            .find(|(_, s, _)| s.name == spec_ref.name)
        else {
            continue;
        };
        if let Some(t) = imported_types
            .resolved
            .get(name)
            .or_else(|| imported_types.unit_index.get(name))
        {
            return Some(Arc::clone(t));
        }
    }
    None
}

/// Result of a decomposition-based type lookup in scope.
///
/// Used by both `infer_expression_type` (to promote anonymous results to named types) and the
/// rule-boundary check (to produce precise error messages naming candidate types).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecompositionMatch {
    /// No declared quantity type in scope has this decomposition.
    None,
    /// Exactly one declared quantity type in scope has this decomposition.
    Unique(String),
    /// Multiple declared quantity types in scope share this decomposition; the names
    /// are sorted for stable diagnostic ordering.
    Multiple(Vec<String>),
}

/// Find the quantity type(s) in scope whose decomposition matches `decomposition` exactly.
///
/// Replaces the special-case `find_duration_type_in_spec` and the inline decomposition
/// match-loop. It walks every declared quantity type in `spec_arc` plus any quantity types
/// reachable through imports, deduplicates by name, and reports whether exactly one matches.
pub fn find_unique_quantity_type_by_decomposition(
    resolved_types: &ResolvedTypesMap,
    spec_arc: &Arc<LemmaSpec>,
    decomposition: &BaseQuantityVector,
) -> DecompositionMatch {
    let mut seen: HashSet<String> = HashSet::new();

    let consider = |lemma_type: &LemmaType, seen: &mut HashSet<String>| {
        if !matches!(
            lemma_type.specifications,
            TypeSpecification::Quantity { .. }
        ) {
            return;
        }
        if lemma_type
            .quantity_type_decomposition()
            .is_none_or(|d| d != decomposition)
        {
            return;
        }
        seen.insert(lemma_type.name().to_string());
    };

    if let Some(spec_types) = find_types_by_spec(resolved_types, spec_arc) {
        for lemma_type in spec_types.resolved.values() {
            consider(lemma_type.as_ref(), &mut seen);
        }
        for lemma_type in spec_types.unit_index.values() {
            consider(lemma_type.as_ref(), &mut seen);
        }
    }

    for data in &spec_arc.data {
        let ParsedDataValue::Import(spec_ref) = &data.value else {
            continue;
        };
        let Some((_, _, imported_types)) = resolved_types
            .iter()
            .find(|(_, s, _)| s.name == spec_ref.name)
        else {
            continue;
        };
        for lemma_type in imported_types.resolved.values() {
            consider(lemma_type.as_ref(), &mut seen);
        }
        for lemma_type in imported_types.unit_index.values() {
            consider(lemma_type.as_ref(), &mut seen);
        }
    }

    match seen.len() {
        0 => DecompositionMatch::None,
        1 => DecompositionMatch::Unique(
            seen.into_iter()
                .next()
                .expect("BUG: seen has exactly one element, len checked"),
        ),
        _ => {
            let mut names: Vec<String> = seen.into_iter().collect();
            names.sort();
            DecompositionMatch::Multiple(names)
        }
    }
}

/// True iff an anonymous quantity at the rule boundary must be rejected.
///
/// Planning promotes anonymous intermediates to named types when a unique in-scope type
/// shares the decomposition. Any anonymous quantity that still reaches a rule boundary
/// cannot be serialized on the API (no declared unit map to emit).
fn anonymous_rule_boundary_requires_rejection() -> bool {
    true
}

/// Build a precise error message for an anonymous quantity that survives to a rule boundary.
///
/// The boundary check rejects such intermediates: a rule must produce a named type so its
/// result is unambiguous. If multiple in-scope types share the decomposition, the message
/// names them as a hint to `as <type>` cast the result.
fn anonymous_rule_boundary_error(
    rule_path: &RulePath,
    spec_arc: &Arc<LemmaSpec>,
    resolved_types: &ResolvedTypesMap,
    decomposition: &BaseQuantityVector,
    branch_index: Option<usize>,
) -> String {
    let candidates_hint = match find_unique_quantity_type_by_decomposition(
        resolved_types,
        spec_arc,
        decomposition,
    ) {
        DecompositionMatch::Multiple(names) => format!(
            " Multiple types in scope share these dimensions: {}. Give the rule an explicit named type.",
            names.join(", ")
        ),
        _ => String::new(),
    };
    match branch_index {
        Some(index) => format!(
            "Unless clause {} in rule '{}' (spec '{}') returns an anonymous intermediate with \
             unresolved dimensions {:?}. Give the rule a named quantity or ratio type with \
             declared units, or rewrite the expression so dimensions resolve to a named type in scope.{}",
            index, rule_path.rule, spec_arc.name, decomposition, candidates_hint
        ),
        None => format!(
            "Rule '{}' in spec '{}' returns an anonymous intermediate with unresolved \
             dimensions {:?}. Give the rule a named quantity or ratio type with declared units, \
             or rewrite the expression so dimensions resolve to a named type in scope.{}",
            rule_path.rule, spec_arc.name, decomposition, candidates_hint
        ),
    }
}

fn compute_arithmetic_result_type(
    left_type: Arc<LemmaType>,
    op: &ArithmeticComputation,
    right_type: Arc<LemmaType>,
) -> Arc<LemmaType> {
    compute_arithmetic_result_type_recursive(left_type, op, right_type, false)
}

fn compute_arithmetic_result_type_recursive(
    left_type: Arc<LemmaType>,
    op: &ArithmeticComputation,
    right_type: Arc<LemmaType>,
    swapped: bool,
) -> Arc<LemmaType> {
    match (&left_type.specifications, &right_type.specifications) {
        (TypeSpecification::Veto { .. }, _) | (_, TypeSpecification::Veto { .. }) => {
            Arc::new(LemmaType::veto_type())
        }
        (TypeSpecification::Undetermined, _) => Arc::new(LemmaType::undetermined_type()),

        (TypeSpecification::Date { .. }, TypeSpecification::Time { .. }) => Arc::new(
            LemmaType::anonymous_for_decomposition(duration_decomposition()),
        ),

        // Quantity pairs must fall through to operator-specific arms below.
        // The general equal-type guard must not short-circuit those.
        _ if *left_type == *right_type
            && !matches!(
                &left_type.specifications,
                TypeSpecification::Quantity { .. }
                    | TypeSpecification::QuantityRange { .. }
                    | TypeSpecification::NumberRange { .. }
                    | TypeSpecification::DateRange { .. }
                    | TypeSpecification::TimeRange { .. }
                    | TypeSpecification::RatioRange { .. }
            ) =>
        {
            Arc::clone(&left_type)
        }

        (TypeSpecification::Date { .. }, TypeSpecification::Quantity { .. })
            if right_type.is_duration_like_quantity() =>
        {
            Arc::clone(&left_type)
        }
        (TypeSpecification::Date { .. }, TypeSpecification::Quantity { .. })
            if right_type.is_calendar_like_quantity() =>
        {
            Arc::clone(&left_type)
        }
        (TypeSpecification::Quantity { .. }, TypeSpecification::Date { .. })
            if left_type.is_calendar_like_quantity() =>
        {
            Arc::clone(&right_type)
        }
        (TypeSpecification::Time { .. }, TypeSpecification::Quantity { .. })
            if right_type.is_duration_like_quantity() =>
        {
            Arc::clone(&left_type)
        }

        (TypeSpecification::Quantity { .. }, TypeSpecification::Ratio { .. }) => {
            Arc::clone(&left_type)
        }
        (TypeSpecification::Quantity { .. }, TypeSpecification::Number { .. }) => match op {
            ArithmeticComputation::Multiply
            | ArithmeticComputation::Divide
            | ArithmeticComputation::Modulo
            | ArithmeticComputation::Power => Arc::clone(&left_type),
            _ => Arc::new(LemmaType::undetermined_type()),
        },
        (
            TypeSpecification::Quantity {
                decomposition: l_decomp_opt,
                ..
            },
            TypeSpecification::Quantity {
                decomposition: r_decomp_opt,
                ..
            },
        ) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                if left_type.compatible_with_anonymous_quantity(&right_type)
                    || right_type.compatible_with_anonymous_quantity(&left_type)
                {
                    let left_decomp = left_type.quantity_type_decomposition();
                    let right_decomp = right_type.quantity_type_decomposition();
                    if let (Some(ld), Some(rd)) = (left_decomp, right_decomp) {
                        if ld == rd {
                            if *ld == duration_decomposition() {
                                Arc::new(LemmaType::anonymous_for_decomposition(
                                    duration_decomposition(),
                                ))
                            } else {
                                Arc::new(LemmaType::anonymous_for_decomposition(ld.clone()))
                            }
                        } else if left_type.is_duration_like_quantity()
                            && right_type.is_duration_like_quantity()
                        {
                            Arc::new(LemmaType::anonymous_for_decomposition(
                                duration_decomposition(),
                            ))
                        } else if left_type.is_calendar_like() && right_type.is_calendar_like() {
                            Arc::new(LemmaType::anonymous_for_decomposition(
                                calendar_decomposition(),
                            ))
                        } else {
                            Arc::clone(&left_type)
                        }
                    } else if left_type.is_duration_like_quantity()
                        && right_type.is_duration_like_quantity()
                    {
                        Arc::new(LemmaType::anonymous_for_decomposition(
                            duration_decomposition(),
                        ))
                    } else if left_type.is_calendar_like() && right_type.is_calendar_like() {
                        Arc::new(LemmaType::anonymous_for_decomposition(
                            calendar_decomposition(),
                        ))
                    } else {
                        Arc::clone(&left_type)
                    }
                } else {
                    Arc::clone(&left_type)
                }
            }
            ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {
                match (l_decomp_opt, r_decomp_opt) {
                    (Some(l_decomp), Some(r_decomp)) => {
                        let combined = combine_decompositions(
                            l_decomp,
                            r_decomp,
                            matches!(op, ArithmeticComputation::Multiply),
                        );
                        if combined.is_empty() {
                            primitive_number_arc().clone()
                        } else {
                            Arc::new(LemmaType::anonymous_for_decomposition(combined))
                        }
                    }
                    _ => Arc::clone(&left_type),
                }
            }
            _ => primitive_number_arc().clone(),
        },

        (
            TypeSpecification::Number { .. },
            TypeSpecification::Quantity {
                decomposition: r_decomp_opt,
                ..
            },
        ) => match op {
            ArithmeticComputation::Multiply => Arc::clone(&right_type),
            ArithmeticComputation::Divide => match r_decomp_opt {
                Some(r_decomp) if !r_decomp.is_empty() => {
                    let negated: BaseQuantityVector =
                        r_decomp.iter().map(|(k, &e)| (k.clone(), -e)).collect();
                    Arc::new(LemmaType::anonymous_for_decomposition(negated))
                }
                _ => primitive_number_arc().clone(),
            },
            _ => Arc::new(LemmaType::undetermined_type()),
        },

        (TypeSpecification::Number { .. }, TypeSpecification::Ratio { .. }) => {
            primitive_number_arc().clone()
        }
        (TypeSpecification::Number { .. }, TypeSpecification::Number { .. }) => {
            primitive_number_arc().clone()
        }

        (TypeSpecification::Ratio { .. }, TypeSpecification::Ratio { .. }) => {
            Arc::clone(&left_type)
        }
        (TypeSpecification::DateRange { .. }, TypeSpecification::DateRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_span_type(&left_type)
            }
            _ => Arc::new(LemmaType::undetermined_type()),
        },
        (TypeSpecification::NumberRange { .. }, TypeSpecification::NumberRange { .. }) => {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_span_type(&left_type)
                }
                _ => Arc::new(LemmaType::undetermined_type()),
            }
        }
        (TypeSpecification::QuantityRange { .. }, TypeSpecification::QuantityRange { .. }) => {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
                    if range_matches_range_quantity(&left_type, &right_type) =>
                {
                    range_span_type(&left_type)
                }
                _ => Arc::new(LemmaType::undetermined_type()),
            }
        }
        (TypeSpecification::RatioRange { .. }, TypeSpecification::RatioRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_span_type(&left_type)
            }
            _ => Arc::new(LemmaType::undetermined_type()),
        },
        (TypeSpecification::NumberRange { .. }, TypeSpecification::Number { .. })
        | (TypeSpecification::RatioRange { .. }, TypeSpecification::Ratio { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_quantity_type_for_operand(&left_type, &right_type)
            }
            _ => Arc::new(LemmaType::undetermined_type()),
        },
        (TypeSpecification::QuantityRange { .. }, TypeSpecification::Quantity { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract
                if range_matches_quantity_type(&left_type, &right_type) =>
            {
                range_quantity_type_for_operand(&left_type, &right_type)
            }
            _ => Arc::new(LemmaType::undetermined_type()),
        },
        (TypeSpecification::Number { .. }, TypeSpecification::NumberRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_quantity_type_for_operand(&right_type, &left_type)
            }
            _ => Arc::new(LemmaType::undetermined_type()),
        },
        (TypeSpecification::Quantity { .. }, TypeSpecification::QuantityRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract
                if range_matches_quantity_type(&right_type, &left_type) =>
            {
                range_quantity_type_for_operand(&right_type, &left_type)
            }
            _ => Arc::new(LemmaType::undetermined_type()),
        },
        (TypeSpecification::Ratio { .. }, TypeSpecification::RatioRange { .. }) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                range_quantity_type_for_operand(&right_type, &left_type)
            }
            _ => Arc::new(LemmaType::undetermined_type()),
        },
        (TypeSpecification::DateRange { .. }, TypeSpecification::Quantity { .. })
            if right_type.is_duration_like_quantity() =>
        {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_quantity_type_for_operand(&left_type, &right_type)
                }
                _ => Arc::new(LemmaType::undetermined_type()),
            }
        }
        (TypeSpecification::DateRange { .. }, TypeSpecification::Quantity { .. })
            if right_type.is_calendar_like_quantity() =>
        {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_quantity_type_for_operand(&left_type, &right_type)
                }
                _ => Arc::new(LemmaType::undetermined_type()),
            }
        }
        (TypeSpecification::Quantity { .. }, TypeSpecification::DateRange { .. })
            if left_type.is_duration_like_quantity() =>
        {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_quantity_type_for_operand(&right_type, &left_type)
                }
                _ => Arc::new(LemmaType::undetermined_type()),
            }
        }
        (TypeSpecification::Quantity { .. }, TypeSpecification::DateRange { .. })
            if left_type.is_calendar_like_quantity() =>
        {
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    range_quantity_type_for_operand(&right_type, &left_type)
                }
                _ => Arc::new(LemmaType::undetermined_type()),
            }
        }
        _ => {
            if swapped {
                Arc::new(LemmaType::undetermined_type())
            } else {
                compute_arithmetic_result_type_recursive(right_type, op, left_type, true)
            }
        }
    }
}

fn infer_range_type_from_endpoint_types(
    left_type: &LemmaType,
    right_type: &LemmaType,
) -> Arc<LemmaType> {
    range_type_specification_from_endpoints(left_type, right_type)
        .map(|spec| Arc::new(LemmaType::primitive(spec)))
        .unwrap_or_else(|| Arc::new(LemmaType::undetermined_type()))
}

fn range_span_type(range_type: &LemmaType) -> Arc<LemmaType> {
    match &range_type.specifications {
        TypeSpecification::DateRange { .. } => Arc::new(LemmaType::anonymous_for_decomposition(
            duration_decomposition(),
        )),
        TypeSpecification::TimeRange { .. } => Arc::new(LemmaType::anonymous_for_decomposition(
            duration_decomposition(),
        )),
        TypeSpecification::NumberRange { .. } => primitive_number_arc().clone(),
        TypeSpecification::QuantityRange { units, .. } => {
            Arc::new(LemmaType::primitive(TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: units.clone(),
                traits: Vec::new(),
                decomposition: None,
                help: String::new(),
            }))
        }
        TypeSpecification::RatioRange { units, .. } => {
            Arc::new(LemmaType::primitive(TypeSpecification::Ratio {
                minimum: None,
                maximum: None,
                decimals: None,
                units: units.clone(),
                help: String::new(),
            }))
        }
        _ => Arc::new(LemmaType::undetermined_type()),
    }
}

fn range_quantity_type_for_operand(
    range_type: &LemmaType,
    other_type: &LemmaType,
) -> Arc<LemmaType> {
    let _ = other_type;
    if range_type.is_range() {
        Arc::new(range_type.clone())
    } else {
        range_span_type(range_type)
    }
}

fn range_matches_quantity_type(range_type: &LemmaType, measure_type: &LemmaType) -> bool {
    match &range_type.specifications {
        TypeSpecification::DateRange { .. } => {
            measure_type.is_duration_like() || measure_type.is_calendar_like()
        }
        TypeSpecification::TimeRange { .. } => measure_type.is_duration_like(),
        TypeSpecification::NumberRange { .. } => measure_type.is_number(),
        TypeSpecification::QuantityRange { .. } => {
            measure_type.is_quantity() && quantity_range_matches_quantity(range_type, measure_type)
        }
        TypeSpecification::RatioRange { .. } => measure_type.is_ratio(),
        _ => false,
    }
}

fn range_matches_range_quantity(left_range: &LemmaType, right_range: &LemmaType) -> bool {
    let right_measure_type = range_span_type(right_range);
    !right_measure_type.is_undetermined()
        && range_matches_quantity_type(left_range, &right_measure_type)
}

fn quantity_range_matches_quantity(range_type: &LemmaType, quantity_type: &LemmaType) -> bool {
    if !quantity_type.is_quantity() {
        return false;
    }
    if let Some(TypeSpecification::Quantity {
        units,
        decomposition,
        ..
    }) = range_type.specifications.element_from_range()
    {
        let endpoint_type = LemmaType::primitive(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units,
            traits: Vec::new(),
            decomposition,
            help: String::new(),
        });
        if endpoint_type.same_quantity_family(quantity_type)
            || endpoint_type.compatible_with_anonymous_quantity(quantity_type)
            || quantity_type.compatible_with_anonymous_quantity(&endpoint_type)
        {
            return true;
        }
    }
    match (&range_type.specifications, &quantity_type.specifications) {
        (
            TypeSpecification::QuantityRange {
                units: range_units,
                decomposition: range_decomposition,
                ..
            },
            TypeSpecification::Quantity {
                units: quantity_units,
                decomposition: quantity_decomposition,
                ..
            },
        ) => {
            if range_units.0.is_empty() && range_decomposition.is_none() {
                true
            } else if quantity_decomposition.is_none() {
                range_units == quantity_units
            } else {
                range_units == quantity_units && range_decomposition == quantity_decomposition
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
    computed_rule_types: &HashMap<RulePath, Arc<LemmaType>>,
    resolved_types: &ResolvedTypesMap,
    spec_arc: &Arc<LemmaSpec>,
) -> Arc<LemmaType> {
    match &expression.kind {
        ExpressionKind::Literal(literal_value) => Arc::clone(&literal_value.lemma_type),

        ExpressionKind::DataPath(data_path) => {
            infer_data_type(data_path, graph, computed_rule_types)
        }

        ExpressionKind::RulePath(rule_path) => computed_rule_types
            .get(rule_path)
            .cloned()
            .unwrap_or_else(|| Arc::new(LemmaType::undetermined_type())),

        ExpressionKind::LogicalAnd(left, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            if left_type.vetoed() || right_type.vetoed() {
                return Arc::new(LemmaType::veto_type());
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return Arc::new(LemmaType::undetermined_type());
            }
            if !left_type.is_boolean() {
                return Arc::new(LemmaType::undetermined_type());
            }
            if right_type.is_boolean() {
                primitive_boolean_arc().clone()
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
                return Arc::new(LemmaType::veto_type());
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return Arc::new(LemmaType::undetermined_type());
            }
            if left_type.is_boolean() && right_type.is_boolean() {
                return primitive_boolean_arc().clone();
            }
            if *left_type == *right_type {
                return left_type;
            }
            Arc::new(LemmaType::undetermined_type())
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
                return Arc::new(LemmaType::veto_type());
            }
            if operand_type.is_undetermined() {
                return Arc::new(LemmaType::undetermined_type());
            }
            primitive_boolean_arc().clone()
        }

        ExpressionKind::Comparison(left, _op, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            if left_type.vetoed() || right_type.vetoed() {
                return Arc::new(LemmaType::veto_type());
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return Arc::new(LemmaType::undetermined_type());
            }
            primitive_boolean_arc().clone()
        }

        ExpressionKind::Arithmetic(left, operator, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            let mut result = compute_arithmetic_result_type(
                Arc::clone(&left_type),
                operator,
                Arc::clone(&right_type),
            );
            if result.is_anonymous_quantity() {
                if let Some(decomp) = result.quantity_type_decomposition() {
                    if !decomp.is_empty() {
                        if let DecompositionMatch::Unique(name) =
                            find_unique_quantity_type_by_decomposition(
                                resolved_types,
                                spec_arc,
                                decomp,
                            )
                        {
                            result = find_type_by_name_in_scope(resolved_types, spec_arc, &name)
                                .expect("BUG: Unique decomposition match must be findable by name");
                        }
                    }
                }
            }
            if matches!(operator, ArithmeticComputation::Divide)
                && left_type.is_number()
                && right_type.is_quantity()
                && result.is_anonymous_quantity()
            {
                result = primitive_number_arc().clone();
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
            match target {
                SemanticConversionTarget::Type(PrimitiveKind::Number) => {
                    primitive_number_arc().clone()
                }
                SemanticConversionTarget::Type(PrimitiveKind::Text) => primitive_text_arc().clone(),
                SemanticConversionTarget::Type(PrimitiveKind::Boolean) => {
                    primitive_boolean_arc().clone()
                }
                SemanticConversionTarget::Type(kind)
                    if source_type.matches_primitive_kind(*kind) =>
                {
                    source_type
                }
                SemanticConversionTarget::Unit { unit_name } => {
                    lookup_unit_type(resolved_types, spec_arc, unit_name)
                        .unwrap_or_else(|| Arc::new(LemmaType::undetermined_type()))
                }
                SemanticConversionTarget::Type(_) => Arc::new(LemmaType::undetermined_type()),
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
                return Arc::new(LemmaType::veto_type());
            }
            if operand_type.is_undetermined() {
                return Arc::new(LemmaType::undetermined_type());
            }
            primitive_number_arc().clone()
        }

        ExpressionKind::Veto(_) => Arc::new(LemmaType::veto_type()),

        ExpressionKind::ResultIsVeto(operand) => {
            let _ = infer_expression_type(
                operand,
                graph,
                computed_rule_types,
                resolved_types,
                spec_arc,
            );
            primitive_boolean_arc().clone()
        }

        ExpressionKind::Now => primitive_date_arc().clone(),

        ExpressionKind::DateRelative(..)
        | ExpressionKind::DateCalendar(..)
        | ExpressionKind::RangeContainment(..) => primitive_boolean_arc().clone(),

        ExpressionKind::RangeLiteral(left, right) => {
            let left_type =
                infer_expression_type(left, graph, computed_rule_types, resolved_types, spec_arc);
            let right_type =
                infer_expression_type(right, graph, computed_rule_types, resolved_types, spec_arc);
            if left_type.vetoed() || right_type.vetoed() {
                return Arc::new(LemmaType::veto_type());
            }
            if left_type.is_undetermined() || right_type.is_undetermined() {
                return Arc::new(LemmaType::undetermined_type());
            }
            infer_range_type_from_endpoint_types(left_type.as_ref(), right_type.as_ref())
        }

        ExpressionKind::PastFutureRange(..) => primitive_date_range_arc().clone(),

        ExpressionKind::Piecewise(arms) => {
            let mut result_type: Option<Arc<LemmaType>> = None;
            for (condition, result) in arms {
                let condition_type = infer_expression_type(
                    condition,
                    graph,
                    computed_rule_types,
                    resolved_types,
                    spec_arc,
                );
                if !condition_type.is_boolean() && !condition_type.is_undetermined() {
                    return Arc::new(LemmaType::undetermined_type());
                }
                let arm_result_type = infer_expression_type(
                    result,
                    graph,
                    computed_rule_types,
                    resolved_types,
                    spec_arc,
                );
                match &result_type {
                    None => result_type = Some(arm_result_type),
                    Some(existing) if *existing == arm_result_type => {}
                    Some(_)
                        if arm_result_type.is_undetermined()
                            || result_type.as_ref().is_some_and(|t| t.is_undetermined()) =>
                    {
                        result_type = Some(Arc::new(LemmaType::undetermined_type()));
                    }
                    Some(_) => return Arc::new(LemmaType::undetermined_type()),
                }
            }
            result_type.unwrap_or_else(|| Arc::new(LemmaType::undetermined_type()))
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
    computed_rule_types: &HashMap<RulePath, Arc<LemmaType>>,
) -> Arc<LemmaType> {
    let entry = match graph.data().get(data_path) {
        Some(e) => e,
        None => return Arc::new(LemmaType::undetermined_type()),
    };
    match entry {
        DataDefinition::Value { value, .. } => Arc::clone(&value.lemma_type),
        DataDefinition::TypeDeclaration { resolved_type, .. } => Arc::clone(resolved_type),
        DataDefinition::Reference {
            target: ReferenceTarget::Rule(target_rule),
            resolved_type,
            ..
        } => {
            if !resolved_type.is_undetermined() {
                Arc::clone(resolved_type)
            } else {
                computed_rule_types
                    .get(target_rule)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(LemmaType::undetermined_type()))
            }
        }
        DataDefinition::Reference { resolved_type, .. } => Arc::clone(resolved_type),
        DataDefinition::Import { .. } => Arc::new(LemmaType::undetermined_type()),
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
                "Logical operation requires boolean operands, got {} for left operand",
                left_type
            ),
        ));
    }
    if !right_type.is_boolean() {
        errors.push(engine_error_at_graph(
            graph,
            source,
            format!(
                "Logical operation requires boolean operands, got {} for right operand",
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
            "Logical OR requires matching types (unless-chain / De Morgan), got {} and {}",
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
                "Logical AND requires boolean left operand, got {}",
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
                "Logical negation requires boolean operand, got {}",
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
            format!("Cannot compare {} with {}", left_type, right_type),
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
        let same_decomposition = match (
            left_type.quantity_type_decomposition(),
            right_type.quantity_type_decomposition(),
        ) {
            (Some(ld), Some(rd)) => ld == rd,
            _ => false,
        };
        if !left_type.same_quantity_family(right_type)
            && !left_type.compatible_with_anonymous_quantity(right_type)
            && !same_decomposition
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
    if left_type.is_calendar_like() && right_type.is_calendar_like() {
        return Ok(());
    }
    if left_type.is_calendar_like() && right_type.is_number() {
        return Ok(());
    }
    if left_type.is_number() && right_type.is_calendar_like() {
        return Ok(());
    }

    Err(vec![engine_error_at_graph(
        graph,
        source,
        format!("Cannot compare {} with {}", left_type, right_type),
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
                n.denom() == &crate::computation::bigint::BigInt::one()
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
        if range_measure_allowed {
            return Ok(());
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
        if matches!(operator, ArithmeticComputation::Subtract) {
            if left_type.is_date() && right_type.is_date() {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    "Cannot subtract dates. Use a date range instead: `start...end as days` or `start...end as year`.".to_string(),
                )]);
            }
            if left_type.is_time() && right_type.is_time() {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source,
                    "Cannot subtract times. Use a datetime range instead: `start...end as hours` or `start...end as seconds`.".to_string(),
                )]);
            }
        }

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
                left_type.is_calendar_like(),
                right_type.is_calendar_like(),
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

    if left_type.is_calendar_like() && right_type.is_calendar_like() {
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

    if (left_type.is_duration_like() && right_type.is_calendar_like())
        || (left_type.is_calendar_like() && right_type.is_duration_like())
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
        || left_type.is_calendar_like()
        || left_type.is_ratio();
    let right_valid = right_type.is_quantity()
        || right_type.is_number()
        || right_type.is_duration_like()
        || right_type.is_calendar_like()
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
                || pair(LemmaType::is_quantity, LemmaType::is_calendar_like)
                || pair(LemmaType::is_duration_like_quantity, LemmaType::is_number)
                || pair(LemmaType::is_duration_like_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_calendar_like, LemmaType::is_number)
                || pair(LemmaType::is_calendar_like, LemmaType::is_ratio)
                || pair(LemmaType::is_number, LemmaType::is_ratio)
        }
        ArithmeticComputation::Divide => {
            pair(LemmaType::is_quantity, LemmaType::is_number)
                || pair(LemmaType::is_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_quantity, LemmaType::is_duration_like_quantity)
                || pair(LemmaType::is_quantity, LemmaType::is_calendar_like)
                || (left_type.is_duration_like() && right_type.is_number())
                || (left_type.is_duration_like() && right_type.is_ratio())
                || (left_type.is_calendar_like() && right_type.is_number())
                || (left_type.is_calendar_like() && right_type.is_ratio())
                || (left_type.is_number() && right_type.is_duration_like())
                || (left_type.is_number() && right_type.is_calendar_like())
                || pair(LemmaType::is_number, LemmaType::is_ratio)
        }
        ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
            pair(LemmaType::is_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_duration_like_quantity, LemmaType::is_ratio)
                || pair(LemmaType::is_calendar_like, LemmaType::is_ratio)
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

fn has_explicit_unit(source: &Expression) -> bool {
    match &source.kind {
        ExpressionKind::Literal(lit) => match &lit.value {
            ValueKind::Quantity(_, sig) => sig.len() == 1 && sig[0].1 == 1 && !sig[0].0.is_empty(),
            ValueKind::Ratio(_, unit) => unit.is_some(),
            _ => false,
        },
        ExpressionKind::UnitConversion(_, SemanticConversionTarget::Unit { .. }) => true,
        _ => false,
    }
}

fn expression_suggestion_label(expression: &Expression) -> String {
    match &expression.kind {
        ExpressionKind::DataPath(p) => p.data.clone(),
        ExpressionKind::RulePath(p) => p.rule.clone(),
        ExpressionKind::Arithmetic(left, op, right) => {
            format!(
                "{} {} {}",
                expression_suggestion_label(left),
                op,
                expression_suggestion_label(right),
            )
        }
        ExpressionKind::UnitConversion(inner, target) => {
            format!("{} as {}", expression_suggestion_label(inner), target)
        }
        ExpressionKind::Literal(lit) => match &lit.value {
            ValueKind::Number(n) => crate::computation::rational::rational_to_display_str(n),
            _ => "<value>".to_string(),
        },
        _ => "<expr>".to_string(),
    }
}

fn first_unit_suggestion(source_type: &LemmaType) -> String {
    source_type
        .quantity_unit_names()
        .and_then(|names| names.first().map(|u| (*u).to_string()))
        .unwrap_or_else(|| "<unit>".to_string())
}

fn lookup_unit_type(
    resolved_types: &ResolvedTypesMap,
    spec_arc: &Arc<LemmaSpec>,
    unit_name: &str,
) -> Option<Arc<LemmaType>> {
    find_types_by_spec(resolved_types, spec_arc)
        .and_then(|dt| dt.unit_index.get(unit_name))
        .map(Arc::clone)
}

fn is_valid_range_span_unit(
    source_type: &LemmaType,
    unit_name: &str,
    unit_index: &HashMap<String, Arc<LemmaType>>,
) -> bool {
    let Some(target_type) = unit_index.get(unit_name) else {
        return false;
    };
    if source_type.is_date_range() {
        return target_type.is_duration_like() || target_type.is_calendar_like();
    }
    if source_type.is_time_range() {
        return target_type.is_duration_like();
    }
    if source_type.is_quantity_range() {
        if source_type
            .quantity_unit_names()
            .is_some_and(|names| names.contains(&unit_name))
        {
            return target_type.is_quantity();
        }
        let TypeSpecification::QuantityRange { decomposition, .. } = &source_type.specifications
        else {
            unreachable!("BUG: is_quantity_range without QuantityRange spec");
        };
        let Some(source_decomp) = decomposition else {
            return false;
        };
        return target_type.is_quantity()
            && target_type
                .quantity_type_decomposition()
                .is_some_and(|td| td == source_decomp);
    }
    if source_type.is_ratio_range() {
        return target_type.is_ratio();
    }
    false
}

fn check_unit_conversion_types(
    graph: &Graph,
    source: &Expression,
    source_type: &LemmaType,
    target: &SemanticConversionTarget,
    resolved_types: &ResolvedTypesMap,
    source_loc: &Source,
    spec_arc: &Arc<LemmaSpec>,
) -> Result<(), Vec<Error>> {
    if source_type.vetoed() {
        return Ok(());
    }
    let unit_index = find_types_by_spec(resolved_types, spec_arc)
        .map(|dt| &dt.unit_index)
        .expect("BUG: spec types missing during unit conversion check");

    match target {
        SemanticConversionTarget::Type(PrimitiveKind::Text) => Ok(()),
        SemanticConversionTarget::Type(PrimitiveKind::Boolean) => {
            if source_type.is_boolean() {
                Ok(())
            } else {
                Err(vec![engine_error_at_graph(
                    graph,
                    source_loc,
                    format!("Cannot convert {} to boolean.", source_type.name()),
                )])
            }
        }
        SemanticConversionTarget::Unit { unit_name } => {
            if source_type.is_number() {
                if unit_index.contains_key(unit_name) {
                    return Ok(());
                }
                return Err(vec![engine_error_at_graph(
                    graph,
                    source_loc,
                    format!("Unknown unit '{unit_name}' in spec '{}'.", spec_arc.name),
                )]);
            }
            if source_type.is_quantity() {
                let Some(target_type) = lookup_unit_type(resolved_types, spec_arc, unit_name)
                else {
                    return Err(vec![engine_error_at_graph(
                        graph,
                        source_loc,
                        format!("Unknown unit '{unit_name}' in spec '{}'.", spec_arc.name),
                    )]);
                };
                if source_type.same_quantity_family(&target_type)
                    || source_type.compatible_with_anonymous_quantity(&target_type)
                    || target_type.compatible_with_anonymous_quantity(source_type)
                {
                    return Ok(());
                }
                if !has_explicit_unit(source) {
                    let expr_label = expression_suggestion_label(source);
                    let first_unit = first_unit_suggestion(source_type);
                    return Err(vec![engine_error_at_graph(
                        graph,
                        source_loc,
                        format!(
                            "Cannot convert '{}' to '{unit_name}' (different quantity families). \
                             Express the source unit first, for example '{expr_label} as {first_unit} as {unit_name}'.",
                            source_type.name()
                        ),
                    )]);
                }
                return Ok(());
            }
            if source_type.is_date_range()
                || source_type.is_time_range()
                || source_type.is_quantity_range()
                || source_type.is_ratio_range()
            {
                if is_valid_range_span_unit(source_type, unit_name, unit_index) {
                    return Ok(());
                }
                return Err(vec![engine_error_at_graph(
                    graph,
                    source_loc,
                    format!(
                        "Cannot convert {} span to unit '{unit_name}'.",
                        source_type.name()
                    ),
                )]);
            }
            if source_type.is_calendar_like() {
                if unit_index.contains_key(unit_name) {
                    return Ok(());
                }
                return Err(vec![engine_error_at_graph(
                    graph,
                    source_loc,
                    format!("Cannot convert calendar to unit '{unit_name}'."),
                )]);
            }
            if source_type.is_ratio() {
                if let TypeSpecification::Ratio { units, .. } = &source_type.specifications {
                    if units.get(unit_name).is_ok() {
                        return Ok(());
                    }
                    let valid: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
                    return Err(vec![engine_error_at_graph(
                        graph,
                        source_loc,
                        format!(
                            "Unknown unit '{unit_name}' for ratio type '{}'. Valid units: {}",
                            source_type.name(),
                            valid.join(", ")
                        ),
                    )]);
                }
                return Err(vec![engine_error_at_graph(
                    graph,
                    source_loc,
                    format!("Cannot convert ratio to unit '{unit_name}'."),
                )]);
            }
            Err(vec![engine_error_at_graph(
                graph,
                source_loc,
                format!(
                    "Cannot convert {} to unit '{unit_name}'.",
                    source_type.name()
                ),
            )])
        }
        SemanticConversionTarget::Type(PrimitiveKind::Number) => {
            if source_type.is_date_range()
                || source_type.is_time_range()
                || source_type.is_quantity_range()
            {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source_loc,
                    format!(
                        "Cannot use 'as number' on a {}. \
                         Express the span in a unit first, for example '<range> as <unit> as number'.",
                        source_type.name()
                    ),
                )]);
            }
            if source_type.is_number_range() {
                return Ok(());
            }
            if source_type.is_ratio_range() || source_type.is_calendar_like_range() {
                return Err(vec![engine_error_at_graph(
                    graph,
                    source_loc,
                    format!("Cannot convert {} to number.", source_type.name()),
                )]);
            }
            if source_type.is_anonymous_quantity() {
                if let Some(decomp) = source_type.quantity_type_decomposition() {
                    if !decomp.is_empty() {
                        return Err(vec![engine_error_at_graph(
                            graph,
                            source_loc,
                            format!(
                                "Cannot use 'as number' to strip an anonymous intermediate with unresolved \
                                 dimensions {:?}. Ensure all dimensions cancel before converting to number.",
                                decomp
                            ),
                        )]);
                    }
                }
            }
            if source_type.is_quantity() && !source_type.is_anonymous_quantity() {
                if !has_explicit_unit(source) {
                    let expr_label = expression_suggestion_label(source);
                    let first_unit = first_unit_suggestion(source_type);
                    return Err(vec![engine_error_at_graph(
                        graph,
                        source_loc,
                        format!(
                            "Cannot use 'as number' on quantity '{}' without a unit. \
                             Express it in a unit first, for example '{expr_label} as {first_unit} as number'.",
                            source_type.name()
                        ),
                    )]);
                }
                return Ok(());
            }
            if source_type.is_quantity()
                || source_type.is_number()
                || source_type.is_duration_like()
                || source_type.is_calendar_like()
                || source_type.is_ratio()
            {
                Ok(())
            } else {
                Err(vec![engine_error_at_graph(
                    graph,
                    source_loc,
                    format!("Cannot convert {} to number.", source_type.name()),
                )])
            }
        }
        SemanticConversionTarget::Type(target_kind)
            if source_type.matches_primitive_kind(*target_kind) =>
        {
            Ok(())
        }
        SemanticConversionTarget::Type(target_kind) => Err(vec![engine_error_at_graph(
            graph,
            source_loc,
            format!(
                "Cannot convert {} to {:?}.",
                source_type.name(),
                target_kind
            ),
        )]),
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
                "Mathematical function requires number operand, got {}",
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
fn check_expression(
    expression: &Expression,
    graph: &Graph,
    inferred_types: &HashMap<RulePath, Arc<LemmaType>>,
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

            // Detect `left OP (inner as Unit{..})` where OP is *, /, or % and check whether
            // the `as Unit` was meant to convert the operand or the result of the arithmetic.
            // When the operand conversion is invalid but the result conversion would be valid,
            // emit a targeted error with a parenthesized suggestion instead of the generic
            // "different quantity families" message that `check_unit_conversion_types` would emit.
            //
            // Three outcomes:
            //   CheckedInlineConversionValid   — conversion on operand is valid; right was
            //                                    checked inline; arithmetic checks must run.
            //   CheckedConversionErrorEmitted  — conversion error (targeted or standard)
            //                                    already emitted; arithmetic checks must be
            //                                    skipped to avoid confusing secondary errors.
            //   NotHandledInline               — pattern did not match; fall through to the
            //                                    normal `check_expression(right)` call.
            enum InlineConversionOutcome {
                CheckedInlineConversionValid,
                CheckedConversionErrorEmitted,
                NotHandledInline,
            }

            let is_multiplicative = matches!(
                operator,
                ArithmeticComputation::Multiply
                    | ArithmeticComputation::Divide
                    | ArithmeticComputation::Modulo
            );

            let right_outcome = if is_multiplicative {
                if let ExpressionKind::UnitConversion(
                    inner_source,
                    SemanticConversionTarget::Unit { unit_name },
                ) = &right.kind
                {
                    // Always recurse into the inner source to catch errors in sub-expressions.
                    collect(
                        check_expression(
                            inner_source,
                            graph,
                            inferred_types,
                            resolved_types,
                            spec_arc,
                        ),
                        &mut errors,
                    );

                    let inner_type = infer_expression_type(
                        inner_source,
                        graph,
                        inferred_types,
                        resolved_types,
                        spec_arc,
                    );
                    let expr_source = expression
                        .source_location
                        .as_ref()
                        .expect("BUG: expression missing source in check_expression");

                    let target_type_opt = lookup_unit_type(resolved_types, spec_arc, unit_name);

                    if let Some(target_type) = &target_type_opt {
                        let inner_is_valid_conversion_source = inner_type.is_quantity()
                            && (inner_type.same_quantity_family(target_type)
                                || inner_type.compatible_with_anonymous_quantity(target_type)
                                || target_type.compatible_with_anonymous_quantity(&inner_type));

                        if inner_is_valid_conversion_source {
                            // The parse tree is correct: `left OP (inner as unit)`.
                            // Run check_unit_conversion_types to catch explicit-unit requirements,
                            // then let arithmetic checks run on the full `left OP right` below.
                            collect(
                                check_unit_conversion_types(
                                    graph,
                                    inner_source,
                                    &inner_type,
                                    &SemanticConversionTarget::Unit {
                                        unit_name: unit_name.clone(),
                                    },
                                    resolved_types,
                                    expr_source,
                                    spec_arc,
                                ),
                                &mut errors,
                            );
                            InlineConversionOutcome::CheckedInlineConversionValid
                        } else if inner_type.is_quantity() || inner_type.is_number() {
                            // Conversion on the operand fails. Check whether converting the
                            // result of the whole arithmetic expression would be valid instead.
                            let left_type = infer_expression_type(
                                left,
                                graph,
                                inferred_types,
                                resolved_types,
                                spec_arc,
                            );
                            let combined_type = compute_arithmetic_result_type(
                                Arc::clone(&left_type),
                                operator,
                                Arc::clone(&inner_type),
                            );
                            let combined_is_valid_conversion_source = combined_type.is_quantity()
                                && (combined_type.same_quantity_family(target_type)
                                    || combined_type
                                        .compatible_with_anonymous_quantity(target_type)
                                    || target_type
                                        .compatible_with_anonymous_quantity(&combined_type));

                            if combined_is_valid_conversion_source {
                                let inner_label = expression_suggestion_label(inner_source);
                                let left_label = expression_suggestion_label(left);
                                errors.push(engine_error_at_graph(
                                    graph,
                                    expr_source,
                                    format!(
                                        "'as {unit_name}' converts '{inner_label}' here, not \
                                         the result of the expression. Write \
                                         '({left_label} {operator} {inner_label}) as {unit_name}' \
                                         to convert the result.",
                                    ),
                                ));
                            } else {
                                // Neither interpretation is valid. Emit the standard conversion
                                // error so the user knows the unit is incompatible.
                                collect(
                                    check_unit_conversion_types(
                                        graph,
                                        inner_source,
                                        &inner_type,
                                        &SemanticConversionTarget::Unit {
                                            unit_name: unit_name.clone(),
                                        },
                                        resolved_types,
                                        expr_source,
                                        spec_arc,
                                    ),
                                    &mut errors,
                                );
                            }
                            InlineConversionOutcome::CheckedConversionErrorEmitted
                        } else {
                            // Inner is not quantity/number (e.g. text, boolean, date). Fall through
                            // to standard path so check_expression(right) runs normally.
                            InlineConversionOutcome::NotHandledInline
                        }
                    } else {
                        // Unknown unit — fall through so check_expression(right) emits the standard
                        // "Unknown unit" error from check_unit_conversion_types.
                        InlineConversionOutcome::NotHandledInline
                    }
                } else {
                    InlineConversionOutcome::NotHandledInline
                }
            } else {
                InlineConversionOutcome::NotHandledInline
            };

            match right_outcome {
                InlineConversionOutcome::NotHandledInline => {
                    collect(
                        check_expression(right, graph, inferred_types, resolved_types, spec_arc),
                        &mut errors,
                    );
                }
                InlineConversionOutcome::CheckedInlineConversionValid
                | InlineConversionOutcome::CheckedConversionErrorEmitted => {
                    // Right child was already checked inline; do not recurse again.
                }
            }

            let run_arithmetic_checks = matches!(
                right_outcome,
                InlineConversionOutcome::NotHandledInline
                    | InlineConversionOutcome::CheckedInlineConversionValid
            );

            if run_arithmetic_checks {
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
                    source_expression,
                    &source_type,
                    target,
                    resolved_types,
                    expr_source,
                    spec_arc,
                ),
                &mut errors,
            );
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
            } else if inferred_range_type.is_time_range() {
                if let (ExpressionKind::Literal(left_lit), ExpressionKind::Literal(right_lit)) =
                    (&left.kind, &right.kind)
                {
                    if let (ValueKind::Time(left_time), ValueKind::Time(right_time)) =
                        (&left_lit.value, &right_lit.value)
                    {
                        if !time_range_endpoints_share_timezone(left_time, right_time) {
                            errors.push(engine_error_at_graph(
                                graph,
                                expr_source,
                                "Time range endpoints must use the same timezone".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        ExpressionKind::PastFutureRange(_, offset_expr) => {
            collect(
                check_expression(offset_expr, graph, inferred_types, resolved_types, spec_arc),
                &mut errors,
            );

            let offset_type =
                infer_expression_type(offset_expr, graph, inferred_types, resolved_types, spec_arc);
            if !offset_type.is_duration_like() && !offset_type.is_calendar_like() {
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
                    || (range_type.is_time_range() && value_type.is_time())
                    || (range_type.is_number_range() && value_type.is_number())
                    || (range_type.is_quantity_range()
                        && value_type.is_quantity()
                        && quantity_range_matches_quantity(&range_type, &value_type))
                    || (range_type.is_ratio_range() && value_type.is_ratio())
                    || (range_type.is_calendar_like_range() && value_type.is_calendar_like());
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

        ExpressionKind::Piecewise(arms) => {
            for (condition, result) in arms {
                collect(
                    check_expression(condition, graph, inferred_types, resolved_types, spec_arc),
                    &mut errors,
                );
                collect(
                    check_expression(result, graph, inferred_types, resolved_types, spec_arc),
                    &mut errors,
                );
                let condition_type = infer_expression_type(
                    condition,
                    graph,
                    inferred_types,
                    resolved_types,
                    spec_arc,
                );
                if !condition_type.is_boolean() {
                    let expr_source = condition
                        .source_location
                        .as_ref()
                        .expect("BUG: expression missing source in check_expression");
                    errors.push(engine_error_at_graph(
                        graph,
                        expr_source,
                        format!(
                            "Piecewise condition must be boolean, got type '{}'",
                            condition_type.name()
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
    inferred_types: &HashMap<RulePath, Arc<LemmaType>>,
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

        if default_type.is_anonymous_quantity() {
            if let Some(decomp) = default_type.quantity_type_decomposition() {
                if !decomp.is_empty() && anonymous_rule_boundary_requires_rejection() {
                    let default_source = default_result
                        .source_location
                        .as_ref()
                        .expect("BUG: default branch result expression has no source location");
                    errors.push(engine_error_at_graph(
                        graph,
                        default_source,
                        anonymous_rule_boundary_error(
                            rule_path,
                            spec_arc,
                            resolved_types,
                            decomp,
                            None,
                        ),
                    ));
                }
            }
        }

        let mut non_veto_type: Option<LemmaType> = None;
        if !default_type.vetoed() && !default_type.is_undetermined() {
            non_veto_type = Some(default_type.as_ref().clone());
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
                            "Unless clause condition in rule '{}' must be boolean, got {}",
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

            if result_type.is_anonymous_quantity() {
                if let Some(decomp) = result_type.quantity_type_decomposition() {
                    if !decomp.is_empty() && anonymous_rule_boundary_requires_rejection() {
                        let branch_source = result
                            .source_location
                            .as_ref()
                            .expect("BUG: unless branch result expression has no source location");
                        errors.push(engine_error_at_graph(
                            graph,
                            branch_source,
                            anonymous_rule_boundary_error(
                                rule_path,
                                spec_arc,
                                resolved_types,
                                decomp,
                                Some(branch_index),
                            ),
                        ));
                    }
                }
            }

            if !result_type.vetoed() && !result_type.is_undetermined() {
                if non_veto_type.is_none() {
                    non_veto_type = Some(result_type.as_ref().clone());
                } else if let Some(ref existing_type) = non_veto_type {
                    if !existing_type.has_same_base_type(result_type.as_ref()) {
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
fn apply_inferred_types(graph: &mut Graph, inferred_types: HashMap<RulePath, Arc<LemmaType>>) {
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
) -> HashMap<RulePath, Arc<LemmaType>> {
    let mut computed_types: HashMap<RulePath, Arc<LemmaType>> = HashMap::new();

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

        let mut non_veto_type: Option<Arc<LemmaType>> = None;
        if !default_type.vetoed() && !default_type.is_undetermined() {
            non_veto_type = Some(default_type);
        }

        for (_branch_index, (_condition, result)) in branches.iter().enumerate().skip(1) {
            let result_type =
                infer_expression_type(result, graph, &computed_types, resolved_types, spec_arc);
            if !result_type.vetoed() && !result_type.is_undetermined() && non_veto_type.is_none() {
                non_veto_type = Some(result_type);
            }
        }

        let rule_type = non_veto_type.unwrap_or_else(|| Arc::new(LemmaType::veto_type()));
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
        TypeSpecification::Quantity { traits, .. }
            if traits.contains(&semantics::QuantityTrait::Calendar) =>
        {
            calendar_decomposition()
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

/// Extract owned `LemmaType` from an `Arc` that the build pipeline holds exclusively.
/// During build, every `Arc<LemmaType>` in `ResolvedSpecTypes.{resolved,unit_index}` has
/// refcount 1 until the value is moved into [`ExecutionPlan::resolved_types`]; `try_unwrap`
/// always succeeds without cloning. The fallback clone is defensive and never executes
/// in normal flow.
fn arc_unwrap(arc: Arc<LemmaType>) -> LemmaType {
    Arc::try_unwrap(arc).unwrap_or_else(|shared| (*shared).clone())
}

fn sync_unit_index_from_resolved(
    resolved: &HashMap<String, Arc<LemmaType>>,
    unit_index: HashMap<String, Arc<LemmaType>>,
) -> HashMap<String, Arc<LemmaType>> {
    unit_index
        .into_iter()
        .map(|(unit_name, pre_decomp_type)| {
            let post = pre_decomp_type
                .name
                .as_deref()
                .or_else(|| pre_decomp_type.quantity_family_name())
                .and_then(|type_name| {
                    resolved.get(type_name).or_else(|| {
                        pre_decomp_type
                            .quantity_family_name()
                            .and_then(|family| resolved.get(family))
                    })
                })
                .map(Arc::clone)
                .unwrap_or(pre_decomp_type);
            (unit_name, post)
        })
        .collect()
}

/// `uses`-merged quantity rows in `unit_index` can still have empty `decomposition` until synced from
/// `resolved`. Compound unit resolution consults `unit_index` first; fill simple base quantitys here
/// before building [`UnitDecompLookup`].
fn repair_empty_simple_quantity_decomposition_in_unit_index(
    unit_index: HashMap<String, Arc<LemmaType>>,
) -> HashMap<String, Arc<LemmaType>> {
    unit_index
        .into_iter()
        .map(|(unit_key, arc)| {
            (
                unit_key,
                Arc::new(repair_simple_quantity_decomposition(arc_unwrap(arc))),
            )
        })
        .collect()
}

fn repair_simple_quantity_decomposition(lemma_type: LemmaType) -> LemmaType {
    let Some(base_decomp) = simple_quantity_repair_decomposition(&lemma_type) else {
        return lemma_type;
    };
    lemma_type.map_quantity(|units, _decomposition| {
        let units = units.map(|u| u.with_decomposition(base_decomp.clone()));
        (units, Some(base_decomp))
    })
}

fn simple_quantity_repair_decomposition(lemma_type: &LemmaType) -> Option<BaseQuantityVector> {
    let TypeSpecification::Quantity {
        units,
        decomposition,
        ..
    } = &lemma_type.specifications
    else {
        return None;
    };
    if decomposition.is_some() {
        return None;
    }
    if units.is_empty() || units.iter().any(|u| !quantity_unit_is_simple(u)) {
        return None;
    }
    let type_name = lemma_type.name.as_deref()?;
    if type_name.is_empty() {
        return None;
    }
    let candidate = declared_quantity_decomposition(type_name, lemma_type);
    (!candidate.is_empty()).then_some(candidate)
}

fn owning_quantity_type_name_for_unit(
    unit_name: &str,
    lookup: &UnitDecompLookup,
    unit_index: &HashMap<String, Arc<LemmaType>>,
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
    resolved: &HashMap<String, Arc<LemmaType>>,
    lookup: &UnitDecompLookup,
    unit_index: &HashMap<String, Arc<LemmaType>>,
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

/// Build the reverse signature index over every named-quantity-type unit in the spec.
/// Each unit's canonical-form `derived_quantity_factors` becomes a key; the value is the
/// owning unit name and type. An ambiguity (the same canonical key matched by two units in
/// distinct types) is a planning error: the spec must rename one of the units or change
/// its factor decomposition so it is unique.
pub(crate) fn build_signature_index(
    spec_name: &str,
    unit_index: &HashMap<String, Arc<LemmaType>>,
) -> Result<crate::computation::arithmetic::SignatureIndex, Error> {
    use crate::computation::arithmetic::SignatureIndex;
    let mut signature_index: SignatureIndex = SignatureIndex::new();
    for (unit_name, lemma_type) in unit_index.iter() {
        let TypeSpecification::Quantity { units, .. } = &lemma_type.specifications else {
            continue;
        };
        let Ok(unit) = units.get(unit_name) else {
            continue;
        };
        if unit.derived_quantity_factors.is_empty() {
            continue;
        }
        let owning_type_name = lemma_type.name.clone().unwrap_or_default();
        let signature = unit.derived_quantity_factors.clone();
        match signature_index.get(&signature) {
            Some((existing_unit_name, existing_owning_type))
                if existing_owning_type.name() != lemma_type.name() =>
            {
                let existing_type_name = existing_owning_type.name.clone().unwrap_or_default();
                return Err(Error::validation(
                    format!(
                        "In spec '{}': ambiguous unit signature {:?}: matched by '{}' in type '{}' and '{}' in type '{}'. \
                         Rename one or differentiate factors.",
                        spec_name,
                        signature,
                        existing_unit_name,
                        existing_type_name,
                        unit_name,
                        owning_type_name,
                    ),
                    None::<Source>,
                    None::<String>,
                ));
            }
            _ => {
                signature_index.insert(signature, (unit_name.clone(), Arc::clone(lemma_type)));
            }
        }
    }
    Ok(signature_index)
}

/// Rebuild [`QuantityRange`] (and other range) specs for `ParentType::Ranged` declarations
/// after the decomposition pass has populated parent quantity types.
fn refresh_named_range_specs(
    spec: &Arc<LemmaSpec>,
    data_defs: &HashMap<String, DataTypeDef>,
    resolved: &mut TypeMap,
    declared_defaults: &mut HashMap<String, ValueKind>,
) -> Vec<Error> {
    let mut errors = Vec::new();
    for (type_name, def) in data_defs {
        let ParentType::Ranged { inner } = &def.parent else {
            continue;
        };
        let element = element_parent_type(&def.parent);
        let element_type = match element {
            ParentType::Custom { name } => resolved.get(name.as_str()),
            ParentType::Qualified {
                spec_alias,
                inner: qualified_inner,
            } => {
                let ParentType::Custom { name } = qualified_inner.as_ref() else {
                    continue;
                };
                let import_name = format!("{spec_alias}.{name}");
                resolved
                    .get(name.as_str())
                    .or_else(|| resolved.get(import_name.as_str()))
            }
            ParentType::Primitive { primitive } => {
                let _ = primitive;
                continue;
            }
            ParentType::Ranged { .. } => {
                unreachable!("BUG: element_parent_type must unwrap Ranged")
            }
        };
        let Some(element_type) = element_type else {
            let missing_name = match element {
                ParentType::Custom { name } => name.clone(),
                ParentType::Qualified {
                    spec_alias,
                    inner: qualified_inner,
                } => {
                    let ParentType::Custom { name } = qualified_inner.as_ref() else {
                        continue;
                    };
                    format!("{spec_alias}.{name}")
                }
                _ => continue,
            };
            errors.push(Error::validation_with_context(
                format!(
                    "In spec '{}': ranged type '{}' references missing element type '{}'",
                    spec.name, type_name, missing_name
                ),
                Some(def.source.clone()),
                None::<String>,
                Some(Arc::clone(spec)),
                None,
            ));
            continue;
        };
        let endpoint_type = Arc::clone(element_type);
        let Some(range_spec) = endpoint_type.specifications.range_from_element() else {
            continue;
        };
        let Some(lemma_type) = resolved.get_mut(type_name.as_str()) else {
            continue;
        };
        let mut updated = lemma_type.as_ref().clone();
        updated.specifications = range_spec;
        *lemma_type = Arc::new(updated);

        if let Some(ValueKind::Range(left, right)) = declared_defaults.get_mut(type_name.as_str()) {
            let coerced_left = Graph::coerce_literal_to_schema_type(left.as_ref(), &endpoint_type)
                .unwrap_or_else(|message| {
                    panic!(
                        "BUG: coercing named range default left endpoint for '{}': {}",
                        type_name, message
                    )
                });
            let coerced_right =
                Graph::coerce_literal_to_schema_type(right.as_ref(), &endpoint_type)
                    .unwrap_or_else(|message| {
                        panic!(
                            "BUG: coercing named range default right endpoint for '{}': {}",
                            type_name, message
                        )
                    });
            *declared_defaults
                .get_mut(type_name.as_str())
                .expect("BUG: named range default removed while refreshing endpoints") =
                ValueKind::Range(Box::new(coerced_left), Box::new(coerced_right));
        }

        let _ = inner;
        let _ = spec;
    }
    errors
}

type TypeMap = HashMap<String, Arc<LemmaType>>;

fn resolve_quantity_decompositions(
    spec_name: &str,
    mut resolved: TypeMap,
    mut unit_index: TypeMap,
    type_sources: &HashMap<String, Source>,
) -> (TypeMap, TypeMap, Vec<Error>) {
    let mut errors: Vec<Error> = Vec::new();

    let source_for = |type_name: &str| -> Option<Source> { type_sources.get(type_name).cloned() };

    let base_type_names: Vec<String> = resolved
        .iter()
        .filter_map(|(name, lt)| {
            if let TypeSpecification::Quantity { units, .. } = &lt.specifications {
                if units.iter().all(quantity_unit_is_simple) {
                    return Some(name.clone());
                }
            }
            None
        })
        .collect();

    for type_name in &base_type_names {
        let owned = arc_unwrap(
            resolved
                .remove(type_name)
                .expect("BUG: type_name comes from resolved's own keys"),
        );
        let base_decomp = declared_quantity_decomposition(type_name, &owned);

        let reserved_collision = reserved_calendar_units_in_quantity(&owned);
        if !reserved_collision.is_empty() && !owned.has_trait_calendar() {
            errors.push(Error::validation(
                format!(
                    "In spec '{}': quantity type '{}' declares unit(s) {:?} which are reserved calendar unit names.",
                    spec_name, type_name, reserved_collision
                ),
                source_for(type_name),
                None::<String>,
            ));
            resolved.insert(type_name.clone(), Arc::new(owned));
            continue;
        }

        let updated = owned.map_quantity(|units, _decomposition| {
            let units = units.map(|u| u.with_decomposition(base_decomp.clone()));
            (units, Some(base_decomp.clone()))
        });
        resolved.insert(type_name.clone(), Arc::new(updated));
    }

    unit_index = repair_empty_simple_quantity_decomposition_in_unit_index(unit_index);

    let mut lookup = UnitDecompLookup::new();

    for (unit_name, lemma_type) in unit_index.iter() {
        if let TypeSpecification::Quantity {
            decomposition: Some(decomposition),
            units,
            ..
        } = &lemma_type.specifications
        {
            let quantity_name = lemma_type.name.clone().unwrap_or_default();
            let factor = units
                .iter()
                .find(|u| &u.name == unit_name)
                .unwrap_or_else(|| {
                    panic!(
                        "BUG: unit_name '{}' from unit_index must be in its owning type's units",
                        unit_name
                    )
                })
                .factor
                .clone();
            lookup.insert(
                unit_name.clone(),
                (quantity_name, decomposition.clone(), factor),
            );
        }
    }

    for (type_name, lemma_type) in resolved.iter() {
        if let TypeSpecification::Quantity {
            units,
            decomposition: Some(decomposition),
            ..
        } = &lemma_type.specifications
        {
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
                    (
                        type_name.clone(),
                        decomposition.clone(),
                        unit.factor.clone(),
                    ),
                );
            }
        }
    }

    let derived_quantity_type_names_unsorted: Vec<String> = resolved
        .iter()
        .filter_map(|(name, lemma_type)| {
            if let TypeSpecification::Quantity { units, .. } = &lemma_type.specifications {
                if units.iter().any(|unit| !quantity_unit_is_simple(unit)) {
                    return Some(name.clone());
                }
            }
            None
        })
        .collect();

    let derived_quantity_type_names = match sort_derived_quantity_types_for_resolution(
        spec_name,
        derived_quantity_type_names_unsorted,
        &resolved,
        &lookup,
        &unit_index,
        &|type_name| source_for(type_name),
    ) {
        Ok(sorted) => sorted,
        Err(error) => {
            errors.push(error);
            return (resolved, unit_index, errors);
        }
    };

    for type_name in &derived_quantity_type_names {
        let type_source = source_for(type_name);

        let units_snapshot = match &resolved[type_name].specifications {
            TypeSpecification::Quantity { units, .. } => units.clone(),
            _ => continue,
        };

        let mut resolved_type_decomp: Option<BaseQuantityVector> = None;
        let mut unit_errors: Vec<Error> = Vec::new();
        let mut resolved_unit_factors: Vec<Option<crate::computation::rational::RationalInteger>> =
            vec![None; units_snapshot.len()];

        for (unit_idx, unit) in units_snapshot.iter().enumerate() {
            if crate::planning::semantics::calendar_unit_factor(&unit.name).is_some()
                && !resolved[type_name].has_trait_calendar()
            {
                unit_errors.push(Error::validation(
                    format!(
                        "In spec '{}': quantity type '{}' declares unit '{}' which is a reserved calendar unit name.",
                        spec_name, type_name, unit.name
                    ),
                    type_source.clone(),
                    None::<String>,
                ));
                continue;
            }
            if quantity_unit_is_simple(unit) {
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

                resolved_unit_factors[unit_idx] = Some(unit.factor.clone());
                continue;
            }

            match resolve_compound_unit(
                spec_name,
                type_name,
                &unit.name,
                unit.factor.clone(),
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

        let owned = arc_unwrap(
            resolved
                .remove(type_name)
                .expect("BUG: type_name comes from resolved's own keys"),
        );

        let updated = owned.map_quantity(|units, _decomposition| {
            let units = QuantityUnits(
                units
                    .0
                    .into_iter()
                    .enumerate()
                    .map(|(idx, u)| {
                        let u = u.with_decomposition(type_decomp.clone());
                        match resolved_unit_factors[idx].clone() {
                            Some(factor) => u.with_factor(factor.clone()),
                            None => u,
                        }
                    })
                    .collect(),
            );
            (units, Some(type_decomp.clone()))
        });

        if let TypeSpecification::Quantity { units, .. } = &updated.specifications {
            for unit in units.iter() {
                lookup.insert(
                    unit.name.clone(),
                    (type_name.clone(), type_decomp.clone(), unit.factor.clone()),
                );
            }
        }
        resolved.insert(type_name.clone(), Arc::new(updated));
    }

    let resolved = canonicalize_unit_signatures(resolved);
    let unit_index = sync_unit_index_from_resolved(&resolved, unit_index);
    let unit_index = repair_empty_simple_quantity_decomposition_in_unit_index(unit_index);
    let unit_index = canonicalize_unit_signatures(unit_index);

    (resolved, unit_index, errors)
}

/// Functional wrapper around the still-`&mut`-based
/// [`semantics::finalize_quantity_unit_constraint_magnitudes`]. Consumes the
/// `LemmaType`, performs the validation, and returns either the updated value
/// or an error message (the type is dropped on failure).
fn finalize_lemma_quantity_magnitudes(
    lemma_type: LemmaType,
    declared_default: Option<&ValueKind>,
    type_name: &str,
) -> Result<LemmaType, String> {
    let LemmaType {
        name,
        mut specifications,
        extends,
    } = lemma_type;
    semantics::finalize_quantity_unit_constraint_magnitudes(
        &mut specifications,
        declared_default,
        type_name,
    )?;
    Ok(LemmaType {
        name,
        specifications,
        extends,
    })
}

fn finalize_quantity_magnitudes_in_resolved(
    resolved: HashMap<String, Arc<LemmaType>>,
    declared_defaults: &HashMap<String, ValueKind>,
    type_sources: &HashMap<String, Source>,
    spec_name: &str,
    spec: &Arc<LemmaSpec>,
) -> (HashMap<String, Arc<LemmaType>>, Vec<Error>) {
    resolved.into_iter().fold(
        (HashMap::new(), Vec::new()),
        |(mut acc, mut errors), (type_name, arc)| {
            let source = type_sources.get(&type_name).cloned().unwrap_or_else(|| {
                unreachable!(
                    "BUG: resolved type '{}' has no corresponding DataTypeDef in spec '{}'",
                    type_name, spec_name
                )
            });
            let fallback = (*arc).clone();
            let lemma_type = match finalize_lemma_quantity_magnitudes(
                arc_unwrap(arc),
                declared_defaults.get(&type_name),
                &type_name,
            ) {
                Ok(lt) => lt,
                Err(message) => {
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
                    fallback
                }
            };
            acc.insert(type_name, Arc::new(lemma_type));
            (acc, errors)
        },
    )
}

fn finalize_quantity_magnitudes_in_unit_index(
    unit_index: HashMap<String, Arc<LemmaType>>,
    declared_defaults: &HashMap<String, ValueKind>,
    type_sources: &HashMap<String, Source>,
    all_data_types: &[(Arc<LemmaSpec>, HashMap<String, DataTypeDef>)],
    spec: &Arc<LemmaSpec>,
) -> (HashMap<String, Arc<LemmaType>>, Vec<Error>) {
    unit_index.into_iter().fold(
        (HashMap::new(), Vec::new()),
        |(mut acc, mut errors), (unit_name, arc)| {
            let type_name_opt = arc
                .name
                .as_deref()
                .or_else(|| arc.quantity_family_name())
                .map(str::to_string);
            let Some(type_name) = type_name_opt else {
                acc.insert(unit_name, arc);
                return (acc, errors);
            };
            if !arc.is_quantity() {
                acc.insert(unit_name, arc);
                return (acc, errors);
            }
            let fallback = (*arc).clone();
            let lemma_type = match finalize_lemma_quantity_magnitudes(
                arc_unwrap(arc),
                declared_defaults.get(type_name.as_str()),
                type_name.as_str(),
            ) {
                Ok(lt) => lt,
                Err(message) => {
                    let source = type_sources
                        .get(type_name.as_str())
                        .cloned()
                        .or_else(|| {
                            all_data_types.iter().find_map(|(_, defs)| {
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
                    fallback
                }
            };
            acc.insert(unit_name, Arc::new(lemma_type));
            (acc, errors)
        },
    )
}

/// Populate every unit's `derived_quantity_factors` to its canonical symbolic signature.
///
/// - Compound declarations keep their parsed factors, canonicalized.
/// - Simple units (empty `derived_quantity_factors`) get `[(unit.name, 1)]`.
///
/// After this pass every unit carries a non-empty, canonical-form signature so cross-type
/// arithmetic can combine them deterministically and `signature_index` can be built.
fn canonicalize_unit_signatures(
    types: HashMap<String, Arc<LemmaType>>,
) -> HashMap<String, Arc<LemmaType>> {
    types
        .into_iter()
        .map(|(name, arc)| {
            (
                name,
                Arc::new(canonicalize_lemma_unit_signatures(arc_unwrap(arc))),
            )
        })
        .collect()
}

fn canonicalize_lemma_unit_signatures(lemma_type: LemmaType) -> LemmaType {
    lemma_type.map_quantity(|units, decomposition| {
        let units = units.map(canonicalize_unit_for_quantity);
        (units, decomposition)
    })
}

fn reserved_calendar_units_in_quantity(lemma_type: &LemmaType) -> Vec<String> {
    let TypeSpecification::Quantity { units, .. } = &lemma_type.specifications else {
        return Vec::new();
    };
    units
        .iter()
        .filter(|u| crate::planning::semantics::calendar_unit_factor(&u.name).is_some())
        .map(|u| u.name.clone())
        .collect()
}

fn quantity_unit_is_simple(unit: &QuantityUnit) -> bool {
    unit.derived_quantity_factors.is_empty()
        || (unit.derived_quantity_factors.len() == 1
            && unit.derived_quantity_factors[0].0 == unit.name
            && unit.derived_quantity_factors[0].1 == 1)
}

fn canonicalize_unit_for_quantity(unit: QuantityUnit) -> QuantityUnit {
    let factors = if quantity_unit_is_simple(&unit) {
        vec![(unit.name.clone(), 1)]
    } else {
        canonicalize_signature(&unit.derived_quantity_factors)
    };
    unit.with_derived_quantity_factors(factors)
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
        match Graph::build(&ctx, &repository, &main_spec_arc, &dag, &effective, false) {
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
        // Source branches preserve RulePath; normalized_expression must inline deps and must not.
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
#[derive(Debug, Clone, Default)]
pub struct ResolvedSpecTypes {
    /// Resolved [`LemmaType`] for each **data type row name** declared in this spec (`data name: …`).
    /// Planning-only: includes quantity units and post-`resolve_quantity_decompositions` decomposition.
    pub resolved: HashMap<String, Arc<LemmaType>>,

    /// Declared default per named type (e.g. `type rate: ratio -> default 50%`).
    /// Only present for types that declared a `-> default ...` constraint anywhere
    /// in their extension chain; the inner-most `-> default` wins. Defaults live
    /// outside [`TypeSpecification`] so the type itself stays free of binding data.
    /// Populated after [`RawDefault`] materialization (post-decomposition).
    pub declared_defaults: HashMap<String, ValueKind>,

    /// Defaults captured during type resolution, before quantity unit factors are final.
    pub(crate) raw_defaults: Vec<(String, RawDefault)>,

    /// Raw defaults retained after materialization for cross-spec parent lookup during later specs.
    pub(crate) source_defaults: HashMap<String, RawDefault>,

    /// Unit index: unit_name -> resolved type.
    /// Built during resolution — if unit appears in multiple types, resolution fails.
    pub unit_index: HashMap<String, Arc<LemmaType>>,
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
    /// Specs for which [`TypeResolver::register_all`] could not register every
    /// type-declaring data row. Planning must skip building these specs:
    /// downstream code (`add_data`) relies on every type-resolving data row
    /// being present in the resolved type map.
    specs_with_failed_registration: Vec<Arc<LemmaSpec>>,
}

/// Parent type used for quantity-family lookup, unwrapping [`ParentType::Ranged`].
fn element_parent_type(parent: &ParentType) -> &ParentType {
    match parent {
        ParentType::Ranged { inner } => element_parent_type(inner),
        other => other,
    }
}

fn time_range_endpoints_share_timezone(
    left: &semantics::SemanticTime,
    right: &semantics::SemanticTime,
) -> bool {
    match (&left.timezone, &right.timezone) {
        (None, None) => true,
        (Some(left_tz), Some(right_tz)) => {
            left_tz.offset_hours == right_tz.offset_hours
                && left_tz.offset_minutes == right_tz.offset_minutes
        }
        _ => false,
    }
}

/// Display name for a parser literal's base type, matching the names produced
/// by [`LemmaType::name`] for the corresponding semantic literal. Used in
/// range endpoint validation error messages.
fn parser_value_base_type_display_name(value: &ast::Value) -> &'static str {
    match value {
        ast::Value::Number(_) => "number",
        ast::Value::Text(_) => "text",
        ast::Value::Boolean(_) => "boolean",
        ast::Value::Date(_) => "date",
        ast::Value::Time(_) => "time",
        ast::Value::NumberWithUnit(_, unit) => match unit.as_str() {
            "percent" | "permille" => "ratio",
            _ => "quantity",
        },
        ast::Value::Range(_, _) => "range",
    }
}

/// Infer primitive [`ParentType`] from a literal RHS (`data x: 3.14`).
///
/// Returns an error for range literals whose endpoint types are not a
/// supported range element combination (e.g. `1 ... yes`, `"a" ... "b"`).
fn inferred_parent_type_from_literal(value: &ast::Value) -> Result<ParentType, String> {
    let parent_type = match value {
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
        ast::Value::Range(left, right) => {
            let primitive = match (left.as_ref(), right.as_ref()) {
                (ast::Value::Number(_), ast::Value::Number(_)) => PrimitiveKind::NumberRange,
                (ast::Value::Date(_), ast::Value::Date(_)) => PrimitiveKind::DateRange,
                (ast::Value::Time(_), ast::Value::Time(_)) => PrimitiveKind::TimeRange,
                (ast::Value::NumberWithUnit(_, u1), ast::Value::NumberWithUnit(_, u2))
                    if u1 == u2 && matches!(u1.as_str(), "percent" | "permille") =>
                {
                    PrimitiveKind::RatioRange
                }
                (ast::Value::NumberWithUnit(_, _), ast::Value::NumberWithUnit(_, _)) => {
                    PrimitiveKind::QuantityRange
                }
                (left_value, right_value) => {
                    return Err(format!(
                        "range endpoints must have the same supported base type, got {} and {}",
                        parser_value_base_type_display_name(left_value),
                        parser_value_base_type_display_name(right_value)
                    ));
                }
            };
            ParentType::Primitive { primitive }
        }
    };
    Ok(parent_type)
}

impl<'a> TypeResolver<'a> {
    pub fn new(context: &'a Context) -> Self {
        TypeResolver {
            data_types: Vec::new(),
            context,
            all_registered_specs: Vec::new(),
            specs_with_failed_registration: Vec::new(),
        }
    }

    pub fn is_registered(&self, spec: &Arc<LemmaSpec>) -> bool {
        self.all_registered_specs
            .iter()
            .any(|(_, s)| Arc::ptr_eq(s, spec))
    }

    /// Record that a type-declaring data row of `spec` could not be registered.
    fn mark_registration_failure(&mut self, spec: &Arc<LemmaSpec>) {
        if !self
            .specs_with_failed_registration
            .iter()
            .any(|s| Arc::ptr_eq(s, spec))
        {
            self.specs_with_failed_registration.push(Arc::clone(spec));
        }
    }

    /// Whether [`TypeResolver::register_all`] failed to register a
    /// type-declaring data row of `spec`. The corresponding errors were
    /// returned by `register_all`; specs with failed registrations must not
    /// be built.
    pub fn registration_failed(&self, spec: &Arc<LemmaSpec>) -> bool {
        self.specs_with_failed_registration
            .iter()
            .any(|s| Arc::ptr_eq(s, spec))
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
                        (None, Some(v)) => match inferred_parent_type_from_literal(v) {
                            Ok(parent) => parent,
                            Err(message) => {
                                errors.push(Error::validation_with_context(
                                    message,
                                    Some(data.source_location.clone()),
                                    None::<String>,
                                    Some(Arc::clone(spec)),
                                    None,
                                ));
                                self.mark_registration_failure(spec);
                                continue;
                            }
                        },
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
                            self.mark_registration_failure(spec);
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
                ParsedDataValue::With(_) | ParsedDataValue::Import(_) => {}
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
        already_resolved: &ResolvedTypesMap,
    ) -> Result<ResolvedSpecTypes, Vec<Error>> {
        let mut resolved_types = self.resolve_types_internal(spec, at, already_resolved)?;
        resolved_types.source_defaults = resolved_types
            .raw_defaults
            .iter()
            .map(|(name, raw)| (name.clone(), raw.clone()))
            .collect();
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
        let (new_resolved, new_unit_index, decomp_errors) = resolve_quantity_decompositions(
            &spec.name,
            std::mem::take(&mut resolved_types.resolved),
            std::mem::take(&mut resolved_types.unit_index),
            &type_sources,
        );
        resolved_types.resolved = new_resolved;
        resolved_types.unit_index = new_unit_index;
        errors.extend(decomp_errors);

        for (type_name, raw) in std::mem::take(&mut resolved_types.raw_defaults) {
            let lemma_type = resolved_types
                .resolved
                .get(&type_name)
                .expect("BUG: raw default for type not in resolved");
            match materialize_raw_default(raw, &lemma_type.specifications, &type_name) {
                Ok(value_kind) => {
                    resolved_types
                        .declared_defaults
                        .insert(type_name, value_kind);
                }
                Err(message) => {
                    let source = type_sources
                        .get(&type_name)
                        .cloned()
                        .unwrap_or_else(|| unreachable!("BUG: type '{}' has no source", type_name));
                    errors.push(Error::validation_with_context(
                        message,
                        Some(source),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    ));
                }
            }
        }

        if let Some((_, data_defs)) = self.data_types.iter().find(|(s, _)| Arc::ptr_eq(s, spec)) {
            errors.extend(refresh_named_range_specs(
                spec,
                data_defs,
                &mut resolved_types.resolved,
                &mut resolved_types.declared_defaults,
            ));
        }

        if let Err(error) = build_signature_index(&spec.name, &resolved_types.unit_index) {
            errors.push(error);
        }

        let (new_resolved, resolved_errors) = finalize_quantity_magnitudes_in_resolved(
            std::mem::take(&mut resolved_types.resolved),
            &resolved_types.declared_defaults,
            &type_sources,
            &spec.name,
            spec,
        );
        resolved_types.resolved = new_resolved;

        resolved_types.unit_index = sync_unit_index_from_resolved(
            &resolved_types.resolved,
            std::mem::take(&mut resolved_types.unit_index),
        );

        let (new_unit_index, unit_index_errors) = finalize_quantity_magnitudes_in_unit_index(
            std::mem::take(&mut resolved_types.unit_index),
            &resolved_types.declared_defaults,
            &type_sources,
            &self.data_types,
            spec,
        );
        resolved_types.unit_index = new_unit_index;

        if !resolved_errors.is_empty() || !unit_index_errors.is_empty() {
            errors.extend(resolved_errors);
            errors.extend(unit_index_errors);
            return Err(errors);
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
        already_resolved: &ResolvedTypesMap,
    ) -> Result<ResolvedSpecTypes, Vec<Error>> {
        let data_defs = self
            .data_types
            .iter()
            .find(|(s, _)| Arc::ptr_eq(s, spec))
            .map(|(_, defs)| defs)
            .cloned()
            .unwrap_or_default();

        let type_names: Vec<String> = data_defs.keys().cloned().collect();
        let sorted_type_names = if type_names.is_empty() {
            type_names
        } else {
            let type_index: HashMap<&str, usize> = type_names
                .iter()
                .enumerate()
                .map(|(index, name)| (name.as_str(), index))
                .collect();
            let type_count = type_names.len();
            let mut in_degree = vec![0usize; type_count];
            let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); type_count];
            for (child_index, type_name) in type_names.iter().enumerate() {
                let custom_parent = match &data_defs[type_name].parent {
                    ParentType::Ranged { inner } => match element_parent_type(inner) {
                        ParentType::Custom { name } => Some(name.as_str()),
                        _ => None,
                    },
                    ParentType::Custom { name } => Some(name.as_str()),
                    _ => None,
                };
                let Some(parent_name) = custom_parent else {
                    continue;
                };
                let Some(&parent_index) = type_index.get(parent_name) else {
                    continue;
                };
                if parent_index == child_index {
                    continue;
                }
                in_degree[child_index] += 1;
                dependents[parent_index].push(child_index);
            }
            let mut queue: std::collections::VecDeque<usize> = (0..type_count)
                .filter(|&index| in_degree[index] == 0)
                .collect();
            let mut sorted = Vec::with_capacity(type_count);
            while let Some(index) = queue.pop_front() {
                sorted.push(type_names[index].clone());
                for &dependent_index in &dependents[index] {
                    in_degree[dependent_index] -= 1;
                    if in_degree[dependent_index] == 0 {
                        queue.push_back(dependent_index);
                    }
                }
            }
            if sorted.len() != type_count {
                let cycle_type_name = type_names
                    .iter()
                    .find(|name| in_degree[type_index[name.as_str()]] > 0)
                    .expect("BUG: incomplete topo sort without cycle participant");
                return Err(vec![Error::validation_with_context(
                    format!(
                        "Circular dependency detected in type resolution: {}::{}",
                        spec.name, cycle_type_name
                    ),
                    Some(data_defs[cycle_type_name].source.clone()),
                    None::<String>,
                    Some(Arc::clone(spec)),
                    None,
                )]);
            }
            sorted
        };

        let mut resolved: HashMap<String, (Arc<LemmaType>, Option<RawDefault>)> = HashMap::new();

        fn lookup_parent_type(
            resolver: &TypeResolver<'_>,
            spec: &Arc<LemmaSpec>,
            parent: &ParentType,
            source: &Source,
            at: &EffectiveDate,
            already_resolved: &ResolvedTypesMap,
            resolved: &HashMap<String, (Arc<LemmaType>, Option<RawDefault>)>,
        ) -> Result<(TypeSpecification, Option<RawDefault>, Arc<LemmaType>), Vec<Error>> {
            match parent {
                ParentType::Ranged { inner } => {
                    let (element_specs, element_default, element_type) = lookup_parent_type(
                        resolver,
                        spec,
                        inner,
                        source,
                        at,
                        already_resolved,
                        resolved,
                    )?;
                    let range_spec = element_specs.range_from_element().ok_or_else(|| {
                        vec![Error::validation_with_context(
                            format!(
                                "'{inner}' is not rangeable: only quantity, number, ratio, date, and time types support ranges"
                            ),
                            Some(source.clone()),
                            None::<String>,
                            Some(Arc::clone(spec)),
                            None,
                        )]
                    })?;
                    Ok((range_spec, element_default, element_type))
                }
                ParentType::Primitive { primitive: kind } => {
                    let lemma_type = Arc::new(LemmaType::primitive(
                        semantics::type_spec_for_primitive(*kind),
                    ));
                    Ok((lemma_type.as_ref().specifications.clone(), None, lemma_type))
                }
                ParentType::Custom { name } => {
                    if let Some((parent_type, parent_default)) = resolved.get(name.as_str()) {
                        return Ok((
                            parent_type.as_ref().specifications.clone(),
                            parent_default.clone(),
                            Arc::clone(parent_type),
                        ));
                    }
                    let type_exists = resolver
                        .data_types
                        .iter()
                        .find(|(s, _)| Arc::ptr_eq(s, spec))
                        .map(|(_, type_map)| type_map.contains_key(name.as_str()))
                        .unwrap_or(false);
                    if !type_exists {
                        if spec.data.iter().any(|data| {
                            data.reference.is_local()
                                && data.reference.name == name.as_str()
                                && matches!(&data.value, ParsedDataValue::Import(_))
                        }) {
                            return Err(vec![Error::validation_with_context(
                                format!(
                                    "'{name}' names a spec import alias, not a type: use `data x: {name}.TypeName` after `uses`"
                                ),
                                Some(source.clone()),
                                None::<String>,
                                Some(Arc::clone(spec)),
                                None,
                            )]);
                        }
                        return Err(vec![Error::validation_with_context(
                            format!(
                                "Unknown parent '{parent}' for data definition. Parent must be defined before use. Valid primitive types are: boolean, quantity, number, ratio, text, date, time, duration, percent"
                            ),
                            Some(source.clone()),
                            None::<String>,
                            Some(Arc::clone(spec)),
                            None,
                        )]);
                    }
                    Err(vec![Error::validation_with_context(
                        format!(
                            "Circular dependency detected in type resolution: {}::{}",
                            spec.name, name
                        ),
                        Some(source.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    )])
                }
                ParentType::Qualified { spec_alias, inner } => {
                    let spec_ref = ast::SpecRef::same_repository(spec_alias.clone());
                    let (_, target_spec) =
                        match resolver.resolve_spec_for_import(spec, &spec_ref, source, at) {
                            Ok(import_pair) => import_pair,
                            Err(error) => return Err(vec![error]),
                        };
                    match inner.as_ref() {
                        ParentType::Primitive { primitive: kind } => {
                            let lemma_type = Arc::new(LemmaType::primitive(
                                semantics::type_spec_for_primitive(*kind),
                            ));
                            Ok((lemma_type.as_ref().specifications.clone(), None, lemma_type))
                        }
                        ParentType::Custom { name } => {
                            let Some(resolved_spec_types) = already_resolved
                                .iter()
                                .find(|(_, imported, _)| Arc::ptr_eq(imported, &target_spec))
                                .map(|(_, _, resolved_spec_types)| resolved_spec_types)
                            else {
                                // The import target exists but failed its own type
                                // resolution (its errors were already collected), so
                                // the consumer cannot resolve this qualified type.
                                return Err(vec![Error::validation_with_context(
                                    format!(
                                        "Cannot resolve type '{name}' from spec '{}' (via import '{spec_alias}'): spec '{}' failed type resolution",
                                        target_spec.name, target_spec.name
                                    ),
                                    Some(source.clone()),
                                    None::<String>,
                                    Some(Arc::clone(spec)),
                                    None,
                                )]);
                            };
                            let Some(parent_type) = resolved_spec_types.resolved.get(name.as_str())
                            else {
                                let type_exists = resolver
                                    .data_types
                                    .iter()
                                    .find(|(s, _)| Arc::ptr_eq(s, &target_spec))
                                    .map(|(_, type_map)| type_map.contains_key(name.as_str()))
                                    .unwrap_or(false);
                                if !type_exists {
                                    return Err(vec![Error::validation_with_context(
                                        format!(
                                            "Type '{name}' is not defined in spec '{}' (via import '{spec_alias}')",
                                            target_spec.name
                                        ),
                                        Some(source.clone()),
                                        None::<String>,
                                        Some(Arc::clone(spec)),
                                        None,
                                    )]);
                                }
                                return Err(vec![Error::validation_with_context(
                                    format!(
                                        "Circular dependency detected in type resolution: {}::{}",
                                        target_spec.name, name
                                    ),
                                    Some(source.clone()),
                                    None::<String>,
                                    Some(Arc::clone(spec)),
                                    None,
                                )]);
                            };
                            Ok((
                                parent_type.as_ref().specifications.clone(),
                                resolved_spec_types
                                    .source_defaults
                                    .get(name.as_str())
                                    .cloned(),
                                Arc::clone(parent_type),
                            ))
                        }
                        ParentType::Qualified { .. } => Err(vec![Error::validation_with_context(
                            "Nested qualified parent types are invalid",
                            Some(source.clone()),
                            None::<String>,
                            Some(Arc::clone(spec)),
                            None,
                        )]),
                        ParentType::Ranged { .. } => Err(vec![Error::validation_with_context(
                            "Nested ranged parent types are invalid",
                            Some(source.clone()),
                            None::<String>,
                            Some(Arc::clone(spec)),
                            None,
                        )]),
                    }
                }
            }
        }

        for type_name in &sorted_type_names {
            let ftd = data_defs
                .get(type_name.as_str())
                .expect("BUG: topo-sorted type missing from registry");
            let parent = ftd.parent.clone();
            let constraints = ftd.constraints.clone();

            let (parent_specs, parent_default, parent_type) = lookup_parent_type(
                self,
                spec,
                &parent,
                &ftd.source,
                at,
                already_resolved,
                &resolved,
            )?;

            let mut declared_default = parent_default;
            let final_specs = if let Some(constraints) = &constraints {
                apply_constraints_to_spec(
                    spec,
                    &constraint_application_type_name(&parent, type_name),
                    parent_specs,
                    constraints,
                    &ftd.source,
                    &mut declared_default,
                )?
            } else {
                parent_specs
            };

            let import_target =
                if let ParentType::Qualified { spec_alias, .. } = element_parent_type(&parent) {
                    let spec_ref = ast::SpecRef::same_repository(spec_alias.clone());
                    Some(
                        self.resolve_spec_for_import(spec, &spec_ref, &ftd.source, at)
                            .map_err(|error| vec![error])?
                            .1,
                    )
                } else {
                    None
                };

            let family = match element_parent_type(&parent) {
                ParentType::Primitive { .. } => type_name.clone(),
                _ => parent_type
                    .quantity_family_name()
                    .map(String::from)
                    .unwrap_or_else(|| type_name.clone()),
            };

            let extends = TypeExtends::Custom {
                parent: parent.to_string(),
                family,
                defining_spec: if let Some(import_spec) = import_target {
                    TypeDefiningSpec::Import { spec: import_spec }
                } else {
                    TypeDefiningSpec::Local
                },
            };

            let declared_default = match &ftd.bound_literal {
                Some(literal) => match semantics::parser_value_to_value_kind(literal, &final_specs)
                {
                    Ok(value_kind) => Some(RawDefault::Value(value_kind)),
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

            let lemma_type = Arc::new(LemmaType {
                name: Some(type_name.clone()),
                specifications: final_specs,
                extends,
            });
            resolved.insert(type_name.clone(), (lemma_type, declared_default));
        }

        let mut unit_index_tmp: HashMap<String, (Arc<LemmaType>, Option<DataTypeDef>)> =
            HashMap::new();
        let mut errors = Vec::new();
        let prim_ratio = semantics::primitive_ratio_arc().clone();
        for unit in Self::extract_units_from_type(&prim_ratio.as_ref().specifications) {
            unit_index_tmp.insert(unit, (Arc::clone(&prim_ratio), None));
        }

        for (type_name, (type_arc, _)) in &resolved {
            let data_type_def = data_defs
                .get(type_name.as_str())
                .expect("BUG: type was resolved but not in registry");
            let merge_result = if type_arc.is_quantity() {
                Self::add_quantity_units_to_index(
                    spec,
                    &mut unit_index_tmp,
                    type_arc,
                    data_type_def,
                )
            } else if type_arc.is_ratio() {
                Self::add_ratio_units_to_index(spec, &mut unit_index_tmp, type_arc, data_type_def)
            } else {
                Ok(())
            };
            if let Err(error) = merge_result {
                errors.push(error);
            }
        }

        for data_row in &spec.data {
            let ParsedDataValue::Import(spec_ref) = &data_row.value else {
                continue;
            };
            let (_, imported_spec) =
                match self.resolve_spec_for_import(spec, spec_ref, &data_row.source_location, at) {
                    Ok(import_pair) => import_pair,
                    Err(_) => continue,
                };
            let Some(imported_resolved) = already_resolved
                .iter()
                .find(|(_, imported, _)| Arc::ptr_eq(imported, &imported_spec))
                .map(|(_, _, resolved_spec_types)| resolved_spec_types)
            else {
                continue;
            };
            let Some(imported_defs) = self
                .data_types
                .iter()
                .find(|(s, _)| Arc::ptr_eq(s, &imported_spec))
                .map(|(_, defs)| defs)
            else {
                continue;
            };
            for (imported_type_name, def) in imported_defs {
                if matches!(def.parent, ParentType::Qualified { .. }) {
                    continue;
                }
                let Some(type_arc) = imported_resolved.resolved.get(imported_type_name.as_str())
                else {
                    continue;
                };
                let merge_result = if type_arc.is_quantity() {
                    Self::add_quantity_units_to_index(spec, &mut unit_index_tmp, type_arc, def)
                } else if type_arc.is_ratio() {
                    Self::add_ratio_units_to_index(spec, &mut unit_index_tmp, type_arc, def)
                } else {
                    Ok(())
                };
                if let Err(error) = merge_result {
                    errors.push(error);
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let unit_index: HashMap<String, Arc<LemmaType>> = unit_index_tmp
            .into_iter()
            .map(|(unit_name, (lemma_type, _))| (unit_name, lemma_type))
            .collect();

        let mut raw_defaults = Vec::new();
        let mut resolved_types = HashMap::new();
        for (type_name, (lemma_type, default)) in resolved {
            if let Some(raw_default) = default {
                raw_defaults.push((type_name.clone(), raw_default));
            }
            resolved_types.insert(type_name, lemma_type);
        }

        Ok(ResolvedSpecTypes {
            resolved: resolved_types,
            declared_defaults: HashMap::new(),
            raw_defaults,
            source_defaults: HashMap::new(),
            unit_index,
        })
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
        unit_index: &mut HashMap<String, (Arc<LemmaType>, Option<DataTypeDef>)>,
        resolved_type: &Arc<LemmaType>,
        defined_by: &DataTypeDef,
    ) -> Result<(), Error> {
        let resolved_ref = resolved_type.as_ref();
        let units = Self::extract_units_from_type(&resolved_ref.specifications);
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
                let current_extends_existing = resolved_ref
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
                        unit_index
                            .insert(unit, (Arc::clone(resolved_type), Some(defined_by.clone())));
                    }
                    continue;
                }

                if existing_type.same_quantity_family(resolved_ref) {
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
            unit_index.insert(unit, (Arc::clone(resolved_type), Some(defined_by.clone())));
        }
        Ok(())
    }

    fn add_ratio_units_to_index(
        spec: &Arc<LemmaSpec>,
        unit_index: &mut HashMap<String, (Arc<LemmaType>, Option<DataTypeDef>)>,
        resolved_type: &Arc<LemmaType>,
        defined_by: &DataTypeDef,
    ) -> Result<(), Error> {
        let resolved_ref = resolved_type.as_ref();
        let units = Self::extract_units_from_type(&resolved_ref.specifications);
        for unit in units {
            if let Some((existing_type, existing_def)) = unit_index.get(&unit) {
                if existing_type.is_ratio() {
                    if existing_def.is_none() {
                        unit_index.insert(
                            unit.clone(),
                            (Arc::clone(resolved_type), Some(defined_by.clone())),
                        );
                        continue;
                    }
                    if existing_type.name() == resolved_ref.name() {
                        continue;
                    }
                    if let (
                        TypeSpecification::Ratio {
                            units: existing_units,
                            ..
                        },
                        TypeSpecification::Ratio {
                            units: new_units, ..
                        },
                    ) = (&existing_type.specifications, &resolved_ref.specifications)
                    {
                        let same_factor = existing_units
                            .iter()
                            .find(|u| u.name == unit)
                            .zip(new_units.iter().find(|u| u.name == unit))
                            .is_some_and(|(eu, nu)| eu.value == nu.value);
                        if same_factor {
                            continue;
                        }
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
            unit_index.insert(unit, (Arc::clone(resolved_type), Some(defined_by.clone())));
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
    use crate::computation::rational::rational_new;
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
            PrimitiveKind::TimeRange,
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .unwrap();
        let dice_type = resolved_types.resolved.get("dice").unwrap();

        match &dice_type.specifications {
            TypeSpecification::Number {
                minimum, maximum, ..
            } => {
                assert_eq!(minimum, &Some(rational_new(0, 1)));
                assert_eq!(maximum, &Some(rational_new(6, 1)));
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .unwrap();
        let price_type = resolved_types.resolved.get("price").unwrap();

        match &price_type.specifications {
            TypeSpecification::Number {
                decimals, minimum, ..
            } => {
                assert_eq!(*decimals, Some(2));
                assert_eq!(minimum, &Some(rational_new(0, 1)));
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
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
    fn typedef_default_inherits_through_extension_chain() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: quantity -> unit eur 1 -> default 4 eur
data price: money
data final_price: price"#,
        );

        let resolved = resolver
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .expect("extension chain must resolve");

        assert!(
            resolved.declared_defaults.contains_key("money"),
            "base typedef default must materialize"
        );
        assert!(
            resolved.declared_defaults.contains_key("final_price"),
            "default must inherit through extension chain (money → price → final_price)"
        );
    }

    #[test]
    fn test_ratio_type_with_default_command() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data percentage: ratio -> minimum 0% -> maximum 100% -> default 50%"#,
        );

        let resolved_types = resolver
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .unwrap();
        let percentage_type = resolved_types.resolved.get("percentage").unwrap();

        match &percentage_type.specifications {
            TypeSpecification::Ratio {
                minimum, maximum, ..
            } => {
                assert_eq!(
                    *minimum,
                    Some(rational_new(0, 1)),
                    "ratio type should have minimum 0"
                );
                assert_eq!(
                    *maximum,
                    Some(rational_new(1, 1)),
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
                assert_eq!(v, &rational_new(1, 2));
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

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
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

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
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

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .unwrap();
        let money2 = resolved.resolved.get("money2").unwrap();
        match &money2.specifications {
            TypeSpecification::Quantity { units, .. } => {
                assert_eq!(units.len(), 3);
                let eur = units.iter().find(|u| u.name == "eur").unwrap();
                let usd = units.iter().find(|u| u.name == "usd").unwrap();
                let gbp = units.iter().find(|u| u.name == "gbp").unwrap();
                assert_eq!(
                    crate::computation::rational::commit_rational_to_decimal(&eur.factor).unwrap(),
                    Decimal::from_str_exact("1.20").unwrap()
                );
                assert_eq!(
                    crate::computation::rational::commit_rational_to_decimal(&usd.factor).unwrap(),
                    Decimal::from_str_exact("1.21").unwrap()
                );
                assert_eq!(
                    crate::computation::rational::commit_rational_to_decimal(&gbp.factor).unwrap(),
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

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
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

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
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
    fn test_same_ratio_unit_same_factor_across_types_allowed() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data spread_a: ratio
  -> unit basis_points 10000

data spread_b: ratio
  -> unit basis_points 10000"#,
        );

        resolver
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .expect("same unit name and factor across ratio types must be allowed");
    }

    #[test]
    fn test_different_ratio_unit_factor_across_types_errors() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data spread_a: ratio
  -> unit basis_points 10000

data spread_b: ratio
  -> unit basis_points 5000"#,
        );

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
        assert!(
            result.is_err(),
            "unrelated ratio types with different factors for the same unit must error"
        );
        let error_msg = result
            .unwrap_err()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            error_msg.contains("spread_a") && error_msg.contains("spread_b"),
            "expected ambiguous ratio unit between types, got: {}",
            error_msg
        );
    }

    #[test]
    fn test_multiple_builtin_ratio_types_share_percent_in_unit_index() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec targets
data standard_margin_pct: ratio
  -> minimum 0%
  -> default 15%

data default_credit_insurance_pct: ratio
  -> default 1.5%"#,
        );

        resolver
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .expect("multiple ratio types with built-in percent must not conflict");
    }

    #[test]
    fn test_three_ratio_types_share_builtin_and_custom_unit() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data margin: ratio -> default 10%
data fee: ratio
  -> unit tenths 10
data tax: ratio -> default 1%"#,
        );

        resolver
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .expect("builtin percent/permille plus custom tenths on second type must load");
    }

    #[test]
    fn test_ratio_unit_index_allows_builtin_after_two_named_types() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data first_ratio: ratio -> default 5%
data second_ratio: ratio -> default 10%"#,
        );

        let resolved = resolver
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .expect("two ratio types with only builtin units");

        assert!(
            resolved.unit_index.contains_key("percent"),
            "percent must remain in unit index"
        );
        assert!(
            resolved.unit_index.contains_key("permille"),
            "permille must remain in unit index"
        );
    }

    #[test]
    fn test_number_type_cannot_have_units() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data price: number
  -> unit eur 1.00"#,
        );

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
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
            .resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new())
            .unwrap();
        let z = resolved.resolved.get("z").unwrap();
        match &z.specifications {
            TypeSpecification::Quantity { units, .. } => {
                assert_eq!(units.len(), 1);
                let usd = units.iter().find(|u| u.name == "usd").unwrap();
                assert_eq!(
                    crate::computation::rational::commit_rational_to_decimal(&usd.factor).unwrap(),
                    Decimal::from_str_exact("0.84").unwrap()
                );
            }
            other => panic!("Expected Quantity type specifications, got {:?}", other),
        }
    }

    #[test]
    fn test_duplicate_unit_name_in_same_type_is_error() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data money: quantity
  -> unit eur 1.00
  -> unit eur 1.19"#,
        );

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
        assert!(result.is_err(), "duplicate unit name must be rejected");
        let errs = result.unwrap_err();
        assert!(!errs.is_empty());
        let msg = errs[0].to_string();
        assert!(
            msg.contains("eur"),
            "error must name the duplicate unit; got: {msg}"
        );
    }

    #[test]
    fn test_duplicate_unit_name_compound_is_error() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
data currency: quantity
  -> unit usd 1.00
  -> unit eur 0.92

data time_period: quantity
  -> unit year 1

data run_rate: quantity
  -> unit arr usd/year
  -> unit arr eur/year"#,
        );

        let result = resolver.resolve_and_validate(&spec_arc, &EffectiveDate::Origin, &Vec::new());
        assert!(
            result.is_err(),
            "duplicate compound unit name must be rejected"
        );
        let errs = result.unwrap_err();
        assert!(!errs.is_empty());
        let msg = errs[0].to_string();
        assert!(
            msg.contains("arr"),
            "error must name the duplicate unit; got: {msg}"
        );
    }
}

// ============================================================================
// Validation (formerly validation.rs)
// ============================================================================

fn push_uncommittable_bound_error(
    errors: &mut Vec<Error>,
    type_name: &str,
    bound: &crate::computation::rational::RationalInteger,
    label: &str,
    source: &Source,
    spec_context: Option<Arc<LemmaSpec>>,
) {
    if crate::computation::rational::commit_rational_to_decimal(bound).is_err() {
        errors.push(Error::validation_with_context(
            format!(
                "Type '{}' has {} bound {} that cannot be represented as a decimal",
                type_name, label, bound
            ),
            Some(source.clone()),
            None::<String>,
            spec_context,
            None,
        ));
    }
}

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

            if let Some((bound, unit_name)) = minimum {
                push_uncommittable_bound_error(
                    &mut errors,
                    type_name,
                    bound,
                    &format!("minimum {unit_name}"),
                    source,
                    spec_context.clone(),
                );
            }
            if let Some((bound, unit_name)) = maximum {
                push_uncommittable_bound_error(
                    &mut errors,
                    type_name,
                    bound,
                    &format!("maximum {unit_name}"),
                    source,
                    spec_context.clone(),
                );
            }
            for unit in units.iter() {
                if let Some(bound) = &unit.minimum {
                    push_uncommittable_bound_error(
                        &mut errors,
                        type_name,
                        bound,
                        &format!("unit '{}' minimum", unit.name),
                        source,
                        spec_context.clone(),
                    );
                }
                if let Some(bound) = &unit.maximum {
                    push_uncommittable_bound_error(
                        &mut errors,
                        type_name,
                        bound,
                        &format!("unit '{}' maximum", unit.name),
                        source,
                        spec_context.clone(),
                    );
                }
                if let Some(bound) = &unit.default_magnitude {
                    push_uncommittable_bound_error(
                        &mut errors,
                        type_name,
                        bound,
                        &format!("unit '{}' default", unit.name),
                        source,
                        spec_context.clone(),
                    );
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

            if let Some(ValueKind::Quantity(_def_value, def_signature)) = declared_default {
                let def_unit = def_signature
                    .first()
                    .map(|(n, _)| n.as_str())
                    .unwrap_or("");
                if !units.iter().any(|u| u.name == def_unit) {
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

                    // Validate unit names are unique within the type
                    if seen_names.contains(&unit.name) {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has duplicate unit name '{}'. Unit names must be unique within a type.", type_name, unit.name),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    if !unit.is_positive_factor() {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has unit '{}' with invalid value {}/{}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.factor.numer(), unit.factor.denom()),
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

            if let Some(bound) = minimum {
                push_uncommittable_bound_error(
                    &mut errors,
                    type_name,
                    bound,
                    "minimum",
                    source,
                    spec_context.clone(),
                );
            }
            if let Some(bound) = maximum {
                push_uncommittable_bound_error(
                    &mut errors,
                    type_name,
                    bound,
                    "maximum",
                    source,
                    spec_context.clone(),
                );
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

            if let Some(bound) = minimum {
                push_uncommittable_bound_error(
                    &mut errors,
                    type_name,
                    bound,
                    "minimum",
                    source,
                    spec_context.clone(),
                );
            }
            if let Some(bound) = maximum {
                push_uncommittable_bound_error(
                    &mut errors,
                    type_name,
                    bound,
                    "maximum",
                    source,
                    spec_context.clone(),
                );
            }
            for unit in units.iter() {
                if let Some(bound) = &unit.minimum {
                    push_uncommittable_bound_error(
                        &mut errors,
                        type_name,
                        bound,
                        &format!("unit '{}' minimum", unit.name),
                        source,
                        spec_context.clone(),
                    );
                }
                if let Some(bound) = &unit.maximum {
                    push_uncommittable_bound_error(
                        &mut errors,
                        type_name,
                        bound,
                        &format!("unit '{}' maximum", unit.name),
                        source,
                        spec_context.clone(),
                    );
                }
                if let Some(bound) = &unit.default_magnitude {
                    push_uncommittable_bound_error(
                        &mut errors,
                        type_name,
                        bound,
                        &format!("unit '{}' default", unit.name),
                        source,
                        spec_context.clone(),
                    );
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

                    // Validate unit names are unique within the type
                    if seen_names.contains(&unit.name) {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has duplicate unit name '{}'. Unit names must be unique within a type.", type_name, unit.name),
                            Some(source.clone()),
                            None::<String>,
                            spec_context.clone(),
                            None,
                        ));
                    } else {
                        seen_names.push(unit.name.clone());
                    }

                    if unit.value.numer() <= &crate::computation::bigint::BigInt::from_i64(0) {
                        errors.push(Error::validation_with_context(
                            format!("Type '{}' has unit '{}' with invalid value {}/{}. Unit values must be positive (conversion factor relative to type base).", type_name, unit.name, unit.value.numer(), unit.value.denom()),
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
        | TypeSpecification::TimeRange { .. }
        | TypeSpecification::QuantityRange { .. }
        | TypeSpecification::RatioRange { .. }
        | TypeSpecification::Boolean { .. } => {
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
    use crate::computation::rational::rational_new;
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
        let mut default: Option<RawDefault> = None;
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
            minimum: Some(rational_new(10, 1)),
            maximum: None,
            decimals: None,
            help: String::new(),
        };
        let default = ValueKind::Number(rational_new(5, 1));

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
            maximum: Some(rational_new(100, 1)),
            decimals: None,
            help: String::new(),
        };
        let default = ValueKind::Number(rational_new(150, 1));

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
            minimum: Some(rational_new(0, 1)),
            maximum: Some(rational_new(100, 1)),
            decimals: None,
            help: String::new(),
        };
        let default = ValueKind::Number(rational_new(50, 1));

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
            minimum: Some(rational_new(2, 1)),
            maximum: Some(rational_new(1, 1)),
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

    #[test]
    fn uncommittable_minimum_rejected_at_planning() {
        use crate::computation::rational::{commit_rational_to_decimal, try_rational_new, BigInt};
        use crate::literals::QuantityUnits;
        use crate::planning::semantics::TypeSpecification;

        let too_large = uncommittable_out_of_decimal_commit_range();
        assert!(
            commit_rational_to_decimal(&too_large).is_err(),
            "test setup: bound must be valid in Q but not committable to decimal"
        );

        let spec = TypeSpecification::Quantity {
            minimum: Some((too_large, "eur".to_string())),
            maximum: None,
            decimals: None,
            units: QuantityUnits(vec![crate::literals::QuantityUnit {
                name: "eur".to_string(),
                factor: try_rational_new(BigInt::one(), BigInt::one()).expect("BUG: test rational"),
                derived_quantity_factors: Default::default(),
                decomposition: Default::default(),
                minimum: None,
                maximum: None,
                default_magnitude: None,
            }]),
            traits: vec![],
            decomposition: None,
            help: String::new(),
        };

        let TypeSpecification::Quantity { minimum, .. } = &spec else {
            panic!("BUG: test constructs Quantity spec");
        };
        assert!(
            minimum.is_some(),
            "internal spec retains declared minimum in Q"
        );
        let src = test_source();
        let errors = validate_type_specifications(&spec, None, "money", &src, None);
        assert_eq!(
            errors.len(),
            1,
            "got: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
        assert!(
            errors[0]
                .to_string()
                .contains("cannot be represented as a decimal"),
            "got: {}",
            errors[0]
        );
    }

    #[test]
    fn uncommittable_maximum_rejected_at_planning() {
        use crate::computation::rational::{commit_rational_to_decimal, try_rational_new, BigInt};
        use crate::literals::QuantityUnits;
        use crate::planning::semantics::TypeSpecification;

        let too_large = uncommittable_out_of_decimal_commit_range();
        assert!(commit_rational_to_decimal(&too_large).is_err());

        let spec = TypeSpecification::Quantity {
            minimum: None,
            maximum: Some((too_large, "eur".to_string())),
            decimals: None,
            units: QuantityUnits(vec![crate::literals::QuantityUnit {
                name: "eur".to_string(),
                factor: try_rational_new(BigInt::one(), BigInt::one()).expect("BUG: test rational"),
                derived_quantity_factors: Default::default(),
                decomposition: Default::default(),
                minimum: None,
                maximum: None,
                default_magnitude: None,
            }]),
            traits: vec![],
            decomposition: None,
            help: String::new(),
        };

        let src = test_source();
        let errors = validate_type_specifications(&spec, None, "money", &src, None);
        assert_eq!(
            errors.len(),
            1,
            "got: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
        assert!(
            errors[0]
                .to_string()
                .contains("cannot be represented as a decimal"),
            "got: {}",
            errors[0]
        );
    }

    #[test]
    fn uncommittable_default_magnitude_rejected_at_planning() {
        use crate::computation::rational::{commit_rational_to_decimal, try_rational_new, BigInt};
        use crate::literals::QuantityUnits;
        use crate::planning::semantics::TypeSpecification;

        let too_large = uncommittable_out_of_decimal_commit_range();
        assert!(commit_rational_to_decimal(&too_large).is_err());

        let spec = TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units: QuantityUnits(vec![crate::literals::QuantityUnit {
                name: "eur".to_string(),
                factor: try_rational_new(BigInt::one(), BigInt::one()).expect("BUG: test rational"),
                derived_quantity_factors: Default::default(),
                decomposition: Default::default(),
                minimum: None,
                maximum: None,
                default_magnitude: Some(too_large),
            }]),
            traits: vec![],
            decomposition: None,
            help: String::new(),
        };

        let src = test_source();
        let errors = validate_type_specifications(&spec, None, "money", &src, None);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0]
                .to_string()
                .contains("cannot be represented as a decimal"),
            "got: {}",
            errors[0]
        );
        assert!(
            errors[0].to_string().contains("unit 'eur' default"),
            "got: {}",
            errors[0]
        );
    }

    /// Rational in Q that exceeds `rust_decimal` commit range (OUT boundary), e.g. per-unit sync product.
    fn uncommittable_out_of_decimal_commit_range() -> crate::computation::rational::RationalInteger
    {
        use crate::computation::rational::{try_rational_new, BigInt};
        try_rational_new(
            BigInt::try_from_str_radix("10000000000000000000000000000000", 10).unwrap(),
            BigInt::one(),
        )
        .expect("BUG: test rational must construct")
    }

    #[test]
    fn empty_quantity_signature_unit_must_not_coerce_as_unknown_empty_string() {
        use crate::computation::rational::rational_new;
        use crate::literals::QuantityUnits;
        use crate::planning::semantics::{LemmaType, LiteralValue, TypeSpecification, ValueKind};

        let schema_type = Arc::new(LemmaType {
            name: Some("money".to_string()),
            extends: TypeExtends::Primitive,
            specifications: TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: None,
                units: QuantityUnits(vec![crate::literals::QuantityUnit {
                    name: "eur".to_string(),
                    factor: rational_new(1, 1),
                    derived_quantity_factors: Default::default(),
                    decomposition: Default::default(),
                    minimum: None,
                    maximum: None,
                    default_magnitude: None,
                }]),
                traits: vec![],
                decomposition: Default::default(),
                help: String::new(),
            },
        });

        let literal = LiteralValue {
            value: ValueKind::Quantity(rational_new(1, 1), vec![("".to_string(), 1)]),
            lemma_type: Arc::clone(&schema_type),
        };

        let result = Graph::coerce_literal_to_schema_type(&literal, &schema_type);

        assert!(
            result.is_err(),
            "empty unit name in quantity signature must not coerce successfully"
        );
        if let Err(message) = result {
            assert!(
                !message.contains("unknown unit ''"),
                "must not treat missing unit as empty-string validation error, got: {message}"
            );
        }
    }

    #[test]
    fn refresh_named_range_specs_missing_element_must_not_leave_unconverted_quantity() {
        use crate::parsing::ast::{LemmaSpec, ParentType, Span};
        use crate::parsing::source::{Source, SourceType};
        use crate::planning::semantics::primitive_quantity_arc;

        let spec = Arc::new(LemmaSpec::new("t".to_string()));
        let source = Source::new(
            SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let mut data_defs = HashMap::new();
        data_defs.insert(
            "band".to_string(),
            DataTypeDef {
                parent: ParentType::Ranged {
                    inner: Box::new(ParentType::Custom {
                        name: "ghost".to_string(),
                    }),
                },
                constraints: None,
                source: source.clone(),
                name: "band".to_string(),
                bound_literal: None,
            },
        );
        let mut resolved = HashMap::new();
        resolved.insert("band".to_string(), primitive_quantity_arc().clone());
        let mut defaults = HashMap::new();

        let errors = refresh_named_range_specs(&spec, &data_defs, &mut resolved, &mut defaults);

        assert!(
            !errors.is_empty(),
            "planning must error when ranged inner type is missing"
        );
        assert!(
            errors[0]
                .to_string()
                .contains("references missing element type 'ghost'"),
            "got: {}",
            errors[0]
        );
    }
}
