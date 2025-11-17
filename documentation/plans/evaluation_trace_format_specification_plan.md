# Evaluation Trace Format v2 Specification - Plan

## Overview
Create a formal specification document that defines the rules for generating evaluation traces in the tree-based format. The specification should be implementable for CLI formatting, API responses, and other output formats.

## Document Structure Plan

### 1. **Introduction & Scope**
- Purpose: Define the canonical format for Lemma evaluation traces
- Target audiences: CLI formatters, API implementers, documentation generators
- Format version: v2 (tree-based)

### 2. **Structural Elements**

#### 2.1 Box Drawing Characters
- `├─` - Branch connector (continuing tree)
- `│` - Vertical line (continuation of parent branch)
- `└─` - Final item in branch
- `└>` - Result marker (final value)
- `×─` - Non-matched unless clause marker
- Box borders: `┌`, `├`, `└`, `─`, `│`, `┐`, `┤`, `┘`

#### 2.2 Header Format
- Pattern: `│ rule_name = value │`
- Top border: `┌─...─┐`
- Separator: `├─...─┤`
- Bottom border: `└─...─┘`
- Width calculation rules

#### 2.3 Indentation Rules
- Base indentation: 1 space per level
- Tree structure: Each nested level adds indentation
- Continuation lines: Aligned with opening

### 3. **Content Display Rules**

#### 3.1 Fact Lookups
- **Rule**: Simple fact lookups shown directly with `└> value` (no `=`)
- **Pattern**: `│ │  └> fact_value`
- **No expansion**: Facts are leaf nodes

#### 3.2 Rule References
- **Rule**: Rule references always expanded to show evaluation
- **Pattern**: 
  ```
  │ ├─ rule_name?
  │ │  └> value (if simple)
  │ │  ├─ ... (if complex, show expansion)
  ```
- **Expansion depth**: Full expansion until facts or simple calculations

#### 3.3 Calculations
- **Rule**: Progressive substitution pattern
- **Pattern**: 
  ```
  │ ├─ = expression_with_substitutions
  │ └> = final_result
  ```
- **Unknown resolution**: All rule references resolved before showing complete formula
- **No intermediate steps**: Show formula with all substitutions, then result directly

#### 3.4 Results
- **Direct values**: `└> value` (no `=`)
- **Computed values**: `└> = value` (with `=`)
- **Unless clause results**: Same rules apply (direct vs computed)

### 4. **Unless Clause Rules**

#### 4.1 Ordering
- **Rule**: Matched branch shown first (whether default or unless clause)
- **Rule**: Only subsequent non-matched branches shown
- **Rationale**: Explains why the matched branch was chosen

#### 4.2 Matched Clause Display
- **Rule**: Fully expanded showing:
  - Condition evaluation (if applicable)
  - Result expression or value
  - Full expansion of result if it's an expression
- **Pattern**:
  ```
  │ ├─ condition
  │ │  ├─ ... (condition evaluation)
  │ │  └─ = condition_result
  │ └> result_expression_or_value
  │    ├─ ... (if expression, expand it)
  │    └> = final_value
  ```

#### 4.3 Non-Matched Clause Display
- **Rule**: Shown with `×─` marker
- **Rule**: Show condition evaluation only (not the result expression)
- **Rule**: Expanded to show why condition failed
- **Pattern**:
  ```
  │ ×─ condition
  │    ├─ ... (condition evaluation)
  │    └─ = false (implicit, ×─ indicates false)
  ```

#### 4.4 Default Values
- **Rule**: Treated same as unless clauses (no special handling)
- **Rule**: If default matches, show it first with `└> value`
- **Rule**: Then show subsequent non-matched unless clauses

### 5. **Type Handling**

#### 5.1 Type Display
- **Numbers**: Displayed as-is with appropriate formatting
- **Percentages**: Shown with `%` suffix, 2 decimal precision
- **Money**: Shown with currency and amount
- **Text**: Shown with quotes
- **Booleans**: Shown as the actual boolean literal from the Lemma document (e.g., `true`, `false`, `yes`, `no`, `accept`, `reject`)

