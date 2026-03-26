---
layout: default
title: Lemma Documentation
---

# Lemma Documentation

**A language that means business.**

Lemma is a declarative language for expressing rules, facts, and business logic that both humans and computers can understand.

## Quick Links

- [Main README](../README.md) -- installation and quick start
- [Reference](reference.md) -- all operators, units, and types
- [CLI Reference](CLI.md) -- all commands and flags
- [Examples](examples/) -- example specs
- [Registry](registry.md) -- shared specs and `@` references
- [Veto Semantics](veto_semantics.md) -- when rules produce no value
- [WebAssembly](wasm.md) -- using Lemma in the browser

## Syntax

Lemma is whitespace-insensitive. Use formatting that makes your rules readable:

```lemma
spec pricing

fact quantity: [number]
fact base_price: 100
fact is_member: false

rule price_with_vat: base_price + 21%

rule bulk_discount: quantity >= 100 and price_with_vat > 500

rule discount: 0%
  unless quantity >= 10  then 10%
  unless bulk_discount   then 15%
  unless is_member       then 20%

rule price_with_discount: base_price - discount
```

## Language Concepts

### Specs

Every Lemma file contains specs -- namespaces for facts and rules:

```lemma
spec employee/contract
"""
Optional description in triple quotes.
"""
```

Specs support hierarchical naming: `contract/employment`, `company/policies/vacation`.

### Facts

Named values with rich types:

```lemma
fact name: "Alice"
fact age: 35
fact start_date: 2024-01-15
fact salary: 75000
fact tax_rate: 15%
fact is_manager: true
fact workweek: 40 hours
```

**Type annotations** -- declare expected types without values:

```lemma
type length: scale
  -> unit meter 1.0
  -> unit kilometer 1000.0

fact birth_date: [date]
fact distance: [length]
```

Or inline:

```lemma
fact age: [number -> minimum 0 -> maximum 120]
fact price: [scale -> unit eur 1.00 -> unit usd 1.10]
```

See: [reference.md -- Type Annotations](reference.md#type-annotations)

### Rules

Compute values based on facts and other rules:

```lemma
rule annual_salary: monthly_salary * 12
rule is_senior: age >= 40
rule total_weight: package_weight + box_weight
```

### Unless clauses

Conditional logic where **the last matching condition wins**:

```lemma
rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip then 25%
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
  unless age < 0 then veto "Age must be a positive number"
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

Reference other rules by name (the engine resolves whether a name is a fact or a rule):

```lemma
rule is_adult: age >= 18

rule has_license: license_status == "valid"

rule can_drive: is_adult and has_license
  unless license_suspended then veto "License suspended"
```

### Spec composition

Reference facts and rules across specs:

```lemma
spec base_employee
fact name: "John Doe"
fact salary: 5000

spec manager
fact employee: spec base_employee
fact employee.name: "Alice Smith"
fact employee.salary: 8000

rule manager_bonus: employee.salary * 0.15
```

Spec names may include a `.version_tag` suffix (e.g. `spec pricing.v1`). An unversioned reference resolves to the latest loaded temporal version by natural sort.

See: [reference.md -- Spec References](reference.md#spec-references)

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
rule is_eligible: age >= 18 and income > 30000
```

Operators: `>`, `<`, `>=`, `<=`, `==`, `!=`, `is`, `is not`

### Logical

```lemma
rule can_approve_loan: credit_score >= 650 and income_verified and not has_bankruptcy
```

Operators: `and`, `not`

### Mathematical

```lemma
rule hypotenuse: sqrt(a^2 + b^2)
rule sine_value: sin(angle)
rule log_value: log(10)
```

Prefix operators (parentheses optional): `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`

## User-Defined Types

```lemma
type money: scale
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0

type mass: scale
  -> unit kilogram 1.0
  -> unit gram 0.001
  -> unit pound 0.453592

fact price: 100 eur
fact weight: 75 kilogram
```

**Type imports** -- reuse types across specs:

```lemma
type currency from base_types
type discount_rate from pricing -> maximum 0.5
```

See: [reference.md -- User-Defined Types](reference.md#user-defined-types)

## Unit Conversions

Conversions work within the same type definition:

```lemma
type money: scale
  -> unit eur 1.00
  -> unit usd 1.10

fact price: 100 eur
rule price_usd: price in usd
```

Duration units are built-in:

```lemma
fact workweek: 40 hours
rule workweek_days: workweek in days
```

Number to ratio:

```lemma
rule as_percent: 0.25 in percent
```

## Literal Types

| Type | Example | Notes |
|------|---------|-------|
| **Number** | `42`, `3.14`, `1.23e10` | Integers and floats |
| **Text** | `"hello"` | String literals |
| **Boolean** | `true`, `false`, `yes`, `no`, `accept`, `reject` | Aliases |
| **Date** | `2024-01-15`, `2024-01-15T14:30:00Z` | ISO 8601 |
| **Duration** | `5 hours`, `3 days`, `2 weeks` | Built-in units |
| **Ratio** | `15 percent`, `15%`, `5 permille`, `5%%` | Proportional values |
| **Scale** | `100 eur`, `75 kilogram` | Requires user-defined type |

## Date and Time

```lemma
fact today: 2024-09-30
fact deadline: 2024-12-31
fact meeting_time: 2024-09-30T14:30:00Z

rule days_until_deadline: deadline - today
rule is_overdue: today > deadline
```

## Examples

Browse [examples/](examples/) or [cli/tests/integrations/examples/](../cli/tests/integrations/examples/):

1. **[01_simple_facts](../cli/tests/integrations/examples/01_simple_facts.lemma)** -- all fact types and literals
2. **[02_rules_and_unless](../cli/tests/integrations/examples/02_rules_and_unless.lemma)** -- conditional logic, veto
3. **[03_spec_references](../cli/tests/integrations/examples/03_spec_references.lemma)** -- spec composition
4. **[04_unit_conversions](../cli/tests/integrations/examples/04_unit_conversions.lemma)** -- typed units
5. **[05_date_handling](../cli/tests/integrations/examples/05_date_handling.lemma)** -- date arithmetic
6. **[06_tax_calculation](../cli/tests/integrations/examples/06_tax_calculation.lemma)** -- progressive tax rules
7. **[07_shipping_policy](../cli/tests/integrations/examples/07_shipping_policy.lemma)** -- complex business logic
8. **[08_rule_references](../cli/tests/integrations/examples/08_rule_references.lemma)** -- rule composition
