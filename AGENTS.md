# AGENTS.md — Guide for AI assistants working on Lemma

This file helps AI agents understand Lemma’s design, guarantees, and codebase so they can contribute correctly and preserve the language’s reliability goals.

---

## 1. What Lemma is

Lemma is a **declarative, strictly typed, pure language without functions**. It is designed for business logic, regulations, and contracts that must be both human-readable and machine-executable with **extreme certainty**.

- **Documents** (`doc`) contain **facts** (named values) and **rules** (expressions that may have `unless` clauses).
- There are **no functions, no side effects, no mutable state** during evaluation.
- **Types** include primitives (`boolean`, `number`, `scale`, `text`, `date`, `time`, `duration`, `ratio`) and user-defined types (e.g. `scale` with units like `eur`, `kilogram`).
- **Rule references** use `rule_name?` to refer to another rule’s result; **last matching `unless` wins**.

Lemma aims to be reliable enough for critical domains (e.g. aerospace, global regulations). That implies strict validation, deterministic execution, and a clear split between “invalid document” (errors) and “valid but rule doesn’t yield a value” (Veto).

**Lemma has no inline comments** (by design, for clarity and unambiguous parsing). Another benefit is that it reduces the risk of ambiguity and contradiction between comments and the actual rules. The only documentation syntax is an optional **commentary block** in triple quotes immediately after a `doc` declaration (see below).

---

## 2. Core guarantees (do not weaken these)

### 2.1 Validation before execution

- **Planning** (semantic + graph + execution plan) fully validates the document. Only if planning succeeds does the engine run evaluation.
- Invalid Lemma must be rejected with **LemmaError** (Parse, Semantic, Engine, CircularDependency, etc.). Never “try to run and see.”

### 2.2 Post-validation: evaluation is guaranteed to complete

- **After** a document has been successfully planned, evaluation is **guaranteed** to run to completion for the requested rules (modulo resource limits). The execution plan is self-contained and fully resolved.
- If something **impossible** happens during evaluation (e.g. missing proof node, wrong variant in a match that was supposed to be exhaustive), the program must **fail fast**: use `unreachable!()` or `panic!()`. Do **not** return a fallback value or “best guess.”

### 2.3 No silent defaults or heuristics

- **Silent defaults and heuristics are forbidden.** If the semantics don’t define a value, the code must either:
  - return a **LemmaError** (during parse/planning), or
  - return **Veto** (during evaluation, for domain-level “no value”), or
  - **panic / unreachable** when an invariant is violated.
- Never infer or guess to “keep going”; that undermines Lemma’s certainty.

### 2.4 Determinism

- **Execution plans** are deterministic: same document + same fact inputs ⇒ same evaluation order and same results (or same Veto).
- Evaluation must not depend on undefined order (e.g. iteration over unordered collections in a way that affects results). Rules are evaluated in **topological order** by dependency.

### 2.5 Veto is a result, not an error

- Rules that **cannot produce a value** (e.g. division by zero, or user-defined `veto "reason"`) yield **Veto**, not LemmaError.
- **OperationResult** is either `Value(LiteralValue)` or `Veto(Option<String>)`.
- Veto propagates to dependent rules only when the dependent rule **needs** the vetoed value; if an `unless` branch provides a value without evaluating the vetoed rule, that branch can still succeed. See `documentation/veto_semantics.md`.

### 2.6 No placeholders in Lemma code

- **Using placeholders in Lemma code is unacceptable.** Placeholders (e.g. dummy values, “TODO” literals, fake numbers, or placeholder text) undermine Lemma’s goal of **extreme certainty**: they make documents look valid while hiding missing or wrong logic.
- Lemma documents must contain **real, intended facts and rules**. When examples or tests need concrete values, use **actual** values that match the domain. If something is not yet defined, leave it out or fail validation—do **not** fill gaps with placeholders.
- In documentation, tests, or any `.lemma` content: **no placeholder data.** This is non-negotiable for reliability.

---

## 3. Pipeline and code layout

High-level flow:

