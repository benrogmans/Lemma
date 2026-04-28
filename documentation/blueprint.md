---
layout: default
title: Lemma Blueprint
---

# Lemma Blueprint

This document is the **overarching blueprint** for Lemma: why it exists, how it is built, and what it offers. It states **how Lemma must behave**—normative semantics, not a record of a particular build. It complements [index.md](index.md), the [README](../README.md), and [reference.md](reference.md).

---

## 1. What Lemma is for (epic user stories)

These are the outcomes Lemma is designed to enable—not a feature checklist, but the **jobs** the system is meant to do.

### 1.1 Single source of truth for policy

- **As a business or compliance owner**, I want regulations, contracts, pricing, and eligibility written in a form my team can read **and** that production systems evaluate **identically**, so policy and code cannot drift silently.
- **As an auditor**, I want every decision traceable to named data and rules with a stable, repeatable evaluation story, so reviews and disputes do not depend on digging through imperative code.

### 1.2 Time-aware rules

- **As an operator**, I want rules that **change on a calendar** (new law, new price list) without redeploying application code, and I want evaluations pinned to "what was true at instant *T*," so backdating and what-if analysis are honest.
- **As an integrator**, I want later `effective_from` versions to **supersede** earlier ones along a timeline, with clear semantics for which text applies when.
- **As a modeller of composed policies**, I want **dependencies that evolve on their own timeline** to remain **safe**: if my spec spans multiple effective ranges, Lemma should either prove the contract I rely on from each dependency is **unchanged** across those ranges, or **refuse to plan** until I publish a **new temporal version of my spec** that explicitly matches the new reality—not a silent mismatch at evaluation time.

### 1.3 Certainty over heuristics

- **As a product owner**, I want **deterministic** outcomes: same spec, same data, same effective time → same result (or the same explicit "no value"), not model drift or hidden defaults.
- **As an engineer**, I want **invalid rules rejected before execution**, and **no surprise type or operator errors at runtime** after a successful plan—so invalidity is an **error** at plan time; domain-level "no value" is a **veto** at evaluation time (see below).

  **Rule results (`OperationResult`)** are either a **value** (a typed literal) or a **veto**. A **veto** is not a bug and not a planning error: it means the rule has **no value** for domain reasons the spec allows—e.g. missing data, division by zero, date overflow, or `veto "reason"` in the source. Vetoes can be **optional** (no message) or carry a **string**; callers and APIs surface them as **first-class results** alongside values. Downstream rules that **need** a vetoed operand may veto in turn; branches that avoid needing that value (e.g. a matching `unless`) can still succeed. See [veto_semantics.md](veto_semantics.md).

### 1.4 Composition and reuse

- **As a domain modeller**, I want to **compose** small rule modules (specs), reference other specs' data and rules, and optionally pull shared definitions from a **registry**, so large policy surfaces stay modular and reusable. **Composition is how temporality is wired across specs** (§2.1): a consumer depends on another spec that may itself have several effective-dated bodies.

### 1.5 Integration without lock-in

- **As a developer**, I want to embed the same logic in a **CLI**, **HTTP service**, **browser (WASM)**, **LSP**, or **MCP** clients, with **OpenAPI** and stable bindings, so Lemma fits existing stacks without a mandatory database or proprietary runtime.

### 1.6 Analysis beyond forward evaluation (direction)

- **As an analyst**, I want to ask **constraint-style** questions (e.g. what inputs would satisfy a target outcome) where the engine supports it, complementing forward rule evaluation—aligned with Lemma's ongoing **inversion** and **explainability** direction (see [README](../README.md) "Direction").

---

## 2. How Lemma is implemented (architecture)

Lemma is not "interpret rules line by line in order." It is a **compiled pipeline**: text → AST → **planning** (static analysis) → **execution plan** → **evaluation**. The public story in [AGENTS.md](AGENTS.md) is that **validation happens before execution**; evaluation after a successful plan is **guaranteed to complete** (subject to resource limits), and **bugs** surface as panics/`unreachable!`, not as guessed values. **§2.1** is the architectural cornerstone: how **time** and **composed dependencies** fit together and what **interface compatibility** means in planning.

### 2.1 Temporality, composition, and dependency interfaces (cornerstone)

A **spec** is a namespace of **data** (inputs / parameters), **rules** (derived values), and **user-defined types**. **Data imports** (`data money from dep_spec`, or from a registry bundle) tie the consumer to another spec's **type definitions**; they follow the same **temporal range** and **reference** rules as `with` references (see below). **Slice-interface** validation ensures the **data, referenced rules, and named types** the consumer actually uses stay **compatible** across the consumer's temporal slices.

Lemma's approach to **time** and **composition** is one story: every context has a **temporal range** that decides which temporal versions of dependencies are visible; composable references (data, rules, **and** imported types) express that wiring; the engine checks **contract stability** across slices when an **unqualified** reference spans multiple dependency versions.

