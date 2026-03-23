use std::process::Command;

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn run(args: &[&str]) {
    let status = Command::new(cargo_bin())
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {} {:?}: {e}", cargo_bin(), args));
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn main() {
    eprintln!("xtask: fmt --check");
    run(&["fmt", "--all", "--", "--check"]);
    eprintln!("xtask: clippy");
    run(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ]);
    eprintln!("xtask: nextest");
    run(&[
        "nextest",
        "run",
        "--workspace",
        "--all-features",
        "--run-ignored",
        "all",
    ]);
    eprintln!("xtask: deny");
    run(&["deny", "check", "--config", ".cargo/deny.toml"]);
    eprintln!("xtask: done");
}
