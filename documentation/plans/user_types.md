---
layout: default
title: User Types
---

# User Types Implementation Plan

## Overview

Extend Lemma to support user-defined types within documents. Types define units with optional numeric values, enabling custom enumerations, priorities, statuses, and domain-specific measurements. Types are scoped to their document and accessed via existing doc reference syntax.

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
```

Loaded at engine initialization. Available everywhere without qualification.

### User Types (Doc-Scoped)

```lemma
doc order_workflow

type order_status
unit draft
unit pending
unit approved

type priority
unit low = 1
unit medium = 2
unit high = 3

fact status = [order_status]
fact urgency = [priority]

rule can_process = status is approved
rule needs_escalation = urgency > 2
```

Types defined in a doc are local to that doc. Units are unqualified within the doc.

### Cross-Doc Access

```lemma
doc shipment
fact order = doc order_workflow
fact order.status = approved

rule can_ship = order.status is order_workflow.approved
```

Access units from other docs using doc reference syntax: `doc_name.unit_name`

### Type Annotations

```lemma
doc order_workflow

type order_status
unit draft
unit approved

fact status = [order_status]

doc shipment
fact weight = [mass]
fact order_status = [order_workflow.order_status]
```

Type annotations use the type name in brackets. Cross-doc types use `doc_name.type_name` syntax.

## Grammar Changes

**File: `lemma/src/parser/lemma.pest`**

```pest
document = { SOI ~ doc ~ doc_name ~ commentary? ~ (type_def | fact | rule)* ~ EOI }
doc = { "doc" }

type_def = { "type" ~ identifier ~ unit+ }
unit = { "unit" ~ identifier ~ ("=" ~ expression)? }
```

Types are defined within docs alongside facts and rules.

## Semantic Model

**File: `lemma/src/semantic.rs`**

```rust
pub struct LemmaUnit {
    pub name: String,
    pub expression: Option<Expression>,  // None for value-less units
}

pub struct LemmaType {
    pub name: String,
    pub units: Vec<LemmaUnit>,
}

pub struct LemmaDoc {
    pub name: String,
    pub source: Option<String>,
    pub start_line: usize,
    pub commentary: Option<String>,
    pub types: Vec<LemmaType>,      // NEW
    pub facts: Vec<LemmaFact>,
    pub rules: Vec<LemmaRule>,
}

pub enum LiteralValue {
    Number(Decimal),
    CustomTyped {
        value: Decimal,
        type_name: String,
        unit_name: String,
    },
    // ... existing variants
}
```

## Unit Resolution

**Within a doc:**
1. Parse `approved` as identifier
2. Search local types for unit `approved`
3. If found, resolve to `CustomTyped { type_name: "order_status", unit_name: "approved", value: <optional> }`
4. If not found, search global types (stdlib)
5. If not found, error: "Unknown unit 'approved'"

**Cross-doc access:**
1. Parse `order_workflow.approved` as qualified reference
2. Look up doc `order_workflow`
3. Find unit `approved` in that doc's types
4. Resolve type and unit

## Validation

**Type Definition Validation:**

For numeric types with conversions:
- Identify base unit (one unit with static numeric value)
- Validate all other units reference the base unit (directly or indirectly)
- Validate no circular references

For ordered enums (units with numeric values but no references):
- Allow any numeric assignments
- Enable comparisons (`>`, `<`, `>=`, `<=`)
- No conversion support

For value-less enums:
- Allow equality checks only (`is`, `is not`)
- No comparisons or conversions

**Global Uniqueness (Stdlib Only):**

Stdlib types have globally unique unit names. User types are scoped to docs, so no global conflicts.

## Transpilation Strategy

### Base + Derived Units (Conversions)

For types with a base unit and conversions:

```lemma
type temperature
unit celsius = 1
unit fahrenheit = celsius * 9/5 + 32
```

Transpile to Prolog module:

```prolog
:- module(temperature, [to_base_celsius/2, to_base_fahrenheit/2,
                        from_base_celsius/2, from_base_fahrenheit/2,
                        convert_temperature/4]).

% Base unit (identity)
to_base_celsius(Value, Value).
from_base_celsius(Value, Value).

% Derived unit (algebraic inversion)
to_base_fahrenheit(Value, Result) :- Result is (Value - 32) * 5/9.
from_base_fahrenheit(Value, Result) :- Result is Value * 9/5 + 32.

% Conversion predicate
convert_temperature(Value, FromUnit, ToUnit, Result) :-
    call(to_base_ + FromUnit, Value, BaseValue),
    call(from_base_ + ToUnit, BaseValue, Result).
```

### Ordered Enums (No Conversions)

```lemma
type priority
unit low = 1
unit high = 3
```

Transpile to simple value mappings:

```prolog
priority_value(low, 1).
priority_value(high, 3).
```

Used for comparisons only.

### Value-less Enums

```lemma
type status
unit draft
unit approved
```

Transpile to atoms:

```prolog
status_unit(draft).
status_unit(approved).
```

Used for equality checks only.

## Expression Inversion

**File: `lemma/src/evaluator/expression_inverter.rs`** (new)

Use general symbolic inversion. If the base unit appears **exactly once** in the expression, invert by unwinding operations:

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

Remove `units.pl` conversion predicates entirely.

## Implementation Phases

### Phase 1: Parser (1 day)
- Add `type` and `unit` to grammar
- Parse types within doc declarations
- Parse unit definitions with optional expressions

### Phase 2: Semantic Model (1 day)
- Add `LemmaType` and `LemmaUnit` structs
- Add `types` field to `LemmaDoc`
- Add `CustomTyped` literal variant
- Unit resolution within doc scope
- Cross-doc unit resolution

### Phase 3: Validation (1 day)
- Validate type definitions (base unit, references, no cycles)
- Classify types: convertible, ordered enum, value-less enum
- Type compatibility checking
- Global uniqueness for stdlib

### Phase 4: Transpilation (3 days)
- Expression inverter for algebraic patterns
- Type module transpilation (to_base/from_base predicates)
- Ordered enum transpilation (value mappings)
- Value-less enum transpilation (atoms)
- Update conversion expression handling

### Phase 5: Standard Library (1 day)
- Create `stdlib/types.lemma` with all units
- Auto-load in `Engine::new()`
- Remove `units.pl` conversion logic

### Phase 6: Testing (2 days)
- Parser tests (type and unit parsing)
- Validation tests (base unit, references, cycles)
- Transpilation tests (inversion patterns)
- Integration tests (conversions, enums, cross-doc access)
- Stdlib loading tests

### Phase 7: Examples & Documentation (1 day)
- Update language docs with type syntax
- Add example: custom type with conversions
- Add example: enum types for business logic
- Update reference with supported patterns

## Timeline

**Total: 10 days**

## Key Design Points

1. **Doc-scoped types** - Types belong to docs, accessed via doc references
2. **Declarative equations** - Unit definitions are relationships, not functions
3. **Algebraic inversion** - Automatic bidirectional conversions
4. **Three type modes** - Convertible, ordered enum, value-less enum
5. **Stdlib as types** - Replace hardcoded conversions with Lemma definitions
6. **Unqualified within doc** - Clean syntax for domain experts
7. **No transition enforcement** - Business rules stay in doc logic
