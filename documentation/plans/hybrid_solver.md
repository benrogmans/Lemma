# World-Based Symbolic Execution Architecture

## Current Status

**Phase 1: COMPLETE** ✅
- All deletions completed
- Useful code moved to new locations
- Code intentionally broken (as expected)

**Phase 2: COMPLETE** ✅
- Algebra module created
- All files moved from computation/ to algebra/
- All imports updated
- No backward compatibility re-exports
- Clean module separation established

**Phase 3: COMPLETE** ✅
- ExecutionPlan::optimize() method added
- Branches have optimized_condition field
- Optimization moved from planning to post-symbolic-evaluation (called after Phase 4)

**Phase 4: COMPLETE** ✅
- EvaluationResult enum added (Evaluated/Symbolic)
- Symbolic evaluation mode in Evaluator
- Partial evaluation with unknown facts
- Branch pruning (false conditions and last-wins optimization)
- Returns reduced ExecutionPlan

**Phase 5: COMPLETE** ✅
- FactConstraint helper methods added (is_exact, exact, contains)
- World struct created with merge() method
- Constraint intersection-based pruning

**Phase 6: COMPLETE** ✅
- WorldBuilder struct created
- build_worlds() implemented with caching
- collect_rule_paths() and substitute_rule_path() helpers
- Recursive world building with cross-product merging

**Phase 7: COMPLETE** ✅
- InversionResult and InversionSolution structs added
- invert_expression() implemented for recursive inversion
- Arithmetic inversion helpers (left/right isolation)
- Mathematical inversion helpers (sqrt, pow, trig - some require numerical impl)
- evaluate_to_literal() helper for constant evaluation
- EvaluationContext::new_for_inversion() public method added

**Next: Phase 8** - Wire up new inversion flow

---

## Executive Summary

**Goal:** Replace equation-based inversion with world-based approach for linear complexity.

**Key Changes:**
1. **Delete** `planning/equation.rs` (recursive substitution) - **Phase 1** ✅
2. **Delete** `inversion/solver.rs` (after moving useful parts) - **Phase 1** ✅
3. **Move** isolation functions to `algebra/isolation.rs` - **Phase 1** ✅
4. **Move** extract_constraints to `computation/constraints.rs` - **Phase 1** ✅
5. **Create** new `algebra/` module - **Phase 2** ✅
6. **Move** `computation/expansion.rs` → `algebra/expansion.rs` - **Phase 2** ✅
7. **Move** `computation/simplification.rs` → `algebra/simplification.rs` - **Phase 2** ✅
8. **Move** `computation/constraints.rs` → `algebra/constraints.rs` - **Phase 2** ✅
9. **Add** ExecutionPlan::optimize() method for DNF/simplification - **Phase 3** ✅
10. **Add** symbolic evaluation (critical optimization) - **Phase 4** ✅
11. **Add** World structure with full Expression support - **Phase 5** ✅
12. **Add** WorldBuilder with symbolic eval integration - **Phase 6** ✅
13. **Enhance** algebraic isolation for non-linear math - **Phase 7** ✅
14. **Wire up** new inversion flow - **Phase 8**

**Result:** Clean separation between `computation/` (runtime) and `algebra/` (reasoning), with path-based inversion + symbolic evaluation avoiding exponential complexity.

**Critical Innovation (from guide):** 
**Symbolic Evaluation** (Phase 4) is the game-changer. Before building any paths, substitute known facts and prune dead branches. This transforms N-dimensional problems (income × state × status) into 1D problems (just income), reducing search space by orders of magnitude.

**Example:** Query "income for tax=X" with `state="CA"`, `status="single"` known:
- **Without symbolic eval**: Build 50 states × 4 statuses = 200 paths, then filter
- **With symbolic eval**: Prune to 1 relevant path immediately

**Phase Overview:**
- **Phase 1**: DELETE old approach (equation.rs, solver.rs) ✅ **COMPLETE**
- **Phase 2**: CREATE algebra module, MOVE files from computation/ ✅ **COMPLETE**
- **Phase 3**: ADD ExecutionPlan::optimize() method for DNF/simplification ✅ **COMPLETE**
- **Phase 4**: ADD symbolic evaluation (substitute knowns, prune branches) ✅ **COMPLETE**
- **Phase 5**: ADD world structure (Expression-based values) ✅ **COMPLETE**
- **Phase 6**: ADD world builder (works with reduced plan from Phase 4) ✅ **COMPLETE**
- **Phase 7**: ENHANCE algebraic isolation (non-linear inversion) ✅ **COMPLETE**
- **Phase 8**: WIRE UP new inversion flow

---

## Problem Statement

Current equation-based approach has exponential complexity:
- Substitutes all rule references recursively before solving
- Cross-multiplies branches: rule with 3 branches referencing rule with 3 branches = 9+ branches
- Causes stack overflow on deep hierarchies
- Builds massive equations even when target filters most branches

## Solution: World-Based Inversion

Replace equation-based inversion with world construction:
- Build worlds on-demand during inversion queries
- Each World = constraint universe + value
- Cross-product with automatic constraint-based pruning
- Algebraic solving per-world (not per massive equation)

**Key Insight**: Current approach expands expressions across ALL rule boundaries recursively. World-based approach only expands within single branch conditions, then merges via constraint intersection.

---

## Architecture

### Core Data Structure: World

```rust
struct World {
    // Constraints defining this "universe"
    // Key: FactPath, Value: FactConstraint (interval or exact value)
    constraints: HashMap<FactPath, FactConstraint>,
    
    // The value expression in this universe
    // Full Expression tree - supports linear AND non-linear (sqrt, pow, sin, etc.)
    value: Expression,
}
```

**Key Design Decision:** Using full `Expression` instead of limited `LinearExpr` enum allows:
- Non-linear math: `sqrt(income)`, `price^2`, `sin(angle)`
- Mathematical computations from AST
- Natural integration with existing expression system
- Recursive inversion via `algebra/isolation.rs`

### Inversion Flow

```
OLD (Equation-Based):
  1. Substitute all rule references recursively
  2. Expand entire equation (exponential cross-product)
  3. Simplify entire equation
  4. Solve branches
  → Exponential memory, stack overflow

NEW (World-Based with Symbolic Evaluation):
  1. APPLY SYMBOLIC EVALUATION (Phase 4) - before world building
     - Inject known facts into plan: `plan.with_typed_values({"state": "CA", ...})`
     - Call: `reduced_plan = evaluator.evaluate_symbolic(&plan)`
     - Example: `state == "CA" && income > X` becomes `income > X`
     - False branches pruned, earlier branches pruned if one becomes true
  2. Build worlds from reduced plan (Phase 6)
     For each branch in reduced plan:
     a. Condition already simplified (from optimized_condition)
     b. If condition → true: world applies unconditionally
     c. Extract constraints from condition → World
     d. If references other rules:
        - Recursively build referenced rule's worlds (using same reduced plan)
        - Merge via constraint intersection
        - Contradictions return None (auto-prune)
  3. Solve each world:
     - Linear: use existing algebraic isolation
     - Non-linear: use enhanced inversion (sqrt, pow, trig)
  → Linear memory, no recursion explosion, minimal search space
```

**Key Optimization:** Symbolic evaluation (Phase 4) transforms multi-dimensional problems to 1D:
- Query: "income needed for tax=10000" with `state="CA"`, `filing_status="single"`
- Without symbolic eval: Build paths for 50 states × 4 statuses = 200 combinations
- With symbolic eval: Substitute knowns → only 1 relevant path remains

---

## New Module Architecture

**Current:**
```
computation/
  ├── expansion.rs          ← Logical + algebraic mixed
  ├── simplification.rs     ← Logical + algebraic mixed
  └── constraints.rs        ← Used by inversion
inversion/
  └── solver.rs             ← Algebraic isolation buried here
```

**Proposed:**
```
algebra/                    ← Mathematical reasoning tools (engine-level)
  ├── mod.rs
  ├── expansion.rs          ← Moved from computation/ (DNF, distribution) ✅
  ├── simplification.rs     ← Moved from computation/ (contradiction, folding) ✅
  ├── constraints.rs        ← Moved + ENHANCE Phase 5 (is_exact, contains)
  ├── isolation.rs          ← Extracted + ENHANCE Phase 7 (wraps computation/arithmetic)
  └── math_properties.rs    ← Moved (Algebraic identities) ✅
computation/                ← Runtime operations (REUSED by algebra/)
  ├── arithmetic.rs         ← Type-aware arithmetic (REUSED in Phase 7)
  ├── comparison.rs         ← Type-aware comparisons
  ├── datetime.rs           ← Date/time operations  
  ├── units.rs              ← Unit conversions
  └── mod.rs
evaluation/                 ← Execution of plans with full or symbolic facts
  ├── mod.rs                ← MODIFIED: symbolic_mode, evaluate_symbolic() Phase 4 ✅
  ├── expression.rs         ← MODIFIED: EvaluationResult enum, symbolic mode Phase 4 ✅
  └── operations.rs         ← Existing: arithmetic/comparison operations
inversion/
  ├── world.rs              ← NEW Phase 5: World (reuses constraint.intersect())
  ├── world_builder.rs      ← NEW Phase 6: WorldBuilder (reuses extract_constraints, separate collect_rule_paths)
  ├── response.rs           ← Existing (REUSED Phase 8: Solution/InversionResponse)
  └── mod.rs                ← MODIFY Phase 8: new invert() (reuses Target/TargetOp)
planning/
  └── execution_plan.rs     ← MODIFIED: optimize() method Phase 3 ✅
```

