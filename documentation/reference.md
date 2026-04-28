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
| `in` | Convert units | `duration in hours`, `price in usd` |

The `in` operator converts between units:
- **Duration units** (built-in): `duration in hours`, `duration in days`
- **User-defined scale types**: Units must be defined in the same type
- **Number to ratio**: `0.5 in percent` converts to `50 percent`

```lemma
data money: scale -> unit eur 1.00 -> unit usd 1.10

data price: 100 eur
rule price_usd: price in usd

data workweek: 40 hours
rule workweek_days: workweek in days
```

## Spec References (`with`)

Reference other specs with the `with` keyword.

- `with spec_name` — alias defaults to the last path segment (base name without version tag).
- `with alias: spec_name` — explicit alias.
- `with spec_name 2025-01-01` — temporal pin (ISO datetime or bare year `YYYY` → Jan 1 00:00).
- `with a, b, c` — comma-separated bare imports (no aliases, no temporal pins).

Comma-separated form is for quick bare imports only. For aliases or temporal pins, use separate `with` lines.

A spec name may carry an optional `.version_tag` suffix (spec base names cannot contain a period).

### Versioned names

```lemma
spec pricing.v1
data base_price: 100 eur

spec pricing.v2
data base_price: 120 eur

spec order
with pricing.v1
rule total: pricing.base_price
```

`spec pricing.v1` and `spec pricing.v2` are distinct specs; they do not share
data, rules, or state.

### Version resolution

- A **versioned** reference (`pricing.v1`) resolves by exact match.
- An **unversioned** reference (`pricing`) resolves to the spec with the
  highest version tag among all loaded specs with that base name, using
  natural sort order (numeric segments compared numerically, so `v10` > `v2`).
  If only an unversioned spec exists, it resolves to that.

### Temporal version resolution

- **Datetime:** `with x: pricing 2025-01-01` or `with x: pricing 2025` (bare year → that year’s Jan 1 00:00, same as datetime literals) picks the temporal version at that instant.

### Self-reference restriction

A spec cannot reference any temporal version of itself (same base name). This is a
semantic error caught during planning:

```lemma
spec pricing.v2
with old: pricing.v1
```

### Version tag syntax

Version tags follow the period and may contain alphanumeric characters, `_`, `-`,
and `.` — for example `v1`, `v2`, `1.2.3`, `rc.1`, `2024-01`. Document base names cannot contain a period.

### Registry references

Registry references use the `@` prefix and also support version tags:

```lemma
with @lemma/std/finance.v2
data money from @lemma/std/finance.v2
```

## Primitive types

Lemma provides these primitive types:

- **`boolean`** - true/false values
- **`number`** - dimensionless numeric values (no units)
- **`scale`** - numeric values that can have units
- **`text`** - string values
- **`date`** - ISO 8601 dates
- **`time`** - time values
- **`duration`** - time periods (hours, days, weeks, etc.)
- **`ratio`** - proportional values (percent, permille)

## User-Defined Types

Data can define custom types with units, constraints, and validation. The `data` keyword is used for both value declarations and type definitions.

### Data Type Definitions

```lemma
data money: scale
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0
```

Data can also extend other data' types:

```lemma
data price: money -> minimum 0
```

### Type Commands

**For `scale` and `number` types:**
- `unit <name> <value>` - Define a unit (scale only)
- `decimals <n>` - Set decimal precision (0-255)
- `minimum <value>` - Set minimum value
- `maximum <value>` - Set maximum value
- `precision <value>` - Set precision value
- `help "<text>"` - Add help text
- `default <value>` - Set default value

**For `ratio` type:**
- `unit <name> <value>` - Define custom ratio units
- `minimum <value>` - Set minimum value
- `maximum <value>` - Set maximum value
- `help "<text>"` - Add help text
- `default <value>` - Set default value

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

**For `duration` type:**
- `minimum <value>` - Set minimum duration
- `maximum <value>` - Set maximum duration
- `help "<text>"` - Add help text
- `default <value>` - Set default value

**For `boolean` type:**
- `help "<text>"` - Add help text
- `default <value>` - Set default value

### Data Imports

