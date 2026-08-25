//! Emits `cli/documentation/llms.txt` from `engine/documentation/guide/*.md`.
//!
//! `cargo run -p xtask -- llms` regenerates the file; regeneration must be a no-op
//! against the checked-in copy (enforced by `cli/tests/integrations/llms_txt.rs`).

use std::fs;
use std::path::{Path, PathBuf};

const GUIDE_REL_DIR: &str = "engine/documentation/guide";
const LLMS_REL_PATH: &str = "cli/documentation/llms.txt";

pub fn concat_guide_fragments(guide_dir: &Path) -> Result<String, String> {
    let mut entries = fs::read_dir(guide_dir)
        .map_err(|e| format!("read {}: {e}", guide_dir.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? == "md"
                && !path.file_stem()?.to_str()?.ends_with("_improved")
            {
                Some(path)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    entries.sort();
    let mut content = String::new();
    for (i, path) in entries.iter().enumerate() {
        if i > 0 {
            content.push_str("\n\n---\n\n");
        }
        let fragment =
            fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        content.push_str(&fragment);
    }
    Ok(content)
}

fn guide_dir(root: &Path) -> PathBuf {
    root.join(GUIDE_REL_DIR)
}

fn llms_path(root: &Path) -> PathBuf {
    root.join(LLMS_REL_PATH)
}

pub fn run(root: &Path) -> Result<(), String> {
    let content = concat_guide_fragments(&guide_dir(root))?;
    let path = llms_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    eprintln!("xtask: wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions;

    #[test]
    fn concat_guide_fragments_uses_separator() {
        let root = versions::workspace_root();
        let content = concat_guide_fragments(&guide_dir(&root)).expect("concat");
        assert!(content.contains("\n\n---\n\n"));
        assert!(content.contains("# Lemma"));
    }
}