**Key distinction:**
- `computation/` = Lemma language features users write (runtime operations)
- `algebra/` = Mathematical reasoning tools the engine uses (meta-level)

---

## Implementation Plan

### Phase 1: DELETE (Break All Inversion Tests)

**Goal: PURE DELETION. No additions. Break everything. Tests WILL fail.**

**File: `lemma/src/computation/expansion.rs`**

Replace line 131 (in `cross_multiply_arithmetic`):
```rust
// OLD:
results.push(expand(product));

// NEW:
results.push(product);
```

Replace line 151 (in `cross_multiply_comparison`):
```rust
// OLD:
results.push(expand(product));

// NEW:
results.push(product);
```

**Reason**: Removes recursive `expand()` calls that cause stack overflow. The children are already expanded, so re-expanding the product is unnecessary and causes exponential recursion.

**File: `lemma/src/computation/simplification.rs`**

Delete from `reduce()` function:
```rust
// Line 51 - DELETE:
term_sets = apply_absorption(term_sets);

// Line 54 - DELETE:
term_sets = apply_term_combination(term_sets);

// Line 57 - DELETE:
term_sets = apply_consensus(term_sets);
```

Delete entire functions:
```rust
// Lines ~311-333 - DELETE:
fn apply_absorption(...)
fn is_subset(...)

// Lines ~339-384 - DELETE:
fn apply_term_combination(...)

// Lines ~388-435 - DELETE:
fn try_combine(...)

// Lines ~441-535 - DELETE:
fn apply_consensus(...)
fn is_consensus_term(...)
```

**File: `lemma/src/planning/equation.rs`**

```rust
// DELETE ENTIRE FILE (332 lines)
```

**File: `lemma/src/planning/mod.rs`**

```rust
// DELETE line:
pub mod equation;
```

**File: `lemma/src/planning/execution_plan.rs`**

Delete equation-related code:
```rust
// Line 7 - DELETE:
use crate::planning::equation;

// Lines 65-67 - DELETE from ExecutableRule struct:
/// Symbolic equation for inversion: (cond_0 ∧ result_0) ∨ (cond_1 ∧ result_1) ∨ ...
/// Rule references are substituted with their equations (computed in topo order).
pub equation: Expression,

// Lines 122-125 - DELETE from ExecutableRule initialization:
equation: Expression::new(
    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)),
    None,
),

// Line 130 - DELETE:
build_equations_for_rules(&mut executable_rules, graph);

// Lines 168-180 - DELETE entire function:
fn build_equations_for_rules(rules: &mut [ExecutableRule], graph: &Graph) {
    let mut cache: HashMap<RulePath, std::sync::Arc<Expression>> = HashMap::new();
    for rule in rules.iter_mut() {
        let has_dependencies = graph.rules().get(&rule.path)
            .map(|node| !node.depends_on_rules.is_empty())
            .unwrap_or(false);
        rule.equation = equation::build_equation(&rule.branches, &rule.path, &mut cache, has_dependencies);
    }
}

// Test code - DELETE equation field from 4 test ExecutableRule constructors (lines ~560, 768, 865, 940):
equation: create_literal_expr(LiteralValue::Boolean(BooleanValue::False)),
```

**File: `lemma/src/inversion/mod.rs`**

Delete entire function body (lines ~240-305):
```rust
pub fn invert(...) {
    // DELETE EVERYTHING INSIDE
}
```

**File: `lemma/src/algebra/mod.rs`** (NEW)

Create new module:
```rust
//! Mathematical reasoning tools for the Lemma engine

pub mod isolation;
```

**File: `lemma/src/algebra/isolation.rs`** (NEW - move from solver.rs)

**MOVE isolation functions (lines 491-1011) from `lemma/src/inversion/solver.rs`:**

```rust
// Lines 491-1011 - MOVE to algebra/isolation.rs:
pub enum IsolationResult { ... }
fn collect_facts(...) { ... }
fn collect_facts_recursive(...) { ... }
fn contains_fact(...) { ... }
pub fn try_isolate_comparison(...) { ... }  // Make public
fn isolate_single_fact(...) { ... }
fn isolate_from_left(...) { ... }
fn isolate_from_right(...) { ... }
fn flip_comparison(...) { ... }
fn try_simplify_constants(...) { ... }
fn extract_constant_sum(...) { ... }
```

**File: `lemma/src/computation/constraints.rs`**

**ADD extract_constraints function (lines 340-489) from `lemma/src/inversion/solver.rs`:**

```rust
// ADD at end of file - move from solver.rs lines 340-489:
/// Extract constraints from an expression into a ConstraintSet
///
/// Converts an optimized condition (already in DNF) into fact constraints.
pub fn extract_constraints(expression: &Expression, constraint_set: &mut ConstraintSet) {
    // ... (move entire implementation from solver.rs)
}
```

**File: `lemma/src/inversion/solver.rs`**

**DELETE ENTIRE FILE** (all 2083 lines - useful parts already moved above):
```rust
// DELETE EVERYTHING - isolation moved to algebra/, extract_constraints moved to computation/
```

**File: `lemma/src/lib.rs`**

Add new module declaration:
```rust
// ADD:
pub mod algebra;
```

**Expected Result:** 
- ❌ Code does NOT compile
- ❌ Many functions missing (solver.rs deleted, equation.rs deleted)
- ❌ All inversion broken
- ✅ algebra/isolation.rs exists with moved code
- ✅ computation/constraints.rs has extract_constraints
- **THIS IS THE GOAL - broken code is expected**

### Phase 2: Create Algebra Module (Move & Reorganize)

**Goal: Create clean separation between computation (runtime) and algebra (reasoning).**

**File: `lemma/src/algebra/mod.rs`** (NEW)

```rust
//! Mathematical reasoning tools for the Lemma engine
//!
//! Provides algebraic operations for planning, evaluation, and inversion.
//! NOT to be confused with computation/ which contains Lemma's runtime operations.

pub mod expansion;
pub mod simplification;
pub mod constraints;
pub mod isolation;
pub mod math_properties;
```

**File: `lemma/src/algebra/expansion.rs`** (MOVE from `computation/expansion.rs`)

```rust
// Move entire file from computation/expansion.rs
// Already has recursive expand() removed from Phase 1
```

**File: `lemma/src/algebra/simplification.rs`** (MOVE from `computation/simplification.rs`)

```rust
// Move entire file from computation/simplification.rs
// With multi-branch optimizations already deleted from Phase 1
```

**File: `lemma/src/algebra/constraints.rs`** (MOVE from `computation/constraints.rs`)

```rust
// Move entire file from computation/constraints.rs
// Already includes extract_constraints (added in Phase 1)
```

**File: `lemma/src/algebra/isolation.rs`** (Already moved in Phase 1)

```rust
// Already exists from Phase 1 (moved from solver.rs lines 491-1011)
// Just update imports to use algebra::constraints instead of computation::
```

Update imports:
```rust
// OLD:
use crate::computation::{OperationResult, UnsatReason};

// NEW:
use crate::algebra::constraints::UnsatReason;
```

**File: `lemma/src/algebra/math_properties.rs`** (NEW)

```rust
//! Algebraic identities and properties
//!
//! Commutativity, associativity, distributivity, etc.
//! To be used by future optimization passes.

// Placeholder for now - to be filled in later phases
```

**Update imports throughout codebase:**

```rust
// OLD:
use crate::computation::{expand, simplification, ConstraintSet};

// NEW:
use crate::algebra::{expand, simplification, ConstraintSet};
```

**Files to update:**
- `lemma/src/planning/graph.rs`
- `lemma/src/planning/execution_plan.rs`
- `lemma/src/inversion/mod.rs`
- Any other files importing from `computation/expansion.rs` or `computation/simplification.rs`

### Phase 3: Add Planning-Time Branch Optimization

**File: `lemma/src/planning/optimization.rs`** (NEW)

Modify Branch struct (around line 69):
```rust
```rust
pub struct Branch {
    pub condition: Expression,
    
