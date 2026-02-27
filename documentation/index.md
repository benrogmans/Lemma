---
layout: default
title: Lemma Documentation
---

# Lemma Documentation

**Logic for man and machine**

Lemma is a declarative logic language for expressing rules, facts, and business logic that both humans and computers can understand.

## Quick Links

- [Main README](../README.md) - Installation and quick start
- [Reference](reference.md) - All operators, units, and types
- [Examples](examples/) - 10 comprehensive example files
- [WebAssembly](wasm.md) - Using Lemma in the browser

## Syntax & Formatting

Lemma is whitespace-insensitive. Use formatting that makes your rules readable:

```lemma
doc pricing

fact quantity: [number]
fact base_price: 100
fact is_member: false

rule price_with_vat: base_price + 21%

rule bulk_discount
  : quantity >= 100 and price_with_vat > 500

rule discount: 0%
  unless quantity >= 10	then 10%
  unless bulk_discount then 15%
  unless is_member		then 20%

rule price_with_discount: base_price - discount
```

Format for clarity - all examples below show formatting styles, not requirements.

## Language Concepts

### Documents

Every Lemma file contains documents - namespaces for facts and rules:

```lemma
doc employee/contract
"""
Optional documentation in triple quotes
"""
```

Documents support hierarchical naming: `contract/employment`, `company/policies/vacation`.

See: [examples/03_document_references.lemma](examples/03_document_references.lemma)

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

**Type Annotations** - Declare expected types without values:

```lemma
type length: scale -> unit meter 1.0 -> unit kilometer 1000.0

fact birth_date: [date]
fact distance: [length]
```

Or use inline type definitions:

```lemma
fact age: [number -> minimum 0 -> maximum 120]
fact price: [scale -> unit eur 1.00 -> unit usd 1.10]
```

