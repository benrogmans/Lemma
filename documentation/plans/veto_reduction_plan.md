# Veto Reduction Plan

Add veto comparison handling to `bdd.rs::reduce()` so veto branches are properly eliminated when solving for value targets.

---


## Solver Considerations

Analysis of how the solver should handle Lemma's semantics correctly.

---

## Branch Normalization (Already Implemented)

The planning stage normalizes "last matching wins" semantics into **mutually exclusive branches** before the solver sees them.

### Example

```lemma
rule x = y
  unless y < 0 then veto "Must be positive"
  unless y > 20 then veto
  unless y >= 25 then 25
```

**Original branches:**
- Branch 0: (true, y)
- Branch 1: (y < 0, veto)
- Branch 2: (y > 20, veto)
- Branch 3: (y >= 25, 25)

**After normalization** (each branch excludes later branches):
- Branch 0: `0 ≤ y ≤ 20` → x = y
- Branch 1: `y < 0` → veto "Must be positive"
- Branch 2: `20 < y < 25` → veto
- Branch 3: `y ≥ 25` → x = 25

**Equation:**
```
(0 ≤ y ≤ 20 ∧ y) ∨ (y < 0 ∧ veto) ∨ (20 < y < 25 ∧ veto) ∨ (y ≥ 25 ∧ 25)
```

### Implications for Solver

1. Branches are **disjoint** - no priority logic needed during solving
2. Branches are **exhaustive** - every input maps to exactly one branch
3. OR of branches can be solved independently
4. `reduce()` can simplify within branches without affecting semantics

---

## Veto Semantics

Veto is NOT just "an unresolved value" - it's **constraint information** that tells us a domain is blocked.

### Veto in Lemma

From `veto_semantics.md`:
- Veto is for **data validation** - when input is invalid or out of range
- When a rule references a vetoed rule and needs its value, the veto propagates
- Unless clauses evaluated in reverse order can avoid veto if an earlier clause provides a value

### Veto in Constraints

**Type relationships:**
- `veto == numeric_value` → **false** (type mismatch)
- `veto == text_value` → **false** (type mismatch)
- `veto == boolean_value` → **false** (type mismatch)
- `veto == veto` → **true** (all vetos are equivalent for constraint purposes)

**Message handling:**
- For constraint simplification: **ignore messages** - all vetos represent "blocked outcome"
- For explicit veto targets: **match messages** when user searches for specific veto

### Veto as Target

Users can explicitly search for veto outcomes:

```
Target: veto "Must be positive"
Result: y < 0
```

This is legitimate inversion - find what inputs produce a specific validation error.

### Veto Branch Elimination

When target is a value (not veto), veto branches are eliminated:

For `x <= 25`:
- Branch 0: `0 ≤ y ≤ 20` → x = y → satisfies (y ≤ 20 ≤ 25) ✓
- Branch 1: `y < 0` → veto → **eliminated** (veto ≠ value)
- Branch 2: `20 < y < 25` → veto → **eliminated**
- Branch 3: `y ≥ 25` → x = 25 → satisfies (25 ≤ 25) ✓

**Solutions:** `{y ≥ 25 → x = 25}` ∪ `{0 ≤ y ≤ 20 → x = y}`

Or in constraint notation: `[25, {0..20}]`

---

## Implementation Requirements

### In `bdd.rs::reduce()`

Add veto comparison handling:

```rust
ExpressionKind::Comparison(left, op, right) => {
    // ... existing reduction ...
    
    // Veto comparisons
    if let ExpressionKind::Veto(_) = &left_reduced.kind {
        if let ExpressionKind::Veto(_) = &right_reduced.kind {
            // veto == veto → true (messages ignored)
            if op.is_equal() {
                return literal_true();
            }
        } else {
            // veto == value → false (type mismatch)
            if op.is_equal() {
                return literal_false();
            }
        }
    }
    // Symmetric case: value == veto
    if let ExpressionKind::Veto(_) = &right_reduced.kind {
        if op.is_equal() {
            return literal_false();
        }
    }
}
```

### In `solver.rs::apply_target()`

Already handles veto targets correctly:
- `any_veto` → checks if result is any Veto → true/false
- `veto("message")` → checks if result is Veto with matching message → true/false
- `value(x)` → creates comparison `result == x` (veto will reduce to false)

### In Multi-Solution Response

