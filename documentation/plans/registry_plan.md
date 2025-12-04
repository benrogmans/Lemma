# Registry Implementation Plan

## Overview

Enable remote document references via a Registry system. Documents starting with `@` are fetched from a configurable Registry rather than being provided locally. This enables sharing and reusing Lemma documents across organizations and workspaces.

**Key Principle**: Lemma is completely agnostic about the structure of remote document names. It just treats `@` followed by characters as a remote document name. The Registry implementation handles all parsing, versioning, and structure interpretation.

## Syntax

### Document Reference Format

Remote documents use the syntax: `doc @doc_name`

- `@` prefix indicates remote document
- `@doc_name` can contain any characters (including `/`, `:`, `-`, `_`)
- Lemma treats the entire string after `@` as an opaque document identifier
- The Registry implementation interprets the structure

Examples:
```lemma
fact pricing = doc @acme/pricing/base_pricing
fact taxes = doc @acme/compliance/tax_rules:v2
fact discounts = doc @partner/promotions/seasonal
```

### Local vs Remote

- Local: `fact contract = doc employment_contract`
- Remote: `fact pricing = doc @org/workspace/pricing`

## Architecture

### Registry Trait

```rust
/// Registry-specific error type
/// Registry implementations define their own error variants
pub enum RegistryError {
    NoRegistry,  // Only error Lemma cares about - no registry configured
    // Other variants are Registry implementation-specific
}

/// Trait for fetching remote documents
/// 
/// Registry implementations handle their own configuration (auth, endpoints, etc.)
/// via their constructors or builder patterns. The trait itself is agnostic about
/// how registries are configured.
pub trait Registry: Send + Sync {
    /// Fetch a document and all its dependencies
    /// `doc_ref` is the full remote document name including @ (e.g., "@org/workspace/pricing:v2")
    /// Registry implementation parses this string however it wants (version, segments, etc.)
    /// Returns the document content and all transitive dependencies
    /// Returns (doc_name, content) pairs where doc_name is the full remote name with @ prefix
    /// Errors are Registry-specific and returned as RegistryError
    fn fetch(&self, doc_ref: &str) -> Result<Vec<(String, String)>, RegistryError>;
}
```

**Configuration Pattern**: Registry implementations are created with their config externally, then passed to Engine:

```rust
// Example: LemmaBaseRegistry with token-based auth
let registry = LemmaBaseRegistry::new()
    .with_token("abc123")
    .with_endpoint("https://lemmabase.com/api")
    .build();

// Example: CustomRegistry with OAuth
let registry = CustomRegistry::new()
    .with_oauth_client(client)
    .with_cache(cache)
    .build();

// Pass to Engine
let engine = Engine::new().with_registry(Box::new(registry));
```

This keeps Lemma clean - it doesn't need to know about auth methods, endpoints, or other Registry-specific configuration.

### Semantic Types

Add to `lemma/src/semantic.rs`:

```rust
/// A remote document declaration
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteDoc {
    pub name: String,  // Full name including @ (e.g., "@org/workspace/pricing")
    pub source: Source,
}

/// Fact value for remote document references
pub enum FactValue {
    Literal(LiteralValue),
    DocumentReference(String),           // Local: "pricing"
    RemoteDocumentReference(String),     // Remote: "@org/workspace/pricing" (opaque string)
    TypeAnnotation(TypeAnnotation),
}
```

## Implementation Steps

### Phase 1: Grammar & Parsing

1. **Extend lemma.pest Grammar**
   - Extend `identifier` to allow `-`, `_`, `/`, `:` characters
   - Add `remote_doc_name` rule: `"@" ~ (!(SPACE | NEWLINE) ~ ANY)+`
   - Update `doc_name` to accept `remote_doc_name` or regular `identifier` path
   - Update `doc_declaration` to handle remote docs

2. **Parser Changes**
   - Update `parse_fact_document_reference()` to detect `@` prefix
   - If `@` detected, create `FactValue::RemoteDocumentReference(full_string)`
   - Store the complete string after `@` - no parsing or interpretation
   - If no `@`, create `FactValue::DocumentReference(doc_name)` as before
   - Add parsing for `RemoteDoc` in document declarations (for parsing fetched documents that declare themselves with @)

### Phase 2: Registry Module

