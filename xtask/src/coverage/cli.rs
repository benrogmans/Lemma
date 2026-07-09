use super::{check_report, run_coverage_and_write, ReportConfig};
use std::path::Path;

pub const RESULTS_RELATIVE: &str = "documentation/reference/coverage/cli.md";

pub const INPUT_PATHS: &[&str] = &["cli/src", "cli/tests", "cli/Cargo.toml"];

const METHODOLOGY: &str = "\
- Tool: [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) driving \
  [`cargo-nextest`](https://nexte.st/) on native targets.
- Scope: `lemma` CLI crate library unit tests, integration tests (`cli/tests/**`), \
  and the `lemma` binary entrypoint via \
  `cargo llvm-cov nextest -p lemma --lib --tests --bin lemma`.
- Line, function, and region percentages come from LLVM source-based coverage.
- Each run starts with `cargo llvm-cov clean` on the target crate so repeated measurements stay deterministic.
- Tests run single-threaded (`NEXTEST_TEST_THREADS=1`) so coverage counters stay stable across runs.";

const OUT_OF_SCOPE: &str = "\
### Out of scope

- `lemma-engine` source (see [Engine test coverage](engine.md); CLI integration tests \
  exercise engine code but engine line coverage is authoritative there)
- WASM npm package, Hex NIF, LSP, and OpenAPI crates";

const RELATED: &str = "\
- [CLI integration test catalog](../../../cli/tests/README.md)
- [Engine test coverage](engine.md)
- [CLI benchmarks](../benchmarks/cli.md)";

pub fn check(root: &Path) -> Result<(), String> {
    check_report(root, RESULTS_RELATIVE, INPUT_PATHS)
}

pub fn run(root: &Path) -> Result<(), String> {
    let config = ReportConfig {
        command_label: "cli",
        nav_title: "CLI test coverage",
        nav_order: 56,
        title: "CLI test coverage",
        results_relative: RESULTS_RELATIVE,
        src_prefix: "cli/src",
        input_paths: INPUT_PATHS,
        methodology: METHODOLOGY,
        out_of_scope: OUT_OF_SCOPE,
        related_docs: RELATED,
    };
    run_coverage_and_write(
        root,
        &config,
        "lemma",
        &["-p", "lemma", "--lib", "--tests", "--bin", "lemma"],
    )
}
