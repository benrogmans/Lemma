---
layout: default
title: Inversion
---

# Inversion for Lemma

## Overview

This document describes the architectural approach for adding inversion capabilities to Lemma, enabling inverse reasoning without changing the language itself.

Instead of searching for solutions, the system will derive the algebraic shape of relationships between values, allowing you to answer questions like “what inputs produce this output?” through symbolic manipulation.

## Core Concept

Lemma rules already define relationships between values through their expressions. Inversion extracts these relationships to work backwards from outputs to inputs.

### Example

```lemma
doc pricing
fact price = [money]
fact quantity = [number]

rule total = price * quantity
```

Forward evaluation (current): Given `price=10 EUR, quantity=5` → `total=50 EUR`

Inversion (new): Given `total=50 EUR` → derive that `price * quantity = 50`, which represents:
- Shape: {(price, quantity) | price * quantity = 50}
- Relationship: `quantity = 50 EUR / price`

## What is a “Shape”?

A shape is the mathematical relationship between unknowns, extracted from rule definitions.

### Example 1: Linear relationship

```lemma
rule total = price * quantity
```

If `total = 100 EUR`:
- Shape: `price * quantity = 100 EUR`
- Explicit form: `quantity = 100 EUR / price`
- Free variables: `price` (can vary freely)

### Example 2: With unless clauses (piecewise)

```lemma
rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
```

If `discount = 10%`:
- Shape: `10 <= quantity < 50` (from piecewise analysis)

### Example 3: With veto in piecewise function

```lemma
rule shipping_cost = 5 EUR
  unless weight >= 10 then 10 EUR
  unless weight >= 50 then 25 EUR
  unless weight < 0 then veto "invalid"
  unless weight > 100 then veto "too heavy"
```

This is a piecewise function where some regions produce values and others produce vetos. You can query both:

**Value queries:**
- `shipping_cost = 10 EUR` → Shape: `10 <= weight < 50`
- `shipping_cost = 25 EUR` → Shape: `50 <= weight <= 100`

**Veto queries:**
- `shipping_cost = veto "invalid"` → Shape: `weight < 0`
- `shipping_cost = veto "too heavy"` → Shape: `weight > 100`
- `shipping_cost = veto` (any veto) → Shape: `weight < 0 OR weight > 100`

Note: Last matching clause wins, so a later clause can override an earlier veto.

## Architecture: Workspace with Inversion

### Unified Architecture (Engine renamed to Workspace)

The Workspace becomes the single, unified core. It encapsulates parsing, validation, forward evaluation, and the new relation/constraint indexes used for inversion.

```
┌─────────────────────────────────────────────────────────┐
│                       Public APIs                       │
│              (programmatic integrations)                │
└───────────────────────┬─────────────────────────────────┘
                        │
┌───────────────────────▼─────────────────────────────────┐
│                      Workspace                          │
│  • Document store                                       │
│  • Parser + Validator                                   │
│  • Forward Evaluator (topological)                      │
│  • RelationGraph (bidirectional deps, piecewise, veto)  │
│  • Inversion (symbolic)                                 │
└───────────────────────┬─────────────────────────────────┘
                        │
            (internal modules and data)
```

Internals (parser, validator, evaluator) are retained; this is a rename to align naming with inversion features.

The same Workspace serves both directions:
- Forward: Facts → Rules (deterministic evaluation)
- Inverse: Rules → Shapes over Facts (symbolic inversion)
- Piecewise: Unless clauses (including vetos) define the complete function structure

### Data Structures

