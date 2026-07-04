---
nav_title: Specs, data, and rules
nav_order: 20
---

# Specs, Data, and Rules

A Lemma file contains one or more Specs. Each Spec is a namespace of Data (inputs) and Rules (computed outputs). Multiple Specs with the same name can coexist when they have different effective-from datetimes (covered later in [Composing Specs](composing_specs.md)).

## Specs

```lemma
spec employee/contract
"""
Optional commentary in triple quotes,
immediately after the spec line.
"""

data monthly_salary: 5_000

rule annual_salary: monthly_salary * 12
```

Spec names consist of letters, digits, underscores, slashes, hyphens, and dots (`_ / - .`), plus an optional effective datetime (`spec pricing 2026-01-01`).

## Data

Named values with inferred types, which consumers can override:

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

### Open inputs

Declare Data without a value to request the value at evaluation time:

```lemma
spec loan_application

data birth_date: date

data rating: number
  -> minimum 0
  -> maximum 100

data status: text
  -> option "active"
  -> option "inactive"

data amount: measure
  -> unit eur 1.00
  -> unit usd 1.19
  -> decimals 2
```

Constraints chain with `-> minimum`, `-> maximum`, `-> option`, `-> unit`, `-> decimals`, `-> default`, `-> help`, and more, depending on the primitive. To see which inputs a Spec requires:

```bash
lemma schema loan_application
```

See [Data in the language reference](../reference/readme.md#data).

## Rules

Rules compute values based on Data and other Rules:

```lemma
spec loan_application

uses lemma units

data birth_date: date

data rating: number
  -> minimum 0
  -> maximum 100

data amount: measure
  -> unit eur 1.00
  -> unit usd 1.19
  -> decimals 2

rule max_amount: 100_000 eur
  unless rating > 90 then 120_000 eur

rule loan_approved:
  rating > 50 and amount < max_amount
  unless birth_date...now < 18 years then no
```

## Rule references

Reference other Rules by name. The engine resolves whether a name is Data or a Rule:

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

Rules evaluate in dependency order: `can_drive` depends on `is_adult` and `has_license`, so those run first.

## Next up

[Conditional logic](conditional_logic.md): Unless chains, last-matching-wins, and boolean literals.