    /// Optimized condition (expanded to DNF + simplified)
    /// Set by ExecutionPlan::optimize() after symbolic evaluation
    pub optimized_condition: Option<Expression>,  // NEW
    
    pub result: Expression,
    pub source: Option<Source>,
}
```

Add `optimize()` method to ExecutionPlan impl (after other methods):

```rust
/// Optimize branch conditions for constraint extraction
///
/// Expands to DNF and simplifies boolean expressions.
/// Should be called after symbolic evaluation to optimize only surviving branches.
pub fn optimize(mut self) -> Self {
    for rule in &mut self.rules {
        for branch in &mut rule.branches {
            let expanded = crate::algebra::expand(branch.condition.clone());
            let simplified = crate::algebra::simplification::reduce(expanded);
            branch.optimized_condition = Some(simplified);
        }
    }
    self
}
```

Update all Branch constructors in tests to include `optimized_condition: None`

### Phase 4: Add Symbolic Evaluation (Critical Optimization)

This is the most important optimization - reduces search space before world building by partially
evaluating with known facts. Transforms 50 states × 4 statuses = 200 paths → 1 path when state/status known.

**Key Design Decision:** Reuse existing evaluation infrastructure with a `symbolic_mode` flag instead
of reimplementing evaluation logic. This is partial evaluation - evaluate what you can (known facts),
leave what you can't (unknown facts) symbolic.

#### Changes:

**File: `lemma/src/evaluation/expression.rs`** (ADD `EvaluationResult` enum)

Add enum at top of file:

```rust
/// Result of expression evaluation
pub enum EvaluationResult {
    /// Successfully evaluated to a value or veto
    Evaluated(OperationResult),
    /// Contains unknown facts (symbolic mode only)
    Symbolic(Expression),
}
```

Change `evaluate_expression` return type:

```rust
pub(crate) fn evaluate_expression(
    expr: &Expression,
    context: &mut crate::evaluation::EvaluationContext,
) -> crate::LemmaResult<EvaluationResult>  // Changed from OperationResult
```

Update FactPath handling to return `Symbolic` instead of `Err`:

```rust
None => {
    if context.is_symbolic() {
        return Ok(EvaluationResult::Symbolic(current.clone()));
    } else {
        // Normal mode veto
        return Ok(EvaluationResult::Evaluated(OperationResult::Veto(...)));
    }
}
```

Wrap all other `Ok(OperationResult::...)` returns with `Ok(EvaluationResult::Evaluated(...))`

Update callers in `evaluate_rule` to unwrap `Evaluated` variant (panic on Symbolic in normal mode)

**File: `lemma/src/evaluation/mod.rs`** (MODIFY `EvaluationContext`)

Add `symbolic_mode` field to `EvaluationContext` struct (around line 22):

```rust
pub struct EvaluationContext {
    facts: HashMap<FactPath, LemmaFact>,
    rule_results: HashMap<RulePath, OperationResult>,
    rule_proofs: HashMap<RulePath, crate::evaluation::proof::Proof>,
    operations: Vec<crate::OperationRecord>,
    source_text: HashMap<String, (String, String)>,
    proof_nodes: HashMap<crate::Expression, crate::evaluation::proof::ProofNode>,
    symbolic_mode: bool, // NEW: when true, return symbolic expr for unknown facts
}
```

Add constructor and helper method to `EvaluationContext` impl (around line 32):

```rust
impl EvaluationContext {
    fn new(plan: &ExecutionPlan) -> Self {
        Self {
            facts: plan.facts.clone(),
            rule_results: HashMap::new(),
            rule_proofs: HashMap::new(),
            operations: Vec::new(),
            source_text: plan.graph().sources().clone(),
            proof_nodes: HashMap::new(),
            symbolic_mode: false, // Normal mode
        }
    }

    // NEW: Create context for symbolic evaluation
    fn new_symbolic(plan: &ExecutionPlan) -> Self {
        Self {
            facts: plan.facts.clone(),
            rule_results: HashMap::new(),
            rule_proofs: HashMap::new(),
            operations: Vec::new(),
            source_text: plan.graph().sources().clone(),
            proof_nodes: HashMap::new(),
            symbolic_mode: true, // Symbolic mode - unknown facts return original expr
        }
    }

    // ... existing get_fact, push_operation, etc. ...

    // NEW: Check if we're in symbolic mode
    fn is_symbolic(&self) -> bool {
        self.symbolic_mode
    }
}
```

**File: `lemma/src/evaluation/mod.rs`** (ADD method to `Evaluator` impl)

Add `evaluate_symbolic` method after the `evaluate` method (around line 130):

```rust
/// Symbolically reduce execution plan using known fact values
///
/// Partially evaluates branch conditions and results using known facts,
/// leaving unknown facts symbolic. Prunes branches that evaluate to false.
/// Also prunes earlier branches when a branch becomes unconditionally true
/// (last-wins optimization).
///
/// Example:
/// - Plan has branches: `state == "CA" && income > 50000`, `state == "NY" && ...`
/// - Plan has `state = "CA"` (known), `income` not set (unknown)
/// - Branch 1 simplifies to: `income > 50000` (keep)
/// - Branch 2 simplifies to: `false` (pruned)
///
/// This transforms multi-dimensional search (50 states × 4 statuses = 200 paths)
/// into 1D search (just income) when state and status are known.
///
/// Known facts should be injected into the plan using `with_values`/`with_typed_values` first.
pub fn evaluate_symbolic(&self, plan: &ExecutionPlan) -> ExecutionPlan {
    use crate::planning::{Branch, ExecutableRule};
    use crate::semantic::{BooleanValue, ExpressionKind, LiteralValue};

    let mut context = EvaluationContext::new_symbolic(plan);

    let reduced_rules: Vec<ExecutableRule> = plan
        .rules
        .iter()
        .map(|rule| {
            let mut simplified_branches: Vec<Branch> = Vec::new();

            for branch in &rule.branches {
                context.operations.clear();
                context.proof_nodes.clear();

                // Evaluate condition symbolically (returns expr if unknown facts)
                let simplified_condition = 
                    expression::evaluate_expression(&branch.condition, &mut context)
                        .unwrap_or_else(|_| branch.condition.clone());

                // Prune branches that evaluate to false
                if matches!(
                    &simplified_condition.kind,
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False))
                ) {
                    continue; // Skip this branch
                }

                // Evaluate result symbolically
                let simplified_result = 
                    expression::evaluate_expression(&branch.result, &mut context)
                        .unwrap_or_else(|_| branch.result.clone());

                simplified_branches.push(Branch {
                    condition: branch.condition.clone(),
                    optimized_condition: Some(simplified_condition),
                    result: simplified_result,
                    source: branch.source.clone(),
                });
            }

            // Last-wins optimization: if a branch is unconditionally true,
            // prune all earlier branches (they'll never be reached)
            let final_branches = if let Some(pos) = simplified_branches.iter().position(|b| {
                matches!(
                    &b.optimized_condition.as_ref().unwrap().kind,
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True))
                )
            }) {
                // Keep only from this position onward
                simplified_branches.into_iter().skip(pos).collect()
            } else {
                simplified_branches
            };

            ExecutableRule {
                path: rule.path.clone(),
                name: rule.name.clone(),
                branches: final_branches,
                needs_facts: rule.needs_facts.clone(),
                source: rule.source.clone(),
            }
        })
        .collect();

    ExecutionPlan::new(
        plan.doc_name.clone(),
        plan.facts.clone(),
        reduced_rules,
        plan.graph().clone(),
    )
}
```

**Key Benefits:**
- Clean `EvaluationResult` enum - no error-as-control-flow
- Reuses ALL existing evaluation logic (arithmetic, comparisons, boolean ops, etc.)
- Single flag change (`symbolic_mode`) enables partial evaluation
- Aggressive pruning: removes false branches AND unreachable earlier branches
- Natural integration with existing evaluation infrastructure
- Optimization happens AFTER symbolic evaluation (only on surviving branches)

### Phase 5: Add World Structure

**File: `lemma/src/algebra/constraints.rs`** (ENHANCE existing FactConstraint)

Add helper methods to existing FactConstraint impl (around line 408):

```rust
impl FactConstraint {
    // ... existing methods ...
    
    /// Check if constraint represents a single exact value
    pub fn is_exact(&self) -> bool {
        matches!(self, FactConstraint::Enumeration(vals) if vals.len() == 1)
    }
    
    /// Create constraint for exact value
    pub fn exact(value: LiteralValue) -> Self {
        FactConstraint::Enumeration(vec![value])
    }
    
