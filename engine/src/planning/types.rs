//! Type registry for managing custom type definitions and resolution
//!
//! This module provides the `TypeResolver` (formerly TypeRegistry) which handles:
//! - Registering user-defined types for each spec
//! - Resolving type hierarchies and inheritance chains
//! - Detecting and preventing circular dependencies
//! - Applying constraints to create final type specifications

use crate::error::Error;
use crate::parsing::ast::{self as ast, CommandArg, LemmaSpec, Reference, TypeDef};
use crate::planning::semantics::{self, LemmaType, TypeExtends, TypeSpecification};
use crate::planning::validation::validate_type_specifications;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Fully resolved types for a single spec
/// After resolution, all imports are inlined - specs are independent
#[derive(Debug, Clone)]
pub struct ResolvedSpecTypes {
    /// Named types: type_name -> fully resolved type
    pub named_types: HashMap<String, LemmaType>,

    /// Inline type definitions: fact reference -> fully resolved type
    pub inline_type_definitions: HashMap<Reference, LemmaType>,

    /// Unit index: unit_name -> (resolved type, defining AST node if user-defined)
    /// Built during resolution - if unit appears in multiple types, resolution fails.
    /// TypeDef is kept for conflict detection (identity, extends-check, source location).
    /// Primitives (percent, permille) have no TypeDef.
    pub unit_index: HashMap<String, (LemmaType, Option<TypeDef>)>,
}

/// Registry for managing and resolving custom types
///
/// Types are organized per spec (keyed by Arc<LemmaSpec>) and support inheritance through parent references.
/// The registry handles cycle detection and accumulates constraints through the inheritance chain.
/// name_to_arc maps base spec name to the earliest Arc for that name (by effective_from) for cross-spec resolution.
#[derive(Debug, Clone)]
pub struct TypeResolver {
    named_types: HashMap<Arc<LemmaSpec>, HashMap<String, TypeDef>>,
    inline_type_definitions: HashMap<Arc<LemmaSpec>, HashMap<Reference, TypeDef>>,
    /// Earliest spec Arc per base name, for cross-spec type resolution.
    name_to_arc: HashMap<String, Arc<LemmaSpec>>,
}

impl TypeResolver {
    pub fn new() -> Self {
        TypeResolver {
            named_types: HashMap::new(),
            inline_type_definitions: HashMap::new(),
            name_to_arc: HashMap::new(),
        }
    }

