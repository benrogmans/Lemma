# Refactoring `invert()` Function - Detailed Plan

## Overview

Refactor the `invert()` function to improve maintainability, testability, and clarity by breaking it into focused, well-documented functions that leverage the improved `ExecutionPlan` with normalized branches.

## Current State Analysis

### Current Function Structure (182 lines)
The `invert()` function currently performs these operations in sequence:

1. **Rule Lookup** (lines 42-48)
   - Get `ExecutableRule` from plan
   - Extract rule path string for error messages
   - Build inversion branches (now simplified to direct mapping)

2. **Expansion Phase** (lines 50-92)
   - Expand rule references in conditions and results
   - Create cross-products of condition and result branches
   - Handle both `RulePath` and non-`RulePath` expressions

3. **Processing Phase** (lines 97-156)
   - Hydrate expressions (substitute provided facts)
   - Simplify via constant folding
   - Extract outcomes (Value or Veto)
   - Filter branches based on target
   - Collect available outcomes for error messages

4. **Error Handling** (lines 158-164)
   - Check if any branches matched
   - Build error message with available outcomes

5. **Unification Phase** (lines 166-176)
   - Unify branches with same outcome (unless `Target::any_value()`)

6. **Undetermined Facts Collection** (lines 178-179)
   - Collect undetermined facts from all branches
   - Remove provided facts

7. **Shape Construction** (line 181)
   - Create final `Shape` with unified branches and undetermined facts

### Key Improvements from Normalized Branches

- **No redundant normalization**: Branches already have explicit conditions with last-wins semantics
- **Simpler branch extraction**: Direct mapping from `ExecutableRule.branches`
- **Cleaner expansion**: Can use normalized branches directly from `ExecutableRule`

## Refactoring Plan

### Phase 1: Extract Expansion Logic

#### Function: `expand_rule_branches()`

**Purpose**: Expand all rule references in a rule's branches, creating cross-products of condition and result branches.

**Signature**:
```rust
/// Expand rule references in all branches of an executable rule.
///
/// Takes normalized branches from `ExecutableRule` and expands any `RulePath` nodes
/// in both conditions and results. Creates cross-products of expanded branches.
///
/// # Arguments
/// * `executable_rule` - The rule whose branches to expand
/// * `plan` - Execution plan for rule lookups and hydration
/// * `provided_facts` - Facts that are given (will be hydrated during expansion)
///
/// # Returns
/// * `Ok(Vec<(Expression, Expression)>)` - Vector of (condition, result) pairs after expansion
/// * `Err(LemmaError)` - If rule expansion fails
///
/// # Example
/// For a rule with branches:
/// - Branch 0: (NOT(cond1) AND NOT(cond2), result0)
/// - Branch 1: (cond1 AND NOT(cond2), result1)
///
/// If result1 contains `RulePath("other_rule")`, this function expands that reference
/// and creates cross-products with all branches from `other_rule`.
fn expand_rule_branches(
    executable_rule: &ExecutableRule,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Vec<(Expression, Expression)>>
```

**Implementation Details**:
- Extract lines 48-92 from `invert()`
- Use `build_inversion_branches()` internally (or inline if simple)
- Handle cross-product creation for condition and result branches
- Leverage normalized branches (no need to handle optional conditions)

**Testing**:
- Test with rule containing no `RulePath` nodes
- Test with rule containing `RulePath` in result only
- Test with rule containing `RulePath` in condition only
- Test with rule containing `RulePath` in both
- Test with nested rule references
- Test cross-product creation

---

### Phase 2: Extract Processing Logic

#### Function: `process_expanded_branches()`

**Purpose**: Hydrate, simplify, and extract outcomes from expanded branches, then filter based on target.

**Signature**:
```rust
/// Process expanded branches: hydrate, simplify, and filter based on target.
///
/// Takes expanded (condition, result) pairs and:
/// 1. Hydrates expressions (substitutes provided facts)
/// 2. Simplifies via constant folding
/// 3. Extracts outcomes (Value or Veto)
/// 4. Filters branches that match the target
///
/// # Arguments
/// * `expanded_branches` - Branches after rule reference expansion
/// * `target` - Target outcome to filter for
/// * `plan` - Execution plan for hydration
/// * `provided_facts` - Facts that are given
///
/// # Returns
/// * `Ok((Vec<ShapeBranch>, Vec<String>))` - (filtered branches, available outcomes for errors)
/// * `Err(LemmaError)` - If processing fails
fn process_expanded_branches(
    expanded_branches: Vec<(Expression, Expression)>,
    target: &Target,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<(Vec<ShapeBranch>, Vec<String>)>
```

