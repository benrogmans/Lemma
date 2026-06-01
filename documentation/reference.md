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

### Type cast (`as`)

Chained casts nest left-to-right: `expr as unit₁ as unit₂ … as number`.

| Form | Description | Example |
|------|-------------|---------|
| `as <unit>` | Convert, relabel, or construct a quantity/ratio in that unit | `mass as gram`, `5 as eur`, `rate as hours` |
| `as number` | Strip to raw magnitude (requires explicit unit on prior step for quantities/ranges) | `10 eur as number`, `span as days as number` |

Same-family conversion applies factors (`2 kilogram as gram` → `2000 gram`). Cross-family relabel keeps magnitude (`5 eur as kg` → `5 kg`).

`as` binds tighter than `*`, `/`, and `%`. In `balance / rate as month`, the `as month` applies to `rate`, not to the quotient. To convert the result of an arithmetic expression, use parentheses: `(balance / rate) as month`.

Literal quantities and ratios carry an explicit unit, so `10 eur as number` and `25% as number` plan without an extra unit step. Named data references need the full chain: `amount as eur as number`, not `amount as number`.

Date, quantity, and **time** ranges cannot use bare `as number` (unit is ambiguous). Project span width with `as <unit> as number` — e.g. `start...end as days as number`, `(09:00...17:00) as hours as number`, `(30 kilogram...35 kilogram) as stone as number`. Calendar span units are singular `year` and `month`. Duration span units (`days`, `hours`, `seconds`, …) require `uses lemma units` when not declared locally.

Number ranges allow bare `as number` (span width with no unit).

## Spec References (`uses`)

Reference other specs with the `uses` keyword. For how unpinned vs pinned imports, temporal slices, coverage, and interface checks fit together, see [Composing specs](spec_composability.md).


- `uses spec_name` — alias defaults to the last path segment of the target name.
- `uses alias: spec_name` — explicit alias.
- `uses spec_name 2025-01-01` — temporal pin (ISO datetime or bare year `YYYY` → Jan 1 00:00).

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

Evaluating **`order`** before 2025-01-01 uses `base_price: 100`; from 2025-01-01 onward, `120`. See [Composing specs](spec_composability.md) for unpinned vs pinned imports.

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

uses fin: @lemma/std finance

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
- **`time range`** - half-open time-of-day intervals (`09:00...17:00`)
- **`ratio`** - proportional values (percent, permille)
- **`ratio range`** - half-open ratio intervals

Numbers are stored and computed as **exact rationals** (ℚ); API output is a **decimal string**. See [Numeric precision](numeric_precision.md).

## Ranges

Ranges express **half-open** intervals: **lower inclusive, upper exclusive**. Containment uses `in`:

```lemma
uses lemma units

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
| **`time range`** | Times (`09:00...17:00`) | Business hours, shift windows |
| **`number range`** | Dimensionless numbers (`0...100`) | Scores, tiers |
| **`quantity range`** | Quantities in one unit family (`30 kilogram...35 kilogram`) | Weight bands, duration bands (with `trait duration`), money bands |
| **`ratio range`** | Ratios (`0%...50%`) | Allowed discount bands |

Any **rangeable named quantity type** can also be declared with a `range` suffix, e.g. `data estimated: money range` or `data band: units.calendar -> default 18 year...67 year` (specializes to a quantity range with that type's units).

**Calendar intervals** use month/year units from `uses lemma units` (`units.calendar`). Inline literals (`18 year...67 year`) and `in` work like other ranges. Endpoints must be calendar units (`year`, `month`, and declared plurals). Do not mix calendar and duration units in one literal (`12 year...7 day` is a planning error). Do not mix dates and calendar endpoints (`2024-01-01...18 year` is rejected).

**Time ranges** are half-open like all ranges. Endpoints must share the same timezone (including both absent). Literal order does not imply midnight wraparound: `22:00...02:00` is ordered `[02:00, 22:00)` with an 20-hour span, not an overnight window.

Declare range slots on `data`:

```lemma
uses lemma units

data band: units.calendar
  -> default 18 year...67 year

data window: time range
  -> default 09:00...17:00

data period: date range
  -> default 2024-01-01...2024-12-31

data tier: number range
  -> default 0...100
