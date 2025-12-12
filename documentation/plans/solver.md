# Solver Module Implementation Plan

## Overview

Replace the syntactic `simplify.rs` with a semantic constraint solver `solver.rs`. The solver performs exact symbolic reasoning about constraint satisfiability, extracts precise domains where algebraically possible, preserves constraints symbolically where not reducible, and never approximates.

---

## Problem Statement

Current `simplify.rs` only handles:
- Boolean identities (`X ∧ true → X`)
- Literal evaluation (`5 == 5 → true`)

This is insufficient. Lemma expressions can contain:
- **Multi-fact arithmetic**: `a * b`, `price * quantity`
- **Cross-fact comparisons**: `a > b`
- **Transcendental functions**: `sin(a)`, `cos(a * b)`, `tan(x)`
- **Nested rule references**: `x? / tan(a) + 10 > b * 2`
- **Non-linear relationships**: `b * cos(a)`

Example:
```lemma
fact a = [number]
fact b = [number]

rule x = b * cos(a)
  unless b > 5 then cos(a + 2)

rule y = x? / tan(a) + 10 > b * 2
  unless sin(a * b) > 12 then 10
```

After expanding rule references, branch conditions contain multi-variable transcendental expressions. The solver must handle this complexity while preserving precision.

### Requirements

1. **Preserve precision** — no numeric approximation
2. **Extract exact domains** where algebraically possible
3. **Preserve symbolic constraints** where not reducible
4. **Detect contradictions** using function properties (range, domain)
5. **Track domain restrictions** where expressions are undefined

---

## Existing Code Analysis

### equation.rs (unchanged)

Builds symbolic equations from rules. Outputs `Expression` trees:

| Function | Purpose |
|----------|---------|
| `build_equations` | Rules → Expression (topo order, caches) |
| `build_rule_equation` | `(cond ∧ result) ∨ ...` construction |
| `combine_with_or` | Join expressions with OR |
| `substitute_rule_references` | Inline `rule?` references |

### ExecutionPlan Structure

From `execution_plan.rs`:
- `rules` are topologically sorted (dependencies first)
- Branches are **mutually exclusive** (graph builder adds negations)
- Each branch: `condition AND result`
- Equation is OR of mutually exclusive branches

### simplify.rs (to be replaced)

| Function | Reuse in solver.rs? |
|----------|---------------------|
| `extract_or_branches` | ✓ Move to solver.rs |
| `extract_and_parts` | ✓ Move to solver.rs |
| `apply_constraint_to_result` | ✓ Move to solver.rs |
| `extract_outcome` | ✓ Rework |
| `simplify` | ✗ Replace entirely |
| `solve_equation` | ✗ Replace entirely |

---

## Architecture

```
inversion/
├── mod.rs          # Entry point, orchestration
├── equation.rs     # Equation building from rules (unchanged)
├── solver.rs       # Constraint solving (replaces simplify.rs)
├── functions.rs    # Function properties (range, domain, inverses)
├── extract.rs      # Domain types (simplified)
└── response.rs     # Response types (unchanged)
```

---

## Core Data Structures

### SolveResult

```rust
pub enum SolveResult {
    /// Fully solved to concrete domains
    Solved {
        fact_constraints: HashMap<FactPath, FactConstraint>,
    },
    
    /// Partially solved — some constraints remain symbolic
    Partial {
        fact_constraints: HashMap<FactPath, FactConstraint>,
        remaining_constraints: Vec<Expression>,
        domain_restrictions: Vec<DomainRestriction>,
    },
    
    /// Contradiction detected — no valid solution
    Unsatisfiable {
        reason: UnsatReason,
    },
}
```

### DomainRestriction

```rust
pub struct DomainRestriction {
    /// The fact(s) involved
    pub facts: Vec<FactPath>,
    
    /// The restriction expression (e.g., "a ≠ π/2 + nπ")
    pub restriction: Expression,
    
    /// Human-readable source (e.g., "tan undefined")
    pub source: String,
}
```

### UnsatReason

