# Solver Implementation Plan

## Overview

The solver determines what fact values satisfy a target constraint on a rule equation.

**Current state:** Basic constraint extraction (`fact op literal` → bounds), contradiction detection, domain restrictions.

**Goal:** Full algebraic solving with OR handling, distribution, and multi-solution support.

---

## Architecture

```
Planning (compile-time)          Inversion (query-time)
─────────────────────────        ────────────────────────
                                 
build_equation()                 apply_target()
    │                                │
    ▼                                ▼
reduce()  ──────────────────►   reduce()
    │                                │
    ▼                                ▼
ExecutableRule.equation          solve()
                                     │
                                     ▼
                                 SolveResult
```

The equation is pre-built and reduced at planning time. At query time:
1. Apply target constraint
2. Reduce again (now with target values)
3. Solve the reduced expression

---

## Phase 1: OR Handling (Multiple Solutions)

### Goal
Split OR branches into separate solutions instead of punting to "symbolic".

### Current Behavior
```rust
ExpressionKind::LogicalOr(left, right) => {
    constraint_set.add_symbolic(expression.clone());  // punt
}
```

### New Behavior
```rust
fn solve_expression(expr: Expression) -> Vec<SolveResult> {
    let branches = flatten_or(expr);
    branches.iter().map(|b| solve_branch(b)).collect()
}
```

### Requirements
- Top-level OR produces multiple solutions
- Each OR branch solved independently
- Unsatisfiable branches filtered out
- All branches unsat → `Unsatisfiable`

### Expected Behaviors
- `false ∨ (x > 10)` → single solution: `x > 10`
- `false ∨ false` → unsatisfiable
- `true ∨ (x > 10)` → single solution: unconstrained (true absorbs)
- `(a ∨ b) ∨ c` → three branches flattened
- `x > 0 ∧ (y = 1 ∨ y = 2)` → two solutions: `{x > 0, y = 1}`, `{x > 0, y = 2}`

---

## Phase 2: Distribution

### Goal
Push OR out of arithmetic and comparisons so it becomes top-level.

### Distribution Rules

```
(A ∨ B) op C    →    (A op C) ∨ (B op C)
C op (A ∨ B)    →    (C op A) ∨ (C op B)
(A ∨ B) * C    →    (A * C) ∨ (B * C)
(A ∨ B) + C    →    (A + C) ∨ (B + C)
```

### Where
In `bdd.rs::reduce()`, when encountering Comparison or Arithmetic with OR operand.

### Requirements
- OR in left operand of comparison distributes
- OR in right operand of comparison distributes
- OR in left operand of arithmetic distributes
- OR in right operand of arithmetic distributes
- Distribution applies recursively until no OR inside arith/comparison

### Expected Behaviors
- `((c₀ ∧ 10) ∨ (c₁ ∧ 20)) == 15` → `false` (both branches fail)
- `((c₀ ∧ 10) ∨ (c₁ ∧ 20)) * x == 100` → two branches with different x constraints
- `x + ((a ∧ 5) ∨ (b ∧ 10)) == 20` → two branches for different conditions
- `((A ∨ B) + (C ∨ D)) == 10` → four branches after full distribution
- `((a ∧ 3) ∨ (b ∧ 5)) * 5 == 25` → simplifies to `b` (only second branch satisfies)

---

## Phase 2.5: Boolean Simplification

### Goal
Eliminate redundant and contradictory branches after DNF conversion.

### Where
In `bdd.rs`, after distribution produces flat DNF structure.

### Simplification Rules

```
A ∧ A           →  A                    (idempotence)
A ∨ A           →  A                    (idempotence)
A ∨ (A ∧ B)     →  A                    (absorption)
(A ∧ B) ∨ (¬A ∧ C) ∨ (B ∧ C)  →  (A ∧ B) ∨ (¬A ∧ C)   (consensus)
```

### Fact Contradiction Detection

After DNF, each branch is a flat AND. Check for same-fact conflicts:

```
(fact == X ∧ fact == Y)  →  false       (when X ≠ Y)
```

This requires flat branches, so it runs AFTER distribution/DNF conversion.

### Requirements
- Idempotence elimination
- Absorption of subsumed branches
- Consensus elimination of redundant branches
- Fact equality contradiction → false
- `false ∨ X` → `X` cleans up contradictory branches

### Expected Behaviors
- `(drink == "latte" ∧ drink == "latte")` → `drink == "latte"`
- `(drink == "latte" ∧ drink == "espresso")` → `false`
- `(A ∧ B) ∨ (¬A ∧ C) ∨ (B ∧ C)` → `(A ∧ B) ∨ (¬A ∧ C)`
- `A ∨ (A ∧ B)` → `A`

---

## Phase 3: Algebraic Solving

### Goal
Isolate unknowns in equations to extract bounds.

### Supported Transformations

```
x + c == v    →    x == v - c
x - c == v    →    x == v + c
c - x == v    →    x == c - v
x * c == v    →    x == v / c  (c ≠ 0)
x / c == v    →    x == v * c
c / x == v    →    x == c / v  (v ≠ 0)
```

### Requirements
- Single unknown with addition: solve for unknown
- Single unknown with subtraction: solve for unknown
- Single unknown with multiplication: solve for unknown
- Single unknown with division: solve for unknown
- Division by zero produces domain restriction, not solution
- Multiple unknowns: simplify constants, return reduced constraint (e.g., `x + y + 10 == 100` → `x + y == 90`)
- Nested arithmetic: flatten and simplify

### Expected Behaviors
- `x * 0 == 5` → unsatisfiable (0 ≠ 5)
- `x * 0 == 0` → unconstrained (any x works)
- `5 / x == 0` → unsatisfiable (5/x is never 0)
- `0 / x == 0` → domain restriction x ≠ 0
- `x + y + 10 == 100` → constraint `x + y == 90`
- `(x + 5) * 2 == 30` → `x == 10`
- `x * x == 16` → symbolic (non-linear) or `x ∈ {-4, 4}`
- `sqrt(x) == 4` → `x == 16` with domain restriction `x >= 0`

---

## Phase 4: Inequality Solving

### Goal
Handle inequality constraints with algebraic manipulation.

### Transformations

```
x + c < v     →    x < v - c
x * c < v     →    x < v / c      (c > 0)
x * c < v     →    x > v / c      (c < 0, flip!)
```

### Requirements
- Addition/subtraction: shift constant, preserve direction
- Multiplication by positive: divide, preserve direction
- Multiplication by negative: divide, flip direction
- Division: multiply, handle sign
- Strict vs non-strict preserved correctly

### Expected Behaviors
- `x * (-2) > 10` → `x < -5` (flipped)
- `x * (-2) >= 10` → `x <= -5` (flipped, strict preserved)
- `-x > 5` → `x < -5`
- `x * 0 > 5` → unsatisfiable
- `x * 0 >= 0` → unconstrained

---

## Phase 5: Mathematical Functions

### Goal
Invert mathematical functions where possible.

### Invertible Functions

```
sqrt(x) == v    →    x == v²     (v >= 0)
exp(x) == v     →    x == ln(v)  (v > 0)
log(x) == v     →    x == exp(v)
sin(x) == v     →    x ∈ { arcsin(v) + 2πn, π - arcsin(v) + 2πn }  (|v| <= 1)
abs(x) == v     →    x ∈ {v, -v}  (v >= 0)
```

### Requirements
- sqrt inverse with domain check
- exp inverse with positive check
- log inverse
- abs produces two solutions
- Trig functions: symbolic or periodic solutions

### Expected Behaviors
- `sqrt(x) == -3` → unsatisfiable (sqrt always >= 0)
- `exp(x) == 0` → unsatisfiable (exp always > 0)
- `exp(x) == -5` → unsatisfiable
- `log(x) == 0` → `x == 1`
- `abs(x) == 5` → two solutions: `x = 5` or `x = -5`
- `abs(x) == -5` → unsatisfiable
- `sin(x) == 2` → unsatisfiable (already handled by range check)