    /// Check if a value satisfies this constraint
    pub fn contains(&self, value: &LiteralValue) -> bool {
        match self {
            FactConstraint::Unconstrained => true,
            FactConstraint::Enumeration(vals) => vals.contains(value),
            FactConstraint::Range { min, max } => {
                value_in_bounds(value, min) && value_in_bounds(value, max)
            }
            FactConstraint::Union(parts) => parts.iter().any(|p| p.contains(value)),
            FactConstraint::Complement(inner) => !inner.contains(value),
        }
    }
}

fn value_in_bounds(value: &LiteralValue, bound: &Bound) -> bool {
    match bound {
        Bound::Unbounded => true,
        Bound::Inclusive(b) => value <= b,
        Bound::Exclusive(b) => value < b,
    }
}
```

**File: `lemma/src/inversion/world.rs`** (NEW)

Simplified - value is just an Expression (supports non-linear math).

```rust
use std::collections::HashMap;
use crate::semantic::{Expression, FactPath};
use crate::algebra::constraints::FactConstraint;

/// A World represents one "universe" where specific constraints hold
#[derive(Clone, Debug)]
pub struct World {
    /// Constraints defining this universe (e.g., income in [11000, 44000])
    pub constraints: HashMap<FactPath, FactConstraint>,
    
    /// The value expression valid in this universe
    /// Can be literal, linear, or non-linear (sqrt, pow, etc.)
    pub value: Expression,
}

impl World {
    /// Merge two worlds (used in cross-product)
    /// Returns None if constraints contradict
    pub fn merge(
        &self,
        other: &World,
        combine_values: impl FnOnce(&Expression, &Expression) -> Expression,
    ) -> Option<World> {
        let mut new_constraints = self.constraints.clone();
        
        // Constraint intersection - THE KEY PRUNING MECHANISM
        for (fact, constraint_b) in &other.constraints {
            match new_constraints.get(fact) {
                Some(constraint_a) => {
                    // If they contradict, return None (world is impossible)
                    let intersection = constraint_a.intersect(constraint_b)?;
                    new_constraints.insert(fact.clone(), intersection);
                }
                None => {
                    new_constraints.insert(fact.clone(), constraint_b.clone());
                }
            }
        }
        
        // Combine values using provided function
        let new_value = combine_values(&self.value, &other.value);
        
        Some(World {
            constraints: new_constraints,
            value: new_value,
        })
    }
}

```

### Phase 6: Add World Builder

**File: `lemma/src/inversion/world_builder.rs`** (NEW)

Works with pre-reduced ExecutionPlan (symbolic evaluation already applied).

```rust
//! On-demand world building for inversion queries

use crate::algebra::constraints::{ConstraintSet, extract_constraints};
use crate::planning::ExecutionPlan;
use crate::semantic::{BooleanValue, Expression, ExpressionKind, FactPath, LiteralValue, RulePath};
use crate::LemmaError;
use std::collections::HashMap;

use super::world::World;

/// Builds worlds on-demand for inversion queries
pub struct WorldBuilder<'a> {
    /// Pre-reduced execution plan (evaluate_symbolic already called)
    plan: &'a ExecutionPlan,
    /// Cache to avoid rebuilding same rule's worlds
    cache: HashMap<RulePath, Vec<World>>,
}

impl<'a> WorldBuilder<'a> {
    /// Create WorldBuilder with pre-reduced and optimized plan
    /// 
    /// The plan should have been:
    /// 1. Injected with known facts via with_typed_values()
    /// 2. Symbolically evaluated via evaluate_symbolic()
    /// 3. Optimized via optimize() for DNF structure
    pub fn new(plan: &'a ExecutionPlan) -> Self {
        Self {
            plan,
            cache: HashMap::new(),
        }
    }
    
    /// Build worlds for a rule (lazy, on-demand)
    /// 
    /// Branches have already been symbolically evaluated and pruned.
    /// This extracts constraints and builds worlds from simplified branches.
    pub fn build_worlds(&mut self, rule_name: &str) -> Result<Vec<World>, LemmaError> {
        let rule = self.plan.get_rule(rule_name)
            .ok_or_else(|| LemmaError::Engine(format!("Rule not found: {}", rule_name)))?;
        let rule_path = rule.path.clone();
        
        // Check cache
        if let Some(cached) = self.cache.get(&rule_path) {
            return Ok(cached.clone());
        }
        
        let mut worlds = Vec::new();
        
        for branch in &rule.branches {
            // Branch already symbolically evaluated - use optimized_condition if available
            let condition = branch.optimized_condition.as_ref().unwrap_or(&branch.condition);
            let result = &branch.result;
            
            // If condition is literal true, result applies unconditionally
            if matches!(&condition.kind, 
                ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True))) {
                worlds.push(World {
                    constraints: HashMap::new(),
                    value: result.clone(),
                });
                continue;
            }
            
            // Extract constraints from condition (benefits from DNF optimization)
            let mut constraint_set = ConstraintSet::new();
            extract_constraints(condition, &mut constraint_set);
            let constraints = constraint_set.to_fact_constraints();
            
            // Check if result references other rules
            let rule_refs = extract_rule_references(result);
            
            if rule_refs.is_empty() {
                // Simple case: no rule dependencies in result
                worlds.push(World {
                    constraints,
                    value: result.clone(),
                });
            } else {
                // Complex case: recursively build referenced rule worlds
                let branch_worlds = self.build_with_references(
                    constraints,
                    result,
                    &rule_refs
                )?;
                worlds.extend(branch_worlds);
            }
        }
        
        // Cache for future queries
        self.cache.insert(rule_path, worlds.clone());
        Ok(worlds)
    }
    
    /// Build worlds with rule references (cross-product with pruning)
    fn build_with_references(
        &mut self,
        base_constraints: HashMap<FactPath, FactConstraint>,
        result: &Expression,
        rule_refs: &[RuleReference],
    ) -> Result<Vec<World>, Error> {
        let mut worlds = vec![World {
            constraints: base_constraints,
            value: result.clone(),
        }];
        
        // For each referenced rule, cross-product merge
        for rule_ref in rule_refs {
            // Recursively build worlds for referenced rule
            let ref_worlds = self.build_worlds(&rule_ref.path.to_string())?;
            
            // Cross-product merge with pruning
            let mut new_worlds = Vec::new();
            for base_world in &worlds {
                for ref_world in &ref_worlds {
                    // Merge constraints; returns None if contradiction
                    if let Some(merged) = base_world.merge(ref_world, |base_val, ref_val| {
                        // Substitute rule reference in base_val with ref_val
                        substitute_rule_reference(base_val, &rule_ref.path, ref_val)
                    }) {
                        new_worlds.push(merged);
                    }
                    // Contradictions are auto-pruned (merge returns None)
                }
            }
            worlds = new_worlds;
        }
        
        Ok(worlds)
    }
}

// Helper functions
// NOTE: These utilities already exist in the codebase:
// - expr.is_boolean_true() - semantic.rs line 171
// - expr.is_boolean_false() - semantic.rs line 162
// - algebra::isolation::collect_facts() - pattern for walking expression tree
// - algebra::constraints::extract_constraints() - extracts constraints from conditions

/// Collect all rule paths from an expression (similar to algebra::isolation::collect_facts)
fn collect_rule_paths(expr: &Expression) -> HashSet<RulePath> {
    let mut paths = HashSet::new();
    collect_rule_paths_recursive(expr, &mut paths);
    paths
}

fn collect_rule_paths_recursive(expr: &Expression, paths: &mut HashSet<RulePath>) {
    match &expr.kind {
        ExpressionKind::RulePath(path) => {
            paths.insert(path.clone());
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            collect_rule_paths_recursive(left, paths);
            collect_rule_paths_recursive(right, paths);
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner)
        | ExpressionKind::UnitConversion(inner, _) => {
            collect_rule_paths_recursive(inner, paths);
        }
        _ => {}
    }
}