Import data (including their types) from other specs:

```lemma
data currency from base_types
data discount_rate from pricing -> maximum 0.5
```

Data imports support explicit effective datetimes, identical to `with` spec refs:

```lemma
data money from finance 2026-01-15
```

Data imports participate in temporal slicing: the engine
creates slice boundaries when the imported spec has multiple temporal versions.

### Inline Type Constraints

Define type constraints directly in data declarations:

```lemma
data age: number -> minimum 0 -> maximum 120
data price: scale -> unit eur 1.00 -> unit usd 1.10
data status: text -> option "active" -> option "inactive"
```

## Type Annotations

Declare expected types without specifying values:

```lemma
data mass: scale -> unit kilogram 1.0 -> unit pound 0.453592

data unknown_date: date
data optional_field: text
data user_age: number
data is_active: boolean
data weight: mass
data duration: duration
```

You can also add inline type constraints:

```lemma
data age: number -> minimum 0 -> maximum 120
data price: scale -> unit eur 1.00 -> decimals 2
```

## Data References

A **data reference** copies the value of another data or the result of a rule
into the declared name. The runtime value flows from the target to the
reference; both names then carry the same value within the slice.

A data reference is recognised in two surface forms:

1. **Dotted RHS** — `data license2: law.other`. A dotted right-hand side is
   never a type name, so it always means "copy from this data or rule path."
2. **Non-dotted RHS in a binding LHS** — `data i.slot: src`. When the
   left-hand side has path segments (a binding into a referenced spec) the
   right-hand side is read as a value-copy reference to a name in the
   enclosing spec, not as a type.

`data x: someident` (LHS without segments, RHS without dots) stays a type
annotation; `someident` is treated as a typedef name.

```lemma
spec law
data other: number -> default 42

spec license
with l: law
data license2: l.other
rule check: license2 > 10
```

References can target a **rule** as well as a data; the rule's evaluated
result is the value copied. Rule-target references are resolved lazily on
first read once the target rule has been evaluated.

```lemma
spec pricing
data base: 100 eur
rule discounted: base * (1 - 10%)

spec invoice
with p: pricing
data line_total: p.discounted
rule due: line_total
```

### Local constraints

Reference declarations may add their own `-> ...` constraints. They are
applied to the copied value at evaluation time, on top of whatever
constraints the target's own type already enforces.

```lemma
data clamped_price: pricing.discounted -> minimum 0 -> maximum 1_000 eur
```

Constraint failure on the copied value produces a Veto (the reference cannot
yield a valid value), not a planning error.

### Local default

A `-> default <value>` tail on a reference is the value used when the target
has no value (missing input, target rule vetoes for missing data). The
default is also surfaced in the spec schema.

```lemma
data fallback_rate: pricing.rate -> default 0%
```

### Binding form

When the LHS is a binding path, the reference copies from the enclosing
spec into the bound child. The bound child must exist in the referenced
spec and its declared type must be compatible with the source.

```lemma
spec inner
data slot: number -> minimum 0 -> maximum 100

spec outer
with i: inner
data src: 42
data i.slot: src
rule r: i.slot
```

The merged type the reference must satisfy is the binding's declared type
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
data is_active: true
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
data date_only: 2024-01-15
data date_time: 2024-01-15T14:30:00Z
data with_timezone: 2024-01-15T14:30:00+01:00
```

## Ratios

Ratio values represent proportions. The `ratio` type includes `percent` and `permille` units by default.

**Literal syntax:**
- `15 percent` or `15%` - 15 percent (0.15 as ratio)
- `5 permille` or `5%%` - 5 permille (0.005 as ratio)

```lemma
data tax_rate: 15 percent
data discount: 20%
data completion: 87.5 percent
data error_rate: 2 permille
```

**Custom ratio types:**

```lemma
data discount_ratio: ratio
  -> minimum 0
  -> maximum 1

data discount: 0.25
```

**Use in calculations:**

```lemma
rule discount_amount: price * discount_rate
rule after_discount: price * (1 - discount_rate)
```

**Number to ratio conversion:**

```lemma
rule discount_as_percent: 0.25 in percent
```
