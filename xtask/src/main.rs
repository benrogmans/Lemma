mod versions;
mod versions_diff;

use std::path::Path;
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

/// True if the working tree has staged, unstaged, or untracked paths under `pathspec` (git pathspec, repo-relative).
fn git_has_changes_under(repo_root: &Path, pathspec: &str) -> bool {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["status", "--porcelain", pathspec])
        .output()
        .unwrap_or_else(|e| panic!("failed to run git status --porcelain {pathspec}: {e}"));
    if !output.status.success() {
        panic!(
            "git status --porcelain {pathspec} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    !output.stdout.trim_ascii().is_empty()
}

const HEX_PACKAGE_DIR: &str = "engine/packages/hex";
const VSCODE_EXTENSION_DIR: &str = "engine/lsp/editors/vscode";

fn run_mix_precommit() {
    let root = versions::workspace_root();
    if !git_has_changes_under(&root, HEX_PACKAGE_DIR) {
        eprintln!("xtask: mix precommit (skipped, no changes under {HEX_PACKAGE_DIR})");
        return;
    }
    let hex_dir = root.join(HEX_PACKAGE_DIR);
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
    let root = versions::workspace_root();
    if !git_has_changes_under(&root, VSCODE_EXTENSION_DIR) {
        eprintln!("xtask: vscode npm precommit (skipped, no changes under {VSCODE_EXTENSION_DIR})");
        return;
    }
    let dir = root.join(VSCODE_EXTENSION_DIR);
    let status = Command::new("npm")
        .current_dir(&dir)
        .args(["run", "precommit"])
        .status()
        .unwrap_or_else(|e| panic!("failed to run npm run precommit in {}: {e}", dir.display()));
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
    eprintln!("xtask: deny");
    run(&["deny", "check", "--config", ".cargo/deny.toml"]);
    eprintln!("xtask: done");
}

fn usage() {
    eprintln!(
        "usage:\n  cargo precommit | cargo run -p xtask\n  cargo verify   | cargo run -p xtask -- versions-verify\n  cargo bump <version> | cargo run -p xtask -- versions-bump <version>\n  cargo changelog | cargo run -p xtask -- versions-diff [semver]"
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
