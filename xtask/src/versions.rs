//! Bump and verify the workspace release version across Rust, Elixir, docs, and VS Code.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Paths and expectations shared by [`versions_bump`] and [`versions_verify`].
mod tracked {
    pub const WORKSPACE_CARGO: &str = "Cargo.toml";

    pub const PATH_DEP_MANIFESTS: &[&str] = &[
        "cli/Cargo.toml",
        "openapi/Cargo.toml",
        "engine/lsp/Cargo.toml",
    ];

    pub const HEX_MIX: &str = "engine/packages/hex/mix.exs";
    pub const ENGINE_README: &str = "engine/README.md";
    pub const VSCODE_PACKAGE_JSON: &str = "engine/lsp/editors/vscode/package.json";
}

fn dep_pin_needle(v: &str) -> String {
    format!(r#"version = "={v}""#)
}

fn mix_needle(v: &str) -> String {
    format!(r#"@version "{v}""#)
}

fn readme_needle(v: &str) -> String {
    format!(r#"lemma-engine = "{v}""#)
}

/// Paths relative to workspace root that must carry the same release version as `[workspace.package]`.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate must live under workspace root")
        .to_path_buf()
}

pub fn read_workspace_version(root: &Path) -> Result<String, String> {
    let cargo = root.join(tracked::WORKSPACE_CARGO);
    let content = fs::read_to_string(&cargo).map_err(|e| format!("{}: {e}", cargo.display()))?;
    parse_workspace_version(&content)
}

fn parse_workspace_version(content: &str) -> Result<String, String> {
    let mut in_workspace_package = false;
    for line in content.lines() {
        let t = line.trim();
        if t == "[workspace.package]" {
            in_workspace_package = true;
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') && in_workspace_package {
            break;
        }
        if !in_workspace_package {
            continue;
        }
        if let Some(rest) = t.strip_prefix("version") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                if let Some(inner) = rest.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                    return Ok(inner.to_string());
                }
            }
        }
    }
    Err("no version in [workspace.package] in Cargo.toml".into())
}

