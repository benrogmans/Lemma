use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};
use std::alloc::System;
use std::fmt::Write;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

mod common;
#[path = "common/eval.rs"]
mod eval;

const WARMUP_ITERATIONS: usize = 100;
const MEASURED_ITERATIONS: usize = 1_000;

struct IterationStats {
    allocations: usize,
    bytes_allocated: usize,
    reallocations: usize,
    net_bytes_retained: i128,
}

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
        let engine = eval::load_engine(&fixture);
        let terminal = eval::terminal_rule(fixture.spec_name).to_string();

        for _ in 0..WARMUP_ITERATIONS {
            let response = eval::evaluate_loaded(&engine, &fixture, &terminal, false);
            std::hint::black_box(response);
        }

        let mut totals = IterationStats {
            allocations: 0,
            bytes_allocated: 0,
            reallocations: 0,
            net_bytes_retained: 0,
        };
        for _ in 0..MEASURED_ITERATIONS {
            let region = Region::new(GLOBAL);
            let response = eval::evaluate_loaded(&engine, &fixture, &terminal, false);
            std::hint::black_box(response);
            let stats = region.change();
            totals.allocations += stats.allocations;
            totals.bytes_allocated += stats.bytes_allocated;
            totals.reallocations += stats.reallocations;
            totals.net_bytes_retained +=
                stats.bytes_allocated as i128 - stats.bytes_deallocated as i128;
        }

        let n = MEASURED_ITERATIONS as f64;
        writeln!(
            &mut table,
            "| {} | {} | {:.2} | {:.0} | {:.2} | {:.2} |",
            fixture.spec_name,
            MEASURED_ITERATIONS,
            totals.allocations as f64 / n,
            totals.bytes_allocated as f64 / n,
            totals.reallocations as f64 / n,
            totals.net_bytes_retained as f64 / n,
        )
        .expect("BUG: writing to String never fails");
    }

    print!("{table}");
}
