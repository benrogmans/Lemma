# AGENTS.md — Guide for AI assistants working on Lemma

This file helps AI agents understand Lemma’s design, guarantees, and codebase so they can contribute correctly and preserve the language’s reliability goals.

---

## 1. What Lemma is

Lemma is a **declarative, strictly typed, pure language without functions**. It is designed for business logic, regulations, and contracts that must be both human-readable and machine-executable with **extreme certainty**.

- **Specs** (`spec`) contain **facts** (named values) and **rules** (expressions that may have `unless` clauses).
- There are **no functions, no side effects, no mutable state** during evaluation.
- **Types** include primitives (`boolean`, `number`, `scale`, `text`, `date`, `time`, `duration`, `ratio`) and user-defined types (e.g. `scale` with units like `eur`, `kilogram`).
- **References** (facts and rules) are written by name; the engine resolves whether each name is a fact or a rule during planning (e.g. `rule_name` or `spec_ref.rule_name`). References to rule’s result; **last matching `unless` wins**.

Lemma aims to be reliable enough for critical domains (e.g. aerospace, global regulations). That implies strict validation, deterministic execution, and a clear split between “invalid spec” (errors) and “valid but rule doesn’t yield a value” (Veto).

**Lemma has no inline comments** (by design, for clarity and unambiguous parsing). Another benefit is that it reduces the risk of ambiguity and contradiction between comments and the actual rules. The only documentation syntax is an optional **commentary block** in triple quotes immediately after a `spec` declaration (see below).

---

## 2. Core guarantees (do not weaken these)

### 2.1 Validation before execution

- **Planning** (semantic + graph + execution plan) fully validates the spec. Only if planning succeeds does the engine run evaluation.
- Invalid Lemma must be rejected with **Error** (Parse, Semantic, Engine, CircularDependency, etc.). Never “try to run and see.”

### 2.2 Post-validation: evaluation is guaranteed to complete

- **After** a spec has been successfully planned, evaluation is **guaranteed** to run to completion for the requested rules (modulo resource limits). The execution plan is self-contained and fully resolved.
- If something **impossible** happens during evaluation (e.g. missing explanation node, wrong variant in a match that was supposed to be exhaustive), the program must **fail fast**: use `unreachable!()` or `panic!()`. Do **not** return a fallback value or “best guess.”

### 2.3 No silent defaults or heuristics

- **Silent defaults and heuristics are forbidden.** If the semantics don’t define a value, the code must either:
  - return a **Error** (during parse/planning), or
  - return **Veto** (during evaluation, for domain-level “no value”), or
  - **panic / unreachable** when an invariant is violated.
- Never infer or guess to “keep going”; that undermines Lemma’s certainty.

### 2.4 Determinism

- **Execution plans** are deterministic: same spec + same fact inputs ⇒ same evaluation order and same results (or same Veto).
- Evaluation must not depend on undefined order (e.g. iteration over unordered collections in a way that affects results). Rules are evaluated in **topological order** by dependency.

### 2.5 Veto is a result, not an error

- Rules that **cannot produce a value** (e.g. division by zero, missing fact, user-defined `veto "reason"`, date overflow) yield **Veto**, not Error.
- **OperationResult** is either `Value(LiteralValue)` or `Veto(Option<String>)`.
- Veto propagates to dependent rules only when the dependent rule **needs** the vetoed value; if an `unless` branch provides a value without evaluating the vetoed rule, that branch can still succeed. See `documentation/veto_semantics.md`.
- **Planning returns Error** for invalid specs (type mismatches, unsupported operations). **Runtime panics** for bugs that fell through validation (use `unreachable!()`). **Veto** is only for domain-level "no value".

### 2.6 No placeholders in Lemma code

- **Using placeholders in Lemma code is unacceptable.** Placeholders (e.g. dummy values, “TODO” literals, fake numbers, or placeholder text) undermine Lemma’s goal of **extreme certainty**: they make specs look valid while hiding missing or wrong logic.
- Lemma specs must contain **real, intended facts and rules**. When examples or tests need concrete values, use **actual** values that match the domain. If something is not yet defined, leave it out or fail validation—do **not** fill gaps with placeholders.
- In documentation, tests, or any `.lemma` content: **no placeholder data.** This is non-negotiable for reliability.

---

## 3. Pipeline and code layout

High-level flow:

1. **Parse** (`engine/src/parsing/`)  
   - Grammar: `lemma/src/parsing/lemma.pest` (Pest).  
   - Produces AST; conversion to **LemmaSpec** (spec AST) happens in the same parse pipeline (see `parse::parse_spec` and `parsing/ast.rs`).

2. **AST** (`engine/src/parsing/ast.rs`)  
   - **LemmaSpec**, **LemmaRule**, **LemmaFact**, **Expression**, **ExpressionKind**, **FactValue**, **TypeSpecification**, etc.  
   - No evaluation here; this is the resolved spec structure used by planning.

