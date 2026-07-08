use criterion::{criterion_group, criterion_main, Criterion, Throughput};
mod common;

fn bench_fixture(criterion: &mut Criterion, fixture: &common::Fixture) {
    let engine = common::build_engine(fixture);
    let plan = engine
        .get_plan(None, fixture.spec_name, Some(&fixture.effective))
        .expect("BUG: bench fixture must produce execution plan");
    let data_template = &fixture.data;
    let target_rule = common::terminal_rule(fixture.spec_name).to_string();

    let mut group = criterion.benchmark_group(fixture.spec_name);
    group.throughput(Throughput::Elements(1));

    group.bench_function("run_plan", |bencher| {
        bencher.iter(|| {
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

    group.bench_function("run_plan_explain", |bencher| {
        bencher.iter(|| {
            let data = data_template.clone();
            let response = engine
                .run_plan(
                    plan,
                    Some(&fixture.effective),
                    data,
                    true,
                    Some(std::slice::from_ref(&target_rule)),
                )
                .expect("BUG: bench fixture must evaluate");
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
