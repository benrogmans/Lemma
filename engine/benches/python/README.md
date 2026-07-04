# Python ports of the Lemma benchmark specs

Hand-written Python implementations of the three Lemma specs in
[`../specs/`](../specs). Used as the Python side of the Lemma vs
Python comparison in [`../../../documentation/reference/benchmarks/engine.md`](../../../documentation/reference/benchmarks/engine.md).

## Layout

```
business_rules/
  __init__.py
  rational.py      # parse_rational, rational_to_decimal_string (Fraction arithmetic)
  shipping.py      # mirrors ../specs/shipping.lemma
  pricing.py       # mirrors ../specs/pricing.lemma
  order_pipeline.py  # mirrors ../specs/order_pipeline.lemma
benchmark.py
pyproject.toml
```

Each `business_rules/<spec>.py` module exposes:

- `Inputs`: a `@dataclass(frozen=True, slots=True)` with one field per
  Lemma `data` declaration.
- `Outputs`: a `@dataclass(frozen=True, slots=True)` with one field per
  Lemma `rule`.
- `build_inputs(raw: dict[str, str]) -> Inputs`: lifts fixture string
  values to `Fraction`/`bool`/`str`. Raises on missing or malformed input.
- `compute(inputs: Inputs) -> Outputs`: the rule pipeline. Pure function,
  no I/O.

Numeric values use stdlib `fractions.Fraction` for exact rational
arithmetic, aligned with Lemma's internal arbitrary-precision rational
model. Python uses unbounded integers, matching Lemma's unbounded
magnitudes.
Decimal strings are produced only at the output boundary via
`rational_to_decimal_string`.

## Running standalone

```sh
python3 benchmark.py
```

From this directory. Requires Python 3.11+ (no external dependencies).
The script reads the same `../specs/*.inputs.json` files the Rust bench
uses, runs the warmup + latency loop and a single output-capture per
spec, and writes one JSON document to stdout.

## Running as part of the full bench report

```sh
cargo benchmarks engine
```

From the workspace root. This runs the `evaluate` and `outputs` Rust
benches and `python3 engine/benches/python/benchmark.py`, joins
everything, and rewrites `documentation/reference/benchmarks/engine.md` with latency and
numerical-accuracy tables side by side.

## Per-call boundary

Fixture JSON is loaded once per spec before warmup. The timed loop measures
`compute(build_inputs(raw_dict))` where `raw_dict` is the pre-parsed
`dict[str, str]`. The Rust side clones a pre-built
`HashMap<String, DataValueInput>` and calls `Engine::run_plan`. JSON parsing
at API boundaries (CLI, WASM) is out of scope.