/// Substitute a rule path with its value expression
fn substitute_rule_path(expr: &Expression, target: &RulePath, replacement: &Expression) -> Expression {
    match &expr.kind {
        ExpressionKind::RulePath(path) if path == target => replacement.clone(),
        ExpressionKind::Arithmetic(left, op, right) => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(substitute_rule_path(left, target, replacement)),
                *op,
                Arc::new(substitute_rule_path(right, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::Comparison(left, op, right) => Expression::new(
            ExpressionKind::Comparison(
                Arc::new(substitute_rule_path(left, target, replacement)),
                *op,
                Arc::new(substitute_rule_path(right, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::LogicalAnd(left, right) => Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(substitute_rule_path(left, target, replacement)),
                Arc::new(substitute_rule_path(right, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::LogicalOr(left, right) => Expression::new(
            ExpressionKind::LogicalOr(
                Arc::new(substitute_rule_path(left, target, replacement)),
                Arc::new(substitute_rule_path(right, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::LogicalNegation(inner, style) => Expression::new(
            ExpressionKind::LogicalNegation(
                Arc::new(substitute_rule_path(inner, target, replacement)),
                *style,
            ),
            expr.source.clone(),
        ),
        ExpressionKind::MathematicalComputation(op, inner) => Expression::new(
            ExpressionKind::MathematicalComputation(
                *op,
                Arc::new(substitute_rule_path(inner, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::UnitConversion(inner, target_unit) => Expression::new(
            ExpressionKind::UnitConversion(
                Arc::new(substitute_rule_path(inner, target, replacement)),
                target_unit.clone(),
            ),
            expr.source.clone(),
        ),
        _ => expr.clone(),
    }
}

struct RuleReference {
    path: RulePath,
}
```

### Phase 7: Enhance Algebraic Isolation (Non-Linear Inversion)

**File: `lemma/src/algebra/isolation.rs`**

Add recursive expression inversion to handle non-linear math.

```rust
// ADD to existing isolation.rs:

/// A single solution from inversion (mini-world)
#[derive(Debug, Clone)]
pub struct InversionSolution {
    /// The value for this solution
    pub value: LiteralValue,
    
    /// Additional constraints for this solution branch
    /// Example: x^2 = 4 gives x=2 with constraint x>0
    pub constraints: HashMap<FactPath, FactConstraint>,
    
    /// Domain restrictions encountered during inversion
    pub restrictions: Vec<DomainRestriction>,
}

/// Result of inverting an expression
/// 
/// Can have 0 (unsatisfiable), 1 (typical), or multiple solutions (quadratic, abs, etc.)
/// All outcomes are domain information - empty vec = empty valid domain
#[derive(Debug, Clone)]
pub struct InversionResult {
    /// All possible solution branches
    /// Empty = unsatisfiable (no valid domain)
    pub solutions: Vec<InversionSolution>,
}

impl InversionResult {
    pub fn solved(value: LiteralValue) -> Self {
        Self {
            solutions: vec![InversionSolution {
                value,
                constraints: HashMap::new(),
                restrictions: vec![],
            }],
        }
    }
    
    pub fn unsatisfiable(restriction: DomainRestriction) -> Self {
        Self {
            solutions: vec![],
        }
    }
    
    pub fn is_unsatisfiable(&self) -> bool {
        self.solutions.is_empty()
    }
}

/// Recursively invert an expression to solve for a target fact
/// Example: solve sqrt(income) = 500 for income
///          -> income = 500^2 = 250000
///
/// Returns InversionResult with 0+ solutions (empty = unsatisfiable)
pub fn invert_expression(
    expr: &Expression,
    target_fact: &FactPath,
    target_value: &LiteralValue,
) -> InversionResult {
    match &expr.kind {
        // Base case: found the target fact
        ExpressionKind::FactPath(path) if path == target_fact => {
            InversionResult::solved(target_value.clone())
        }
        
        // Arithmetic: y = x + C => x = y - C
        ExpressionKind::Arithmetic(left, op, right) => {
            use ArithmeticComputation::*;
            
            let left_has_fact = contains_fact(left, target_fact);
            let right_has_fact = contains_fact(right, target_fact);
            
            // Can only invert if fact appears on one side only
            if left_has_fact && !right_has_fact {
                // Isolate from left: y = x op C => x = ...
                let right_result = evaluate_to_literal(right);
                if right_result.is_unsatisfiable() {
                    return right_result;
                }
                let right_value = &right_result.solutions[0].value;
                
                let inverted = invert_arithmetic_left(target_value, *op, right_value);
                if inverted.is_unsatisfiable() {
                    return inverted;
                }
                let new_target = &inverted.solutions[0].value;
                
                invert_expression(left, target_fact, new_target)
            } else if !left_has_fact && right_has_fact {
                // Isolate from right: y = C op x => x = ...
                let left_result = evaluate_to_literal(left);
                if left_result.is_unsatisfiable() {
                    return left_result;
                }
                let left_value = &left_result.solutions[0].value;
                
                let inverted = invert_arithmetic_right(left_value, *op, target_value);
                if inverted.is_unsatisfiable() {
                    return inverted;
                }
                let new_target = &inverted.solutions[0].value;
                
                invert_expression(right, target_fact, new_target)
            } else {
                InversionResult::unsatisfiable(DomainRestriction {
                    facts: vec![target_fact.clone()],
                    description: "Fact appears multiple times in expression".to_string(),
                    source: "inversion".to_string(),
                })
            }
        }
        
        // Non-linear: y = sqrt(x) => x = y^2
        ExpressionKind::MathematicalComputation(op, inner) => {
            use MathematicalComputation::*;
            
            let inverted_result = match op {
                Sqrt => square_value(target_value),
                Sin | Cos | Tan => apply_inverse_trig(*op, target_value),
                Log => exp_value(target_value),
                Exp => log_value(target_value),
                Abs => {
                    // y = |x| => x = ±y (TWO solutions!)
                    // TODO: implement multiple solution branches
                    return InversionResult::unsatisfiable(DomainRestriction {
                        facts: vec![target_fact.clone()],
                        description: "Absolute value inversion not yet implemented (±)".to_string(),
                        source: "abs inversion".to_string(),
                    });
                }
                _ => return InversionResult::unsatisfiable(DomainRestriction {
                    facts: vec![target_fact.clone()],
                    description: format!("Unsupported operation: {:?}", op),
                    source: "inversion".to_string(),
                }),
            };
            
            if inverted_result.is_unsatisfiable() {
                return inverted_result;
            }
            
            let new_target = &inverted_result.solutions[0].value;
            invert_expression(inner, target_fact, new_target)
        }
        
        // Comparison: already handled by try_isolate_comparison
        ExpressionKind::Comparison(_, _, _) => {
            InversionResult::unsatisfiable(DomainRestriction {
                facts: vec![],
                description: "Cannot invert through comparison".to_string(),
                source: "inversion".to_string(),
            })
        }
        
        // Cannot invert through logical operations
        ExpressionKind::LogicalAnd(_, _) | ExpressionKind::LogicalOr(_, _) => {
            InversionResult::unsatisfiable(DomainRestriction {
                facts: vec![],
                description: "Cannot invert through logical operations".to_string(),
                source: "inversion".to_string(),
            })
        }
        
        // Literal doesn't contain the fact
        ExpressionKind::Literal(_) => {
            InversionResult::unsatisfiable(DomainRestriction {
                facts: vec![],
                description: "Fact not found in expression".to_string(),
                source: "inversion".to_string(),
            })
        }
        
        _ => InversionResult::unsatisfiable(DomainRestriction {
            facts: vec![],
            description: "Unsupported expression type".to_string(),
            source: "inversion".to_string(),
        }),
    }
}

// Arithmetic inversion helpers
fn invert_arithmetic_left(
    target: &LiteralValue,
    op: ArithmeticComputation,
    right: &LiteralValue,
) -> InversionResult {
    use ArithmeticComputation::*;
    use crate::computation::arithmetic::arithmetic_operation;
    
    let (inverse_op, inverse_right) = match op {
        Add => (Subtract, right.clone()),           // y = x + C => x = y - C
        Subtract => (Add, right.clone()),           // y = x - C => x = y + C
        Multiply => (Divide, right.clone()),        // y = x * C => x = y / C
        Divide => (Multiply, right.clone()),        // y = x / C => x = y * C
        Power => {
            // y = x ^ C => x = y ^ (1/C)
            let one = LiteralValue::Number(Decimal::from(1));
            match arithmetic_operation(&one, &Divide, right) {
                OperationResult::Value(inv_exp) => (Power, inv_exp),
                OperationResult::Veto(msg) => {
                    return InversionResult::unsatisfiable(DomainRestriction {
                        facts: vec![],
                        description: format!("Cannot compute 1/{}: {}", right, msg.unwrap_or_default()),
                        source: "power inversion".to_string(),
                    });
                }
            }
        }
        _ => return InversionResult::unsatisfiable(DomainRestriction {
            facts: vec![],
            description: format!("Unsupported operation: {:?}", op),
            source: "arithmetic inversion".to_string(),
        }),
    };
    
    match arithmetic_operation(target, &inverse_op, &inverse_right) {
        OperationResult::Value(result) => InversionResult::solved(result),
        OperationResult::Veto(msg) => InversionResult::unsatisfiable(DomainRestriction {
            facts: vec![],
            description: msg.unwrap_or_else(|| "Arithmetic operation failed".to_string()),
            source: "arithmetic inversion".to_string(),
        }),
    }
}

fn invert_arithmetic_right(
    left: &LiteralValue,
    op: ArithmeticComputation,
    target: &LiteralValue,
) -> InversionResult {
    use ArithmeticComputation::*;
    use crate::computation::arithmetic::arithmetic_operation;
    
    let result = match op {
        Add => arithmetic_operation(target, &Subtract, left),        // y = C + x => x = y - C
        Subtract => arithmetic_operation(left, &Subtract, target),   // y = C - x => x = C - y
        Multiply => arithmetic_operation(target, &Divide, left),     // y = C * x => x = y / C
        Divide => arithmetic_operation(left, &Divide, target),       // y = C / x => x = C / y
        Power => {
            // y = C ^ x => x = log_C(y) - needs numerical implementation
            return InversionResult::unsatisfiable(DomainRestriction {
                facts: vec![],
                description: "Logarithm inversion requires numerical implementation".to_string(),
                source: "power inversion".to_string(),
            });
        }
        _ => return InversionResult::unsatisfiable(DomainRestriction {
            facts: vec![],
            description: format!("Unsupported operation: {:?}", op),
            source: "arithmetic inversion".to_string(),
        }),
    };
    
    match result {
        OperationResult::Value(value) => InversionResult::solved(value),
        OperationResult::Veto(msg) => InversionResult::unsatisfiable(DomainRestriction {
            facts: vec![],
            description: msg.unwrap_or_else(|| "Arithmetic operation failed".to_string()),
            source: "arithmetic inversion".to_string(),
        }),
    }
}

// Mathematical operation helpers
// NOTE: Use computation::arithmetic::arithmetic_operation directly
// Veto results become unsatisfiable domain restrictions

fn square_value(value: &LiteralValue) -> InversionResult {
    use crate::computation::arithmetic::arithmetic_operation;
    let two = LiteralValue::Number(Decimal::from(2));
    match arithmetic_operation(value, &ArithmeticComputation::Power, &two) {
        OperationResult::Value(result) => InversionResult::solved(result),
        OperationResult::Veto(msg) => InversionResult::unsatisfiable(DomainRestriction {
            facts: vec![],
            description: msg.unwrap_or_else(|| "Cannot square value".to_string()),
            source: "sqrt inversion".to_string(),
        }),
    }
}

fn exp_value(value: &LiteralValue) -> InversionResult {
    // Use e^x - needs numerical implementation
    InversionResult::unsatisfiable(DomainRestriction {
        facts: vec![],
        description: "Exponential requires numerical implementation".to_string(),
        source: "log inversion".to_string(),
    })
}

fn log_value(value: &LiteralValue) -> InversionResult {
    // Natural log - needs numerical implementation
    InversionResult::unsatisfiable(DomainRestriction {
        facts: vec![],
        description: "Logarithm requires numerical implementation".to_string(),
        source: "exp inversion".to_string(),
    })
}

fn apply_inverse_trig(op: MathematicalComputation, value: &LiteralValue) -> InversionResult {
    // asin, acos, atan - needs numerical implementation
    InversionResult::unsatisfiable(DomainRestriction {
        facts: vec![],
        description: format!("Inverse {:?} requires numerical implementation", op),
        source: "trig inversion".to_string(),
    })
}

fn evaluate_to_literal(expr: &Expression) -> InversionResult {
    // Evaluate constant expression using existing evaluator
    let plan = crate::planning::ExecutionPlan::empty();
    let mut context = crate::evaluation::EvaluationContext::new(&plan);
    
    match crate::evaluation::expression::evaluate_expression(expr, &mut context) {
        Ok(crate::evaluation::expression::EvaluationResult::Evaluated(OperationResult::Value(v))) => {
            InversionResult::solved(v)
        }
        Ok(crate::evaluation::expression::EvaluationResult::Evaluated(OperationResult::Veto(msg))) => {
            InversionResult::unsatisfiable(DomainRestriction {
                facts: vec![],
                description: msg.unwrap_or_else(|| "Expression vetoed".to_string()),
                source: "constant evaluation".to_string(),
            })
        }
        _ => InversionResult::unsatisfiable(DomainRestriction {
            facts: vec![],
            description: "Expression contains unknown facts".to_string(),
            source: "constant evaluation".to_string(),
        }),
    }
}

// NOTE: All outcomes are domain information!
// Empty solutions vec = empty valid domain (unsatisfiable)
// Multiple solutions = multiple valid domains (e.g., x^2=4 gives ±2)
// Each solution carries its own constraints and restrictions
```

**Key reuse opportunities in Phase 7:**
- ✅ All arithmetic operations use `computation::arithmetic::arithmetic_operation` directly
- ✅ **Veto treated as constraint**, not error - becomes unsatisfiable domain
- ✅ No wrapper functions needed - use `arithmetic_operation` directly
- ✅ Existing linear isolation in `try_isolate_comparison()` and `isolate_single_fact()` unchanged
- ✅ `evaluate_to_literal()` reuses evaluation infrastructure with empty plan
- ⚠️ Non-linear functions (exp, log, trig) need numerical implementation via Rust standard library

**Design improvement:**
- `InversionResult` struct with `Vec<InversionSolution>` (0+ solutions)
- `InversionSolution` has value, constraints, and restrictions (mini-world)
- Division by zero? → Empty solutions vec (empty domain)
- Sqrt of negative? → Empty solutions vec
- x^2 = 4? → TWO solutions: {value: 2, constraints: x>0} and {value: -2, constraints: x<0}
- **All outcomes are domain information** - empty vec = empty valid domain

This gives us:
- **Linear inversion**: Add, Subtract, Multiply, Divide (existing in `try_isolate_comparison`)
- **Non-linear inversion**: Sqrt, Pow, Log, Exp, Trig functions (NEW in `invert_expression`)
- **Multiple solutions**: Natural support for quadratics, abs value, periodic functions
- **Clean semantics**: Everything is constraint/domain information, no special error cases

### Phase 8: Wire Up New Inversion

**File: `lemma/src/inversion/mod.rs`**

Replace the invert function to use new world-based approach:

```rust
use crate::algebra::isolation::invert_expression;
use crate::evaluation::symbolic;
use crate::inversion::world_builder::WorldBuilder;
use crate::semantic::{Expression, ExpressionKind, FactPath, LiteralValue};
use std::collections::HashMap;

pub fn invert(
    plan: &ExecutionPlan,
    rule_name: &str,
    operator: &str,
    outcome: Option<OperationResult>
) -> LemmaResult<InversionResponse> {
    let target = Target::from_str(operator, outcome)?;
    
    // 1. Build worlds on-demand with symbolic evaluation
    let mut builder = WorldBuilder::new(plan);
    let rule_worlds = builder.build_worlds(rule_name)?;
    
    // 2. Filter worlds matching target
    let matching_worlds: Vec<&World> = rule_worlds.iter()
        .filter(|w| matches_target(&w.value, &target))
        .collect();
    
    // 3. For each world, solve algebraically
    let solutions: Vec<Solution> = matching_worlds.iter()
        .flat_map(|world| solve_world(world, &target))
        .collect();
    
    Ok(InversionResponse { solutions })
}

/// Solve a single world
fn solve_world(world: &World, target: &Target) -> Vec<Solution> {
    // World.value is now an Expression (not Value enum)
    match &world.value.kind {
        // Case 1: Literal value
        ExpressionKind::Literal(lit) => {
            // Already matches (pre-filtered)
            vec![Solution::new(
                OperationResult::Value(lit.clone()),
                world.constraints.clone()
            )]
        }
        
        // Case 2: Veto
        ExpressionKind::Veto(veto) => {
            vec![Solution::new(
                OperationResult::Veto(veto.message.clone()),
                world.constraints.clone()
            )]
        }
        
        // Case 3: Expression containing unknown facts - needs algebraic inversion
        _ => {
            // Extract target value
            let target_value = match &target.outcome {
                Some(OperationResult::Value(val)) => val.clone(),
                _ => return vec![],
            };
            
            // Find which fact needs to be solved for
            let unknown_fact = find_unknown_fact(&world.value, &world.constraints)?;
            
            // Use algebraic inversion (handles linear + non-linear, multiple solutions)
            let inversion_result = invert_expression(&world.value, &unknown_fact, &target_value);
            
            let mut solutions = Vec::new();
            for inv_solution in inversion_result.solutions {
                // Verify solution satisfies world constraints
                if world.constraints.get(&unknown_fact).map_or(true, |c| c.contains(&inv_solution.value)) {
                    let mut solution_constraints = world.constraints.clone();
                    
                    // Merge solution's constraints
                    for (fact, constraint) in inv_solution.constraints {
                        solution_constraints.insert(fact, constraint);
                    }
                    
                    // Add exact constraint for the solved fact
                    solution_constraints.insert(
                        unknown_fact.clone(),
                        FactConstraint::exact(inv_solution.value)
                    );
                    
                    solutions.push(Solution::new(
                        OperationResult::Value(target_value.clone()),
                        solution_constraints
                    ));
                }
                // else: solution outside valid range, skip
                // todo: what does this mean?
                // should we error? should we inform about a veto/constraint?
            }
            
            // Return all valid solutions (empty if none work or inversion failed)
            solutions
        }
    }
}

fn find_unknown_fact(expr: &Expression, constraints: &HashMap<FactPath, FactConstraint>) -> Result<FactPath, Error> {
    // Find facts in expression that don't have exact values in constraints
    let facts_in_expr = collect_facts(expr);
    
    for fact in facts_in_expr {
        if let Some(constraint) = constraints.get(&fact) {
            if !constraint.is_exact() {
                return Ok(fact);
            }
        } else {
            return Ok(fact);
        }
    }
    
    Err(Error::NoUnknownFact)
}

fn parse_provided_facts(
    provided: &HashMap<String, String>,
    plan: &ExecutionPlan
) -> Result<HashMap<FactPath, LiteralValue>, Error> {
    todo!("Convert user-provided string facts to typed literals")
}
```

**Key changes in Phase 8:**
- WorldBuilder takes `known_facts` parameter (Phase 6)
- World.value is `Expression` not `Value` enum (Phase 5)
- Uses `invert_expression` for non-linear solving (Phase 7)
- Symbolic evaluation applied automatically by WorldBuilder (Phase 4)

### Validation

**After implementing phases 1-6:**

```bash
# Run tests
cargo nextest run

# Expected results:
✅ expansion tests: PASS (expand() still works)
✅ simplification tests: PASS (reduce() still works)
✅ forward evaluation tests: PASS (unchanged)
✅ inversion tests: PASS (now using path-based)

# Performance check:
✅ No stack overflow on deep hierarchies
✅ Memory usage linear with depth
✅ Faster on complex rule references
```

**Success Criteria:**
- All tests pass
- No equation substitution code remains
- Inversion uses Path.merge for cross-rule combination
- expand() only called on single branch conditions

---

## Examples

### Example 1: Simple Rule Reference

```lemma
rule tier = "bronze"
  unless points >= 100 then "silver"

rule rate = 5%
  unless tier? == "silver" then 10%
```

**World Construction (Forward Pass):**

**Step 1: Evaluate `tier`** (no dependencies)
- Branch 1: `points < 100 ∧ "bronze"` → World { constraints: {points: (-inf, 100)}, value: "bronze" }
- Branch 2: `points >= 100 ∧ "silver"` → World { constraints: {points: [100, +inf)}, value: "silver" }

**Step 2: Evaluate `rate`** (references tier)
- Branch 1: `tier? != "silver" ∧ 5%`
  - Cross-product with tier worlds:
    - tier World 1 ("bronze"): "bronze" != "silver" ✓ → World { {points: (-inf, 100)}, 5% }
    - tier World 2 ("silver"): "silver" != "silver" ✗ → Contradiction, pruned!
- Branch 2: `tier? == "silver" ∧ 10%`
  - Cross-product with tier worlds:
    - tier World 1 ("bronze"): "bronze" == "silver" ✗ → Contradiction, pruned!
    - tier World 2 ("silver"): "silver" == "silver" ✓ → World { {points: [100, +inf)}, 10% }

**Final: rate has 2 worlds** (not 4!)

**Inversion Query (Backward Pass):**
Query rate worlds for outcome = 10% → Returns World { {points: [100, +inf)}, 10% }

**Old approach:** 2×2 = 4 branches, simplify, solve
**New approach:** Cross-product creates 4 candidate worlds, 2 auto-pruned during merge, 1 filtered by target

### Example 2: Algebraic Expressions

```lemma
rule base_tax = income * 0.10

rule adjusted_tax = base_tax?
  unless income > 50000 then base_tax? + 1000
```

**World Construction:**

**Step 1: Evaluate `base_tax`**
- Single branch: `true ∧ income * 0.10`
  - World { constraints: {}, value: LinearExpr(income, 0.10, 0) }

**Step 2: Evaluate `adjusted_tax`** (references base_tax)
- Branch 1: `income <= 50000 ∧ base_tax?`
  - Merge with base_tax World: 
    - Constraints: {income: (-inf, 50000]}
    - Value: LinearExpr(income, 0.10, 0)
- Branch 2: `income > 50000 ∧ base_tax? + 1000`
  - Merge with base_tax World:
    - Constraints: {income: (50000, +inf)}
    - Value: LinearExpr(income, 0.10, 0) + 1000 = LinearExpr(income, 0.10, 1000)

**Final: adjusted_tax has 2 worlds**

**Inversion Query for adjusted_tax = 8000:**
- World 1: 8000 = 0.10 * income → income = 80,000
  - Check constraint: 80,000 <= 50,000? NO → Invalid
- World 2: 8000 = 0.10 * income + 1000 → income = 70,000
  - Check constraint: 70,000 > 50,000? YES → Valid!
- Solution: income = 70,000

**Key**: Algebraic solving happens per-world with automatic constraint validation

---

## Migration Strategy

### Step 1: Add World Solver Alongside (No Breaking Changes)
- Create `world_solver.rs`
- Add feature flag: `use_world_solver`
- Both solvers coexist

### Step 2: Validate with Tests
- Run all inversion tests with both solvers
- Compare results for correctness
- Benchmark performance

### Step 3: Switch Default
- Make path solver the default
- Keep equation solver for reference
- Monitor for issues

### Step 4: Clean Up
- Remove equation-based inversion code
- Simplify equation building (forward-eval only)
- Remove unused complexity

---

## Performance Expectations

### Memory
- **Current**: O(branches^depth) - exponential
- **New**: O(relevant_paths × max_depth) - linear in depth

### Time
- **Current**: Build all combinations then filter
- **New**: Only explore filtered paths
- **Speedup**: 10-100x for complex hierarchies with specific targets

### Example Metrics
```
Document: employment_contract (5 levels deep, avg 3 branches)
Current: 3^5 = 243 branches
New with target: ~3-10 paths explored
```

---

## Open Questions

1. **Caching**: Should we cache sub-rule results during path exploration?
   - Pro: Avoid re-exploring same rule multiple times
   - Con: Cache invalidation, memory

2. **Target filtering**: Should we filter at every level or only at leaves?
   - Current plan: Filter at top level, combine at lower levels

3. **Symbolic solutions**: How to handle when some paths have symbolic constraints?
   - Keep current `SolveResult::Partial` behavior

4. **Forward evaluation**: Does forward eval still need equations?
   - Likely yes - forward eval benefits from pre-computed equations
   - Could also use path-finding but less benefit (no target filter)

---

## Why This Works

### 1. No More Recursive Expansion Across Rules

```
OLD:
  rate references tier
  tier references points
  → Substitute tier into rate
  → Expand: distribute comparisons through all OR branches
  → Stack overflow / exponential blowup

NEW:
  rate: Build 2 worlds (condition per branch)
  tier: Build 2 worlds (condition per branch)
  → Merge via constraint intersection
  → 2×2=4 attempts, 2 contradictions pruned → 2 worlds
  → No explosion!
```

### 2. Expand/Simplify Stay, But Used Differently

```
OLD:
  expand(entire_substituted_equation_with_all_rule_refs)
  → Recursive expand() calls in cross_multiply
  → Exponential

NEW:
  For each branch:
    expand(branch.condition)  // Single branch only!
    simplify(expanded)
    extract_constraints()
  → Linear per branch
  → Cross-rule via World.merge (not expand)
```

### 3. The Key Deletion

```diff
fn cross_multiply_comparison(...) {
    for left in left_branches {
        for right in &right_branches {
            let product = make_comparison(...);
-           results.push(expand(product));  // ← DELETE THIS
+           results.push(product);           // ← Just the product
        }
    }
}
```

**Why delete?** 
- With path-based inversion, we never cross-multiply across rules anymore
- expand() is only called on single branch conditions
- Cross-multiply only used within expand() for OR distribution (NOT across rules)

---

## Success Criteria

**Phase 1 (DELETE):** ✅ **COMPLETE**
- ✅ Code does NOT compile (expected)
- ✅ Many functions missing (expected)
- ✅ Everything broken (expected)
- ✅ Useful parts moved (isolation, extract_constraints)
- ✅ `planning/equation.rs` deleted
- ✅ `inversion/solver.rs` deleted
- ✅ `algebra/isolation.rs` created with moved functions
- ✅ `computation/constraints.rs` has `extract_constraints`
- ✅ `invert()` function replaced with `todo!()`
- ✅ Recursive `expand()` calls removed from cross_multiply functions
- ✅ Multi-branch optimization functions deleted from simplification.rs

**Phase 2 (ALGEBRA MODULE):** ✅ **COMPLETE**
- ✅ `algebra/mod.rs` created with all submodules
- ✅ `computation/expansion.rs` → `algebra/expansion.rs` moved
- ✅ `computation/simplification.rs` → `algebra/simplification.rs` moved
- ✅ `computation/constraints.rs` → `algebra/constraints.rs` moved
- ✅ `algebra/math_properties.rs` placeholder created
- ✅ All imports updated throughout codebase
- ✅ `algebra/isolation.rs` imports updated to use `algebra::constraints`
- ✅ Old computation files deleted
- ✅ No backward compatibility re-exports
- ✅ Clean module separation: `algebra/` (reasoning) vs `computation/` (runtime)
- ❌ All inversion tests fail (old entry points deleted - expected)

**Phase 3 (BRANCH OPTIMIZATION):** ✅ **COMPLETE**
- ✅ Branches have `optimized_condition: Option<Expression>` field
- ✅ `ExecutionPlan::optimize()` method added (inlined optimization logic)
- ✅ Optimization removed from planning (moved to post-symbolic-evaluation)
- ✅ `planning/optimization.rs` module removed (logic inlined)
- ✅ All Branch constructors in tests updated with `optimized_condition: None`

**Phase 4 (SYMBOLIC EVALUATION):** ✅ **COMPLETE**
- ✅ Code compiles
- ✅ `EvaluationResult` enum added (Evaluated/Symbolic) - no error-as-control-flow
- ✅ `symbolic_mode: bool` field added to `EvaluationContext`
- ✅ `new_symbolic()` constructor and `is_symbolic()` method added to `EvaluationContext`
- ✅ `expression::evaluate_expression` returns `EvaluationResult` and handles unknown facts
- ✅ All return points updated to return `EvaluationResult::Evaluated` or `Symbolic`
- ✅ `evaluate_mathematical_operator` and `propagate_veto_proof` updated to return `EvaluationResult`
- ✅ `Evaluator::evaluate_symbolic()` method added with `evaluate_to_expression` helper
- ✅ Can partially evaluate with known facts, leave unknown facts symbolic
- ✅ Prunes branches that evaluate to false
- ✅ Prunes earlier branches when one becomes unconditionally true (last-wins optimization)
- ✅ Returns reduced ExecutionPlan with simplified conditions

**Phase 5 (WORLD STRUCTURE):** ✅ **COMPLETE**
- ✅ Code compiles
- ✅ `inversion/world.rs` created
- ✅ World struct with constraints HashMap and value Expression
- ✅ World::merge() method with constraint intersection and pruning
- ✅ FactConstraint helper methods added: is_exact(), exact(), contains()
- ✅ value_in_bounds() helper function added
- ✅ eval_at() removed (not needed for current implementation)

**Phase 6 (WORLD BUILDER):** ✅ **COMPLETE**
- ✅ Code compiles
- ✅ `inversion/world_builder.rs` created
- ✅ WorldBuilder struct with plan reference and cache
- ✅ build_worlds() method with lazy on-demand building
- ✅ Recursive world building for rule references
- ✅ Cross-product merging with automatic constraint pruning
- ✅ collect_rule_paths() helper for expression tree walking
- ✅ substitute_rule_path() helper for expression substitution
- ✅ extract_rule_references() helper

**Phase 7 (ENHANCED ISOLATION):** ✅ **COMPLETE**
- ✅ Code compiles
- ✅ `algebra/isolation.rs` extended with inversion structures
- ✅ InversionResult and InversionSolution structs added
- ✅ invert_expression() implemented for recursive expression inversion
- ✅ Supports arithmetic inversion (add, subtract, multiply, divide, power)
- ✅ Supports mathematical inversion (sqrt implemented, exp/log/trig require numerical impl)
- ✅ invert_arithmetic_left() and invert_arithmetic_right() helpers
- ✅ square_value(), exp_value(), log_value(), apply_inverse_trig() helpers
- ✅ evaluate_to_literal() helper for constant expression evaluation
- ✅ EvaluationContext::new_for_inversion() public method added
- ✅ Veto results converted to unsatisfiable domain restrictions (not errors)

**Phase 8 (WIRE UP):**
- ✅ All tests pass
- ✅ No stack overflow
- ✅ Memory O(n) not O(branches^depth)
- ✅ Fast inversion (symbolic eval + pre-optimized conditions)
- ✅ Clean module separation: `algebra/` for reasoning, `computation/` for runtime
- ✅ equation.rs deleted, optimization.rs added
- ✅ No equation substitution code exists
- ✅ Minimal search space via symbolic evaluation

---

## File Summary

**DELETED (Phase 1):** ✅
- `lemma/src/planning/equation.rs` (332 lines) - recursive substitution ✅
- `lemma/src/inversion/solver.rs` (2083 lines) - ENTIRE FILE (useful parts moved first) ✅
- Parts of `computation/simplification.rs` (~250 lines) - multi-branch optimizations ✅
- Parts of `execution_plan.rs` (~26 lines) - equation field and builder ✅
- Parts of `inversion/mod.rs` (~70 lines) - equation-based entry point ✅

**MOVED (Phase 1):** ✅
- `inversion/solver.rs` (lines 340-489) → `computation/constraints.rs` as `extract_constraints` (~150 lines) ✅
- `inversion/solver.rs` (lines 491-1011) → `algebra/isolation.rs` (~520 lines) ✅

**MOVED (Phase 2):** ✅
- `computation/expansion.rs` → `algebra/expansion.rs` (~698 lines) ✅
- `computation/simplification.rs` → `algebra/simplification.rs` (~448 lines after deletions) ✅
- `computation/constraints.rs` → `algebra/constraints.rs` (~980 lines) ✅

**ADDED (Phase 1):** ✅
- `lemma/src/algebra/mod.rs` (~10 lines) - NEW module ✅
- `lemma/src/algebra/isolation.rs` (~520 lines) - isolation functions moved from solver.rs ✅

**ADDED (Phase 2):** ✅
- `lemma/src/algebra/math_properties.rs` (~10 lines) - placeholder ✅

**ADDED (Phase 3):** ✅
- Parts of `execution_plan.rs` (~25 lines) - optimized_condition field + optimize() method ✅

**ADDED (Phase 4):** ✅
- `lemma/src/evaluation/expression.rs` (~20 lines) - EvaluationResult enum, symbolic mode handling ✅
- `lemma/src/evaluation/mod.rs` (~85 lines) - symbolic_mode, evaluate_symbolic() ✅

**ADDED (Phase 5):** ✅
- `lemma/src/algebra/constraints.rs` (~30 lines) - is_exact(), exact(), contains() methods, value_in_bounds() helper ✅
- `lemma/src/inversion/world.rs` (~57 lines) - World struct, merge() method ✅
- `lemma/src/inversion/mod.rs` - world module export added ✅

**ADDED (Phase 6):** ✅
- `lemma/src/inversion/world_builder.rs` (~238 lines) - WorldBuilder, build_worlds(), collect_rule_paths(), substitute_rule_path(), extract_rule_references() ✅
- `lemma/src/inversion/mod.rs` - world_builder module export added ✅

**ADDED (Phase 7):** ✅
- `lemma/src/algebra/isolation.rs` (~400 lines added) - InversionResult/InversionSolution structs, invert_expression(), arithmetic/mathematical inversion helpers ✅
- `lemma/src/evaluation/mod.rs` - EvaluationContext::new_for_inversion() public method added ✅

**TO ADD (Phase 8):**
- `lemma/src/inversion/mod.rs` (~150 lines modified) - new invert(), solve_world(), find_unknown_fact()

**IMPORT UPDATES (Phase 2):** ✅
- All files using `computation::expand` → `algebra::expand` ✅
- All files using `computation::simplification` → `algebra::simplification` ✅
- All files using `computation::ConstraintSet` → `algebra::ConstraintSet` ✅
- `planning/graph.rs` → uses `algebra::simplification` ✅
- `inversion/mod.rs` → uses `algebra::constraints` ✅
- `inversion/response.rs` → uses `algebra::constraints::FactConstraint` ✅
- `algebra/isolation.rs` → uses `algebra::constraints::UnsatReason` ✅
- `algebra/constraints.rs` → uses `algebra::expansion::reverse_comparison` ✅
- `computation/math_properties.rs` → uses `algebra::constraints` ✅
- No backward compatibility re-exports in `computation/mod.rs` ✅

**Net change:** 
- ~2761 lines deleted
- ~2956 lines reorganized
- ~625 lines new (evaluate_symbolic, world structure, world builder, enhanced isolation, optimization)
- **Total: +820 lines with massively cleaner architecture**

The increase provides:
- **Symbolic evaluation** - reduces N-dimensional problems to 1D (critical optimization)
- **Non-linear math support** - sqrt, pow, trig inversion
- **World-based solving** - linear complexity instead of exponential
- Clean separation: `algebra/` (reasoning) vs `computation/` (runtime) vs `evaluation/` (execution)
- No equation-based recursion


