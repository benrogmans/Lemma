// Analysis tests

// Engine tests
mod engine;

// Parser tests (moved to src/parsing modules)
mod fact_bindings;

// AST tests (types from parsing::ast, formerly semantic)
mod ast;

// Error tests
mod error;

// Serializer tests
mod serializers;

// Proof tests moved to integration tests (tests/proof_e2e.rs)
