//! Per-slice type resolution for Lemma specs
//!
//! This module provides `PerSliceTypeResolver` which handles:
//! - Registering user-defined types for each spec
//! - Resolving type hierarchies and inheritance chains per temporal slice
//! - Detecting and preventing circular type dependencies
//! - Applying constraints to create final type specifications
//!
//! Cross-spec type imports are resolved via `Context.get_spec(name, resolve_at)`,
//! ensuring each temporal slice sees the correct dependency version.

use crate::engine::Context;
use crate::error::Error;
use crate::parsing::ast::FactValue as ParsedFactValue;
use crate::parsing::ast::{
    self as ast, Constraint, DateTimeValue, LemmaSpec, ParentType, Reference, TypeDef,
};
use crate::planning::semantics::{
    self, LemmaType, TypeDefiningSpec, TypeExtends, TypeSpecification,
};
use crate::planning::validation::validate_type_specifications;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Fully resolved types for a single spec.
/// After resolution, all imports are inlined — specs are independent.
#[derive(Debug, Clone)]
pub struct ResolvedSpecTypes {
    /// Named types: type_name -> fully resolved type
    pub named_types: HashMap<String, LemmaType>,

    /// Inline type definitions: fact reference -> fully resolved type
    pub inline_type_definitions: HashMap<Reference, LemmaType>,

    /// Unit index: unit_name -> (resolved type, defining AST node if user-defined)
    /// Built during resolution — if unit appears in multiple types, resolution fails.
    /// TypeDef is kept for conflict detection (identity, extends-check, source location).
    /// Primitives (percent, permille) have no TypeDef.
    pub unit_index: HashMap<String, (LemmaType, Option<TypeDef>)>,
}

/// Resolved spec for a parent type reference (same-spec or cross-spec import).
#[derive(Debug, Clone)]
pub(crate) struct ResolvedParentSpec {
    pub spec: Arc<LemmaSpec>,
    /// Set when this is a cross-spec import (from.is_some()).
    pub resolved_plan_hash: Option<String>,
}

/// Per-slice type resolver. Constructed for each `Graph::build` call.
///
/// Cross-spec type imports are resolved via `Context.get_spec(name, resolve_at)` so
/// each temporal slice sees the dependency version active at that point.
/// Named types are keyed by `Arc<LemmaSpec>` and support inheritance through parent references.
/// The resolver handles cycle detection and accumulates constraints through the inheritance chain.
#[derive(Debug, Clone)]
pub(crate) struct PerSliceTypeResolver<'a> {
    named_types: HashMap<Arc<LemmaSpec>, HashMap<String, TypeDef>>,
    inline_type_definitions: HashMap<Arc<LemmaSpec>, HashMap<Reference, TypeDef>>,
    context: &'a Context,
    resolve_at: Option<DateTimeValue>,
    plan_hashes: &'a super::PlanHashRegistry,
    /// All spec arcs that have been registered, in registration order.
    /// Includes specs without types (they still need a unit_index with primitive ratio units).
    all_registered_specs: Vec<Arc<LemmaSpec>>,
}

impl<'a> PerSliceTypeResolver<'a> {
    pub fn new(
        context: &'a Context,
        resolve_at: Option<DateTimeValue>,
        plan_hashes: &'a super::PlanHashRegistry,
    ) -> Self {
        PerSliceTypeResolver {
            named_types: HashMap::new(),
            inline_type_definitions: HashMap::new(),
            context,
            resolve_at,
            plan_hashes,
            all_registered_specs: Vec::new(),
        }
    }

    /// Register all named types from a spec (skips inline types).
    pub fn register_all(&mut self, spec: &Arc<LemmaSpec>) -> Vec<Error> {
        if !self
            .all_registered_specs
            .iter()
            .any(|s| Arc::ptr_eq(s, spec))
        {
            self.all_registered_specs.push(Arc::clone(spec));
        }

        let mut errors = Vec::new();
        for type_def in &spec.types {
            let type_name = match type_def {
                ast::TypeDef::Regular { name, .. } | ast::TypeDef::Import { name, .. } => {
                    Some(name.as_str())
                }
                ast::TypeDef::Inline { .. } => None,
            };
            if let Some(name) = type_name {
                if let Err(e) = crate::limits::check_max_length(
                    name,
                    crate::limits::MAX_TYPE_NAME_LENGTH,
                    "type",
                    Some(type_def.source_location().clone()),
                ) {
                    errors.push(e);
                    continue;
                }
            }
            if let Err(e) = self.register_type(spec, type_def.clone()) {
                errors.push(e);
            }
        }
        errors
    }

