# Composing specs with `uses`

A Lemma source file defines one or more **specs**. Each spec is a namespace of **data** and **rules**. With **`uses`**, you import another spec under an alias and refer to its members through that alias:

- **Data** — `alias.field` in expressions, or `with alias.field: …` to supply or override values in the consumer spec.
- **Rules** — `alias.rule_name` in expressions (the engine evaluates the dependency rule in the composed graph).
- **Types** — `data name: alias.TypeName` when the dependency exposes a named type (qualified parent on a `data` declaration).

This page explains unpinned vs pinned imports, temporal versions of the same name, planning checks, and how evaluation picks bodies at a point in time.

For syntax see [Spec references (`uses`)](reference.md#spec-references-uses). For registry packages see [registry.md](registry.md).

---

## Importing another spec

```lemma
spec premium_membership

data discount_rate: 10%

rule free_shipping_threshold: 50

rule monthly_bonus_points: 100


spec membership_benefits

uses membership: premium_membership

data monthly_spend: 150

rule discount: monthly_spend * membership.discount_rate

rule bonus_points: membership.monthly_bonus_points
```

- **`uses membership: premium_membership`** — bind alias `membership` to spec `premium_membership`.
- **`uses premium_membership`** — implicit alias: last path segment of the target name.
- **`membership.discount_rate`** — read data from the imported spec.
- **`membership.monthly_bonus_points`** — use a rule from the imported spec.

---

## Repositories (namespaces)

A file may declare **`repo`** blocks. Each repo is a **namespace**: two repos can both define `spec invoice` without colliding.

```lemma
repo accounting

spec invoice

data total: 1


spec billing

uses inv: accounting invoice

rule out: inv.total
```

Cross-repo targets use a repo qualifier on the `uses` line (`accounting invoice`). When you **`run`** a spec from the workspace (main) repository, use its unqualified name; the CLI does not pick between two loaded specs with the same name in different repos.

---

## Temporal versions (effective dates)

The same **spec name** may appear several times with different **effective** datetimes (`spec Name 2025-05-01`). Each declaration is **immutable**; you add a new row on the timeline instead of editing history in place.

```lemma
spec policy

rule discount: 10


spec policy 2025-05-01

rule discount: 25
```

**Which row applies** at evaluation time is determined by the **`--effective`** instant (CLI) or **Accept-Datetime** (HTTP) for the spec you run.

---

## Unpinned vs pinned `uses`

| Form | Meaning |
|------|--------|
| **`uses dep`** | **Unpinned.** Every temporal version of `dep` whose range **intersects** this spec’s range can matter. Planning may split this spec into **temporal slices** at dependency `effective_from` boundaries inside that range. Values resolved through the import can change when `dep` gains a new row. |
| **`uses dep 2025-06-01`** | **Pinned to an instant.** One body of `dep` active at that datetime, including its transitive imports. Later rows of `dep` do not affect this edge. (Not the same as a qualified **type** parent `alias.TypeName` on data.) |

### Unpinned: values follow the timeline

```lemma
spec policy

rule discount: 10


spec policy 2025-05-01

rule discount: 25


spec shop 2025-01-01

uses p: policy

rule d: p.discount
```

Evaluating **`shop`** in early 2025 yields `d = 10`; from May 2025 onward, `d = 25`. The `uses` line is unchanged; the **resolved policy body** changes at the slice boundary.

A consumer with **no** effective date on its `spec` line (origin) still gets slices when a dependency’s `effective_from` falls inside its range.

### Pinned: freeze a dependency at one instant

```lemma
spec finance

data money: quantity
  -> unit eur 1.00


spec finance 2025-07-01

data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91


spec shop 2025-01-01

uses f: finance 2025-02-01

data money: f.money
data price: money

rule doubled: price * 2
```

`shop` keeps the **February 2025** finance shape (EUR only) even when you evaluate in September 2025. Pinning locks a tariff or regulation snapshot into the consumer.

---

## Planning checks on composed specs

Before `run`, planning validates the dependency graph. Two temporal failures authors see most often:

### Temporal coverage (gaps)

**Unpinned** `uses dep` requires `dep` to exist for **every** instant in the consumer’s temporal range. If the consumer starts in January but `dep` only exists from July, planning fails (no active version / not active at that instant).

**Fix:** add an earlier `spec dep` row, move the consumer’s `effective_from` later, or **pin** `uses dep 2025-08-01` so the consumer no longer requires `dep` to cover its whole range.

### Interface compatibility (contract changes)

When unpinned imports span **multiple** rows of the same dependency, every row the consumer touches must expose **compatible types** for the same names (rule result types, data types, compatible quantity units, etc.). New names only in a later row are fine if the consumer does not use them.

**Incompatible example:** `money` gains a `usd` unit in a later `finance` row while `shop` still has unpinned `uses finance` and `data price: finance.money`. Planning reports that the dependency **changed its interface between temporal slices**.

**Fix:** pin `uses f: finance 2025-02-01`, or add a new temporal row of `shop` written for the new finance body.

**Coverage** is presence on the timeline; **interface** validation is type sameness across slices the consumer needs.

---

## Same name, different bodies

You may import an **earlier** temporal row that shares your base name:

```lemma
spec finance 2026-01-01

data rate: 1


spec finance 2027-01-01

uses f26: finance 2026-01-01

rule ok: f26.rate
```

Planning **rejects** a `uses` edge that resolves to the **same** spec body (self-reference):

```lemma-skip
spec finance

uses finance


spec finance 2026-01-01

uses finance 2026-01-01
```

A later row must not use unpinned `uses finance` when that resolves to itself at that row’s instant. Pin an earlier row (`uses f26: finance 2026-01-01`) instead.

Cycles across temporal rows (2026 → 2027 → 2026) are rejected as **dependency cycles**.

---

## Setting data on an import (`with`)

`uses` registers an import; it does not set runtime values on the dependency. Use **`with alias.field: …`** to assign a literal or reference to a **data slot declared on the imported spec**:

```lemma
spec inner

data x: number
  -> default 1


spec outer

uses i: inner
with i.x: 42

rule r: i.x
```

- **`with i.x: 42`** — sets **data** `x` on `inner` to `42`.
- **`with i.x: i`** — error: `i` is a spec reference, not a value.
- **`with copy: i.x`** — parse error: `with` must use an import path on the left (`with i.x: …`), not a local name.

Runtime inputs to `run` can still override bound import paths (e.g. `i.x`) where planning allows.

To read import data without overriding it, reference the path in a rule: `rule r: i.x`.

---

## Registry and shared libraries

External packages use **`@org/path`** qualifiers ([registry.md](registry.md)). The engine does not fetch the network; load sources with `lemma fetch` (or your embedder), then `uses iso: @iso/countries alpha2` resolves like any other repo-qualified import.

---

## Evaluating a composed spec

You always **`run`** (or call the API for) a **named root spec** in a repository, with an effective instant:

```bash
lemma run membership_benefits --effective 2025-03-01
```

- The engine selects the **consumer’s** temporal slice that contains that instant.
- **Unpinned** imports: paths such as `p.discount` use dependency bodies resolved for that slice (slice start instant for planning).
- **Pinned** imports: the dependency body stays at the pinned instant; eval date does not move that edge (unless unpinned links remain inside the dependency’s own subtree).

Use **`lemma schema`** with the same `--effective` to see required inputs for that slice.

---

## Quick decision guide

| Goal | Pattern |
|------|---------|
| Track compatible dependency rows across the consumer’s lifetime | `uses dep` (unpinned) |
| Lock a regulation / tariff / schema at a known date | `uses dep 2025-06-01` |
| Import an earlier row of the same spec name | `uses prev: finance 2026-01-01` |
| Shared types from a library | `uses iso: @iso/countries alpha2` and `data x: iso.code` |
| Set data on an imported spec | `with alias.field: value` |
| Read import data in a rule | `rule r: alias.field` |

If planning fails, check whether the message is **coverage**, **interface**, **self-reference**, or **cycle** — each remedy is different above.