```

**Type commands** on range types: `help`, `default`; **`quantity range`** also accepts `unit` rows like `quantity`.

### Span: `(lo...hi) as <unit>`

Parentheses around a range literal or expression, then **`as`**, yield the **width** of the interval in the target unit (a scalar), not another range:

```lemma
uses lemma units

rule days_between: (2024-06-01...2024-06-15) as days

rule width_kg: (30 kilogram...35 kilogram) as kilogram

rule span_years: (1990-05-20...2024-06-15) as years
```

| Range type | `as` targets (examples) | Notes |
|------------|-------------------------|--------|
| **date range** | `days`, `months`, `years`, duration units, calendar units | Calendar-aware where applicable |
| **time range** | Duration units only (`hours`, `minutes`, `seconds`, …) | No calendar units; bare `as number` rejected |
| **number range** | `number`, duration units | |
| **quantity range** | Same-family quantity units (mass, money, duration, calendar, …) | Cross-family span (e.g. mass range `as days`) is rejected at planning |
| **ratio range** | Ratio units (`percent`, …) | |
| **calendar interval** (inline or `units.calendar` / `units.calendar` default) | Same-family calendar units | Span uses month/year arithmetic via quantity-range machinery |

### Range arithmetic and comparison

Ranges support comparison and arithmetic consistent with their kind (see engine tests `date_range`, `calendar_range`, `range_generic`):

```lemma
rule long_enough: 2024-06-01...2024-06-15 >= 7 days

rule shifted: 18 years...67 years + 2 years

rule extended: 2024-01-01...2024-06-15 + 1 months
```

Date endpoints can be built from separate `date` values: `hire_date...today`.

For **trait-duration** quantities, import SI types with `uses lemma units` so literals like `25 years` and `18 years...67 years` resolve (`units.duration`).

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

**For `date range`, `number range`, `quantity range`, `time range`, and `ratio range`:**
- `help "<text>"` - Add help text
- `default <lo>...<hi>` - Default interval (half-open)

**For `quantity range` only:** `unit <name> <value>` — same as `quantity` (endpoints must share one unit family).

**For `quantity` types with `-> trait duration` (time periods):**
- `trait duration` (after canonical `second` unit) — see embedded `spec units` (`uses lemma units`, type `units.duration`)
- `minimum <value>` / `maximum <value>` / `default <value>` use the same quantity literal rules as other quantities

**For `quantity` types with `-> trait calendar` (calendar periods):**
- `trait calendar` (after canonical `month` unit) — see `units.calendar` in `uses lemma units`, or `units.calendar` via `uses lemma units`
- `default <lo>...<hi>` with calendar units specializes the slot to `quantity range` at planning

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

## Setting data on an imported spec (`with`)

**`with`** sets a literal or reference on a **data slot of a spec you `uses`**. The left-hand side must be an import path (`alias.field` or `alias.nested.field`). Local names (`with x: …`) are rejected — use **`data`** for slots in the current spec.

Constraint chains (`-> ...`) belong on **`data` only**, not on `with`.

```lemma
spec base_employee
data name: text
data monthly_salary: number

spec specific_employee
uses employee: base_employee
with employee.name: "Alice Smith"
with employee.monthly_salary: 7_500

rule employee_summary: employee.name
```

Read imported data or rules in expressions without `with` when you do not need to override values:

```lemma
spec outer
uses i: inner
rule r: i.x
```

### Scenario parameters (same import, different values)

```lemma
spec pricing
data price: 100
data discount: 0%
rule final_price: price * (1 - discount)

spec scenarios
uses retail: pricing
with retail.discount: 5%

uses wholesale: pricing
with wholesale.discount: 15%
with wholesale.price: 80

rule retail_final: retail.final_price
rule wholesale_final: wholesale.final_price
```

### Binding with a local source

When the RHS is a name in the enclosing spec (not a dotted import path), the LHS must still be an import path:

```lemma
spec inner
data slot: number
  -> minimum 0
  -> maximum 100

spec outer
uses i: inner
data src: 42
with i.slot: src
rule r: i.slot
```

`data x: someident` (local LHS, non-dotted RHS) declares a slot or parent type; `someident` is a typedef name, not a value copy.

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