#### Temporal range

Every planning / evaluation context carries a **temporal range**. It bounds **which** temporal versions of a referenced spec name are in play.

- **API level** (CLI `run` / `schema`, HTTP, etc.): the range is **(-inf, +inf)** for **which loaded sources** matter—no spec body is excluded by the API's own clock; **point-in-time evaluation** still picks bodies via **`--effective`** / **Accept-Datetime** at that instant.
- **Spec level:** each temporal version of a spec has range **`[effective_from, superseded)`**, where **superseded** is the next **`effective_from`** on the **same** name, or unbounded if none. That interval is the spec's **temporal range** when planning that consumer.

#### Temporality (only versioning scheme)

- The same spec **name** may appear **multiple times** with different **`effective_from`** datetimes (`spec Name YYYY-MM-DD`, etc.). Each declaration is **immutable**; you add a new body on the timeline rather than editing history in place.
- At a given **instant**, **which** declaration applies for a name is determined by **effective datetime**. There is no separate parallel "version tag" track beside this timeline.

#### Composability and how references resolve

- Specs **reference** other specs (`with dep: other`) and **import types** from them (`data name from dep_spec`) or from registry sources. **Hierarchical** names (e.g. `company/policies/pricing`) organize namespaces; they are not a second versioning mechanism.

**Unqualified reference** (`with dep` with **no** datetime on the reference): the consumer sees **every** temporal version of **`dep`** whose **temporal range** **intersects** the **consumer's** temporal range (§2.1 **Temporal range**). That intersection is what can yield **multiple** resolved bodies over the consumer's lifetime.

**Qualified reference** (`with dep 2025-06-01`): **point-in-time**. The engine selects the **one** temporal version of **`dep`** active at that instant and resolves **`dep`'s entire transitive subtree** at **that same instant**—not over the consumer's range.

#### Temporal slicing (consequence of unqualified references)

When the consumer uses **unqualified** spec or type references, planning may see **several** dependency **`effective_from`** boundaries inside the consumer's range. **Temporal slices** are the **partition** of the consumer's **`[effective_from, superseded)`** at those boundaries:

- A **boundary** is inserted at every **`effective_from`** of an **implicitly referenced** dependency (spec ref or data import—**transitively**) that falls **strictly inside** the consumer's range, for references resolved **per slice** (unqualified form).
- Between boundaries, the **transitive dependency tree** resolves to one **consistent set** of effective-dated bodies; with **no** such references or **no** boundaries in range, there is **one** slice covering the full consumer range. (See `LemmaSpecSet::effective_dates` in `engine/src/planning/spec_set.rs` and the per-slice plan loop in `engine/src/planning/mod.rs::plan`.)

**Per slice:** For each slice, the engine builds a **dependency graph and execution plan** using the slice's **start instant** (`slice.from`) as the resolution time for **unqualified** links. **Values** of data and rules can **change from slice to slice** while the **logical links** stay the same—**interface** sameness is what `validate_dependency_interfaces` checks when there is more than one dep slice. **Qualified** references do not expand the consumer's slice structure from that edge: they fix resolution to their written instant.

#### Dependency interface compatibility

- Each referenced spec exposes its **interface schema** (data with types, rules with result types, named types) via `ExecutionPlan::interface_schema`. For every consumer slice, planning checks that all dep slices intersecting the consumer's range expose **type-compatible** schemas — see `validate_dependency_interfaces` in `engine/src/planning/discovery.rs` and `SpecSchema::is_type_compatible` in `engine/src/planning/execution_plan.rs`.
- **Compatible:** every overlapping pair of dep slices agrees on the type of every name they both expose; either slice may add new names that the other lacks (added rules / data / named types are unused-by-default and do not break the contract).
- **Incompatible:** the same name has different types in different slices (type change, scale-family retargeting, options change, …). Planning **fails** with a validation error naming the dependency and consumer; the remedy is to introduce a **new temporal version of the consumer** whose rules explicitly align with the dependency's evolution. Note: a name **removed** in one slice but **used** by the consumer surfaces as a per-slice graph-build error (missing data/rule), not as the cross-slice interface check.

#### Temporal coverage (separate check)

