//! Type registry for managing custom type definitions and resolution
//!
//! This module provides the `TypeRegistry` which handles:
//! - Registering user-defined types for each document
//! - Resolving type hierarchies and inheritance chains
//! - Detecting and preventing circular dependencies
//! - Applying constraints to create final type specifications

use crate::error::LemmaError;
use crate::parsing::ast::{FactReference, TypeDef};
use crate::planning::semantics::{self, LemmaType, TypeExtends, TypeSpecification};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Fully resolved types for a single document
/// After resolution, all imports are inlined - documents are independent
#[derive(Debug, Clone)]
pub struct ResolvedDocumentTypes {
    /// Named types: type_name -> fully resolved type
    pub named_types: HashMap<String, LemmaType>,

    /// Inline type definitions: fact reference -> fully resolved type
    pub inline_type_definitions: HashMap<FactReference, LemmaType>,

    /// Unit index: unit_name -> type that defines it
    /// Built during resolution - if unit appears in multiple types, resolution fails
    pub unit_index: HashMap<String, LemmaType>,
}

/// Registry for managing and resolving custom types
///
/// Types are organized per document and support inheritance through parent references.
/// The registry handles cycle detection and accumulates constraints through the inheritance chain.
#[derive(Debug, Clone)]
pub struct TypeRegistry {
    /// Named types per document: doc_name -> (type_name -> TypeDef)
    /// Stores the raw definitions extracted from the AST
    named_types: HashMap<String, HashMap<String, TypeDef>>,
    /// Inline type definitions per document: doc_name -> (fact_reference -> TypeDef)
    /// Stores inline type definitions keyed by their fact reference
    inline_type_definitions: HashMap<String, HashMap<FactReference, TypeDef>>,
    /// Source text per file attribute, used for error reporting.
    /// Maps source attribute (e.g. filename) to the full source code string.
    sources: HashMap<String, String>,
}

impl TypeRegistry {
    /// Create a new, empty registry with access to source text for error reporting.
    pub fn new(sources: HashMap<String, String>) -> Self {
        TypeRegistry {
            named_types: HashMap::new(),
            inline_type_definitions: HashMap::new(),
            sources,
        }
    }

    /// Look up the source text for a given source location.
    /// Panics if the source attribute is not found in the sources map,
    /// because that indicates a bug in the pipeline (all sources must be registered).
    fn source_text_for(&self, source: &crate::Source) -> Arc<str> {
        let text = self.sources.get(&source.attribute).unwrap_or_else(|| {
            unreachable!(
                "BUG: missing source text for attribute '{}' (doc '{}') in TypeRegistry",
                source.attribute, source.doc_name
            )
        });
        Arc::from(text.as_str())
    }

    /// Register a user-defined type for a given document
    pub fn register_type(&mut self, doc: &str, def: TypeDef) -> Result<(), LemmaError> {
        let def_loc = def.source_location().clone();
        match &def {
            TypeDef::Regular { name, .. } | TypeDef::Import { name, .. } => {
                // Named type
                let doc_types = self.named_types.entry(doc.to_string()).or_default();

                // Check if this type already exists
                if doc_types.contains_key(name) {
                    return Err(LemmaError::engine(
                        format!("Type '{}' is already defined in document '{}'", name, doc),
                        def_loc.clone(),
                        self.source_text_for(&def_loc),
                        None::<String>,
                    ));
                }

                // Store the type definition
                doc_types.insert(name.clone(), def);
            }
            TypeDef::Inline { fact_ref, .. } => {
                // Inline type definition
                let doc_inline_types = self
                    .inline_type_definitions
                    .entry(doc.to_string())
                    .or_default();

                // Check if this inline type definition already exists
                if doc_inline_types.contains_key(fact_ref) {
                    return Err(LemmaError::engine(
                        format!(
                            "Inline type definition for fact '{}' is already defined in document '{}'",
                            fact_ref.fact, doc
                        ),
                        def_loc.clone(),
                        self.source_text_for(&def_loc),
                        None::<String>,
                    ));
                }

                // Store the inline type definition
                doc_inline_types.insert(fact_ref.clone(), def);
            }
        }
        Ok(())
    }

