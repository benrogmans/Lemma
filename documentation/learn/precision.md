---
nav_title: Numeric precision
nav_order: 70
---

# Numeric precision

Lemma stores and computes numbers as exact rationals. This chapter explains how that works, where decimal appears, and what Spec authors and API clients need to know.

## How Lemma handles numbers

Lemma uses three layers:

1. Compute (ℚ): magnitudes stored as exact rationals (arbitrary-precision `BigInt` numerator/denominator). Arithmetic, comparisons, and unit conversions run in ℚ. No per-step decimal rounding.
2. Commit (decimal): a single conversion to `rust_decimal` at boundaries: API/JSON output, schema checks, transcendental functions.
3. Output (decimal string): API responses send magnitudes as JSON strings (`"37"`, `"99.50"`), never JSON number literals.

```
spec → parse (decimal literal → ℚ) → plan → evaluate (ℚ) → commit → decimal string
```

## Limits

| Layer | Constraint |
|-------|------------|
| Literals, JSON input, API input | Magnitude ±79,228,162,514,264,337,593,543,950,335 (~7.92×10²⁸); at most 28 decimal digits (`rust_decimal`) |
| Internal compute (ℚ) | Arbitrary precision; bounded by available memory (all BigInt allocation is fallible) |

Intermediate values during evaluation may exceed the decimal range or use more precision than 28 digits. Only top-level Rule results committed for output must fit `rust_decimal`. Oversized or uncommittable final results Veto with `Calculated result exceeds decimal value limit`.

When an exact rational grows past what memory allows, evaluation Vetoes with `out of memory` instead of crashing the process. This is resource exhaustion, not a decimal commit failure.

JSON/API output never emits fraction strings (`"37/47"`) or scientific notation.

## Display vs API output

CLI and human-readable formatting may show `numer/denom` when a value cannot commit to decimal (for debugging or edge cases). JSON and API surfaces use decimal strings only; uncommittable top-level Rule results Veto instead of emitting a fraction.

## Exact paths

These stay in ℚ end-to-end (no mid-pipeline decimal):

- `+`, `-`, `*`, `/`, `%` on numbers, ratios, quantities
- Unit conversion (`as <unit>`): `magnitude × from_factor ÷ to_factor`
- Comparisons
- Integer powers; rational powers when the result is exact (e.g. `4 ^ 0.5` → `2`)

Long conversion chains telescope in ℚ. Example: `37 base` through fourteen prime units forward and back stays exactly `37`; stepwise decimal at fixed precision drifts to `36.999…`.

## Approximation paths

| Case | Behavior |
|------|----------|
| `^` on irrationals (e.g. `2 ^ 0.5`) | ~28-significant-digit Decimal fallback when no exact root |
| `sqrt`, `sin`, `cos`, `log`, … | ~28-significant-digit Decimal; inputs must commit to decimal |
| `floor`, `ceil`, `round` on measures | Operate at Decimal precision |
| ℚ allocation failure | Veto (`out of memory`); no decimal fallback |
| Division by zero | Literal zero divisor in a Rule (e.g. `1 / 0`) → planning Error. Zero from runtime Data → Veto (never approximated) |

## Boundary number contract

All API surfaces enforce a uniform rule for numeric data inputs:

| Input type | Accepted? | Rationale |
|-----------|-----------|-----------|
| Integer (native number) | Yes | Exact on all surfaces |
| Decimal / float (native number) | **Rejected** | IEEE 754 f64 cannot represent most decimals exactly |
| String `"0.1"`, `"99.50"` | Yes | Parsed as exact decimal → ℚ |

This applies identically to the WASM/JavaScript API, the HTTP/JSON API, and the Elixir NIF. Non-integer numeric values are rejected with `"decimal values must be passed as strings to preserve exactness"`. Rust callers using `Engine::run` directly are unaffected (they already provide string magnitudes).

## Clients (JavaScript / Python)

**Sending data:** pass decimal values as strings, integers may be native numbers:

```javascript
engine.run(null, "pricing", null, { quantity: 42, rate: "0.075" });
```

```python
engine.run("pricing", data={"quantity": 42, "rate": "0.075"})
```

**Reading results:** parse numeric fields as decimal strings, not floats:

JavaScript:

```javascript
import Decimal from "decimal.js";
const n = new Decimal(json.results.price.result.value.value.number);
```

Python:

```python
from decimal import Decimal
n = Decimal(response["results"]["price"]["result"]["value"]["value"]["number"])
```

Never `Number()` / `parseFloat()` / `float()`. Precision breaks above ~9×10¹⁵.

## Spec authors

- Money, units, ratios: exact by default; define unit factors as clean literals.
- `decimals n`: constrains decimal scale when validating Data inputs, defaults, and value copies, not internal ℚ compute or intermediate Rule steps.
- Keep deliverable top-level Rule results within the 28-digit decimal limit; oversized finals Veto.

## Related

- [Reference: primitive types](../reference/readme.md#primitive-types)
- [Veto](types_and_units.md#veto): when results exceed limits or division by zero occurs at runtime

You have completed the Learn guide. For exhaustive syntax and operators, see the [Language reference](../reference/readme.md). To embed Lemma, see [Tools & SDKs](../tools/readme.md).