```rust
pub struct Workspace {
    // Primary store of validated documents
    documents: HashMap<String, LemmaDoc>,

    // Relation graph powering inversion
    relations: RelationGraph,

    // Global metadata and limits
    sources: HashMap<String, String>,
    limits: ResourceLimits,
}

pub struct RelationGraph {
    // Map each rule to its algebraic relation
    relations: HashMap<RulePath, Relation>,

    // Dependency structure (bidirectional)
    dependencies: BidirectionalDeps,
}

pub struct Relation {
    rule_path: RulePath,

    // The algebraic expression defining this rule (base case)
    expression: Expression,

    // Unless clauses as piecewise functions (includes vetos)
    branches: Vec<Branch>,
}

pub struct Branch {
    condition: Expression,       // When this applies
    result: OperationResult,     // What it produces (reuses existing type!)
}

pub struct BidirectionalDeps {
    // Rule → Facts it reads
    reads: HashMap<RulePath, HashSet<FactPath>>,

    // Fact → Rules that read it
    read_by: HashMap<FactPath, HashSet<RulePath>>,

    // Rule → Rules it references
    depends_on: HashMap<RulePath, HashSet<RulePath>>,

    // Rule → Rules that reference it
    depended_by: HashMap<RulePath, HashSet<RulePath>>,
}
```

Note: `OperationResult` is reused from Lemma's existing forward evaluation types:
```rust
pub enum OperationResult {
    Value(LiteralValue),
    Veto(Option<String>),
}
```

This provides semantic consistency: the same type represents outcomes in both forward evaluation and inversion.

### Shape Representation

```rust
pub struct Shape {
    // Algebraic relationships between unknowns
    pub relationships: Vec<Relationship>,

    // Variables that can vary freely (not fully constrained)
    pub free_variables: Vec<FactPath>,
}

pub enum Relationship {
    // Simple: x = expression(y, z, ...)
    Equation {
        lhs: FactPath,
        rhs: Expression,
    },

    // Piecewise: x = expr1 if cond1, veto if cond2, ...
    Piecewise {
        variable: FactPath,
        branches: Vec<PiecewiseBranch>,
    },

    // Implicit: f(x, y, z) = value (can't solve for any single variable)
    Implicit {
        expression: Expression,
        value: LiteralValue,
    },
}

pub struct PiecewiseBranch {
    pub condition: Expression,      // When this branch applies
    pub outcome: OperationResult,   // What happens (reuses existing type!)
}

pub enum Bound {
    Inclusive(LiteralValue),
    Exclusive(LiteralValue),
    Unbounded,
}

pub enum Domain {
    // Numeric/comparable ranges with type-safe bounds
    Range {
        min: Bound,
        max: Bound,
    },

    // Multiple disjoint ranges
    Union(Vec<Domain>),

    // Specific values only
    Enumeration(Vec<LiteralValue>),

    // Everything except these constraints
    Complement(Box<Domain>),

    // Any value (no constraints)
    Unconstrained,
}
```

### Comparisons and Boolean Logic

We reuse Lemma’s existing `Expression` AST for guards and values:

- Comparisons use the existing `ComparisonOperator` (Eq, Neq, Lt, Lte, Gt, Gte).
- Boolean connectives reuse existing logical nodes (AND, OR, NOT) from the AST.
- Semantics:
  - AND corresponds to intersection of regions.
  - OR corresponds to union of regions.
  - NOT is preserved symbolically; basic normalization may be applied.
- Use in shapes:
  - Piecewise branch conditions are `Expression` nodes with boolean type.
  - Branch outcomes use `OperationResult` (Value or Veto).
  - Given facts are substituted to constant-fold; unsatisfiable branches are dropped.
- Scope:
  - Guards are preserved symbolically; no SAT/SMT solving or global boolean minimization.

## API Design

### Inversion Method

