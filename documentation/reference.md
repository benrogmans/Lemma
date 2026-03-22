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
| `==` | Equal | `status == "active"` |
| `!=` | Not equal | `type != "admin"` |
| `is` | Equal (text-friendly) | `status is "approved"` |
| `is not` | Not equal (text-friendly) | `status is not "cancelled"` |

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
type money: scale -> unit eur 1.00 -> unit usd 1.10

fact price: 100 eur
rule price_usd: price in usd

fact workweek: 40 hours
rule workweek_days: workweek in days
```

## Spec References

Reference other specs with `fact name: spec other_spec`. The spec name is
**required**. You may optionally add a datetime (effective, for temporal version resolution) and/or a plan
hash pin for verification. Syntax: `spec name`, `spec name datetime`, `spec name datetime~hash`,
or `spec name~hash`. Whitespace around `~` is allowed. A spec name may carry an optional `.version_tag`
suffix (spec base names cannot contain a period).

### Versioned names

```lemma
spec pricing.v1
fact base_price: 100 eur

spec pricing.v2
fact base_price: 120 eur

spec order
fact pricing: spec pricing.v1
rule total: pricing.base_price
```

`spec pricing.v1` and `spec pricing.v2` are distinct specs; they do not share
facts, rules, or state.

### Version resolution

- A **versioned** reference (`spec pricing.v1`) resolves by exact match.
- An **unversioned** reference (`spec pricing`) resolves to the spec with the
  highest version tag among all loaded specs with that base name, using
  natural sort order (numeric segments compared numerically, so `v10` > `v2`).
  If only an unversioned spec exists, it resolves to that.

### Temporal version resolution and plan hash

- **Datetime:** `fact x: spec pricing 2025` resolves the spec at effective 2025-01-01T00:00:00.
  Use when you need a specific temporal version (see temporal versioning).
- **Plan hash pin:** `fact x: spec pricing~a1b2c3d4` or `fact x: spec pricing 2025~a1b2c3d4`
  verifies that the resolved spec’s plan hash equals the given value (8 hex chars, e.g. `a1b2c3d4`).
  Whitespace around `~` is allowed (e.g. `spec pricing ~ a1b2c3d4`).
  Hash is **verification only**; resolution is always by (name, effective). Mismatch ⇒ validation error.
  Compute the hash with `lemma schema <spec> [--effective T]` (hash is shown in the output).

### Self-reference restriction

A spec cannot reference any temporal version of itself (same base name). This is a
semantic error caught during planning:

```lemma
spec pricing.v2
fact old: spec pricing.v1
```

### Version tag syntax

Version tags follow the period and may contain alphanumeric characters, `_`, `-`,
and `.` — for example `v1`, `v2`, `1.2.3`, `rc.1`, `2024-01`. Document base names cannot contain a period.

### Registry references

Registry references use the `@` prefix and also support version tags:

```lemma
fact finance: spec @lemma/std/finance.v2
type money from @lemma/std/finance.v2
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

Define custom types with units, constraints, and validation:

### Basic Type Definition

```lemma
type money: scale
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0
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
- `minimum <n>` - Minimum string length
- `maximum <n>` - Maximum string length
- `length <n>` - Exact string length
- `help "<text>"` - Add help text
- `default "<value>"` - Set default value

**For `date` and `time` types:**
- `minimum <value>` - Minimum date/time
- `maximum <value>` - Maximum date/time
- `help "<text>"` - Add help text
- `default <value>` - Set default value

**For `boolean` and `duration` types:**
- `help "<text>"` - Add help text
- `default <value>` - Set default value

### Type Imports

Import types from other specs:

```lemma
type currency from base_types
type discount_rate from pricing -> maximum 0.5
```

### Inline Type Definitions

Define types inline in fact declarations:

```lemma
fact age: [number -> minimum 0 -> maximum 120]
fact price: [scale -> unit eur 1.00 -> unit usd 1.10]
fact status: [text -> option "active" -> option "inactive"]
```

## Type Annotations

Declare expected types without specifying values:

```lemma
type mass: scale -> unit kilogram 1.0 -> unit pound 0.453592

fact unknown_date: [date]
fact optional_field: [text]
fact user_age: [number]
fact is_active: [boolean]
fact weight: [mass]
fact duration: [duration]
```

You can also use inline type definitions:

```lemma
fact age: [number -> minimum 0 -> maximum 120]
fact price: [scale -> unit eur 1.00 -> decimals 2]
```

## Boolean Literals

Multiple aliases for readability:

```lemma
true = yes = accept
false = no = reject
```

All are interchangeable:

```lemma
fact is_active: true
fact is_approved: yes
fact can_proceed: accept
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
fact date_only: 2024-01-15
fact date_time: 2024-01-15T14:30:00Z
fact with_timezone: 2024-01-15T14:30:00+01:00
```

## Ratios

Ratio values represent proportions. The `ratio` type includes `percent` and `permille` units by default.

**Literal syntax:**
- `15 percent` or `15%` - 15 percent (0.15 as ratio)
- `5 permille` or `5%%` - 5 permille (0.005 as ratio)

```lemma
fact tax_rate: 15 percent
fact discount: 20%
fact completion: 87.5 percent
fact error_rate: 2 permille
```

**Custom ratio types:**

```lemma
type discount_ratio: ratio
  -> minimum 0
  -> maximum 1

fact discount: 0.25
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
