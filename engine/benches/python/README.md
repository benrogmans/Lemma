# Python ports of the Lemma benchmark specs

Hand-written Python implementations of the three Lemma specs in
[`../specs/`](../specs). Used as the Python side of the hand-written Lemma vs
hand-written Python comparison in
[`../../../documentation/reference/benchmarks/engine.md`](../../../documentation/reference/benchmarks/engine.md).

## Per-call boundary (latency)

Cross-language latency compares steady-state evaluation only (C-vs-Python model):

- **Lemma**: compile once (`load` before measurement); timed = inline input literals + `run_plan` → terminal rule
- **Python**: import once before measurement; timed = inline input literals + `build_inputs(raw)` + `compute_terminal(inputs)`

Lemma compile cost is reported separately, not in the Python/Lemma ratio. No disk I/O. No JSON input sidecars.

## Running

```sh
python3 benchmark.py          # standalone JSON to stdout
cargo benchmarks engine     # full report from workspace root
```