Solutions should indicate:
1. **Value solutions:** fact constraints that produce a specific value
2. **Veto solutions:** fact constraints that produce veto (when target is veto)
3. **Eliminated branches:** not included in results (veto when target is value)

---

## Example Walkthrough

```lemma
fact y = [number]
rule x = y
  unless y < 0 then veto "Must be positive"
  unless y > 20 then veto
  unless y >= 25 then 25
```

### Query: "What values of y make x <= 25?"

**Equation after normalization:**
```
(0 ≤ y ≤ 20 ∧ y) ∨ (y < 0 ∧ veto) ∨ (20 < y < 25 ∧ veto) ∨ (y ≥ 25 ∧ 25)
```

**After applying target `x <= 25`:**
```
(0 ≤ y ≤ 20 ∧ (y <= 25)) ∨ (y < 0 ∧ (veto <= 25)) ∨ (20 < y < 25 ∧ (veto <= 25)) ∨ (y ≥ 25 ∧ (25 <= 25))
```

**After reduction:**
- `y <= 25` is always true when `0 ≤ y ≤ 20`
- `veto <= 25` → **false** (type mismatch)
- `25 <= 25` → **true**

```
(0 ≤ y ≤ 20 ∧ true) ∨ (y < 0 ∧ false) ∨ (20 < y < 25 ∧ false) ∨ (y ≥ 25 ∧ true)
```

**Simplified:**
```
(0 ≤ y ≤ 20) ∨ (y ≥ 25)
```

**Solutions:**
1. `{y: [0, 20]}` → x = y (range 0 to 20)
2. `{y: [25, ∞)}` → x = 25

### Query: "What values of y make x = any veto?"

**After applying target `x == any_veto`:**

`match_result_to_target` directly checks if result IS a veto (no comparison created):
```
(0 ≤ y ≤ 20 ∧ false) ∨ (y < 0 ∧ true) ∨ (20 < y < 25 ∧ true) ∨ (y ≥ 25 ∧ false)
```
- Branch 0: result is `y` (not veto) → `false`
- Branch 1: result is `veto "Must be positive"` → `true`
- Branch 2: result is `veto` → `true`
- Branch 3: result is `25` (not veto) → `false`

**Simplified:**
```
(y < 0) ∨ (20 < y < 25)
```

**Solutions:**
1. `{y: (-∞, 0)}` → veto "Must be positive"
2. `{y: (20, 25)}` → veto

### Query: "What values of y make x = veto 'Must be positive'?"

**After applying target `x == veto("Must be positive")`:**

`match_result_to_target` checks if result is veto WITH matching message:
```
(0 ≤ y ≤ 20 ∧ false) ∨ (y < 0 ∧ true) ∨ (20 < y < 25 ∧ false) ∨ (y ≥ 25 ∧ false)
```
- Branch 0: result is `y` (not veto) → `false`
- Branch 1: result is `veto "Must be positive"` → message matches → `true`
- Branch 2: result is `veto` (no message) → doesn't match → `false`
- Branch 3: result is `25` (not veto) → `false`

**Simplified:**
```
(y < 0)
```

**Solution:**
1. `{y: (-∞, 0)}` → veto "Must be positive"

---

## Target Type Summary

| Target Type | How Handled | Needs reduce()? |
|-------------|-------------|-----------------|
| `value(x)` | Creates comparison `result == x` | Yes - veto == value → false |
| `any_veto` | Direct check: is result a Veto? | No |
| `veto("msg")` | Direct check: is result a Veto with matching message? | No |

---

## Summary

1. **Branch normalization** happens at planning time - solver sees disjoint branches
2. **Veto is constraint information** - not just an unresolved value
3. **`veto == value` → false** - type mismatch, handled in reduction
4. **`veto == veto` → true** - messages ignored for constraint purposes
5. **Veto as target is valid** - users can search for validation errors
6. **Veto branches are eliminated** when target is a value


---

## Problem

When target is a value (e.g., `x == 25`), branches that produce veto should be eliminated. Currently:

```
(y < 0 ∧ veto) with target x == 25
→ apply_target creates: (y < 0 ∧ (veto == 25))
→ reduce() does NOT simplify (veto == 25)
→ comparison stays as symbolic constraint
```

The comparison `veto == 25` should reduce to `false` (type mismatch).

---

## Solution

Add veto comparison handling in `bdd.rs::reduce()`.