    /// Register all named types from a spec (skips inline types).
    pub fn register_all(&mut self, spec: &Arc<LemmaSpec>) -> Vec<Error> {
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

    /// Resolve all named types for every spec and validate their specifications.
    /// Produces an entry for every spec (even those without named types) because
    /// every spec needs a unit_index containing at least the primitive ratio units.
    pub fn resolve(
        &self,
        all_specs: impl IntoIterator<Item = Arc<LemmaSpec>>,
    ) -> (HashMap<Arc<LemmaSpec>, ResolvedSpecTypes>, Vec<Error>) {
        let mut result = HashMap::new();
        let mut errors = Vec::new();

        for spec_arc in all_specs {
            let spec_arc = &spec_arc;
            match self.resolve_named_types(spec_arc) {
                Ok(resolved_types) => {
                    for (type_name, lemma_type) in &resolved_types.named_types {
                        let source = spec_arc
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
                                    "BUG: resolved named type '{}' has no corresponding TypeDef in spec '{}'",
                                    type_name, spec_arc.name
                                )
                            });
                        let mut spec_errors = validate_type_specifications(
                            &lemma_type.specifications,
                            type_name,
                            &source,
                            Some(Arc::clone(spec_arc)),
                        );
                        errors.append(&mut spec_errors);
                    }
                    result.insert(Arc::clone(spec_arc), resolved_types);
                }
                Err(es) => errors.extend(es),
            }
        }

        (result, errors)
    }

    /// Register a user-defined type for a given spec (keyed by Arc<LemmaSpec>).
    /// Updates name_to_arc to keep the earliest spec per base name for cross-spec resolution.
    pub fn register_type(&mut self, spec: &Arc<LemmaSpec>, def: TypeDef) -> Result<(), Error> {
        self.name_to_arc
            .entry(spec.name.clone())
            .and_modify(|existing| {
                if spec.effective_from() < existing.effective_from() {
                    *existing = Arc::clone(spec);
                }
            })
            .or_insert_with(|| Arc::clone(spec));

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

    /// Resolve all types for a certain spec (keyed by Arc<LemmaSpec>).
    pub fn resolve_types(&self, spec: &Arc<LemmaSpec>) -> Result<ResolvedSpecTypes, Vec<Error>> {
        self.resolve_types_internal(spec, true)
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

        // Resolve the parent type (standard or custom)
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
                source_type.clone(),
                Some(from.clone()),
                constraints.clone(),
                name.clone(),
            ),
            TypeDef::Inline { .. } => {
                // Inline types are resolved separately
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
                // Parent type not found - this is an error for named types
                // (inline type definitions might have forward references, but named types should be resolvable)
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
            match self.apply_constraints(
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

        let extends = if self.resolve_primitive_type(&parent).is_some() {
            TypeExtends::Primitive
        } else {
            let parent_spec_name = from
                .as_ref()
                .map(|r| r.name.as_str())
                .unwrap_or(spec.name.as_str());
            let parent_arc = self.name_to_arc.get(parent_spec_name);
            let family = match parent_arc {
                Some(arc) => match self.resolve_type_internal(arc, &parent, visited) {
                    Ok(Some(parent_type)) => parent_type
                        .scale_family_name()
                        .map(String::from)
                        .unwrap_or_else(|| parent.clone()),
                    Ok(None) => parent.clone(),
                    Err(es) => return Err(es),
                },
                None => parent.clone(),
            };
            TypeExtends::Custom {
                parent: parent.clone(),
                family,
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
        parent: &str,
        from: &Option<crate::parsing::ast::SpecRef>,
        visited: &mut HashSet<String>,
        source: &crate::Source,
    ) -> Result<Option<TypeSpecification>, Vec<Error>> {
        if let Some(specs) = self.resolve_primitive_type(parent) {
            return Ok(Some(specs));
        }

        let parent_spec_name = from
            .as_ref()
            .map(|r| r.name.as_str())
            .unwrap_or(spec.name.as_str());
        let parent_arc = self.name_to_arc.get(parent_spec_name);
        let result = match parent_arc {
            Some(arc) => self.resolve_type_internal(arc, parent, visited),
            None => Ok(None),
        };
        match result {
            Ok(Some(t)) => Ok(Some(t.specifications)),
            Ok(None) => {
                let type_exists = parent_arc
                    .and_then(|arc| self.named_types.get(arc))
                    .map(|spec_types| spec_types.contains_key(parent))
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

    /// Resolve a primitive type by name
    pub fn resolve_primitive_type(&self, name: &str) -> Option<TypeSpecification> {
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

    /// Apply command-argument constraints to a TypeSpecification.
    /// Each TypeSpecification variant handles its own commands; we just apply them in order.
    fn apply_constraints(
        &self,
        spec: &Arc<LemmaSpec>,
        mut specs: TypeSpecification,
        constraints: &[(String, Vec<CommandArg>)],
        source: &crate::Source,
    ) -> Result<TypeSpecification, Vec<Error>> {
        let mut errors = Vec::new();
        for (command, args) in constraints {
            let specs_clone = specs.clone();
            match specs.apply_constraint(command, args) {
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
            self.apply_constraints(spec, parent_specs, constraints, &def_loc)?
        } else {
            parent_specs
        };

        let extends = if self.resolve_primitive_type(parent).is_some() {
            TypeExtends::Primitive
        } else {
            let parent_spec_name = from
                .as_ref()
                .map(|r| r.name.as_str())
                .unwrap_or(spec.name.as_str());
            let family = match self.name_to_arc.get(parent_spec_name) {
                Some(arc) => match self.resolve_type_internal(arc, parent, visited) {
                    Ok(Some(parent_type)) => parent_type
                        .scale_family_name()
                        .map(String::from)
                        .unwrap_or_else(|| parent.to_string()),
                    Ok(None) => parent.to_string(),
                    Err(es) => return Err(es),
                },
                None => parent.to_string(),
            };
            TypeExtends::Custom {
                parent: parent.to_string(),
                family,
            }
        };

        Ok(Some(LemmaType::without_name(final_specs, extends)))
    }

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

impl Default for TypeResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::parsing::ast::LemmaSpec;
    use crate::ResourceLimits;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    fn test_registry() -> TypeResolver {
        TypeResolver::new()
    }

    fn test_spec_arc() -> Arc<LemmaSpec> {
        Arc::new(LemmaSpec::new("test_spec".to_string()))
    }

    #[test]
    fn test_registry_creation() {
        let registry = test_registry();
        let spec_arc = test_spec_arc();
        let resolved = registry.resolve_types(&spec_arc).unwrap();
        assert!(resolved.named_types.is_empty());
        assert!(resolved.inline_type_definitions.is_empty());
    }

    #[test]
    fn test_resolve_primitive_types() {
        let registry = test_registry();

        assert!(registry.resolve_primitive_type("boolean").is_some());
        assert!(registry.resolve_primitive_type("scale").is_some());
        assert!(registry.resolve_primitive_type("number").is_some());
        assert!(registry.resolve_primitive_type("ratio").is_some());
        assert!(registry.resolve_primitive_type("text").is_some());
        assert!(registry.resolve_primitive_type("date").is_some());
        assert!(registry.resolve_primitive_type("time").is_some());
        assert!(registry.resolve_primitive_type("duration").is_some());
        assert!(registry.resolve_primitive_type("unknown").is_none());
    }

    #[test]
    fn test_register_named_type() {
        let mut registry = test_registry();
        let type_def = TypeDef::Regular {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                Arc::from("spec test\nfact x: 1"),
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        let result = registry.register_type(&test_spec_arc(), type_def);
        assert!(result.is_ok());
    }

    #[test]
    fn test_register_inline_type_definition() {
        use crate::parsing::ast::Reference;
        let mut registry = test_registry();
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
                Arc::from("spec test\nfact x: 1"),
            ),
            parent: "number".to_string(),
            constraints: Some(vec![
                (
                    "minimum".to_string(),
                    vec![CommandArg::Number("0".to_string())],
                ),
                (
                    "maximum".to_string(),
                    vec![CommandArg::Number("150".to_string())],
                ),
            ]),
            fact_ref: fact_ref.clone(),
            from: None,
        };

        let spec_arc = test_spec_arc();
        let result = registry.register_type(&spec_arc, type_def);
        assert!(result.is_ok());
        let resolved = registry.resolve_types(&spec_arc).unwrap();
        assert!(resolved.inline_type_definitions.contains_key(&fact_ref));
    }

    #[test]
    fn test_register_duplicate_type_fails() {
        let mut registry = test_registry();
        let type_def = TypeDef::Regular {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                Arc::from("spec test\nfact x: 1"),
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        let spec_arc = test_spec_arc();
        registry.register_type(&spec_arc, type_def.clone()).unwrap();
        let result = registry.register_type(&spec_arc, type_def);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_custom_type_from_primitive() {
        let mut registry = test_registry();
        let type_def = TypeDef::Regular {
            source_location: crate::Source::new(
                "<test>",
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                Arc::from("spec test\nfact x: 1"),
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        let spec_arc = test_spec_arc();
        registry.register_type(&spec_arc, type_def).unwrap();
        let resolved = registry.resolve_types(&spec_arc).unwrap();

        assert!(resolved.named_types.contains_key("money"));
        let money_type = resolved.named_types.get("money").unwrap();
        assert_eq!(money_type.name, Some("money".to_string()));
    }

    #[test]
    fn test_type_definition_resolution() {
        let code = r#"spec test
type dice: number -> minimum 0 -> maximum 6"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        // Use TypeResolver to resolve the type
        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
        let dice_type = resolved_types.named_types.get("dice").unwrap();

        // Verify it's a Number type (dimensionless) with the correct constraints
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
        let code = r#"spec test
type money: scale -> decimals 2 -> unit eur 1.0 -> unit usd 1.18"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];
        let type_def = &spec.types[0];

        // Use TypeResolver to resolve the type
        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), type_def.clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
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
        let code = r#"spec test
type price: number -> decimals 2 -> minimum 0"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        // Use TypeResolver to resolve the type
        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
        let price_type = resolved_types.named_types.get("price").unwrap();

        // Verify it's a Number type with decimals set to 2
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
        let code = r#"spec test
type precise_number: number -> decimals 4"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
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
        let code = r#"spec test
type weight: scale -> unit kg 1 -> decimals 3"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
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
        let code = r#"spec test
type ratio_type: ratio -> decimals 2"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
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
        let code = r#"spec test
type percentage: ratio -> minimum 0 -> maximum 1 -> default 0.5"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
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
        let code = r#"spec test
type money: scale -> unit eur 1
type money2: money -> unit usd 1.24"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        for type_def in &spec.types {
            registry
                .register_type(&Arc::new(spec.clone()), type_def.clone())
                .unwrap();
        }

        let result = registry.resolve_types(&Arc::new(spec.clone()));
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
        let code = r#"spec test
type invalid: nonexistent_type -> minimum 0"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&Arc::new(spec.clone()));
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
        // "choice" is not a primitive type; this should fail resolution.
        let code = r#"spec test
type invalid: choice -> option "a""#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&Arc::new(spec.clone()));
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
        // Regression guard: overriding existing units should not silently succeed.
        let code = r#"spec test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19

type money2: money
  -> unit eur 1.20
  -> unit usd 1.21
  -> unit gbp 1.30"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        for type_def in &spec.types {
            registry
                .register_type(&Arc::new(spec.clone()), type_def.clone())
                .unwrap();
        }

        let result = registry.resolve_types(&Arc::new(spec.clone()));
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
        // Regression guard: the same unit name must not be defined by multiple types in one spec.
        let code = r#"spec test
type money_a: scale
  -> unit eur 1.00
  -> unit usd 1.19

type money_b: scale
  -> unit eur 1.00
  -> unit usd 1.20

type length_a: scale
  -> unit meter 1.0

type length_b: scale
  -> unit meter 1.0"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        for type_def in &spec.types {
            registry
                .register_type(&Arc::new(spec.clone()), type_def.clone())
                .unwrap();
        }

        let result = registry.resolve_types(&Arc::new(spec.clone()));
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
        let code = r#"spec test
type price: number
  -> unit eur 1.00"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&Arc::new(spec.clone()));
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
        let code = r#"spec test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let resolved = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
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
        let code = r#"spec test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19

type my_money: money
  -> unit gbp 1.30"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        for type_def in &spec.types {
            registry
                .register_type(&Arc::new(spec.clone()), type_def.clone())
                .unwrap();
        }

        let resolved = registry.resolve_types(&Arc::new(spec.clone())).unwrap();
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
        let code = r#"spec test
type money: scale
  -> unit eur 1.00
  -> unit eur 1.19"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec = &specs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(spec.clone()), spec.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&Arc::new(spec.clone()));
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
        use crate::parsing::ast::CommandArg;
        let code = r#"spec nettoloon
type geld: scale
  -> decimals 2
  -> unit eur 1.00
  -> minimum 0 eur
fact bruto_salaris: 0 eur"#;
        let specs = parse(code, "nettoloon.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec_arc = Arc::new(specs[0].clone());
        let mut registry = test_registry();
        for td in &spec_arc.types {
            registry.register_type(&spec_arc, td.clone()).unwrap();
        }
        let fact_ref = Reference::local("bruto_salaris".to_string());
        let inline_def = TypeDef::Inline {
            source_location: spec_arc.types[0].source_location().clone(),
            parent: "scale".to_string(),
            constraints: Some(vec![(
                "unit".to_string(),
                vec![
                    CommandArg::Label("eur".to_string()),
                    CommandArg::Number("1.00".to_string()),
                ],
            )]),
            fact_ref: fact_ref.clone(),
            from: None,
        };
        registry.register_type(&spec_arc, inline_def).unwrap();
        let _ = registry.resolve_types(&spec_arc);
    }
}
