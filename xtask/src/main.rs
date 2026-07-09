mod benchmarks;
mod coverage;
mod hex_standalone;
mod lsp;
mod versions;
mod versions_diff;

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

const HEX_PACKAGE_DIR: &str = "engine/packages/hex";
const NPM_WASM_DIR: &str = "engine/packages/npm";

fn require_command(name: &str, install_hint: &str) {
    let ok = Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        panic!("{name} not found on PATH. {install_hint}");
    }
}

fn run_npm_wasm_precommit() {
    require_command("node", "Install Node.js (https://nodejs.org/).");
    require_command(
        "wasm-pack",
        "Install wasm-pack: cargo install wasm-pack --version 0.14.0 --locked",
    );
    let root = versions::workspace_root();
    let npm_dir = root.join(NPM_WASM_DIR);
    for script in ["build.js", "test.js"] {
        eprintln!("xtask: npm wasm (node {script})");
        let status = Command::new("node")
            .arg(script)
            .current_dir(&npm_dir)
            .status()
            .unwrap_or_else(|e| {
                panic!("failed to run node {script} in {}: {e}", npm_dir.display())
            });
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}

fn run_mix_precommit() {
    require_command(
        "mix",
        "Install Elixir and Mix (https://elixir-lang.org/install.html).",
    );
    let hex_dir = versions::workspace_root().join(HEX_PACKAGE_DIR);
    let status = Command::new("mix")
        .current_dir(&hex_dir)
        .arg("precommit")
        .status()
        .unwrap_or_else(|e| panic!("failed to run mix precommit in {}: {e}", hex_dir.display()));
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn run_vscode_precommit() {
    require_command("npm", "Install Node.js (https://nodejs.org/).");
    let dir = versions::workspace_root().join(lsp::VSCODE_EXTENSION_REL);
    let status = Command::new("npm")
        .current_dir(&dir)
        .args(["run", "precommit"])
        .status()
        .unwrap_or_else(|e| panic!("failed to run npm run precommit in {}: {e}", dir.display()));
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn run_deny_precommit() {
    let config = versions::workspace_root().join(".cargo/deny.toml");
    eprintln!("xtask: deny ({})", config.display());
    let status = Command::new(cargo_bin())
        .args(["deny", "check", "--config"])
        .arg(&config)
        .status()
        .unwrap_or_else(|e| panic!("failed to run cargo deny: {e}"));
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn precommit() {
    eprintln!("xtask: versions-verify");
    run_versions_verify();
    eprintln!("xtask: mix precommit");
    run_mix_precommit();
    eprintln!("xtask: vscode npm precommit");
    run_vscode_precommit();
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
    eprintln!("xtask: npm wasm package");
    run_npm_wasm_precommit();
    run_deny_precommit();
    eprintln!("xtask: coverage --check");
    let root = versions::workspace_root();
    if let Err(e) = coverage::run(&root, &[String::from("all"), String::from("--check")]) {
        eprintln!("coverage: {e}");
        std::process::exit(1);
    }
    eprintln!("xtask: done");
}

fn usage() {
    eprintln!(
        "usage:\n  cargo precommit | cargo run -p xtask\n  cargo verify   | cargo run -p xtask -- versions-verify\n  cargo bump <version> | cargo run -p xtask -- versions-bump <version>\n  cargo changelog | cargo run -p xtask -- versions-diff [semver]\n  cargo lsp | cargo run -p xtask -- lsp [vsix|prepare|--help]\n  cargo run -p xtask -- hex-standalone\n  cargo benchmarks <engine|cli|all> | cargo run -p xtask -- benchmarks <engine|cli|all>\n  cargo coverage <engine|cli|all> [--check] | cargo run -p xtask -- coverage <engine|cli|all> [--check]"
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
        Some("hex-standalone") => {
            if args.next().is_some() {
                eprintln!("hex-standalone: takes no arguments");
                usage();
                std::process::exit(1);
            }
            let root = versions::workspace_root();
            if let Err(e) = hex_standalone::run(&root) {
                eprintln!("hex-standalone: {e}");
                std::process::exit(1);
            }
        }
        Some("versions-diff") => {
            let ver = args.next();
            if args.next().is_some() {
                eprintln!("versions-diff: too many arguments");
                usage();
                std::process::exit(1);
            }
            let root = versions::workspace_root();
            if let Err(e) = versions_diff::run_versions_diff(&root, ver.as_deref()) {
                eprintln!("versions-diff: {e}");
                std::process::exit(1);
            }
        }
        Some("lsp") => {
            let root = versions::workspace_root();
            let rest: Vec<String> = args.collect();
            if let Err(e) = lsp::run(&root, &rest) {
                eprintln!("lsp: {e}");
                std::process::exit(1);
            }
        }
        Some("benchmarks") => {
            let rest: Vec<String> = args.collect();
            let root = versions::workspace_root();
            if let Err(e) = benchmarks::run(&root, &rest) {
                eprintln!("benchmarks: {e}");
                usage();
                std::process::exit(1);
            }
        }
        Some("coverage") => {
            let rest: Vec<String> = args.collect();
            let root = versions::workspace_root();
            if let Err(e) = coverage::run(&root, &rest) {
                eprintln!("coverage: {e}");
                usage();
                std::process::exit(1);
            }
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
