---
layout: default
title: Veto Semantics
---

# Veto semantics

## Purpose

Use `veto` for **data validation** - when input data is invalid or out of acceptable range.

```lemma
rule validated_age: age
  unless age < 0    then veto "Age cannot be negative"
  unless age > 120  then veto "Invalid age value"
```

**Important**: Use veto for invalid data, not for negative business results. Use boolean values for business logic.

## When veto applies

If a rule references a vetoed rule and needs its value, the veto applies to the dependent rule too.

### Veto applies to dependent rule

```lemma
rule validated_price: price
  unless price < 0 then veto "Price cannot be negative"

rule total: validated_price * quantity
```

If `validated_price` is vetoed, `total` is also vetoed because we need the price value.

### Veto does not apply to dependent rule

```lemma
rule validated_weight: weight
  unless weight < 0 then veto "Weight cannot be negative"

rule shipping_weight: validated_weight
  unless use_estimated then 5
```

If `validated_weight` is vetoed but `use_estimated` is true, then `shipping_weight` = 5. The veto doesn't apply because `validated_weight` is never evaluated (the unless clause provides the value).

## Veto vs Error vs Panic

Lemma distinguishes three outcomes:

| Outcome | When | Example |
|---------|------|---------|
| **Planning Error** | Invalid spec (wrong types, unsupported operations) | `5 and "text"` — logical AND requires boolean operands |
| **Veto** | Domain "no value" at runtime | Division by zero, missing fact, user `veto "..."`, date overflow |
| **Panic** | Bug (invariant violated; should never happen after planning) | Internal consistency failure |

**Veto is only for domain-level "no value"**, not for type errors or invalid operations. Those are caught at planning time. If the engine reaches code that would have returned a type-error Veto, it panics instead — planning should have rejected the spec.