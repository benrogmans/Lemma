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
- Planning-time branch optimization added
- Branches have optimized_condition field
- optimize_branches called during planning

**Phase 4: COMPLETE** ✅
- Symbolic evaluation added to Evaluator
- Partial evaluation with unknown facts
- Branch pruning (false conditions and last-wins optimization)
- Returns reduced ExecutionPlan

**Next: Phase 5** - Add World structure

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
9. **Add** planning-time branch optimization - **Phase 3** ✅
10. **Add** symbolic evaluation (critical optimization) - **Phase 4**
11. **Add** path structure with full Expression support - **Phase 5**
12. **Add** path builder with symbolic eval integration - **Phase 6**
13. **Enhance** algebraic isolation for non-linear math - **Phase 7**
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
- **Phase 3**: ADD planning-time branch optimization ✅ **COMPLETE**
- **Phase 4**: ADD symbolic evaluation (substitute knowns, prune branches) ✅ **COMPLETE**
- **Phase 5**: ADD world structure (Expression-based values)
- **Phase 6**: ADD world builder (with symbolic eval integration)
- **Phase 7**: ENHANCE algebraic isolation (non-linear inversion)
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
algebra/                    ← NEW: Mathematical reasoning tools (engine-level)
  ├── mod.rs
  ├── expansion.rs          ← Move from computation/ (DNF, distribution)
  ├── simplification.rs     ← Move from computation/ (contradiction, folding)
  ├── constraints.rs        ← Move from computation/ (solution spaces)
  ├── isolation.rs          ← Extract from inversion/solver.rs (equation solving)
  └── math_properties.rs    ← NEW: Algebraic identities, commutativity
computation/                ← Runtime evaluation only (kept for future)
  ├── arithmetic.rs         ← Evaluate +, -, *, / (existing)
  ├── comparison.rs         ← Evaluate >, <, == (existing)
  └── mod.rs
evaluation/                 ← Execution of plans with full or symbolic facts
  ├── mod.rs                ← MODIFY: add symbolic_mode to EvaluationContext, evaluate_symbolic() (Phase 4)
  ├── expression.rs         ← MODIFY: handle unknown facts in symbolic mode (Phase 4)
  └── operations.rs         ← Existing: arithmetic/comparison operations
inversion/
  ├── world.rs              ← NEW: World structure (Phase 5)
  └── world_builder.rs      ← NEW: On-demand world building (Phase 6)
planning/
  └── optimization.rs       ← NEW: Per-branch optimization (Phase 3)
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

```rust
//! Per-branch optimization during planning
//!
//! Prepares branches for fast inversion by:
//! 1. Expanding conditions to DNF
//! 2. Simplifying (contradiction detection, constant folding)
//! 3. Storing optimized conditions
//!
//! This happens ONCE during document loading, not during every inversion query.

use crate::algebra::{expand, simplification};  // ← Note: algebra, not computation
use crate::semantic::Expression;
use super::execution_plan::Branch;

/// Optimize all branches for a rule during planning
///
/// Expands + simplifies each branch condition for fast inversion runtime.
/// No cross-rule substitution - just local per-branch optimization.
pub fn optimize_branches(branches: &mut [Branch]) {
    for branch in branches {
        // Expand condition to DNF
        let expanded = expand(branch.condition.clone());
        
        // Simplify (detect contradictions, fold constants, remove redundancies)
        let simplified = simplification::reduce(expanded);
        
        // Store for fast inversion runtime
        branch.optimized_condition = Some(simplified);
    }
}
```

**File: `lemma/src/planning/mod.rs`**

```rust
// ADD:
pub mod optimization;
```

**File: `lemma/src/planning/execution_plan.rs`**

Add import near line 7:
```rust
use crate::planning::optimization;
```

