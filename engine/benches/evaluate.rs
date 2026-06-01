use criterion::{criterion_group, criterion_main, Criterion, Throughput};
mod common;

fn bench_fixture(criterion: &mut Criterion, fixture: &common::Fixture) {
    let engine = common::build_engine(fixture);
    let plan = engine
        .get_plan(None, fixture.spec_name, Some(&fixture.effective))
        .expect("BUG: bench fixture must produce execution plan");
    let raw_bytes = fixture.data_json.as_bytes();

    let mut group = criterion.benchmark_group(fixture.spec_name);
    group.throughput(Throughput::Elements(1));

    group.bench_function("run_plan", |bencher| {
        bencher.iter(|| {
            let data = common::parse_data_values(raw_bytes);
            let response = engine
                .run_plan(plan, Some(&fixture.effective), data, false, true)
                .expect("BUG: bench fixture must evaluate");
            std::hint::black_box(response);
        });
    });

    group.bench_function("run_plan_traced", |bencher| {
        bencher.iter(|| {
            let data = common::parse_data_values(raw_bytes);
            let response = engine
                .run_plan(plan, Some(&fixture.effective), data, true, true)
                .expect("BUG: bench fixture must evaluate with trace");
            std::hint::black_box(response);
        });
    });

    group.finish();
}

fn benches(criterion: &mut Criterion) {
    for fixture in common::fixtures() {
        bench_fixture(criterion, &fixture);
    }
}

criterion_group!(group, benches);
criterion_main!(group);