3. **Create Registry Module**
   - Create `lemma/src/registry/mod.rs`
   - Define `Registry` trait and `RegistryError` (see above)
   - Create `resolve_remote_docs()` function:
     ```rust
     pub fn resolve_remote_docs(
         docs: Vec<LemmaDoc>,
         registry: Option<&dyn Registry>,
     ) -> Result<Vec<LemmaDoc>, RegistryError>
     ```
   - Function walks AST to collect all `RemoteDocumentReference` values
   - If remote refs exist but no registry, return `RegistryError::NoRegistry`
   - For each, calls `registry.fetch()` 
   - Registry returns `RegistryError` on failure (implementation-specific)
   - Parses fetched content and returns all docs (original + fetched)
   - Replaces `RemoteDocumentReference("@org/workspace/pricing")` with `DocumentReference("@org/workspace/pricing")` in AST
   - Note: After resolution, `@` is just part of the document name - no special handling needed
   - Source tracking: Same as local docs, but source_id uses database ID instead of filename

### Phase 3: Engine Integration

4. **Extend Engine**
   - Add `registry: Option<Box<dyn Registry>>` field to `Engine`
   - Add `with_registry(registry: Box<dyn Registry>)` constructor (similar to `with_limits()`)
   - Registry implementations are created externally with their config, then passed to Engine

5. **Update add_lemma_code()**
   - Flow: **parsing → registry resolution → planning → evaluation/inversion**
   - After parsing, before planning:
     - Call `registry::resolve_remote_docs(new_docs, registry.as_deref())`
     - Returns `Result<Vec<LemmaDoc>, RegistryError>`
     - Convert `RegistryError` to `LemmaError` if needed
     - Replace `RemoteDocumentReference` with `DocumentReference` in resolved docs
     - Continue with normal planning using resolved docs

6. **Validation**
   - Reject documents with `@` prefix in their own `doc` declaration
   - After parsing, check if any document name starts with `@`
   - Error: "Document names starting with '@' must be fetched from Registry, not provided directly"
   - Registry resolution handles the "no registry" case via `RegistryError::NoRegistry`

### Phase 4: Error Handling

7. **Error Handling**
   - Registry implementations return `RegistryError` (implementation-specific)
   - Lemma only checks: if remote refs exist but no registry configured, return error
   - All other errors come from Registry implementation via `RegistryError`
   - Registry fetch failures prevent document addition (atomic operation)

## Resolution Flow

1. **Parsing**: Parser detects `@` prefix and creates `FactValue::RemoteDocumentReference("@org/workspace/pricing")`
2. **Registry Resolution** (after parsing, before planning):
   - `resolve_remote_docs()` collects all `RemoteDocumentReference` values
   - For each, calls `registry.fetch("@org/workspace/pricing")`
   - Registry implementation parses the string however it wants
   - Parses fetched content (docs declare themselves as `doc @org/workspace/pricing`)
   - Replaces `RemoteDocumentReference("@org/workspace/pricing")` with `DocumentReference("@org/workspace/pricing")` in AST
   - Returns all docs (original + fetched)
3. **Planning**: Proceeds normally - all references are now `DocumentReference` with `@` as part of the name, everything works as before
4. **Evaluation/Inversion**: Unchanged

## Testing Strategy

### Unit Tests

1. **Parser Tests**
   - Parse `doc @org/workspace/doc`
   - Parse `doc @org/workspace/doc:v2`
   - Parse `doc @org-workspace_doc:version`
   - Verify `RemoteDocumentReference` created with full string
   - Reject documents with `@` in their own declaration

2. **Registry Module Tests**
   - Mock registry implementation
   - Test `resolve_remote_docs()` with remote refs
   - Test dependency resolution
   - Test error propagation
   - Test when registry is None

3. **Engine Integration Tests**
   - Add local doc that references remote doc
   - Verify remote doc is fetched and added
   - Verify error when registry unavailable
   - Verify error when remote doc doesn't exist
   - Test dependency chains (A -> @B -> @C)

### Integration Tests

4. **End-to-End**
   - Test with mock registry implementation
   - Test dependency chains
   - Test error cases

## Future: Registry Implementations

Future implementations (e.g., LemmaBaseRegistry) will:
- Parse the remote doc name string to extract @org, workspace, doc, version
- Handle their own configuration (auth tokens, OAuth, API keys, endpoints, etc.)
- Make HTTP requests to fetch documents
- Parse API responses
- Return documents with their full remote names

Registry implementations are created externally with their config, then passed to Engine.
This keeps Lemma agnostic about auth methods, endpoints, and Registry-specific details.