"""Benchmark the Python ports of the Lemma bench specs.

Timed loop on both sides: build inline input literals, evaluate terminal rule.
Lemma side (Criterion): Engine loaded once; timed = inputs + Engine::run → terminal.
Python side: module imported once; timed = build_inputs + compute_terminal.
compute_terminal evaluates only the terminal rule dependency closure.
Literal Fraction constants are module-level (import time).
Harness stdout is JSON for xtask ingestion only (not timed).
"""

import dataclasses
import gc
import importlib
import json
import statistics
import sys
import time
from fractions import Fraction
from pathlib import Path
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))

from business_rules.rational import rational_to_decimal_string

WARMUP_ITERATIONS = 100
LATENCY_ITERATIONS = 10_000


def shipping_raw() -> dict[str, str]:
    return {
        "weight": "3",
        "destination": "domestic",
        "is_member": "false",
    }


def pricing_raw() -> dict[str, str]:
    return {
        "product_type": "premium",
        "quantity": "25",
        "unit_price": "100",
        "coupon_percent": "5",
        "loyalty_years": "2",
        "is_member": "true",
        "is_loyalty": "true",
        "is_tax_exempt": "false",
    }


def order_pipeline_raw() -> dict[str, str]:
    return {
        "customer_tier": "gold",
        "payment_method": "credit",
        "shipping_zone": "national",
        "quantity": "12",
        "unit_price": "85",
        "package_weight": "3.5",
        "delivery_distance": "180",
        "loyalty_points": "6500",
        "coupon_percent": "10",
        "is_fragile": "true",
        "is_express": "true",
        "is_hazardous": "false",
        "is_gift": "false",
        "is_first_time": "false",
    }


SPECS: list[tuple[str, str, Callable[[], dict[str, str]]]] = [
    ("bench_shipping", "business_rules.shipping", shipping_raw),
    ("bench_pricing", "business_rules.pricing", pricing_raw),
    ("bench_order_pipeline", "business_rules.order_pipeline", order_pipeline_raw),
]


def render_value(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, Fraction):
        return rational_to_decimal_string(value)
    if isinstance(value, str):
        return value
    raise TypeError(
        f"BUG: unhandled output field type {type(value).__name__}: {value!r}"
    )


def render_outputs(outputs: Any) -> dict[str, str]:
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


def bench_spec(
    spec_name: str, module_name: str, raw_builder: Callable[[], dict[str, str]]
) -> dict[str, Any]:
    module = importlib.import_module(module_name)

    def run_one() -> Any:
        inputs = module.build_inputs(raw_builder())
        return module.compute_terminal(inputs)

    for _ in range(WARMUP_ITERATIONS):
        run_one()

    samples: list[int] = [0] * LATENCY_ITERATIONS
    perf_counter_ns = time.perf_counter_ns
    gc.collect()
    gc.disable()
    try:
        for i in range(LATENCY_ITERATIONS):
            start = perf_counter_ns()
            run_one()
            samples[i] = perf_counter_ns() - start
    finally:
        gc.enable()

    median_ns = statistics.median(samples)
    std_dev_ns = statistics.pstdev(samples)

    inputs = module.build_inputs(raw_builder())
    rendered = render_outputs(module.compute(inputs))

    return {
        "spec_name": spec_name,
        "iterations_latency": LATENCY_ITERATIONS,
        "latency_median_ns": float(median_ns),
        "latency_std_dev_ns": float(std_dev_ns),
        "outputs": rendered,
    }


def main() -> None:
    fixtures = [
        bench_spec(spec_name, module_name, raw_builder)
        for spec_name, module_name, raw_builder in SPECS
    ]
    json.dump({"fixtures": fixtures}, sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