```rust
pub enum UnsatReason {
    /// min > max for a fact
    BoundsContradiction {
        fact: FactPath,
        min: Bound,
        max: Bound,
    },
    
    /// Value outside function's codomain (e.g., sin(x) > 2)
    FunctionRangeViolation {
        function: String,
        required_value: LiteralValue,
        valid_range: (Option<LiteralValue>, Option<LiteralValue>),
    },
    
    /// Conflicting exact values (e.g., x == "a" AND x == "b")
    EnumContradiction {
        fact: FactPath,
        values: Vec<LiteralValue>,
    },
    
    /// Exact value in excluded set (e.g., x == 5 AND x != 5)
    ExclusionContradiction {
        fact: FactPath,
        value: LiteralValue,
    },
}
```

### FactBounds

```rust
pub struct FactBounds {
    /// Lower bound: (value, is_inclusive)
    pub min: Option<(LiteralValue, bool)>,
    
    /// Upper bound: (value, is_inclusive)
    pub max: Option<(LiteralValue, bool)>,
    
    /// Exact value from equality constraint
    pub exact: Option<LiteralValue>,
    
    /// Excluded values from != constraints
    pub excluded: Vec<LiteralValue>,
}
```

### ConstraintSet

```rust
pub struct ConstraintSet {
    /// Bounds accumulated per fact
    pub facts: HashMap<FactPath, FactBounds>,
    
    /// Relational constraints between facts (for transitivity)
    pub relations: Vec<(FactPath, ComparisonOp, FactPath)>,
    
    /// Constraints that couldn't be reduced to single-fact bounds
    pub symbolic: Vec<Expression>,
    
    /// Domain restrictions from function domains
    pub restrictions: Vec<DomainRestriction>,
    
    /// Has a contradiction been detected?
    pub contradiction: Option<UnsatReason>,
}
```

---

## Function Properties

The solver must know exact properties of mathematical functions.

### Range Constraints (Codomain)

| Function | Range | Detection |
|----------|-------|-----------|
| `sin(x)` | [-1, 1] | `sin(x) > 2` → UNSAT |
| `cos(x)` | [-1, 1] | `cos(x) == 5` → UNSAT |
| `exp(x)` | (0, +∞) | `exp(x) < 0` → UNSAT |
| `sqrt(x)` | [0, +∞) | `sqrt(x) < 0` → UNSAT |
| `abs(x)` | [0, +∞) | `abs(x) < 0` → UNSAT |
| `log(x)` | (-∞, +∞) | No range restriction |

### Domain Restrictions

| Function | Domain | Restriction Added |
|----------|--------|-------------------|
| `sqrt(x)` | x >= 0 | `x >= 0` |
| `log(x)` | x > 0 | `x > 0` |
| `tan(x)` | x ≠ π/2 + nπ | `x ≠ π/2 + nπ` |
| `1/x` | x ≠ 0 | `x ≠ 0` |
| `asin(x)` | x ∈ [-1, 1] | `-1 <= x <= 1` |
| `acos(x)` | x ∈ [-1, 1] | `-1 <= x <= 1` |

### Exact Inverses (Bijective Functions)

| Expression | Solution | Condition |
|------------|----------|-----------|
| `exp(x) == k` | `x == ln(k)` | k > 0 |
| `log(x) == k` | `x == e^k` | always |
| `sqrt(x) == k` | `x == k²` | k >= 0 |
| `abs(x) == k` | `x == k OR x == -k` | k >= 0 |
| `x² == k` | `x == √k OR x == -√k` | k >= 0 |

### Periodic Functions (Non-Bijective)

For periodic functions, preserve symbolic constraint with exact solution set:

| Expression | Solution Set |
|------------|-------------|
| `sin(x) == k` | `x ∈ {arcsin(k) + 2πn, π - arcsin(k) + 2πn}` for k ∈ [-1,1] |
| `cos(x) == k` | `x ∈ {±arccos(k) + 2πn}` for k ∈ [-1,1] |
| `tan(x) == k` | `x ∈ {arctan(k) + πn}` for all k |