1. **Parse** (`lemma/src/parsing/`)  
   - Grammar: `lemma/src/parsing/lemma.pest` (Pest).  
   - Produces AST; conversion to **LemmaDoc** (document AST) happens in the same parse pipeline (see `parse::parse_doc` and `parsing/ast.rs`).

2. **AST** (`lemma/src/parsing/ast.rs`)  
   - **LemmaDoc**, **LemmaRule**, **LemmaFact**, **Expression**, **ExpressionKind**, **FactValue**, **TypeSpecification**, etc.  
   - No evaluation here; this is the resolved document structure used by planning.

3. **Planning** (`lemma/src/planning/`)  
   - **Validation** (`validation.rs`): type/structure checks.  
   - **Graph** (`graph.rs`): builds dependency graph, resolves types and references, converts fact/rule references to **FactPath** / **RulePath**. Returns **Vec&lt;LemmaError&gt;** on failure.  
   - **Execution plan** (`execution_plan.rs`): builds **ExecutionPlan** from graph (topologically sorted rules, fact schema, fact values, sources).  
   - **Types** (`types.rs`): type registry and scale/unit resolution.  
   - Entry: `plan::plan(main_doc, all_docs, sources)` → `Result<ExecutionPlan, Vec<LemmaError>>`.

4. **Evaluation** (`lemma/src/evaluation/`)  
   - **Expression** (`expression.rs`): evaluates expressions against an **EvaluationContext**; produces **OperationResult** (Value or Veto).  
   - **Operations** (`operations.rs`): **OperationResult**, **OperationKind**, proof-related types.  
   - **Proof** (`proof.rs`): proof trees for explanations.  
   - **Response** (`response.rs`): **Response**, **RuleResult**, **Facts**.  
   - Only runs after a successful plan; any “impossible” state must panic/unreachable.

5. **Computation** (`lemma/src/computation/`)  
   - **Arithmetic** (`arithmetic.rs`): division by zero (and similar) returns **Veto**, not panic.  
   - **Comparison**, **datetime**, **units**: used by evaluation; must preserve “no silent defaults” and use Veto where specified.  
   - Scale comparison and conversion use **same scale family** (via `TypeExtends::Custom`’s `family` field and `LemmaType::same_scale_family`), not exact type equality, so types in the same extension chain (e.g. `type x = scale ...` and `type x2 = x ...`) are compatible.

6. **Engine** (`lemma/src/engine.rs`)  
   - **Engine** holds documents, sources, and **execution plans**.  
   - `add_lemma_code` → parse then `plan(...)`; on success, stores execution plan.  
   - `evaluate` runs the plan; no document parsing during evaluate.

7. **Errors** (`lemma/src/error.rs`)  
   - **LemmaError**: Parse, Semantic, Inversion, Runtime, Engine, MissingFact, CircularDependency, ResourceLimitExceeded, MultipleErrors.  
   - **LemmaResult&lt;T&gt;** = `Result<T, LemmaError>`.

---

## 4. Lemma syntax (quick reference and examples)

Lemma has **no inline comments** (`//` and `#` do not exist in the language). Use optional doc-level commentary in triple quotes after `doc name` when you need to document a document.

### Documents and commentary

```lemma
doc pricing

doc shipping_policy
"""
Shipping rules and fees.
Optional description in triple quotes only.
"""
```

### Facts

```lemma
fact quantity = 10
fact name = "Alice"
fact is_active = true
fact price = 99.50 eur
fact workweek = 40 hours
fact tax_rate = 21 percent
fact deadline = 2024-01-15
fact birth_date = [date]
fact age = [number -> minimum 0 -> maximum 120]
fact product = [text -> option "A" -> option "B"]
fact currency = doc base_types
fact other_doc.price = 15
```

Values: literals (number, text, boolean, date, duration, percent, number+unit), type annotation `[type]` or `[type -> ...]`, or `doc other_doc`. Bind a fact in another doc with `fact doc.name = value`.

### Types

```lemma
type money = scale
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0

type weight = scale
  -> unit kilogram 1.0
  -> unit gram 0.001
  -> minimum 0 kilogram

type status = text
  -> option "active"
  -> option "inactive"

type discount_ratio = ratio
  -> minimum 0
  -> maximum 1

type age = number
  -> minimum 0
  -> maximum 120

type currency from base_doc
type discount_rate from pricing -> maximum 0.5
```

