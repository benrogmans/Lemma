use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};
use std::alloc::System;
use std::fmt::Write;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

mod common;

fn latency_target_rule(spec_name: &str) -> &'static str {
    match spec_name {
        "bench_shipping" | "bench_pricing" => "total",
        "bench_order_pipeline" => "grand_total",
        other => panic!("BUG: no latency target rule for bench spec '{other}'"),
    }
}

const WARMUP_ITERATIONS: usize = 1_000;
const MEASURED_ITERATIONS: usize = 10_000;

fn main() {
    let mut table = String::new();
    writeln!(
        &mut table,
        "| Spec | Iterations | Allocations/eval | Bytes allocated/eval | Reallocations/eval | Net bytes retained/eval |"
    )
    .expect("BUG: writing to String never fails");
    writeln!(
        &mut table,
        "|------|-----------:|-----------------:|---------------------:|-------------------:|------------------------:|"
    )
    .expect("BUG: writing to String never fails");

    for fixture in common::fixtures() {
        let engine = common::build_engine(&fixture);
        let plan = engine
            .get_plan(None, fixture.spec_name, Some(&fixture.effective))
            .expect("BUG: bench fixture must produce execution plan");
        let data_template = &fixture.data;
        let target_rule = latency_target_rule(fixture.spec_name).to_string();

        for _ in 0..WARMUP_ITERATIONS {
            let data = data_template.clone();
            let response = engine
                .run_plan(
                    plan,
                    Some(&fixture.effective),
                    data,
                    false,
                    Some(std::slice::from_ref(&target_rule)),
                )
                .expect("BUG: warmup must evaluate");
            std::hint::black_box(response);
        }

        let region = Region::new(GLOBAL);
        for _ in 0..MEASURED_ITERATIONS {
            let data = data_template.clone();
            let response = engine
                .run_plan(
                    plan,
                    Some(&fixture.effective),
                    data,
                    false,
                    Some(std::slice::from_ref(&target_rule)),
                )
                .expect("BUG: memory iteration must evaluate");
            std::hint::black_box(response);
        }
        let stats = region.change();

        let n = MEASURED_ITERATIONS as f64;
        let net_bytes_retained = stats.bytes_allocated as i128 - stats.bytes_deallocated as i128;
        writeln!(
            &mut table,
            "| {} | {} | {:.2} | {:.0} | {:.2} | {:.2} |",
            fixture.spec_name,
            MEASURED_ITERATIONS,
            stats.allocations as f64 / n,
            stats.bytes_allocated as f64 / n,
            stats.reallocations as f64 / n,
            net_bytes_retained as f64 / n,
        )
        .expect("BUG: writing to String never fails");
    }

    print!("{table}");
}
