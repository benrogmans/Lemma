//! Generated cli/documentation/llms.txt must match engine embedded guide.

use std::fs;
use std::path::PathBuf;

#[test]
fn llms_txt_matches_engine_embedded_full_guide() {
    let llms_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("documentation/llms.txt");
    let llms = fs::read_to_string(&llms_path)
        .unwrap_or_else(|e| panic!("BUG: read {}: {e}", llms_path.display()));
    assert_eq!(llms, lemma::documentation::GuideTopic::Full.section_text());
}