### Type Relationships

| Comparison | Result | Reason |
|------------|--------|--------|
| `veto == value` | `false` | Type mismatch |
| `value == veto` | `false` | Type mismatch |
| `veto == veto` | `true` | All vetos equivalent for constraints |
| `veto > value` | `false` | Type mismatch |
| `veto < value` | `false` | Type mismatch |
| Any other veto comparison | `false` | Type mismatch |

---

## Implementation

### Location

`lemma/src/computation/bdd.rs` in the `reduce()` function, within `ExpressionKind::Comparison` handling.

### Insert After

The distribution logic (OR distribution into comparisons) but before the literal comparison evaluation.

### Code

```rust
ExpressionKind::Comparison(left, op, right) => {
    let left_reduced = reduce(*left);
    let right_reduced = reduce(*right);

    // ... existing OR distribution logic (lines 111-167) ...

    // Veto comparison handling
    let left_is_veto = matches!(left_reduced.kind, ExpressionKind::Veto(_));
    let right_is_veto = matches!(right_reduced.kind, ExpressionKind::Veto(_));

    if left_is_veto || right_is_veto {
        if left_is_veto && right_is_veto {
            // veto == veto → true (messages ignored)
            // veto != veto → false
            let result = op.is_equal();
            return Expression::new(
                ExpressionKind::Literal(LiteralValue::Boolean(if result {
                    BooleanValue::True
                } else {
                    BooleanValue::False
                })),
                None,
            );
        }
        // veto compared to non-veto → always false (type mismatch)
        return Expression::new(
            ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
            None,
        );
    }

    // ... existing literal comparison logic (lines 169-180) ...
    // ... existing range violation check (lines 182-188) ...
}
```

---

## Tests

Add tests in `lemma/src/computation/bdd.rs` test module.

### Test 1: veto == literal reduces to false

```rust
#[test]
fn test_veto_equals_literal_reduces_to_false() {
    let veto = Expression::new(
        ExpressionKind::Veto(Veto { message: Some("blocked".to_string()) }),
        None,
    );
    let literal = Expression::new(
        ExpressionKind::Literal(LiteralValue::Number(Decimal::from(25))),
        None,
    );
    let comparison = Expression::new(
        ExpressionKind::Comparison(
            Box::new(veto),
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            Box::new(literal),
        ),
        None,
    );
    
    let reduced = reduce(comparison);
    assert!(reduced.is_boolean_false(), "veto == 25 should reduce to false");
}
```

### Test 2: literal == veto reduces to false

```rust
#[test]
fn test_literal_equals_veto_reduces_to_false() {
    let literal = Expression::new(
        ExpressionKind::Literal(LiteralValue::Number(Decimal::from(25))),
        None,
    );
    let veto = Expression::new(
        ExpressionKind::Veto(Veto { message: None }),
        None,
    );
    let comparison = Expression::new(
        ExpressionKind::Comparison(
            Box::new(literal),
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            Box::new(veto),
        ),
        None,
    );
    
    let reduced = reduce(comparison);
    assert!(reduced.is_boolean_false(), "25 == veto should reduce to false");
}
```

### Test 3: veto == veto reduces to true

```rust
#[test]
fn test_veto_equals_veto_reduces_to_true() {
    let veto1 = Expression::new(
        ExpressionKind::Veto(Veto { message: Some("error A".to_string()) }),
        None,
    );
    let veto2 = Expression::new(
        ExpressionKind::Veto(Veto { message: Some("error B".to_string()) }),
        None,
    );
    let comparison = Expression::new(
        ExpressionKind::Comparison(
            Box::new(veto1),
            ComparisonComputation::Equal(EqualityNotation::Symbol),
            Box::new(veto2),
        ),
        None,
    );
    
    let reduced = reduce(comparison);
    assert!(reduced.is_boolean_true(), "veto == veto should reduce to true (messages ignored)");
}
```

### Test 4: veto inequality reduces to false

```rust
#[test]
fn test_veto_greater_than_literal_reduces_to_false() {
    let veto = Expression::new(
        ExpressionKind::Veto(Veto { message: None }),
        None,
    );
    let literal = Expression::new(
        ExpressionKind::Literal(LiteralValue::Number(Decimal::from(10))),
        None,
    );
    let comparison = Expression::new(
        ExpressionKind::Comparison(
            Box::new(veto),
            ComparisonComputation::GreaterThan,
            Box::new(literal),
        ),
        None,
    );
    
    let reduced = reduce(comparison);
    assert!(reduced.is_boolean_false(), "veto > 10 should reduce to false");
}
```

