"""Benchmark the Python ports of the Lemma bench specs.

Per-call measured boundary: JSON input bytes -> Outputs in memory.

Emits one JSON document to stdout:

  {
    "fixtures": [
      {
        "spec_name": "bench_shipping",
        "iterations_latency": 100000,
        "latency_median_ns": <f>,
        "latency_std_dev_ns": <f>,
        "outputs": { "<rule_name>": "<string>" }
      },
      ...
    ]
  }
"""

import dataclasses
import gc
import json
import statistics
import sys
import time
from decimal import Decimal
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from business_rules import order_pipeline, pricing, shipping

SPECS_DIR = Path(__file__).resolve().parent.parent / "specs"

WARMUP_ITERATIONS = 1_000
LATENCY_ITERATIONS = 100_000


SPECS = [
    ("bench_shipping", "shipping.inputs.json", shipping),
    ("bench_pricing", "pricing.inputs.json", pricing),
    ("bench_order_pipeline", "order_pipeline.inputs.json", order_pipeline),
]


def render_value(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, Decimal):
        return str(value)
    if isinstance(value, str):
        return value
    raise TypeError(
        f"BUG: unhandled output field type {type(value).__name__}: {value!r}"
    )


def render_outputs(outputs: Any) -> dict[str, str]:
    """Render Outputs fields to a flat ``{rule_name: string_value}`` dict.

    Fields named ``<rule>_veto`` are not emitted directly. When the
    companion ``<rule>`` field exists and ``<rule>_veto`` is non-None,
    the rule is reported with the veto reason as its value (mirroring
    Lemma's `OperationResult::Veto` which replaces the boolean/numeric
    result entirely). When ``<rule>_veto`` is None the rule renders
    normally.
    """
    field_names = {field.name for field in dataclasses.fields(outputs)}
    rendered: dict[str, str] = {}
    for field in dataclasses.fields(outputs):
        name = field.name
        if name.endswith("_veto") and name[: -len("_veto")] in field_names:
            continue
        value = getattr(outputs, name)
        veto_field = f"{name}_veto"
        if veto_field in field_names:
            veto_reason = getattr(outputs, veto_field)
            if veto_reason is not None:
                rendered[name] = veto_reason
                continue
        rendered[name] = render_value(value)
    return rendered


def bench_spec(spec_name: str, inputs_file: str, module: Any) -> dict[str, Any]:
    raw_bytes = (SPECS_DIR / inputs_file).read_bytes()
    build_inputs = module.build_inputs
    compute = module.compute
    loads = json.loads

    for _ in range(WARMUP_ITERATIONS):
        compute(build_inputs(loads(raw_bytes)))

    samples: list[int] = [0] * LATENCY_ITERATIONS
    perf_counter_ns = time.perf_counter_ns
    gc.collect()
    gc.disable()
    try:
        for i in range(LATENCY_ITERATIONS):
            start = perf_counter_ns()
            compute(build_inputs(loads(raw_bytes)))
            samples[i] = perf_counter_ns() - start
    finally:
        gc.enable()

    median_ns = statistics.median(samples)
    std_dev_ns = statistics.pstdev(samples)

    outputs = compute(build_inputs(loads(raw_bytes)))
    rendered = render_outputs(outputs)

    return {
        "spec_name": spec_name,
        "iterations_latency": LATENCY_ITERATIONS,
        "latency_median_ns": float(median_ns),
        "latency_std_dev_ns": float(std_dev_ns),
        "outputs": rendered,
    }


def main() -> None:
    fixtures = [bench_spec(spec, inputs, module) for spec, inputs, module in SPECS]
    json.dump({"fixtures": fixtures}, sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
