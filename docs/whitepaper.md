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

Lemma bridges this gap. It is a declarative language designed specifically for expressing business logic in a form that flows like natural language. Lemma documents can encode pricing rules, tax calculations, eligibility criteria, contracts, and policies in a way that business stakeholders can read and validate, while software systems can enforce and automate them.

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
- **Types matter**: Built-in support for money, dates, durations, units, and percentages with automatic conversions eliminates a major source of bugs.
- **Last wins semantics**: The "unless" clause uses "last matching wins" logic that mirrors how humans naturally express exceptions and special cases.
- **Fully executable**: Despite its natural syntax, Lemma uses a pure Rust evaluator, providing rigorous logical inference and deterministic evaluation.
- **Composable**: Documents reference and extend each other, enabling modular rule design.
- **Auditable**: Every decision can be traced back to specific facts and rules with operation records.

### 1.3 Example

Consider a simple pricing rule:

```lemma
doc pricing

fact quantity   = [number]
fact is_vip     = false

rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip         then 25%

rule price = 200 eur - discount?
```

This document is immediately readable by business stakeholders while being fully executable by software systems. The semantics are clear: start with no discount, but if quantity is 10 or more, apply 10%; if 50 or more, apply 20%; if customer is VIP, apply 25%. The last matching condition wins, so a VIP customer with 100 units gets 25%, not 20%.

---

## 2. Design philosophy

### 2.1 Natural language semantics

Lemma's syntax is designed to mirror how people naturally express business rules. Consider how you might explain a shipping policy:

> "Shipping is $12.99, unless you're in Canada then it's $25, unless you're ordering over 100 dollars then it's free."

Lemma encodes this exactly as stated:

```lemma
rule shipping = 12.99 usd
  unless destination == "CA" then 25.00 usd
  unless order_total >= 100 usd then 0 usd
```

This "last matching wins" semantic is counterintuitive to programmers accustomed to "first match" or "most specific match" logic, but it aligns perfectly with natural language. When we say "X, unless Y, unless Z," we mean that Z overrides Y, which overrides X.

### 2.2 Declarative by design

Lemma is purely declarative. You describe *what* should be true, not *how* to compute it. This has several advantages:

1. **Clarity**: Rules state relationships between values without implementation details.
2. **Optimization**: The execution engine can optimize and reorder operations.
3. **Reasoning**: Logical inference can derive implications and detect contradictions.
4. **Parallelization**: Declarative semantics enable safe concurrent evaluation.

### 2.3 Type safety without syntax overhead

Programming languages typically require verbose type annotations. Lemma infers types from literals while providing a rich type system:

```lemma
fact salary = 75000 usd          // Money type inferred
fact vacation = 3 weeks          // Duration type inferred
fact weight = 15 kilograms       // Mass type inferred
fact deadline = 2024-12-31       // Date type inferred
fact tax_rate = 22%              // Percentage type inferred
```