**Implementation Details**:
- Extract lines 97-156 from `invert()`
- Split into helper functions:
  - `hydrate_and_simplify_condition()` - Lines 101-102
  - `extract_outcome()` - Lines 104-130
  - `format_outcome_description()` - Lines 133-143
  - `filter_branch()` - Already exists, keep as-is

**Helper Functions**:

##### `hydrate_and_simplify_condition()`
```rust
/// Hydrate and simplify a condition expression.
///
/// Substitutes provided facts and performs constant folding.
fn hydrate_and_simplify_condition(
    condition: &Expression,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> Expression
```

##### `extract_outcome()`
```rust
/// Extract outcome from a result expression.
///
/// Returns `BranchOutcome::Veto` for veto expressions, `BranchOutcome::Value` otherwise.
/// Validates that no `RulePath` nodes remain (defensive check).
///
/// # Errors
/// Returns error if `RulePath` nodes are found (indicates expansion bug).
fn extract_outcome(
    result: &Expression,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<BranchOutcome>
```

##### `format_outcome_description()`
```rust
/// Format an outcome for error messages.
///
/// Creates human-readable description of an outcome for use in error messages.
fn format_outcome_description(outcome: &BranchOutcome) -> String
```

**Testing**:
- Test hydration with various fact types
- Test constant folding simplification
- Test outcome extraction for Value and Veto
- Test filtering with different target types
- Test error handling for remaining `RulePath` nodes

---

### Phase 3: Extract Unification Logic

#### Function: `unify_branches_if_needed()`

**Purpose**: Unify branches with same outcome, unless target is `any_value()`.

**Signature**:
```rust
/// Unify branches with the same outcome, if appropriate.
///
/// For `Target::any_value()`, preserves all branches to show distinct combinations.
/// For specific targets, unifies branches with identical outcomes using OR.
///
/// # Arguments
/// * `branches` - Branches to potentially unify
/// * `target` - Target to determine if unification should occur
///
/// # Returns
/// Unified branches (or original if `any_value()` target)
fn unify_branches_if_needed(
    branches: Vec<ShapeBranch>,
    target: &Target,
) -> Vec<ShapeBranch>
```

**Implementation Details**:
- Extract lines 166-176 from `invert()`
- Use existing `unify_branches()` function
- Simple wrapper that checks target type

**Testing**:
- Test with `Target::any_value()` (no unification)
- Test with specific target (unification occurs)
- Test with multiple branches having same outcome
- Test with all branches having different outcomes

---

### Phase 4: Extract Undetermined Facts Collection

#### Function: `collect_undetermined_facts()`

**Purpose**: Collect undetermined facts from branches, excluding provided facts.

**Signature**:
```rust
/// Collect undetermined facts from shape branches.
///
/// Extracts all fact paths referenced in branch conditions and outcomes,
/// then removes provided facts (which are fixed/determined).
///
/// # Arguments
/// * `branches` - Branches to extract facts from
/// * `plan` - Execution plan for rule reference resolution
/// * `provided_facts` - Facts to exclude (they're given/determined)
///
/// # Returns
/// Sorted, deduplicated list of undetermined fact paths
fn collect_undetermined_facts(
    branches: &[ShapeBranch],
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> Vec<FactPath>
```

**Implementation Details**:
- Extract lines 178-179 from `invert()`
- Use existing `collect_undetermined_facts_from_branches()` and `filter_provided_facts()`
- Or inline if simple enough

**Testing**:
- Test with branches containing various fact references
- Test with rule references (should expand and collect)
- Test that provided facts are excluded
- Test deduplication and sorting

---

### Phase 5: Refactored `invert()` Function

**New Structure**:
```rust
/// Invert a rule to find input domains that produce a desired outcome.
///
/// Given an execution plan and rule name, determines what values the unknown
/// facts must have to produce the target outcome.
///
/// The `provided_facts` set contains fact paths that are fixed (user-provided values).
/// Only these facts are substituted during hydration; other fact values remain as
/// undetermined facts for inversion.
///
/// Returns a [`Shape`] representing all valid solutions as a piecewise function.
pub fn invert(
    rule_name: &str,
    target: Target,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Shape> {
    // Phase 1: Lookup rule
    let executable_rule = plan.get_rule(rule_name).ok_or_else(|| {
        LemmaError::Engine(format!("Rule not found: {}.{}", plan.doc_name, rule_name))
    })?;
    let rule_path_string = executable_rule.path.to_string();

    // Phase 2: Expand rule references
    let expanded_branches = expand_rule_branches(executable_rule, plan, provided_facts)?;

    // Phase 3: Process and filter branches
    let (filtered_branches, available_outcomes) = process_expanded_branches(
        expanded_branches,
        &target,
        plan,
        provided_facts,
    )?;

    // Phase 4: Error handling
    if filtered_branches.is_empty() {
        return Err(build_no_solution_error(
            &rule_path_string,
            &target,
            &available_outcomes,
        ));
    }

    // Phase 5: Unify branches if needed
    let unified_branches = unify_branches_if_needed(filtered_branches, &target);

    // Phase 6: Collect undetermined facts
    let undetermined_facts = collect_undetermined_facts(&unified_branches, plan, provided_facts);

    // Phase 7: Build final shape
    Ok(Shape::new(unified_branches, undetermined_facts))
}
```