Modify Branch struct (lines 75-84):
```rust
pub struct Branch {
    /// Condition expression (always present, explicit with last-wins semantics applied)
    pub condition: Expression,
    
    /// Pre-optimized condition (expanded + simplified during planning)
    /// Used by inversion for fast constraint extraction
    pub optimized_condition: Option<Expression>,  // NEW
    
    /// Result expression
    pub result: Expression,
    
    /// Source location for error messages
    pub source: Option<Source>,
}
```

In build() function (after line 129), add:
```rust
// Optimize branches for fast inversion runtime
for rule in &mut executable_rules {
    optimization::optimize_branches(&mut rule.branches);
}
```

In test code, update all Branch constructors (lines ~538, 722, 732, 747, 840, 918):
```rust
Branch {
    condition: ...,
    optimized_condition: None,  // Tests don't need pre-optimization
    result: ...,
    source: ...,
}
```

### Phase 4: Add Symbolic Evaluation (Critical Optimization)

This is the most important optimization - reduces search space before world building by partially
evaluating with known facts. Transforms 50 states × 4 statuses = 200 paths → 1 path when state/status known.

**Key Design Decision:** Reuse existing evaluation infrastructure with a `symbolic_mode` flag instead
of reimplementing evaluation logic. This is partial evaluation - evaluate what you can (known facts),
leave what you can't (unknown facts) symbolic.

#### Changes:

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

**File: `lemma/src/evaluation/expression.rs`** (MODIFY `evaluate_expression`)

Find the `ExpressionKind::FactPath` match arm and modify to handle symbolic mode:

```rust
ExpressionKind::FactPath(path) => {
    if let Some(value) = context.get_fact(path) {
        // Fact is known - substitute with literal value
        Ok(Expression::new(
            ExpressionKind::Literal(value.clone()),
            expr.source.clone(),
        ))
    } else if context.is_symbolic() {
        // Symbolic mode: return original expression for unknown facts
        Ok(expr.clone())
    } else {
        // Normal mode: error on unknown facts
        Err(LemmaError::Engine(format!(
            "Fact not found: {}",
            path
        )))
    }
}
```

**File: `lemma/src/evaluation/mod.rs`** (ADD method to `Evaluator` impl)

Add after the `evaluate` method (around line 130):

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
- Reuses ALL existing evaluation logic (arithmetic, comparisons, boolean ops, etc.)
- Single flag change (`symbolic_mode`) enables partial evaluation
- Aggressive pruning: removes false branches AND unreachable earlier branches
- Natural integration with existing evaluation infrastructure

### Phase 5: Add World Structure

**File: `lemma/src/inversion/world.rs`** (NEW)

Simplified - value is just an Expression (supports non-linear math).

