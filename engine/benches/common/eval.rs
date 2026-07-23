use crate::common::{effective, source_path, Fixture};
use lemma::{Engine, Response, SourceType};
use std::sync::Arc;

pub fn terminal_rule(spec_name: &str) -> &'static str {
    match spec_name {
        "bench_shipping" | "bench_pricing" => "total",
        "bench_order_pipeline" => "grand_total",
        other => panic!("BUG: no terminal rule for bench spec '{other}'"),
    }
}

pub fn load_engine(fixture: &Fixture) -> Engine {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Path(Arc::new(source_path(fixture.lemma_path))),
            fixture.source.to_string(),
        )])
        .expect("BUG: bench fixture spec must load");
    engine
}

pub fn evaluate_loaded(
    engine: &Engine,
    fixture: &Fixture,
    terminal: &str,
    explain: bool,
) -> Response {
    let effective = effective();
    let data = fixture.inputs();
    let rules = [terminal.to_string()];
    engine
        .run(
            None,
            fixture.spec_name,
            Some(&effective),
            data,
            Some(rules.as_slice()),
            explain,
        )
        .expect("BUG: bench fixture must evaluate")
}
