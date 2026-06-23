---
layout: default
title: Lemma Documentation
---

# Lemma Documentation

**A pure, declarative language for business rules.**

Lemma is a declarative language for business rules. Rules are written in **specs** that humans can read and systems can evaluate deterministically: same spec, same data, same effective instant, same result. Planning validates a spec before evaluation ever runs; results are values or **vetoes**, and every result can carry an explanation.

## Quick Links

- [Main README](../README.md) -- installation and quick start
- [Reference](reference.md) -- all operators, units, types, and ranges
- [Composing specs](spec_composability.md) -- `uses`, temporal versions and pins, planning checks
- [CLI Reference](CLI.md) -- all commands and flags
- [Examples](examples/) -- example specs
- [LLM guide (llms.txt)](llms.txt) -- guide for AI agents authoring Lemma from business logic
- [Registry](registry.md) -- shared specs and `@` references
- [Veto Semantics](veto_semantics.md) -- when rules produce no value
- [Numeric precision](numeric_precision.md) -- exact rational arithmetic and when decimal is used
- [Blueprint](blueprint.md) -- architecture and normative semantics
- [WebAssembly](wasm.md) -- using Lemma in the browser

## Syntax

The four basic building blocks are `repo`, `spec`, `data` and `rule`. A repo is a set of specs, a spec is a set of data and rules. Data provides input, rule provides output. The syntax is intended to be foolproof, by relying on keywords instead of colons or brackets. Indentation is for readability only: even new lines are entirely optional. To make all specs look more or less the same, Lemma has a standard format which you can apply with `lemma format`.

Example:

```lemma
spec pricing 2026-01-01
"""
This is a commentary section.
"""

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

There is no `#`, `//`, or `--` comment syntax. The only in-source documentation is a **commentary** block: triple-quoted `"""..."""` placed immediately after the `spec` line (before any `uses`, `data`, or `rule`). 

## Language Concepts

### Specs

A Lemma file contains one or more specs:

```lemma
spec employee/contract
"""
Optional commentary in triple quotes,
immediately after the spec line.
"""

data monthly_salary: 5_000

rule annual_salary: monthly_salary * 12
```