---

## Phase 6: Multi-Solution Response

### Goal
Return all valid solutions with their outcomes.

### Response Structure

```rust
pub struct InversionResponse {
    pub solutions: Vec<Solution>,
}

pub struct Solution {
    pub outcome: OperationResult,
    pub fact_constraints: HashMap<FactPath, FactConstraint>,
    pub domain_restrictions: Vec<DomainRestriction>,
}
```

### Requirements
- Each OR branch produces separate solution
- Duplicate solutions deduplicated
- Solutions sorted by specificity (more constrained first)
- Veto outcomes included as solutions
- `any_value` target returns all outcomes

### Expected Behaviors
- Same constraints from different branches → deduplicated to one solution
- Overlapping ranges → keep as separate solutions (user can merge)
- All branches veto → all solutions have Veto outcome
- Mix of value and veto → both types in solutions list

---

## Implementation Order

### Milestone 1: Multiple Solutions
1. Change `SolveResult` to support multiple solutions
2. Implement `flatten_or` to collect OR branches
3. Solve branches independently
4. Filter unsatisfiable branches
5. Update `InversionResponse` structure

### Milestone 2: Distribution
6. Add distribution to `bdd.rs::reduce()` for Comparison
7. Add distribution for Arithmetic
8. Ensure recursive application
9. Test nested OR distribution

### Milestone 2.5: Boolean Simplification
10. Implement idempotence (`A ∧ A` → `A`)
11. Implement absorption (`A ∨ (A ∧ B)` → `A`)
12. Implement consensus elimination
13. Implement fact contradiction detection (`fact == X ∧ fact == Y` → `false`)
14. Clean up false branches (`false ∨ X` → `X`)

### Milestone 3: Basic Algebra
15. Implement single-unknown linear solving
16. Handle addition/subtraction
17. Handle multiplication/division
18. Handle division-by-zero cases

### Milestone 4: Inequality Algebra
19. Extend algebra to inequalities
20. Handle sign flipping for negative multipliers
21. Preserve strict vs non-strict

### Milestone 5: Math Functions
22. Implement sqrt/exp/log inversion
23. Implement abs (two solutions)
24. Keep trig as symbolic

### Milestone 6: Polish
25. Solution sorting by specificity
26. Documentation
27. Integration tests

---

## Test Requirements

Tests live in:
- `computation/bdd.rs` - distribution and boolean simplification tests
- `tests/bdd_consensus.rs` - boolean simplification integration tests
- `inversion/solver.rs` - solving tests
- `inversion/mod.rs` - integration tests

---

## Future Phases (Lower Priority)

The following are goals but deferred to later milestones:

### Phase 7: Non-Linear Equations
- `x² = 16` → `x ∈ {-4, 4}`
- `x * y = 100` with one known → solve for other
- Polynomial factoring for simple cases

### Phase 8: Symbolic Simplification
- `x + x` → `2x`
- `x * 1` → `x`
- `x - x` → `0`
- Collect like terms before solving

### Phase 9: Constraint Propagation
- If solution A implies `x > 10` and solution B implies `x < 5`, they're disjoint
- Use bounds from one constraint to simplify others
- Detect implied constraints

### Phase 10: Optimization
- Find minimum/maximum x satisfying constraints
- Return bounds as `[min, max]` when possible
- Support "find tightest constraint" queries

---

## Non-Goals (Current Scope)

- General symbolic algebra (CAS-level manipulation like Mathematica)
- Transcendental equation solving (e.g., `x = sin(x)`)
- Full interval arithmetic with precision tracking

## Note on SMT

What we're building IS an SMT solver specialized for:
- **Theory**: Linear arithmetic over decimals + booleans
- **Features**: Bound extraction, contradiction detection, solution enumeration

We're not integrating an external solver (Z3, CVC5).