See all available types: [reference.md - Type Annotations](reference.md#type-annotations)

See: [examples/01_simple_facts.lemma](examples/01_simple_facts.lemma)

### Rules

Compute values based on facts and other rules:

```lemma
rule annual_salary: monthly_salary * 12
rule is_senior: age >= 40
rule total_weight: package_weight + box_weight
```

See: [examples/02_rules_and_unless.lemma](examples/02_rules_and_unless.lemma), [examples/06_tax_calculation.lemma](examples/06_tax_calculation.lemma)

### Unless Clauses

Conditional logic where **the last matching condition wins**:

```lemma
rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip then 25%
```

If a VIP customer orders 75 items, they get 25% (last matching wins), not 20%.

This matches natural language: "It's 0%, unless you buy 10+ then 10%, unless you buy 50+ then 20%, unless you're VIP then 25%."

**Best Practice:** Place veto clauses last so they override all other logic.

See: [examples/02_rules_and_unless.lemma](examples/02_rules_and_unless.lemma), [examples/07_shipping_policy.lemma](examples/07_shipping_policy.lemma)

### Boolean Literals

Multiple aliases for readability:

| True Values | False Values |
|-------------|--------------|
| `true` | `false` |
| `yes` | `no` |
| `accept` | `reject` |

All aliases in each column are interchangeable.

```lemma
rule is_eligible: false
  unless age >= 18 then true

rule can_proceed: accept
  unless is_blocked then reject
```

See: [reference.md - Boolean Literals](reference.md#boolean-literals)

### Veto

Use `veto` to block a rule entirely when the input data is invalid or constraints are violated:

```lemma
rule validated_age: age
  unless age < 0 then veto "Age must be a positive number"
  unless age > 120 then veto "Invalid age value"
```

When a veto applies, the rule produces **no valid result** - it's blocked completely. This is useful for data validation and hard constraints. Use veto when the data itself is invalid, not for negative business logic results.

**Key behavior**: If a rule references a vetoed rule and needs its value, the veto applies to the dependent rule. If an unless clause provides an alternative value, the veto doesn't apply:

```lemma
rule validated_score: score
  unless score < 0 then veto "Invalid score"

rule result: validated_score
  unless use_default then 50
```

If `validated_score` is vetoed but `use_default` is true, `result` = 50 (veto does not apply).

**Best practice:** Put veto clauses last so they override all other logic.

See: [examples/02_rules_and_unless.lemma](examples/02_rules_and_unless.lemma), [examples/08_rule_references.lemma](examples/08_rule_references.lemma), [veto_semantics.md](veto_semantics.md)

### Rule References

Reference other rules by name (the engine resolves whether a name is a fact or a rule):

```lemma
rule is_adult
  : age >= 18

rule has_license
  : license_status == "valid"

rule can_drive
  : is_adult and has_license
  unless license_suspended then veto "License suspended"
```

See: [examples/08_rule_references.lemma](examples/08_rule_references.lemma)

### Document References

Compose documents by referencing and overriding:

```lemma
doc base_employee
fact name: "John Doe"
fact salary: 5000

doc manager
fact employee: doc base_employee
fact employee.name: "Alice Smith"
fact employee.salary: 8000

rule manager_bonus: employee.salary * 0.15
```

Document names may include a `.version_tag` suffix (e.g. `doc pricing.v1`). Base names cannot contain a period.
Versioned and unversioned documents with the same base name are distinct.
An unversioned reference resolves to the latest loaded version by natural sort.
A document cannot reference any version of itself.

See: [reference.md - Document References](reference.md#document-references) and
[examples/03_document_references.lemma](examples/03_document_references.lemma)

## Expressions

### Arithmetic

```lemma
rule total
  : (price + tax) * quantity

rule compound
  : principal * (1 + rate) ^ years
```

Operators: `+`, `-`, `*`, `/`, `%`, `^`

See: [reference.md - Arithmetic](reference.md#arithmetic)

### Comparison

```lemma
rule status_ok: status is "approved"

rule not_cancelled
  : status is not "cancelled"

rule is_eligible
  : age >= 18
    and income > 30000

```

Operators: `>`, `<`, `>=`, `<=`, `==`, `!=`, `is`, `is not`

See: [reference.md - Comparison](reference.md#comparison)

### Logical

```lemma
rule can_approve_loan
  : credit_score >= 650
    and income_verified
    and not has_bankruptcy

rule needs_manager_review
  : loan_amount > 100000
    or risk_score > 7
```

Operators: `and`, `or`, `not`

See: [reference.md - Logical](reference.md#logical)

### Mathematical

```lemma
rule hypotenuse: sqrt(a^2 + b^2)
rule sine_value: sin(angle)
rule log_value: log(10)
```

Operators: `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`

Note: These are prefix operators, not functions. Both `sin angle` and `sin(angle)` are valid.

See: [reference.md - Mathematical](reference.md#mathematical)

## User-Defined Types

Define custom types with units, constraints, and validation:

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
fact weight: 75 kilograms
```

**Type Imports** - Reuse types across documents:

```lemma
type currency from base_types
type discount_rate from pricing -> maximum 0.5
```

See: [reference.md - User-Defined Types](reference.md#user-defined-types)

## Unit Conversions

Unit conversions work within the same type definition:

```lemma
type money: scale -> unit eur 1.00 -> unit usd 1.10

fact price: 100 eur
rule price_usd: price in usd  // Converts to 110 usd
```

**Duration conversions** (duration units are built-in):

```lemma
fact workweek: 40 hours
rule workweek_days: workweek in days  // Converts to ~1.67 days
```

**Number to ratio conversion:**

```lemma
rule discount_as_percent: 0.25 in percent  // Converts to 25 percent
```

See: [examples/04_unit_conversions.lemma](examples/04_unit_conversions.lemma)

## Literal Types

| Type | Example | Notes |
|------|---------|-------|
| **Number** | `42`, `3.14`, `1.23e10` | Integers and floats |
| **Text** | `"hello"` | String literals |
| **Boolean** | `true`, `false`, `yes`, `no`, `accept`, `reject` | Aliases allowed |
| **Date** | `2024-01-15`, `2024-01-15T14:30:00Z` | ISO 8601 format |
| **Duration** | `5 hours`, `3 days`, `2 weeks` | Time periods (built-in units) |
| **Ratio** | `15 percent`, `15%`, `5 permille`, `5%%` | Proportional values |
| **Scale** | `100 eur`, `75 kilograms` | Requires user-defined type with units |

**Note:** Units like `kilograms`, `eur`, `celsius` must be defined in custom types. Only duration units are built-in.

See: [examples/01_simple_facts.lemma](examples/01_simple_facts.lemma), [reference.md](reference.md)

## Date and Time

```lemma
fact today: 2024-09-30
fact deadline: 2024-12-31
fact meeting_time: 2024-09-30T14:30:00Z

rule days_until_deadline: deadline - today
rule is_overdue: today > deadline
```

See: [examples/05_date_handling.lemma](examples/05_date_handling.lemma)

## Inverse Reasoning

Inversion allows you to find what input values produce a desired output. This is useful for questions like "What quantity gives me a 30% discount?" or "What salary produces a total compensation of €100,000?"

**Note:** Inversion is available in the Rust engine library.

### Example

```rust
use lemma::{Engine, Target, LiteralValue};
use std::collections::HashMap;

let mut engine = Engine::new();

engine.add_lemma_files(HashMap::from([("pricing.lemma".into(), r#"
    doc pricing
    fact quantity: [number]
    fact is_vip: false

    rule discount: 0%
      unless quantity >= 10 then 10%
      unless quantity >= 50 then 20%
      unless is_vip then 25%
"#.into())]))?;

// Find what gives a 25% discount
use rust_decimal::Decimal;
let response = engine.invert(
    "pricing",
    "discount",
    Target::value(LiteralValue::Ratio(Decimal::from(25), Some("percent".to_string()))),
    HashMap::new()
)?;

// Response shows: is_vip must be true (regardless of quantity)
```

The inversion response contains:
- **Solutions**: Domain constraints for each variable
- **Shape**: Symbolic representation of all valid solutions
- **Free variables**: Facts that can vary while still satisfying the target

See the [engine README](../engine/README.md#inverse-reasoning) for detailed API documentation.

## Complete Examples

Browse [examples/](examples/) directory:

1. **[01_simple_facts.lemma](examples/01_simple_facts.lemma)** - All fact types and literals
2. **[02_rules_and_unless.lemma](examples/02_rules_and_unless.lemma)** - Conditional logic, veto usage
3. **[03_document_references.lemma](examples/03_document_references.lemma)** - Document composition
4. **[04_unit_conversions.lemma](examples/04_unit_conversions.lemma)** - Working with typed units
5. **[05_date_handling.lemma](examples/05_date_handling.lemma)** - Date arithmetic and comparisons
6. **[06_tax_calculation.lemma](examples/06_tax_calculation.lemma)** - Real-world progressive tax rules
7. **[07_shipping_policy.lemma](examples/07_shipping_policy.lemma)** - Complex business logic
8. **[08_rule_references.lemma](examples/08_rule_references.lemma)** - Rule composition and references

## Implementation

Lemma uses a pure Rust evaluator for fast and deterministic execution:

```bash
# Run a document
lemma run document

# Provide fact values
lemma run document age=25 income=50000

# Load multiple documents
lemma workspace ./policies/
```

See the [main README](../README.md) for installation and CLI usage.