Spec names consist of letters, digits, underscores, slashes, hyphens and dots (`_ / - .`) and an optional **effective datetime** (`spec pricing 2026-01-01`) for [temporal versioning](#temporal-versions).

### Repositories

A file may declare **`repo`** blocks. A repo is a namespace for specs: two repos can both define `spec invoice` without colliding. Cross-repo targets use a repo qualifier on the `uses` line:

```lemma
repo accounting

spec invoice

data total: 1


spec billing

uses inv: accounting invoice

rule out: inv.total
```

Most workspaces never need `repo` blocks -- files without one belong to the implicit workspace repository. Registry dependencies live in their own `@owner/name` repositories (see [Registry references](#registry-references)).

### Data

Named values with inferred types, which can be overridden by consumers:

```lemma
spec employment

uses lemma units

data name:       "Alice"
data age:        35
data start_date: 2024-01-15
data tax_rate:   15%
data is_manager: true
data workweek:   40 hours
data salary:     75_000
```

**Open inputs**: declare data without a value to request the value at evaluation time:

```lemma
spec loan_application

data birth_date: date

data rating: number
  -> minimum 0
  -> maximum 100

data status: text
  -> option "active"
  -> option "inactive"

data amount: quantity
  -> unit eur 1.00
  -> unit usd 1.19
  -> decimals 2
```

Constraints chain with `-> minimum`, `-> maximum`, `-> option`, `-> unit`, `-> decimals`, `-> default`, `-> help`, and more, depending on the primitive. `lemma schema` lists which inputs a spec requires.

See: [reference.md -- Data](reference.md#data)

### Rules

Compute values based on data and other rules:

```lemma
spec loan_application

uses lemma units

data birth_date: date

data rating: number
  -> minimum 0
  -> maximum 100

data amount: quantity
  -> unit eur 1.00
  -> unit usd 1.19
  -> decimals 2

rule max_amount: 100_000 eur
  unless rating > 90 then 120_000 eur

rule loan_approved:
  rating > 50 and amount < max_amount
  unless birth_date...now < 18 years then no
```

### Unless clauses

Conditional logic where **the last matching condition wins**:

```lemma
spec order_discount

data quantity: number
data is_vip:   false

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

Use `veto` to block a rule entirely when the answer is impossible -- not when the business answer is legitimately `false`:

```lemma
spec age_validation

data age: number

rule validated_age: age
  unless age < 0   then veto "Age must be a positive number"
  unless age > 120 then veto "Invalid age value"
```

A vetoed rule produces **no result**. If a rule references a vetoed rule and needs its value, the veto propagates. If an unless clause provides an alternative, the veto does not propagate:

```lemma
spec scoring

data score:       number
data use_default: false

rule validated_score: score
  unless score < 0 then veto "Invalid score"

rule result: validated_score
  unless use_default then 50
```

If `validated_score` is vetoed but `use_default` is true, `result` = 50.

Test for a veto without propagating it using `is veto` / `is not veto` (returns a boolean):

```lemma
spec veto_checks

data score: number

rule validated_score: score
  unless score < 0 then veto "Invalid score"

rule has_valid_score: validated_score is not veto
```

See: [veto_semantics.md](veto_semantics.md)

### Rule references

Reference other rules by name (the engine resolves whether a name is a data or a rule):

```lemma
spec driving

data age:                number
data license_status:     text
data license_suspended:  boolean

rule is_adult: age >= 18

rule has_license: license_status is "valid"

rule can_drive: is_adult and has_license
  unless license_suspended then veto "License suspended"
```

### Spec composition

Import another spec with `uses` and reference its members through the alias. Set data on an imported spec with `with`:

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

- `uses alias: target` -- import under an explicit alias (`uses target` defaults the alias to the last path segment).
- `alias.field` / `alias.rule_name` -- read imported data or rules in expressions.
- `with alias.field: value` -- set a data slot on the imported spec. The left-hand side must be an import path; local slots use `data`.

See [Composing specs](spec_composability.md).

### Temporal versions

The same spec **name** may appear on several effective-dated rows. Each row is immutable; change is a new row on the timeline:

```lemma
spec pricing

data base_price: 20
data quantity:   number

rule total: base_price * quantity


spec pricing 2025-01-01

data base_price: 25
data quantity:   number

rule total: base_price * quantity
```

```bash
lemma run pricing --effective 2024-06-01   # uses base_price: 20
lemma run pricing --effective 2025-06-01   # uses base_price: 25
```

An **unpinned** import (`uses pricing`) follows the dependency's timeline; a **pinned** import (`uses p: pricing 2025-06-01`) freezes the dependency at that instant. Planning checks temporal coverage and interface compatibility across slices -- see [Composing specs](spec_composability.md) and [reference.md -- Spec References](reference.md#spec-references-uses).

### Registry references

Shared specs from a registry (default: [LemmaBase.com](https://lemmabase.com)) are imported with `@owner/repo` qualifiers:

```lemma
spec invoicing

uses @iso/countries alpha2

data price: quantity
  -> unit eur 1

data country: alpha2.code

rule tariff: 0 eur
  unless country is "NL" then price * 5%

rule total: price + tariff
```

```bash
lemma fetch --all           # fetch all @... dependencies into lemma_deps/
lemma fetch @iso/countries -f   # force re-fetch if content changed
```

See: [registry.md](registry.md)

## Expressions

### Arithmetic

```lemma
spec arithmetic_examples

data price:     100
data tax:       21
data quantity:  3
data principal: 1_000
data rate:      0.05
data years:     10

rule total: (price + tax) * quantity

rule compound: principal * (1 + rate) ^ years
```

Operators: `+`, `-`, `*`, `/`, `%`, `^`

### Comparison

```lemma
spec comparison_examples

data status: text
data age:    number
data income: number

rule status_ok: status is "approved"

rule not_cancelled: status is not "cancelled"

rule is_eligible: age >= 18 and income > 30_000
```

Operators: `>`, `<`, `>=`, `<=`, `is`, `is not`, `is veto`, `is not veto`

### Logical

```lemma
spec loan_approval

data credit_score:    number
data income_verified: boolean
data has_bankruptcy:  boolean

rule can_approve_loan:
  credit_score >= 650 and income_verified and not has_bankruptcy
```

Operators: `and`, `not` (there is no `or` -- unless chains accommodate such logic)

### Mathematical

```lemma
spec math_examples

data a:     3
data b:     4
data angle: 0.5

rule hypotenuse: sqrt (a ^ 2 + b ^ 2)

rule sine_value: sin angle

rule log_value: log 10
```

Prefix operators (parentheses optional): `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`

## Extending data

Declare a primitive or parent data name on the right-hand side, then chain `->` data commands. Other data extend it by naming it:

```lemma
spec warehouse_types

data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91
  -> decimals 2
  -> minimum 0 eur

data mass: quantity
  -> unit kilogram 1.0
  -> unit gram 0.001
  -> unit pound 0.453592

data price:  100 eur
data weight: 75 kilogram
```

**Reuse across specs** — `uses` plus qualified parents (`alias.field`):

```lemma
spec base_types

data currency: text
  -> option "EUR"
  -> option "USD"

data rate: ratio
  -> maximum 100%


spec checkout

uses base: base_types

data payment_currency: base.currency
data discount_rate: base.rate
  -> maximum 50%
```

See: [reference.md -- Extending data](reference.md#extending-data)

## Standard library: `uses lemma units`

Lemma embeds SI units in the standard library (repo `lemma`, spec `units`). Import with `uses lemma units`, then use the units directly in literals or reference the types as `units.mass`, `units.duration`, `units.length`, `units.calendar`, and others:

```lemma
spec logistics

uses lemma units

data package_weight: 12 kilogram
data shift_length:   8 hours
data route_distance: 45 kilometer

rule weight_grams:  package_weight as gram
rule is_heavy:      package_weight > 20 kilogram
rule is_long_shift: shift_length >= 8 hours
```

Duration units (`hours`, `days`, `weeks`, ...) come from `units.duration`; calendar periods (`year`, `month`) from `units.calendar`. Prefer the stdlib types over redefining kilogram or hour in every spec.

## Unit Conversions

Convert within a unit family with `as`:

```lemma
spec conversion_examples

data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91

data price: 100 eur

rule price_usd: price as usd

rule as_percent: 0.25 as percent
```

Durations convert the same way:

```lemma
spec schedule

uses lemma units

data workweek: 40 hours

rule workweek_days: workweek as days
```

Strip to a bare number with a chained cast: `amount as eur as number`. See [reference.md -- Type cast](reference.md#type-cast-as).

## Literal Types

| Type | Example | Notes |
|------|---------|-------|
| **Number** | `42`, `3.14`, `1.23e10` | Exact rational arithmetic |
| **Text** | `"hello"` | String literals |
| **Boolean** | `true`, `false`, `yes`, `no`, `accept`, `reject` | Aliases |
| **Date** | `2024-01-15`, `2024-01-15T14:30:00Z` | ISO 8601 |
| **Time** | `14:30:00` | Time of day |
| **Quantity** | `100 eur`, `40 hours`, `12 kilogram` | Unit must be declared by a quantity type in scope (own typedef or `uses lemma units`) |
| **Ratio** | `15 percent`, `15%`, `5 permille`, `5%%` | Proportional values |
| **Range** | `0...100`, `2024-01-01...2024-06-15`, `18 year...67 year` | Half-open intervals |

## Ranges

Intervals use `lo...hi` (lower inclusive, upper exclusive). Test membership with `in`; project width with `(lo...hi) as <unit>`. Range slots use `number range`, `date range`, `time range`, `quantity range`, `ratio range`, or a named `<type> range`:

```lemma
spec eligibility

uses lemma units

data age:   25 year
data score: 50

rule in_working_age: age in 18 year...67 year

rule in_band: score in 0...100

rule days_in_q2: (2024-04-01...2024-07-01) as days
```

See: [reference.md -- Ranges](reference.md#ranges)

## Date and Time

Dates compare directly; spans between dates are ranges projected to a unit; durations add to dates:

```lemma
spec deadlines

uses lemma units

data today:    2024-09-30
data deadline: 2024-12-31

rule days_until_deadline: (today...deadline) as days

rule is_overdue: today > deadline

rule follow_up_date: deadline + 14 days
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
3. **[03_spec_references](../cli/tests/integrations/examples/03_spec_references.lemma)** -- compound units, `uses lemma units`
4. **[04_unit_conversions](../cli/tests/integrations/examples/04_unit_conversions.lemma)** -- typed units, `as` conversions
5. **[05_date_handling](../cli/tests/integrations/examples/05_date_handling.lemma)** -- date ranges and arithmetic
6. **[06_tax_calculation](../cli/tests/integrations/examples/06_tax_calculation.lemma)** -- progressive tax rules
7. **[07_shipping_policy](../cli/tests/integrations/examples/07_shipping_policy.lemma)** -- complex business logic
8. **[08_rule_references](../cli/tests/integrations/examples/08_rule_references.lemma)** -- rule composition
9. **[09_stress_test](../cli/tests/integrations/examples/09_stress_test.lemma)** -- large rule graphs
10. **[10_compensation_policy](../cli/tests/integrations/examples/10_compensation_policy.lemma)** -- layered policy rules
11. **[11_spec_composition](../cli/tests/integrations/examples/11_spec_composition.lemma)** -- multi-spec hierarchy
12. **[12_registry_references](../cli/tests/integrations/examples/12_registry_references.lemma)** -- `uses @owner/repo spec_name`
13. **[13_temporal_versioning](../cli/tests/integrations/examples/13_temporal_versioning.lemma)** -- effective-dated spec rows
