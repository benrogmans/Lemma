use criterion::{criterion_group, criterion_main, Criterion};
use lemma::*;
use std::collections::HashMap;

fn load_engine() -> Engine {
    let examples_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate must have parent dir")
        .join("documentation/examples");

    let mut paths = Vec::new();
    for entry in std::fs::read_dir(&examples_dir).expect("read examples dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("lemma") {
            paths.push(path);
        }
    }

    let mut engine = Engine::new();
    engine
        .load_from_paths(&paths, false)
        .expect("specs must load");
    engine
}

fn salary_facts() -> HashMap<String, String> {
    [
        ("gross_salary", "5000 eur"),
        ("pay_period", "month"),
        ("income_source", "employment"),
        ("pension_contribution", "150 eur"),
        ("payroll_tax_credit", "true"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

fn bench_dutch_salary_profile(c: &mut Criterion) {
    let engine = load_engine();
    let now = parsing::ast::DateTimeValue::now();
    let spec = "nl/tax/net_salary";
    let facts = salary_facts();

    let mut group = c.benchmark_group("dutch_salary");

    // full engine.evaluate (clone plan + parse facts + evaluate + build response)
    group.bench_function("engine_evaluate", |b| {
        b.iter(|| {
            let resp = engine
                .run(spec, Some(&now), facts.clone(), false)
                .expect("run");
            std::hint::black_box(resp);
        });
    });

    // plan clone + with_fact_values (fact parsing only, no eval)
    group.bench_function("fact_parsing", |b| {
        let base_plan = engine.get_plan(spec, Some(&now)).expect("plan exists");
        b.iter(|| {
            let plan = base_plan
                .clone()
                .with_fact_values(facts.clone(), &ResourceLimits::default())
                .expect("with_fact_values");
            std::hint::black_box(plan);
        });
    });

    // just the plan clone (no fact parsing, no eval)
    group.bench_function("plan_clone", |b| {
        let base_plan = engine.get_plan(spec, Some(&now)).expect("plan exists");
        b.iter(|| {
            let plan = base_plan.clone();
            std::hint::black_box(plan);
        });
    });

    // evaluate single rule only (to measure per-rule cost)
    group.bench_function("single_rule", |b| {
        b.iter(|| {
            let mut resp = engine
                .run(spec, Some(&now), facts.clone(), false)
                .expect("run");
            resp.filter_rules(&[String::from("periods_per_year")]);
            std::hint::black_box(resp);
        });
    });

    // response→JSON for what the HTTP server actually sends (the envelope)
    group.bench_function("json_envelope", |b| {
        let response = engine
            .run(spec, Some(&now), facts.clone(), false)
            .expect("run");
        b.iter(|| {
            let envelope = build_envelope(&response, spec, &now);
            let json = serde_json::to_vec(&envelope).expect("serialize");
            std::hint::black_box(json);
        });
    });

    // raw Response serde (much larger than envelope — includes explanations, types, etc.)
    group.bench_function("json_raw_response", |b| {
        let response = engine
            .run(spec, Some(&now), facts.clone(), false)
            .expect("run");
        b.iter(|| {
            let json = serde_json::to_vec(&response).expect("serialize");
            std::hint::black_box(json);
        });
    });

    group.finish();
}

fn build_envelope(
    response: &Response,
    spec_name: &str,
    effective: &parsing::ast::DateTimeValue,
) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    for (name, rule_result) in &response.results {
        let mut entry = serde_json::Map::new();
        match &rule_result.result {
            OperationResult::Value(v) => {
                let (val, unit) = serialization::literal_value_to_json(v);
                entry.insert("value".into(), val);
                if let Some(u) = unit {
                    entry.insert("unit".into(), serde_json::Value::String(u));
                }
                entry.insert(
                    "display".into(),
                    serde_json::Value::String(v.display_value()),
                );
                entry.insert("vetoed".into(), serde_json::Value::Bool(false));
            }
            OperationResult::Veto(msg) => {
                entry.insert("vetoed".into(), serde_json::Value::Bool(true));
                if let Some(m) = msg {
                    entry.insert("veto_reason".into(), serde_json::Value::String(m.clone()));
                }
            }
        }
        entry.insert(
            "rule_type".into(),
            serde_json::Value::String(rule_result.rule_type.name()),
        );
        result.insert(name.clone(), serde_json::Value::Object(entry));
    }
    serde_json::json!({
        "spec": spec_name,
        "effective": effective.to_string(),
        "result": result,
    })
}

criterion_group!(benches, bench_dutch_salary_profile);
criterion_main!(benches);