### Rules and unless (last matching wins)

```lemma
rule discount = 0 percent
  unless quantity >= 10 then 10 percent
  unless quantity >= 50 then 20 percent
  unless is_vip then 25 percent

rule daily_fee = 0 eur
  unless book_type is "regular" then 0.25 eur
  unless book_type is "reference" then 0.50 eur

rule total = quantity * price? - discount?
```

### Veto (user-defined “no value”)

```lemma
rule validated_price = price
  unless price < 0 then veto "Price cannot be negative"

rule can_drive = is_adult? and has_license?
  unless license_suspended then veto "License suspended"
```

### Rule references and cross-document

```lemma
rule price_per_cup = base_price? * size_multiplier?
rule subtotal = price_per_cup? * number_of_cups
rule total = subtotal? - discount_amount?

fact membership = doc premium_membership
rule discount = monthly_spend * membership.discount_rate?
rule shipping_cost = 10
  unless monthly_spend >= membership.free_shipping_threshold? then 0
```

Use `rule_name?` in the same doc; use `doc_name.rule_name?` when the rule lives in another document (referenced via a `fact x = doc other_doc`).

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
- **Logical:** `and` `or` `not`
- **Conversion:** `value in unit` (e.g. `price in usd`, `duration in days`)
- **Math-like (prefix):** `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round` (parentheses optional)

See **documentation/reference.md** and **documentation/index.md** for full syntax and more examples.

---

## 5. Error handling rules for implementers

- **Parse/planning:** Invalid input ⇒ return **LemmaError** with clear, localized message (include source location where possible). Do not continue with a “best effort” plan.
- **Evaluation:**  
  - Domain failures (e.g. division by zero, user veto) ⇒ **OperationResult::Veto(...)**.  
  - Bug or invariant violation (e.g. missing node, wrong enum variant) ⇒ **panic!()** or **unreachable!()** with a message that includes context (e.g. “BUG: …”).
- **No silent fallbacks:** Do not use default values or heuristics to avoid failing; that violates Lemma’s guarantee of certainty.

---

## 6. Testing and development

- **Use TDD.** Failing tests define missing or broken behaviour; do not hide or remove them to make the suite pass.
- **Unit tests** live in the same module (to allow testing private functions); **integration tests** in `lemma/tests/`.
- Run tests with **cargo nextest**, not `cargo test`.
- When adding features, add tests that lock in the intended behaviour (including Veto propagation and error cases).

---

## 7. Key documentation

| Resource | Purpose |
|----------|--------|
| **README.md** | Project overview, quick start, features |
| **documentation/index.md** | Language concepts, documents, facts, rules |
| **documentation/reference.md** | Operators, types, literals, syntax |
| **documentation/veto_semantics.md** | When Veto applies and propagates |
| **documentation/examples/** | Example `.lemma` files |
| **.cursor/rules/** | Project rules (e.g. tests, TDD) |

---

## 8. Summary for agents

- Lemma is **strictly typed, pure, and function-free**; its goal is **extreme certainty** for documents that pass validation.
- **Planning** validates everything and returns **LemmaError** on any misuse; **after** planning, evaluation is **guaranteed** to run to completion (no “maybe” evaluation).
- **Unexpected state during evaluation** ⇒ crash with `unreachable!()` or `panic!()`. **No silent defaults or heuristics.**
- **Execution plans** are **deterministic**; same inputs ⇒ same outputs (or same Veto).
- **Veto** is the way “this rule has no value” is expressed (e.g. division by zero, user `veto "..."`); it is not an error. Propagate Veto according to `veto_semantics.md`.
- When editing the codebase, preserve these guarantees, use **LemmaError** for invalid Lemma, use **Veto** for domain-level “no value,” and **panic/unreachable** for bugs. Prefer **cargo nextest** and TDD as in the project rules.
- **Never use placeholders in Lemma code** (no dummy values, TODO literals, or fake data in `.lemma` files, docs, or tests). Placeholders destroy certainty; use real, intended values or omit/fail instead.