fn replace_workspace_package_version(
    content: &str,
    old: &str,
    new: &str,
) -> Result<String, String> {
    let old_line = format!(r#"version = "{old}""#);
    let new_line = format!(r#"version = "{new}""#);
    let mut in_workspace_package = false;
    let mut replaced = false;
    let mut out = String::new();
    for line in content.lines() {
        let t = line.trim();
        if t == "[workspace.package]" {
            in_workspace_package = true;
        } else if t.starts_with('[') && t.ends_with(']') && in_workspace_package {
            in_workspace_package = false;
        }
        if in_workspace_package && line.trim() == old_line {
            out.push_str(&new_line);
            replaced = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !replaced {
        return Err(format!(
            "root Cargo.toml: expected `{old_line}` inside [workspace.package]"
        ));
    }
    Ok(out)
}

fn replace_dep_pins(content: &str, old: &str, new: &str) -> String {
    let from = dep_pin_needle(old);
    let to = dep_pin_needle(new);
    content.replace(&from, &to)
}

fn replace_mix_version(content: &str, old: &str, new: &str) -> String {
    let from = mix_needle(old);
    let to = mix_needle(new);
    content.replace(&from, &to)
}

fn replace_readme_engine_line(content: &str, old: &str, new: &str) -> String {
    let from = readme_needle(old);
    let to = readme_needle(new);
    content.replace(&from, &to)
}

fn bump_package_json_version(path: &Path, old: &str, new: &str) -> Result<(), String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let from = format!(r#""version": "{old}""#);
    if !raw.contains(&from) {
        return Err(format!(
            "{}: expected top-level `\"version\": \"{old}\"`",
            path.display()
        ));
    }
    let to = format!(r#""version": "{new}""#);
    let updated = raw.replacen(&from, &to, 1);
    fs::write(path, updated).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

/// `cargo bump <new>` — set workspace release to `new` everywhere and refresh lockfile metadata.
pub fn versions_bump(root: &Path, new: &str) -> Result<(), String> {
    semver::Version::parse(new).map_err(|e| format!("invalid semver {new:?}: {e}"))?;
    let old = read_workspace_version(root)?;
    if old == new {
        return Err(format!("already at version {new}"));
    }

    let root_cargo = root.join(tracked::WORKSPACE_CARGO);
    let raw =
        fs::read_to_string(&root_cargo).map_err(|e| format!("{}: {e}", root_cargo.display()))?;
    let updated = replace_workspace_package_version(&raw, &old, new)?;
    fs::write(&root_cargo, updated).map_err(|e| format!("{}: {e}", root_cargo.display()))?;

    for rel in tracked::PATH_DEP_MANIFESTS {
        let p = root.join(rel);
        let c = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let c2 = replace_dep_pins(&c, &old, new);
        if c2 == c {
            return Err(format!(
                "{}: expected `{}` dependency pins",
                p.display(),
                dep_pin_needle(&old)
            ));
        }
        fs::write(&p, c2).map_err(|e| format!("{}: {e}", p.display()))?;
    }

    let mix = root.join(tracked::HEX_MIX);
    let mix_raw = fs::read_to_string(&mix).map_err(|e| format!("{}: {e}", mix.display()))?;
    let mix2 = replace_mix_version(&mix_raw, &old, new);
    if mix2 == mix_raw {
        return Err(format!(
            "{}: expected `{}`",
            mix.display(),
            mix_needle(&old)
        ));
    }
    fs::write(&mix, mix2).map_err(|e| format!("{}: {e}", mix.display()))?;

    let readme = root.join(tracked::ENGINE_README);
    let rm = fs::read_to_string(&readme).map_err(|e| format!("{}: {e}", readme.display()))?;
    let rm2 = replace_readme_engine_line(&rm, &old, new);
    if rm2 == rm {
        return Err(format!(
            "{}: expected `{}`",
            readme.display(),
            readme_needle(&old)
        ));
    }
    fs::write(&readme, rm2).map_err(|e| format!("{}: {e}", readme.display()))?;

    let pkg = root.join(tracked::VSCODE_PACKAGE_JSON);
    bump_package_json_version(&pkg, &old, new)?;

    run_cargo_metadata(root)?;
    Ok(())
}

fn run_cargo_metadata(root: &Path) -> Result<(), String> {
    eprintln!("xtask: cargo metadata (refresh lockfile)");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let st = Command::new(&cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("failed to run {cargo} metadata: {e}"))?;
    if !st.success() {
        return Err("cargo metadata failed".into());
    }
    Ok(())
}

/// `cargo verify` — ensure every tracked location matches `[workspace.package] version`.
pub fn versions_verify(root: &Path) -> Result<(), String> {
    let v = read_workspace_version(root)?;
    let mut errs: Vec<String> = Vec::new();

    let root_cargo = root.join(tracked::WORKSPACE_CARGO);
    match fs::read_to_string(&root_cargo) {
        Ok(s) => {
            if parse_workspace_version(&s).as_ref() != Ok(&v) {
                errs.push(format!(
                    "{}: [workspace.package] version does not match canonical {v}",
                    root_cargo.display()
                ));
            }
        }
        Err(e) => errs.push(format!("{}: {e}", root_cargo.display())),
    }

    for rel in tracked::PATH_DEP_MANIFESTS {
        let p = root.join(rel);
        match fs::read_to_string(&p) {
            Ok(s) => {
                let needle = dep_pin_needle(&v);
                if !s.contains(&needle) {
                    errs.push(format!(
                        "{}: expected path deps to contain `{needle}`",
                        p.display()
                    ));
                }
            }
            Err(e) => errs.push(format!("{}: {e}", p.display())),
        }
    }

    let mix = root.join(tracked::HEX_MIX);
    match fs::read_to_string(&mix) {
        Ok(s) => {
            let needle = mix_needle(&v);
            if !s.contains(&needle) {
                errs.push(format!("{}: expected `{needle}`", mix.display()));
            }
        }
        Err(e) => errs.push(format!("{}: {e}", mix.display())),
    }

    let readme = root.join(tracked::ENGINE_README);
    match fs::read_to_string(&readme) {
        Ok(s) => {
            let needle = readme_needle(&v);
            if !s.contains(&needle) {
                errs.push(format!(
                    "{}: expected `{needle}` in Quick start example",
                    readme.display()
                ));
            }
        }
        Err(e) => errs.push(format!("{}: {e}", readme.display())),
    }

    let pkg = root.join(tracked::VSCODE_PACKAGE_JSON);
    match fs::read_to_string(&pkg) {
        Ok(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(j) => match j.get("version").and_then(|x| x.as_str()) {
                Some(pv) if pv == v => {}
                Some(pv) => errs.push(format!(
                    "{}: version is {pv:?}, expected {v:?}",
                    pkg.display()
                )),
                None => errs.push(format!("{}: missing top-level \"version\"", pkg.display())),
            },
            Err(e) => errs.push(format!("{}: {e}", pkg.display())),
        },
        Err(e) => errs.push(format!("{}: {e}", pkg.display())),
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_workspace_version_finds_package_version() {
        let t = r#"
[workspace]
members = []

[workspace.package]
version = "0.8.4"
edition = "2021"
"#;
        assert_eq!(parse_workspace_version(t).unwrap(), "0.8.4");
    }

    #[test]
    fn replace_workspace_package_version_replaces() {
        let t = r#"[workspace.package]
version = "0.8.4"
"#;
        let out = replace_workspace_package_version(t, "0.8.4", "0.8.5").unwrap();
        assert!(out.contains("version = \"0.8.5\""));
        assert!(!out.contains("0.8.4"));
    }

    #[test]
    fn replace_dep_pins_replaces() {
        let s = r#"lemma = { version = "=0.8.4", path = ".." }"#;
        let out = replace_dep_pins(s, "0.8.4", "0.8.5");
        assert!(out.contains("=0.8.5"));
    }

    #[test]
    fn needles_align_with_bump_replace_substrings() {
        let v = "1.2.3";
        assert_eq!(dep_pin_needle(v), r#"version = "=1.2.3""#);
        assert_eq!(mix_needle(v), r#"@version "1.2.3""#);
        assert_eq!(readme_needle(v), r#"lemma-engine = "1.2.3""#);
    }

    #[test]
    fn tracked_paths_cover_bump_targets() {
        assert!(!tracked::PATH_DEP_MANIFESTS.is_empty());
        assert!(tracked::PATH_DEP_MANIFESTS.contains(&"cli/Cargo.toml"));
    }
}
