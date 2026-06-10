---
layout: default
title: Lemma Documentation
---

# Lemma Documentation

**A language that means business.**

Lemma is a declarative language for expressing rules, data, and business logic that both humans and computers can understand.

## Quick Links

- [Main README](../README.md) -- installation and quick start
- [Reference](reference.md) -- all operators, units, types, and ranges
- [Composing specs](spec_composability.md) -- `uses`, temporal versions, pins, planning checks
- [CLI Reference](CLI.md) -- all commands and flags
- [Examples](examples/) -- example specs
- [LLM guide (llms.txt)](llms.txt) -- guide for AI agents authoring Lemma from business logic
- [Registry](registry.md) -- shared specs and `@` references
- [Veto Semantics](veto_semantics.md) -- when rules produce no value
- [Numeric precision](numeric_precision.md) -- exact rational arithmetic and when decimal is used
- [WebAssembly](wasm.md) -- using Lemma in the browser

## Syntax

Lemma is whitespace-insensitive. Use formatting that makes your rules readable:

```lemma
spec pricing

data quantity:   number
data base_price: 100
data is_member:  false

rule price_with_vat: base_price + 21%

rule bulk_discount:
  quantity >= 100 and price_with_vat > 500

rule discount: 0%
  unless quantity >= 10  then 10%
  unless bulk_discount   then 15%
  unless is_member       then 20%

rule price_with_discount: base_price - discount
```

## Language Concepts

### Specs

Every Lemma file contains specs -- namespaces for data and rules:

```lemma
spec employee/contract
"""
Optional description in triple quotes.
"""
```

Specs support hierarchical naming: `contract/employment`, `company/policies/vacation`.

### Data

Named values with rich types:

```lemma
uses lemma units

data name:       "Alice"
data age:        35
data start_date: 2024-01-15
data salary:     75_000
data tax_rate:   15%
data is_manager: true
data workweek:   40 hours
```

**Type annotations** -- declare expected types without values:

```lemma
data length: quantity
  -> unit meter 1.0
  -> unit kilometer 1000.0

data birth_date: date
data distance:   length
```

Or with inline type constraints:

```lemma
data age: number
  -> minimum 0
  -> maximum 120

data price: quantity
  -> unit eur 1.00
  -> unit usd 0.91
```

