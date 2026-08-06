//! Dependency layout under `<workdir>/lemma_deps/`, shared by CLI install and LSP.

use std::path::{Path, PathBuf};

/// Workspace-relative directory name holding fetched registry bundles. Committed
/// to version control: there is no lock file, so the on-disk source is authoritative.
pub const LEMMA_DEPS_DIR_NAME: &str = "lemma_deps";

#[must_use]
pub fn lemma_deps_dir(workdir: &Path) -> PathBuf {
    workdir.join(LEMMA_DEPS_DIR_NAME)
}

/// Relative path under `lemma_deps/` for a fetched registry bundle (`GET /@id.lemma` identity includes `@`).
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

/// Derive registry dependency id from a `lemma_deps/` `.lemma` file path (same string CLI passes as `dependency_id`).
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

/// Absolute path where `lemma install` writes the bundle for this registry qualifier.
#[must_use]
pub fn dependency_cache_file(workdir: &Path, registry_qualifier: &str) -> PathBuf {
    lemma_deps_dir(workdir).join(relative_dependency_cache_path(registry_qualifier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lemma_deps_dir_uses_lemma_deps_segment() {
        let root = Path::new("/tmp/workspace");
        assert_eq!(lemma_deps_dir(root), root.join("lemma_deps"));
        assert_eq!(LEMMA_DEPS_DIR_NAME, "lemma_deps");
    }

    #[test]
    fn dependency_cache_file_resolves_under_lemma_deps() {
        let root = Path::new("/tmp/workspace");
        assert_eq!(
            dependency_cache_file(root, "@iso/countries"),
            root.join("lemma_deps").join("@iso").join("countries.lemma"),
        );
        assert_eq!(
            dependency_cache_file(root, "@org/project/bundle"),
            root.join("lemma_deps")
                .join("@org")
                .join("project")
                .join("bundle.lemma"),
        );
    }

    #[test]
    fn dependency_identifier_round_trips_through_lemma_deps() {
        let root = Path::new("/tmp/workspace");
        let dep_file = dependency_cache_file(root, "@iso/countries");
        assert_eq!(
            dependency_identifier_from_dependency_path(root, &dep_file),
            "@iso/countries",
        );
    }
}
