//! Shared registry install for CLI `install` and MCP `install`.

use crate::deps::{lemma_deps_dir, relative_dependency_cache_path};
use lemma::Engine;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use walkdir::WalkDir;

fn registry_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Runtime::new().expect("BUG: failed to create registry install runtime")
    })
}

/// Result of a successful single-id registry install.
#[derive(Debug)]
pub enum InstallOutcome {
    Written {
        relative_path: PathBuf,
        source: String,
    },
    AlreadyUpToDate {
        relative_path: PathBuf,
        source: String,
    },
}

/// Failure installing a registry dependency (I/O, registry, conflict, or plan).
pub enum InstallError {
    Message(String),
    Plan(lemma::Errors),
}

impl std::fmt::Debug for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(m) => write!(f, "{m}"),
            Self::Plan(errors) => write!(
                f,
                "Planning installed dependency failed ({} error(s))",
                errors.errors.len()
            ),
        }
    }
}

impl From<String> for InstallError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

/// Drive an async registry future on the process-wide install runtime.
pub fn block_on_registry<F: std::future::Future>(future: F) -> F::Output {
    registry_runtime().block_on(future)
}

/// Download, conflict-scan, plan-probe, and persist one registry dependency.
///
/// CLI semantics: identical content anywhere under `lemma_deps/` → already up to date;
/// any spec-name conflict → error unless `force` (then remove conflicting files).
pub fn install_registry_dependency(
    workdir: &Path,
    id: &str,
    force: bool,
    registry: &dyn lemma::Registry,
) -> Result<InstallOutcome, InstallError> {
    let downloaded = block_on_registry(download_dependency(registry, id))?;

    let scan = scan_lemma_deps(workdir, id, &downloaded.source, &downloaded.spec_names)
        .map_err(|e| InstallError::Message(e.to_string()))?;

    if scan.any_identical_content() {
        return Ok(InstallOutcome::AlreadyUpToDate {
            relative_path: scan.destination_relative,
            source: downloaded.source,
        });
    }

    if !scan.conflict_paths.is_empty() {
        if !force {
            let limits = lemma::ResourceLimits::default();
            let path = &scan.conflict_paths[0];
            let existing_content =
                fs::read_to_string(path).map_err(|e| InstallError::Message(e.to_string()))?;
            let existing_specs = lemma::parse(
                &existing_content,
                lemma::SourceType::Path(Arc::new(path.to_path_buf())),
                &limits,
            )
            .map_err(|e| InstallError::Message(e.to_string()))?
            .into_flattened_specs();
            let conflict: Vec<&str> = existing_specs
                .iter()
                .filter(|s| downloaded.spec_names.contains(&s.name))
                .map(|s| s.name.as_str())
                .collect();
            return Err(InstallError::Message(format!(
                "Dependency containing spec(s) {} already exists in {}.\n\
                 Content has changed on the registry. Re-run with --force to overwrite.",
                conflict.join(", "),
                path.display()
            )));
        }
        for path in &scan.conflict_paths {
            fs::remove_file(path).map_err(|e| InstallError::Message(e.to_string()))?;
        }
    }

    let mut probe = Engine::new();
    if let Err(load_err) = probe.load([(
        lemma::SourceType::Dependency(id.to_string()),
        downloaded.source.clone(),
    )]) {
        return Err(InstallError::Plan(load_err));
    }

    atomic_write(&scan.destination_absolute, &downloaded.source)
        .map_err(|e| InstallError::Message(e.to_string()))?;

    Ok(InstallOutcome::Written {
        relative_path: scan.destination_relative,
        source: downloaded.source,
    })
}

/// Downloaded registry bundle ready for conflict scan / install.
pub struct DownloadedDependency {
    pub source: String,
    pub spec_names: HashSet<String>,
}

/// Download and parse a registry dependency. Registry owns id reachability.
pub async fn download_dependency(
    registry: &dyn lemma::Registry,
    dependency: &str,
) -> Result<DownloadedDependency, String> {
    let bundle = registry
        .get(dependency)
        .await
        .map_err(|e| format!("Registry error for {dependency}: {}", e.message))?;

    let limits = lemma::ResourceLimits::default();
    let new_specs = lemma::parse(
        &bundle.source,
        lemma::SourceType::Dependency(dependency.to_string()),
        &limits,
    )
    .map_err(|e| format!("Registry returned unparseable dependency: {}", e.message()))?
    .into_flattened_specs();
    let spec_names = new_specs.iter().map(|s| s.name.clone()).collect();

    Ok(DownloadedDependency {
        source: bundle.source,
        spec_names,
    })
}

/// Result of walking `lemma_deps/` against a candidate dependency source.
pub struct LemmaDepsScan {
    pub destination_absolute: PathBuf,
    pub destination_relative: PathBuf,
    /// Paths under `lemma_deps/` whose file content equals the candidate source.
    pub paths_with_identical_content: Vec<PathBuf>,
    /// Paths that define overlapping spec names with the candidate.
    pub conflict_paths: Vec<PathBuf>,
}

