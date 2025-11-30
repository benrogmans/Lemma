---
layout: default
title: User Types
---

# User Types Implementation Plan

## Overview

Extend Lemma to support user-defined types within documents. Types define units with optional numeric values, enabling custom enumerations, priorities, statuses, and domain-specific measurements. Types are scoped to the workspace.

## Design

### Standard Library Types (Global)

```lemma
type mass
unit gram = 1
unit kilogram = gram * 1000
unit pound = kilogram * 0.453592

type temperature
unit celsius = 1
unit fahrenheit = celsius * 9/5 + 32
unit kelvin = celsius + 273.15

type currency
"""
No relationships between currencies (yet) - no conversions possible.
"""
unit eur = 1
unit usd = 1
unit gbp = 1
```

Loaded at engine initialization. Available everywhere without qualification.

### User Types (Doc-Scoped)

```lemma
type order_status
unit draft
unit pending
unit approved

type priority
"""
A generic priority type
"""

unit low = 1
unit medium = 2
unit high = 3


doc order_workflow

fact status = [type @coolblue/order_status]
fact urgency = [type priority]

rule can_process = status is order_status.approved
rule needs_escalation = urgency > 2
```

Types defined in a doc are local to that doc. Units are unqualified within the doc.

### Cross-Doc Access

```lemma
doc shipment
fact order = doc order_workflow
fact order.status = approved

rule can_ship = order.status is order_status.approved
```

Access units from other docs using doc reference syntax: `type_name.unit_name`

### Type Annotations

```lemma
type order_status
unit draft
unit approved

doc order_workflow
fact status = [order_status]

doc shipment
fact weight = [mass]
fact order_status = [order_status]
```

Type annotations use the type name in brackets.

## Grammar Changes

**File: `lemma/src/parser/lemma.pest`**

```pest
document = { SOI ~ doc ~ doc_name ~ commentary? ~ (type_def | fact | rule)* ~ EOI }
doc = { "doc" }

type_def = { "type" ~ identifier ~ unit+ }
unit = { "unit" ~ identifier ~ ("=" ~ expression)? }
```


This means that all existing types, in semantic and all processing, need to be removed and replaced by a generic NumericUnit type. This is a major clean up.

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

## Standard Library

**File: `lemma/src/stdlib/types.lemma`** (new)

Define all current hardcoded units as type definitions. Load automatically on engine initialization.

```lemma
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

type duration
unit second = 1
unit minute = second * 60
unit hour = minute * 60
unit day = hour * 24

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