---

## Algorithm

### Main Flow

```
solve_branches(equation, target, known_facts) -> Vec<(SolveResult, OperationResult)>
    │
    ├── 1. Extract OR branches from equation          [REUSE: extract_or_branches]
    │      (Branches are mutually exclusive from graph builder)
    │
    └── For each branch:
        │
        ├── 2. Extract (condition, result)            [REUSE: extract_and_parts]
        │
        ├── 3. Apply target constraint                [REUSE: apply_target_constraint]
        │      result == target_value
        │
        ├── 4. Extract outcome                        [REWORK]
        │
        ├── 5. Substitute known facts                 [NEW]
        │      Replace fact references with provided values
        │
        ├── 6. Simplify algebraically (exact only)    [NEW]
        │      Constant folding, identity laws, boolean laws
        │
        ├── 7. Analyze function constraints           [NEW]
        │      - Check codomain bounds
        │      - Propagate domain requirements
        │      - Apply exact inverses
        │
        ├── 8. Partition constraints                  [NEW]
        │      - Single-fact linear → extract bounds
        │      - Single-fact invertible → apply inverse
        │      - Multi-fact / periodic → keep symbolic
        │
        ├── 9. Extract single-fact domains            [NEW]
        │      - Accumulate bounds per fact
        │      - Intersect ranges
        │      - Detect contradictions
        │
        ├── 10. Advanced reasoning                    [NEW]
        │       - Transitivity closure
        │       - Unit propagation
        │
        └── 11. Return SolveResult
```

### Solving Strategy

```rust
fn solve_branch(
    condition: Expression,
    known_facts: &HashMap<FactPath, LiteralValue>,
) -> SolveResult {
    let mut set = ConstraintSet::new();
    
    // 1. Substitute known facts
    let condition = substitute_known(condition, known_facts);
    
    // 2. Simplify algebraically (exact only)
    let condition = simplify_exact(condition);
    
    // 3. Check for trivial true/false
    if condition.is_boolean_true() {
        return SolveResult::Solved { fact_constraints: HashMap::new() };
    }
    if condition.is_boolean_false() {
        return SolveResult::Unsatisfiable { 
            reason: UnsatReason::BoundsContradiction { ... } 
        };
    }
    
    // 4. Extract and analyze constraints
    extract_constraints(&condition, &mut set);
    
    // 5. Check for contradictions
    if let Some(reason) = set.contradiction {
        return SolveResult::Unsatisfiable { reason };
    }
    
    // 6. Convert to result
    let fact_constraints = set.to_fact_constraints();
    
    if set.symbolic.is_empty() && set.restrictions.is_empty() {
        SolveResult::Solved { fact_constraints }
    } else {
        SolveResult::Partial {
            fact_constraints,
            remaining_constraints: set.symbolic,
            domain_restrictions: set.restrictions,
        }
    }
}
```

### Key Principle

**Never approximate. Never lose information.**

- If we can solve exactly → solve
- If we can't → preserve the exact constraint
- If contradictory → report with reason

---

## Implementation Phases

### Phase 1: Foundation + Function Knowledge

**Goals:**
- Create solver.rs with data structures
- Move reusable utilities from simplify.rs
- Implement function property tables
- Range violation detection

**Move from simplify.rs:**
```rust
fn extract_or_branches(expr: &Expression) -> Vec<Expression>;
fn extract_and_parts(expr: &Expression) -> (Expression, Expression);
fn apply_target_constraint(result: &Expression, target: &Target) -> Expression;
```

**New:**
```rust
// Function properties
fn function_range(op: &MathematicalComputation) -> Option<(Bound, Bound)>;
fn function_domain_restriction(op: &MathematicalComputation, arg: &Expression) -> Option<DomainRestriction>;
fn is_in_range(value: &LiteralValue, min: &Bound, max: &Bound) -> bool;

// Range violation
fn check_range_violation(expr: &Expression) -> Option<UnsatReason>;
```

