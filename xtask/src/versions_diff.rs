//! `git diff` / `git log` between `cli-v*` release tags.
//!
//! Runs `git fetch --tags` first so local tag refs match the remote (e.g. CI-created release tags)
//! before resolving tags.
//!
//! **No version argument:** `git diff` / `git diff --stat` compare the latest `cli-v*` tag to the
//! **working tree** (including uncommitted changes). `git log` is still `tag..HEAD` (commits only).
//!
//! **`versions-diff <semver>`:** compares the previous `cli-v*` tag to `cli-v{semver}` on history
//! (two commits; no working tree).

use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use semver::Version;

/// `cli-v` + semver, e.g. `cli-v0.8.4`.
pub(crate) fn parse_cli_v_version(tag: &str) -> Option<Version> {
    tag.strip_prefix("cli-v")?.parse().ok()
}

fn git_fetch_tags(root: &Path) -> Result<(), String> {
    let o = Command::new("git")
        .args(["fetch", "--tags", "--quiet", "-f"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("failed to run git fetch --tags: {e}"))?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        let err = err.trim();
        if err.is_empty() {
            return Err("git fetch --tags failed".into());
        }
        return Err(format!("git fetch --tags failed: {err}"));
    }
    Ok(())
}

fn git_output(args: &[&str], cwd: &Path) -> Result<Vec<u8>, String> {
    let o = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        let err = err.trim();
        if err.is_empty() {
            return Err("git command failed".into());
        }
        return Err(err.to_string());
    }
    Ok(o.stdout)
}

fn list_cli_v_tags_sorted(root: &Path) -> Result<Vec<(Version, String)>, String> {
    let out = git_output(&["tag", "-l", "cli-v*"], root)?;
    let mut tags: Vec<(Version, String)> = String::from_utf8_lossy(&out)
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let ver = parse_cli_v_version(line)?;
            Some((ver, line.to_string()))
        })
        .collect();
    tags.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(tags)
}

fn write_stdout(bytes: &[u8]) -> Result<(), String> {
    io::stdout().write_all(bytes).map_err(|e| e.to_string())?;
    Ok(())
}

/// True if index or working tree differs from `HEAD` (uncommitted or unstaged changes).
fn worktree_differs_from_head(root: &Path) -> bool {
    Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .current_dir(root)
        .status()
        .map(|s| !s.success())
        .unwrap_or(false)
}

/// Print `git diff --stat`, `git log`, then `git diff` for the resolved range or tag → worktree.
pub fn run_versions_diff(root: &Path, version_arg: Option<&str>) -> Result<(), String> {
    git_fetch_tags(root)?;
    let tags = list_cli_v_tags_sorted(root)?;
    if tags.is_empty() {
        return Err("no cli-v* tags found".into());
    }

    match version_arg {
        None => {
            let tag = tags.last().expect("non-empty").1.clone();
            if worktree_differs_from_head(root) {
                eprintln!(
                    "versions-diff: working tree differs from HEAD; diff includes uncommitted changes; log is tag..HEAD only."
                );
            }

            let stat = git_output(&["diff", "--stat", &tag], root)?;
            write_stdout(&stat)?;
            write_stdout(b"\n")?;

            let log_range = format!("{tag}..HEAD");
            let log = git_output(&["log", "--no-merges", &log_range, "--oneline"], root)?;
            write_stdout(&log)?;
            write_stdout(b"\n")?;

            let diff = git_output(&["diff", &tag], root)?;
            write_stdout(&diff)?;
        }
        Some(v) => {
            let want = Version::parse(v).map_err(|e| format!("invalid semver {v:?}: {e}"))?;
            let idx = tags
                .iter()
                .position(|(ver, _)| ver == &want)
                .ok_or_else(|| format!("no tag cli-v{want}"))?;
            if idx == 0 {
                return Err(format!("no previous cli-v* tag before cli-v{want}"));
            }
            let prev = tags[idx - 1].1.clone();
            let end = tags[idx].1.clone();
            let range = format!("{prev}..{end}");

            let stat = git_output(&["diff", "--stat", &range], root)?;
            write_stdout(&stat)?;
            write_stdout(b"\n")?;

            let log = git_output(&["log", "--no-merges", &range, "--oneline"], root)?;
            write_stdout(&log)?;
            write_stdout(b"\n")?;

            let diff = git_output(&["diff", &range], root)?;
            write_stdout(&diff)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_v_version_accepts() {
        assert_eq!(
            parse_cli_v_version("cli-v0.8.4"),
            Some(Version::new(0, 8, 4))
        );
        assert_eq!(parse_cli_v_version("v0.8.4"), None);
    }
}
