//! Differential testing: the optimized instruction stream and the
//! source-shaped (un-optimized) stream must produce identical results.
//!
//! Normalization is an optional rewrite pass over the compiled normal form;
//! by definition it must preserve observable semantics. Explanations execute
//! the source stream while normal evaluation executes the optimized stream,
//! so this invariant is what guarantees `-x` shows the same numbers as a
//! plain run. Evaluating the same spec with `explain` on and off exercises
//! both streams end to end.

use lemma::{DateTimeValue, Engine};
use std::collections::HashMap;

fn assert_streams_agree(code: &str, spec: &str, inputs: &[(&str, &str)]) {
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .unwrap_or_else(|e| panic!("spec must load: {e:?}"));
    let now = DateTimeValue::now();
    let data: HashMap<String, String> = inputs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // explain=false executes the optimized stream; explain=true executes the
    // source stream with recording. Results must be identical.
    let optimized = engine
        .run(None, spec, Some(&now), data.clone(), false, None)
        .expect("optimized evaluation succeeds");
    let source = engine
        .run(None, spec, Some(&now), data, true, None)
        .expect("source-stream evaluation succeeds");

    let mut optimized_rules: Vec<&String> = optimized.results.keys().collect();
    let mut source_rules: Vec<&String> = source.results.keys().collect();
    optimized_rules.sort();
    source_rules.sort();
    assert_eq!(optimized_rules, source_rules, "rule sets must match");

    for (name, optimized_result) in &optimized.results {
        let source_result = &source.results[name];
        assert_eq!(
            optimized_result.display, source_result.display,
            "rule '{name}': optimized vs source display"
        );
        assert_eq!(
            optimized_result.vetoed, source_result.vetoed,
            "rule '{name}': optimized vs source veto status"
        );
        assert_eq!(
            optimized_result.veto_reason, source_result.veto_reason,
            "rule '{name}': optimized vs source veto reason"
        );
    }
}

#[test]
fn arithmetic_and_unless_chain_streams_agree() {
    let code = r#"
spec calc

data money: quantity
  -> decimals 2
  -> unit eur 1

data hourly_rate: 85.00 eur
data hours_worked: 37.5
data is_rush: boolean
data is_super_rush: boolean

rule labor: hourly_rate * hours_worked
rule rush_surcharge: 0 eur
  unless is_rush then labor * 25%
  unless is_super_rush then labor * 50%
rule subtotal: labor + rush_surcharge
rule vat: subtotal * 21%
rule total: subtotal + vat
"#;
    for (rush, super_rush) in [
        ("false", "false"),
        ("true", "false"),
        ("true", "true"),
        ("false", "true"),
    ] {
        assert_streams_agree(
            code,
            "calc",
            &[("is_rush", rush), ("is_super_rush", super_rush)],
        );
    }
}

#[test]
fn unit_conversions_and_compound_signatures_streams_agree() {
    let code = r#"
spec delivery

data distance: quantity
  -> unit meter 1
  -> unit kilometer 1000
  -> unit mile 1609.34

data money: quantity
  -> unit eur 1.00
  -> unit usd 0.84

data distance_rate: quantity
  -> unit eur_per_km eur/kilometer
  -> unit usd_per_mile usd/mile

rule delivery_cost:
  0.50 eur_per_km * distance
  unless distance < 5 mile then 0 usd
rule distance_in_miles: distance as mile
"#;
    for value in ["50 kilometer", "3 mile", "800 meter"] {
        assert_streams_agree(code, "delivery", &[("distance", value)]);
    }
}

#[test]
fn vetoes_and_logic_streams_agree() {
    let code = r#"
spec guards

data input: number
data flag: boolean
data other: boolean

rule guard: input
  unless input > 1000 then veto "too large"
rule conjunction: flag and other
rule negated: not flag
rule chained: 1
  unless guard > 50 then 2
rule math: sqrt input
rule identity: input * 1 + 0
"#;
    assert_streams_agree(
        code,
        "guards",
        &[("input", "2000"), ("flag", "true"), ("other", "false")],
    );
    assert_streams_agree(
        code,
        "guards",
        &[("input", "49"), ("flag", "false"), ("other", "true")],
    );
    // Missing data exercises veto propagation through both streams.
    assert_streams_agree(code, "guards", &[("flag", "false"), ("other", "false")]);
}

#[test]
fn documentation_examples_streams_agree_on_defaults() {
    // Every documentation example must produce identical results in both
    // streams when run with defaults only (rules without satisfiable data
    // veto identically in both streams).
    let examples = std::path::Path::new("../documentation/examples");
    if !examples.exists() {
        panic!("documentation/examples directory must exist for differential coverage");
    }
    let mut checked = 0usize;
    let mut stack = vec![examples.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read examples dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("lemma") {
                continue;
            }
            let code = std::fs::read_to_string(&path).expect("read example");
            let mut engine = Engine::new();
            if engine.load(&code, lemma::SourceType::Volatile).is_err() {
                // Examples with external dependencies are out of scope here.
                continue;
            }
            let specs: Vec<String> = engine
                .list()
                .into_iter()
                .flat_map(|repo| repo.specs.into_iter().map(|s| s.name.clone()))
                .collect();
            let now = DateTimeValue::now();
            for spec_name in specs {
                let optimized =
                    engine.run(None, &spec_name, Some(&now), HashMap::new(), false, None);
                let source = engine.run(None, &spec_name, Some(&now), HashMap::new(), true, None);
                let (Ok(optimized), Ok(source)) = (optimized, source) else {
                    continue;
                };
                for (name, optimized_result) in &optimized.results {
                    let source_result = &source.results[name];
                    assert_eq!(
                        optimized_result.display,
                        source_result.display,
                        "{}: rule '{name}' optimized vs source display",
                        path.display()
                    );
                    assert_eq!(
                        optimized_result.vetoed,
                        source_result.vetoed,
                        "{}: rule '{name}' optimized vs source veto status",
                        path.display()
                    );
                }
                checked += 1;
            }
        }
    }
    assert!(
        checked > 0,
        "differential coverage must evaluate at least one documentation example"
    );
}
