//! Generated cli/documentation/llms.txt must match engine guide fragments.

use std::fs;
use std::path::PathBuf;

fn guide_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/documentation/guide")
}

fn concat_guide_fragments() -> String {
    let mut entries = fs::read_dir(guide_dir())
        .expect("BUG: engine/documentation/guide/ must exist")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? == "md" {
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
        content.push_str(
            &fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("BUG: read {}: {e}", path.display())),
        );
    }
    content
}

#[test]
fn llms_txt_matches_engine_guide_fragments() {
    let llms_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("documentation/llms.txt");
    let llms = fs::read_to_string(&llms_path)
        .unwrap_or_else(|e| panic!("BUG: read {}: {e}", llms_path.display()));
    assert_eq!(llms, concat_guide_fragments());
}

#[test]
fn llms_txt_matches_engine_embedded_full_guide() {
    let llms_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("documentation/llms.txt");
    let llms = fs::read_to_string(&llms_path)
        .unwrap_or_else(|e| panic!("BUG: read {}: {e}", llms_path.display()));
    assert_eq!(llms, lemma::documentation::GuideTopic::Full.section_text());
}
