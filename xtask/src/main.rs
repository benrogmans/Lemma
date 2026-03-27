mod versions;

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

fn run_versions_verify() {
    let root = versions::workspace_root();
    if let Err(e) = versions::versions_verify(&root) {
        eprintln!("versions-verify: failed:\n{e}");
        std::process::exit(1);
    }
    eprintln!(
        "versions-verify: ok ({})",
        versions::read_workspace_version(&root).unwrap_or_default()
    );
}

fn precommit() {
    eprintln!("xtask: versions-verify");
    run_versions_verify();
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

fn usage() {
    eprintln!(
        "usage:\n  cargo precommit | cargo run -p xtask\n  cargo verify   | cargo run -p xtask -- versions-verify\n  cargo bump <version> | cargo run -p xtask -- versions-bump <version>"
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let sub = args.next();
    match sub.as_deref() {
        None | Some("precommit") => precommit(),
        Some("versions-verify") => {
            run_versions_verify();
        }
        Some("versions-bump") => {
            let Some(new_v) = args.next() else {
                eprintln!("versions-bump: missing <version>");
                usage();
                std::process::exit(1);
            };
            if args.next().is_some() {
                eprintln!("versions-bump: too many arguments");
                usage();
                std::process::exit(1);
            }
            let root = versions::workspace_root();
            if let Err(e) = versions::versions_bump(&root, &new_v) {
                eprintln!("versions-bump: {e}");
                std::process::exit(1);
            }
            eprintln!("versions-bump: set to {new_v}");
        }
        Some("-h" | "--help" | "help") => {
            usage();
        }
        Some(other) => {
            eprintln!("xtask: unknown subcommand {other:?}");
            usage();
            std::process::exit(1);
        }
    }
}
