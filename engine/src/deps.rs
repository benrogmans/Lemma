//! Dependency cache layout under `<workdir>/.deps/`, shared by CLI fetch and LSP.

use std::path::{Path, PathBuf};

#[must_use]
pub fn lemma_deps_dir(workdir: &Path) -> PathBuf {
    workdir.join(".deps")
}

/// Relative path under `.deps/` for a fetched registry bundle (`GET /@id.lemma` identity includes `@`).
#[must_use]
pub fn relative_dependency_cache_path(registry_attribute_display: &str) -> PathBuf {
    let last_slash_position = registry_attribute_display.rfind('/');
    let (directory_segment, leaf_segment) = match last_slash_position {
        Some(slice_index) => (
            &registry_attribute_display[..slice_index],
            &registry_attribute_display[slice_index + 1..],
        ),
        None => ("", registry_attribute_display),
    };
    let filename_with_suffix = format!("{}.lemma", leaf_segment);
    if directory_segment.is_empty() {
        PathBuf::from(filename_with_suffix)
    } else {
        PathBuf::from(directory_segment).join(filename_with_suffix)
    }
}

/// Derive registry dependency id from a `.deps/` `.lemma` file path (same string CLI `load_batch` passes as `dependency_id`).
#[must_use]
pub fn dependency_identifier_from_dependency_path(
    workdir: &Path,
    dependency_path: &Path,
) -> String {
    let deps_dir = lemma_deps_dir(workdir);
    let relative = dependency_path
        .strip_prefix(&deps_dir)
        .unwrap_or(dependency_path.as_ref());
    let without_ext = relative.with_extension("");
    without_ext.to_string_lossy().to_string()
}

/// Absolute path where `lemma fetch` writes the bundle for this registry qualifier.
#[must_use]
pub fn dependency_cache_file(workdir: &Path, registry_qualifier: &str) -> PathBuf {
    lemma_deps_dir(workdir).join(relative_dependency_cache_path(registry_qualifier))
}
