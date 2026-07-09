//! Isolated micro-benchmarks for evaluation hot-path components.
//!
//! Run: `cargo bench -p lemma-engine --bench internal_micro`
//!
//! Attribution workflow for remaining `order_pipeline` allocations:
//! - `data_overlay_resolve_order_pipeline` — parse + validate caller inputs into overlay
//! - `run_plan_order_pipeline` — full per-call path including API `HashMap` clone
//!
//! CPU attribution: `cargo flamegraph -p lemma-engine --bench evaluate -- --bench bench_order_pipeline/run_plan`
//! Memory totals: `cargo bench -p lemma-engine --bench memory -- --noplot`

use criterion::{criterion_group, criterion_main, Criterion};
mod common;

fn bench_data_overlay_resolve(c: &mut Criterion) {
    let fixture = common::fixtures()
        .into_iter()
        .find(|f| f.spec_name == "bench_order_pipeline")
        .expect("BUG: order_pipeline fixture must exist");
    let engine = common::build_engine(&fixture);
    let plan = engine
        .get_plan(None, fixture.spec_name, Some(&fixture.effective))
        .expect("BUG: bench fixture must produce execution plan");
    let data_template = fixture.data.clone();

    c.bench_function("data_overlay_resolve_order_pipeline", |b| {
        b.iter(|| {
            let overlay = lemma::DataOverlay::resolve(
                plan,
                data_template.clone(),
                &lemma::ResourceLimits::default(),
            )
            .expect("BUG: overlay must resolve");
            std::hint::black_box(overlay);
        });
    });
}

fn bench_run_plan_order_pipeline(c: &mut Criterion) {
    let fixture = common::fixtures()
        .into_iter()
        .find(|f| f.spec_name == "bench_order_pipeline")
        .expect("BUG: order_pipeline fixture must exist");
    let engine = common::build_engine(&fixture);
    let plan = engine
        .get_plan(None, fixture.spec_name, Some(&fixture.effective))
        .expect("BUG: bench fixture must produce execution plan");
    let data_template = fixture.data;
    let target_rule = common::terminal_rule(fixture.spec_name).to_string();

    c.bench_function("run_plan_order_pipeline", |b| {
        b.iter(|| {
            let data = data_template.clone();
            let response = engine
                .run_plan(
                    plan,
                    Some(&fixture.effective),
                    data,
                    false,
                    Some(std::slice::from_ref(&target_rule)),
                )
                .expect("BUG: bench fixture must evaluate");
            std::hint::black_box(response);
        });
    });
}

criterion_group!(
    internal_micro,
    bench_data_overlay_resolve,
    bench_run_plan_order_pipeline,
);
criterion_main!(internal_micro);
