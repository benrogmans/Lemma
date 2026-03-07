//! Type registry for managing custom type definitions and resolution
//!
//! This module provides the `TypeResolver` (formerly TypeRegistry) which handles:
//! - Registering user-defined types for each document
//! - Resolving type hierarchies and inheritance chains
//! - Detecting and preventing circular dependencies
//! - Applying constraints to create final type specifications

use crate::error::Error;
use crate::parsing::ast::{self as ast, CommandArg, LemmaDoc, Reference, TypeDef};
use crate::planning::semantics::{self, LemmaType, TypeExtends, TypeSpecification};
use crate::planning::validation::validate_type_specifications;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Fully resolved types for a single document
/// After resolution, all imports are inlined - documents are independent
#[derive(Debug, Clone)]
pub struct ResolvedDocumentTypes {
    /// Named types: type_name -> fully resolved type
    pub named_types: HashMap<String, LemmaType>,

    /// Inline type definitions: fact reference -> fully resolved type
    pub inline_type_definitions: HashMap<Reference, LemmaType>,

    /// Unit index: unit_name -> type that defines it
    /// Built during resolution - if unit appears in multiple types, resolution fails
    pub unit_index: HashMap<String, LemmaType>,
}

/// Registry for managing and resolving custom types
///
/// Types are organized per document (keyed by Arc<LemmaDoc>) and support inheritance through parent references.
/// The registry handles cycle detection and accumulates constraints through the inheritance chain.
/// name_to_arc maps base document name to the earliest Arc for that name (by effective_from) for cross-doc resolution.
#[derive(Debug, Clone)]
pub struct TypeResolver {
    named_types: HashMap<Arc<LemmaDoc>, HashMap<String, TypeDef>>,
    inline_type_definitions: HashMap<Arc<LemmaDoc>, HashMap<Reference, TypeDef>>,
    /// Earliest doc Arc per base name, for cross-document type resolution.
    name_to_arc: HashMap<String, Arc<LemmaDoc>>,
}

impl TypeResolver {
    pub fn new() -> Self {
        TypeResolver {
            named_types: HashMap::new(),
            inline_type_definitions: HashMap::new(),
            name_to_arc: HashMap::new(),
        }
    }

