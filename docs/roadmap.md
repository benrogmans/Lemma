# Roadmap

This document tracks planned improvements and known issues for the Lemma project.

## Security

### Input Validation Review (Priority: High)

**Issue**: Need security audit of parser and evaluator to ensure proper handling of user input.

**Areas to review**:
- String literal parsing and escaping
- Identifier validation and sanitization
- Expression evaluation safety

### Resource Limits (Priority: High)

**Issue**: No limits on computational resources.

**Risks**:
- Maximum document size (files stored in memory via Arc)
- Maximum expression depth (stack overflow risk during parsing/evaluation)
- Evaluation timeout (infinite loops or very expensive computations)
- Memory usage limits (thousands of documents)
- Maximum identifier/string length

**Solution**: Add configurable limits with reasonable defaults.

**Suggested Defaults**:
- Max file size: 10 MB
- Max expression depth: 100 levels
- Max evaluation time: 10 seconds
- Max documents in workspace: 1000
- Max identifier length: 256 characters

---

## Performance

### Add Benchmarks (Priority: Medium)

**Issue**: No performance baseline or regression detection.

**Solution**: Use `criterion` to benchmark:
- Parse time for various document sizes
- Evaluation time for different complexity levels
- End-to-end evaluation time
- Unit conversion operations

### Optimize Module Caching (Priority: Low)

**Issue**: `evaluate()` with fact overrides may recreate evaluation context each time.

**Solution**: Cache or reuse evaluation contexts for repeated queries with same fact overrides.

---

## Developer Experience

### LSP Support (Priority: Medium)

**Issue**: No IDE support for `.lemma` files.

**Solution**: Create Language Server Protocol implementation for:
- Syntax highlighting
- Error checking
- Auto-completion
- Go-to-definition
- Hover documentation

### Enhanced Interactive Mode (Priority: Low)

**Issue**: Interactive mode exists but could be more powerful.

**Enhancements**:
- Multi-line document editing
- Command history and autocomplete
- Live evaluation as you type
- Step-through debugging of rule evaluation

---

## Planned Features

### User Types

**Feature**: Allow users to define custom types with units and conversions within documents.

**Benefits**:
- Domain-specific units (currencies, stock prices, custom measurements)
- Enum types for business logic (status, priority levels)
- Declarative conversion equations with automatic bidirectional conversion
- Doc-scoped types prevent namespace pollution
- Enables currency conversion with custom exchange rates

**Implementation**: See [user_types.md](user_types.md) for implementation plan.

**Estimated Effort**: 10 days

### Multi Facts

**Feature**: Support facts that hold multiple values with declarative list operations.

**Benefits**:
- Work with collections of data (salaries, employees, transactions)
- Declarative aggregations (sum, avg, min, max, count)
- Filtering with `where` clauses
- Indexed access to list items
- Type-safe operations on collections

**Implementation**: See [multi_facts.md](multi_facts.md) for details.

**Estimated Effort**: 20 days

---

## How to Contribute

If you'd like to work on any of these improvements:

1. Check if someone else is already working on it (check open issues/PRs)
2. Create an issue discussing your approach
3. Submit a PR referencing the issue
4. Ensure tests pass and documentation is updated
