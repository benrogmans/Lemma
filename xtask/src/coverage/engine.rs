use super::{check_report, run_coverage_and_write, ReportConfig};
use std::path::Path;

pub const RESULTS_RELATIVE: &str = "cli/documentation/reference/coverage/engine.md";

pub const INPUT_PATHS: &[&str] = &["engine/src", "engine/tests", "engine/Cargo.toml"];

const METHODOLOGY: &str = "\
- Tool: [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) driving \
  [`cargo-nextest`](https://nexte.st/) on native (non-wasm) targets.
- Scope: `lemma-engine` library unit tests (`engine/src/**`) plus integration \
  tests (`engine/tests/**`) via `cargo llvm-cov nextest -p lemma-engine --lib --tests`.
- Line, function, and region percentages come from LLVM source-based coverage.
- Each run starts with `cargo llvm-cov clean` on the target crate so repeated measurements stay deterministic.
- Tests run single-threaded (`NEXTEST_TEST_THREADS=1`) so coverage counters stay stable across runs.";

const OUT_OF_SCOPE: &str = "\
### Out of scope

- `engine/src/wasm.rs` (built for `wasm32-unknown-unknown` only)
- Fuzz targets under `engine/fuzz/`
- Hex NIF (`lemma_hex`), LSP, OpenAPI, and CLI crates";

const RELATED: &str = "\
- [Engine integration test catalog](../../../engine/tests/README.md) — qualitative map \
  of scenarios and subsystem overlap clusters
- [CLI test coverage](cli.md)
- [Engine benchmarks](../benchmarks/engine.md)";

pub fn check(root: &Path) -> Result<(), String> {
    check_report(root, RESULTS_RELATIVE, INPUT_PATHS)
}

pub fn run(root: &Path) -> Result<(), String> {
    let config = ReportConfig {
        command_label: "engine",
        nav_title: "Engine test coverage",
        nav_order: 55,
        title: "Engine test coverage",
        results_relative: RESULTS_RELATIVE,
        src_prefix: "engine/src",
        input_paths: INPUT_PATHS,
        methodology: METHODOLOGY,
        out_of_scope: OUT_OF_SCOPE,
        related_docs: RELATED,
    };
    run_coverage_and_write(
        root,
        &config,
        "lemma-engine",
        &["-p", "lemma-engine", "--lib", "--tests"],
    )
}
