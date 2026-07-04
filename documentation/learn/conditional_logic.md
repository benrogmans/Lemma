---
nav_title: Conditional logic
nav_order: 30
---

# Conditional logic

Lemma expresses business rules the way documents are written: a default applies, then exceptions override it. The primary mechanism is the Unless clause.

## Unless chains

Conditional logic where the last matching condition wins:

```lemma
spec order_discount

data quantity: number
data is_vip:   false

rule discount: 0%
  unless quantity >= 10  then 10%
  unless quantity >= 50  then 20%
  unless is_vip          then 25%
```

If a VIP customer orders 75 items, they get 25% (last matching wins), not 20%.

Best practice: place Veto clauses last so they override all other logic.

There is no `or` operator. Unless chains accommodate disjunctive logic. For boolean composition within a single condition, use `and` and `not`.

## Boolean literals

Multiple aliases for readability:

| Value | Aliases |
|-------|---------|
| **True** | `true`, `yes`, `accept` |
| **False** | `false`, `no`, `reject` |

All aliases in each row are interchangeable.

## Next up

[Types and units](types_and_units.md): literals, operators, quantities, conversions, ranges, dates, and Veto.
