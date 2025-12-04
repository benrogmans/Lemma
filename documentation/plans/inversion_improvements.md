# Inversion Implementation Improvements Plan

This document outlines the plan to address engineering concerns and improve the inversion implementation.

## Priority 1: High Impact, Low Risk

### 1.1 Extract Rule Normalization Logic (Task: inv-1)
**File**: `lemma/src/inversion/expansion.rs`  
**Complexity**: Medium  
**Impact**: High (readability, maintainability)

**Current Issue**: The "last wins" semantics logic (lines 451-523) is deeply nested and mixes rule-level normalization with expression expansion.

**Architectural Insight**: When encountering a `RulePath`, we should normalize the entire rule (all branches together) to apply precedence semantics, rather than normalizing individual branch conditions in isolation.

**Implementation**:
1. Rename `expand_expression_to_branches()` to `normalize_expression()` - handles general expression normalization

2. Create new function `normalize_rule()` that handles rule-level normalization:
   ```rust
   /// Normalize a rule by expanding all branches and applying precedence semantics.
   ///
   /// In Lemma, when multiple `unless` clauses exist, the last matching
   /// branch takes precedence. This function:
   /// 1. Expands all rule branches (recursively handling nested rule references)
   /// 2. Applies "last wins" semantics to each branch's condition
   /// 3. Returns normalized (condition, result) branch pairs
   ///
   /// # Example
   /// For a rule: `x = 0 unless a > 10 then 1 unless a > 20 then 2`
   /// Returns:
   /// - Branch 0: (NOT(a > 10) AND NOT(a > 20), 0)
   /// - Branch 1: ((a > 10) AND NOT(a > 20), 1)
   /// - Branch 2: ((a > 20), 2)
   ///
   /// # Arguments
   /// * `rule_path` - The rule to normalize
   /// * `graph` - Rule graph containing all rules
   /// * `plan` - Execution plan for hydration
   /// * `provided_facts` - Facts that are given (will be hydrated)
   ///
   /// # Returns
   /// Vector of normalized (condition, result) branch pairs
   fn normalize_rule(
       rule_path: &RulePath,
       graph: &Graph,
       plan: &ExecutionPlan,
       provided_facts: &HashSet<FactPath>,
   ) -> LemmaResult<Vec<(Expression, Expression)>>
   ```

3. Create helper function `normalize_condition()` for normalizing individual conditions:
   ```rust
   /// Normalize a condition expression by expanding rule references.
   ///
   /// This is a helper that recursively expands rule references in conditions.
   /// When a rule reference is encountered, it calls `normalize_rule()` to get
   /// the normalized rule branches.
   ///
   /// # Arguments
   /// * `condition` - The condition expression to normalize
   /// * `graph` - Rule graph for expansion
   /// * `plan` - Execution plan for hydration
   /// * `provided_facts` - Facts that are given
   ///
   /// # Returns
   /// Normalized condition expression with all rule references expanded
   fn normalize_condition(
       condition: Expression,
       graph: &Graph,
       plan: &ExecutionPlan,
       provided_facts: &HashSet<FactPath>,
   ) -> LemmaResult<Expression>
   ```

4. Refactor `normalize_expression()` (formerly `expand_expression_to_branches`):
   - When encountering `RulePath`, call `normalize_rule()` instead of inline logic
   - For other expression types, continue current expansion approach
   - Use `normalize_condition()` for condition normalization

**Testing**: Add test cases for:
- Simple last-wins (2 branches)
- Nested rule references in conditions
- Rules with no unless clauses
- Rules where later branches reference other rules

---

### 1.2 Improve Error Messages (Task: inv-6)
**File**: `lemma/src/inversion/mod.rs`, `lemma/src/inversion/collapse.rs`  
**Complexity**: Low  
**Impact**: High (developer experience)

**Implementation**:
1. Enhance `build_no_solution_error()` to include:
   - Which branches were considered
   - Why each branch was filtered out (if possible)
   - Sample of unsatisfiable conditions

