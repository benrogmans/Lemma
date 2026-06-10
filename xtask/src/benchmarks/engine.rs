//! Engine evaluation benchmark report.
//!
//! Run via `cargo benchmarks engine`. Keep [`FIXTURES`] in sync with
//! `engine/benches/common/mod.rs`.

use super::common::{
    capture_environment, capture_stdout, format_latency_ns, format_ratio, push_environment_block,
    read_latency_estimate, read_relative, run_criterion_bench, write_report, EnvironmentInfo,
    LatencyRow,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;

/// Spec name -> (lemma source path, inputs JSON path) under workspace root.
/// Order here is the order rendered in the report.
pub const FIXTURES: &[Fixture] = &[
    Fixture {
        spec_name: "bench_shipping",
        lemma_path: "engine/benches/specs/shipping.lemma",
        inputs_path: "engine/benches/specs/shipping.inputs.json",
    },
    Fixture {
        spec_name: "bench_pricing",
        lemma_path: "engine/benches/specs/pricing.lemma",
        inputs_path: "engine/benches/specs/pricing.inputs.json",
    },
    Fixture {
        spec_name: "bench_order_pipeline",
        lemma_path: "engine/benches/specs/order_pipeline.lemma",
        inputs_path: "engine/benches/specs/order_pipeline.inputs.json",
    },
];

pub struct Fixture {
    pub spec_name: &'static str,
    pub lemma_path: &'static str,
    pub inputs_path: &'static str,
}

const BENCH_EFFECTIVE_ISO: &str = "2026-01-01T00:00:00Z";
pub const RESULTS_RELATIVE: &str = "documentation/benchmarks/engine.md";
const PYTHON_BENCH_RELATIVE: &str = "engine/benches/python/benchmark.py";

#[derive(Debug, Deserialize)]
struct PythonReport {
    fixtures: Vec<PythonFixture>,
}

#[derive(Debug, Deserialize)]
struct PythonFixture {
    spec_name: String,
    iterations_latency: u64,
    latency_median_ns: f64,
    latency_std_dev_ns: f64,
    outputs: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct OutputsReport {
    fixtures: Vec<OutputsFixture>,
}

#[derive(Debug, Deserialize)]
struct OutputsFixture {
    spec_name: String,
    outputs: BTreeMap<String, LemmaOutput>,
}

#[derive(Debug, Deserialize)]
struct LemmaOutput {
    kind: String,
    value: String,
    unit: Option<String>,
}

pub fn run(root: &Path) -> Result<(), String> {
    require_python3()?;

    run_criterion_bench(root, "lemma-engine", "evaluate")?;
    let outputs_stdout = run_outputs_bench(root)?;
    let outputs_report: OutputsReport = serde_json::from_str(outputs_stdout.trim())
        .map_err(|error| format!("outputs bench stdout was not valid JSON: {error}"))?;
    let python_stdout = run_python_benchmark(root)?;
    let python_report: PythonReport = serde_json::from_str(python_stdout.trim())
        .map_err(|error| format!("python benchmark stdout was not valid JSON: {error}"))?;

    let mut latency_rows: BTreeMap<&'static str, LatencyRow> = BTreeMap::new();
    for fixture in FIXTURES {
        let untraced = read_latency_estimate(root, fixture.spec_name, "run_plan")?;
        latency_rows.insert(fixture.spec_name, untraced);
    }

    let python_by_spec: BTreeMap<String, &PythonFixture> = python_report
        .fixtures
        .iter()
        .map(|f| (f.spec_name.clone(), f))
        .collect();
    let outputs_by_spec: BTreeMap<String, &OutputsFixture> = outputs_report
        .fixtures
        .iter()
        .map(|f| (f.spec_name.clone(), f))
        .collect();
    for fixture in FIXTURES {
        if !python_by_spec.contains_key(fixture.spec_name) {
            return Err(format!(
                "python benchmark JSON missing spec '{}'",
                fixture.spec_name
            ));
        }
        if !outputs_by_spec.contains_key(fixture.spec_name) {
            return Err(format!(
                "outputs bench JSON missing spec '{}'",
                fixture.spec_name
            ));
        }
    }

    let accuracy = compute_accuracy(&outputs_by_spec, &python_by_spec)?;
    let env = capture_environment(root)?;
    let python_version = capture_stdout("python3", &["--version"], None)?;

    let report = compose_report(ComposeReportContext {
        root,
        env: &env,
        python_version: &python_version,
        latency_rows: &latency_rows,
        python_by_spec: &python_by_spec,
        accuracy: &accuracy,
    })?;

    write_report(root, RESULTS_RELATIVE, &report)
}

fn require_python3() -> Result<(), String> {
    let ok = Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return Err(
            "python3 not found on PATH; install Python 3.11+ before running benchmarks engine"
                .into(),
        );
    }
    Ok(())
}

fn run_outputs_bench(root: &Path) -> Result<String, String> {
    let output = Command::new("cargo")
        .current_dir(root)
        .args(["bench", "-p", "lemma-engine", "--bench", "outputs"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("failed to spawn cargo bench outputs: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo bench outputs exited with code {:?}",
            output.status.code()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("outputs bench stdout was not UTF-8: {error}"))
}

fn run_python_benchmark(root: &Path) -> Result<String, String> {
    let output = Command::new("python3")
        .current_dir(root)
        .arg(PYTHON_BENCH_RELATIVE)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("failed to spawn python3 {PYTHON_BENCH_RELATIVE}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "python3 {PYTHON_BENCH_RELATIVE} exited with code {:?}",
            output.status.code()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("python benchmark stdout was not UTF-8: {error}"))
}

fn latency_terminal_rule(spec_name: &str) -> &'static str {
    match spec_name {
        "bench_shipping" | "bench_pricing" => "total",
        "bench_order_pipeline" => "grand_total",
        other => panic!("BUG: unknown bench spec '{other}'"),
    }
}

#[derive(Debug, Clone)]
struct AccuracyDeviation {
    spec_name: &'static str,
    rule_name: String,
    lemma_repr: String,
    python_repr: String,
    abs_delta: Option<String>,
    rel_delta_percent: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
struct AccuracyStats {
    compared: usize,
    deviations: usize,
}

fn compute_accuracy(
    outputs_by_spec: &BTreeMap<String, &OutputsFixture>,
    python_by_spec: &BTreeMap<String, &PythonFixture>,
) -> Result<(AccuracyStats, Vec<AccuracyDeviation>), String> {
    let mut stats = AccuracyStats::default();
    let mut deviations: Vec<AccuracyDeviation> = Vec::new();

    for fixture in FIXTURES {
        let lemma_outputs = &outputs_by_spec[fixture.spec_name].outputs;
        let python_outputs = &python_by_spec[fixture.spec_name].outputs;

        let mut all_rules: std::collections::BTreeSet<&String> = lemma_outputs.keys().collect();
        for k in python_outputs.keys() {
            all_rules.insert(k);
        }

        for rule_name in all_rules {
            stats.compared += 1;
            let lemma = lemma_outputs.get(rule_name);
            let python = python_outputs.get(rule_name);

            match (lemma, python) {
                (Some(l), Some(p)) => {
                    if let Some(dev) = diff_pair(fixture.spec_name, rule_name, l, p)? {
                        stats.deviations += 1;
                        deviations.push(dev);
                    }
                }
                (Some(l), None) => {
                    stats.deviations += 1;
                    deviations.push(AccuracyDeviation {
                        spec_name: fixture.spec_name,
                        rule_name: rule_name.clone(),
                        lemma_repr: format!("{} = {}", l.kind, l.value),
                        python_repr: "<missing>".to_string(),
                        abs_delta: None,
                        rel_delta_percent: None,
                    });
                }
                (None, Some(p)) => {
                    stats.deviations += 1;
                    deviations.push(AccuracyDeviation {
                        spec_name: fixture.spec_name,
                        rule_name: rule_name.clone(),
                        lemma_repr: "<missing>".to_string(),
                        python_repr: p.clone(),
                        abs_delta: None,
                        rel_delta_percent: None,
                    });
                }
                (None, None) => unreachable!("BUG: rule name came from union of both maps"),
            }
        }
    }
    Ok((stats, deviations))
}

fn diff_pair(
    spec_name: &'static str,
    rule_name: &str,
    lemma: &LemmaOutput,
    python_value: &String,
) -> Result<Option<AccuracyDeviation>, String> {
    match lemma.kind.as_str() {
        "number" | "ratio" | "quantity" | "calendar" => {
            let lemma_dec = Decimal::from_str(&lemma.value).map_err(|error| {
                format!(
                    "BUG: spec '{spec_name}' rule '{rule_name}' Lemma value '{}' exceeds rust_decimal precision: {error}",
                    lemma.value
                )
            })?;
            let python_dec = Decimal::from_str(python_value).map_err(|error| {
                format!(
                    "BUG: spec '{spec_name}' rule '{rule_name}' Python value '{python_value}' exceeds rust_decimal precision: {error}"
                )
            })?;
            if lemma_dec == python_dec {
                return Ok(None);
            }
            let abs_delta = (lemma_dec - python_dec).abs();
            let rel_delta_percent = if lemma_dec.is_zero() {
                if python_dec.is_zero() {
                    Some("0".to_string())
                } else {
                    Some("undefined".to_string())
                }
            } else {
                let raw = (abs_delta / lemma_dec.abs()) * Decimal::from(100);
                Some(raw.to_string())
            };
            Ok(Some(AccuracyDeviation {
                spec_name,
                rule_name: rule_name.to_string(),
                lemma_repr: lemma_display_repr(lemma),
                python_repr: python_value.clone(),
                abs_delta: Some(abs_delta.to_string()),
                rel_delta_percent,
            }))
        }
        "boolean" | "text" | "date" | "time" | "veto" => {
            if lemma.value == *python_value {
                Ok(None)
            } else {
                Ok(Some(AccuracyDeviation {
                    spec_name,
                    rule_name: rule_name.to_string(),
                    lemma_repr: format!("{} = {}", lemma.kind, lemma.value),
                    python_repr: python_value.clone(),
                    abs_delta: None,
                    rel_delta_percent: None,
                }))
            }
        }
        other => Err(format!(
            "BUG: spec '{spec_name}' rule '{rule_name}' has unknown Lemma kind '{other}'"
        )),
    }
}

struct ComposeReportContext<'a> {
    root: &'a Path,
    env: &'a EnvironmentInfo,
    python_version: &'a str,
    latency_rows: &'a BTreeMap<&'static str, LatencyRow>,
    python_by_spec: &'a BTreeMap<String, &'a PythonFixture>,
    accuracy: &'a (AccuracyStats, Vec<AccuracyDeviation>),
}

fn lemma_display_repr(lemma: &LemmaOutput) -> String {
    match lemma.unit.as_deref() {
        Some(unit) => format!("{} {}", lemma.value, unit),
        None => lemma.value.clone(),
    }
}

fn compose_report(ctx: ComposeReportContext<'_>) -> Result<String, String> {
    let ComposeReportContext {
        root,
        env,
        python_version,
        latency_rows,
        python_by_spec,
        accuracy,
    } = ctx;
    let (stats, deviations) = accuracy;
    let mut out = String::new();
    out.push_str("# Engine evaluation benchmarks\n\n");
    out.push_str(
        "Numbers are produced by `cargo benchmarks engine`. \
         Lemma and the hand-written Python ports of the same business rules \
         are measured on identical pinned inputs.\n\n",
    );

    out.push_str("## Methodology\n\n");
    out.push_str(
        "- Per-call boundary on both sides: typed inputs -> outputs in memory. \
         Fixture JSON is loaded once per spec before warmup.\n",
    );
    out.push_str(
        "- Lemma per-call work: clone a pre-built `HashMap<String, DataValueInput>`, \
         then `Engine::run_plan(plan, Some(&effective), data, explain: false, Some(&[terminal_rule]))` \
         where `terminal_rule` is `total` (shipping, pricing) or `grand_total` (order_pipeline). \
         One VM pass per call. `run_plan` applies declared defaults via `DataOverlay::resolve`, \
         converts data values to typed `LiteralValue`, evaluates the requested rule(s), and \
         constructs a `Response` (explanation trees omitted).\n",
    );
    out.push_str(
        "- Python per-call work: `compute(build_inputs(raw_dict))` where \
         `raw_dict` is the pre-parsed `dict[str, str]` from the fixture JSON. \
         `build_inputs` lifts string values to exact `fractions.Fraction`; \
         `compute` returns a typed `Outputs` dataclass with one field per \
         Lemma rule.\n",
    );
    out.push_str(&format!(
        "- Effective pinned to `{BENCH_EFFECTIVE_ISO}` (no timezone) on the Lemma side; Python rules carry no temporal logic.\n",
    ));
    out.push_str(
        "- Latency: Criterion (3s warmup, 5s measurement) for Lemma; \
         1_000 warmup + 100_000 measured `time.perf_counter_ns()` samples with \
         `gc.disable()` bracketing for Python. Median and standard deviation reported.\n",
    );
    out.push_str(
        "- Numerical precision: Lemma evaluates with exact arbitrary-precision \
         rationals internally; API output commits rationals to decimal strings. Python ports use \
         exact `fractions.Fraction` arithmetic and commit to decimal strings at \
         the output boundary. The accuracy table compares both sides via \
         `rust_decimal::Decimal` (28-digit precision).\n",
    );
    out.push_str(
        "- API note: `Engine::run_plan` accepts `HashMap<String, DataValueInput>`. \
         The benchmark mirrors what native callers pay after constructing typed inputs; \
         JSON parsing at API boundaries (CLI, WASM) is out of scope.\n\n",
    );

    push_environment_block(&mut out, env, Some(python_version));

    out.push_str("## Latency\n\n");
    out.push_str(
        "| Spec | Terminal rule | Lemma median | Lemma std dev | Python median | Python iter | Python std dev | Python / Lemma |\n",
    );
    out.push_str(
        "|------|---------------|-------------:|--------------:|--------------:|------------:|---------------:|---------------:|\n",
    );
    for fixture in FIXTURES {
        let terminal_rule = latency_terminal_rule(fixture.spec_name);
        let lemma = latency_rows
            .get(fixture.spec_name)
            .copied()
            .ok_or_else(|| format!("missing latency row for {}", fixture.spec_name))?;
        let python = python_by_spec
            .get(fixture.spec_name)
            .ok_or_else(|| format!("missing python fixture for {}", fixture.spec_name))?;
        out.push_str(&latency_table_row(
            fixture.spec_name,
            terminal_rule,
            lemma,
            python,
        ));
    }
    out.push('\n');

    out.push_str("## Numerical accuracy\n\n");
    out.push_str(&format!(
        "{} rule outputs compared across the three fixtures; {} deviations.\n\n",
        stats.compared, stats.deviations,
    ));
    if !deviations.is_empty() {
        out.push_str("| Spec | Rule | Lemma | Python | Abs delta | Rel delta % |\n");
        out.push_str("|------|------|------:|-------:|----------:|------------:|\n");
        for dev in deviations {
            out.push_str(&format!(
                "| `{}` | `{}` | `{}` | `{}` | {} | {} |\n",
                dev.spec_name,
                dev.rule_name,
                dev.lemma_repr,
                dev.python_repr,
                dev.abs_delta.as_deref().unwrap_or("—"),
                dev.rel_delta_percent.as_deref().unwrap_or("—"),
            ));
        }
        out.push('\n');
    }

    out.push_str("## Python implementation\n\n");
    out.push_str(
        "Hand-written ports of the three Lemma specs live in \
         [`../../engine/benches/python/business_rules/`](../../engine/benches/python/business_rules). \
         Each module exports `Inputs`, `Outputs`, `build_inputs(raw)`, `compute(inputs)`. \
         Standard library only (`decimal`, `dataclasses`, `json`, `time`, \
         `gc`, `pathlib`, `statistics`). \
         The Python benchmark harness is [`../../engine/benches/python/benchmark.py`](../../engine/benches/python/benchmark.py).\n\n",
    );

    out.push_str("## Inputs\n\n");
    out.push_str(&format!(
        "All fixtures share `effective = {BENCH_EFFECTIVE_ISO}` (no timezone). \
         Data values are JSON strings; the benchmark parses them into the \
         engine's `HashMap<String, serde_json::Value>` on every iteration.\n\n",
    ));
    for fixture in FIXTURES {
        let inputs = read_relative(root, fixture.inputs_path)?;
        let lemma_link = format!("../../{}", fixture.lemma_path);
        let inputs_link = format!("../../{}", fixture.inputs_path);
        out.push_str(&format!("### `{}`\n\n", fixture.spec_name));
        out.push_str(&format!(
            "Source: [`{}`]({lemma_link}). Inputs: [`{}`]({inputs_link}).\n\n",
            fixture.lemma_path, fixture.inputs_path,
        ));
        out.push_str("```json\n");
        out.push_str(inputs.trim_end());
        out.push_str("\n```\n\n");
    }

    Ok(out)
}

fn latency_table_row(
    spec_name: &str,
    terminal_rule: &str,
    lemma: LatencyRow,
    python: &PythonFixture,
) -> String {
    format!(
        "| `{}` | `{}` | {} | {} | {} | {} | {} | {} |\n",
        spec_name,
        terminal_rule,
        format_latency_ns(lemma.median_ns),
        format_latency_ns(lemma.std_dev_ns),
        format_latency_ns(python.latency_median_ns),
        python.iterations_latency,
        format_latency_ns(python.latency_std_dev_ns),
        format_ratio(python.latency_median_ns, lemma.median_ns),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_python() -> PythonFixture {
        PythonFixture {
            spec_name: "bench_shipping".to_string(),
            iterations_latency: 100_000,
            latency_median_ns: 6_990.0,
            latency_std_dev_ns: 4_170.0,
            outputs: BTreeMap::new(),
        }
    }

    #[test]
    fn latency_table_row_has_eight_columns_including_ratio() {
        let row = latency_table_row(
            "bench_shipping",
            "total",
            LatencyRow {
                median_ns: 22_580.0,
                std_dev_ns: 4_940.0,
            },
            &sample_python(),
        );
        let cells: Vec<&str> = row
            .trim()
            .split('|')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .collect();
        assert_eq!(cells.len(), 8, "row: {row}");
        assert_eq!(cells[0], "`bench_shipping`");
        assert_eq!(cells[4], "6.99 us");
        assert_eq!(cells[5], "100000");
        assert_eq!(
            cells[7],
            format_ratio(sample_python().latency_median_ns, 22_580.0)
        );
    }

    #[test]
    fn compose_report_mentions_engine_suite_command() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let env = EnvironmentInfo {
            rustc_version: "rustc test".to_string(),
            uname: "Linux test x86_64".to_string(),
            git_sha: "abc123".to_string(),
        };
        let row = LatencyRow {
            median_ns: 1.0,
            std_dev_ns: 1.0,
        };
        let mut latency_rows = BTreeMap::new();
        let python_owned: Vec<PythonFixture> = FIXTURES
            .iter()
            .map(|fixture| PythonFixture {
                spec_name: fixture.spec_name.to_string(),
                iterations_latency: 100_000,
                latency_median_ns: 1.0,
                latency_std_dev_ns: 1.0,
                outputs: BTreeMap::new(),
            })
            .collect();
        let mut python_by_spec = BTreeMap::new();
        for (fixture, python) in FIXTURES.iter().zip(python_owned.iter()) {
            latency_rows.insert(fixture.spec_name, row);
            python_by_spec.insert(fixture.spec_name.to_string(), python);
        }
        let accuracy = (AccuracyStats::default(), Vec::new());

        let report = compose_report(ComposeReportContext {
            root: &root,
            env: &env,
            python_version: "Python 3.12.3",
            latency_rows: &latency_rows,
            python_by_spec: &python_by_spec,
            accuracy: &accuracy,
        })
        .expect("compose");

        assert!(report.contains("cargo benchmarks engine"));
        assert!(report.contains("../../engine/benches/specs/shipping.lemma"));
    }
}
