//! Type registry for managing custom type definitions and resolution
//!
//! This module provides the `TypeRegistry` which handles:
//! - Registering user-defined types for each document
//! - Resolving type hierarchies and inheritance chains
//! - Detecting and preventing circular dependencies
//! - Applying overrides to create final type specifications

use crate::error::LemmaError;
use crate::parsing::ast::Span;
use crate::semantic::{FactReference, LemmaType, TypeDef, TypeSpecification};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Fully resolved types for a single document
/// After resolution, all imports are inlined - documents are independent
#[derive(Debug)]
pub struct ResolvedDocumentTypes {
    /// Named types: type_name -> fully resolved type
    pub named_types: HashMap<String, LemmaType>,

    /// Inline type definitions: fact_reference -> fully resolved type
    pub inline_type_definitions: HashMap<FactReference, LemmaType>,

    /// Unit index: unit_name -> type that defines it
    /// Built during resolution - if unit appears in multiple types, resolution fails
    pub unit_index: HashMap<String, LemmaType>,
}

/// Registry for managing and resolving custom types
///
/// Types are organized per document and support inheritance through parent references.
/// The registry handles cycle detection and accumulates overrides through the inheritance chain.
#[derive(Debug, Clone)]
pub struct TypeRegistry {
    /// Named types per document: doc_name -> (type_name -> TypeDef)
    /// Stores the raw definitions extracted from the AST
    named_types: HashMap<String, HashMap<String, TypeDef>>,
    /// Inline type definitions per document: doc_name -> (fact_reference -> TypeDef)
    /// Stores inline type definitions keyed by their fact reference
    inline_type_definitions: HashMap<String, HashMap<FactReference, TypeDef>>,
}

