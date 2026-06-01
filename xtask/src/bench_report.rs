//! Engine evaluation benchmark report.
//!
//! Orchestrates three benchmark subprocesses on the same pinned fixtures:
//!
//! 1. `cargo bench -p lemma-engine --bench evaluate` - Criterion latency.
//! 2. `cargo bench -p lemma-engine --bench outputs`  - Lemma per-rule outputs.
//! 3. `python3 engine/benches/python/benchmark.py`   - Python latency + per-rule outputs.
//!
//! Then renders `engine/benches/RESULTS.md` with latency and
//! numerical-accuracy tables. Any subprocess failure aborts the run; no
//! partial reports.

use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;

/// Spec name -> (lemma source path, inputs JSON path) under workspace root.
/// Order here is the order rendered in `RESULTS.md`.
const FIXTURES: &[Fixture] = &[
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

struct Fixture {
    spec_name: &'static str,
    lemma_path: &'static str,
    inputs_path: &'static str,
}

const BENCH_EFFECTIVE_ISO: &str = "2026-01-01T00:00:00Z";
const RESULTS_RELATIVE: &str = "engine/benches/RESULTS.md";
const CRITERION_RELATIVE: &str = "target/criterion";
const PYTHON_BENCH_RELATIVE: &str = "engine/benches/python/benchmark.py";

#[derive(Debug, Clone, Copy)]
struct LatencyRow {
    median_ns: f64,
    std_dev_ns: f64,
}

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

    run_evaluate_bench(root)?;
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

    let rustc_version = capture_stdout("rustc", &["-Vv"], None)?;
    let uname = capture_stdout("uname", &["-srm"], None)?;
    let python_version = capture_stdout("python3", &["--version"], None)?;
    let git_sha = capture_stdout("git", &["rev-parse", "HEAD"], Some(root))?;

    let report = compose_report(ComposeReportContext {
        root,
        rustc_version: &rustc_version,
        uname: &uname,
        python_version: &python_version,
        git_sha: &git_sha,
        latency_rows: &latency_rows,
        python_by_spec: &python_by_spec,
        accuracy: &accuracy,
    })?;

    let out = root.join(RESULTS_RELATIVE);
    fs::write(&out, report).map_err(|error| format!("{}: {error}", out.display()))?;
    eprintln!("bench-report: wrote {}", out.display());
    Ok(())
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
            "python3 not found on PATH; install Python 3.11+ before running bench-report".into(),
        );
    }
    Ok(())
}