```rust
/// Target specification for inversion
pub struct Target {
    pub rule: RulePath,
    pub op: TargetOp,
    pub outcome: OperationResult,  // Reuses existing Lemma type!
}

#[derive(Clone, Copy)]
pub enum TargetOp {
    Eq, Neq, Lt, Lte, Gt, Gte,
}

impl Workspace {
    /// Invert a single target rule to derive shapes under given facts.
    ///
    /// Supports both value queries (e.g., "what inputs produce this value?")
    /// and veto queries (e.g., "what inputs trigger this veto?").
    ///
    /// One target per call. To constrain multiple rules, call this
    /// function multiple times and intersect results externally.
    pub fn invert(
        &self,
        doc_name: &str,
        target: Target,
        given_facts: HashMap<FactPath, LiteralValue>,
    ) -> LemmaResult<Shape> {
        // 1) Fetch the rule's relation
        let relation = self.relations.relations
            .get(&target.rule)
            .ok_or_else(|| LemmaError::Engine(format!("Rule not found: {}", target.rule)))?;

        // 2) Build target guard: (relation.expression target.op target.value)
        let target_guard = self.build_target_guard(&relation.expression, &target)?;

        // 3) Hydrate everything with given facts
        let expr_h = self.hydrate_expression(&relation.expression, &given_facts)?;
        let guard_h = self.hydrate_expression(&target_guard, &given_facts)?;

        let branches_h = relation.branches.iter()
            .map(|b| {
                let outcome = match &b.result {
                    OperationResult::Value(expr) =>
                        OperationResult::Value(self.hydrate_expression(expr, &given_facts)?),
                    OperationResult::Veto(message) =>
                        OperationResult::Veto(message.clone()),
                };
                Ok(PiecewiseBranch {
                    condition: self.hydrate_expression(&b.condition, &given_facts)?,
                    outcome,
                })
            })
            .collect::<LemmaResult<Vec<_>>>()?;        // 4) Build relationships
        let relationships = if !branches_h.is_empty() {
            // Piecewise: filter branches matching the target outcome type
            let pw = branches_h.into_iter()
                .filter_map(|mut br| {
                    let matches = match (&br.outcome, &target.outcome) {
                        (BranchOutcome::Value(_), TargetOutcome::Value(val)) => {
                            // Match value branches when querying for a value
                            br.condition = Expression::and(br.condition, guard_h.clone());
                            true
                        }
                        (BranchOutcome::Veto { message }, TargetOutcome::Veto { message: target_msg }) => {
                            // Match veto branches when querying for veto
                            // If target_msg is None, match any veto
                            // If target_msg is Some, match only that specific message
                            if target_msg.is_none() || target_msg.as_ref() == Some(message) {
                                br.condition = Expression::and(br.condition, guard_h.clone());
                                true
                            } else {
                                false
                            }
                        }
                        _ => false, // Type mismatch: value vs veto
                    };
                    if matches { Some(br) } else { None }
                })
                .collect::<Vec<_>>();

            if pw.is_empty() {
                // Target outcome cannot be produced
                let msg = match &target.outcome {
                    TargetOutcome::Value(_) => "Target value unreachable (only vetos match)",
                    TargetOutcome::Veto { message: Some(m) } =>
                        &format!("Veto '{}' unreachable", m),
                    TargetOutcome::Veto { message: None } => "No veto regions found",
                };
                return Err(LemmaError::Engine(msg.to_string()));
            }

            vec![Relationship::Piecewise {
                variable: target.rule.as_fact_path(),
                branches: pw,
            }]
        } else {
            // No piecewise branches - simple expression
            match (&target.outcome, target.op) {
                (OperationResult::Value(val), TargetOp::Eq) => {
                    // Try explicit solve only when there is exactly one unknown fact
                    let unknowns = self.find_unknown_fact_paths(&expr_h, &given_facts);
                    if unknowns.len() == 1 {
                        if let Ok(rhs) = self.algebraic_solve(&expr_h, &unknowns[0], val, &given_facts) {
                            vec![Relationship::Equation { lhs: unknowns[0].clone(), rhs }]
                        } else {
                            vec![Relationship::Implicit { expression: expr_h, value: val.clone() }]
                        }
                    } else {
                        vec![Relationship::Implicit { expression: expr_h, value: val.clone() }]
                    }
                }
                (OperationResult::Veto(_), _) => {
                    // No piecewise branches means no vetos in this rule
                    return Err(LemmaError::Engine("Rule has no veto clauses".to_string()));
                }
                _ => {
                    // Inequalities and '!=' for values
                    vec![Relationship::Implicit { expression: expr_h, value: LiteralValue::Unit }]
                }
            }
        };

        // 5) Free variables = fact refs in relationships minus given facts
        let free_variables = self.identify_free_variables(&relationships, &given_facts);

        Ok(Shape { relationships, free_variables })
    }

    /// Get the valid domain for a fact by inverting veto conditions.
    ///
    /// This derives what values a fact can take without causing the rule to veto.
    /// Useful for validation, form fields, API documentation, and test generation.
    pub fn get_valid_domain(
        &self,
        doc_name: &str,
        rule: RulePath,
        fact: FactPath,
        given_facts: HashMap<FactPath, LiteralValue>,
    ) -> LemmaResult<Domain> {
        // 1) Invert for any veto to find invalid regions
        let veto_result = self.invert(doc_name, Target {
            rule: rule.clone(),
            op: TargetOp::Eq,
            outcome: OperationResult::Veto(None),
        }, given_facts.clone());

        // 2) If invert fails (no vetos exist), domain is unconstrained
        let veto_shape = match veto_result {
            Ok(shape) => shape,
            Err(_) => return Ok(Domain::Unconstrained),
        };

        // 3) Extract constraints on the target fact from veto conditions
        let veto_constraints = self.extract_fact_constraints(&veto_shape, &fact)?;

        // 4) Negate constraints to get valid domain
        let valid_domain = self.negate_domain(veto_constraints)?;

        Ok(valid_domain)
    }
}
```