```rust
use std::collections::HashMap;
use crate::semantic::{Expression, FactPath};
use crate::computation::FactConstraint;

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
        
        // Combine values algebraically
        let new_value = combine_values(&self.value, &other.value);
        
        Some(World {
            constraints: new_constraints,
            value: new_value,
        })
    }
    
    /// Evaluate world at specific fact value (for minimization)
    pub fn eval_at(&self, fact: &FactPath, value: &LiteralValue) -> Option<LiteralValue> {
        // Substitute fact value and evaluate expression
        let mut substitution = HashMap::new();
        substitution.insert(fact.clone(), value.clone());
        
        // Use symbolic evaluation to substitute and fold
        let evaluated = crate::evaluation::symbolic(&self.value, &substitution);
        
        // Extract literal if fully evaluated
        if let ExpressionKind::Literal(lit) = &evaluated.kind {
            Some(lit.clone())
        } else {
            None
        }
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
    /// Create WorldBuilder with pre-reduced plan
    /// 
    /// The plan should have had `Evaluator::evaluate_symbolic()` called on it
    /// to substitute known facts and prune impossible branches.
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
            // Branch condition already symbolically evaluated and stored in optimized_condition
            let condition = branch.optimized_condition.as_ref().unwrap_or(&branch.condition);
            let result = &branch.result; // Already symbolically evaluated
            
            // If condition is literal true, result applies unconditionally
            if matches!(&condition.kind, 
                ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True))) {
                worlds.push(World {
                    constraints: HashMap::new(),
                    value: result.clone(),
                });
                continue;
            }
            
            // Extract constraints from condition
            let mut constraint_set = ConstraintSet::new();
            extract_constraints(condition, &mut constraint_set);
            let constraints = constraint_set.to_fact_constraints();
            
            // 6. Check if result references other rules
            let rule_refs = extract_rule_references(&simplified_result);
            
            if rule_refs.is_empty() {
                // Simple case: no rule dependencies in result
                worlds.push(World {
                    constraints,
                    value: simplified_result,
                });
            } else {
                // Complex case: recursively build referenced rule worlds
                let branch_worlds = self.build_with_references(
                    constraints,
                    &simplified_result,
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
fn is_literal_true(expr: &Expression) -> bool {
    matches!(&expr.kind, ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)))
}

fn is_literal_false(expr: &Expression) -> bool {
    matches!(&expr.kind, ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)))
}

fn extract_rule_references(expr: &Expression) -> Vec<RuleReference> {
    todo!("Extract all rule references from expression tree")
}

fn substitute_rule_reference(expr: &Expression, rule_path: &RulePath, value: &Expression) -> Expression {
    todo!("Recursively replace RulePath(rule_path) with value in expr")
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

/// Recursively invert an expression to solve for a target fact
/// Example: solve sqrt(income) = 500 for income
///          -> income = 500^2 = 250000
pub fn invert_expression(
    expr: &Expression,
    target_fact: &FactPath,
    target_value: &LiteralValue,
) -> Result<LiteralValue, InversionError> {
    match &expr.kind {
        // Base case: found the target fact
        ExpressionKind::FactPath(path) if path == target_fact => {
            Ok(target_value.clone())
        }
        
        // Arithmetic: y = x + C => x = y - C
        ExpressionKind::Arithmetic(left, op, right) => {
            use ArithmeticComputation::*;
            
            let left_has_fact = contains_fact(left, target_fact);
            let right_has_fact = contains_fact(right, target_fact);
            
            // Can only invert if fact appears on one side only
            if left_has_fact && !right_has_fact {
                // Isolate from left: y = x op C => x = ...
                let right_value = evaluate_to_literal(right)?;
                let new_target = invert_arithmetic_left(target_value, *op, &right_value)?;
                invert_expression(left, target_fact, &new_target)
            } else if !left_has_fact && right_has_fact {
                // Isolate from right: y = C op x => x = ...
                let left_value = evaluate_to_literal(left)?;
                let new_target = invert_arithmetic_right(&left_value, *op, target_value)?;
                invert_expression(right, target_fact, &new_target)
            } else {
                Err(InversionError::MultipleOccurrences)
            }
        }
        
        // Non-linear: y = sqrt(x) => x = y^2
        ExpressionKind::MathematicalComputation(op, inner) => {
            use MathematicalComputation::*;
            
            let new_target = match op {
                Sqrt => {
                    // y = sqrt(x) => x = y^2
                    square_value(target_value)?
                }
                Sin | Cos | Tan => {
                    // Inverse trig functions
                    apply_inverse_trig(*op, target_value)?
                }
                Log => {
                    // y = log(x) => x = e^y
                    exp_value(target_value)?
                }
                Exp => {
                    // y = e^x => x = log(y)
                    log_value(target_value)?
                }
                Abs => {
                    // y = |x| => x = ±y (ambiguous - need constraints)
                    return Err(InversionError::AmbiguousInversion);
                }
                _ => return Err(InversionError::UnsupportedOperation),
            };
            
            invert_expression(inner, target_fact, &new_target)
        }
        
        // Comparison: already handled by try_isolate_comparison
        ExpressionKind::Comparison(_, _, _) => {
            Err(InversionError::ComparisonNotInvertible)
        }
        
        // Cannot invert through logical operations
        ExpressionKind::LogicalAnd(_, _) | ExpressionKind::LogicalOr(_, _) => {
            Err(InversionError::LogicalNotInvertible)
        }
        
        // Literal doesn't contain the fact
        ExpressionKind::Literal(_) => {
            Err(InversionError::FactNotFound)
        }
        
        _ => Err(InversionError::UnsupportedExpression),
    }
}

// Arithmetic inversion helpers
fn invert_arithmetic_left(
    target: &LiteralValue,
    op: ArithmeticComputation,
    right: &LiteralValue,
) -> Result<LiteralValue, InversionError> {
    use ArithmeticComputation::*;
    
    match op {
        Add => subtract_values(target, right),      // y = x + C => x = y - C
        Subtract => add_values(target, right),       // y = x - C => x = y + C
        Multiply => divide_values(target, right),    // y = x * C => x = y / C
        Divide => multiply_values(target, right),    // y = x / C => x = y * C
        Power => root_values(target, right),         // y = x ^ C => x = y ^ (1/C)
        _ => Err(InversionError::UnsupportedOperation),
    }
}

fn invert_arithmetic_right(
    left: &LiteralValue,
    op: ArithmeticComputation,
    target: &LiteralValue,
) -> Result<LiteralValue, InversionError> {
    use ArithmeticComputation::*;
    
    match op {
        Add => subtract_values(target, left),        // y = C + x => x = y - C
        Subtract => subtract_values(left, target),   // y = C - x => x = C - y
        Multiply => divide_values(target, left),     // y = C * x => x = y / C
        Divide => divide_values(left, target),       // y = C / x => x = C / y
        Power => log_base_values(target, left),      // y = C ^ x => x = log_C(y)
        _ => Err(InversionError::UnsupportedOperation),
    }
}

// Mathematical operation helpers
fn square_value(value: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Square a numeric literal")
}

fn exp_value(value: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Compute e^value")
}

fn log_value(value: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Compute natural log")
}

fn apply_inverse_trig(op: MathematicalComputation, value: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Apply asin/acos/atan")
}

// Arithmetic helpers
fn add_values(a: &LiteralValue, b: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Add two literals")
}

fn subtract_values(a: &LiteralValue, b: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Subtract two literals")
}

fn multiply_values(a: &LiteralValue, b: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Multiply two literals")
}

fn divide_values(a: &LiteralValue, b: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Divide two literals")
}

fn root_values(base: &LiteralValue, exp: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Compute base^(1/exp)")
}

fn log_base_values(value: &LiteralValue, base: &LiteralValue) -> Result<LiteralValue, InversionError> {
    todo!("Compute log_base(value)")
}

fn evaluate_to_literal(expr: &Expression) -> Result<LiteralValue, InversionError> {
    todo!("Evaluate expression that should be constant")
}

#[derive(Debug)]
pub enum InversionError {
    FactNotFound,
    MultipleOccurrences,
    UnsupportedOperation,
    UnsupportedExpression,
    ComparisonNotInvertible,
    LogicalNotInvertible,
    AmbiguousInversion,
    DivisionByZero,
    InvalidValue,
}
```

