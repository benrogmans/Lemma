---
layout: default
title: Whitepaper
---

# Lemma: A Declarative Language for Business Logic
## Rules for Man and Machine

**Version 1.0**
**October 2025**

---

## Abstract

Business rules are traditionally encoded in either natural language documents that humans can read but machines cannot execute, or in imperative code that machines can execute but humans struggle to read. This creates a fundamental disconnect: legal contracts, compliance policies, and business rules live in one world, while their software implementations live in another. Changes to policies require translation by developers, introducing delay, cost, and the risk of misinterpretation.

Lemma bridges this gap. It is a declarative language designed specifically for expressing business logic in a form that flows like natural language. Lemma specs can encode pricing rules, tax calculations, eligibility criteria, contracts, and policies in a way that business stakeholders can read and validate, while software systems can enforce and automate them.

This white paper introduces Lemma's design principles, core features, implementation architecture, and practical applications. We demonstrate how Lemma's unique "default/unless" semantics, rich type system, and compositional design make it an ideal choice for encoding complex business rules in domains ranging from finance and insurance to e-commerce and human resources.

---

## Table of contents

1. [Introduction](#1-introduction)
2. [Design philosophy](#2-design-philosophy)
3. [Language features](#3-language-features)
4. [Type system](#4-type-system)
5. [Compositional architecture](#5-compositional-architecture)
6. [Technical implementation](#6-technical-implementation)
7. [Use cases](#7-use-cases)
8. [Comparison with existing approaches](#8-comparison-with-existing-approaches)
9. [Future work](#9-future-work)
10. [Conclusion](#10-conclusion)

---

## 1. Introduction

### 1.1 The problem

Modern software systems are governed by complex business rules that are inherently dynamic. Tax codes change annually. Pricing strategies evolve with market conditions. Compliance requirements shift with new regulations. Yet these rules are typically hardcoded in imperative programming languages, requiring developer involvement for every change.

This creates several problems:

1. **Communication Gap**: Business stakeholders describe rules in natural language, but developers must translate them into code, introducing opportunities for misinterpretation.

2. **Verification Difficulty**: Non-technical stakeholders cannot verify that implemented code correctly reflects business requirements.

3. **Maintenance Burden**: Every rule change requires developer time, testing, and deployment, slowing down business adaptation.

4. **Auditability Challenges**: Understanding why a system made a particular decision requires tracing through imperative code logic, making compliance audits difficult.

5. **Documentation Drift**: Written policies and implemented code inevitably diverge over time as one is updated without the other.

### 1.2 The solution

Lemma addresses these problems by providing a declarative language that:

- **Reads like English**: Natural syntax using keywords like "unless," "then," and "is" makes rules immediately comprehensible.
- **Types matter**: User-defined types with units, constraints, and automatic conversions eliminate a major source of bugs.
- **Last wins semantics**: The "unless" clause uses "last matching wins" logic that mirrors how humans naturally express exceptions and special cases.
- **Fully executable**: Despite its natural syntax, Lemma uses a pure Rust evaluator, providing rigorous logical inference and deterministic evaluation.
- **Composable**: Specs reference and extend each other, enabling modular rule design.
- **Auditable**: Every decision can be traced back to specific data and rules with operation records.

### 1.3 Example

Consider a simple pricing rule:

```lemma
spec pricing

data quantity: number
data is_vip:   false

rule discount: 0%
  unless quantity >= 10  then 10%
  unless quantity >= 50  then 20%
  unless is_vip          then 25%

rule price: 200 - discount
```

This spec is immediately readable by business stakeholders while being fully executable by software systems. The semantics are clear: start with no discount, but if quantity is 10 or more, apply 10%; if 50 or more, apply 20%; if customer is VIP, apply 25%. The last matching condition wins, so a VIP customer with 100 units gets 25%, not 20%.

---

## 2. Design philosophy

### 2.1 Natural language semantics

Lemma's syntax is designed to mirror how people naturally express business rules. Consider how you might explain a shipping policy:

> "Shipping is €12.99, unless you're in Canada then it's €25, unless you're ordering over 100 euros then it's free."

Lemma encodes this exactly as stated:

```lemma
spec shipping_policy

data destination: text
data order_total: number

rule shipping: 12.99
  unless destination is "CA" then 25
  unless order_total >= 100  then 0
```

This "last matching wins" semantic might seem counterintuitive to programmers who are accustomed to "early returns" or "first match" logic, but it aligns perfectly with natural language. When we say "X, unless Y, unless Z," we mean that Z overrides Y, which overrides X.

### 2.2 Declarative by design

Lemma is purely declarative. You describe *what* should be true, not *how* to compute it. This has several advantages:

1. **Clarity**: Rules state relationships between values without implementation details.
2. **Optimization**: The execution engine can optimize and reorder operations.
3. **Reasoning**: Logical inference can derive implications and detect contradictions.
4. **Parallelization**: Declarative semantics enable safe concurrent evaluation.

### 2.3 Type safety without syntax overhead

Programming languages typically require verbose type annotations. Lemma infers types from literals while providing a rich type system:

```lemma
spec inferred_types

uses lemma units

data mass: quantity
  -> unit kilogram 1.0
  -> unit pound 0.453592

data salary:   75_000
data vacation: units.duration
  -> default 3 weeks

data weight:   15 kilogram
data deadline: 2024-12-31
data tax_rate: 22%
```

The type system prevents nonsensical operations (you can't add a date to a weight) while enabling automatic unit conversions within the same type:

```lemma
spec weights

data mass: quantity
  -> unit kilogram 1.0
  -> unit pound 0.453592

data weight: 70 kilogram

rule weight_in_pounds: weight as pound
```

### 2.4 Composition over configuration

Lemma encourages building complex systems from simple, composable pieces. Specs can reference other specs, rules can reference other rules, and data can be overridden in specific contexts:

```lemma
spec base_employee

data salary:     50_000
data bonus_rate: 5%


spec manager

uses employee: base_employee

with employee.salary:     80_000
with employee.bonus_rate: 15%

rule manager_bonus: employee.salary * employee.bonus_rate
```

This compositional design enables reusable rule libraries and reduces duplication.

---

## 3. Language features

### 3.1 Data

Data are named values of a certain type. They represent inputs to the system:

```lemma
spec employee_record

data name:       "Alice"
data age:        35
data start_date: 2024-01-15
data salary:     75_000
data is_manager: true
```

Data without a literal declares an open input — the type and constraints are known, the value is supplied at evaluation:

```lemma
spec open_inputs

data birth_date:     date
data employee_count: number
data location:       text
```

### 3.2 Rules

Rules compute values based on data and other rules. A rule has a name, a default value, and optional "unless" clauses:

```lemma
spec order_discount

data quantity: number
data is_vip:   boolean

rule discount: 0%
  unless quantity >= 10  then 10%
  unless quantity >= 50  then 20%
  unless is_vip          then 25%
```

Rules can reference other rules by name (the engine resolves whether a name is a data or a rule during planning):

```lemma
spec driving

data age:            number
data license_status: text

rule is_adult: age >= 18

rule has_license: license_status is "valid"

rule can_drive: is_adult and has_license
```

### 3.3 Unless clauses

The "unless" clause is Lemma's primary conditional construct. Unlike if-else chains that stop at the first match, unless clauses use "last matching wins" semantics:

```lemma
spec grading

data score: number

rule status: "standard"
  unless score >= 70 then "good"
  unless score >= 90 then "excellent"
```

If `score` is 95, the result is "excellent" (not "good"), because the last matching condition wins. This matches natural language: "It's standard, unless your score is at least 70 then good, unless it's at least 90 then excellent."

### 3.4 Veto

While "unless" clauses override values, sometimes you need to block a rule entirely. The `veto` keyword does this:

```lemma
spec lending

data credit_score:    number
data age:             number
data bankruptcy_flag: boolean

rule loan_approval: reject
  unless credit_score >= 600 then accept
  unless age < 18
    then veto "Must be 18 or older"
  unless bankruptcy_flag
    then veto "Cannot approve due to bankruptcy"
```

When a veto applies, the rule produces no valid result. This is useful for validation and hard constraints.

### 3.5 Operators

Lemma provides comprehensive operators for arithmetic, comparison, logical operations, and mathematical functions:

**Arithmetic**: `+`, `-`, `*`, `/`, `%`, `^`

```lemma
spec interest

data principal: 1_000
data rate:      0.05
data years:     10

rule compound: principal * (1 + rate) ^ years
```

**Comparison**: `>`, `<`, `>=`, `<=`, `is`, `is not`

```lemma
spec eligibility

data age:    number
data income: number

rule is_eligible: age >= 18 and income > 30_000
```

**Logical**: `and`, `not`

```lemma
spec approvals

data is_manager:   boolean
data is_suspended: boolean

rule can_approve: is_manager and not is_suspended
```

**Mathematical**: `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`

```lemma
spec geometry

data a: 3
data b: 4

rule hypotenuse: sqrt (a ^ 2 + b ^ 2)
```

### 3.6 Type-aware arithmetic

Lemma intelligently handles arithmetic between different types. When you write:

```lemma
spec sale

rule discounted_price: 200 - 25%
```

Lemma understands that subtracting a ratio from a number means "subtract 25% of the value," producing `150`.

---

## 4. Type system

### 4.1 Primitive types

Lemma provides several primitive types:

- **Boolean**: true/false, yes/no, accept/reject
- **Number**: Dimensionless numeric values (exact rationals internally; decimal strings in API output — see [numeric_precision.md](numeric_precision.md))
- **Quantity**: Numeric values with units (including **time periods** via `-> trait duration`)
- **Text**: String literals
- **Date**: ISO 8601 format dates and datetimes
- **Time**: Time values
- **Ratio**: Proportional values (percent, permille)

```lemma
spec primitive_examples

uses lemma units

data count:     42
data name:      "Alice"
data is_active: true
data deadline:  2024-12-31
data workweek: units.duration
  -> default 40 hours

data tax_rate: 15%
```

### 4.2 Extending data

Lemma defines reusable data with the `data` keyword and data commands (`-> unit`, `-> minimum`, …). This provides flexibility while maintaining type safety.

**Defining money:**

```lemma
spec warehouse_types

data money: quantity
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0 eur

data mass: quantity
  -> unit kilogram 1.0
  -> unit gram 0.001
  -> unit pound 0.453592

data price:  100 eur
data weight: 75 kilogram
```

**Data commands** allow fine-grained control:

- `unit <name> <value>` - Define units (for `quantity` and `ratio` types)
- `decimals <n>` - Set decimal precision
- `minimum <value>` / `maximum <value>` - Set value constraints
- `option "<value>"` - Define allowed text values
- `help "<text>"` - Add documentation
- `default <value>` - Set default values

Data can also extend another data name:

```lemma
spec extended_types

data money: quantity
  -> unit eur 1.00
  -> unit usd 1.10

data price: money
  -> minimum 0 eur
```

**Cross-spec types** — `uses` plus qualified parent types:

```lemma
spec base_types

data Currency: text
  -> option "EUR"
  -> option "USD"


spec rate_card

data Rate: ratio
  -> maximum 100%


spec product_pricing

uses base: base_types

uses rates: rate_card

data currency: base.Currency
data discount_rate: rates.Rate
  -> maximum 50%
```

**Inline type constraints** in data declarations:

```lemma
spec constrained_inputs

data age: number
  -> minimum 0
  -> maximum 120

data price: quantity
  -> unit eur 1.00
  -> unit usd 1.10
```

### 4.3 Unit conversion

Unit conversions work within the same type definition. This ensures type safety while allowing flexible unit systems.

```lemma
spec currency_conversion

data money: quantity
  -> unit eur 1.00
  -> unit usd 1.10

data price: 100 eur

rule price_usd: price as usd
```

**Trait-duration quantities** (stdlib `units.duration` or your own `quantity` + `trait duration`) use the same `as` conversion rules as other quantities:

```lemma
spec schedule

uses lemma units

data workweek: units.duration
  -> default 40 hours

rule workweek_days: workweek as days
```

**Number to ratio conversion:**

```lemma
spec number_to_ratio

rule discount_as_percent: 0.25 as percent
```

This design eliminates manual conversion logic while maintaining clear type boundaries.

### 4.4 Ratio type

Ratios represent proportional values. The `ratio` type includes `percent` and `permille` units by default.

```lemma
spec ratio_literals

data tax_rate:   15%
data discount:   25%
data error_rate: 2%%
data completion: 87.5%
```

**Custom ratio types:**

```lemma
spec custom_ratios

data discount_ratio: ratio
  -> minimum 0%
  -> maximum 100%

data discount: 25%
```

Ratios interact intelligently with other types in arithmetic operations, automatically applying proportional calculations.

---

## 5. Compositional architecture

### 5.1 Specs

Every Lemma file contains one or more specs. Specs are namespaces that encapsulate related data and rules:

```lemma
spec employee/benefits
"""
Company benefits policy for full-time employees
"""

uses lemma units

data years_of_service: number

rule vacation_days: 15 days
  unless years_of_service >= 5  then 20 days
  unless years_of_service >= 10 then 25 days
```

Spec names can be hierarchical (using `/` separators), enabling logical organization of rule libraries.

### 5.2 Document references

Specs can reference other specs, enabling composition and reuse:

```lemma
spec base_employee

data name:   "John Doe"
data salary: 50_000


spec manager

uses employee: base_employee

with employee.salary: 80_000

rule manager_bonus: employee.salary * 0.15
```

This pattern allows creating specialized variants of base specs without duplication.

### 5.3 Data bindings

Data can be bound at different levels:

```lemma
spec pricing

data quantity:   100
data unit_price: 50


spec wholesale_pricing

uses pricing: pricing

with pricing.quantity:   1000
with pricing.unit_price: 35

rule total: pricing.quantity * pricing.unit_price
```

This enables scenario modeling and context-specific rule evaluation.

### 5.4 Workspace model

Lemma supports loading multiple specs together in a workspace. Specs can reference each other, creating a network of related rules:

```
policies/
  ├── employee/
  │   ├── base.lemma
  │   ├── compensation.lemma
  │   └── benefits.lemma
  ├── customer/
  │   ├── pricing.lemma
  │   └── discounts.lemma
  └── shipping/
      └── rates.lemma
```

List specs in a workspace:

```bash
lemma list ./policies/
```

Run a spec:

```bash
lemma run --prefix ./policies pricing --rules=final_price
```

---

## 6. Technical implementation

### 6.1 Architecture overview

Lemma's implementation follows a multi-stage pipeline:

```
.lemma source
    ↓
[Lexer + Parser] (hand-written)
    ↓
Abstract Syntax Tree
    ↓
[Planning] (validation, dependency graph, compilation)
    ↓
Execution Plan (register-based instruction streams)
    ↓
[Virtual Machine]
    ↓
Values and Vetoes (+ explanations on demand)
```

### 6.2 Parser

The lexer and parser are hand-written Rust, covering:

- Token recognition (numbers, strings, dates, units)
- Expression parsing with correct operator precedence
- Spec structure and statements
- Error reporting with source locations and suggestions

The parser produces an Abstract Syntax Tree (AST) that captures the structure of the source spec.

### 6.3 Planning

After parsing, planning fully validates the spec before any evaluation:

1. **Type Checking**: Ensures operations are performed on compatible types
2. **Reference Resolution**: Verifies that all data, rule, and spec references are valid
3. **Temporal Resolution**: Resolves effective-dated spec versions, builds temporal slices, and checks coverage and interface compatibility of dependencies
4. **Circular Dependency Detection**: Identifies and reports circular rule references
5. **Resource Limits**: Bounds expression count, depth, and instruction-stream size

Planning compiles each rule into a register-based instruction stream and produces an immutable **execution plan**. Type mismatches and invalid operations are plan-time errors, never surprise runtime failures: after planning succeeds, evaluation is guaranteed to complete.

### 6.4 Virtual machine evaluation

A register-based virtual machine executes the compiled instruction streams. Compilation happens once per plan; evaluation dispatches flat instructions over a register file, computing only the requested rules in topological dependency order. One immutable plan serves concurrent requests, with per-request data carried in an overlay.

The VM handles:

- **Data resolution**: Direct lookup of data and rule references
- **Unless clauses**: "Last matching wins" semantics
- **Unit conversions**: Conversion between units within the same type definition
- **Type-aware operations**: Arithmetic and comparisons across numbers, quantities, ratios, dates, and ranges

Rule results are **values or vetoes**. When explanations are requested, the engine executes a second, source-shaped instruction stream and records what happened (branch decisions, the winning arm, operand values); the explanation is rendered from that recording, so it can never disagree with the result it explains.

Example:

```lemma
spec evaluation_example

data quantity: number

rule discount: 0%
  unless quantity >= 10  then 10%
  unless quantity >= 50  then 20%
```

Semantically, the bottommost matching condition wins: if `quantity >= 50` holds the result is `20%`; otherwise if `quantity >= 10` holds it is `10%`; otherwise `0%`.

### 6.5 Type System

Lemma's type system provides:

- **Automatic conversions**: Between units within the same type definition
- **Type safety**: Prevents invalid operations (e.g., adding different quantity types)
- **User-defined types**: Custom types with units, constraints, and validation
- **Validation**: Plan-time checking of type compatibility

### 6.6 Error Handling

Lemma distinguishes three failure modes (see [veto_semantics.md](veto_semantics.md)):

- **Parse and planning errors**: Invalid Lemma is rejected with source locations before evaluation
- **Vetoes**: Domain-level "no value" at runtime — missing data, division by zero from data, constraint violations, user `veto "reason"`
- **Panics**: Engine bugs crash immediately rather than producing a wrong value

### 6.7 Technology stack

- **Language**: Rust (for performance, safety, and modern tooling)
- **Parser**: Hand-written lexer and recursive parser
- **Runtime**: Register-based virtual machine executing compiled instruction streams
- **CLI**: Clap (for command-line interface)
- **Testing**: cargo-nextest suites, including differential tests pinning optimized and source instruction streams to identical results
- **Fuzzing**: cargo-fuzz for robustness testing

---

## 7. Use cases

### 7.1 Tax calculation

Progressive tax systems are naturally expressed in Lemma:

```lemma
spec tax_policy

data income:        85_000
data filing_status: "single"

rule taxable_income: income - standard_deduction

rule standard_deduction: 13_850
  unless filing_status is "married" then 27_700

rule tax_owed: 0
  unless taxable_income > 11_000
    then (taxable_income - 11_000) * 10%
  unless taxable_income > 44_725
    then 3_372.50 + (taxable_income - 44_725) * 12%
  unless taxable_income > 95_375
    then 9_875 + (taxable_income - 95_375) * 22%
```

### 7.2 E-commerce pricing

Complex pricing rules with volume discounts and customer tiers:

```lemma
spec pricing

data quantity:      number
data customer_tier: "standard"
data unit_price:    100

rule volume_discount: 0%
  unless quantity >= 10  then 5%
  unless quantity >= 50  then 10%
  unless quantity >= 100 then 15%

rule tier_discount: 0%
  unless customer_tier is "silver"   then 5%
  unless customer_tier is "gold"     then 10%
  unless customer_tier is "platinum" then 15%

rule best_discount: volume_discount
  unless tier_discount > volume_discount
    then tier_discount

rule final_price:
  quantity * unit_price * (1 - best_discount)
```

### 7.3 Insurance eligibility

Determining eligibility based on multiple criteria:

```lemma
spec insurance/eligibility

data age:                     number
data pre_existing_conditions: boolean
data employment_status:       text
data coverage_start:          date

rule eligible_age: age >= 18 and age <= 65

rule eligible_health: not pre_existing_conditions

rule eligible_employment: false
  unless employment_status is "full_time" then true
  unless employment_status is "part_time" then true

rule is_eligible:
  eligible_age and eligible_health and eligible_employment
  unless not eligible_age
    then veto "Age not within eligible range"
  unless not eligible_health
    then veto "Pre-existing conditions"
  unless not eligible_employment
    then veto "Employment status ineligible"
```

### 7.4 Shipping policy

Complex shipping calculations with multiple factors:

```lemma
spec shipping

data order_total: number
data mass: quantity
  -> unit kilogram 1.0
  -> unit pound 0.453592

data weight:       mass
data destination:  text
data is_expedited: false

rule base_rate: 12.99
  unless destination is "CA" then 25
  unless destination is "MX" then 22

rule weight_surcharge: 0
  unless weight > 5 kilogram  then 7.50
  unless weight > 20 kilogram
    then veto "Too heavy for standard shipping"

rule expedited_fee: 0
  unless is_expedited then 25

rule free_shipping:
  order_total >= 100 and destination is "US"

rule final_shipping:
  base_rate + weight_surcharge + expedited_fee
  unless free_shipping then 0
```

### 7.5 HR compensation policy

Complex compensation rules with multiple variables:

```lemma
spec compensation

data base_salary:        number
data years_of_service:   number
data performance_rating: number
data department:         text

rule tenure_bonus: 0
  unless years_of_service >= 5  then base_salary * 5%
  unless years_of_service >= 10 then base_salary * 10%
  unless years_of_service >= 15 then base_salary * 15%

rule performance_bonus: base_salary * 0%
  unless performance_rating >= 3 then base_salary * 5%
  unless performance_rating >= 4 then base_salary * 10%
  unless performance_rating >= 4.5
    then base_salary * 15%

rule department_bonus: 0
  unless department is "sales" then base_salary * 10%
  unless department is "engineering"
    then base_salary * 5%

rule total_compensation:
  base_salary + tenure_bonus + performance_bonus
  + department_bonus
```

---

## 8. Comparison with existing approaches

### 8.1 Traditional programming languages

**Imperative languages (Python, Java, JavaScript)**

Traditional imperative languages require explicit control flow and state management:

```python
def calculate_discount(quantity, is_vip):
    if is_vip:
        return 0.25
    elif quantity >= 50:
        return 0.20
    elif quantity >= 10:
        return 0.10
    else:
        return 0.0
```

Issues:
- Not readable by non-programmers
- Implementation details obscure intent
- Order matters (if/elif sequence)
- Type conversions are manual and error-prone
- No automatic unit handling

Lemma equivalent is clearer and matches natural language:

```lemma
spec pricing

data quantity: number
data is_vip:   boolean

rule discount: 0%
  unless quantity >= 10  then 10%
  unless quantity >= 50  then 20%
  unless is_vip          then 25%
```

### 8.2 Business rules engines

**Drools, FICO Blaze Advisor**

Traditional business rules engines use forward-chaining or pattern matching:

```drools
rule "VIP Discount"
when
    $order : Order(customer.vipStatus == true)
then
    $order.setDiscount(0.25);
end

rule "Volume Discount 50+"
when
    $order : Order(quantity >= 50, customer.vipStatus == false)
then
    $order.setDiscount(0.20);
end
```

Issues:
- Verbose syntax
- Rule priority and conflict resolution can be complex
- Stateful execution model
- Steep learning curve
- Often requires commercial licenses

Lemma provides simpler syntax with clear "last wins" semantics and is open source.

### 8.3 Domain-specific languages

**SQL, YAML-based configuration, JSON schemas**

Many systems use configuration languages for rules:

```yaml
discounts:
  - condition: "quantity >= 10"
    rate: 0.10
  - condition: "quantity >= 50"
    rate: 0.20
  - condition: "is_vip"
    rate: 0.25
```

Issues:
- Not executable without custom interpreter
- Limited expressiveness
- No type system
- No composition mechanisms
- Evaluation semantics unclear

Lemma is fully executable while remaining declarative.

### 8.4 Logic programming

**Prolog, Datalog**

Pure logic languages like Prolog are expressive but have usability issues:

```prolog
discount(Quantity, IsVip, 0.25) :- IsVip = true.
discount(Quantity, _, 0.20) :- Quantity >= 50.
discount(Quantity, _, 0.10) :- Quantity >= 10.
discount(_, _, 0.0).
```

Issues:
- Cryptic syntax for non-programmers
- No built-in units or rich types
- Requires understanding of unification and backtracking
- Debugging can be difficult

Lemma provides a natural language syntax while remaining deterministic and statically validated.

### 8.5 Spreadsheets

**Excel, Google Sheets**

Spreadsheets are widely used for business calculations:

```
=IF(is_vip, 0.25, IF(quantity >= 50, 0.20, IF(quantity >= 10, 0.10, 0)))
```

Issues:
- Nested IF statements become unreadable
- No version control or collaborative editing
- Difficult to test and validate
- Limited compositional capabilities
- Error-prone (e.g., off-by-one in cell references)

Lemma provides better readability, version control, testing, and composition.

---

## 9. Future work

### 9.1 Tables (collections)

**Planned feature**: Support data that hold multiple values with declarative operations (for example `sum`, `avg`, and `count` over named collections). Syntax and types are not finalized yet.

### 9.2 Language Server Protocol (LSP)

**Available today** — the language server is built into the CLI (`lemma lsp`), with a VS Code/Cursor extension, diagnostics, formatting, semantic tokens, and registry links. Installing the `lemma` CLI is the only requirement for editor support.

Further work: richer completion, go-to-definition, refactoring.

### 9.3 WebAssembly support

**Available today** via `@lemmabase/lemma-engine` (see [wasm.md](wasm.md)). Further work includes interactive documentation with live examples and browser-based policy simulators.

### 9.4 API and integration

**Available today**: REST evaluation and discovery via `lemma server` (OpenAPI at `/openapi.json`, interactive docs at `/docs`). WASM/npm and Rust crate bindings.

**Planned**: gRPC interface, additional native bindings (Python, Java), Kafka/event stream and database integrations.

---

## 10. Conclusion

### 10.1 Summary

Lemma represents a new approach to encoding business logic. By providing a declarative language that reads like natural language while remaining fully executable, Lemma bridges the gap between business stakeholders and software systems.

Key innovations include:

1. **Natural Language Semantics**: "Last matching wins" logic that mirrors how humans express rules
2. **Rich Type System**: User-defined types with units, constraints, and automatic conversions
3. **Type-Aware Arithmetic**: Intelligent handling of operations between different types
4. **Compositional Design**: Specs reference and extend each other, enabling modular rule libraries
5. **Veto Semantics**: Clear distinction between returning false and blocking a rule entirely
6. **Pure Rust Implementation**: Leveraging Rust's safety and performance for robust execution

### 10.2 Benefits

Organizations adopting Lemma can expect:

- **Faster Development**: Business stakeholders can write and validate rules directly
- **Reduced Errors**: Type safety and natural semantics eliminate common bugs
- **Better Communication**: Shared language between business and technical teams
- **Easier Auditing**: Rules are self-documenting and traceable
- **Lower Maintenance**: Rule changes don't require code deployments
- **Greater Agility**: Adapt to changing business requirements quickly

### 10.3 Applicability

Lemma is particularly well-suited for:

- **Financial Services**: Tax calculations, loan eligibility, investment rules
- **Insurance**: Underwriting rules, claims processing, premium calculations
- **E-commerce**: Pricing, discounts, shipping policies, promotions
- **Human Resources**: Compensation policies, benefits eligibility, time-off rules
- **Compliance**: Regulatory rules, data retention policies, access controls
- **Healthcare**: Treatment protocols, eligibility determination, billing rules
- **Logistics**: Routing rules, capacity planning, scheduling policies

### 10.4 Getting started

Lemma is open source under the Apache 2.0 license. To get started:

```bash
# Install
cargo install lemma

# Create a rule file
cat > example.lemma << 'EOF'
spec example

data age: 25
data income: 50000

rule can_vote: false
  unless age >= 18 then true

rule tax_bracket: "10%"
  unless income > 44725 then "12%"
  unless income > 95375 then "22%"
EOF

# Provide data values
lemma run example income=100000
```

Documentation, examples, and source code are available at:
- Repository: https://github.com/lemma/lemma
- Documentation: https://github.com/lemma/lemma/tree/main/documentation
- Examples: https://github.com/lemma/lemma/tree/main/documentation/examples

---

## Appendix A: Complete example

Here is a complete example demonstrating many of Lemma's features:

```lemma
spec employee/compensation
"""
Company Compensation Policy
Effective Date: 2024-01-01

This spec encodes the complete compensation rules including
base salary, bonuses, equity, and benefits.
"""

uses lemma units

data employee_id:        text
data base_salary:        number
data years_of_service:   number
data performance_rating: number
data department:         text
data location:           text
data is_manager:         false

rule cost_of_living_adjustment: 0%
  unless location is "San Francisco" then 25%
  unless location is "New York"      then 20%
  unless location is "Seattle"       then 15%

rule adjusted_salary:
  base_salary * (1 + cost_of_living_adjustment)

rule tenure_bonus_rate: 0%
  unless years_of_service >= 5  then 5%
  unless years_of_service >= 10 then 10%
  unless years_of_service >= 15 then 15%

rule tenure_bonus: adjusted_salary * tenure_bonus_rate

rule performance_multiplier: 1
  unless performance_rating >= 3   then 1
  unless performance_rating >= 4   then 1.5
  unless performance_rating >= 4.5 then 2

rule target_bonus_rate: 10%
  unless is_manager            then 20%
  unless department is "sales" then 30%

rule performance_bonus:
  adjusted_salary * target_bonus_rate
  * performance_multiplier

rule equity_grant_value: 0
  unless is_manager then adjusted_salary * 25%
  unless years_of_service < 1
    then veto "Not eligible for equity in first year"

rule vacation_days: 15 days
  unless years_of_service >= 5  then 20 days
  unless years_of_service >= 10 then 25 days
  unless is_manager             then 30 days

rule total_compensation:
  adjusted_salary + tenure_bonus + performance_bonus
  + equity_grant_value
```

Query examples:

```bash
lemma run compensation --rules=total_compensation \
  base_salary=120000 years_of_service=7 performance_rating=4.2 \
  location="New York" department=engineering is_manager=true

lemma run compensation --rules=vacation_days \
  years_of_service=12 is_manager=true
```

---

## Appendix B: Grammar summary

Core syntax elements:

```
Spec:
  spec <name>
  ["""documentation"""]
  <statements>

Data Definition:
  data <name>: <value>
  data <name>: <type>

Data Binding:
  data <qualified.name>: <value>

Rule Definition:
  rule <name>: <expression>
  [unless <condition> then <expression>]*

Expressions:
  <arithmetic>        // +, -, *, /, %, ^
  <comparison>        // >, <, >=, <=, is, is not
  <logical>          // and, not
  <mathematical>     // sqrt, sin, cos, tan, log, exp, abs, floor, ceil, round
  <unit-conversion>  // <value> as <unit>
  <reference>        // name or path (resolved to data or rule)
  <data-reference>   // <name>
  veto [<message>]

Literals:
  <number>           // 42, 3.14, 1.23e10
  <text>             // "hello world"
  <boolean>          // true, false, yes, no, accept, reject
  <date>             // 2024-01-15, 2024-01-15T14:30:00Z
  <ratio>            // 15%, 15 percent, 5 permille, 5%%
  <unit-value>       // 5 eur (requires quantity type + unit); 3 weeks (trait-duration quantity)
```

---

**Document Version**: 1.1
**Last Updated**: June 2026
**License**: Apache 2.0
**Authors**: Ben Rogmans
**Contact**: https://github.com/lemma/lemma
