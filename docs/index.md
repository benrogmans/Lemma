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

fact quantity   = [number]
fact base_price = 100
fact is_member  = false

rule price_with_vat = base_price + 21%

rule bulk_discount
  = quantity >= 100 and price_with_vat? > 500

rule discount = 0%
  unless quantity >= 10	then 10%
  unless bulk_discount? then 15%
  unless is_member		then 20%

rule price_with_discount = base_price - discount?
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
fact name = "Alice"
fact age = 35
fact start_date = 2024-01-15
fact salary = 75000
fact tax_rate = 15%
fact is_manager = true
fact weight_limit = 50 kilograms
fact email_pattern = /^[\w]+@[\w]+\.[\w]+$/
```

**Type Annotations** - Declare expected types without values:

```lemma
fact birth_date = [date]
fact distance = [length]
```

See all available types: [reference.md - Type Annotations](reference.md#type-annotations)

See: [examples/01_simple_facts.lemma](examples/01_simple_facts.lemma)

### Rules

Compute values based on facts and other rules:

```lemma
rule annual_salary = monthly_salary * 12
rule is_senior = age >= 40
rule total_weight = package_weight + box_weight
```

See: [examples/02_rules_and_unless.lemma](examples/02_rules_and_unless.lemma), [examples/06_tax_calculation.lemma](examples/06_tax_calculation.lemma)

### Unless Clauses

Conditional logic where **the last matching condition wins**:

```lemma
rule discount = 0%
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
rule is_eligible = false
  unless age >= 18 then true

rule can_proceed = accept
  unless is_blocked then reject
```

See: [reference.md - Boolean Literals](reference.md#boolean-literals)

### Veto

Use `veto` to block a rule entirely when constraints are violated:

```lemma
rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless age < 18 then veto "Must be 18 or older"
```

When a veto applies, the rule produces **no valid result** - it's blocked completely. This is useful for validation and hard constraints.

**Best Practice:** Put veto clauses last so they override all other logic.

See: [examples/02_rules_and_unless.lemma](examples/02_rules_and_unless.lemma), [examples/08_rule_references.lemma](examples/08_rule_references.lemma)

### Rule References

Reference other rules using `?` suffix:

```lemma
rule is_adult
  = age >= 18

rule has_license
  = license_status == "valid"

rule can_drive
  = is_adult? and has_license?
  unless license_suspended? then veto "License suspended"
```

Note: Facts don't use `?`, only rule references do.

See: [examples/08_rule_references.lemma](examples/08_rule_references.lemma)

### Document References

Compose documents by referencing and overriding:

```lemma
doc base_employee
fact name = "John Doe"
fact salary = 5000

doc manager
fact employee = doc base_employee
fact employee.name = "Alice Smith"
fact employee.salary = 8000

rule manager_bonus = employee.salary * 0.15
```

See: [examples/03_document_references.lemma](examples/03_document_references.lemma)

## Expressions

### Arithmetic

```lemma
rule total
  = (price + tax) * quantity

rule compound
  = principal * (1 + rate) ^ years
```

Operators: `+`, `-`, `*`, `/`, `%`, `^`

See: [reference.md - Arithmetic](reference.md#arithmetic)

### Comparison

```lemma
rule status_ok = status is "approved"

rule not_cancelled
  = status is not "cancelled"

rule is_eligible
  = age >= 18
    and income > 30000

```

Operators: `>`, `<`, `>=`, `<=`, `==`, `!=`, `is`, `is not`

See: [reference.md - Comparison](reference.md#comparison)

### Logical

```lemma
rule can_approve_loan
  = credit_score >= 650
    and income_verified?
    and not has_bankruptcy?

rule needs_manager_review
  = loan_amount > 100000
    or risk_score > 7

rule has_cosigner
  = have application.cosigner_name

rule missing_documentation
  = not have applicant.tax_returns
```

Operators: `and`, `or`, `not`, `have`, `have not`, `not have`

The `have` operator checks if a fact has any value (useful for optional fields).

See: [reference.md - Logical](reference.md#logical)

### Mathematical

```lemma
rule hypotenuse = sqrt(a^2 + b^2)
rule sine_value = sin(angle)
rule log_value = log(10)
```

Operators: `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`

Note: These are prefix operators, not functions. Both `sin angle` and `sin(angle)` are valid.

See: [reference.md - Mathematical](reference.md#mathematical)

## Unit Conversions

Automatic conversions between compatible units:

```lemma
fact weight = 10 kilograms
rule weight_in_pounds = weight in pounds

fact distance = 5 kilometers
rule distance_in_miles = distance in miles
```

See: [examples/04_unit_conversions.lemma](examples/04_unit_conversions.lemma)

Supported types: mass, length, volume, duration, temperature, power, force, pressure, energy, frequency, data size

Full list with all unit keywords: [reference.md - Unit Types](reference.md#unit-types)

## Literal Types

| Type | Example | Notes |
|------|---------|-------|
| **Number** | `42`, `3.14`, `1.23e10` | Integers and floats |
| **Text** | `"hello"` | String literals |
| **Boolean** | `true`, `false`, `yes`, `no`, `accept`, `reject` | Aliases allowed |
| **Date** | `2024-01-15`, `2024-01-15T14:30:00Z` | ISO 8601 format |
| **Duration** | `5 hours`, `3 days`, `2 weeks` | Time periods |
| **Mass** | `10 kilograms`, `5 pounds` | Weight/mass |
| **Length** | `100 meters`, `5 kilometers` | Distance |
| **Temperature** | `25 celsius`, `98 fahrenheit` | Heat |
| **Percentage** | `15%`, `100%` | 0-100 range |
| **Regex** | `/[A-Z]{3}-\d{4}/` | Pattern matching |
| **Volume** | `2 liters`, `1 gallon` | Liquid volume |
| **Power** | `1000 watts`, `5 kilowatts` | Electrical power |
| **Data** | `10 megabytes`, `1 gigabyte` | File sizes |

See: [examples/01_simple_facts.lemma](examples/01_simple_facts.lemma), [reference.md](reference.md)

## Date and Time

```lemma
fact today = 2024-09-30
fact deadline = 2024-12-31
fact meeting_time = 2024-09-30T14:30:00Z

rule days_until_deadline = deadline - today
rule is_overdue = today > deadline
```

See: [examples/05_date_handling.lemma](examples/05_date_handling.lemma)

## Complete Examples

Browse [examples/](examples/) directory:

1. **[01_simple_facts.lemma](examples/01_simple_facts.lemma)** - All fact types and literals
2. **[02_rules_and_unless.lemma](examples/02_rules_and_unless.lemma)** - Conditional logic, veto usage
3. **[03_document_references.lemma](examples/03_document_references.lemma)** - Document composition
4. **[04_unit_conversions.lemma](examples/04_unit_conversions.lemma)** - Working with typed units
5. **[05_date_handling.lemma](examples/05_date_handling.lemma)** - Date arithmetic and comparisons
6. **[06_tax_calculation.lemma](examples/06_tax_calculation.lemma)** - Real-world progressive tax rules
7. **[07_shipping_policy.lemma](examples/07_shipping_policy.lemma)** - Complex business logic
8. **[08_rule_references.lemma](examples/08_rule_references.lemma)** - Rule composition with `?` syntax

## Implementation

Lemma uses a pure Rust evaluator for fast and deterministic execution:

```bash
# Run a document
lemma run document

# Override facts
lemma run document age=25 income=50000

# Load multiple documents
lemma workspace ./policies/
```

See the [main README](../README.md) for installation and CLI usage.