2. Improve `shape_to_domains()` error message:
   ```rust
   // Current:
   "No valid solutions: all {} branch constraint(s) are unsatisfiable"
   
   // Improved:
   "No valid solutions: all {} branch constraint(s) are unsatisfiable.
   Branches checked:
   {}
   Common issues: contradictory constraints, empty domains after simplification"
   ```

3. Add helper function to format branch conditions for error messages

**Testing**: Verify error messages are helpful in test cases that trigger them

---

### 1.3 Refactor `invert()` Function (Task: inv-4)
**File**: `lemma/src/inversion/mod.rs`  
**Complexity**: Medium  
**Impact**: Medium (maintainability)

**Implementation**:
Split `invert()` into focused functions:

1. `expand_rule_branches()` - Handles expansion phase (lines 130-174)
2. `apply_last_wins_to_branches()` - Applies last-wins semantics to expanded branches (lines 180-189)
3. `process_and_filter_branches()` - Hydration, simplification, filtering (lines 185-248)
4. `build_final_shape()` - Unification and free variable collection (lines 258-273)

Each function should:
- Have clear doc comments
- Return `LemmaResult` for error handling
- Be testable in isolation

**Testing**: Ensure existing tests still pass, add unit tests for each extracted function

---

## Priority 2: High Impact, Medium Risk

### 2.1 Early Pruning During Expansion (Task: inv-2)
**File**: `lemma/src/inversion/expansion.rs`  
**Complexity**: High  
**Impact**: Very High (performance)

**Current Issue**: Cross-product explosion creates many branches that are later filtered out.

**Implementation Strategy**:
1. Add `prune_unsatisfiable_branches()` function that:
   - Takes a list of (condition, result) branch pairs
   - Attempts early simplification of conditions
   - Filters branches with `false` conditions before cross-product
   - Uses lightweight checks (constant folding, simple boolean evaluation)

2. Integrate pruning at key points:
   - After `expand_expression_to_branches()` returns
   - Before creating cross-products in arithmetic/comparison expansion
   - After each recursive expansion level

3. Add configurable limit: `max_branches_before_pruning` in `ResourceLimits`

**Implementation Details**:
```rust
/// Prune branches with unsatisfiable conditions early.
///
/// This performs lightweight checks to filter out branches that
/// can never be satisfied, reducing the size of cross-products.
///
/// # Limitations
/// - Only performs constant folding and simple boolean checks
/// - May not catch all unsatisfiable cases (conservative)
/// - Does not perform full algebraic solving
fn prune_unsatisfiable_branches(
    branches: Vec<(Expression, Expression)>,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> Vec<(Expression, Expression)>
```

**Testing**:
- Test with rules that create large cross-products
- Verify pruning doesn't remove valid branches
- Measure performance improvement on complex cases

**Risk Mitigation**:
- Start conservative (only prune obviously false conditions)
- Add flag to disable pruning for debugging
- Extensive testing to ensure no valid branches are removed

---

### 2.2 Integrate Factoring into Algebraic Solver (Task: inv-3)
**File**: `lemma/src/inversion/solver.rs`  
**Complexity**: Medium  
**Impact**: High (capability)

**Current Issue**: `try_factor_common_term()` exists but isn't used in main solver path.

**Implementation**:
1. Modify `algebraic_solve()` to attempt factoring before giving up:
   ```rust
   ExpressionKind::Arithmetic(l, op, r) => {
       let l_contains = contains_unknown(l, unknown, fact_matcher);
       let r_contains = contains_unknown(r, unknown, fact_matcher);
       
       if l_contains && r_contains {
           // Try factoring before giving up
           if let Some(factored) = try_factor_common_term(expr, unknown, fact_matcher) {
               return algebraic_solve(&factored, unknown, target, fact_matcher);
           }
           // ... existing error handling
       }
       // ... rest of existing logic
   }
   ```

2. Add test cases for:
   - `(x * a) - (x * b)` → `x * (a - b)`
   - `(x * a) + (x * b)` → `x * (a + b)`
   - Cases where factoring isn't possible

