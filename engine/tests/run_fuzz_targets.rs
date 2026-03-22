//! Ignored test that runs fuzz targets the same way CI does.
//! Run with: `cargo test --ignored run_fuzz_targets` (from repo root or engine/).
//! Requires: `rustup install nightly` and `cargo install cargo-fuzz`.

use std::path::Path;
use std::process::Command;

const FUZZ_TARGETS: &[&str] = &[
    "fuzz_parser",
    "fuzz_expressions",
    "fuzz_literals",
    "fuzz_deeply_nested",
    "fuzz_fact_bindings",
];

#[test]
#[ignore = "requires nightly + cargo-fuzz; run with: cargo test --ignored run_fuzz_targets"]
fn run_fuzz_targets() {
    let engine_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fuzz_dir = engine_dir.join("fuzz");
    assert!(
        fuzz_dir.is_dir(),
        "fuzz dir not found at {}",
        fuzz_dir.display()
    );

    let mut cmd = Command::new("cargo");
    cmd.args(["+nightly", "fuzz", "build"])
        .current_dir(&fuzz_dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    let status = cmd.status().expect("failed to run cargo fuzz build");
    assert!(
        status.success(),
        "cargo +nightly fuzz build failed with {}",
        status
    );

    for target in FUZZ_TARGETS {
        let mut cmd = Command::new("cargo");
        cmd.args([
            "+nightly",
            "fuzz",
            "run",
            target,
            "--",
            "-max_total_time=10",
            "-timeout=5",
        ])
        .current_dir(&fuzz_dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
        let status = cmd.status().expect("failed to run fuzz target");
        assert!(
            status.success(),
            "fuzz target {} failed with {}",
            target,
            status
        );
    }
}