See: [reference.md -- Type Annotations](reference.md#type-annotations)

### Rules

Compute values based on data and other rules:

```lemma
rule annual_salary: monthly_salary * 12

rule is_senior: age >= 40

rule total_weight: package_weight + box_weight
```

### Unless clauses

Conditional logic where **the last matching condition wins**:

```lemma
rule discount: 0%
  unless quantity >= 10  then 10%
  unless quantity >= 50  then 20%
  unless is_vip          then 25%
```

If a VIP customer orders 75 items, they get 25% (last matching wins), not 20%.

This matches natural language: "It's 0%, unless you buy 10+ then 10%, unless you buy 50+ then 20%, unless you're VIP then 25%."

**Best practice:** place veto clauses last so they override all other logic.

### Boolean literals

Multiple aliases for readability:

| True Values | False Values |
|-------------|--------------|
| `true` | `false` |
| `yes` | `no` |
| `accept` | `reject` |

All aliases in each column are interchangeable.

### Veto

Use `veto` to block a rule entirely when input data is invalid:

```lemma
rule validated_age: age
  unless age < 0   then veto "Age must be a positive number"
  unless age > 120 then veto "Invalid age value"
```

A vetoed rule produces **no result**. If a rule references a vetoed rule and needs its value, the veto propagates. If an unless clause provides an alternative, the veto does not propagate:

```lemma
rule validated_score: score
  unless score < 0 then veto "Invalid score"

rule result: validated_score
  unless use_default then 50
```

If `validated_score` is vetoed but `use_default` is true, `result` = 50.

See: [veto_semantics.md](veto_semantics.md)

### Rule references

Reference other rules by name (the engine resolves whether a name is a data or a rule):

```lemma
rule is_adult: age >= 18

rule has_license: license_status is "valid"

rule can_drive: is_adult and has_license
  unless license_suspended then veto "License suspended"
```

### Spec composition

Reference data and rules across specs:

```lemma
spec base_employee

data name:   "John Doe"
data salary: 5000


spec manager

uses employee: base_employee

with employee.name:   "Alice Smith"
with employee.salary: 8000

rule manager_bonus: employee.salary * 0.15
```

The same spec **name** may appear on several **effective-dated rows** (`spec pricing 2025-01-01`). Pin a dependency with `uses p: pricing 2025-06-01`; evaluate with `lemma run … --effective <datetime>`. See [Composing specs](spec_composability.md) and [reference.md — Spec References](reference.md#spec-references-uses).

## Expressions

### Arithmetic

```lemma
rule total: (price + tax) * quantity

rule compound: principal * (1 + rate) ^ years
```

Operators: `+`, `-`, `*`, `/`, `%`, `^`

### Comparison

```lemma
rule status_ok: status is "approved"

rule not_cancelled: status is not "cancelled"

rule is_eligible: age >= 18 and income > 30_000
```

Operators: `>`, `<`, `>=`, `<=`, `is`, `is not`

### Logical

```lemma
rule can_approve_loan:
  credit_score >= 650 and income_verified and not has_bankruptcy
```

Operators: `and`, `not`

### Mathematical

```lemma
rule hypotenuse: sqrt (a ^ 2 + b ^ 2)

rule sine_value: sin angle

rule log_value: log 10
```

Prefix operators (parentheses optional): `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`

## User-Defined Types

Data define custom types using the `data` keyword with type commands:

```lemma
data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91
  -> decimals 2
  -> minimum 0

data mass: quantity
  -> unit kilogram 1.0
  -> unit gram 0.001
  -> unit pound 0.453592

data price:  100 eur
data weight: 75 kilogram
```

**Data reuse across specs** — `uses` plus qualified parent types:

```lemma
uses base: base_types

uses rates: pricing

data currency: base.Currency
data discount_rate: rates.Rate
  -> maximum 0.5
```

See: [reference.md -- User-Defined Types](reference.md#user-defined-types)

## Unit Conversions

Conversions work within the same type definition:

```lemma
data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91

data price: 100 eur

rule price_usd: price as usd
```

Trait-duration **quantity** types (after `uses lemma units` or an equivalent typedef) accept **quantity** literals for time periods:

```lemma
uses lemma units

data workweek: units.duration
  -> default 40 hours

rule workweek_days: workweek as days
```

Number to ratio:

```lemma
rule as_percent: 0.25 as percent
```

## Literal Types

| Type | Example | Notes |
|------|---------|-------|
| **Number** | `42`, `3.14`, `1.23e10` | Integers and floats |
| **Text** | `"hello"` | String literals |
| **Boolean** | `true`, `false`, `yes`, `no`, `accept`, `reject` | Aliases |
| **Date** | `2024-01-15`, `2024-01-15T14:30:00Z` | ISO 8601 |
| **Quantity** | `100 eur`, `40 hours` (when the quantity type declares matching units and, for time, `trait duration`) | User-defined quantity type with `unit` / `trait` commands |
| **Ratio** | `15 percent`, `15%`, `5 permille`, `5%%` | Proportional values |
| **Ranges** | `0...100`, `2024-01-01...2024-06-15`, `18 years...67 years` | Half-open intervals; see [reference.md — Ranges](reference.md#ranges) |

## Ranges

Intervals use `lo...hi` (lower inclusive, upper exclusive). Test membership with `in`, project width with `(lo...hi) as <unit> as number`, and declare slots with `date range`, `number range`, `quantity range`, or `ratio range`. Calendar month/year bands use `uses lemma units` and literals like `18 year...67 year`.

```lemma
uses lemma units

data age: 25 year

rule in_working_age: age in 18 year...67 year

rule days_in_q2: (2024-04-01...2024-07-01) as days
```

See: [reference.md — Ranges](reference.md#ranges)

## Date and Time

```lemma
data today:        2024-09-30
data deadline:     2024-12-31
data meeting_time: 2024-09-30T14:30:00Z

rule days_until_deadline: deadline - today

rule is_overdue: today > deadline
```

## Examples

### Language examples (`documentation/examples/`)

Self-contained specs demonstrating core features:

- **[01_coffee_order](examples/01_coffee_order.lemma)** -- types, unless clauses, arithmetic
- **[02_library_fees](examples/02_library_fees.lemma)** -- conditional fees, grace periods
- **[03_recipe_scaling](examples/03_recipe_scaling.lemma)** -- calculations, stdlib duration units
- **[04_membership_benefits](examples/04_membership_benefits.lemma)** -- spec composition with `uses`
- **[05_weather_clothing](examples/05_weather_clothing.lemma)** -- temporal versioning, text rules
- **[nl/tax/net_salary](examples/nl/tax/net_salary.lemma)** -- progressive tax brackets, multi-rule pipeline

### CLI integration examples (`cli/tests/integrations/examples/`)

Feature-focused specs used in CLI integration tests:

1. **[01_simple_data](../cli/tests/integrations/examples/01_simple_data.lemma)** -- all data types and literals
2. **[02_rules_and_unless](../cli/tests/integrations/examples/02_rules_and_unless.lemma)** -- conditional logic, veto
3. **[03_spec_references](../cli/tests/integrations/examples/03_spec_references.lemma)** -- spec composition
4. **[04_unit_conversions](../cli/tests/integrations/examples/04_unit_conversions.lemma)** -- typed units
5. **[05_date_handling](../cli/tests/integrations/examples/05_date_handling.lemma)** -- date arithmetic
6. **[06_tax_calculation](../cli/tests/integrations/examples/06_tax_calculation.lemma)** -- progressive tax rules
7. **[07_shipping_policy](../cli/tests/integrations/examples/07_shipping_policy.lemma)** -- complex business logic
8. **[08_rule_references](../cli/tests/integrations/examples/08_rule_references.lemma)** -- rule composition