    /// Register all named types from a document (skips inline types).
    pub fn register_all(&mut self, doc: &Arc<LemmaDoc>) -> Vec<Error> {
        let mut errors = Vec::new();
        for type_def in &doc.types {
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
                ) {
                    errors.push(e);
                    continue;
                }
            }
            if let Err(e) = self.register_type(doc, type_def.clone()) {
                errors.push(e);
            }
        }
        errors
    }

    /// Resolve all named types for every doc and validate their specifications.
    /// Produces an entry for every doc (even those without named types) because
    /// every doc needs a unit_index containing at least the primitive ratio units.
    pub fn resolve(
        &self,
        all_docs: impl IntoIterator<Item = Arc<LemmaDoc>>,
    ) -> (HashMap<Arc<LemmaDoc>, ResolvedDocumentTypes>, Vec<Error>) {
        let mut result = HashMap::new();
        let mut errors = Vec::new();

        for doc_arc in all_docs {
            let doc_arc = &doc_arc;
            match self.resolve_named_types(doc_arc) {
                Ok(document_types) => {
                    for (type_name, lemma_type) in &document_types.named_types {
                        let source = doc_arc
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
                                    type_name, doc_arc.name
                                )
                            });
                        let mut spec_errors = validate_type_specifications(
                            &lemma_type.specifications,
                            type_name,
                            &source,
                        );
                        errors.append(&mut spec_errors);
                    }
                    result.insert(Arc::clone(doc_arc), document_types);
                }
                Err(es) => errors.extend(es),
            }
        }

        (result, errors)
    }

    /// Register a user-defined type for a given document (keyed by Arc<LemmaDoc>).
    /// Updates name_to_arc to keep the earliest doc per base name for cross-doc resolution.
    pub fn register_type(&mut self, doc: &Arc<LemmaDoc>, def: TypeDef) -> Result<(), Error> {
        self.name_to_arc
            .entry(doc.name.clone())
            .and_modify(|existing| {
                if doc.effective_from() < existing.effective_from() {
                    *existing = Arc::clone(doc);
                }
            })
            .or_insert_with(|| Arc::clone(doc));

        let def_loc = def.source_location().clone();
        let doc_name = &doc.name;
        match &def {
            TypeDef::Regular { name, .. } | TypeDef::Import { name, .. } => {
                let doc_types = self.named_types.entry(Arc::clone(doc)).or_default();
                if doc_types.contains_key(name) {
                    return Err(Error::validation(
                        format!(
                            "Type '{}' is already defined in document '{}'",
                            name, doc_name
                        ),
                        Some(def_loc.clone()),
                        None::<String>,
                    ));
                }
                doc_types.insert(name.clone(), def);
            }
            TypeDef::Inline { fact_ref, .. } => {
                let doc_inline_types = self
                    .inline_type_definitions
                    .entry(Arc::clone(doc))
                    .or_default();
                if doc_inline_types.contains_key(fact_ref) {
                    return Err(Error::validation(
                        format!(
                            "Inline type definition for fact '{}' is already defined in document '{}'",
                            fact_ref.name, doc_name
                        ),
                        Some(def_loc.clone()),
                        None::<String>,
                    ));
                }
                doc_inline_types.insert(fact_ref.clone(), def);
            }
        }
        Ok(())
    }

    /// Resolve all types for a certain document (keyed by Arc<LemmaDoc>).
    pub fn resolve_types(&self, doc: &Arc<LemmaDoc>) -> Result<ResolvedDocumentTypes, Vec<Error>> {
        self.resolve_types_internal(doc, true)
    }

    /// Resolve only named types (for validation before inline type definitions are registered).
    pub fn resolve_named_types(
        &self,
        doc: &Arc<LemmaDoc>,
    ) -> Result<ResolvedDocumentTypes, Vec<Error>> {
        self.resolve_types_internal(doc, false)
    }

    /// Resolve only inline type definitions and merge them into an existing
    /// `ResolvedDocumentTypes` that already contains the named types.
    pub fn resolve_inline_types(
        &self,
        doc: &Arc<LemmaDoc>,
        mut existing: ResolvedDocumentTypes,
    ) -> Result<ResolvedDocumentTypes, Vec<Error>> {
        let mut errors = Vec::new();

        if let Some(doc_inline_types) = self.inline_type_definitions.get(doc) {
            for (fact_ref, type_def) in doc_inline_types {
                let mut visited = HashSet::new();
                match self.resolve_inline_type_definition(doc, type_def, &mut visited) {
                    Ok(Some(resolved_type)) => {
                        existing
                            .inline_type_definitions
                            .insert(fact_ref.clone(), resolved_type);
                    }
                    Ok(None) => {
                        unreachable!(
                            "BUG: registered inline type definition for fact '{}' could not be resolved (doc='{}')",
                            fact_ref, doc.name
                        );
                    }
                    Err(es) => return Err(es),
                }
            }
        }

        for (fact_ref, resolved_type) in &existing.inline_type_definitions {
            let inline_type_name = format!("{}::{}", doc.name, fact_ref);
            let e: Result<(), Error> = if resolved_type.is_scale() {
                self.add_scale_units_to_index(
                    &mut existing.unit_index,
                    resolved_type,
                    doc,
                    &inline_type_name,
                )
            } else if resolved_type.is_ratio() {
                self.add_ratio_units_to_index(
                    &mut existing.unit_index,
                    resolved_type,
                    doc,
                    &inline_type_name,
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

        Ok(existing)
    }

    fn resolve_types_internal(
        &self,
        doc: &Arc<LemmaDoc>,
        include_anonymous: bool,
    ) -> Result<ResolvedDocumentTypes, Vec<Error>> {
        let mut named_types = HashMap::new();
        let mut inline_type_definitions = HashMap::new();
        let mut visited = HashSet::new();

        if let Some(doc_types) = self.named_types.get(doc) {
            for type_name in doc_types.keys() {
                match self.resolve_type_internal(doc, type_name, &mut visited) {
                    Ok(Some(resolved_type)) => {
                        named_types.insert(type_name.clone(), resolved_type);
                    }
                    Ok(None) => {
                        unreachable!(
                            "BUG: registered named type '{}' could not be resolved (doc='{}')",
                            type_name, doc.name
                        );
                    }
                    Err(es) => return Err(es),
                }
                visited.clear();
            }
        }

        if include_anonymous {
            if let Some(doc_inline_types) = self.inline_type_definitions.get(doc) {
                for (fact_ref, type_def) in doc_inline_types {
                    let mut visited = HashSet::new();
                    match self.resolve_inline_type_definition(doc, type_def, &mut visited) {
                        Ok(Some(resolved_type)) => {
                            inline_type_definitions.insert(fact_ref.clone(), resolved_type);
                        }
                        Ok(None) => {
                            unreachable!(
                                "BUG: registered inline type definition for fact '{}' could not be resolved (doc='{}')",
                                fact_ref, doc.name
                            );
                        }
                        Err(es) => return Err(es),
                    }
                }
            }
        }

        // Build unit index from types that have units (primitive types first, then document types)
        let mut unit_index: HashMap<String, LemmaType> = HashMap::new();
        let mut errors = Vec::new();

        if let Err(error) = self.add_ratio_units_to_index(
            &mut unit_index,
            semantics::primitive_ratio(),
            doc,
            "ratio",
        ) {
            errors.push(error);
        }

        // Add units from named types (collect all errors)
        for resolved_type in named_types.values() {
            let type_name = resolved_type.name.as_deref().unwrap_or("inline");
            let e: Result<(), Error> = if resolved_type.is_scale() {
                self.add_scale_units_to_index(&mut unit_index, resolved_type, doc, type_name)
            } else if resolved_type.is_ratio() {
                self.add_ratio_units_to_index(&mut unit_index, resolved_type, doc, type_name)
            } else {
                Ok(())
            };
            if let Err(e) = e {
                errors.push(e);
            }
        }

        // Add units from inline type definitions (collect all errors)
        for (fact_ref, resolved_type) in &inline_type_definitions {
            let inline_type_name = format!("{}::{}", doc.name, fact_ref);
            let e: Result<(), Error> = if resolved_type.is_scale() {
                self.add_scale_units_to_index(
                    &mut unit_index,
                    resolved_type,
                    doc,
                    &inline_type_name,
                )
            } else if resolved_type.is_ratio() {
                self.add_ratio_units_to_index(
                    &mut unit_index,
                    resolved_type,
                    doc,
                    &inline_type_name,
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

        Ok(ResolvedDocumentTypes {
            named_types,
            inline_type_definitions,
            unit_index,
        })
    }

    fn resolve_type_internal(
        &self,
        doc: &Arc<LemmaDoc>,
        name: &str,
        visited: &mut HashSet<String>,
    ) -> Result<Option<LemmaType>, Vec<Error>> {
        let key = format!("{}::{}", doc.name, name);
        if visited.contains(&key) {
            let source_location = self
                .named_types
                .get(doc)
                .and_then(|dt| dt.get(name))
                .map(|td| td.source_location().clone())
                .unwrap_or_else(|| {
                    unreachable!(
                        "BUG: circular dependency detected for type '{}::{}' but type definition not found in registry",
                        doc.name, name
                    )
                });
            return Err(vec![Error::validation(
                format!("Circular dependency detected in type resolution: {}", key),
                Some(source_location),
                None::<String>,
            )]);
        }
        visited.insert(key.clone());

        let type_def = match self.named_types.get(doc).and_then(|dt| dt.get(name)) {
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
            doc,
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
                return Err(vec![Error::validation(
                    format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                    Some(source.clone()),
                    None::<String>,
                )]);
            }
            Err(es) => {
                visited.remove(&key);
                return Err(es);
            }
        };

        let final_specs = if let Some(constraints) = &constraints {
            match self.apply_constraints(parent_specs, constraints, type_def.source_location()) {
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
            let parent_doc_name = from
                .as_ref()
                .map(|r| r.name.as_str())
                .unwrap_or(doc.name.as_str());
            let parent_arc = self.name_to_arc.get(parent_doc_name);
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
        doc: &Arc<LemmaDoc>,
        parent: &str,
        from: &Option<crate::parsing::ast::DocRef>,
        visited: &mut HashSet<String>,
        source: &crate::Source,
    ) -> Result<Option<TypeSpecification>, Vec<Error>> {
        if let Some(specs) = self.resolve_primitive_type(parent) {
            return Ok(Some(specs));
        }

        let parent_doc_name = from
            .as_ref()
            .map(|r| r.name.as_str())
            .unwrap_or(doc.name.as_str());
        let parent_arc = self.name_to_arc.get(parent_doc_name);
        let result = match parent_arc {
            Some(arc) => self.resolve_type_internal(arc, parent, visited),
            None => Ok(None),
        };
        match result {
            Ok(Some(t)) => Ok(Some(t.specifications)),
            Ok(None) => {
                let type_exists = parent_arc
                    .and_then(|arc| self.named_types.get(arc))
                    .map(|doc_types| doc_types.contains_key(parent))
                    .unwrap_or(false);

                if !type_exists {
                    Err(vec![Error::validation(
                        format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                        Some(source.clone()),
                        None::<String>,
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
                    errors.push(Error::validation(
                        format!("Failed to apply constraint '{}': {}", command, e),
                        Some(source.clone()),
                        None::<String>,
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
        doc: &Arc<LemmaDoc>,
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

        let parent_specs = match self.resolve_parent(doc, parent, from, visited, &def_loc) {
            Ok(Some(specs)) => specs,
            Ok(None) => {
                return Err(vec![Error::validation(
                    format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                    Some(def_loc.clone()),
                    None::<String>,
                )]);
            }
            Err(es) => return Err(es),
        };

        let final_specs = if let Some(constraints) = constraints {
            self.apply_constraints(parent_specs, constraints, &def_loc)?
        } else {
            parent_specs
        };

        let extends = if self.resolve_primitive_type(parent).is_some() {
            TypeExtends::Primitive
        } else {
            let parent_doc_name = from
                .as_ref()
                .map(|r| r.name.as_str())
                .unwrap_or(doc.name.as_str());
            let family = match self.name_to_arc.get(parent_doc_name) {
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
        &self,
        unit_index: &mut HashMap<String, LemmaType>,
        resolved_type: &LemmaType,
        doc: &Arc<LemmaDoc>,
        type_name: &str,
    ) -> Result<(), Error> {
        let units = self.extract_units_from_specs(&resolved_type.specifications);
        for unit in units {
            if let Some(existing_type) = unit_index.get(&unit) {
                let existing_name = existing_type.name.as_deref().unwrap_or("inline");
                let same_type = existing_type.name.as_deref() == resolved_type.name.as_deref();

                if same_type {
                    let source = self
                        .named_types
                        .get(doc)
                        .and_then(|defs| defs.get(type_name))
                        .map(|def| def.source_location().clone())
                        .expect("BUG: named type definition must have source location");

                    return Err(Error::validation(
                        format!(
                            "Unit '{}' is defined more than once in type '{}'",
                            unit, type_name
                        ),
                        Some(source.clone()),
                        None::<String>,
                    ));
                }

                let current_extends_existing = resolved_type
                    .extends
                    .parent_name()
                    .map(|p| existing_name == p)
                    .unwrap_or(false);
                let existing_extends_current = existing_type
                    .extends
                    .parent_name()
                    .map(|p| p == resolved_type.name.as_deref().unwrap_or(""))
                    .unwrap_or(false);

                if existing_type.is_scale()
                    && (current_extends_existing || existing_extends_current)
                {
                    if current_extends_existing {
                        unit_index.insert(unit, resolved_type.clone());
                    }
                    continue;
                }

                // Siblings in the same scale family (e.g. both extend "money")
                // inherit the same unit — not ambiguous.
                if existing_type.same_scale_family(resolved_type) {
                    continue;
                }

                let source = self
                    .named_types
                    .get(doc)
                    .and_then(|defs| defs.get(type_name))
                    .map(|def| def.source_location().clone())
                    .expect("BUG: named type definition must have source location");

                return Err(Error::validation(
                    format!(
                        "Ambiguous unit '{}' in document '{}'. Defined in multiple types: {} and {}",
                        unit, doc.name, existing_name, type_name
                    ),
                    Some(source.clone()),
                    None::<String>,
                ));
            }
            unit_index.insert(unit, resolved_type.clone());
        }
        Ok(())
    }

    fn add_ratio_units_to_index(
        &self,
        unit_index: &mut HashMap<String, LemmaType>,
        resolved_type: &LemmaType,
        doc: &Arc<LemmaDoc>,
        type_name: &str,
    ) -> Result<(), Error> {
        let units = self.extract_units_from_specs(&resolved_type.specifications);
        for unit in units {
            if let Some(existing_type) = unit_index.get(&unit) {
                if existing_type.is_ratio() {
                    continue;
                }
                let existing_name = existing_type.name.as_deref().unwrap_or("inline");
                let source = self
                    .named_types
                    .get(doc)
                    .and_then(|defs| defs.get(type_name))
                    .map(|def| def.source_location().clone())
                    .expect("BUG: named type definition must have source location");

                return Err(Error::validation(
                    format!(
                        "Ambiguous unit '{}' in document '{}'. Defined in multiple types: {} and {}",
                        unit, doc.name, existing_name, type_name
                    ),
                    Some(source.clone()),
                    None::<String>,
                ));
            }
            unit_index.insert(unit, resolved_type.clone());
        }
        Ok(())
    }

    /// Extract all unit names from a TypeSpecification
    /// Only Scale types can have units (Number types are dimensionless)
    fn extract_units_from_specs(&self, specs: &TypeSpecification) -> Vec<String> {
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
    use crate::parsing::ast::LemmaDoc;
    use crate::ResourceLimits;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    fn test_registry() -> TypeResolver {
        TypeResolver::new()
    }

    fn test_doc_arc() -> Arc<LemmaDoc> {
        Arc::new(LemmaDoc::new("test_doc".to_string()))
    }

    #[test]
    fn test_registry_creation() {
        let registry = test_registry();
        let doc = test_doc_arc();
        let resolved = registry.resolve_types(&doc).unwrap();
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
                "test_doc",
                Arc::from("doc test\nfact x: 1"),
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        let result = registry.register_type(&test_doc_arc(), type_def);
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
                "test_doc",
                Arc::from("doc test\nfact x: 1"),
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

        let doc = test_doc_arc();
        let result = registry.register_type(&doc, type_def);
        assert!(result.is_ok());
        let resolved = registry.resolve_types(&doc).unwrap();
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
                "test_doc",
                Arc::from("doc test\nfact x: 1"),
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        let doc = test_doc_arc();
        registry.register_type(&doc, type_def.clone()).unwrap();
        let result = registry.register_type(&doc, type_def);
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
                "test_doc",
                Arc::from("doc test\nfact x: 1"),
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        let doc = test_doc_arc();
        registry.register_type(&doc, type_def).unwrap();
        let resolved = registry.resolve_types(&doc).unwrap();

        assert!(resolved.named_types.contains_key("money"));
        let money_type = resolved.named_types.get("money").unwrap();
        assert_eq!(money_type.name, Some("money".to_string()));
    }

    #[test]
    fn test_type_definition_resolution() {
        let code = r#"doc test
type dice: number -> minimum 0 -> maximum 6"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        // Use TypeResolver to resolve the type
        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type money: scale -> decimals 2 -> unit eur 1.0 -> unit usd 1.18"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];
        let type_def = &doc.types[0];

        // Use TypeResolver to resolve the type
        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), type_def.clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type price: number -> decimals 2 -> minimum 0"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        // Use TypeResolver to resolve the type
        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type precise_number: number -> decimals 4"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type weight: scale -> unit kg 1 -> decimals 3"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type ratio_type: ratio -> decimals 2"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type percentage: ratio -> minimum 0 -> maximum 1 -> default 0.5"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type money: scale -> unit eur 1
type money2: money -> unit usd 1.24"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        for type_def in &doc.types {
            registry
                .register_type(&Arc::new(doc.clone()), type_def.clone())
                .unwrap();
        }

        let result = registry.resolve_types(&Arc::new(doc.clone()));
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
            Some("money2"),
            "more derived type (money2) should own eur for conversion"
        );
        assert_eq!(usd_type.name.as_deref(), Some("money2"));
    }

    #[test]
    fn test_invalid_parent_type_in_named_type_should_error() {
        let code = r#"doc test
type invalid: nonexistent_type -> minimum 0"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&Arc::new(doc.clone()));
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
        let code = r#"doc test
type invalid: choice -> option "a""#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&Arc::new(doc.clone()));
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
        let code = r#"doc test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19

type money2: money
  -> unit eur 1.20
  -> unit usd 1.21
  -> unit gbp 1.30"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        for type_def in &doc.types {
            registry
                .register_type(&Arc::new(doc.clone()), type_def.clone())
                .unwrap();
        }

        let result = registry.resolve_types(&Arc::new(doc.clone()));
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
    fn test_document_level_unit_ambiguity_errors_are_reported() {
        // Regression guard: the same unit name must not be defined by multiple types in one doc.
        let code = r#"doc test
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

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        for type_def in &doc.types {
            registry
                .register_type(&Arc::new(doc.clone()), type_def.clone())
                .unwrap();
        }

        let result = registry.resolve_types(&Arc::new(doc.clone()));
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
        let code = r#"doc test
type price: number
  -> unit eur 1.00"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&Arc::new(doc.clone()));
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
        let code = r#"doc test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let resolved = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19

type my_money: money
  -> unit gbp 1.30"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        for type_def in &doc.types {
            registry
                .register_type(&Arc::new(doc.clone()), type_def.clone())
                .unwrap();
        }

        let resolved = registry.resolve_types(&Arc::new(doc.clone())).unwrap();
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
        let code = r#"doc test
type money: scale
  -> unit eur 1.00
  -> unit eur 1.19"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&Arc::new(doc.clone()), doc.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&Arc::new(doc.clone()));
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
