# Inversion

**Last Updated:** 12 November 2025

Core inversion functionality is complete and production-ready. This document describes current capabilities and enhancement opportunities.

## Current API

```rust
engine.invert(
    document: &str,
    rule: &str,
    target: Target,
    given_facts: HashMap<String, LiteralValue>,
) -> LemmaResult<Vec<HashMap<FactPath, Domain>>>
```

**Target options:**
- `Target::value(v)` - Find inputs producing specific value
- `Target::any_value()` - Find inputs producing any non-veto value
- `Target::any_veto()` - Find inputs that cause vetos
- `Target::with_op(op, outcome)` - Use comparison operators (Eq, Gt, Lt, Gte, Lte)

**Example:**
```rust
// Find valid quantity ranges for a discount rule
let solutions = engine.invert(
    "pricing",
    "discount",
    Target::any_value(),
    HashMap::new()
)?;

// Each solution contains domains for all relevant facts
for solution in &solutions {
    for (fact_path, domain) in solution {
        println!("{}: {:?}", fact_path, domain);
    }
}
```

## Known Limitations

These are design constraints, not bugs:

1. **Single unknown per equation** - Multi-variable systems return implicit relationships
2. **Simple algebraic solving** - Handles `+`, `−`, `×`, `÷`, `^`, `exp`, `log` only
3. **No global optimization** - Returns sound union of guarded branches
4. **No SAT/SMT solving** - Guards preserved symbolically
5. **Opaque rule references** - Not expanded during solving (except simple constants)

## Enhancement Opportunities

### 1. Domain Normalization (2-3 days)

Domain operations produce correct but non-canonical results. Could add:
- Range merging: `[0, 50] ∪ [45, 100]` → `[0, 100]`
- Redundant elimination: `x > 5 AND x > 10` → `x > 10`
- Canonical ordering for deterministic output

Requires careful handling of bound types and type compatibility.

### 2. Enhanced Rule Reference Handling (3-4 days)

Rule references stay symbolic in results. Currently only expands if rule has no branches and hydrates to a literal constant.

Could expand simple constant rules automatically during hydration for cleaner results and better algebraic solving. Need to distinguish "safe to expand" vs "must stay symbolic" cases.

### 3. Better Error Messages (1 day)

Error messages are generic. Should provide context about why inversion failed and suggest valid alternatives.

### 4. Advanced Algebraic Solving (Future)

Currently handles single unknown with basic operators. Could add:
- Inversions when unknown is in exponent/base: `x^2 = 100`, `sqrt(x) = 10`
- More complex forms: `log(x) = target`, modulo in special cases

### 5. RelationGraph Architecture (1-2 weeks, optional)

Currently rules are processed on-demand. Could add eager caching:
- `RelationGraph` structure to cache extracted relations
- `Relation` struct with expression and branches
- `BidirectionalDeps` tracking fact and rule dependencies
- Eager extraction during document loading

Benefits: faster queries, dependency analysis. Current on-demand approach works fine for typical usage.

### 6. Smarter Boolean Simplification Guards (2 days)

BDD simplification has a fixed 64-atom limit to avoid pathological cases. Could use dynamic threshold based on expression structure or add timeout/step counter.

## Test Coverage Gaps

- Complex multi-branch scenarios with mixed value/veto branches
- Edge cases in domain intersection
- Rule reference expansion behavior
- Pathological cases (very deep nesting, huge unions)

## Documentation

API documentation needs expansion with examples and common usage patterns.

