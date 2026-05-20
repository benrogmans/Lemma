---
layout: default
title: Language Reference
---

# Lemma Language Reference

Quick reference for all operators and types in Lemma.

## Operators

### Arithmetic
| Operator | Description | Example |
|----------|-------------|---------|
| `+` | Addition | `price + tax` |
| `-` | Subtraction | `total - discount` |
| `*` | Multiplication | `price * quantity` |
| `/` | Division | `total / count` |
| `%` | Modulo | `value % 10` |
| `^` | Exponentiation | `base ^ exponent` |

### Comparison
| Operator | Description | Example |
|----------|-------------|---------|
| `>` | Greater than | `age > 18` |
| `<` | Less than | `price < 100` |
| `>=` | Greater or equal | `score >= 70` |
| `<=` | Less or equal | `weight <= 50` |
| `is` | Equal | `status is "approved"` |
| `is not` | Not equal | `status is not "cancelled"` |
| `is veto` | Operand has no value | `validated_price is veto` (boolean; same as `veto is validated_price`) |
| `is not veto` | Operand has a value | `validated_price is not veto` |

Bare `veto` in `is veto` is not `veto "message"` (that form is only a rule/unless **result**). See [veto_semantics.md](veto_semantics.md).

### Logical
| Operator | Description | Example |
|----------|-------------|---------|
| `and` | Logical AND | `is_valid and not is_blocked` |
| `not` | Logical NOT | `not is_suspended` |

### Mathematical
| Operator | Description | Example |
|----------|-------------|---------|
| `sqrt` | Square root | `sqrt(value)` or `sqrt value` |
| `sin` | Sine | `sin(angle)` or `sin angle` |
| `cos` | Cosine | `cos(angle)` or `cos angle` |
| `tan` | Tangent | `tan(angle)` or `tan angle` |
| `log` | Natural logarithm | `log(value)` or `log value` |
| `exp` | Exponential | `exp(value)` or `exp value` |
| `abs` | Absolute value | `abs(value)` or `abs value` |
| `floor` | Round down | `floor(value)` or `floor value` |
| `ceil` | Round up | `ceil(value)` or `ceil value` |
| `round` | Round nearest | `round(value)` or `round value` |

Note: Mathematical operators are prefix operators, not functions. Parentheses are optional.

### Unit Conversion
| Operator | Description | Example |
|----------|-------------|---------|
| `as` | Convert units | `elapsed as hours`, `price as usd` |

The `as` operator converts between units:
- **Quantity types** (including **trait duration** quantities for time periods): units must be declared on the type (`unit` / `trait duration` with canonical `second`)
- **Number to ratio**: `0.5 as percent` converts to `50 percent`

```lemma
data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91

data price: 100 eur

rule price_usd: price as usd
```

```lemma
uses lemma si

data workweek: si.duration
  -> default 40 hours

rule workweek_days: workweek as days
```

Unit conversion explanations (`as`) expose ordered `conversion_steps` on each `unit_conversion` computation node in the explanation tree (JSON/WASM/API). Each step has a `role`: `outcome` (converted result), `rule` (unit equivalence such as `1 kilogram is 1000 gram`, or a range span such as `2024-06-15 − 2024-06-01 = 14 days`), then `source` (what was converted, e.g. `The quantity of mass is 2 kilogram`). Clients render these steps; the CLI is one renderer. Arithmetic and comparison nodes keep `expression` / `original_expression`; `unit_conversion` nodes use `conversion_steps` only.

Explanation JSON is a **tree**: walk `operands`, `conversion_steps`, and `expansion` on `rule_reference` nodes. Do not rely on a single top-level `expression` for `unit_conversion` nodes.

**Display unit conversion** (`lemma run --as`, HTTP `as_units`, `rule_result_units` in WASM/API) changes only the **displayed** `result` on that rule (and `explanation.result` to match). `explanation.tree` stays the evaluation audit in computed units (in-rule `as`, arithmetic, data bindings). Dependent rules still see the unconverted value. HTTP: start the server with `--explanations` and send `x-explanations: true` on evaluate requests.

## Spec References (`uses`)

Reference other specs with the `uses` keyword. For how unpinned vs pinned imports, temporal slices, coverage, and interface checks fit together, see [Composing specs](spec_composability.md).


- `uses spec_name` — alias defaults to the last path segment of the target name.
- `uses alias: spec_name` — explicit alias.
- `uses spec_name 2025-01-01` — temporal pin (ISO datetime or bare year `YYYY` → Jan 1 00:00).
- `uses a, b, c` — comma-separated bare imports (no aliases, no temporal pins).

Comma-separated form is for quick bare imports only. For aliases or temporal pins, use separate `uses` lines.