**Benefits**:
- **Clear phases**: Each phase has a single responsibility
- **Testable**: Each function can be tested independently
- **Readable**: Function names document what each phase does
- **Maintainable**: Changes to one phase don't affect others
- **Error handling**: Errors are handled at appropriate points

---

## Implementation Steps

### Step 1: Create Helper Functions (Low Risk)
1. Implement `hydrate_and_simplify_condition()`
2. Implement `extract_outcome()`
3. Implement `format_outcome_description()`
4. Add unit tests for each

### Step 2: Extract Expansion (Medium Risk)
1. Implement `expand_rule_branches()`
2. Update `invert()` to use it
3. Run existing tests to verify behavior unchanged
4. Add unit tests for `expand_rule_branches()`

### Step 3: Extract Processing (Medium Risk)
1. Implement `process_expanded_branches()` using helpers
2. Update `invert()` to use it
3. Run existing tests
4. Add unit tests

### Step 4: Extract Unification (Low Risk)
1. Implement `unify_branches_if_needed()`
2. Update `invert()` to use it
3. Run existing tests

### Step 5: Extract Undetermined Facts Collection (Low Risk)
1. Implement `collect_undetermined_facts()`
2. Update `invert()` to use it
3. Run existing tests

### Step 6: Final Cleanup
1. Remove `build_inversion_branches()` if it's now trivial (or keep if useful)
2. Review and improve doc comments
3. Ensure all error messages are clear
4. Run full test suite

---

## Testing Strategy

### Unit Tests for Each Function

1. **`expand_rule_branches()`**
   - Simple rule (no `RulePath` nodes)
   - Rule with `RulePath` in result
   - Rule with `RulePath` in condition
   - Rule with nested `RulePath` references
   - Cross-product creation

2. **`process_expanded_branches()`**
   - Hydration with various fact types
   - Constant folding
   - Outcome extraction (Value/Veto)
   - Filtering with different targets
   - Error handling

3. **`unify_branches_if_needed()`**
   - `any_value()` target (no unification)
   - Specific target (unification)
   - Multiple branches with same outcome
   - All branches different

4. **`collect_undetermined_facts()`**
   - Simple fact references
   - Rule references (expansion)
   - Provided facts exclusion
   - Deduplication

### Integration Tests

- Run all existing inversion tests
- Verify behavior unchanged
- Test edge cases:
  - Empty rule branches
  - All branches filtered out
  - Complex nested rule references
  - Large cross-products

---

## Risk Assessment

| Function | Risk Level | Mitigation |
|----------|-----------|------------|
| Helper functions | Low | Simple, well-defined operations |
| `expand_rule_branches()` | Medium | Complex logic, extensive testing needed |
| `process_expanded_branches()` | Medium | Multiple operations, test each helper |
| `unify_branches_if_needed()` | Low | Simple wrapper, existing function works |
| `collect_undetermined_facts()` | Low | Uses existing functions |

---

## Success Criteria

1. ✅ `invert()` function is < 30 lines
2. ✅ Each extracted function has clear doc comments
3. ✅ All existing tests pass
4. ✅ New unit tests cover each function
5. ✅ Code is more readable and maintainable
6. ✅ No performance regression
7. ✅ Error messages remain clear and helpful

---

## Future Improvements (Post-Refactoring)

Once refactored, these improvements become easier:

1. **Early Pruning**: Can add pruning in `expand_rule_branches()` or `process_expanded_branches()`
2. **Metrics**: Can add metrics collection at each phase
3. **Parallelization**: Each phase could potentially be parallelized
4. **Caching**: Can cache expansion results for repeated rule references

---

## Estimated Effort

- **Step 1** (Helper functions): 1-2 hours
- **Step 2** (Expansion): 2-3 hours
- **Step 3** (Processing): 2-3 hours
- **Step 4** (Unification): 30 minutes
- **Step 5** (Undetermined facts): 30 minutes
- **Step 6** (Cleanup): 1 hour

**Total**: ~8-10 hours

---

## Notes

- All functions should leverage normalized branches from `ExecutionPlan`
- No need to handle optional conditions (they're always present now)
- Keep error messages informative and helpful
- Maintain backward compatibility (same public API)