- Independently, **unqualified** dependencies must **cover** the consumer's **temporal range** with **no gaps** (an effective-dated body of the dependency must exist for every instant the consumer needs under **unqualified** resolution). **Coverage** is about **presence** on the timeline; **interface** validation is about **sameness of types** across the consumer's slices. Coverage lives in `LemmaSpecSet::coverage_gaps` (`engine/src/planning/spec_set.rs`); interface validation lives in `engine/src/planning/discovery.rs`.

  **Example (coverage vs interface):** Suppose `app` is effective from **2025-01-01** and references `dep`:

  ```lemma
  spec app 2025-01-01
  with d: dep
  rule x: d.rate

  spec dep 2025-07-01
  data rate: 10
  ```

  There is **no** `dep` body active in **early 2025**—only from July onward—so the consumer's range includes instants where **`dep` does not exist**. Planning reports a **temporal coverage** error (gap), not a slice-interface error. **Fix:** add an earlier `spec dep` (or an unbounded-first `spec dep` with a base `rate`), **or** move `app`'s `effective_from` to when `dep` first exists.

  If instead `dep` had a row for January and a row for July with **different** `rate` **types** or a **removed** `rate` in one slice, coverage might succeed but **slice-interface** validation could still **fail**—that is the separate "contract unchanged across slices" check.

  **Qualified reference:** `with d: dep 2025-08-01` resolves **`dep`** (and **`dep`'s** transitive dependencies) at **2025-08-01** only. The consumer's own temporal range no longer drives which **`dep`** bodies appear for that edge; slice boundaries from **`dep`'s** timeline still apply to **other** unqualified links in the graph as usual.

### 2.2 Declarative rules: default / unless

- Rules use a **default expression** plus **`unless` … `then`** branches. **Last matching `unless` wins**, matching how many policies are written in prose ("normally X, unless Y, unless Z…").
- This yields **structural determinism** in rule selection: no reliance on incidental evaluation order inside a single rule's branches.

### 2.3 Strict typing and static planning

- Data and rules carry **types**: primitives (`number`, `text`, `boolean`, `date`, `time`, `duration`, `ratio`, `scale`) and **user-defined types** (units, constraints, options).
- **Data imports** and **registry-backed types** integrate shared definitions.
- **Planning** builds a dependency graph, runs **semantic validation**, resolves **spec references** under each context's **temporal range** (§2.1), builds **temporal slices** where **unqualified** references warrant them, validates **temporal coverage** and (when a spec has multiple slices) **per-dependency slice interfaces**, and produces an **execution plan**. **Type mismatches and invalid operations are errors at plan time**, not surprise runtime failures after planning.

### 2.4 Immutability on the timeline (conceptual)

- Published spec bodies are **immutable**: change is always a **new** `effective_from` declaration; you do not edit an older block in place.
- At instant *T*, the active spec text is **exactly** what appears in the **authored** `spec …` block(s) the resolution rules pick for that instant—**not** a paraphrase, **not** filler invented at runtime. A newer `effective_from` **replaces** older material for its range by explicit written supersession, not by hidden merge rules. The engine **selects** among declarations you loaded; it does not author new policy. §2.1 ties timelines to **composition** and **interface** checks.

### 2.5 Evaluation: values, vetoes, explanations

- **OperationResult** is either a **value** or a **veto** (domain-level "no result"), not a generic planning error.
- **Veto** covers user `veto "reason"`, division by zero, missing data, etc.; propagation follows documented [veto semantics](veto_semantics.md).
- Optional **explanations** expose **why** a result was produced (operation records), supporting audit narratives.

### 2.6 Registry vs engine boundary

- The **engine does not fetch the network**. `@…` registry references are **resolved externally** (CLI `lemma get`, embedders calling `resolve_registry_references`, WASM with `fetch`), then **loaded** as sources. This keeps evaluation **pure** and deployments **predictable**.

### 2.7 Implementation stack (codebase)

- **Core engine (Rust):** parsing, planning, evaluation, formatting, serialization, **inversion** APIs, WASM surface.
- **CLI:** `run`, `schema`, `list`, `format`, `get`, `server`, `mcp`, etc. ([CLI.md](CLI.md)).
- **OpenAPI** for HTTP evaluation and discovery.
- **LSP** for diagnostics, formatting, and workspace-aware validation.
- **npm / WASM** package for browser and Node ([wasm.md](wasm.md)).

---

## 3. Feature catalog (what Lemma offers)

This section is the **extensive** inventory of capabilities—language, engine, and tooling. It describes **what Lemma is**, not a release manifest.

### 3.1 Language: data and rules

- **Data** with literals or **type annotations** only (`data x: type`).
- **Rules** as expressions; **rule references** and **data references** unified by name resolution.
- **`unless` / `then`** chains with **last matching wins**.
- **`veto`** and optional veto messages for domain-level failure.
- **Piecewise / conditional** value selection aligned with `unless` semantics (see examples and reference).

### 3.2 Expressions

- **Arithmetic:** `+`, `-`, `*`, `/`, `%`, `^`.
- **Comparisons:** `>`, `<`, `>=`, `<=`, `is`, `is not`.
- **Logical:** `and`, `not` ([reference.md](reference.md)). Note that there is no `or` keyword as unless clauses accommodate such logic.
- **Math:** `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round` (prefix; parentheses optional).
- **`in` for unit conversion** (durations, scale units, number↔ratio where defined).

### 3.3 Types

- **Primitives:** boolean, number, scale, text, date, time, duration, ratio.
- **User-defined types** with **commands** (`unit`, `decimals`, `minimum`, `maximum`, `option`/`options`, `default`, `help`, etc.—see [reference.md](reference.md)).
- **Data imports** from other specs or registry bundles (`data money from @…`).
- **Inline type constraints** on data where supported.

### 3.4 Dates, times, and calendars

- **ISO 8601** dates and datetimes; **duration** literals with built-in units.
- **Date arithmetic** and comparisons; **timezone** handling where documented in tests/reference.
- **Sugar** such as `in_past`, `in_calendar_year`, etc., where enabled (see reference and examples).

### 3.5 Composition and naming

- **Spec references** (`with parent` / `with child: parent`) compose policies from smaller specs; this is the **mechanism** by which temporality is chained (§2.1). **Unqualified** `with parent` resolves **all** temporal versions of `parent` that **intersect** the referencing spec's **temporal range**; **qualified** `with parent 2025-06-01` resolves `parent` and its subtree at **that instant** only.
- **Hierarchical** spec names (paths / segments) for organization.
- **Registry references** `@org/path/spec` for shared libraries ([registry.md](registry.md)).

### 3.6 Temporal versioning and dependency contracts

- **Multiple `spec Name <effective>`** blocks for one logical name; at an instant, **which** body applies is determined by **effective datetime** only.
- **Temporal range:** each spec temporal version has range **`[effective_from, superseded)`**; the **API** context treats loaded material over **(-inf, +inf)** (§2.1).
- **CLI / API:** `--effective` and **Accept-Datetime** (HTTP) for point-in-time evaluation of the **root** spec.
- **Temporal slicing:** arises when a consumer uses **unqualified** references and **multiple** dependency **`effective_from`** boundaries fall inside the consumer's range; each slice resolves **unqualified** edges at the slice start instant. **Slice-interface validation** ensures the **data, referenced rules, and named types** the consumer needs from each dependency are **unchanged** across those slices, or planning errors (see §2.1).
- **Temporal coverage:** **unqualified** dependencies must **cover** the consumer's temporal range without gaps (separate from interface equality).

### 3.7 Planning and determinism

- **Dependency graph** and **topological** evaluation order.
- **Static analysis** rejecting invalid programs before `run`, including **temporal coverage** and **slice-interface** checks for composed specs (§2.1).
- **Resource limits** (file size, expression depth/count, etc.) to bound work ([resource limits](AGENTS.md) patterns in engine).

### 3.8 Evaluation outcomes

- **Values** as typed literals.
- **Veto** as first-class outcome with propagation rules documented under [veto_semantics.md](veto_semantics.md).
- **Explanations** (operation traces) when requested (`-x` / API flags as supported).

### 3.9 Inversion (constraint solving)

- Engine exposes **inversion** types (`Target`, `Domain`, `InversionResponse`, etc.—see `lemma` crate exports). Use cases: "what values satisfy this rule's target?" subject to **supported** expression shapes; unsupported shapes yield a clear **unsupported** outcome rather than silent wrong answers.

### 3.10 Formatting

- **`lemma format`** and library **format_source** / **format_specs** for consistent layout of `.lemma` sources.

### 3.11 CLI

- **run** with data, rule filters, JSON output, interactive mode, effective time.
- **schema**, **list**, **show**, **get** (registry deps), **format**.
- **server** with OpenAPI, docs route, watch mode.
- **mcp** for assistant integration.

### 3.12 Embeddings

- **HTTP server** with documented routes and OpenAPI.
- **WebAssembly** + JS API for browser/Node ([wasm.md](wasm.md)).
- **LSP** for editors ([wasm.md](wasm.md) LSP client notes).

### 3.13 Interop and documentation

- **OpenAPI** generation for REST evaluation.
- **MCP** for tool-using agents.
- **Examples** under `documentation/examples/` and CLI integration examples.

### 3.14 Direction (not exhaustive; see README)

- Deeper **inversion** coverage and UX.
- **Tables** as a first-class type for data-heavy rules.
- **Performance** work to stay competitive with hand-tuned code paths.

---

## 4. How to read this blueprint

- **Goals (§1)** — why Lemma exists for people and systems (including **safe composition across time**).
- **Implementation (§2)** — how correctness is structured: **§2.1** is the cornerstone (**temporal range → references → slicing → slice interfaces**); then planning-first evaluation, vetoes, registry boundary.
- **Features (§3)** — normative **capability** of Lemma; detailed syntax and flags live in [reference.md](reference.md) and [CLI.md](CLI.md).

For **non-negotiable engineering guarantees** (Error vs Veto vs panic, determinism, no silent defaults), see [AGENTS.md](AGENTS.md).