### Forward Evaluation Method (compatible)

Forward evaluation remains available on Workspace and continues to accept given facts. This is the same behavior users have today, surfaced from the unified core.

```rust
impl Workspace {
    /// Evaluate rules in a document with optional given facts
    pub fn evaluate(
        &self,
        doc_name: &str,
        rule_names: Option<Vec<String>>,      // None = all rules
        given_facts: Option<Vec<LemmaFact>>,  // provided facts that override defaults
    ) -> LemmaResult<Response> {
        // Internally builds facts, performs topological execution,
        // applies veto semantics, and returns a Response
        self.forward_evaluate(doc_name, rule_names, given_facts)
    }
}
```

### Valid Domain Method

```rust
impl Workspace {
    /// Get the valid domain for a fact within a rule's context
    ///
    /// Example: "What salary values won't cause the bonus rule to veto?"
    pub fn get_valid_domain(
        &self,
        doc_name: &str,
        rule: RulePath,
        fact: FactPath,
        given_facts: HashMap<FactPath, LiteralValue>,
    ) -> LemmaResult<Domain>
}
```

Example usage:

```rust
// Find valid salary range
let valid_salary = workspace.get_valid_domain(
    "employee",
    RulePath::local("bonus"),
    FactPath::local("salary"),
    HashMap::new()
)?;

// Result: Domain::Range {
//   min: Bound::Inclusive(30000 EUR),
//   max: Bound::Inclusive(500000 EUR),
// }

// Find valid years_of_service given a specific salary
let mut given = HashMap::new();
given.insert("salary".into(), LiteralValue::Money {
    amount: 40000.into(),
    currency: "EUR".to_string()
});

let valid_tenure = workspace.get_valid_domain(
    "employee",
    RulePath::local("bonus"),
    FactPath::local("years_of_service"),
    given
)?;

// Result: Domain::Range {
//   min: Bound::Inclusive(0),
//   max: Bound::Unbounded,
// }
```

Notes:
- The same `given_facts` parameter can also be used when inverting to reduce free variables and narrow the shape.
- In both APIs, given facts narrow the valid region but do not mutate stored documents. During forward evaluation, they act as overrides of defaults.

## Implementation Phases

### Phase 1: Extract Relations (1-2 weeks)

How `hydrate_expression` differs from `evaluate()`:
- `evaluate()` runs everything to a concrete result (or veto) and records a runtime trace of that single path.
- `hydrate_expression()` fills in the facts you already know and returns a simplified `Expression`. Unknowns stay symbolic so you can see what’s still needed or reuse the partial formula later.

Build the relation graph during document loading.

Note: We build the `RelationGraph` eagerly on `add_document`. No caching or lazy construction is required for this plan.

Goal: Extract algebraic relations as piecewise functions from Lemma rules without changing the language.

Tasks:
1. Extend `Workspace::add_document` to extract relations
2. Parse rule expressions into `Relation` structures
3. Capture all unless clauses (including vetos) as `Branch` structures
4. Build bidirectional dependency graph

Example extraction:

```lemma
rule shipping_cost = 5 EUR
  unless weight >= 10 then 10 EUR
  unless weight >= 50 then 25 EUR
  unless weight < 0 then veto "invalid"
  unless weight > 100 then veto "too heavy"
```

Extracts to a complete piecewise function:
- Base: `shipping_cost = 5 EUR`
- Branch 1: `if weight >= 10 then 10 EUR`
- Branch 2: `if weight >= 50 then 25 EUR`
- Branch 3: `if weight < 0 then Veto("invalid")`
- Branch 4: `if weight > 100 then Veto("too heavy")`

Note: Last matching branch wins, so evaluation order matters.

Sketch:

```rust
impl Workspace {
    pub fn add_document(&mut self, doc: LemmaDoc) -> LemmaResult<()> {
        self.documents.insert(doc.name.clone(), doc.clone());

        // Extract relations (including all unless clauses)
        for rule in &doc.rules {
            let relation = self.extract_relation(&doc, rule)?;
            self.relations.relations.insert(RulePath::local(&rule.name), relation);
        }

        // Build dependencies
        self.relations.dependencies = self.build_bidirectional_deps(&doc)?;
        Ok(())
    }

    fn extract_relation(&self, _doc: &LemmaDoc, rule: &LemmaRule) -> LemmaResult<Relation> {
        let branches = rule.unless_clauses.iter().map(|uc| {
            let result = match &uc.result.kind {
                ExpressionKind::Veto(veto_expr) => OperationResult::Veto(
                    veto_expr.message.clone()
                ),
                _ => OperationResult::Value(uc.result.clone()),
            };

            Branch {
                condition: uc.condition.clone(),
                result,
            }
        }).collect();

        Ok(Relation {
            rule_path: RulePath::local(&rule.name),
            expression: rule.expression.clone(),
            branches,
        })
    }
}
```

### Phase 2: Derive Shapes (2-3 weeks)

Implement symbolic manipulation directly inside `invert`, with small internal helpers. Also implement `get_valid_domain` for practical fact validation.

Goal: Given a single target (rule, operator, outcome) and `given_facts`, produce a `Shape`. Additionally, provide a convenient method to extract valid domains for facts.

Tasks:
1. Build the target guard from the rule's expression: `expr (op) outcome`
2. Hydrate expression, branch conditions/outcomes, and target guard using `hydrate_expression`
3. For simple expressions (no branches):
   - If target is a veto, return error (no veto clauses)
   - If target is a value with `op == Eq` and exactly one unknown, attempt `algebraic_solve`
   - Otherwise keep implicit
4. For piecewise:
   - Filter branches to match target outcome type (value vs veto)
   - For veto queries, optionally match specific veto message
   - Conjoin each remaining branch guard with the hydrated target guard
   - Prune branches that fold to false
   - Return error if no branches match
5. Compute `free_variables =` fact references minus `given_facts`
6. Return `Shape { relationships, free_variables }`
7. Implement `get_valid_domain`:
   - Calls `invert` with `Veto(None)` target
   - Extracts constraints on the specified fact from the veto shape
   - Negates constraints to produce valid domain
   - Returns structured `Domain` enum

Outcome matching:
- `OperationResult::Value(v)`: matches only value-producing branches
- `OperationResult::Veto(Some(m))`: matches only veto with specific message
- `OperationResult::Veto(None)`: matches any veto branch

This enables queries like:
- "What inputs produce value X?" (existing behavior)
- "What inputs trigger veto Y?" (new capability)
- "What inputs cause any veto?" (validation analysis)

Operator behavior:
- `Eq`: attempt explicit solve for a single unknown; otherwise `Implicit { expression, outcome }`
- `Neq` / `Lt` / `Lte` / `Gt` / `Gte`: keep as an implicit constraint within piecewise conditions
- Booleans: `Eq(true/false)` reduces via hydration and branch pruning

Veto inversion:
- Vetos are first-class outcomes that can be queried
- `OperationResult::Veto` filters branches to only veto-producing regions
- Enables answering "under what conditions does this rule veto?"
- Use cases: validation analysis, error boundary detection, compliance checking, test generation