**Testing**: Add comprehensive tests for factorable expressions

---

## Priority 3: Medium Impact, Low Risk

### 3.1 Configurable BDD Limit (Task: inv-7)
**File**: `lemma/src/inversion/solver.rs`, `lemma/src/limits.rs`  
**Complexity**: Low  
**Impact**: Medium (flexibility)

**Implementation**:
1. Add to `ResourceLimits`:
   ```rust
   /// Maximum number of boolean atoms for BDD simplification
   /// Default: 64 (current hard limit)
   /// Set to 0 to disable BDD simplification
   pub max_bdd_atoms: usize,
   ```

2. Update `simplify_boolean()` to use limit from plan/limits
3. Add graceful degradation message when limit exceeded

**Testing**: Test with various limits, verify graceful degradation

---

### 3.2 Add Metrics/Instrumentation (Task: inv-5)
**File**: `lemma/src/inversion/mod.rs`  
**Complexity**: Low  
**Impact**: Medium (observability)

**Implementation**:
1. Create `InversionMetrics` struct:
   ```rust
   #[derive(Debug, Default)]
   pub struct InversionMetrics {
       pub initial_branch_count: usize,
       pub after_expansion_count: usize,
       pub after_pruning_count: usize,
       pub after_filtering_count: usize,
       pub max_expansion_depth: usize,
       pub cross_product_sizes: Vec<usize>,
   }
   ```

2. Pass metrics through inversion pipeline (optional, behind feature flag)
3. Log or return metrics for debugging/optimization

**Testing**: Verify metrics are accurate, test with feature flag disabled

---

## Priority 4: Low Priority (Future Consideration)

### 4.1 Expression Sharing Optimization (Task: inv-8)
**File**: Multiple files  
**Complexity**: Very High  
**Impact**: Low-Medium (memory efficiency)

**Current Issue**: Frequent cloning of expressions in deep nesting.

**Implementation** (Future):
- Consider `Arc<Expression>` for shared sub-expressions
- Requires careful analysis of mutation points
- May need expression interning/hashing
- Benchmark to verify memory savings justify complexity

**Decision**: Defer until profiling shows memory issues

---

## Implementation Order

### Phase 1 (Quick Wins - 1-2 days)
1. ✅ Extract last-wins logic (1.1)
2. ✅ Improve error messages (1.2)
3. ✅ Refactor `invert()` function (1.3)

### Phase 2 (Performance - 3-5 days)
4. ✅ Early pruning (2.1) - Most critical
5. ✅ Integrate factoring (2.2)

### Phase 3 (Polish - 1-2 days)
6. ✅ Configurable BDD limit (3.1)
7. ✅ Add metrics (3.2)

### Phase 4 (Future)
8. ⏸️ Expression sharing (4.1) - Defer until needed

---

## Testing Strategy

For each improvement:
1. **Unit Tests**: Test extracted functions in isolation
2. **Integration Tests**: Verify existing inversion tests still pass
3. **Performance Tests**: Measure improvement for complex cases
4. **Regression Tests**: Ensure no valid branches are incorrectly filtered

## Risk Assessment

| Task | Risk Level | Mitigation |
|------|-----------|------------|
| 1.1 Last-wins extraction | Low | Well-isolated, extensive tests |
| 1.2 Error messages | Low | Non-breaking change |
| 1.3 Refactor | Medium | Ensure all tests pass |
| 2.1 Early pruning | High | Conservative approach, extensive testing |
| 2.2 Factoring | Low | Already tested function, just integration |
| 3.1 BDD limit | Low | Simple config change |
| 3.2 Metrics | Low | Optional feature |
| 4.1 Expression sharing | Very High | Defer until needed |

## Success Criteria

- ✅ All existing tests pass
- ✅ No performance regressions
- ✅ Measurable improvement in complex cases (2x+ reduction in branches)
- ✅ Code is more maintainable (lower cyclomatic complexity)
- ✅ Better error messages help debugging
- ✅ Documentation is clear and complete

