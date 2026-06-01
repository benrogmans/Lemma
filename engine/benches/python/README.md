# Python ports of the Lemma benchmark specs

Hand-written Python implementations of the three Lemma specs in
[`../specs/`](../specs). Used as the Python side of the Lemma vs
Python comparison in [`../RESULTS.md`](../RESULTS.md).

## Layout

```
business_rules/
  __init__.py
  shipping.py        # mirrors ../specs/shipping.lemma
  pricing.py         # mirrors ../specs/pricing.lemma
  order_pipeline.py  # mirrors ../specs/order_pipeline.lemma
benchmark.py
pyproject.toml
```

Each `business_rules/<spec>.py` module exposes:

- `Inputs`: a `@dataclass(frozen=True, slots=True)` with one field per
  Lemma `data` declaration.
- `Outputs`: a `@dataclass(frozen=True, slots=True)` with one field per
  Lemma `rule`.
- `build_inputs(raw: dict[str, str]) -> Inputs`: converts the JSON string
  values to `Decimal`/`bool`/`str`. Raises on missing or malformed input.
- `compute(inputs: Inputs) -> Outputs`: the rule pipeline. Pure function,
  no I/O.

`decimal.Decimal` is used for every money or ratio value. The default
context (`getcontext().prec = 28`, `ROUND_HALF_EVEN`) is left untouched.

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
cargo run -p xtask -- bench-report
```

From the workspace root. This runs the `evaluate` and `outputs` Rust
benches and `python3 engine/benches/python/benchmark.py`, joins
everything, and rewrites `engine/benches/RESULTS.md` with latency and
numerical-accuracy tables side by side.

## Per-call boundary

The Python timed loop measures `compute(build_inputs(json.loads(raw_bytes)))`:
JSON input bytes in, `Outputs` dataclass in memory out. The Rust side's
`Engine::run_plan` does `serde_json::from_slice` + plan clone + defaults
+ data-value typed conversion + evaluation in the same timed call. Both
sides pay for JSON parsing.