**Tests:**
```rust
#[test] fn sin_greater_than_2_is_unsat();
#[test] fn cos_equals_5_is_unsat();
#[test] fn exp_less_than_0_is_unsat();
#[test] fn sqrt_requires_non_negative_domain();
#[test] fn tan_has_undefined_points();
```

### Phase 2: Single-Fact Solving

**Goals:**
- Extract bounds from single-fact comparisons
- Accumulate and intersect bounds
- Detect contradictions
- Apply exact inverses for bijective functions

**Functions:**
```rust
fn add_comparison(set: &mut ConstraintSet, fact: FactPath, op: ComparisonOp, value: LiteralValue);
fn intersect_bounds(existing: &mut FactBounds, new_bound: Bound, is_min: bool) -> bool;
fn check_bounds_contradiction(bounds: &FactBounds) -> Option<UnsatReason>;

// Exact inverses
fn solve_exp_equals(k: &LiteralValue) -> Option<LiteralValue>;  // x = ln(k)
fn solve_log_equals(k: &LiteralValue) -> Option<LiteralValue>;  // x = e^k
fn solve_sqrt_equals(k: &LiteralValue) -> Option<LiteralValue>; // x = k²
```

**Tests:**
```rust
#[test] fn single_lower_bound();
#[test] fn single_upper_bound();
#[test] fn bound_intersection();
#[test] fn detects_min_greater_than_max();
#[test] fn exp_x_equals_10_gives_ln_10();
#[test] fn sqrt_x_equals_5_gives_25();
#[test] fn log_x_equals_2_gives_e_squared();
```

### Phase 3: Expression Simplification

**Goals:**
- Substitute known facts into expressions
- Algebraic simplification (exact only)
- Boolean simplification
- Constant folding

**Functions:**
```rust
fn substitute_known(expr: Expression, known: &HashMap<FactPath, LiteralValue>) -> Expression;
fn simplify_exact(expr: Expression) -> Expression;
fn fold_constants(expr: Expression) -> Expression;
fn simplify_boolean(expr: Expression) -> Expression;
```

**Tests:**
```rust
#[test] fn substitutes_known_fact();
#[test] fn folds_constant_arithmetic();
#[test] fn simplifies_boolean_identities();
#[test] fn does_not_approximate_transcendentals();
```

### Phase 4: Symbolic Constraint Handling

**Goals:**
- Partition constraints into solvable vs symbolic
- Preserve multi-fact constraints exactly
- Generate solution set notation for periodic functions
- Collect domain restrictions

**Functions:**
```rust
fn partition_constraint(expr: &Expression) -> ConstraintClass;
fn preserve_symbolic(set: &mut ConstraintSet, expr: Expression);
fn periodic_solution_set(func: &str, value: &LiteralValue) -> Expression;
fn collect_domain_restrictions(expr: &Expression) -> Vec<DomainRestriction>;
```

**Tests:**
```rust
#[test] fn multi_fact_preserved_symbolically();
#[test] fn cos_a_equals_half_gives_solution_set();
#[test] fn division_adds_nonzero_restriction();
#[test] fn tan_adds_undefined_points_restriction();
```

### Phase 5: Advanced Reasoning

**Goals:**
- Transitivity for cross-fact comparisons
- Unit propagation for disjunctions
- Negation lowering

**Functions:**
```rust
fn add_relation(set: &mut ConstraintSet, left: FactPath, op: ComparisonOp, right: FactPath);
fn transitivity_closure(set: &mut ConstraintSet);
fn check_cycle_contradiction(set: &ConstraintSet) -> Option<UnsatReason>;
fn lower_negation(expr: Expression) -> Expression;
fn unit_propagation(set: &mut ConstraintSet, disjunctions: &mut Vec<Vec<Expression>>);
```

**Tests:**
```rust
#[test] fn transitivity_gt_gt();
#[test] fn transitivity_detects_cycle();
#[test] fn unit_propagation_single();
#[test] fn lowers_not_gte_to_lt();
```