fn run_evaluate_bench(root: &Path) -> Result<(), String> {
    let status = Command::new("cargo")
        .current_dir(root)
        .args([
            "bench",
            "-p",
            "lemma-engine",
            "--bench",
            "evaluate",
            "--",
            "--warm-up-time",
            "3",
            "--measurement-time",
            "5",
        ])
        .status()
        .map_err(|error| format!("failed to spawn cargo bench evaluate: {error}"))?;
    if !status.success() {
        return Err(format!(
            "cargo bench evaluate exited with code {:?}",
            status.code()
        ));
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

fn read_latency_estimate(
    root: &Path,
    spec_name: &str,
    function: &str,
) -> Result<LatencyRow, String> {
    let path = root
        .join(CRITERION_RELATIVE)
        .join(spec_name)
        .join(function)
        .join("new")
        .join("estimates.json");
    let raw = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let parsed: Value = serde_json::from_str(&raw)
        .map_err(|error| format!("{}: invalid JSON: {error}", path.display()))?;
    let median_ns = parsed
        .get("median")
        .and_then(|v| v.get("point_estimate"))
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{}: missing median.point_estimate", path.display()))?;
    let std_dev_ns = parsed
        .get("std_dev")
        .and_then(|v| v.get("point_estimate"))
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{}: missing std_dev.point_estimate", path.display()))?;
    Ok(LatencyRow {
        median_ns,
        std_dev_ns,
    })
}

fn capture_stdout(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to spawn {program}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{program} {args:?} exited {:?}: {}",
            output.status.code(),
            stderr.trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .map_err(|error| format!("{program} stdout not UTF-8: {error}"))
}

fn count_rules(source: &str) -> usize {
    source
        .lines()
        .filter(|line| line.trim_start().starts_with("rule "))
        .count()
}

fn format_latency_ns(nanoseconds: f64) -> String {
    if nanoseconds >= 1_000_000.0 {
        format!("{:.3} ms", nanoseconds / 1_000_000.0)
    } else if nanoseconds >= 1_000.0 {
        format!("{:.2} us", nanoseconds / 1_000.0)
    } else {
        format!("{nanoseconds:.0} ns")
    }
}

/// Format a dimensionless ratio with three significant figures.
fn format_ratio(numerator: f64, denominator: f64) -> String {
    if denominator <= 0.0 || numerator <= 0.0 {
        return "—".to_string();
    }
    let ratio = numerator / denominator;
    if ratio >= 1000.0 {
        format!("{ratio:.0}")
    } else if ratio >= 100.0 {
        format!("{ratio:.1}")
    } else if ratio >= 10.0 {
        format!("{ratio:.2}")
    } else if ratio >= 1.0 {
        format!("{ratio:.3}")
    } else if ratio >= 0.01 {
        format!("{ratio:.4}")
    } else if ratio >= 0.0001 {
        format!("{ratio:.5}")
    } else {
        format!("{ratio:.2e}")
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
    rustc_version: &'a str,
    uname: &'a str,
    python_version: &'a str,
    git_sha: &'a str,
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
        rustc_version,
        uname,
        python_version,
        git_sha,
        latency_rows,
        python_by_spec,
        accuracy,
    } = ctx;
    let (stats, deviations) = accuracy;
    let mut out = String::new();
    out.push_str("# Engine evaluation benchmarks\n\n");
    out.push_str(
        "Numbers are produced by `cargo run -p xtask -- bench-report`. \
         Lemma and the hand-written Python ports of the same business rules \
         are measured on identical pinned inputs.\n\n",
    );

    out.push_str("## Methodology\n\n");
    out.push_str("- Per-call boundary on both sides: JSON input bytes -> outputs in memory.\n");
    out.push_str(
        "- Lemma per-call work: `serde_json::from_slice` of the inputs JSON into \
         `HashMap<String, serde_json::Value>`, then \
         `Engine::run_plan(plan, Some(&effective), data, false)`. \
         `run_plan` clones the execution plan, applies declared defaults, \
         converts data values to typed `LiteralValue`, evaluates, and \
         constructs a `Response`.\n",
    );
    out.push_str(
        "- Python per-call work: `compute(build_inputs(json.loads(raw_bytes)))`. \
         `build_inputs` converts the raw `dict[str, str]` to a typed \
         `Inputs` dataclass (every `Decimal` constructed inside the call); \
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
        "- Numerical precision: Lemma's arithmetic uses `num_rational::BigRational` \
         and `rust_decimal::Decimal` internally (see `engine/Cargo.toml`); intermediates \
         stay exact until API output, where they serialize as decimal strings. \
         Python uses `decimal.Decimal` at the default context (`prec=28`, `ROUND_HALF_EVEN`). \
         The accuracy comparison uses `rust_decimal::Decimal` (28-digit precision) so \
         the diff arithmetic matches Python's context.\n",
    );
    out.push_str(
        "- API note: `Engine::run_plan` accepts `HashMap<String, serde_json::Value>`, \
         coupling the public surface to JSON as a wire format. The benchmark mirrors \
         what callers actually pay; an API revision exposing a typed input map is out \
         of scope here.\n\n",
    );

    out.push_str("## Environment\n\n");
    out.push_str(&format!("- Host: `{uname}`\n"));
    out.push_str(&format!("- Lemma git SHA: `{git_sha}`\n"));
    out.push_str(&format!("- Python: `{python_version}`\n"));
    out.push_str("- Rustc:\n\n```\n");
    out.push_str(rustc_version);
    out.push_str("\n```\n\n");

    out.push_str("## Latency\n\n");
    out.push_str(
        "| Spec | Rules | Lemma median | Lemma std dev | Python median | Python iter | Python std dev | Python / Lemma |\n",
    );
    out.push_str(
        "|------|------:|-------------:|--------------:|--------------:|------------:|---------------:|---------------:|\n",
    );
    for fixture in FIXTURES {
        let source = read_relative(root, fixture.lemma_path)?;
        let rules = count_rules(&source);
        let lemma = latency_rows
            .get(fixture.spec_name)
            .copied()
            .ok_or_else(|| format!("missing latency row for {}", fixture.spec_name))?;
        let python = python_by_spec
            .get(fixture.spec_name)
            .ok_or_else(|| format!("missing python fixture for {}", fixture.spec_name))?;
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} (n={}) | {} | {} |\n",
            fixture.spec_name,
            rules,
            format_latency_ns(lemma.median_ns),
            format_latency_ns(lemma.std_dev_ns),
            format_latency_ns(python.latency_median_ns),
            python.iterations_latency,
            format_latency_ns(python.latency_std_dev_ns),
            format_ratio(python.latency_median_ns, lemma.median_ns),
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
         [`python/business_rules/`](python/business_rules). Each module exports \
         `Inputs`, `Outputs`, `build_inputs(raw)`, `compute(inputs)`. \
         Standard library only (`decimal`, `dataclasses`, `json`, `time`, \
         `gc`, `pathlib`, `statistics`). \
         The Python benchmark harness is [`python/benchmark.py`](python/benchmark.py).\n\n",
    );

    out.push_str("## Inputs\n\n");
    out.push_str(&format!(
        "All fixtures share `effective = {BENCH_EFFECTIVE_ISO}` (no timezone). \
         Data values are JSON strings; the benchmark parses them into the \
         engine's `HashMap<String, serde_json::Value>` on every iteration.\n\n",
    ));
    for fixture in FIXTURES {
        let inputs = read_relative(root, fixture.inputs_path)?;
        out.push_str(&format!("### `{}`\n\n", fixture.spec_name));
        out.push_str(&format!(
            "Source: [`{}`]({}). Inputs: [`{}`]({}).\n\n",
            fixture.lemma_path, fixture.lemma_path, fixture.inputs_path, fixture.inputs_path,
        ));
        out.push_str("```json\n");
        out.push_str(inputs.trim_end());
        out.push_str("\n```\n\n");
    }

    Ok(out)
}

fn read_relative(root: &Path, relative: &str) -> Result<String, String> {
    let path = root.join(relative);
    fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
}
