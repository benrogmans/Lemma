//! Build the Lemma CLI and prepare the VS Code / Cursor extension from this workspace.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const VSCODE_EXTENSION_REL: &str = "engine/lsp/editors/vscode";

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn run_npm(vscode_dir: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("npm")
        .current_dir(vscode_dir)
        .args(args)
        .status()
        .map_err(|e| {
            format!(
                "failed to run npm {} in {}: {e}",
                args.join(" "),
                vscode_dir.display()
            )
        })?;
    if !status.success() {
        return Err(format!(
            "npm {} exited with status {:?}",
            args.join(" "),
            status.code()
        ));
    }
    Ok(())
}

fn build_release_lemma(root: &Path) -> Result<(), String> {
    let status = Command::new(cargo_bin())
        .current_dir(root)
        .args(["build", "--release", "-p", "lemma"])
        .status()
        .map_err(|e| format!("failed to run cargo build --release -p lemma: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cargo build --release -p lemma exited with status {:?}",
            status.code()
        ));
    }
    Ok(())
}

fn prepare_extension(root: &Path, vscode_dir: &Path) -> Result<(), String> {
    eprintln!("xtask: cargo build --release -p lemma");
    build_release_lemma(root)?;
    eprintln!("xtask: npm ci (vscode extension)");
    run_npm(vscode_dir, &["ci"])?;
    eprintln!("xtask: npm run compile (vscode extension)");
    run_npm(vscode_dir, &["run", "compile"])?;
    Ok(())
}

fn newest_vsix(vscode_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(vscode_dir).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension() else {
            continue;
        };
        let Some(ext) = ext.to_str() else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("vsix") {
            continue;
        }
        let modified = entry.metadata().ok()?.modified().ok()?;
        match &best {
            None => best = Some((modified, path)),
            Some((t, _)) if modified > *t => best = Some((modified, path)),
            _ => {}
        }
    }
    best.map(|(_, p)| p)
}

fn print_paths(root: &Path, vscode_dir: &Path) {
    let binary = root.join("target").join("release").join("lemma");
    eprintln!("xtask: lemma binary: {}", binary.display());
    eprintln!("xtask: vscode extension dir: {}", vscode_dir.display());
}

/// `rest`: optional subcommand (`vsix`, `prepare`); empty means default prepare (binary + extension compile).
pub fn run(root: &Path, rest: &[String]) -> Result<(), String> {
    let vscode_dir = root.join(VSCODE_EXTENSION_REL);

    match rest.first().map(|s| s.as_str()) {
        None => {
            prepare_extension(root, &vscode_dir)?;
            print_paths(root, &vscode_dir);
        }
        Some("prepare") => {
            if rest.len() != 1 {
                return Err(format!(
                    "`prepare` takes no extra arguments (got: {})",
                    rest[1..].join(" ")
                ));
            }
            prepare_extension(root, &vscode_dir)?;
            print_paths(root, &vscode_dir);
        }
        Some("vsix") => {
            if rest.len() != 1 {
                return Err(format!(
                    "extra arguments after vsix: {}",
                    rest[1..].join(" ")
                ));
            }
            prepare_extension(root, &vscode_dir)?;
            eprintln!("xtask: npm run package (vscode extension)");
            run_npm(&vscode_dir, &["run", "package"])?;
            match newest_vsix(&vscode_dir) {
                Some(p) => eprintln!("xtask: VSIX: {}", p.display()),
                None => eprintln!(
                    "xtask: npm run package finished; look for *.vsix under {}",
                    vscode_dir.display()
                ),
            }
            print_paths(root, &vscode_dir);
        }
        Some("-h" | "--help" | "help") => {
            if rest.len() != 1 {
                return Err(format!(
                    "unexpected arguments after help: {}",
                    rest[1..].join(" ")
                ));
            }
            eprintln!(
                "cargo lsp           — release-build `lemma` + npm ci && npm run compile in {}",
                vscode_dir.display()
            );
            eprintln!(
                "cargo lsp vsix      — same, then npm run package (install the .vsix in Cursor / VS Code)"
            );
        }
        Some(other) => {
            return Err(format!(
                "unknown subcommand {other:?}; try `cargo lsp --help`"
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_rejects_extra_arguments() {
        assert!(run(
            Path::new("/tmp"),
            &["prepare".to_string(), "nope".to_string()]
        )
        .unwrap_err()
        .contains("prepare"));
    }

    #[test]
    fn rejects_unknown_subcommand() {
        assert!(run(Path::new("/tmp"), &["not-a-command".to_string()])
            .unwrap_err()
            .contains("unknown"));
    }
}