Internal helpers used (no public exposure):
- `hydrate_expression(expr, given_facts)`: substitutes given facts and simplifies locally
- `algebraic_solve(expr, unknown_fact, target_value, given_facts)`: supports `+`, `−`, `×`, `÷` only
- `find_unknown_fact_paths(expr, given_facts)`: inspects `FactReference` only (rule references remain opaque)
- `build_target_guard(expr, target)`: constructs the boolean guard `Expression` for the target
- `extract_fact_constraints(shape, fact)`: extracts constraints on a specific fact from a shape
- `negate_domain(domain)`: negates domain constraints to produce the complement

Example queries:

```rust
// Value query: "What weight produces 25 EUR shipping?"
invert("shipping_cost", Target {
    rule: "shipping_cost",
    op: Eq,
    outcome: OperationResult::Value(LiteralValue::Money {
        amount: 25.into(),
        currency: "EUR".to_string()
    })
}) → Shape { relationships: [Piecewise { "50 <= weight <= 100" }] }

// Veto query: "When does shipping veto as 'too heavy'?"
invert("shipping_cost", Target {
    rule: "shipping_cost",
    op: Eq,
    outcome: OperationResult::Veto(Some("too heavy".to_string()))
}) → Shape { relationships: [Piecewise { "weight > 100" }] }

// Any veto query: "When does shipping fail validation?"
invert("shipping_cost", Target {
    rule: "shipping_cost",
    op: Eq,
    outcome: OperationResult::Veto(None)
}) → Shape { relationships: [Piecewise { "weight < 0 OR weight > 100" }] }

// Valid domain query: "What weight values are valid?"
get_valid_domain("shipping_doc", "shipping_cost", "weight", HashMap::new())
  → Domain::Range {
      min: Bound::Inclusive(0),
      max: Bound::Inclusive(100),
    }
```

Valid domain use cases:
- **Form validation**: Generate client-side validation rules
- **API documentation**: Document valid parameter ranges
- **Error messages**: "salary must be between 30000 and 500000 EUR"
- **Test generation**: Generate test cases within valid bounds
- **Configuration validation**: Validate config files against rule constraints

### Phase 3: Shape Representation (1 week)

Make shapes easy to inspect and consume in a minimal, consistent way:

Goals:
- Provide a concise `Display` implementation for core types (no extensive formatting guidance)
- Keep outputs stable and deterministic for testing
- Clearly distinguish between value-producing and veto-producing branches in output

## Hydrate expression: `hydrate_expression`

Purpose
- Fill in any known facts and clean up the expression without changing its meaning. The result is a simple, stable `Expression` you can read, test, or carry forward even when some inputs are still unknown.

Contract (inputs/outputs/invariants)
- Input: `Expression` (guards or values), `given_facts: Map<FactPath, LiteralValue>`.
- Output: `Expression` with the same type as input, simplified where safe.
- Deterministic and idempotent.
- Type- and unit-preserving; respect Lemma’s unit system.
- No global solving: do not perform SAT/SMT, system solving, or global piecewise minimization.

Rewrite pipeline (in order)
1) Substitute given facts
2) Unit and literal normalization (no FX; no cross-dimension coercion)
3) Constant folding (safe-only: arithmetic/boolean/comparisons/dates/durations per runtime semantics)
4) Comparison canonicalization (variable-like on left, literal on right; normalize operator direction)
5) Boolean normalization (NOT pushdown for comparisons, light De Morgan, flatten, identities, dedupe)
6) Arithmetic cleanup (neutral-element removal, flatten associative/commutative; stable ordering)
7) Piecewise and guard pruning (drop false, keep true; no cross-branch merging)

Edge cases and safety
- Never fold divide-by-zero; leave as expression.
- Incompatible units remain unchanged; no currency conversion.
- Timezones/datetime: fold only if pure and timezone-resolved per Lemma.
- Veto payloads are opaque; do not rewrite their semantics.

Success criteria
- Determinism and idempotence.
- Preservation: forward evaluation of original vs hydrated expressions is equivalent (within Lemma’s semantics and limits).
- Typical expressions shrink or stay equal; no pathological growth.