    /// Register a user-defined type for a given spec.
    pub fn register_type(&mut self, spec: &Arc<LemmaSpec>, def: TypeDef) -> Result<(), Error> {
        if !self
            .all_registered_specs
            .iter()
            .any(|s| Arc::ptr_eq(s, spec))
        {
            self.all_registered_specs.push(Arc::clone(spec));
        }

        let def_loc = def.source_location().clone();
        let spec_name = &spec.name;
        match &def {
            TypeDef::Regular { name, .. } | TypeDef::Import { name, .. } => {
                let spec_types = self.named_types.entry(Arc::clone(spec)).or_default();
                if spec_types.contains_key(name) {
                    return Err(Error::validation_with_context(
                        format!("Type '{}' is already defined in spec '{}'", name, spec_name),
                        Some(def_loc.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    ));
                }
                spec_types.insert(name.clone(), def);
            }
            TypeDef::Inline { fact_ref, .. } => {
                let spec_inline_types = self
                    .inline_type_definitions
                    .entry(Arc::clone(spec))
                    .or_default();
                if spec_inline_types.contains_key(fact_ref) {
                    return Err(Error::validation_with_context(
                        format!(
                            "Inline type definition for fact '{}' is already defined in spec '{}'",
                            fact_ref.name, spec_name
                        ),
                        Some(def_loc.clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    ));
                }
                spec_inline_types.insert(fact_ref.clone(), def);
            }
        }
        Ok(())
    }

    /// Register types from all specs transitively reachable from `spec` via type imports,
    /// fact spec references, and fact type declarations with `from`.
    pub fn register_dependency_types(&mut self, spec: &Arc<LemmaSpec>) -> Vec<Error> {
        let mut errors = Vec::new();
        let mut visited_spec_names: HashSet<String> = HashSet::new();
        visited_spec_names.insert(spec.name.clone());
        self.register_dependency_types_recursive(spec, &mut visited_spec_names, &mut errors);
        errors
    }

    fn register_dependency_types_recursive(
        &mut self,
        spec: &Arc<LemmaSpec>,
        visited: &mut HashSet<String>,
        errors: &mut Vec<Error>,
    ) {
        for type_def in &spec.types {
            if let TypeDef::Import { from, .. } = type_def {
                self.try_register_dep_spec(&from.name, from.effective.as_ref(), visited, errors);
            }
        }

        for fact in &spec.facts {
            match &fact.value {
                ParsedFactValue::SpecReference(spec_ref) => {
                    self.try_register_dep_spec(
                        &spec_ref.name,
                        spec_ref.effective.as_ref(),
                        visited,
                        errors,
                    );
                }
                ParsedFactValue::TypeDeclaration {
                    from: Some(from_ref),
                    ..
                } => {
                    self.try_register_dep_spec(
                        &from_ref.name,
                        from_ref.effective.as_ref(),
                        visited,
                        errors,
                    );
                }
                _ => {}
            }
        }
    }

    fn try_register_dep_spec(
        &mut self,
        name: &str,
        explicit_effective: Option<&DateTimeValue>,
        visited: &mut HashSet<String>,
        errors: &mut Vec<Error>,
    ) {
        if visited.contains(name) {
            return;
        }
        visited.insert(name.to_string());

        let at = explicit_effective.or(self.resolve_at.as_ref());
        let dep_spec = match at {
            Some(dt) => self.context.get_spec(name, dt),
            None => self.context.specs_for_name(name).into_iter().next(),
        };

        if let Some(dep_spec) = dep_spec {
            errors.extend(self.register_all(&dep_spec));
            self.register_dependency_types_recursive(&dep_spec, visited, errors);
        }
    }

    /// Resolve named types for all registered specs and validate their specifications.
    /// Returns resolved types per spec and any validation errors.
    pub fn resolve_all_registered_specs(
        &self,
    ) -> (HashMap<Arc<LemmaSpec>, ResolvedSpecTypes>, Vec<Error>) {
        let mut result = HashMap::new();
        let mut errors = Vec::new();

        for spec_arc in &self.all_registered_specs {
            match self.resolve_and_validate_named_types(spec_arc) {
                Ok(resolved_types) => {
                    result.insert(Arc::clone(spec_arc), resolved_types);
                }
                Err(es) => errors.extend(es),
            }
        }

        (result, errors)
    }

    /// Resolve named types for a single spec and validate their specifications.
    pub fn resolve_and_validate_named_types(
        &self,
        spec: &Arc<LemmaSpec>,
    ) -> Result<ResolvedSpecTypes, Vec<Error>> {
        let resolved_types = self.resolve_named_types(spec)?;
        let mut errors = Vec::new();

        for (type_name, lemma_type) in &resolved_types.named_types {
            let source = spec
                .types
                .iter()
                .find(|td| match td {
                    ast::TypeDef::Regular { name, .. } | ast::TypeDef::Import { name, .. } => {
                        name == type_name
                    }
                    ast::TypeDef::Inline { .. } => false,
                })
                .map(|td| td.source_location().clone())
                .unwrap_or_else(|| {
                    unreachable!(
                        "BUG: resolved named type '{}' has no corresponding TypeDef in spec '{}'",
                        type_name, spec.name
                    )
                });
            let mut spec_errors = validate_type_specifications(
                &lemma_type.specifications,
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

    /// Resolve only named types (for validation before inline type definitions are registered).
    pub fn resolve_named_types(
        &self,
        spec: &Arc<LemmaSpec>,
    ) -> Result<ResolvedSpecTypes, Vec<Error>> {
        self.resolve_types_internal(spec, false)
    }

    /// Resolve only inline type definitions and merge them into an existing
    /// `ResolvedSpecTypes` that already contains the named types.
    pub fn resolve_inline_types(
        &self,
        spec: &Arc<LemmaSpec>,
        mut existing: ResolvedSpecTypes,
    ) -> Result<ResolvedSpecTypes, Vec<Error>> {
        let mut errors = Vec::new();

        if let Some(spec_inline_types) = self.inline_type_definitions.get(spec) {
            for (fact_ref, type_def) in spec_inline_types {
                let mut visited = HashSet::new();
                match self.resolve_inline_type_definition(spec, type_def, &mut visited) {
                    Ok(Some(resolved_type)) => {
                        existing
                            .inline_type_definitions
                            .insert(fact_ref.clone(), resolved_type);
                    }
                    Ok(None) => {
                        unreachable!(
                            "BUG: registered inline type definition for fact '{}' could not be resolved (spec='{}')",
                            fact_ref, spec.name
                        );
                    }
                    Err(es) => return Err(es),
                }
            }
        }

        if let Some(spec_inline_defs) = self.inline_type_definitions.get(spec) {
            for (fact_ref, type_def) in spec_inline_defs {
                let Some(resolved_type) = existing.inline_type_definitions.get(fact_ref) else {
                    continue;
                };
                let e: Result<(), Error> = if resolved_type.is_scale() {
                    Self::add_scale_units_to_index(
                        spec,
                        &mut existing.unit_index,
                        resolved_type,
                        type_def,
                    )
                } else if resolved_type.is_ratio() {
                    Self::add_ratio_units_to_index(
                        spec,
                        &mut existing.unit_index,
                        resolved_type,
                        type_def,
                    )
                } else {
                    Ok(())
                };
                if let Err(e) = e {
                    errors.push(e);
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(existing)
    }

    // =========================================================================
    // Private resolution methods
    // =========================================================================

    fn resolve_types_internal(
        &self,
        spec: &Arc<LemmaSpec>,
        include_anonymous: bool,
    ) -> Result<ResolvedSpecTypes, Vec<Error>> {
        let mut named_types = HashMap::new();
        let mut inline_type_definitions = HashMap::new();
        let mut visited = HashSet::new();

        if let Some(spec_types) = self.named_types.get(spec) {
            for type_name in spec_types.keys() {
                match self.resolve_type_internal(spec, type_name, &mut visited) {
                    Ok(Some(resolved_type)) => {
                        named_types.insert(type_name.clone(), resolved_type);
                    }
                    Ok(None) => {
                        unreachable!(
                            "BUG: registered named type '{}' could not be resolved (spec='{}')",
                            type_name, spec.name
                        );
                    }
                    Err(es) => return Err(es),
                }
                visited.clear();
            }
        }

        if include_anonymous {
            if let Some(spec_inline_types) = self.inline_type_definitions.get(spec) {
                for (fact_ref, type_def) in spec_inline_types {
                    let mut visited = HashSet::new();
                    match self.resolve_inline_type_definition(spec, type_def, &mut visited) {
                        Ok(Some(resolved_type)) => {
                            inline_type_definitions.insert(fact_ref.clone(), resolved_type);
                        }
                        Ok(None) => {
                            unreachable!(
                                "BUG: registered inline type definition for fact '{}' could not be resolved (spec='{}')",
                                fact_ref, spec.name
                            );
                        }
                        Err(es) => return Err(es),
                    }
                }
            }
        }

        let mut unit_index: HashMap<String, (LemmaType, Option<TypeDef>)> = HashMap::new();
        let mut errors = Vec::new();

        let prim_ratio = semantics::primitive_ratio();
        for unit in Self::extract_units_from_type(&prim_ratio.specifications) {
            unit_index.insert(unit, (prim_ratio.clone(), None));
        }

        for (type_name, resolved_type) in &named_types {
            let type_def = self
                .named_types
                .get(spec)
                .and_then(|defs| defs.get(type_name.as_str()))
                .expect("BUG: named type was resolved but not in registry");
            let e: Result<(), Error> = if resolved_type.is_scale() {
                Self::add_scale_units_to_index(spec, &mut unit_index, resolved_type, type_def)
            } else if resolved_type.is_ratio() {
                Self::add_ratio_units_to_index(spec, &mut unit_index, resolved_type, type_def)
            } else {
                Ok(())
            };
            if let Err(e) = e {
                errors.push(e);
            }
        }

        for (fact_ref, resolved_type) in &inline_type_definitions {
            let type_def = self
                .inline_type_definitions
                .get(spec)
                .and_then(|defs| defs.get(fact_ref))
                .expect("BUG: inline type was resolved but not in registry");
            let e: Result<(), Error> = if resolved_type.is_scale() {
                Self::add_scale_units_to_index(spec, &mut unit_index, resolved_type, type_def)
            } else if resolved_type.is_ratio() {
                Self::add_ratio_units_to_index(spec, &mut unit_index, resolved_type, type_def)
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

        Ok(ResolvedSpecTypes {
            named_types,
            inline_type_definitions,
            unit_index,
        })
    }

    fn resolve_type_internal(
        &self,
        spec: &Arc<LemmaSpec>,
        name: &str,
        visited: &mut HashSet<String>,
    ) -> Result<Option<LemmaType>, Vec<Error>> {
        let key = format!("{}::{}", spec.name, name);
        if visited.contains(&key) {
            let source_location = self
                .named_types
                .get(spec)
                .and_then(|dt| dt.get(name))
                .map(|td| td.source_location().clone())
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

        let type_def = match self.named_types.get(spec).and_then(|dt| dt.get(name)) {
            Some(def) => def.clone(),
            None => {
                visited.remove(&key);
                return Ok(None);
            }
        };

        let (parent, from, constraints, type_name) = match &type_def {
            TypeDef::Regular {
                name,
                parent,
                constraints,
                ..
            } => (parent.clone(), None, constraints.clone(), name.clone()),
            TypeDef::Import {
                name,
                source_type,
                from,
                constraints,
                ..
            } => (
                ParentType::Custom {
                    name: source_type.clone(),
                },
                Some(from.clone()),
                constraints.clone(),
                name.clone(),
            ),
            TypeDef::Inline { .. } => {
                visited.remove(&key);
                return Ok(None);
            }
        };

        let parent_specs = match self.resolve_parent(
            spec,
            &parent,
            &from,
            visited,
            type_def.source_location(),
        ) {
            Ok(Some(specs)) => specs,
            Ok(None) => {
                visited.remove(&key);
                let source = type_def.source_location().clone();
                return Err(vec![Error::validation_with_context(
                    format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                    Some(source.clone()),
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

        let final_specs = if let Some(constraints) = &constraints {
            match Self::apply_constraints(
                spec,
                parent_specs,
                constraints,
                type_def.source_location(),
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

        let extends = if matches!(parent, ParentType::Primitive { .. }) {
            TypeExtends::Primitive
        } else {
            let parent_name = match &parent {
                ParentType::Custom { name } => name.clone(),
                ParentType::Primitive { .. } => unreachable!("already handled above"),
            };
            let parent_spec = match self.get_spec_arc_for_parent(spec, &from) {
                Ok(x) => x,
                Err(e) => return Err(vec![e]),
            };
            let family = match &parent_spec {
                Some(r) => match self.resolve_type_internal(&r.spec, &parent_name, visited) {
                    Ok(Some(parent_type)) => parent_type
                        .scale_family_name()
                        .map(String::from)
                        .unwrap_or_else(|| parent_name.clone()),
                    Ok(None) => parent_name.clone(),
                    Err(es) => return Err(es),
                },
                None => parent_name.clone(),
            };
            let defining_spec = if from.is_some() {
                match &parent_spec {
                    Some(r) => match &r.resolved_plan_hash {
                        Some(hash) => TypeDefiningSpec::Import {
                            spec: Arc::clone(&r.spec),
                            resolved_plan_hash: hash.clone(),
                        },
                        None => unreachable!(
                            "BUG: from.is_some() but get_spec_arc_for_parent returned None for hash"
                        ),
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

        Ok(Some(LemmaType {
            name: Some(type_name),
            specifications: final_specs,
            extends,
        }))
    }

    fn resolve_parent(
        &self,
        spec: &Arc<LemmaSpec>,
        parent: &ParentType,
        from: &Option<crate::parsing::ast::SpecRef>,
        visited: &mut HashSet<String>,
        source: &crate::Source,
    ) -> Result<Option<TypeSpecification>, Vec<Error>> {
        if let ParentType::Primitive { primitive: kind } = parent {
            return Ok(Some(semantics::type_spec_for_primitive(*kind)));
        }

        let parent_name = match parent {
            ParentType::Custom { name } => name.as_str(),
            ParentType::Primitive { .. } => unreachable!("already returned above"),
        };

        let parent_spec = match self.get_spec_arc_for_parent(spec, from) {
            Ok(x) => x,
            Err(e) => return Err(vec![e]),
        };
        let result = match &parent_spec {
            Some(r) => self.resolve_type_internal(&r.spec, parent_name, visited),
            None => Ok(None),
        };
        match result {
            Ok(Some(t)) => Ok(Some(t.specifications)),
            Ok(None) => {
                let type_exists = parent_spec
                    .as_ref()
                    .and_then(|r| self.named_types.get(&r.spec))
                    .map(|spec_types| spec_types.contains_key(parent_name))
                    .unwrap_or(false);

                if !type_exists {
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

    /// Get the spec arc (and plan hash when import) for resolving a parent type reference.
    /// For same-spec extension (from is None): resolved_plan_hash is None.
    /// For cross-spec import (from is Some): resolved_plan_hash is Some.
    fn get_spec_arc_for_parent(
        &self,
        spec: &Arc<LemmaSpec>,
        from: &Option<crate::parsing::ast::SpecRef>,
    ) -> Result<Option<ResolvedParentSpec>, Error> {
        match from {
            Some(from_ref) => self.resolve_spec_for_import(from_ref).map(|(arc, hash)| {
                Some(ResolvedParentSpec {
                    spec: arc,
                    resolved_plan_hash: Some(hash),
                })
            }),
            None => Ok(Some(ResolvedParentSpec {
                spec: Arc::clone(spec),
                resolved_plan_hash: None,
            })),
        }
    }

    /// Resolve a SpecRef to the spec version active at this slice. Returns (arc, plan_hash).
    /// Verifies `hash_pin` against the plan-hash registry when present.
    fn resolve_spec_for_import(
        &self,
        from: &crate::parsing::ast::SpecRef,
    ) -> Result<(Arc<LemmaSpec>, String), Error> {
        if let Some(pin) = &from.hash_pin {
            return match self.plan_hashes.get_by_pin(&from.name, pin) {
                Some(arc) => Ok((Arc::clone(arc), pin.clone())),
                None => Err(Error::validation(
                    format!(
                        "No spec '{}' found with plan hash '{}' for type import",
                        from.name, pin
                    ),
                    None,
                    None::<String>,
                )),
            };
        }

        let at = from.effective.as_ref().or(self.resolve_at.as_ref());
        let resolved = match at {
            Some(dt) => self.context.get_spec(&from.name, dt),
            None => self.context.specs_for_name(&from.name).into_iter().next(),
        };
        let arc = resolved.ok_or_else(|| {
            Error::validation(
                format!("Spec '{}' not found for type import", from.name),
                None,
                None::<String>,
            )
        })?;
        let hash = self
            .plan_hashes
            .get_by_slice(&arc.name, &arc.effective_from)
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| {
                unreachable!(
                    "BUG: resolved type-import dependency must have plan hash; \
                     topological planning guarantees deps are planned first"
                )
            });
        Ok((arc, hash))
    }

    fn apply_constraints(
        spec: &Arc<LemmaSpec>,
        mut specs: TypeSpecification,
        constraints: &[Constraint],
        source: &crate::Source,
    ) -> Result<TypeSpecification, Vec<Error>> {
        let mut errors = Vec::new();
        for (command, args) in constraints {
            let specs_clone = specs.clone();
            match specs.apply_constraint(*command, args) {
                Ok(updated_specs) => specs = updated_specs,
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

    fn resolve_inline_type_definition(
        &self,
        spec: &Arc<LemmaSpec>,
        type_def: &TypeDef,
        visited: &mut HashSet<String>,
    ) -> Result<Option<LemmaType>, Vec<Error>> {
        let def_loc = type_def.source_location().clone();
        let TypeDef::Inline {
            parent,
            constraints,
            fact_ref: _,
            from,
            ..
        } = type_def
        else {
            return Ok(None);
        };

        let parent_specs = match self.resolve_parent(spec, parent, from, visited, &def_loc) {
            Ok(Some(specs)) => specs,
            Ok(None) => {
                return Err(vec![Error::validation_with_context(
                    format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                    Some(def_loc.clone()),
                    None::<String>,
                    Some(Arc::clone(spec)),
                    None,
                )]);
            }
            Err(es) => return Err(es),
        };

        let final_specs = if let Some(constraints) = constraints {
            Self::apply_constraints(spec, parent_specs, constraints, &def_loc)?
        } else {
            parent_specs
        };

        let extends = if matches!(parent, ParentType::Primitive { .. }) {
            TypeExtends::Primitive
        } else {
            let parent_name = match parent {
                ParentType::Custom { ref name } => name.clone(),
                ParentType::Primitive { .. } => unreachable!("already handled above"),
            };
            let parent_spec = match self.get_spec_arc_for_parent(spec, from) {
                Ok(x) => x,
                Err(e) => return Err(vec![e]),
            };
            let family = match &parent_spec {
                Some(r) => match self.resolve_type_internal(&r.spec, &parent_name, visited) {
                    Ok(Some(parent_type)) => parent_type
                        .scale_family_name()
                        .map(String::from)
                        .unwrap_or_else(|| parent_name.clone()),
                    Ok(None) => parent_name.clone(),
                    Err(es) => return Err(es),
                },
                None => parent_name.clone(),
            };
            let defining_spec = if from.is_some() {
                match &parent_spec {
                    Some(r) => match &r.resolved_plan_hash {
                        Some(hash) => TypeDefiningSpec::Import {
                            spec: Arc::clone(&r.spec),
                            resolved_plan_hash: hash.clone(),
                        },
                        None => unreachable!(
                            "BUG: from.is_some() but get_spec_arc_for_parent returned None for hash"
                        ),
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

        Ok(Some(LemmaType::without_name(final_specs, extends)))
    }

    // =========================================================================
    // Static helpers (no &self)
    // =========================================================================

    fn add_scale_units_to_index(
        spec: &Arc<LemmaSpec>,
        unit_index: &mut HashMap<String, (LemmaType, Option<TypeDef>)>,
        resolved_type: &LemmaType,
        defined_by: &TypeDef,
    ) -> Result<(), Error> {
        let units = Self::extract_units_from_type(&resolved_type.specifications);
        for unit in units {
            if let Some((existing_type, existing_def)) = unit_index.get(&unit) {
                let same_type = existing_def.as_ref() == Some(defined_by);

                if same_type {
                    return Err(Error::validation_with_context(
                        format!(
                            "Unit '{}' is defined more than once in type '{}'",
                            unit,
                            defined_by.name()
                        ),
                        Some(defined_by.source_location().clone()),
                        None::<String>,
                        Some(Arc::clone(spec)),
                        None,
                    ));
                }

                let existing_name: String = existing_def
                    .as_ref()
                    .map(|d| d.name().to_owned())
                    .unwrap_or_else(|| existing_type.name());
                let current_extends_existing = resolved_type
                    .extends
                    .parent_name()
                    .map(|p| p == existing_name.as_str())
                    .unwrap_or(false);
                let existing_extends_current = existing_type
                    .extends
                    .parent_name()
                    .map(|p| p == defined_by.name())
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
                        unit,
                        existing_name,
                        defined_by.name()
                    ),
                    Some(defined_by.source_location().clone()),
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
        unit_index: &mut HashMap<String, (LemmaType, Option<TypeDef>)>,
        resolved_type: &LemmaType,
        defined_by: &TypeDef,
    ) -> Result<(), Error> {
        let units = Self::extract_units_from_type(&resolved_type.specifications);
        for unit in units {
            if let Some((existing_type, existing_def)) = unit_index.get(&unit) {
                if existing_type.is_ratio() {
                    continue;
                }
                let existing_name: String = existing_def
                    .as_ref()
                    .map(|d| d.name().to_owned())
                    .unwrap_or_else(|| existing_type.name());
                return Err(Error::validation_with_context(
                    format!(
                        "Ambiguous unit '{}'. Defined in multiple types: '{}' and '{}'",
                        unit,
                        existing_name,
                        defined_by.name()
                    ),
                    Some(defined_by.source_location().clone()),
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
mod tests {
    use super::*;
    use crate::engine::Context;
    use crate::parse;
    use crate::parsing::ast::{
        CommandArg, LemmaSpec, ParentType, PrimitiveKind, TypeConstraintCommand,
    };
    use crate::ResourceLimits;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    fn test_context_and_spec() -> (Context, Arc<LemmaSpec>) {
        let spec = LemmaSpec::new("test_spec".to_string());
        let arc = Arc::new(spec);
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::clone(&arc), false)
            .expect("insert test spec");
        (ctx, arc)
    }

    fn resolver_for_code(code: &str) -> (PerSliceTypeResolver<'static>, Vec<Arc<LemmaSpec>>) {
        // Leak the context so we can return a resolver with 'static lifetime for tests.
        // This is acceptable in test code only.
        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let ctx = Box::leak(Box::new(Context::new()));
        let mut spec_arcs = Vec::new();
        for spec in &specs {
            let arc = Arc::new(spec.clone());
            ctx.insert_spec(Arc::clone(&arc), spec.from_registry)
                .expect("insert spec");
            spec_arcs.push(arc);
        }
        let plan_hashes = Box::leak(Box::new(crate::planning::PlanHashRegistry::default()));
        let mut resolver = PerSliceTypeResolver::new(ctx, None, plan_hashes);
        for spec_arc in &spec_arcs {
            resolver.register_all(spec_arc);
        }
        (resolver, spec_arcs)
    }

    fn resolver_single_spec(code: &str) -> (PerSliceTypeResolver<'static>, Arc<LemmaSpec>) {
        let (resolver, spec_arcs) = resolver_for_code(code);
        let spec_arc = spec_arcs.into_iter().next().expect("at least one spec");
        (resolver, spec_arc)
    }

    #[test]
    fn test_registry_creation() {
        let (ctx, spec_arc) = test_context_and_spec();
        let ph = crate::planning::PlanHashRegistry::default();
        let resolver = PerSliceTypeResolver::new(&ctx, None, &ph);
        let resolved = resolver.resolve_named_types(&spec_arc).unwrap();
        assert!(resolved.named_types.is_empty());
        assert!(resolved.inline_type_definitions.is_empty());
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
    fn test_register_named_type() {
        let (ctx, spec_arc) = test_context_and_spec();
        let ph = crate::planning::PlanHashRegistry::default();
        let mut resolver = PerSliceTypeResolver::new(&ctx, None, &ph);
        let type_def = TypeDef::Regular {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "money".to_string(),
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: None,
        };

        let result = resolver.register_type(&spec_arc, type_def);
        assert!(result.is_ok());
    }

    #[test]
    fn test_register_inline_type_definition() {
        use crate::parsing::ast::Reference;
        let (ctx, spec_arc) = test_context_and_spec();
        let ph = crate::planning::PlanHashRegistry::default();
        let mut resolver = PerSliceTypeResolver::new(&ctx, None, &ph);
        let fact_ref = Reference::local("age".to_string());
        let type_def = TypeDef::Inline {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: Some(vec![
                (
                    TypeConstraintCommand::Minimum,
                    vec![CommandArg::Number("0".to_string())],
                ),
                (
                    TypeConstraintCommand::Maximum,
                    vec![CommandArg::Number("150".to_string())],
                ),
            ]),
            fact_ref: fact_ref.clone(),
            from: None,
        };

        let result = resolver.register_type(&spec_arc, type_def);
        assert!(result.is_ok());
        let resolved = resolver.resolve_types_internal(&spec_arc, true).unwrap();
        assert!(resolved.inline_type_definitions.contains_key(&fact_ref));
    }

    #[test]
    fn test_register_duplicate_type_fails() {
        let (ctx, spec_arc) = test_context_and_spec();
        let ph = crate::planning::PlanHashRegistry::default();
        let mut resolver = PerSliceTypeResolver::new(&ctx, None, &ph);
        let type_def = TypeDef::Regular {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "money".to_string(),
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: None,
        };

        resolver.register_type(&spec_arc, type_def.clone()).unwrap();
        let result = resolver.register_type(&spec_arc, type_def);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_custom_type_from_primitive() {
        let (ctx, spec_arc) = test_context_and_spec();
        let ph = crate::planning::PlanHashRegistry::default();
        let mut resolver = PerSliceTypeResolver::new(&ctx, None, &ph);
        let type_def = TypeDef::Regular {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
            ),
            name: "money".to_string(),
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Number,
            },
            constraints: None,
        };

        resolver.register_type(&spec_arc, type_def).unwrap();
        let resolved = resolver.resolve_types_internal(&spec_arc, true).unwrap();

        assert!(resolved.named_types.contains_key("money"));
        let money_type = resolved.named_types.get("money").unwrap();
        assert_eq!(money_type.name, Some("money".to_string()));
    }

    #[test]
    fn test_type_definition_resolution() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
type dice: number -> minimum 0 -> maximum 6"#,
        );

        let resolved_types = resolver.resolve_types_internal(&spec_arc, true).unwrap();
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
type money: scale -> decimals 2 -> unit eur 1.0 -> unit usd 1.18"#,
        );

        let resolved_types = resolver.resolve_types_internal(&spec_arc, true).unwrap();
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
type price: number -> decimals 2 -> minimum 0"#,
        );

        let resolved_types = resolver.resolve_types_internal(&spec_arc, true).unwrap();
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
type precise_number: number -> decimals 4"#,
        );

        let resolved_types = resolver.resolve_types_internal(&spec_arc, true).unwrap();
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
type weight: scale -> unit kg 1 -> decimals 3"#,
        );

        let resolved_types = resolver.resolve_types_internal(&spec_arc, true).unwrap();
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
type ratio_type: ratio -> decimals 2"#,
        );

        let resolved_types = resolver.resolve_types_internal(&spec_arc, true).unwrap();
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
type percentage: ratio -> minimum 0 -> maximum 1 -> default 0.5"#,
        );

        let resolved_types = resolver.resolve_types_internal(&spec_arc, true).unwrap();
        let percentage_type = resolved_types.named_types.get("percentage").unwrap();

        match &percentage_type.specifications {
            TypeSpecification::Ratio {
                minimum,
                maximum,
                default,
                ..
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
                assert_eq!(
                    *default,
                    Some(Decimal::from_i128_with_scale(5, 1)),
                    "ratio type with default command must work"
                );
            }
            _ => panic!("Expected Ratio type with minimum, maximum, and default"),
        }
    }

    #[test]
    fn test_scale_extension_chain_same_family_units_allowed() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
type money: scale -> unit eur 1
type money2: money -> unit usd 1.24"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, true);
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
        let (eur_type, _) = resolved.unit_index.get("eur").unwrap();
        let (usd_type, _) = resolved.unit_index.get("usd").unwrap();
        assert_eq!(
            eur_type.name.as_deref(),
            Some("money2"),
            "more derived type (money2) should own eur for conversion"
        );
        assert_eq!(usd_type.name.as_deref(), Some("money2"));
    }

    #[test]
    fn test_invalid_parent_type_in_named_type_should_error() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
type invalid: nonexistent_type -> minimum 0"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, true);
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
type invalid: choice -> option "a""#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, true);
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
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19

type money2: money
  -> unit eur 1.20
  -> unit usd 1.21
  -> unit gbp 1.30"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, true);
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
type money_a: scale
  -> unit eur 1.00
  -> unit usd 1.19

type money_b: scale
  -> unit eur 1.00
  -> unit usd 1.20

type length_a: scale
  -> unit meter 1.0

type length_b: scale
  -> unit meter 1.0"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, true);
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
type price: number
  -> unit eur 1.00"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, true);
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
    fn test_scale_type_can_have_units() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19"#,
        );

        let resolved = resolver.resolve_types_internal(&spec_arc, true).unwrap();
        let money_type = resolved.named_types.get("money").unwrap();

        match &money_type.specifications {
            TypeSpecification::Scale { units, .. } => {
                assert_eq!(units.len(), 2);
                assert!(units.iter().any(|u| u.name == "eur"));
                assert!(units.iter().any(|u| u.name == "usd"));
            }
            other => panic!("Expected Scale type specifications, got {:?}", other),
        }
    }

    #[test]
    fn test_extending_type_inherits_units() {
        let (resolver, spec_arc) = resolver_single_spec(
            r#"spec test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19

type my_money: money
  -> unit gbp 1.30"#,
        );

        let resolved = resolver.resolve_types_internal(&spec_arc, true).unwrap();
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
type money: scale
  -> unit eur 1.00
  -> unit eur 1.19"#,
        );

        let result = resolver.resolve_types_internal(&spec_arc, true);
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

    #[test]
    fn repro_named_type_source_location_panic() {
        use crate::parsing::ast::{CommandArg, ParentType, PrimitiveKind};
        let code = r#"spec nettoloon
type geld: scale
  -> decimals 2
  -> unit eur 1.00
  -> minimum 0 eur
fact bruto_salaris: 0 eur"#;
        let (mut resolver, spec_arc) = resolver_single_spec(code);
        let fact_ref = Reference::local("bruto_salaris".to_string());
        let inline_def = TypeDef::Inline {
            source_location: spec_arc.types[0].source_location().clone(),
            parent: ParentType::Primitive {
                primitive: PrimitiveKind::Scale,
            },
            constraints: Some(vec![(
                TypeConstraintCommand::Unit,
                vec![
                    CommandArg::Label("eur".to_string()),
                    CommandArg::Number("1.00".to_string()),
                ],
            )]),
            fact_ref: fact_ref.clone(),
            from: None,
        };
        resolver.register_type(&spec_arc, inline_def).unwrap();
        let _ = resolver.resolve_types_internal(&spec_arc, true);
    }
}
