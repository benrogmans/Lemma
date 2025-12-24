---
layout: default
title: User Types
---

# User Types Implementation Plan

## Overview

Extend Lemma to support user-defined types within documents. Types define units with optional numeric values, enabling custom enumerations, priorities, statuses, and domain-specific measurements. Types are always defined within the scope of a doc. Types from other docs can be imported using `type x from doc y`, or accessed via document references using `doc_ref.type_name.unit_name` syntax.

**Note**: `Duration` remains a core engine type (not user-defined) due to its special calendar-aware semantics for DateTime operations (e.g., `Year` and `Month` units require special handling).

## Design

### Doc-Scoped Types

```lemma
doc lemma_standard

type mass
unit gram = 1
unit kilogram = gram * 1000
unit pound = kilogram * 0.453592

type temperature
unit celsius = 1
unit fahrenheit = celsius * 9/5 + 32
unit kelvin = celsius + 273.15

doc finance

fact ex_rate_eur = [number]

type money
unit usd = 1.00
unit eur = usd * ex_rate_eur
```

Types defined in a doc are local to that doc. Units can reference facts from the same doc. Units are unqualified within the doc.

### Type Import

```lemma
"""
Coffee Shop Pricing

A simple example showing how facts and rules work together.
Perfect for understanding the basics of Lemma.
"""
doc coffee_order

type coffee
unit espresso
unit latte
unit cappuccino
unit mocha

type size
unit small
unit medium
unit large

type money from finance
```

Types from other docs can be imported using `type x from doc y` syntax. Imported types bring all their units into the current doc's scope.

### Type Access Patterns

Types can be accessed in two ways:

1. **Import the type**: Units become available unqualified in the current scope
2. **Access via document reference**: Use `doc_ref.type_name.unit_name` syntax

```lemma
doc order_workflow

type status
unit pending
unit approved
unit shipped

type priority
unit low = 1
unit medium = 2
unit high = 3

fact status = [status]

doc shipment
type priority from order_workflow

fact order = doc order_workflow
fact order.status = order.status.approved
fact priority = [priority]

rule can_ship = order.status is order.status.approved

rule asap = priority > 2
rule asap = priority > medium
```

In this example:
- `type priority from order_workflow` imports the type, so `medium` is available unqualified
- `order.status.approved` accesses the `status` type from `order_workflow` via document reference
- `order.status` is the fact reference, `order.status.approved` is the type unit access
- Note that `order_workflow` has both `type status` and `fact status = [status]` - this is allowed since the fact is not a document reference

### Type Annotations

Type annotations use the type name in brackets:

```lemma
doc shipment

type order_status
unit draft
unit approved

fact weight = [mass]
fact status = [order_status]
```

### Name Collision Constraints

To maintain unambiguous syntax, the following constraint applies within a single document:

**A fact cannot use the same name as a type when it holds a document reference**: If a document has `type status`, it cannot have `fact status = doc ...`. However, `fact status = [status]` (a regular fact) is allowed.

This ensures that `order.status.approved` is unambiguous:
- `order.status` refers to the fact or document reference (statically validated)
- `order.status.approved` refers to the type unit (statically validated)
- The constraint prevents ambiguity when accessing types via document references
- All references are validated at analysis time - missing references are errors


## Grammar Changes

**File: `lemma/src/parser/lemma.pest`**

```pest
document = { SOI ~ doc ~ doc_name ~ commentary? ~ (type_def | type_import | fact | rule)* ~ EOI }
doc = { "doc" }

type_def = { "type" ~ identifier ~ unit+ }
type_import = { "type" ~ identifier ~ "from" ~ doc_reference }
unit = { "unit" ~ identifier ~ ("=" ~ expression)? }
```

This means that all existing types, in semantic and all processing, need to be removed and replaced by a generic NumericUnit type. This is a major clean up.

### Semantic Validation

**File: `lemma/src/semantic.rs`**

Add validation to enforce name collision constraints:

Reject documents where a fact that holds a document reference uses the same name as a type (i.e., if `type status` exists, reject `fact status = doc ...`).