Spec names cannot contain a period. Versioning is **temporal only** — multiple rows of the same name with different `effective_from` datetimes.

### Temporal versions

The same spec **name** may appear several times with different effective datetimes. Each row is immutable; you add a new row on the timeline instead of editing history in place.

```lemma
spec pricing

data base_price: 100 eur


spec pricing 2025-01-01

data base_price: 120 eur


spec order

uses p: pricing

rule total: p.base_price
```

Evaluating **`order`** before May 2025 uses `base_price: 100`; from 2025 onward, `120`. See [Composing specs](spec_composability.md) for unpinned vs pinned imports.

### Pinning and evaluation instant

| Mechanism | Syntax | Effect |
|-----------|--------|--------|
| **Spec row** | `spec pricing 2025-01-01` | Declares a body effective from that datetime |
| **Pinned import** | `uses f: finance 2025-06-01` or `uses f: finance 2025` | Locks the dependency (and its transitive imports) to that instant |
| **Run instant** | `lemma run pricing --effective 2025-03-01` (CLI) or **Accept-Datetime** (HTTP) | Picks which temporal row of the **root spec** is active |

Bare year on a pin (`2025`) means that year's Jan 1 00:00, same as datetime literals.

### Self-reference restriction

A spec may depend on an **earlier temporal body with the same base name** when the
reference resolves to a **different** `effective_from` row (planning compares spec
bodies by identity, not by bare name):

```lemma
spec finance 2026-01-01
data rate: 1

spec finance 2027-01-01
uses finance 2026-01-01
rule ok: finance.rate
```

Both `uses finance 2026-01-01` (implicit alias) and `uses prev: finance 2026-01-01`
(explicit alias) are valid. Qualified access uses the import alias (`finance.rate` or
`prev.rate`).

Planning **rejects** a reference that resolves to the **same** spec body — for example
`spec finance` with `uses finance`, or `spec finance 2026-01-01` with
`uses finance 2026-01-01`. Dependency cycles across temporal rows (for example 2026
depending on 2027 while 2027 depends on 2026) are rejected as spec dependency cycles.

### Registry references

Registry references use the `@` prefix:

```lemma
spec ledger_spec

uses fin: @lemma/std/finance

data ledger: fin.Money
```

## Primitive types

Lemma provides these primitive types:

