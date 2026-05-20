---
layout: default
title: Numeric precision
---

# Numeric precision

## How Lemma handles numbers

Lemma uses three layers:

1. **Compute (ℚ)** — magnitudes stored as exact rationals (`i128` numerator/denominator). Arithmetic, comparisons, and unit conversions run in ℚ. No per-step decimal rounding.
2. **Commit (decimal)** — a single conversion to `rust_decimal` at boundaries: API/JSON output, schema checks, transcendental functions.
3. **Output (decimal string)** — API responses send magnitudes as JSON **strings** (`"37"`, `"99.50"`), never JSON number literals.

```
spec → parse (decimal literal → ℚ) → plan → evaluate (ℚ) → commit → decimal string
```

## Limits

| Layer | Constraint |
|-------|------------|
| **Literals, JSON input, API input** | Magnitude ±79,228,162,514,264,337,593,543,950,335 (~7.92×10²⁸); at most **28 decimal digits** (`rust_decimal`) |
| **Internal compute (ℚ)** | ~±1.7×10³⁸ (`i128` numerator/denominator) |

Intermediate values during evaluation may exceed the decimal range or use more precision than 28 digits. Only **top-level rule results** committed for output must fit `rust_decimal`. Oversized or uncommittable final results **Veto** with `Calculated result exceeds decimal value limit`.

JSON/API output never emits fraction strings (`"37/47"`) or scientific notation.

## Display vs API output

CLI and human-readable formatting may show `numer/denom` when a value cannot commit to decimal (for debugging or edge cases). JSON and API surfaces use decimal strings only; uncommittable **top-level** rule results Veto instead of emitting a fraction.

## Exact paths

These stay in ℚ end-to-end (no mid-pipeline decimal):

- `+`, `-`, `*`, `/`, `%` on numbers, ratios, quantities
- Unit conversion (`as <unit>`): `magnitude × from_factor ÷ to_factor`
- Comparisons
- Integer powers; rational powers when the result is exact (e.g. `4 ^ 0.5` → `2`)

Long conversion chains telescope in ℚ. Example: `37 base` through fourteen prime units forward and back stays **exactly** `37`; stepwise decimal at fixed precision drifts to `36.999…`.

## Approximation paths

| Case | Behavior |
|------|----------|
| **`^` on irrationals** (e.g. `2 ^ 0.5`) | Decimal fallback when no exact root |
| **`sqrt`, `sin`, `cos`, `log`, …** | Always decimal; inputs must commit to decimal |
| **i128 overflow in arithmetic** | Decimal fallback on operands, then lift back |
| **Division by zero** | Literal zero divisor in a rule (e.g. `1 / 0`) → **planning Error**. Zero from runtime data → **Veto** (never approximated) |

## Clients (JavaScript / Python)

Parse numeric fields as decimal strings — not floats:

```javascript
import Decimal from "decimal.js";
const n = new Decimal(json.results.price.result.value.value.number);
```

```python
from decimal import Decimal
n = Decimal(response["results"]["price"]["result"]["value"]["value"]["number"])
```

Never `Number()` / `parseFloat()` / `float()` — precision breaks above ~9×10¹⁵.

## Spec authors

- Money, units, ratios: exact by default; define unit factors as clean literals.
- `decimals n`: constrains decimal scale when validating **data inputs**, defaults, and value copies — not internal ℚ compute or intermediate rule steps.
- Keep **deliverable top-level rule results** within the 28-digit decimal limit; oversized finals Veto.

## Related

- [Reference — primitive types](reference.md#primitive-types)
- [Veto semantics](veto_semantics.md)