    /// Resolve all types for a certain document
    ///
    /// Returns fully resolved types for the document, including named types, inline type definitions,
    /// and a unit index. After resolution, all imports are inlined - documents are independent.
    /// Follows `parent` chains, accumulates constraints into `specifications`.
    /// Handles cycle detection and cross-document references.
    ///
    /// # Errors
    /// Returns an error if a unit appears in multiple types within the same document (ambiguous unit).
    pub fn resolve_types(&self, doc: &str) -> Result<ResolvedDocumentTypes, LemmaError> {
        self.resolve_types_internal(doc, true)
    }

    /// Resolve only named types (for validation before inline type definitions are registered)
    pub fn resolve_named_types(&self, doc: &str) -> Result<ResolvedDocumentTypes, LemmaError> {
        self.resolve_types_internal(doc, false)
    }

    /// Resolve only inline type definitions and merge them into an existing
    /// `ResolvedDocumentTypes` that already contains the named types.
    ///
    /// This avoids re-resolving named types that were already handled by
    /// [`resolve_named_types`](Self::resolve_named_types) during the
    /// `prepare_types` phase, preventing duplicate errors.
    pub fn resolve_inline_types(
        &self,
        doc: &str,
        mut existing: ResolvedDocumentTypes,
    ) -> Result<ResolvedDocumentTypes, LemmaError> {
        let mut errors = Vec::new();

        // Resolve inline type definitions only
        if let Some(doc_inline_types) = self.inline_type_definitions.get(doc) {
            for (fact_ref, type_def) in doc_inline_types {
                let mut visited = HashSet::new();
                match self.resolve_inline_type_definition(doc, type_def, &mut visited)? {
                    Some(resolved_type) => {
                        existing
                            .inline_type_definitions
                            .insert(fact_ref.clone(), resolved_type);
                    }
                    None => {
                        unreachable!(
                            "BUG: registered inline type definition for fact '{}' could not be resolved (doc='{}')",
                            fact_ref, doc
                        );
                    }
                }
            }
        }

        // Extend the unit index with units from inline type definitions
        for (fact_ref, resolved_type) in &existing.inline_type_definitions {
            let inline_type_name = format!("{}::{}", doc, fact_ref);
            let e = if resolved_type.is_scale() {
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
            return Err(LemmaError::MultipleErrors(errors));
        }

        Ok(existing)
    }

    fn resolve_types_internal(
        &self,
        doc: &str,
        include_anonymous: bool,
    ) -> Result<ResolvedDocumentTypes, LemmaError> {
        let mut named_types = HashMap::new();
        let mut inline_type_definitions = HashMap::new();
        let mut visited = HashSet::new();

        // Resolve named types
        if let Some(doc_types) = self.named_types.get(doc) {
            for type_name in doc_types.keys() {
                match self.resolve_type_internal(doc, type_name, &mut visited)? {
                    Some(resolved_type) => {
                        named_types.insert(type_name.clone(), resolved_type);
                    }
                    None => {
                        unreachable!(
                            "BUG: registered named type '{}' could not be resolved (doc='{}')",
                            type_name, doc
                        );
                    }
                }
                visited.clear();
            }
        }

        // Resolve inline type definitions (only if requested)
        if include_anonymous {
            if let Some(doc_inline_types) = self.inline_type_definitions.get(doc) {
                for (fact_ref, type_def) in doc_inline_types {
                    let mut visited = HashSet::new();
                    match self.resolve_inline_type_definition(doc, type_def, &mut visited)? {
                        Some(resolved_type) => {
                            inline_type_definitions.insert(fact_ref.clone(), resolved_type);
                        }
                        None => {
                            unreachable!(
                                "BUG: registered inline type definition for fact '{}' could not be resolved (doc='{}')",
                                fact_ref, doc
                            );
                        }
                    }
                }
            }
        }

        // Build unit index from types that have units (primitive types first, then document types)
        let mut unit_index: HashMap<String, LemmaType> = HashMap::new();
        let mut errors = Vec::new();

        // Add all standard ratio units to the index
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
            let e = if resolved_type.is_scale() {
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
            let inline_type_name = format!("{}::{}", doc, fact_ref);
            let e = if resolved_type.is_scale() {
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

        // Return all collected errors if any, each with its own real source location
        if !errors.is_empty() {
            return Err(LemmaError::MultipleErrors(errors));
        }

        Ok(ResolvedDocumentTypes {
            named_types,
            inline_type_definitions,
            unit_index,
        })
    }

    /// Resolve a single type with cycle detection
    fn resolve_type_internal(
        &self,
        doc: &str,
        name: &str,
        visited: &mut HashSet<String>,
    ) -> Result<Option<LemmaType>, LemmaError> {
        // Cycle detection using doc::name key
        let key = format!("{}::{}", doc, name);
        if visited.contains(&key) {
            let source_location = self
                .named_types
                .get(doc)
                .and_then(|dt| dt.get(name))
                .map(|td| td.source_location().clone())
                .unwrap_or_else(|| {
                    unreachable!(
                        "BUG: circular dependency detected for type '{}::{}' but type definition not found in registry",
                        doc, name
                    )
                });
            let source_text = self.source_text_for(&source_location);
            return Err(LemmaError::circular_dependency(
                format!("Circular dependency detected in type resolution: {}", key),
                source_location,
                source_text,
                vec![],
                None::<String>,
            ));
        }
        visited.insert(key.clone());

        // Get the TypeDef from the document (check named types)
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
                return Err(LemmaError::engine(
                    format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                    source.clone(),
                    self.source_text_for(&source),
                    None::<String>,
                ));
            }
            Err(e) => {
                visited.remove(&key);
                return Err(e);
            }
        };

        // Apply constraints from the TypeDef
        let final_specs = if let Some(constraints) = &constraints {
            match self.apply_constraints(parent_specs, constraints, type_def.source_location()) {
                Ok(specs) => specs,
                Err(errors) => {
                    visited.remove(&key);
                    return Err(LemmaError::MultipleErrors(errors));
                }
            }
        } else {
            parent_specs
        };

        visited.remove(&key);

        // Determine extends based on whether parent is standard or custom
        let extends = if self.resolve_primitive_type(&parent).is_some() {
            TypeExtends::Primitive
        } else {
            let parent_doc = from.as_ref().map(|r| r.name.as_str()).unwrap_or(doc);
            let family = self
                .resolve_type_internal(parent_doc, &parent, visited)
                .ok()
                .flatten()
                .and_then(|parent_type| parent_type.scale_family_name().map(String::from))
                .unwrap_or_else(|| parent.clone());
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

    /// Resolve a parent type reference (standard or custom)
    fn resolve_parent(
        &self,
        doc: &str,
        parent: &str,
        from: &Option<crate::parsing::ast::DocRef>,
        visited: &mut HashSet<String>,
        source: &crate::Source,
    ) -> Result<Option<TypeSpecification>, LemmaError> {
        // Try primitive types first
        if let Some(specs) = self.resolve_primitive_type(parent) {
            return Ok(Some(specs));
        }

        // Otherwise resolve as a custom type in the specified document (or same document if not specified).
        // DocRef.name is already the clean name (@ stripped by parser).
        let parent_doc = from.as_ref().map(|r| r.name.as_str()).unwrap_or(doc);
        match self.resolve_type_internal(parent_doc, parent, visited) {
            Ok(Some(t)) => Ok(Some(t.specifications)),
            Ok(None) => {
                // Parent type not found - check if it was ever registered
                let type_exists = if let Some(doc_types) = self.named_types.get(parent_doc) {
                    doc_types.contains_key(parent)
                } else {
                    false
                };

                if !type_exists {
                    // Type was never registered - invalid parent type
                    Err(LemmaError::engine(
                        format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                        source.clone(),
                        self.source_text_for(source),
                        None::<String>,
                    ))
                } else {
                    // Type exists but couldn't be resolved (circular dependency or other issue)
                    // Return None - the caller will handle this appropriately
                    Ok(None)
                }
            }
            Err(e) => Err(e),
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
        constraints: &[(String, Vec<String>)],
        source: &crate::Source,
    ) -> Result<TypeSpecification, Vec<LemmaError>> {
        let mut errors = Vec::new();
        for (command, args) in constraints {
            let specs_clone = specs.clone();
            match specs.apply_constraint(command, args) {
                Ok(updated_specs) => specs = updated_specs,
                Err(e) => {
                    errors.push(LemmaError::engine(
                        format!("Failed to apply constraint '{}': {}", command, e),
                        source.clone(),
                        self.source_text_for(source),
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

    /// Resolve an inline type definition from its definition
    fn resolve_inline_type_definition(
        &self,
        doc: &str,
        type_def: &TypeDef,
        visited: &mut HashSet<String>,
    ) -> Result<Option<LemmaType>, LemmaError> {
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
                // Parent type not found - this is an error for inline type definitions too
                // Inline type definitions should have valid parent types
                return Err(LemmaError::engine(
                    format!("Unknown type: '{}'. Type must be defined before use. Valid primitive types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                    def_loc.clone(),
                    self.source_text_for(&def_loc),
                    None::<String>,
                ));
            }
            Err(e) => return Err(e),
        };

        let final_specs = if let Some(constraints) = constraints {
            match self.apply_constraints(parent_specs, constraints, &def_loc) {
                Ok(specs) => specs,
                Err(errors) => {
                    return Err(LemmaError::MultipleErrors(errors));
                }
            }
        } else {
            parent_specs
        };

        // Determine extends based on whether parent is standard or custom
        let extends = if self.resolve_primitive_type(parent).is_some() {
            TypeExtends::Primitive
        } else {
            let parent_doc = from.as_ref().map(|r| r.name.as_str()).unwrap_or(doc);
            let family = self
                .resolve_type_internal(parent_doc, parent, visited)
                .ok()
                .flatten()
                .and_then(|parent_type| parent_type.scale_family_name().map(String::from))
                .unwrap_or_else(|| parent.to_string());
            TypeExtends::Custom {
                parent: parent.to_string(),
                family,
            }
        };

        Ok(Some(LemmaType::without_name(final_specs, extends)))
    }

    /// Add units from a scale type to the unit index.
    /// Same unit in same type = error. Same unit in scale extension chain (same family) = allow. Otherwise ambiguous.
    fn add_scale_units_to_index(
        &self,
        unit_index: &mut HashMap<String, LemmaType>,
        resolved_type: &LemmaType,
        doc: &str,
        type_name: &str,
    ) -> Result<(), LemmaError> {
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

                    return Err(LemmaError::engine(
                        format!(
                            "Unit '{}' is defined more than once in type '{}'",
                            unit, type_name
                        ),
                        source.clone(),
                        self.source_text_for(&source),
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

                let source = self
                    .named_types
                    .get(doc)
                    .and_then(|defs| defs.get(type_name))
                    .map(|def| def.source_location().clone())
                    .expect("BUG: named type definition must have source location");

                return Err(LemmaError::engine(
                    format!(
                        "Ambiguous unit '{}' in document '{}'. Defined in multiple types: {} and {}",
                        unit, doc, existing_name, type_name
                    ),
                    source.clone(),
                    self.source_text_for(&source),
                    None::<String>,
                ));
            }
            unit_index.insert(unit, resolved_type.clone());
        }
        Ok(())
    }

    /// Add ratio units to the unit index. Ratio units are document-scoped singleton: merged across all ratio types.
    fn add_ratio_units_to_index(
        &self,
        unit_index: &mut HashMap<String, LemmaType>,
        resolved_type: &LemmaType,
        doc: &str,
        type_name: &str,
    ) -> Result<(), LemmaError> {
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

                return Err(LemmaError::engine(
                    format!(
                        "Ambiguous unit '{}' in document '{}'. Defined in multiple types: {} and {}",
                        unit, doc, existing_name, type_name
                    ),
                    source.clone(),
                    self.source_text_for(&source),
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

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::ResourceLimits;
    use rust_decimal::Decimal;

    fn test_registry() -> TypeRegistry {
        let mut sources = HashMap::new();
        sources.insert("<test>".to_string(), String::new());
        sources.insert("test.lemma".to_string(), String::new());
        TypeRegistry::new(sources)
    }

    #[test]
    fn test_registry_creation() {
        let registry = test_registry();
        assert!(registry.named_types.is_empty());
        assert!(registry.inline_type_definitions.is_empty());
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
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        let result = registry.register_type("test_doc", type_def);
        assert!(result.is_ok());
    }

    #[test]
    fn test_register_inline_type_definition() {
        use crate::parsing::ast::FactReference;
        let mut registry = test_registry();
        let fact_ref = FactReference::local("age".to_string());
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
            ),
            parent: "number".to_string(),
            constraints: Some(vec![
                ("minimum".to_string(), vec!["0".to_string()]),
                ("maximum".to_string(), vec!["150".to_string()]),
            ]),
            fact_ref: fact_ref.clone(),
            from: None,
        };

        let result = registry.register_type("test_doc", type_def);
        assert!(result.is_ok());

        // Verify the inline type definition is registered
        assert!(registry
            .inline_type_definitions
            .get("test_doc")
            .unwrap()
            .contains_key(&fact_ref));
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
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        registry
            .register_type("test_doc", type_def.clone())
            .unwrap();
        let result = registry.register_type("test_doc", type_def);
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
            ),
            name: "money".to_string(),
            parent: "number".to_string(),
            constraints: None,
        };

        registry.register_type("test_doc", type_def).unwrap();
        let resolved = registry.resolve_types("test_doc").unwrap();

        assert!(resolved.named_types.contains_key("money"));
        let money_type = resolved.named_types.get("money").unwrap();
        assert_eq!(money_type.name, Some("money".to_string()));
    }

    #[test]
    fn test_type_definition_resolution() {
        let code = r#"doc test
type dice = number -> minimum 0 -> maximum 6"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        // Use TypeRegistry to resolve the type
        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&doc.name).unwrap();
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
type money = scale -> decimals 2 -> unit eur 1.0 -> unit usd 1.18"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];
        let type_def = &doc.types[0];

        // Use TypeRegistry to resolve the type
        let mut registry = test_registry();
        registry.register_type(&doc.name, type_def.clone()).unwrap();

        let resolved_types = registry.resolve_types(&doc.name).unwrap();
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
type price = number -> decimals 2 -> minimum 0"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        // Use TypeRegistry to resolve the type
        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&doc.name).unwrap();
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
type precise_number = number -> decimals 4"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&doc.name).unwrap();
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
type weight = scale -> unit kg 1 -> decimals 3"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&doc.name).unwrap();
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
type ratio_type = ratio -> decimals 2"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&doc.name).unwrap();
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
type percentage = ratio -> minimum 0 -> maximum 1 -> default 0.5"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let resolved_types = registry.resolve_types(&doc.name).unwrap();
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
type money = scale -> unit eur 1
type money2 = money -> unit usd 1.24"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        for type_def in &doc.types {
            registry.register_type(&doc.name, type_def.clone()).unwrap();
        }

        let result = registry.resolve_types(&doc.name);
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
type invalid = nonexistent_type -> minimum 0"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&doc.name);
        assert!(result.is_err(), "Should reject invalid parent type");

        let error_msg = result.unwrap_err().to_string();
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
type invalid = choice -> option "a""#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&doc.name);
        assert!(result.is_err(), "Should reject invalid type base 'choice'");

        let error_msg = result.unwrap_err().to_string();
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
type money = scale
  -> unit eur 1.00
  -> unit usd 1.19

type money2 = money
  -> unit eur 1.20
  -> unit usd 1.21
  -> unit gbp 1.30"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        for type_def in &doc.types {
            registry.register_type(&doc.name, type_def.clone()).unwrap();
        }

        let result = registry.resolve_types(&doc.name);
        assert!(
            result.is_err(),
            "Expected unit constraint conflicts to error"
        );

        let error_msg = result.unwrap_err().to_string();
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
type money_a = scale
  -> unit eur 1.00
  -> unit usd 1.19

type money_b = scale
  -> unit eur 1.00
  -> unit usd 1.20

type length_a = scale
  -> unit meter 1.0

type length_b = scale
  -> unit meter 1.0"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        for type_def in &doc.types {
            registry.register_type(&doc.name, type_def.clone()).unwrap();
        }

        let result = registry.resolve_types(&doc.name);
        assert!(
            result.is_err(),
            "Expected ambiguous unit definitions to error"
        );

        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("eur") || error_msg.contains("usd") || error_msg.contains("meter"),
            "Error should mention at least one ambiguous unit. Got: {}",
            error_msg
        );
    }

    #[test]
    fn test_number_type_cannot_have_units() {
        let code = r#"doc test
type price = number
  -> unit eur 1.00"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&doc.name);
        assert!(result.is_err(), "Number types must reject unit commands");

        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("unit") && error_msg.contains("number"),
            "Error should mention units are invalid on number. Got: {}",
            error_msg
        );
    }

    #[test]
    fn test_scale_type_can_have_units() {
        let code = r#"doc test
type money = scale
  -> unit eur 1.00
  -> unit usd 1.19"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let resolved = registry.resolve_types(&doc.name).unwrap();
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
type money = scale
  -> unit eur 1.00
  -> unit usd 1.19

type my_money = money
  -> unit gbp 1.30"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        for type_def in &doc.types {
            registry.register_type(&doc.name, type_def.clone()).unwrap();
        }

        let resolved = registry.resolve_types(&doc.name).unwrap();
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
type money = scale
  -> unit eur 1.00
  -> unit eur 1.19"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = test_registry();
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        let result = registry.resolve_types(&doc.name);
        assert!(
            result.is_err(),
            "Duplicate units within a type should error"
        );

        let error_msg = result.unwrap_err().to_string();
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