3. **Planning** (`engine/src/planning/`)  
   - **Validation** (`validation.rs`): type/structure checks.  
   - **Graph** (`graph.rs`): builds dependency graph, resolves types and references, converts fact/rule references to **FactPath** / **RulePath**. Returns **Vec&lt;Error&gt;** on failure.  
   - **Execution plan** (`execution_plan.rs`): builds **ExecutionPlan** from graph (topologically sorted rules, fact schema, fact values, sources).  
   - **Types** (`types.rs`): type registry and scale/unit resolution.  
   - Entry: `plan::plan(main_spec, all_specs, sources)` → `Result<ExecutionPlan, Vec<Error>>`.

4. **Evaluation** (`engine/src/evaluation/`)  
   - **Expression** (`expression.rs`): evaluates expressions against an **EvaluationContext**; produces **OperationResult** (Value or Veto).  
   - **Operations** (`operations.rs`): **OperationResult**, **OperationKind**, explanation-related types.  
   - **Explanation** (`explanation.rs`): explanation trees for evaluation traces.  
   - **Response** (`response.rs`): **Response**, **RuleResult**, **Facts**.  
   - Only runs after a successful plan; any “impossible” state must panic/unreachable.

5. **Computation** (`engine/src/computation/`)  
   - **Arithmetic** (`arithmetic.rs`): division by zero (and similar) returns **Veto**, not panic.  
   - **Comparison**, **datetime**, **units**: used by evaluation; must preserve “no silent defaults” and use Veto where specified.  
   - Scale comparison and conversion use **same scale family** (via `TypeExtends::Custom`’s `family` field and `LemmaType::same_scale_family`), not exact type equality, so types in the same extension chain (e.g. `type x: scale ...` and `type x2: x ...`) are compatible.

6. **Engine** (`engine/src/engine.rs`)  
   - **Engine** holds specs, sources, and **execution plans**.  
   - `load` / `load_from_paths` → parse; after registry resolution, `plan(...)` all specs; on success, stores execution plans.  
   - `run` / `run_plan` evaluate; no spec parsing during run.

7. **Errors** (`engine/src/error.rs`)  
   - **Error**: Parsing, Validation (semantic/planning, including circular dependency), Inversion, Registry, ResourceLimitExceeded, Request (invalid API request, e.g. spec not found).  

---

## 4. Lemma syntax (quick reference and examples)

Lemma has **no inline comments** (`//` and `#` do not exist in the language). Use optional spec-level commentary in triple quotes after `spec name` when you need to describe a spec.

### Specs and commentary

```lemma
spec pricing
spec pricing.v2

spec shipping_policy
"""
Shipping rules and fees.
Optional description in triple quotes only.
"""
```

Spec names may carry an optional `.version_tag` suffix (e.g. `spec pricing.v1`). Base names cannot contain a period. `pricing.v1` and `pricing.v2` are distinct specs. An unversioned reference resolves to the latest loaded temporal version by natural sort. A spec cannot reference any temporal version of itself (same base name).

### Facts

```lemma
fact quantity: 10
fact name: "Alice"
fact is_active: true
fact price: 99.50 eur
fact workweek: 40 hours
fact tax_rate: 21 percent
fact deadline: 2024-01-15
fact birth_date: [date]
fact age: [number -> minimum 0 -> maximum 120]
fact product: [text -> option "A" -> option "B"]
fact currency: spec base_types
fact other_spec.price: 15
```

Spec reference syntax: **name required**; optional datetime and optional plan hash pin for verification:
`spec name`, `spec name datetime`, `spec name datetime~hash`, `spec name~hash`. Whitespace around `~` is allowed. Hash is verification only; resolution is always by (name, effective).

Values: literals (number, text, boolean, date, duration, percent, number+unit), type annotation `[type]` or `[type -> ...]`, or `spec other_spec`. Bind a fact in another spec with `fact spec_name.name: value`.

### Types

```lemma
type money: scale
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0

type weight: scale
  -> unit kilogram 1.0
  -> unit gram 0.001
  -> minimum 0 kilogram

type status: text
  -> option "active"
  -> option "inactive"

type discount_ratio: ratio
  -> minimum 0
  -> maximum 1

type age: number
  -> minimum 0
  -> maximum 120

type currency from base_spec
type discount_rate from pricing -> maximum 0.5
```

### Rules and unless (last matching wins)

```lemma
rule discount: 0 percent
  unless quantity >= 10 then 10 percent
  unless quantity >= 50 then 20 percent
  unless is_vip then 25 percent

rule daily_fee: 0 eur
  unless book_type is "regular" then 0.25 eur
  unless book_type is "reference" then 0.50 eur

rule total: quantity * price - discount
```

### Veto (user-defined “no value”)