---

## Integration

### Changes to mod.rs

```rust
// Before:
mod simplify;
use simplify::apply_target_and_simplify;

// After:
mod solver;
mod functions;
use solver::solve_branches;

// In invert_with_target():
let branch_solutions = solve_branches(target_equation.clone(), &target, &known_facts);

for (solve_result, branch_outcome) in branch_solutions {
    match solve_result {
        SolveResult::Solved { fact_constraints } => {
            solutions.push(Solution::new(branch_outcome, fact_constraints));
        }
        SolveResult::Partial { fact_constraints, remaining_constraints, domain_restrictions } => {
            solutions.push(Solution::partial(
                branch_outcome, 
                fact_constraints, 
                remaining_constraints,
                domain_restrictions,
            ));
        }
        SolveResult::Unsatisfiable { .. } => {
            // Skip unsatisfiable branches
        }
    }
}
```

### File Changes Summary

| File | Action |
|------|--------|
| `solver.rs` | Create (new) |
| `functions.rs` | Create (new) — function properties |
| `simplify.rs` | Delete (replaced) |
| `mod.rs` | Update imports and call site |
| `extract.rs` | Simplify (keep types, remove extraction logic) |
| `response.rs` | Update Solution for partial results |
| `equation.rs` | No changes |

---

## Implementation Order

### Step 1: Create solver.rs skeleton
- Data structures (FactBounds, ConstraintSet, SolveResult, UnsatReason)
- Move utilities from simplify.rs
- Stub solve_branches

### Step 2: Create functions.rs
- Function range table
- Function domain restriction table
- Exact inverse implementations
- Range violation checking

### Step 3: Wire into mod.rs
- Replace simplify import with solver
- Run existing tests

### Step 4: Phase 1 — Range analysis
- Implement range violation detection
- Test with sin/cos/exp constraints

### Step 5: Phase 2 — Single-fact solving
- Bound extraction and intersection
- Contradiction detection
- Exact inverses

### Step 6: Phase 3 — Simplification
- Known fact substitution
- Algebraic simplification (exact)

### Step 7: Phase 4 — Symbolic handling
- Constraint partitioning
- Symbolic preservation
- Domain restriction collection

### Step 8: Phase 5 — Advanced reasoning
- Transitivity
- Unit propagation

### Step 9: Cleanup
- Delete simplify.rs
- Update response.rs for partial solutions
- Final test pass

---

## Success Criteria

### Correctness (Non-Negotiable)

- Never returns approximate values
- Never loses constraint information
- Detects all function range violations
- Reports all domain restrictions
- Preserves multi-fact constraints exactly

### Capability

| Input | Expected Output |
|-------|-----------------|
| `sin(x) > 2` | UNSAT (range violation) |
| `cos(a) == 0.5` | Symbolic: `a ∈ {±arccos(0.5) + 2πn}` |
| `exp(x) == 10` | Exact: `x == ln(10)` |
| `sqrt(y) == 5` | Exact: `y == 25` |
| `log(z) == 2` | Exact: `z == e²` |
| `a * b == 50` | Symbolic: preserved as constraint |
| `1 / tan(a)` | Domain restriction: `a ≠ π/2 + nπ` |
| `a >= 10 AND a < 5` | UNSAT (bounds contradiction) |
| `x == "a" AND x == "b"` | UNSAT (enum contradiction) |

### Tests

All existing inversion tests pass, plus:

- Function range violation tests
- Exact inverse tests
- Symbolic preservation tests
- Domain restriction tests
- Multi-fact constraint tests
- Contradiction detection tests

---

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Complex expression simplification | Conservative: only simplify what we're certain about |
| Periodic function handling | Explicit solution set notation, not approximation |
| Performance on large expressions | Profile and optimize, but correctness first |
| Missing function properties | Extensible table, add as needed |

---

## Non-Goals

- **Numeric approximation** — Never
- **Incomplete solutions** — Always report what we know and what remains
- **Heuristic solving** — Exact or symbolic, nothing in between