## Usage

This plan defines a minimal programmatic API (see "Inversion Method"). Examples and output mockups are intentionally omitted to keep the document concise.

## Migration Guide: Engine → Workspace

This plan unifies forward evaluation and inversion inside Workspace. Migrating existing code is straightforward:

### Minimal changes

- Construct: `let mut workspace = Workspace::new();`
- Load docs: `workspace.add_lemma_code(code, source)?;`
- Forward eval: `workspace.evaluate(doc, rule_names, given_facts)`
- Value inversion: `workspace.invert(doc, Target { rule, op: Eq, outcome: OperationResult::Value(val) }, given_facts)`
- Veto inversion: `workspace.invert(doc, Target { rule, op: Eq, outcome: OperationResult::Veto(msg) }, given_facts)`
- Valid domain: `workspace.get_valid_domain(doc, rule, fact, given_facts)`

### API mapping

| Before (Engine) | After (Workspace) |
|-----------------|-------------------|
| `Engine::new()` | `Workspace::new()` |
| `add_lemma_code` | `add_lemma_code` (same signature) |
| `evaluate(doc, rules, given_facts)` | `evaluate(doc, rules, given_facts)` |
| — | `invert(doc, Target{rule,op,outcome}, given_facts)` |
| — | `get_valid_domain(doc, rule, fact, given_facts)` |

### Compatibility notes

- Given facts: same `LemmaFact` structures and type parsing (act as overrides during evaluation).
- Forward evaluation semantics and response format remain unchanged (including veto propagation and operation records).
- Type reuse: `OperationResult` is used consistently across forward and inverse directions for semantic alignment.
- A temporary `Engine` type alias can be kept during the rename to ease transition, delegating internally to `Workspace`.

## Benefits

1. No language changes: Lemma syntax remains unchanged
2. No searching: Pure symbolic manipulation, deterministic
3. Unified model: Vetos and values are both first-class piecewise outcomes
4. Simpler architecture: No separate boundary extraction; everything is piecewise
5. Bidirectional: Same workspace serves forward evaluation and inversion
6. Sound: Correctly handles cases where vetos are overridden by later clauses
7. Complete: Can query both "what produces value X?" and "what triggers veto Y?"
8. Transparent: Shows exact mathematical relationships including veto regions
9. Type-safe: Uses Lemma's existing type system
10. Composable: Works with document references and rule composition
11. Practical: `get_valid_domain` enables real-world validation and documentation use cases

## Limitations and Future Work

### Current Limitations

1. Simple algebraic solving only: initially supports basic arithmetic operations (+, −, ×, ÷)
2. Single unknown per equation: multi-variable systems remain implicit
3. No global piecewise minimization: we return a sound union of guarded branches; results may include overlapping or redundant guards
4. No SAT/SMT solving for guards: boolean/relational guards are preserved symbolically with local normalization and constant folding

### Future Enhancements

1. Advanced algebra: support for exponentiation, roots, logarithms
2. System solving: multiple equations with multiple unknowns
3. Constraint propagation: more sophisticated boundary analysis
4. Optimization hints: suggest optimal values within constraints
5. Visualization: graphical representation of solution spaces
6. Domain simplification: reduce complex domain expressions to canonical forms
7. Multi-rule domain analysis: combine constraints from multiple dependent rules

## Timeline

- Phase 1: Extract Relations (1-2 weeks)
- Phase 2: Derive Shapes (2-3 weeks)
- Phase 3: Shape Representation (1 week)

Total estimated time: 4-6 weeks

## Conclusion

Inversion transforms Lemma's execution engine into a bidirectional workspace without changing the language. By treating rules as complete piecewise functions—where each branch can produce either a value or a veto—we can derive the mathematical structure of both successful outcomes and failure conditions.

This unified approach enables powerful queries like "what inputs produce this value?", "what inputs trigger this veto?", and "what values are valid for this fact?", making the system ideal for validation analysis, error boundary detection, compliance checking, automated test generation, and API documentation.

This approach maintains Lemma's strengths (declarative, type-safe, natural language-like) while adding comprehensive inverse reasoning capabilities.