#### 5.2 Type Consistency
- **Rule**: Types preserved throughout trace
- **Rule**: No implicit conversions shown (e.g., don't convert percentage to decimal)
- **Rule**: Type annotations shown when necessary for clarity

### 6. **Evaluation Flow Rules**

#### 6.1 Unknown Resolution
- **Rule**: All rule references (`rule_name?`) must be resolved before showing complete formula
- **Pattern**:
  1. Expand all `rule_name?` references
  2. Show complete formula with all substitutions
  3. Show final result

#### 6.2 Expression Expansion
- **Rule**: Complex expressions expanded to show evaluation
- **Rule**: Simple expressions can be shown inline
- **Decision point**: If expression contains rule references, expand them

#### 6.3 Intermediate Steps
- **Rule**: No unnecessary intermediate calculation steps
- **Rule**: Show: resolve unknowns → show formula → show result
- **Exception**: Only show intermediate steps if they add clarity (e.g., complex nested calculations)

### 7. **Edge Cases & Special Rules**

#### 7.1 Nested Rule References
- **Rule**: Each level of nesting gets its own indentation
- **Rule**: Full expansion at each level before proceeding

#### 7.2 Complex Expressions
- **Rule**: Multi-line expressions aligned properly
- **Rule**: Continuation lines use `│` for alignment

#### 7.3 Veto Handling
- **Rule**: Veto blocks rule from producing any valid result
- **Rule**: When a rule is vetoed, show the veto message (if present)
- **Rule**: Veto in unless clause shown like other unless clause results
- **Pattern**: To be defined based on examples (veto shown where it occurs in unless clause)

#### 7.4 Boolean Conditions
- **Rule**: Boolean expressions expanded to show evaluation
- **Rule**: Show each operand and the logical operation result

### 8. **Implementation Guidelines**

#### 8.1 Width Calculation
- **Rule**: Maximum width: 100 characters
- **Rule**: Consistent width within a trace box
- **Rule**: Long expressions wrap with proper indentation
- **Rule**: Continuation lines aligned with `│` character


## Proposed Specification Format

### Option A: Markdown Specification Document
- Structured markdown with clear sections
- Examples for each rule
- Easy to read and implement
- Good for documentation

### Option B: Lemma Document (Meta-Specification)
- Use Lemma itself to describe the formatting rules
- Facts define format elements
- Rules describe when to apply formatting
- Unless clauses for conditional formatting
- **Challenge**: Lemma is for business logic, not formatting
- **Benefit**: Self-documenting, executable specification

### Option C: Hybrid Approach
- Markdown specification for human readability
- Lemma document for the logic/rules of when to format
- JSON Schema or similar for structural validation

## Recommendation

**Hybrid Approach (Option C)**:
1. Create a comprehensive Markdown specification document
2. Create a Lemma document that encodes the *decision logic* for formatting:
   - When to expand vs inline
   - When to show intermediate steps
   - How to order unless clauses
   - When to use `└>` vs `└> =`
3. The Lemma document would serve as executable rules that formatters can reference

## Next Steps

1. **Extract all rules** from the evaluation_trace_format_v2.md document
2. **Organize rules** into the structure above
3. **Write Markdown specification** with examples
4. **Attempt Lemma meta-specification** for the formatting logic
5. **Create validation examples** to test implementations

## Implementation Decisions

### Width Handling
- **Rule**: Limit trace width to 100 characters
- **Rule**: Wrap long lines with proper indentation
- **Rule**: Continuation lines use `│` for alignment

### Veto Handling
- **Rule**: Veto is not an error - it's a validation mechanism
- **Rule**: When a rule is vetoed, it has no result value
- **Rule**: Veto message (if present) shown where veto occurs
- **Rule**: Veto in unless clause displayed like other unless clause results

### Document References
- **Question**: How to handle document references (e.g., `document.rule_name?`)?
- **Status**: To be addressed - examples avoid document references currently