impl LemmaDepsScan {
    #[must_use]
    pub fn any_identical_content(&self) -> bool {
        !self.paths_with_identical_content.is_empty()
    }
}

/// Walk `lemma_deps/` for identical content and spec-name conflicts.
/// Unparseable or unreadable `.lemma` files are errors (noises are errors).
pub fn scan_lemma_deps(
    workdir: &Path,
    dependency: &str,
    source_text: &str,
    new_spec_names: &HashSet<String>,
) -> io::Result<LemmaDepsScan> {
    let deps_dir = lemma_deps_dir(workdir);
    let destination_relative = relative_dependency_cache_path(dependency);
    let destination_absolute = deps_dir.join(&destination_relative);
    let limits = lemma::ResourceLimits::default();

    let mut paths_with_identical_content = Vec::new();
    let mut conflict_paths = Vec::new();

    if deps_dir.exists() {
        for entry in WalkDir::new(&deps_dir) {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("lemma") {
                continue;
            }
            let path = entry.path();
            let existing_content = fs::read_to_string(path)?;
            if existing_content == source_text {
                paths_with_identical_content.push(path.to_path_buf());
            }
            let existing_specs = lemma::parse(
                &existing_content,
                lemma::SourceType::Path(Arc::new(path.to_path_buf())),
                &limits,
            )
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unparseable lemma_deps file {}: {}", path.display(), e),
                )
            })?
            .into_flattened_specs();
            let has_conflict = existing_specs
                .iter()
                .any(|s| new_spec_names.contains(&s.name));
            if has_conflict {
                conflict_paths.push(path.to_path_buf());
            }
        }
    }

    Ok(LemmaDepsScan {
        destination_absolute,
        destination_relative,
        paths_with_identical_content,
        conflict_paths,
    })
}

/// Atomic write: temp file in same directory, fsync, rename.
pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "persist path must include a file name",
        )
    })?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp", file_name.to_string_lossy()));
    match (|| -> io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })() {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn atomic_write_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.lemma");
        atomic_write(&path, "spec x\ndata a: 1\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "spec x\ndata a: 1\n");
        assert!(
            !dir.path().join(".out.lemma.tmp").exists(),
            "temp file must be cleaned up"
        );
    }

    #[test]
    fn scan_lemma_deps_rejects_unparseable_cache_file() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("lemma_deps").join("@junk");
        fs::create_dir_all(&junk).unwrap();
        fs::write(junk.join("corrupt.lemma"), "{{{not valid lemma").unwrap();

        let source = "spec alpha2\ndata code: text\n";
        let mut names = HashSet::new();
        names.insert("alpha2".to_string());

        let result = scan_lemma_deps(dir.path(), "@iso/countries", source, &names);
        assert!(
            result.is_err(),
            "unparseable lemma_deps .lemma must hard-error"
        );
    }

    #[test]
    fn install_writes_fixture_dependency() {
        let dir = tempfile::tempdir().unwrap();
        let registry = lemma::LemmaBase::test();
        let outcome =
            install_registry_dependency(dir.path(), "@iso/countries", false, &registry).unwrap();
        match outcome {
            InstallOutcome::Written { relative_path, .. } => {
                assert_eq!(relative_path, PathBuf::from("@iso").join("countries.lemma"));
            }
            InstallOutcome::AlreadyUpToDate { .. } => panic!("expected Written"),
        }
        assert!(dir
            .path()
            .join("lemma_deps")
            .join("@iso")
            .join("countries.lemma")
            .exists());
    }

    #[test]
    fn install_identical_content_elsewhere_is_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let registry = lemma::LemmaBase::test();
        let first =
            install_registry_dependency(dir.path(), "@iso/countries", false, &registry).unwrap();
        let source = match first {
            InstallOutcome::Written { source, .. } => source,
            InstallOutcome::AlreadyUpToDate { source, .. } => source,
        };
        let other = dir
            .path()
            .join("lemma_deps")
            .join("@other")
            .join("copy.lemma");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        // Move dest aside so only the foreign copy remains with identical content.
        let dest = dir
            .path()
            .join("lemma_deps")
            .join("@iso")
            .join("countries.lemma");
        fs::rename(&dest, &other).unwrap();

        let second =
            install_registry_dependency(dir.path(), "@iso/countries", false, &registry).unwrap();
        assert!(
            matches!(second, InstallOutcome::AlreadyUpToDate { .. }),
            "identical content under lemma_deps must be up to date"
        );
        assert!(!dest.exists(), "up-to-date skip must not write destination");
        assert_eq!(fs::read_to_string(&other).unwrap(), source);
    }

    #[test]
    fn install_non_at_id_reaches_registry() {
        let dir = tempfile::tempdir().unwrap();
        let registry = lemma::LemmaBase::test();
        let err = install_registry_dependency(dir.path(), "not-a-registry-id", false, &registry)
            .expect_err("missing id must fail via registry");
        let err = err.to_string();
        assert!(
            err.contains("Registry error")
                || err.contains("must start with '@'")
                || err.contains("not-a-registry-id"),
            "error must come from registry authority, got: {err}"
        );
    }
}