This check ensures `doc_ref.type_name.unit_name` syntax remains unambiguous. Note that types and regular facts (non-document references) can share names.

## Expression Inversion

**File: `lemma/src/evaluator/expression_inverter.rs`** (new)

Use general symbolic inversion. Find relationships between the units, invert by unwinding operations:

**Algorithm:**

```rust
fn invert_expression(expr: &Expression, base_unit: &str) -> Result<String> {
    // 1. Verify base_unit appears exactly once
    let count = count_variable_occurrences(expr, base_unit);
    if count == 0 {
        return Err("Base unit not referenced");
    }
    if count > 1 {
        return Err("Base unit appears multiple times - cannot invert");
    }

    // 2. Traverse expression tree, unwinding operations
    let inverted = invert_tree(expr, base_unit, "Value")?;

    Ok(format!("Result is {}", inverted))
}

fn invert_tree(expr: &Expression, base_unit: &str, current_var: &str) -> Result<String> {
    match &expr.kind {
        // Found base unit - return current accumulated variable
        ExpressionKind::Literal(unit) if unit == base_unit => {
            Ok(current_var.to_string())
        }

        // Arithmetic - recurse into side containing base_unit
        ExpressionKind::Arithmetic(left, op, right) => {
            if contains_variable(left, base_unit) {
                // Base is on left, invert operation
                let new_var = match op {
                    Add => format!("({} - ({}))", current_var, transpile(right)),
                    Sub => format!("({} + ({}))", current_var, transpile(right)),
                    Mul => format!("({} / ({}))", current_var, transpile(right)),
                    Div => format!("({} * ({}))", current_var, transpile(right)),
                };
                invert_tree(left, base_unit, &new_var)
            } else {
                // Base is on right, invert differently
                let new_var = match op {
                    Add => format!("({} - ({}))", current_var, transpile(left)),
                    Sub => format!("(({}) - {})", transpile(left), current_var),
                    Mul => format!("({} / ({}))", current_var, transpile(left)),
                    Div => format!("(({}) / {})", transpile(left), current_var),
                };
                invert_tree(right, base_unit, &new_var)
            }
        }

        _ => Err("Unsupported expression in unit definition")
    }
}
```

**Examples:**

```
celsius * 9/5 + 32  →  (Value - 32) * 5/9
kilogram * 1000     →  Value / 1000
pound / 16          →  Value * 16
```

Works for any expression where the base unit appears exactly once.

## Systeme International d'unites

A document to be published on LemmaBase, publically.

```lemma
doc lemma/si_units

type mass
unit kilogram = 1
unit gram = kilogram * 0.001
unit pound = kilogram * 0.453592
unit ounce = pound / 16

type length
unit meter = 1
unit kilometer = meter * 1000
unit foot = meter * 0.3048
unit inch = foot / 12

type temperature
unit celsius = 1
unit fahrenheit = celsius * 9/5 + 32
unit kelvin = celsius + 273.15

type volume
unit liter = 1
unit milliliter = liter * 0.001
unit gallon = liter * 3.78541

type power
unit watt = 1
unit kilowatt = watt * 1000
unit horsepower = watt * 745.7

type energy
unit joule = 1
unit kilojoule = joule * 1000
unit calorie = joule * 4.184

type data_size
unit byte = 1
unit kilobyte = byte * 1000
unit megabyte = kilobyte * 1000
unit gigabyte = megabyte * 1000
unit kibibyte = byte * 1024
unit mebibyte = kibibyte * 1024

type pressure
unit pascal = 1
unit kilopascal = pascal * 1000
unit bar = pascal * 100000

type frequency
unit hertz = 1
unit kilohertz = hertz * 1000
unit megahertz = kilohertz * 1000

type force
unit newton = 1
unit kilonewton = newton * 1000
unit lbf = newton * 4.44822
```
Other docs can then use it as follows:

```lemma
doc shipping
type mass from @lemma/si_units

fact package_weight = [mass]
```