This gives us:
- **Linear inversion**: Add, Subtract, Multiply, Divide (existing)
- **Non-linear inversion**: Sqrt, Pow, Log, Exp, Trig functions (NEW)
- **Fallback**: When symbolic inversion fails, can fall back to numerical methods (bisection)

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
    outcome: Option<OperationResult>,
    provided_facts: &HashMap<String, String>,  // User-provided known facts
) -> LemmaResult<InversionResponse> {
    let target = Target::from_str(operator, outcome)?;
    
    // 1. Convert provided_facts to FactPath -> LiteralValue map
    let known_facts = parse_provided_facts(provided_facts, plan)?;
    
    // 2. Build worlds on-demand with symbolic evaluation
    let mut builder = WorldBuilder::new(plan, known_facts);
    let rule_worlds = builder.build_worlds(rule_name)?;
    
    // 3. Filter worlds matching target
    let matching_worlds: Vec<&World> = rule_worlds.iter()
        .filter(|w| matches_target(&w.value, &target))
        .collect();
    
    // 4. For each world, solve algebraically
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
            
            // Use algebraic inversion (handles linear + non-linear)
            match invert_expression(&world.value, &unknown_fact, &target_value) {
                Ok(solved_value) => {
                    // Verify solution satisfies world constraints
                    if world.constraints.get(&unknown_fact).map_or(true, |c| c.contains(&solved_value)) {
                        let mut solution_constraints = world.constraints.clone();
                        solution_constraints.insert(
                            unknown_fact,
                            FactConstraint::exact(solved_value.clone())
                        );
                        vec![Solution::new(
                            OperationResult::Value(target_value),
                            solution_constraints
                        )]
                    } else {
                        vec![] // Solution outside valid range
                    }
                }
                Err(_) => {
                    // Fallback: If symbolic inversion fails, try numerical methods
                    // (bisection, Newton-Raphson, etc.)
                    vec![]
                }
            }
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

**Phase 3 (PLANNING OPTIMIZATION):** ✅ **COMPLETE**
- ✅ `planning/optimization.rs` created with `optimize_branches` function
- ✅ `pub mod optimization` added to `planning/mod.rs`
- ✅ Branches have `optimized_condition: Option<Expression>` field
- ✅ `optimization::optimize_branches` called during `build_execution_plan`
- ✅ All Branch constructors in tests updated with `optimized_condition: None`
- ✅ Branch constructors in `graph.rs` and `inversion/mod.rs` updated

**Phase 4 (SYMBOLIC EVALUATION):** ✅ **COMPLETE**
- ✅ Code compiles
- ✅ `EvaluationResult` enum added (Evaluated/Symbolic) - no error-as-control-flow
- ✅ `symbolic_mode: bool` field added to `EvaluationContext`
- ✅ `new_symbolic()` constructor and `is_symbolic()` method added to `EvaluationContext`
- ✅ `expression::evaluate_expression` returns `EvaluationResult` and handles unknown facts in symbolic mode
- ✅ `evaluate_mathematical_operator` and `propagate_veto_proof` updated to return `EvaluationResult`
- ✅ `Evaluator::evaluate_symbolic()` method added with `evaluate_to_expression` helper
- ✅ Can partially evaluate with known facts, leave unknown facts symbolic
- ✅ Prunes branches that evaluate to false
- ✅ Prunes earlier branches when one becomes unconditionally true (last-wins optimization)
- ✅ Returns reduced ExecutionPlan with simplified conditions
- ✅ `ExecutionPlan::optimize()` method added (inlined from Phase 3)
- ✅ Phase 3 optimization removed from planning (now called after symbolic evaluation for inversion only)

**Phase 5 (WORLD STRUCTURE):**
- ✅ Code compiles
- ✅ `inversion/world.rs` created
- ✅ World uses full Expression (not limited Value enum)

**Phase 6 (WORLD BUILDER):**
- ✅ Code compiles
- ✅ `inversion/world_builder.rs` created
- ✅ WorldBuilder applies symbolic evaluation first

**Phase 7 (ENHANCED ISOLATION):**
- ✅ Code compiles
- ✅ `algebra/isolation.rs` extended with `invert_expression`
- ✅ Supports non-linear inversion (sqrt, pow, trig)

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
- `lemma/src/planning/optimization.rs` (~30 lines) - per-branch optimization ✅
- Parts of `execution_plan.rs` (~10 lines) - optimized_condition field ✅

**ADDED (Future phases):**
- `lemma/src/evaluation/mod.rs` (~85 lines added) - symbolic_mode, evaluate_symbolic() (Phase 4)
- `lemma/src/evaluation/expression.rs` (~10 lines modified) - handle unknown facts in symbolic mode (Phase 4)
- `lemma/src/inversion/world.rs` (~80 lines) - World structure with Expression (Phase 5)
- `lemma/src/inversion/world_builder.rs` (~150 lines) - world building from reduced plan (Phase 6)
- Enhanced `algebra/isolation.rs` (~300 lines added) - non-linear inversion (Phase 7)

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