- **`boolean`** - true/false values
- **`number`** - dimensionless numeric values (no units)
- **`number range`** - half-open numeric intervals
- **`quantity`** - numeric values with units (mass, money, **time periods** via `-> trait duration`, etc.)
- **`quantity range`** - half-open intervals with quantity endpoints in one unit family
- **`text`** - string values
- **`date`** - ISO 8601 dates
- **`date range`** - half-open date/datetime intervals
- **`time`** - time values
- **`ratio`** - proportional values (percent, permille)
- **`ratio range`** - half-open ratio intervals
- **`calendar range`** - half-open intervals in calendar units (`years`, `months`); see [Ranges](#ranges)

Numbers are stored and computed as **exact rationals** (ℚ); API output is a **decimal string**. See [Numeric precision](numeric_precision.md).

## Ranges

Ranges express **half-open** intervals: **lower inclusive, upper exclusive**. Containment uses `in`:

```lemma
uses lemma si

data age:   25 years
data score: 50
data event: 2024-03-15

rule adult: age in 18 years...67 years

rule in_band: score in 0...100

rule in_period: event in 2024-01-01...2024-06-15
```

At the upper bound, `in` is false (`67 years` is not inside `18 years...67 years`).

### Range kinds

| Type | Endpoints | Typical use |
|------|-----------|-------------|
| **`date range`** | Dates or datetimes (`2024-01-01...2024-06-15`) | Employment periods, billing windows |
| **`number range`** | Dimensionless numbers (`0...100`) | Scores, tiers |
| **`quantity range`** | Quantities in one unit family (`30 kilogram...35 kilogram`) | Weight bands, duration bands (with `trait duration`) |
| **`ratio range`** | Ratios (`0%...50%`) | Allowed discount bands |
| **`calendar range`** | Calendar units only (`18 years...67 years`, `1 year...2 years`) | Age bands, policy windows in years/months |

**`calendar range`** endpoints must use **calendar** units (`years`, `months`). Do not mix calendar and duration units in one literal (`12 years...7 days` is a planning error). Do not mix dates and calendar endpoints (`2024-01-01...18 years` is rejected). **`date + calendar range`** is rejected; use **`date range`** or duration quantities for date arithmetic.

Declare range slots on `data`:

```lemma
data band: calendar range
  -> default 18 years...67 years

data period: date range
  -> default 2024-01-01...2024-12-31

data tier: number range
  -> default 0...100
```

**Type commands** on range types: `help`, `default`; **`quantity range`** also accepts `unit` rows like `quantity`.

### Span: `(lo...hi) as <unit>`

Parentheses around a range literal or expression, then **`as`**, yield the **width** of the interval in the target unit (a scalar), not another range:

```lemma
uses lemma si

rule days_between: (2024-06-01...2024-06-15) as days

rule width_kg: (30 kilogram...35 kilogram) as kilogram

rule span_years: (1990-05-20...2024-06-15) as years
```

| Range type | `as` targets (examples) | Notes |
|------------|-------------------------|--------|
| **date range** | `days`, `months`, `years`, duration units, `number` | Calendar-aware where applicable |
| **number range** | `number`, duration units | |
| **quantity range** | Same-family quantity units; duration ranges → duration units | Mass/money ranges do not span `as days` |
| **ratio range** | Ratio units (`percent`, …) | |
| **calendar range** | — | Span **`as`** is **not** supported (width uses month arithmetic, not quantity units) |

### Range arithmetic and comparison

Ranges support comparison and arithmetic consistent with their kind (see engine tests `date_range`, `calendar_range`, `range_generic`):

```lemma
rule long_enough: 2024-06-01...2024-06-15 >= 7 days

rule shifted: 18 years...67 years + 2 years

rule extended: 2024-01-01...2024-06-15 + 1 months
```

Date endpoints can be built from separate `date` values: `hire_date...today`.

For **trait-duration** quantities, import SI types with `uses lemma si` so literals like `25 years` and `18 years...67 years` resolve (`si.duration`).

## User-Defined Types

Data can define custom types with units, constraints, and validation. The `data` keyword is used for both value declarations and type definitions.

### Data Type Definitions

```lemma
data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91
  -> decimals 2
  -> minimum 0
```

Data can also extend other data' types:

```lemma
data price: money
  -> minimum 0
```

On `quantity` types, `-> unit` **replaces** conversion factors for units already defined on the parent type (same as `ratio`). Add new units with `-> unit` as usual.

### Type Commands

Built-in primitives ship default `help` text that describes what the value represents (for example, the start and end date of a date range). Literal syntax and examples are shown separately via type examples and the sections below; override with `-> help "…"` when you need spec-specific wording.

**For `quantity` and `number` types:**
- `unit <name> <value>` - Define a unit (quantity only)
- `decimals <n>` - Set decimal precision (0-255)
- `minimum <value>` - Set minimum value
- `maximum <value>` - Set maximum value
- `help "<text>"` - Add help text
- `default <value>` - Set default value

**For `ratio` type:**
- `unit <name> <value>` - Define custom ratio units
- `minimum <value>` - Set minimum value
- `maximum <value>` - Set maximum value
- `help "<text>"` - Add help text
- `default <value>` - Set default value

In execution-plan **schema JSON**, `quantity` and `ratio` types expose type-level `minimum` / `maximum` in **canonical** form (same as evaluation). Each `units[]` entry may also include `minimum`, `maximum`, and `default` as magnitudes in that unit (for UI bindings without client-side conversion).

**For `text` type:**
- `option "<value>"` - Add a single allowed option
- `options "<value1>" "<value2>" ...` - Add multiple allowed options
- `length <n>` - Exact string length
- `help "<text>"` - Add help text
- `default "<value>"` - Set default value

**For `date` and `time` types:**
- `minimum <value>` - Minimum date/time
- `maximum <value>` - Maximum date/time
- `help "<text>"` - Add help text
- `default <value>` - Set default value

**For `date range`, `number range`, `quantity range`, `ratio range`, and `calendar range`:**
- `help "<text>"` - Add help text
- `default <lo>...<hi>` - Default interval (half-open)

**For `quantity range` only:** `unit <name> <value>` — same as `quantity` (endpoints must share one unit family).

**For `quantity` types with `-> trait duration` (time periods):**
- `trait duration` (after canonical `second` unit) — see embedded `spec si` (`uses lemma si`, type `si.duration`)
- `minimum <value>` / `maximum <value>` / `default <value>` use the same quantity literal rules as other quantities

**For `boolean` type:**
- `help "<text>"` - Add help text
- `default <value>` - Set default value

### `uses` and qualified parent types

Bring another spec into scope with `uses <alias>: <target>` (optional effective datetime on the target). Reference a type defined there with a qualified parent: `data x: alias.TypeName`. Temporal pins belong on the `uses` line.

```lemma
uses base: base_types

uses rates: pricing

data currency: base.Currency
data discount_rate: rates.Rate
  -> maximum 0.5
```

Pin which temporal version of a dependency applies for that edge:

```lemma
uses fin: finance 2026-01-15

data wallet: fin.Money
```

These edges participate in temporal slicing: the engine creates slice boundaries when a dependency has multiple temporal versions and the consumer references it without a per-edge pin.

### Inline Type Constraints

Define type constraints directly in data declarations:

```lemma
data age: number
  -> minimum 0
  -> maximum 120

data price: quantity
  -> unit eur 1.00
  -> unit usd 0.91

data status: text
  -> option "active"
  -> option "inactive"
```

## Type Annotations

Declare expected types without specifying values:

```lemma
data mass: quantity
  -> unit kilogram 1.0
  -> unit pound 0.453592

data unknown_date:   date
data optional_field: text
data user_age:       number
data is_active:      boolean
data weight:         mass
data elapsed: quantity
  -> unit second 1
  -> unit minute 60
  -> unit hour 3600
  -> trait duration
```

You can also add inline type constraints:

```lemma
data age: number
  -> minimum 0
  -> maximum 120

data price: quantity
  -> unit eur 1.00
  -> decimals 2
```

## Value-copy references (`fill`)

**`fill`** assigns a literal or copies the value of another data slot or rule
result into a name. It can define a new slot (`fill license2: l.other`) or
override a value on an existing `data` row (`data x: number` then `fill x: 42`).
Constraint chains (`-> ...`) belong on **`data` only**, not on `fill`.

Surface forms:

1. **Dotted RHS** — `fill license2: law.other`. A dotted right-hand side is
   never a type name, so it always means "copy from this data or rule path."
2. **Non-dotted RHS with a binding LHS** — `fill i.slot: src`. When the
   left-hand side has path segments, the right-hand side is a value-copy
   reference to a name in the enclosing spec, not a type.

`data x: someident` (LHS without segments, RHS without dots) declares a slot
or parent type; `someident` is a typedef name or keyword, not a copy.

```lemma
spec law

data other: number
  -> default 42


spec license

uses l: law

fill license2: l.other

rule check: license2 > 10
```

Copies can target a **rule**; the rule's evaluated result is the value copied.
Rule-target references resolve lazily on first read once the target rule has
been evaluated.

```lemma
spec pricing

data base: 100 eur

rule discounted: base * (1 - 10%)


spec invoice

uses p: pricing

fill line_total: p.discounted

rule due: line_total
```

When a slot needs constraints or a default, declare them on `data`, then copy
with `fill`:

```lemma
spec outer

uses p: pricing

data clamped: ratio
  -> minimum 0 percent
  -> maximum 100 percent

fill clamped: p.discounted
data fallback: ratio
  -> default 0 percent

fill fallback: p.rate
```

### Binding form

When the LHS is a binding path, `fill` copies from the enclosing spec into
the bound child. The bound child must exist in the referenced spec and its
declared type must be compatible with the source.

```lemma
spec inner

data slot: number
  -> minimum 0
  -> maximum 100


spec outer

uses i: inner

data src: 42

fill i.slot: src

rule r: i.slot
```

The merged type that `fill` must satisfy is the binding's declared type
(`inner.slot`'s `number -> minimum 0 -> maximum 100`), not just the
target's looser type (`src`'s anonymous number).

## Boolean Literals

Multiple aliases for readability:

```lemma
true = yes = accept
false = no = reject
```

All are interchangeable:

```lemma
data is_active:   true
data is_approved: yes
data can_proceed: accept
```

## Special Expressions

### Veto
Blocks the rule entirely (no valid result):

```lemma
rule result: value
  unless constraint_violated then veto "Error message"
```

Not a boolean - prevents any valid verdict from the rule.

## Date Formats

ISO 8601 format:

```lemma
data date_only:     2024-01-15
data date_time:     2024-01-15T14:30:00Z
data with_timezone: 2024-01-15T14:30:00+01:00
```

## Ratios

Ratio values represent proportions. The `ratio` type includes `percent` and `permille` units by default.

**Literal syntax:**
- `15 percent` or `15%` — 15 percent (canonical multiplier 0.15)
- `5 permille` or `5%%` — 5 permille (canonical multiplier 0.005)

```lemma
data tax_rate:   15%
data discount:   20%
data completion: 87.5%
data error_rate: 2%%
```

**Custom ratio types:**

```lemma
data discount_ratio: ratio
  -> minimum 0%
  -> maximum 100%

data discount: 25%
```

**Use in calculations:**

```lemma
rule discount_amount: price * discount_rate

rule after_discount: price * (1 - discount_rate)
```

**Number to ratio conversion:**

```lemma
rule discount_as_percent: 0.25 as percent
```
