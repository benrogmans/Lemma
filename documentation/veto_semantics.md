---
layout: default
title: Veto Semantics
---

# Veto semantics

## Purpose

Use `veto` when a rule cannot produce a meaningful value — the domain says "no answer here."

Common cases:
- **Data validation** — input is out of acceptable range.
- **Missing or unrecognized input** — a required data field has no match (e.g. unknown enum value).
- **Constraint violations at runtime** — a data override breaks a bound or type constraint.

```lemma
rule validated_age: age
  unless age < 0   then veto "Age cannot be negative"
  unless age > 120 then veto "Invalid age value"
```

**Important**: Use veto for "no value" situations, not for negative business results. A rule that evaluates to `false` or `0` is still a valid result — use boolean or numeric values for business logic.

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

### Missing data propagates as veto

When a data field has no value (not provided and no default), rules that depend on it veto with a "Missing data" reason. This propagates through rule references:

```lemma
data product: text
  -> option "latte"

rule base_price: veto "Unknown product"
  unless product is "latte" then 3.50 eur

rule total: base_price * 2
```

If `product` is not provided, `base_price` vetoes with "Missing data: product" — not the default arm's "Unknown product" message. The `total` rule also vetoes because its dependency has no value. A dependent rule never produces a numeric result when an upstream dependency lacks data.

## `is veto` (boolean test, not a new failure mode)

Test whether an expression produced **no value** (`Veto`) and branch on a **boolean** — without propagating the operand’s veto through the test:

```lemma
rule validated_price: price
  unless price < 0 then veto "Price cannot be negative"

rule total: validated_price * quantity
  unless validated_price is veto then 0
```

When `validated_price` vetoes, `validated_price is veto` is **true** and `total` can take the fallback `0`. The test never returns `Veto`; only the rule’s final arm can.

Equivalent forms: `veto is validated_price`, `validated_price is not veto`, `not veto is validated_price`.

For a **rule reference** (`validated_price is veto`), the test uses that rule’s stored `OperationResult` after topological evaluation — not a re-run of the rule’s inlined unless body. For a **compound** operand (`price * qty is veto`), the subexpression is evaluated and the test is true if that evaluation is `Veto`. To test one failing operand inside a sum or product, use `b is veto`, not `a + b is veto` when only `b` failed.

Re-veto: `unless x is veto then veto "outer"` — rule result message is **`"outer"`** only. Explanations may still show the inner veto under the operand of `is veto`.

`veto "message"` is only valid as a **rule or unless result**, not in `is veto` comparisons.

## Veto vs Error vs Panic

Lemma distinguishes three outcomes:

| Outcome | When | Example |
|---------|------|---------|
| **Planning Error** | Invalid spec (wrong types, unsupported operations) | `5 and "text"` — logical AND requires boolean operands; `1 / 0` — literal division by zero |
| **Veto** | Domain "no value" at runtime | Division by zero from data, missing data, invalid data override (constraint violation, unparsable value, value not in options), user `veto "..."`, date overflow |
| **Panic** | Bug (invariant violated; should never happen after planning) | Internal consistency failure |

After planning succeeds, evaluation is guaranteed to complete — it produces rule results (values or vetoes) and never returns an error. Data overrides that violate type constraints, minimum/maximum bounds, or allowed options produce a **Veto** on the affected rules, not a planning error. Planning errors only arise from structurally invalid specs.

**Veto is only for domain-level "no value"**, not for type errors or invalid operations in the spec itself. Those are caught at planning time. If the engine reaches code that would have returned a type-error Veto, it panics instead — planning should have rejected the spec.