### Test 5: Integration - veto branch eliminated when solving for value

```rust
#[test]
fn test_veto_branch_eliminated_for_value_target() {
    // Build equation: (y >= 0 ∧ veto) ∨ (y < 0 ∧ 25)
    // Target: x == 25
    // Expected: veto branch reduces to false, only y < 0 branch remains
    
    let fact_y = FactPath::local("y".to_string());
    
    // Branch 1: y >= 0 ∧ veto
    let y_gte_0 = Expression::new(
        ExpressionKind::Comparison(
            Box::new(Expression::new(ExpressionKind::FactPath(fact_y.clone()), None)),
            ComparisonComputation::GreaterThanOrEqual,
            Box::new(Expression::new(ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)), None)),
        ),
        None,
    );
    let veto_branch = Expression::new(
        ExpressionKind::LogicalAnd(
            Box::new(y_gte_0),
            Box::new(Expression::new(ExpressionKind::Veto(Veto { message: None }), None)),
        ),
        None,
    );
    
    // Branch 2: y < 0 ∧ 25
    let y_lt_0 = Expression::new(
        ExpressionKind::Comparison(
            Box::new(Expression::new(ExpressionKind::FactPath(fact_y.clone()), None)),
            ComparisonComputation::LessThan,
            Box::new(Expression::new(ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)), None)),
        ),
        None,
    );
    let value_branch = Expression::new(
        ExpressionKind::LogicalAnd(
            Box::new(y_lt_0),
            Box::new(Expression::new(ExpressionKind::Literal(LiteralValue::Number(Decimal::from(25))), None)),
        ),
        None,
    );
    
    // Equation: branch1 ∨ branch2
    let equation = Expression::new(
        ExpressionKind::LogicalOr(Box::new(veto_branch), Box::new(value_branch)),
        None,
    );
    
    // Apply target and reduce
    let target = Target::value(LiteralValue::Number(Decimal::from(25)));
    let constrained = apply_target(&equation, &target);
    let reduced = reduce(constrained);
    
    // The veto branch should be eliminated (reduced to false)
    // Result should be: (y < 0 ∧ true) = y < 0
    let results = solve(equation, &target);
    assert_eq!(results.len(), 1, "should have exactly one solution (veto branch eliminated)");
}
```

### Test 6: any_veto target returns veto branches

```rust
#[test]
fn test_any_veto_target_returns_veto_branches() {
    // Equation: (y >= 0 ∧ veto "A") ∨ (y < 0 ∧ 25)
    // Target: any_veto
    // Expected: only y >= 0 branch remains (veto branch)
    // ...build equation as above, but swap results...
    
    let target = Target::any_veto();
    let results = solve(equation, &target);
    assert_eq!(results.len(), 1, "only veto branch should remain");
}
```

### Test 7: specific veto target matches only that veto

```rust
#[test]
fn test_specific_veto_target_matches_message() {
    // Equation: (y < 0 ∧ veto "A") ∨ (y >= 0 ∧ veto "B")
    // Target: veto("A")
    // Expected: only y < 0 branch remains (matching message)
    
    let target = Target::veto(Some("A".to_string()));
    let results = solve(equation, &target);
    assert_eq!(results.len(), 1, "only matching veto branch should remain");
}
```

---

## Verification

1. Run existing tests to ensure no regressions
2. Run new veto reduction tests
3. Run full test suite

```bash
cargo test --release -p lemma-engine
```

---

## Tasks

| Task | Location | Description |
|------|----------|-------------|
| Add veto comparison handling | `bdd.rs::reduce()` | Check for veto on either side of comparison |
| Test veto == value | `bdd.rs` tests | Should reduce to false |
| Test value == veto | `bdd.rs` tests | Should reduce to false |
| Test veto == veto | `bdd.rs` tests | Should reduce to true |
| Test veto inequality | `bdd.rs` tests | Should reduce to false |
| Test value target | `solver.rs` tests | Veto branch eliminated |
| Test any_veto target | `solver.rs` tests | Only veto branches remain |
| Test specific veto target | `solver.rs` tests | Only matching veto branch remains |