The type system prevents nonsensical operations (you can't add a date to a weight) while enabling automatic unit conversions:

```lemma
fact weight = 70 kilograms
rule weight_in_pounds = weight in pounds  // Automatic conversion
```

### 2.4 Composition over configuration

Lemma encourages building complex systems from simple, composable pieces. Documents can reference other documents, rules can reference other rules, and facts can be overridden in specific contexts:

```lemma
doc base_employee
fact salary = 50000 usd
fact bonus_rate = 5%

doc manager
fact employee = doc base_employee
fact employee.salary = 80000 usd
fact employee.bonus_rate = 15%

rule manager_bonus = employee.salary * employee.bonus_rate
```

This compositional design enables reusable rule libraries and reduces duplication.

---

## 3. Language features

### 3.1 Facts

Facts are named values of a certain type. They represent inputs to the system:

```lemma
fact name = "Alice"
fact age = 35
fact start_date = 2024-01-15
fact salary = 75000 usd
fact is_manager = true
```

Facts can also be type annotations, declaring expected inputs without values:

```lemma
fact birth_date = [date]
fact employee_count = [number]
fact location = [text]
```

### 3.2 Rules

Rules compute values based on facts and other rules. A rule has a name, a default value, and optional "unless" clauses:

```lemma
rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip then 25%
```

Rules can reference other rules using the `?` suffix:

```lemma
rule is_adult = age >= 18
rule has_license = license_status == "valid"
rule can_drive = is_adult? and has_license?
```

### 3.3 Unless clauses

The "unless" clause is Lemma's primary conditional construct. Unlike if-else chains that stop at the first match, unless clauses use "last matching wins" semantics:

```lemma
rule status = "standard"
  unless score >= 70 then "good"
  unless score >= 90 then "excellent"
```

If `score` is 95, the result is "excellent" (not "good"), because the last matching condition wins. This matches natural language: "It's standard, unless your score is at least 70 then good, unless it's at least 90 then excellent."

### 3.4 Veto

While "unless" clauses override values, sometimes you need to block a rule entirely. The `veto` keyword does this:

```lemma
rule loan_approval = reject
  unless credit_score >= 600 then accept
  unless age < 18 then veto "Must be 18 or older"
  unless bankruptcy_flag then veto "Cannot approve due to bankruptcy"
```

When a veto applies, the rule produces no valid result. This is useful for validation and hard constraints.

### 3.5 Operators

Lemma provides comprehensive operators for arithmetic, comparison, logical operations, and mathematical functions:

**Arithmetic**: `+`, `-`, `*`, `/`, `%`, `^`

```lemma
rule compound = principal * (1 + rate) ^ years
```

**Comparison**: `>`, `<`, `>=`, `<=`, `==`, `!=`, `is`, `is not`

```lemma
rule is_eligible = age >= 18 and income > 30000
```

**Logical**: `and`, `or`, `not`, `have`, `have not`

```lemma
rule can_approve = is_manager? and not is_suspended?
rule has_email = have customer.email
```

**Mathematical**: `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`

```lemma
rule hypotenuse = sqrt(a^2 + b^2)
```

### 3.6 Type-aware arithmetic

Lemma intelligently handles arithmetic between different types. When you write:

```lemma
rule discounted_price = 200 eur - 25%
```

Lemma understands that subtracting a percentage from money means "subtract 25% of the money value," producing `150 eur`.

---

## 4. Type system

### 4.1 Primitive types

Lemma provides several primitive types:

- **Number**: Integers and floating-point values
- **Text**: String literals
- **Boolean**: true/false, yes/no, accept/reject
- **Date**: ISO 8601 format dates and datetimes
- **Regex**: Pattern matching with standard regex syntax

```lemma
fact count = 42
fact name = "Alice"
fact is_active = true
fact deadline = 2024-12-31
fact email_pattern = /^[\w]+@[\w]+\.[\w]+$/
```

### 4.2 Unit types

Lemma has built-in support for physical and business units:

**Money**: USD, EUR, GBP, JPY, CNY, CHF, CAD, AUD, INR, etc.

```lemma
fact revenue = 1000000 usd
fact cost = 500000 eur
```

**Mass**: kilogram, gram, milligram, pound, ounce

```lemma
fact weight = 75 kilograms
fact package_weight = 5 pounds
```

**Length**: kilometer, meter, centimeter, millimeter, mile, foot, inch

```lemma
fact distance = 100 kilometers
fact height = 6 feet
```

**Duration**: year, month, week, day, hour, minute, second

```lemma
fact project_duration = 6 months
fact meeting_length = 90 minutes
```

**Temperature**: celsius, fahrenheit, kelvin

```lemma
fact room_temp = 22 celsius
```

**Other Units**: volume, power, force, pressure, energy, frequency, data size

### 4.3 Automatic unit conversion

Lemma automatically converts between compatible units:

```lemma
fact weight = 70 kilograms
rule weight_in_pounds = weight in pounds  // Returns 154.32 pounds

fact distance = 5 kilometers
rule distance_in_miles = distance in miles  // Returns 3.107 miles
```

This eliminates manual conversion logic and reduces errors in international applications.

### 4.4 Percentage type

Percentages are a first-class type in Lemma:

```lemma
fact tax_rate = 15%
fact discount = 25%
fact completion = 87.5%
```

Percentages interact intelligently with other types in arithmetic operations, automatically applying proportional calculations.

---

## 5. Compositional architecture

### 5.1 Documents

Every Lemma file contains one or more documents. Documents are namespaces that encapsulate related facts and rules:

```lemma
doc employee/benefits
"""
Company benefits policy for full-time employees
"""

fact base_vacation = 15 days
fact years_of_service = [number]

rule vacation_days = base_vacation
  unless years_of_service >= 5 then 20 days
  unless years_of_service >= 10 then 25 days
```

Document names can be hierarchical (using `/` separators), enabling logical organization of rule libraries.

### 5.2 Document references

Documents can reference other documents, enabling composition and reuse:

```lemma
doc base_employee
fact name = "John Doe"
fact salary = 50000 usd

doc manager
fact employee = doc base_employee
fact employee.salary = 80000 usd

rule manager_bonus = employee.salary * 0.15
```

This pattern allows creating specialized variants of base documents without duplication.

### 5.3 Fact overrides

Facts can be overridden at different levels:

```lemma
doc pricing
fact quantity = 100
fact unit_price = 50 usd

doc wholesale_pricing
fact pricing.quantity = 1000
fact pricing.unit_price = 35 usd

rule total = pricing.quantity * pricing.unit_price
```

This enables scenario modeling and context-specific rule evaluation.

### 5.4 Workspace model

Lemma supports loading multiple documents together in a workspace. Documents can reference each other, creating a network of related rules:

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

The CLI can show a workspace summary:

```bash
lemma workspace ./policies/
```

Or run queries against documents in a workspace:

```bash
lemma run pricing final_price --workdir ./policies/
```

---

## 6. Technical implementation

### 6.1 Architecture overview

Lemma's implementation follows a multi-stage pipeline:

```
.lemma source
    ↓
[Parser] (Pest grammar)
    ↓
Abstract Syntax Tree
    ↓
[Semantic Validator]
    ↓
Semantic Model
    ↓
[Pure Rust Evaluator]
    ↓
Typed Values
```

### 6.2 Parser

The parser is built using Pest, a parsing expression grammar (PEG) parser generator for Rust. The grammar (`lemma.pest`) defines the complete syntax of Lemma, including:

- Token recognition (numbers, strings, dates, units)
- Expression parsing with correct operator precedence
- Document structure and statements
- Error recovery and reporting

The parser produces an Abstract Syntax Tree (AST) that captures the structure of the source document.

### 6.3 Semantic validation

After parsing, the semantic validator performs several checks:

1. **Type Checking**: Ensures operations are performed on compatible types
2. **Reference Resolution**: Verifies that all fact and rule references are valid
3. **Scope Checking**: Ensures identifiers are used in appropriate scopes
4. **Circular Dependency Detection**: Identifies and reports circular rule references

The validator produces a semantic model that captures the meaning of the document, independent of its syntactic representation.

### 6.4 Pure Rust Evaluator

The evaluator processes the semantic model directly in Rust, providing fast and deterministic execution without external dependencies.

The evaluator handles:

- **Fact resolution**: Direct lookup of facts and rule references
- **Rule evaluation**: Recursive evaluation of expressions with proper dependency ordering
- **Unless clauses**: "Last match wins" semantics implemented through conditional evaluation
- **Unit conversions**: Built-in conversion between compatible unit types
- **Type-aware operations**: Automatic type checking and conversion for arithmetic and comparisons

Example evaluation:

```lemma
rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
```

The evaluator processes this by:
1. Evaluating the condition `quantity >= 50`
2. If true, returning `20%`
3. Otherwise, evaluating `quantity >= 10`
4. If true, returning `10%`
5. Otherwise, returning `0%`

### 6.5 Type System

Lemma's type system provides:

- **Automatic conversions**: Between compatible units (e.g., kilograms to pounds)
- **Type safety**: Prevents invalid operations (e.g., adding different currencies)
- **Rich types**: Money, dates, durations, percentages, and physical units
- **Validation**: Compile-time checking of type compatibility

### 6.6 Error Handling

The evaluator provides comprehensive error handling:

- **Parse errors**: Clear syntax error messages with source locations
- **Semantic errors**: Type mismatches and invalid references
- **Runtime errors**: Division by zero, invalid operations
- **Circular dependencies**: Detection and reporting of rule cycles

### 6.7 Technology stack

- **Language**: Rust (for performance, safety, and modern tooling)
- **Parser**: Pest (for maintainable, readable grammar)
- **Runtime**: Pure Rust evaluator (for logical inference)
- **CLI**: Clap (for command-line interface)
- **Testing**: Built-in Rust testing + property-based testing with Proptest
- **Fuzzing**: cargo-fuzz for robustness testing

---

## 7. Use cases

### 7.1 Tax calculation

Progressive tax systems are naturally expressed in Lemma:

```lemma
doc tax_policy

fact income = 85000 usd
fact filing_status = "single"

rule taxable_income = income - standard_deduction?

rule standard_deduction = 13850 usd
  unless filing_status == "married" then 27700 usd

rule tax_owed = 0 usd
  unless taxable_income? > 11000 usd
    then (taxable_income? - 11000 usd) * 10%
  unless taxable_income? > 44725 usd
    then 3372.50 usd + (taxable_income? - 44725 usd) * 12%
  unless taxable_income? > 95375 usd
    then 9875 usd + (taxable_income? - 95375 usd) * 22%
```

### 7.2 E-commerce pricing

Complex pricing rules with volume discounts and customer tiers:

```lemma
doc pricing

fact quantity = [number]
fact customer_tier = "standard"
fact unit_price = 100 usd

rule volume_discount = 0%
  unless quantity >= 10 then 5%
  unless quantity >= 50 then 10%
  unless quantity >= 100 then 15%

rule tier_discount = 0%
  unless customer_tier == "silver" then 5%
  unless customer_tier == "gold" then 10%
  unless customer_tier == "platinum" then 15%

rule best_discount = volume_discount?
  unless tier_discount? > volume_discount? then tier_discount?

rule final_price = quantity * unit_price * (1 - best_discount?)
```

### 7.3 Insurance eligibility

Determining eligibility based on multiple criteria:

```lemma
doc insurance/eligibility

fact age = [number]
fact pre_existing_conditions = [boolean]
fact employment_status = [text]
fact coverage_start = [date]

rule eligible_age = age >= 18 and age <= 65

rule eligible_health = not pre_existing_conditions

rule eligible_employment = employment_status == "full_time"
  or employment_status == "part_time"

rule is_eligible = eligible_age? and eligible_health? and eligible_employment?
  unless eligible_age? == false then veto "Age not within eligible range"
  unless eligible_health? == false then veto "Pre-existing conditions"
  unless eligible_employment? == false then veto "Employment status ineligible"
```

### 7.4 Shipping policy

Complex shipping calculations with multiple factors:

```lemma
doc shipping

fact order_total = [money]
fact weight = [mass]
fact destination = [text]
fact is_expedited = false

rule base_rate = 12.99 usd
  unless destination == "CA" then 25.00 usd
  unless destination == "MX" then 22.00 usd

rule weight_surcharge = 0 usd
  unless weight > 5 kilograms then 7.50 usd
  unless weight > 20 kilograms then veto "Too heavy for standard shipping"

rule expedited_fee = 0 usd
  unless is_expedited then 25.00 usd

rule free_shipping = order_total >= 100 usd and destination == "US"

rule final_shipping = base_rate? + weight_surcharge? + expedited_fee?
  unless free_shipping? then 0 usd
```

### 7.5 HR compensation policy

Complex compensation rules with multiple variables:

```lemma
doc compensation

fact base_salary = [money]
fact years_of_service = [number]
fact performance_rating = [number]
fact department = [text]

rule tenure_bonus = 0 usd
  unless years_of_service >= 5 then base_salary * 5%
  unless years_of_service >= 10 then base_salary * 10%
  unless years_of_service >= 15 then base_salary * 15%

rule performance_bonus = base_salary * 0%
  unless performance_rating >= 3 then base_salary * 5%
  unless performance_rating >= 4 then base_salary * 10%
  unless performance_rating >= 4.5 then base_salary * 15%

rule department_bonus = 0 usd
  unless department == "sales" then base_salary * 10%
  unless department == "engineering" then base_salary * 5%

rule total_compensation = base_salary + tenure_bonus?
                          + performance_bonus? + department_bonus?
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
rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip then 25%
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

Lemma provides a natural language syntax while leveraging logical inference through its pure Rust evaluator.

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

### 9.1 User-defined types

**Planned feature**: Allow users to define custom types within documents.

```lemma
type priority = low | medium | high | critical

type custom_currency = cryptocurrency
  convert 1 bitcoin = 45000 usd
  convert 1 ethereum = 2500 usd

fact task_priority = [priority]
fact wallet_balance = 0.5 bitcoin
```

This will enable domain-specific types and custom unit conversions.

### 9.2 Multi-facts (collections)

**Planned feature**: Support facts that hold multiple values with declarative operations.

```lemma
fact employees = multi [text]
fact salaries = multi [money]

rule total_payroll = sum of salaries
rule average_salary = avg of salaries
rule employee_count = count of employees
rule high_earners = salaries where value > 100000 usd
```

This will enable working with collections of data in a declarative way.

### 9.3 Language Server Protocol (LSP)

**Planned feature**: IDE support for `.lemma` files with:
- Syntax highlighting
- Real-time error checking
- Auto-completion
- Go-to-definition
- Hover documentation
- Refactoring support

### 9.4 WebAssembly support

**Planned feature**: Compile Lemma to WebAssembly for browser-based evaluation, enabling:
- Client-side rule evaluation
- Interactive documentation with live examples
- Browser-based policy simulators
- No server round-trip required

### 9.5 API and integration

**Planned features**:
- REST API server for rule evaluation
- gRPC interface for high-performance integrations
- Native bindings for Python, JavaScript, Java
- Kafka/event stream integrations
- Database query integrations

---

## 10. Conclusion

### 10.1 Summary

Lemma represents a new approach to encoding business logic. By providing a declarative language that reads like natural language while remaining fully executable, Lemma bridges the gap between business stakeholders and software systems.

Key innovations include:

1. **Natural Language Semantics**: "Last matching wins" logic that mirrors how humans express rules
2. **Rich Type System**: Built-in support for money, dates, durations, units, and percentages
3. **Type-Aware Arithmetic**: Intelligent handling of operations between different types
4. **Compositional Design**: Documents reference and extend each other, enabling modular rule libraries
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
doc example

fact age = 25
fact income = 50000 usd

rule can_vote = false
  unless age >= 18 then true

rule tax_bracket = "10%"
  unless income > 44725 usd then "12%"
  unless income > 95375 usd then "22%"
EOF

# Override facts
lemma run example income="100000 usd"
```

Documentation, examples, and source code are available at:
- Repository: https://github.com/benrogmans/lemma
- Documentation: https://github.com/benrogmans/lemma/tree/main/docs
- Examples: https://github.com/benrogmans/lemma/tree/main/docs/examples

---

## Appendix A: Complete example

Here is a complete example demonstrating many of Lemma's features:

```lemma
doc employee/compensation
"""
Company Compensation Policy
Effective Date: 2024-01-01

This document encodes the complete compensation rules including
base salary, bonuses, equity, and benefits.
"""

fact employee_id = [text]
fact base_salary = [money]
fact years_of_service = [number]
fact performance_rating = [number]
fact department = [text]
fact location = [text]
fact is_manager = false

rule cost_of_living_adjustment = 0%
  unless location == "San Francisco" then 25%
  unless location == "New York" then 20%
  unless location == "Seattle" then 15%

rule adjusted_salary = base_salary * (1 + cost_of_living_adjustment?)

rule tenure_bonus_rate = 0%
  unless years_of_service >= 5 then 5%
  unless years_of_service >= 10 then 10%
  unless years_of_service >= 15 then 15%

rule tenure_bonus = adjusted_salary? * tenure_bonus_rate?

rule performance_multiplier = 0
  unless performance_rating >= 3.0 then 1.0
  unless performance_rating >= 4.0 then 1.5
  unless performance_rating >= 4.5 then 2.0

rule target_bonus_rate = 10%
  unless is_manager then 20%
  unless department == "sales" then 30%

rule performance_bonus = adjusted_salary? * target_bonus_rate? * performance_multiplier?

rule equity_grant_value = 0 usd
  unless is_manager then adjusted_salary? * 25%
  unless years_of_service < 1 then veto "Not eligible for equity in first year"

rule vacation_days = 15 days
  unless years_of_service >= 5 then 20 days
  unless years_of_service >= 10 then 25 days
  unless is_manager then 30 days

rule total_compensation = adjusted_salary? + tenure_bonus?
                          + performance_bonus? + equity_grant_value?

rule compensation_summary = "Total: " + total_compensation?
```

Query examples:

```bash
lemma run compensation total_compensation \
  base_salary="120000 usd" years_of_service=7 performance_rating=4.2 \
  location="New York" department=engineering is_manager=true

lemma run compensation vacation_days \
  years_of_service=12 is_manager=true
```

---

## Appendix B: Grammar summary

Core syntax elements:

```
Document:
  doc <name>
  ["""documentation"""]
  <statements>

Fact Definition:
  fact <name> = <value>
  fact <name> = [<type>]

Fact Override:
  fact <qualified.name> = <value>

Rule Definition:
  rule <name> = <expression>
  [unless <condition> then <expression>]*

Expressions:
  <arithmetic>        // +, -, *, /, %, ^
  <comparison>        // >, <, >=, <=, ==, !=, is, is not
  <logical>          // and, or, not, have, have not
  <mathematical>     // sqrt, sin, cos, tan, log, exp, abs, floor, ceil, round
  <unit-conversion>  // <value> in <unit>
  <rule-reference>   // <name>?
  <fact-reference>   // <name>
  veto [<message>]

Literals:
  <number>           // 42, 3.14, 1.23e10
  <text>             // "hello world"
  <boolean>          // true, false, yes, no, accept, reject
  <date>             // 2024-01-15, 2024-01-15T14:30:00Z
  <percentage>       // 15%, 100%
  <unit-value>       // 100 usd, 5 kilograms, 3 weeks
  <regex>            // /[A-Z]{3}-\d{4}/
```

---

**Document Version**: 1.0
**Last Updated**: October 2025
**License**: Apache 2.0
**Authors**: Ben Rogmans
**Contact**: https://github.com/benrogmans/lemma