impl TypeRegistry {
    /// Create a new, empty registry
    pub fn new() -> Self {
        TypeRegistry {
            named_types: HashMap::new(),
            inline_type_definitions: HashMap::new(),
        }
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
                        def_loc.span.clone(),
                        &def_loc.attribute,
                        Arc::from(""),
                        &def_loc.doc_name,
                        1,
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
                        def_loc.span.clone(),
                        &def_loc.attribute,
                        Arc::from(""),
                        &def_loc.doc_name,
                        1,
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
    /// Follows `parent` chains, accumulates overrides into `specifications`.
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
                    match self.resolve_inline_type_definition(
                        doc,
                        fact_ref,
                        type_def,
                        &mut visited,
                    )? {
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

        // Build unit index from both named and inline type definitions
        let mut unit_index: HashMap<String, LemmaType> = HashMap::new();
        let mut errors = Vec::new();

        // Add units from named types (collect all errors)
        for resolved_type in named_types.values() {
            if let Err(e) = self.add_units_to_index(&mut unit_index, resolved_type, doc, || {
                resolved_type
                    .name
                    .as_deref()
                    .unwrap_or("inline")
                    .to_string()
            }) {
                errors.push(e);
            }
        }

        // Add units from inline type definitions (collect all errors)
        for (fact_ref, resolved_type) in &inline_type_definitions {
            if let Err(e) = self.add_units_to_index(&mut unit_index, resolved_type, doc, || {
                format!("{}::{}", doc, fact_ref)
            }) {
                errors.push(e);
            }
        }

        // Return all collected errors if any
        if !errors.is_empty() {
            // Combine all errors into a single error message for now
            // (Future: LSP can use Vec<LemmaError> for better diagnostics)
            let combined_message = errors
                .iter()
                .map(|e| match e {
                    LemmaError::Engine(details) => details.message.clone(),
                    LemmaError::CircularDependency { details, .. } => details.message.clone(),
                    LemmaError::Parse(details) => details.message.clone(),
                    LemmaError::Semantic(details) => details.message.clone(),
                    LemmaError::Inversion(details) => details.message.clone(),
                    LemmaError::Runtime(details) => details.message.clone(),
                    LemmaError::MissingFact(details) => details.message.clone(),
                    LemmaError::ResourceLimitExceeded {
                        limit_name,
                        limit_value,
                        actual_value,
                        suggestion,
                    } => {
                        format!(
                            "Resource limit exceeded: {} (limit: {}, actual: {}). {}",
                            limit_name, limit_value, actual_value, suggestion
                        )
                    }
                    LemmaError::MultipleErrors(errs) => errs
                        .iter()
                        .map(|e| match e {
                            LemmaError::Engine(details) => details.message.clone(),
                            LemmaError::CircularDependency { details, .. } => {
                                details.message.clone()
                            }
                            LemmaError::Parse(details) => details.message.clone(),
                            LemmaError::Semantic(details) => details.message.clone(),
                            LemmaError::Inversion(details) => details.message.clone(),
                            LemmaError::Runtime(details) => details.message.clone(),
                            LemmaError::MissingFact(details) => details.message.clone(),
                            LemmaError::ResourceLimitExceeded {
                                limit_name,
                                limit_value,
                                actual_value,
                                suggestion,
                            } => {
                                format!(
                                    "Resource limit exceeded: {} (limit: {}, actual: {}). {}",
                                    limit_name, limit_value, actual_value, suggestion
                                )
                            }
                            LemmaError::MultipleErrors(_) => "Multiple errors".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join("; "),
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(LemmaError::engine(
                &combined_message,
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<internal>",
                Arc::from(""),
                doc,
                1,
                None::<String>,
            ));
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
            return Err(LemmaError::circular_dependency(
                format!("Circular dependency detected in type resolution: {}", key),
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<internal>",
                std::sync::Arc::from(""),
                doc,
                1,
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
        let (parent, from, overrides, type_name) = match &type_def {
            TypeDef::Regular {
                name,
                parent,
                overrides,
                ..
            } => (parent.clone(), None, overrides.clone(), name.clone()),
            TypeDef::Import {
                name,
                source_type,
                from,
                overrides,
                ..
            } => (
                source_type.clone(),
                Some(from.clone()),
                overrides.clone(),
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
                return Err(LemmaError::engine(
                    format!("Unknown type: '{}'. Type must be defined before use. Valid standard types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                    Span { start: 0, end: 0, line: 1, col: 0 },
                    "<internal>",
                    Arc::from(""),
                    doc,
                    1,
                    None::<String>,
                ));
            }
            Err(e) => {
                visited.remove(&key);
                return Err(e);
            }
        };

        // Apply overrides from the TypeDef
        let final_specs = if let Some(overrides) = &overrides {
            match self.apply_overrides(parent_specs, overrides, type_def.source_location()) {
                Ok(specs) => specs,
                Err(errors) => {
                    visited.remove(&key);
                    // Combine all errors into a single error message for now
                    // (Future: LSP can use Vec<LemmaError> for better diagnostics)
                    let combined_message = errors
                        .iter()
                        .map(|e| match e {
                            LemmaError::Engine(details) => details.message.clone(),
                            LemmaError::CircularDependency { details, .. } => details.message.clone(),
                            LemmaError::Parse(details) => details.message.clone(),
                            LemmaError::Semantic(details) => details.message.clone(),
                            LemmaError::Inversion(details) => details.message.clone(),
                            LemmaError::Runtime(details) => details.message.clone(),
                            LemmaError::MissingFact(details) => details.message.clone(),
                            LemmaError::ResourceLimitExceeded { limit_name, limit_value, actual_value, suggestion } => {
                                format!("Resource limit exceeded: {} (limit: {}, actual: {}). {}", limit_name, limit_value, actual_value, suggestion)
                            },
                            LemmaError::MultipleErrors(errs) => {
                                errs.iter().map(|e| match e {
                                    LemmaError::Engine(details) => details.message.clone(),
                                    LemmaError::CircularDependency { details, .. } => details.message.clone(),
                                    LemmaError::Parse(details) => details.message.clone(),
                                    LemmaError::Semantic(details) => details.message.clone(),
                                    LemmaError::Inversion(details) => details.message.clone(),
                                    LemmaError::Runtime(details) => details.message.clone(),
                                    LemmaError::MissingFact(details) => details.message.clone(),
                                    LemmaError::ResourceLimitExceeded { limit_name, limit_value, actual_value, suggestion } => {
                                        format!("Resource limit exceeded: {} (limit: {}, actual: {}). {}", limit_name, limit_value, actual_value, suggestion)
                                    },
                                    LemmaError::MultipleErrors(_) => "Multiple errors".to_string(),
                                }).collect::<Vec<_>>().join("; ")
                            },
                        })
                        .collect::<Vec<_>>()
                        .join("; ");
                    return Err(LemmaError::engine(
                        &combined_message,
                        Span {
                            start: 0,
                            end: 0,
                            line: 1,
                            col: 0,
                        },
                        &type_def.source_location().attribute,
                        Arc::from(""),
                        doc,
                        1,
                        None::<String>,
                    ));
                }
            }
        } else {
            parent_specs
        };

        visited.remove(&key);

        Ok(Some(LemmaType {
            name: Some(type_name),
            specifications: final_specs,
        }))
    }

    /// Resolve a parent type reference (standard or custom)
    fn resolve_parent(
        &self,
        doc: &str,
        parent: &str,
        from: &Option<String>,
        visited: &mut HashSet<String>,
        source: &crate::Source,
    ) -> Result<Option<TypeSpecification>, LemmaError> {
        // Try standard types first
        if let Some(specs) = self.resolve_standard_type(parent) {
            return Ok(Some(specs));
        }

        // Otherwise resolve as a custom type in the specified document (or same document if not specified)
        let parent_doc = from.as_deref().unwrap_or(doc);
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
                        format!("Unknown type: '{}'. Type must be defined before use. Valid standard types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                        source.span.clone(),
                        &source.attribute,
                        Arc::from(""),
                        &source.doc_name,
                        1,
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

    /// Resolve a standard type by name
    pub fn resolve_standard_type(&self, name: &str) -> Option<TypeSpecification> {
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

    /// Find which document a FactReference belongs to (for inline type definitions)
    pub fn find_document_for_fact(&self, fact_ref: &FactReference) -> Option<String> {
        for (doc_name, inline_types) in &self.inline_type_definitions {
            if inline_types.contains_key(fact_ref) {
                return Some(doc_name.clone());
            }
        }
        None
    }

    /// Apply command-argument overrides to a TypeSpecification
    fn apply_overrides(
        &self,
        mut specs: TypeSpecification,
        overrides: &[(String, Vec<String>)],
        source: &crate::Source,
    ) -> Result<TypeSpecification, Vec<LemmaError>> {
        // Extract existing units from parent type before applying overrides
        let mut existing_units: Vec<String> = match &specs {
            TypeSpecification::Scale { units, .. } => {
                units.iter().map(|u| u.name.clone()).collect()
            }
            TypeSpecification::Ratio { units, .. } => {
                units.iter().map(|u| u.name.clone()).collect()
            }
            _ => Vec::new(),
        };

        let mut errors = Vec::new();
        let mut valid_overrides = Vec::new();

        // First pass: validate all unit overrides and collect errors
        for (command, args) in overrides {
            if command == "unit" && !args.is_empty() {
                let unit_name = &args[0];
                if existing_units.iter().any(|u| u == unit_name) {
                    errors.push(LemmaError::engine(
                        format!("Unit '{}' already exists in parent type. Use a different unit name (e.g., 'my_{}') if you need another unit factor.", unit_name, unit_name),
                        source.span.clone(),
                        &source.attribute,
                        Arc::from(""),
                        &source.doc_name,
                        1,
                        None::<String>,
                    ));
                    // Skip this override
                } else {
                    existing_units.push(unit_name.clone());
                    valid_overrides.push((command.clone(), args.clone()));
                }
            } else {
                // Non-unit override - always valid to apply
                valid_overrides.push((command.clone(), args.clone()));
            }
        }

        // Second pass: apply all valid overrides and collect any application errors
        // We apply overrides sequentially, accumulating the result
        // If one fails, we can't continue with a valid state, but we still collect all errors
        for (command, args) in valid_overrides {
            // Clone specs before applying override so we can continue even if this one fails
            let specs_clone = specs.clone();
            match specs.apply_override(&command, &args) {
                Ok(updated_specs) => {
                    specs = updated_specs;
                }
                Err(e) => {
                    errors.push(LemmaError::engine(
                        format!("Failed to apply override '{}': {}", command, e),
                        source.span.clone(),
                        &source.attribute,
                        Arc::from(""),
                        &source.doc_name,
                        1,
                        None::<String>,
                    ));
                    // Restore from clone so we can continue trying other overrides
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
        _fact_ref: &FactReference,
        type_def: &TypeDef,
        visited: &mut HashSet<String>,
    ) -> Result<Option<LemmaType>, LemmaError> {
        let def_loc = type_def.source_location().clone();
        let TypeDef::Inline {
            parent,
            overrides,
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
                    format!("Unknown type: '{}'. Type must be defined before use. Valid standard types are: boolean, scale, number, ratio, text, date, time, duration, percent", parent),
                    def_loc.span.clone(),
                    &def_loc.attribute,
                    Arc::from(""),
                    &def_loc.doc_name,
                    1,
                    None::<String>,
                ));
            }
            Err(e) => return Err(e),
        };

        let final_specs = if let Some(overrides) = overrides {
            match self.apply_overrides(parent_specs, overrides, &def_loc) {
                Ok(specs) => specs,
                Err(errors) => {
                    // Combine all errors into a single error message for now
                    // (Future: LSP can use Vec<LemmaError> for better diagnostics)
                    let combined_message = errors
                        .iter()
                        .map(|e| match e {
                            LemmaError::Engine(details) => details.message.clone(),
                            LemmaError::CircularDependency { details, .. } => details.message.clone(),
                            LemmaError::Parse(details) => details.message.clone(),
                            LemmaError::Semantic(details) => details.message.clone(),
                            LemmaError::Inversion(details) => details.message.clone(),
                            LemmaError::Runtime(details) => details.message.clone(),
                            LemmaError::MissingFact(details) => details.message.clone(),
                            LemmaError::ResourceLimitExceeded { limit_name, limit_value, actual_value, suggestion } => {
                                format!("Resource limit exceeded: {} (limit: {}, actual: {}). {}", limit_name, limit_value, actual_value, suggestion)
                            },
                            LemmaError::MultipleErrors(errs) => {
                                errs.iter().map(|e| match e {
                                    LemmaError::Engine(details) => details.message.clone(),
                                    LemmaError::CircularDependency { details, .. } => details.message.clone(),
                                    LemmaError::Parse(details) => details.message.clone(),
                                    LemmaError::Semantic(details) => details.message.clone(),
                                    LemmaError::Inversion(details) => details.message.clone(),
                                    LemmaError::Runtime(details) => details.message.clone(),
                                    LemmaError::MissingFact(details) => details.message.clone(),
                                    LemmaError::ResourceLimitExceeded { limit_name, limit_value, actual_value, suggestion } => {
                                        format!("Resource limit exceeded: {} (limit: {}, actual: {}). {}", limit_name, limit_value, actual_value, suggestion)
                                    },
                                    LemmaError::MultipleErrors(_) => "Multiple errors".to_string(),
                                }).collect::<Vec<_>>().join("; ")
                            },
                        })
                        .collect::<Vec<_>>()
                        .join("; ");
                    return Err(LemmaError::engine(
                        &combined_message,
                        Span {
                            start: 0,
                            end: 0,
                            line: 1,
                            col: 0,
                        },
                        &def_loc.attribute,
                        Arc::from(""),
                        &def_loc.doc_name,
                        1,
                        None::<String>,
                    ));
                }
            }
        } else {
            parent_specs
        };

        Ok(Some(LemmaType::without_name(final_specs)))
    }

    /// Get all units defined across all types, organized by unit name
    ///
    /// Returns a map from unit name to a list of (document_name, type_name) pairs
    /// where that unit is defined. Useful for error messages and debugging.
    ///
    /// # Returns
    /// A HashMap where:
    /// - Key: unit name (e.g., "celsius", "kilogram")
    /// - Value: list of (document_name, type_name) pairs where the unit is defined
    pub fn get_all_units(&self) -> HashMap<String, Vec<(String, String)>> {
        let mut unit_map: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut visited = HashSet::new();

        // Process named types
        for (doc_name, doc_types) in &self.named_types {
            for type_name in doc_types.keys() {
                if let Ok(Some(resolved_type)) =
                    self.resolve_type_internal(doc_name, type_name, &mut visited)
                {
                    let units = self.extract_units_from_specs(&resolved_type.specifications);
                    for unit in units {
                        unit_map
                            .entry(unit)
                            .or_default()
                            .push((doc_name.clone(), type_name.clone()));
                    }
                }
                visited.clear();
            }
        }

        // Process inline type definitions
        for (doc_name, inline_types) in &self.inline_type_definitions {
            for (fact_ref, type_def) in inline_types {
                if let Ok(Some(resolved_type)) = self.resolve_inline_type_definition(
                    doc_name,
                    fact_ref,
                    type_def,
                    &mut HashSet::new(),
                ) {
                    let units = self.extract_units_from_specs(&resolved_type.specifications);
                    let type_name = format!("{}::{}", doc_name, fact_ref);
                    for unit in units {
                        unit_map
                            .entry(unit)
                            .or_default()
                            .push((doc_name.clone(), type_name.clone()));
                    }
                }
            }
        }

        unit_map
    }

    /// Add units from a resolved type to the unit index
    /// Returns an error if a unit is already defined in another type (ambiguous unit)
    fn add_units_to_index<F>(
        &self,
        unit_index: &mut HashMap<String, LemmaType>,
        resolved_type: &LemmaType,
        doc: &str,
        get_type_name: F,
    ) -> Result<(), LemmaError>
    where
        F: FnOnce() -> String,
    {
        let units = self.extract_units_from_specs(&resolved_type.specifications);
        let current_name = get_type_name(); // Call FnOnce before the loop
        for unit in units {
            if let Some(existing_type) = unit_index.get(&unit) {
                let existing_name = existing_type.name.as_deref().unwrap_or("inline");

                // Check if one type extends the other
                // If the existing type's name matches the current type's name, they're the same type
                let same_type = existing_type.name.as_deref() == resolved_type.name.as_deref();
                if same_type {
                    // Same type - not ambiguous, just continue
                    continue;
                }

                // Check if types share the same base and if one might extend the other
                // by comparing their specifications - if one has all units from the other plus MORE,
                // it's likely an extension (strict superset required)
                // Check both directions: current extends existing OR existing extends current
                let might_be_inheritance =
                    match (&existing_type.specifications, &resolved_type.specifications) {
                        (
                            TypeSpecification::Scale {
                                units: existing_units,
                                ..
                            },
                            TypeSpecification::Scale {
                                units: current_units,
                                ..
                            },
                        ) => {
                            // Check if current extends existing (current has all of existing + more)
                            let current_extends_existing = existing_units.len()
                                < current_units.len()
                                && existing_units
                                    .iter()
                                    .all(|eu| current_units.iter().any(|cu| cu.name == eu.name));
                            // Check if existing extends current (existing has all of current + more)
                            let existing_extends_current = current_units.len()
                                < existing_units.len()
                                && current_units
                                    .iter()
                                    .all(|cu| existing_units.iter().any(|eu| eu.name == cu.name));
                            current_extends_existing || existing_extends_current
                        }
                        (
                            TypeSpecification::Ratio {
                                units: existing_units,
                                ..
                            },
                            TypeSpecification::Ratio {
                                units: current_units,
                                ..
                            },
                        ) => {
                            // Check if current extends existing (current has all of existing + more)
                            let current_extends_existing = existing_units.len()
                                < current_units.len()
                                && existing_units
                                    .iter()
                                    .all(|eu| current_units.iter().any(|cu| cu.name == eu.name));
                            // Check if existing extends current (existing has all of current + more)
                            let existing_extends_current = current_units.len()
                                < existing_units.len()
                                && current_units
                                    .iter()
                                    .all(|cu| existing_units.iter().any(|eu| eu.name == cu.name));
                            current_extends_existing || existing_extends_current
                        }
                        _ => false,
                    };

                if might_be_inheritance {
                    // One type likely extends the other - allow unit sharing via inheritance
                    continue;
                }

                let source = self
                    .named_types
                    .get(doc)
                    .and_then(|defs| defs.get(&current_name))
                    .map(|def| def.source_location());

                return Err(LemmaError::engine(
                    format!("Ambiguous unit '{}' in document '{}'. Defined in multiple types: {} and {}", unit, doc, existing_name, current_name),
                    source
                        .map(|s| s.span.clone())
                        .unwrap_or(Span {
                            start: 0,
                            end: 0,
                            line: 1,
                            col: 0,
                        }),
                    source.map(|s| s.attribute.as_str()).unwrap_or("<input>"),
                    Arc::from(""),
                    source.map(|s| s.doc_name.as_str()).unwrap_or(doc),
                    1,
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
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::ResourceLimits;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn test_registry_creation() {
        let registry = TypeRegistry::new();
        assert!(registry.named_types.is_empty());
        assert!(registry.inline_type_definitions.is_empty());
    }

    #[test]
    fn test_resolve_standard_types() {
        let registry = TypeRegistry::new();

        assert!(registry.resolve_standard_type("boolean").is_some());
        assert!(registry.resolve_standard_type("scale").is_some());
        assert!(registry.resolve_standard_type("number").is_some());
        assert!(registry.resolve_standard_type("ratio").is_some());
        assert!(registry.resolve_standard_type("text").is_some());
        assert!(registry.resolve_standard_type("date").is_some());
        assert!(registry.resolve_standard_type("time").is_some());
        assert!(registry.resolve_standard_type("duration").is_some());
        assert!(registry.resolve_standard_type("unknown").is_none());
    }

    #[test]
    fn test_register_named_type() {
        let mut registry = TypeRegistry::new();
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
            overrides: None,
        };

        let result = registry.register_type("test_doc", type_def);
        assert!(result.is_ok());
    }

    #[test]
    fn test_register_inline_type_definition() {
        use crate::semantic::FactReference;
        let mut registry = TypeRegistry::new();
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
            overrides: Some(vec![
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
        let mut registry = TypeRegistry::new();
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
            overrides: None,
        };

        registry
            .register_type("test_doc", type_def.clone())
            .unwrap();
        let result = registry.register_type("test_doc", type_def);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_custom_type_from_standard() {
        let mut registry = TypeRegistry::new();
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
            overrides: None,
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
        let mut registry = TypeRegistry::new();
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
        let mut registry = TypeRegistry::new();
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
        let mut registry = TypeRegistry::new();
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

        let mut registry = TypeRegistry::new();
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
type weight = scale -> decimals 3"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = TypeRegistry::new();
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
    fn test_ratio_type_rejects_decimals_command() {
        let code = r#"doc test
type ratio_type = ratio -> decimals 2"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = TypeRegistry::new();
        // register_type only stores the definition, it doesn't apply overrides
        registry
            .register_type(&doc.name, doc.types[0].clone())
            .unwrap();

        // resolve_types applies overrides, so the error should occur here
        let result = registry.resolve_types(&doc.name);

        // Ratio type should reject decimals command since it doesn't have a decimals field
        assert!(
            result.is_err(),
            "Ratio type should reject decimals command during resolution"
        );
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("decimals") || error_msg.contains("Invalid command"),
            "Error message should mention decimals or invalid command, got: {}",
            error_msg
        );
    }

    #[test]
    fn test_ratio_type_with_default_command() {
        let code = r#"doc test
type percentage = ratio -> minimum 0 -> maximum 1 -> default 0.5"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = TypeRegistry::new();
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
                assert_eq!(*minimum, Some(Decimal::from(0)));
                assert_eq!(*maximum, Some(Decimal::from(1)));
                assert_eq!(*default, Some(Decimal::from_str("0.5").unwrap()));
            }
            _ => panic!("Expected Ratio type specifications"),
        }
    }

    #[test]
    fn test_invalid_parent_type_in_named_type_should_error() {
        let code = r#"doc test
type invalid = nonexistent_type -> minimum 0"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = TypeRegistry::new();
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
    fn test_invalid_standard_type_name_should_error() {
        // "choice" is not a standard type; this should fail resolution.
        let code = r#"doc test
type invalid = choice -> option "a""#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let doc = &docs[0];

        let mut registry = TypeRegistry::new();
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
    fn test_unit_override_validation_errors_are_reported() {
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

        let mut registry = TypeRegistry::new();
        for type_def in &doc.types {
            registry.register_type(&doc.name, type_def.clone()).unwrap();
        }

        let result = registry.resolve_types(&doc.name);
        assert!(result.is_err(), "Expected unit override conflicts to error");

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

        let mut registry = TypeRegistry::new();
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

        let mut registry = TypeRegistry::new();
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

        let mut registry = TypeRegistry::new();
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

        let mut registry = TypeRegistry::new();
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

        let mut registry = TypeRegistry::new();
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
