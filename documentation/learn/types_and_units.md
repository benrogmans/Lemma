---
nav_title: Types and units
nav_order: 40
---

# Types and units

Lemma has a rich type system built from primitives, quantities with units, ratios, and ranges. This chapter covers literals, operators, the standard library, conversions, ranges, dates, and Veto (the domain outcome when a Rule cannot produce a value).

## Literal types

| Type | Example | Notes |
|------|---------|-------|
| Number | `42`, `3.14`, `1.23e10` | Exact rational arithmetic |
| Text | `"hello"` | String literals |
| Boolean | `true`, `false`, `yes`, `no`, `accept`, `reject` | Aliases |
| Date | `2024-01-15`, `2024-01-15T14:30:00Z` | ISO 8601 |
| Time | `14:30:00` | Time of day |
| Measure | `100 eur`, `40 hours`, `12 kilogram` | Unit must be declared by a measure type in scope |
| Ratio | `15 percent`, `15%`, `5 permille`, `5%%` | Proportional values |
| Range | `0...100`, `2024-01-01...2024-06-15`, `18 year...67 year` | Half-open intervals |

See [Primitive types in the language reference](../reference/readme.md#primitive-types).

## Arithmetic

```lemma
spec arithmetic_examples

data price:     100
data tax:      21
data quantity:      3
data principal: 1_000
data rate:      0.05
data years:     10

rule total: (price + tax) * quantity

rule compound: principal * (1 + rate) ^ years
```

Operators: `+`, `-`, `*`, `/`, `%`, `^`

## Comparison

```lemma
spec comparison_examples

data status: text
data age:    number
data income: number

rule status_ok: status is "approved"

rule not_cancelled: status is not "cancelled"

rule is_eligible: age >= 18 and income > 30_000
```

Operators: `>`, `<`, `>=`, `<=`, `is`, `is not`, `is veto`, `is not veto`

## Logical

```lemma
spec loan_approval

data credit_score:    number
data income_verified: boolean
data has_bankruptcy:  boolean

rule can_approve_loan:
  credit_score >= 650 and income_verified and not has_bankruptcy
```

Operators: `and`, `not` (there is no `or`: Unless chains accommodate such logic)

## Mathematical

```lemma
spec math_examples

data a:     3
data b:     4
data angle: 0.5

rule hypotenuse: sqrt (a ^ 2 + b ^ 2)

rule sine_value: sin angle

rule log_value: log 10
```

Prefix operators (parentheses optional): `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`

## Standard library: `uses lemma units`

Lemma embeds SI units in the standard library (Repo `lemma`, Spec `units`). Import with `uses lemma units`, then use units directly in literals or reference types as `units.mass`, `units.duration`, `units.length`, `units.calendar`, and others:

```lemma
spec logistics

uses lemma units

data package_weight: 12 kilogram
data shift_length:   8 hours
data route_distance: 45 kilometer

rule weight_grams:  package_weight as gram
rule is_heavy:      package_weight > 20 kilogram
rule is_long_shift: shift_length >= 8 hours
```

Duration units (`hours`, `days`, `weeks`, ...) come from `units.duration`; calendar periods (`year`, `month`) from `units.calendar`. Prefer the stdlib types over redefining kilogram or hour in every Spec.

## Unit conversions

Convert within a unit family with `as`:

```lemma
spec conversion_examples

data money: measure
  -> unit eur 1.00
  -> unit usd 0.91

data price: 100 eur

rule price_usd: price as usd

rule as_percent: 0.25 as percent
```

Durations convert the same way:

```lemma
spec schedule

uses lemma units

data workweek: 40 hours

rule workweek_days: workweek as days
```

Strip to a bare number with a chained cast: `amount as eur as number`. See [Type cast in the language reference](../reference/readme.md#type-cast-as).

## Ranges

Intervals use `lo...hi` (lower inclusive, upper exclusive). Test membership with `in`; project width with `(lo...hi) as <unit>`. Range slots use `number range`, `date range`, `time range`, `measure range`, `ratio range`, or a named `<type> range`:

```lemma
spec eligibility

uses lemma units

data age:   25 year
data score: 50

rule in_working_age: age in 18 year...67 year

rule in_band: score in 0...100

rule days_in_q2: (2024-04-01...2024-07-01) as days
```

See [Ranges in the language reference](../reference/readme.md#ranges).

## Date and time

Dates compare directly; spans between dates are ranges projected to a unit; durations add to dates:

```lemma
spec deadlines

uses lemma units

data today:    2024-09-30
data deadline: 2024-12-31

rule days_until_deadline: (today...deadline) as days

rule is_overdue: today > deadline

rule follow_up_date: deadline + 14 days
```

### Calendar vs duration arithmetic

Calendar units (`year`, `month`) use calendar-aware arithmetic; duration units (`days`, `hours`, `seconds`, ...) use fixed-length arithmetic. Adding one month to March 1 gives April 1 regardless of month length, while adding 30 days always adds exactly 30 × 86400 seconds.

**Month-end clamping**: adding months or years clamps to the last valid day of the target month. January 31 + 1 month → February 28 (or 29 in a leap year). March 31 − 1 month → February 28/29.

## Veto

Use Veto when a Rule cannot produce a meaningful value: the domain says "no answer here." Use Veto for impossible situations, not for negative business results. A Rule that evaluates to `false` or `0` is still a valid result.

Common cases:

- Data validation: input is out of acceptable range.
- Missing or unrecognized input: a required Data field has no match.
- Constraint violations at runtime: a Data override breaks a bound or type constraint.

```lemma
spec age_validation

data age: number

rule validated_age: age
  unless age < 0   then veto "Age must be a positive number"
  unless age > 120 then veto "Invalid age value"
```

A vetoed Rule produces no result. If a Rule references a vetoed Rule and needs its value, the Veto propagates. If an Unless clause provides an alternative, the Veto does not propagate:

```lemma
spec scoring

data score:       number
data use_default: false

rule validated_score: score
  unless score < 0 then veto "Invalid score"

rule result: validated_score
  unless use_default then 50
```

If `validated_score` is vetoed but `use_default` is true, `result` = 50.

### Veto applies to dependent Rules

```lemma
spec order_total

data price: number
data quantity:   number

rule validated_price: price
  unless price < 0 then veto "Price cannot be negative"

rule total: validated_price * quantity
```

If `validated_price` is vetoed, `total` is also vetoed because we need the price value.

### Veto does not apply when Unless provides a fallback

```lemma
spec shipping_estimate

data weight:        number
data use_estimated: boolean

rule validated_weight: weight
  unless weight < 0 then veto "Weight cannot be negative"

rule shipping_weight: validated_weight
  unless use_estimated then 5
```

If `validated_weight` is vetoed but `use_estimated` is true, then `shipping_weight` = 5. The Veto does not apply because `validated_weight` is never evaluated (the Unless clause provides the value).

### Missing Data propagates as Veto

When a Data field has no value (not provided and no default), Rules that depend on it Veto with a "Missing data" reason:

```lemma
spec coffee_prices

data money: measure
  -> unit eur 1.00
  -> decimals 2

data product: text
  -> option "latte"

rule base_price: veto "Unknown product"
  unless product is "latte" then 3.50 eur

rule total: base_price * 2
```

If `product` is not provided, `base_price` vetoes with "Missing data: product", not the default arm's "Unknown product" message. The `total` Rule also vetoes because its dependency has no value.

### `is veto` (boolean test)

Test whether an expression produced no value and branch on a boolean, without propagating the operand's Veto through the test:

```lemma
spec fallback_total

data price: number
data quantity:   number

rule validated_price: price
  unless price < 0 then veto "Price cannot be negative"

rule total: validated_price * quantity
  unless validated_price is veto then 0
```

When `validated_price` vetoes, `validated_price is veto` is true and `total` can take the fallback `0`. The test never returns Veto; only the Rule's final arm can.

Equivalent forms: `veto is validated_price`, `validated_price is not veto`, `not veto is validated_price`.

When the operand is a Rule reference (`validated_price is veto`), the test reads that Rule's already-computed result. When the operand is a compound expression (`price * quantity is veto`), that expression is evaluated and the test is true when the result is a Veto. To test a single failing operand inside a sum or product, apply `is veto` to that operand (`b is veto`) rather than to the whole expression (`a + b is veto`).

You can Veto again based on a test: `unless x is veto then veto "outer"`. The Rule's result message is then `"outer"`; explanations may still show the inner Veto beneath the `is veto` operand.

`veto "message"` is only valid as a Rule or Unless result, not in `is veto` comparisons. Here `veto` is a keyword meaning "no value", different from `veto "message"`, which produces a Veto. See [Special expressions in the language reference](../reference/readme.md#special-expressions).

Test for a Veto without propagating it:

```lemma
spec veto_checks

data score: number

rule validated_score: score
  unless score < 0 then veto "Invalid score"

rule has_valid_score: validated_score is not veto
```

### Veto vs Error vs Panic

Lemma distinguishes three outcomes:

| Outcome | When | Example |
|---------|------|---------|
| Planning Error | Invalid Spec (wrong types, unsupported operations) | `5 and "text"` (logical AND requires boolean operands); `1 / 0` (literal division by zero) |
| Veto | Domain "no value" at runtime | Division by zero from Data, missing Data, invalid Data override, user `veto "..."`, date overflow |
| Panic | Bug (invariant violated; should never happen after planning) | Internal consistency failure |

After planning succeeds, evaluation is guaranteed to complete: it produces Rule results (values or Vetoes) and never returns an error. Data overrides that violate type constraints, minimum/maximum bounds, or allowed options produce a Veto on the affected Rules, not a planning error.

Veto is only for domain-level "no value", not for type errors or invalid operations in the Spec itself. Those are caught at planning time.

## Next up

[Extending Data](extending_data.md): parent types, Data commands, and reuse across Specs.