```lemma
rule validated_price: price
  unless price < 0 then veto "Price cannot be negative"

rule can_drive: is_adult and has_license
  unless license_suspended then veto "License suspended"
```

### Rule references and cross-spec

```lemma
rule price_per_cup: base_price * size_multiplier
rule subtotal: price_per_cup * number_of_cups
rule total: subtotal - discount_amount

fact membership: spec premium_membership
rule discount: monthly_spend * membership.discount_rate
rule shipping_cost: 10
  unless monthly_spend >= membership.free_shipping_threshold then 0
```

Reference rules by name in the same spec; use `spec_ref.rule_name` when the rule lives in another spec (referenced via a `fact x: spec other_spec`). The engine resolves each reference to a fact or rule during planning.

### Literals

```lemma
true
false
yes
no
accept
reject
42
3.14
"hello"
2024-01-15
2024-01-15T14:30:00Z
40 hours
2 weeks
15 percent
15%
100 eur
5 kilograms
sqrt x
floor value
abs value
```

### Operators

- **Arithmetic:** `+` `-` `*` `/` `%` `^`
- **Comparison:** `>` `<` `>=` `<=` `==` `!=` `is` `is not`
- **Logical:** `and` `not`
- **Conversion:** `value in unit` (e.g. `price in usd`, `duration in days`)
- **Math-like (prefix):** `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round` (parentheses optional)

See **documentation/reference.md** and **documentation/index.md** for full syntax and more examples.

---

## 5. Error handling rules for implementers

- **Parse/planning:** Invalid input ⇒ return **Error** with clear, localized message (include source location where possible). Do not continue with a “best effort” plan.
- **Evaluation:**  
  - Domain failures (e.g. division by zero, missing fact, user veto, date overflow) ⇒ **OperationResult::Veto(...)**.  
  - Type/operator mismatches that planning should have rejected ⇒ **unreachable!()** (planning bug).  
  - Bug or invariant violation (e.g. missing node, wrong enum variant) ⇒ **panic!()** or **unreachable!()** with a message that includes context (e.g. “BUG: …”).
- **No silent fallbacks:** Do not use default values or heuristics to avoid failing; that violates Lemma’s guarantee of certainty.

---

## 6. Testing and development

- **Use TDD.** Failing tests define missing or broken behaviour; do not hide or remove them to make the suite pass.
- **Unit tests** live in the same module (to allow testing private functions); **integration tests** in `engine/tests/`.
- Run tests with **cargo nextest**, not `cargo test`.
- From repo root, **cargo precommit** runs **`versions-verify`**, then `fmt --check`, clippy, nextest, and cargo-deny (needs `cargo-nextest` and `cargo-deny` on `PATH`; CI runs the same checks across lint / test / security jobs).
- **The release version** is `[workspace.package] version` in the root `Cargo.toml`. Use **`cargo bump <version>`** to bump the version everywhere, then commit. **`cargo verify`** verifies that all version values are aligned.
- When adding features, add tests that lock in the intended behaviour (including Veto propagation and error cases).

---

## 7. Key documentation

| Resource | Purpose |
|----------|--------|
| **README.md** | Project overview, quick start, features |
| **xtask/README.md** | precommit, `cargo bump` / `cargo verify`, tracked release paths |
| **documentation/index.md** | Language concepts, specs, facts, rules |
| **documentation/reference.md** | Operators, types, literals, syntax |
| **documentation/veto_semantics.md** | When Veto applies and propagates |
| **documentation/examples/** | Example `.lemma` files |
| **.cursor/rules/** | Project rules (e.g. tests, TDD) |

---

## 8. Summary for agents

- Lemma is **strictly typed, pure, and function-free**; its goal is **extreme certainty** for specs that pass validation.
- **Planning** validates everything and returns **Error** on any misuse; **after** planning, evaluation is **guaranteed** to run to completion (no “maybe” evaluation).
- **Unexpected state during evaluation** ⇒ crash with `unreachable!()` or `panic!()`. **No silent defaults or heuristics.**
- **Execution plans** are **deterministic**; same inputs ⇒ same outputs (or same Veto).
- **Veto** is the way “this rule has no value” is expressed (e.g. division by zero, user `veto "..."`); it is not an error. Propagate Veto according to `veto_semantics.md`.
- When editing the codebase, preserve these guarantees, use **Error** for invalid Lemma, use **Veto** for domain-level “no value,” and **panic/unreachable** for bugs. Prefer **cargo nextest** and TDD as in the project rules.
- **Never use placeholders in Lemma code** (no dummy values, TODO literals, or fake data in `.lemma` files, docs, or tests). Placeholders destroy certainty; use real, intended values or omit/fail instead.
- **CLI:** `lemma schema <spec> [--effective T]` displays spec structure and plan hash. `lemma run <spec~hash> [--effective T]` pins to that hash. **APIs:** evaluate requests take required spec_name, optional effective, optional plan hash pin (verify before evaluate).
