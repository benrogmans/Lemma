use std::fs;
use std::path::Path;

fn main() {
    let guide_dir = Path::new("../engine/documentation/guide");
    let output_path = Path::new("documentation/llms.txt");

    let mut entries = fs::read_dir(guide_dir)
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
        let fragment = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("BUG: failed to read {}: {}", path.display(), e));
        content.push_str(&fragment);
    }

    fs::write(output_path, content)
        .unwrap_or_else(|e| panic!("BUG: failed to write {}: {}", output_path.display(), e));

    println!("cargo:rerun-if-changed=../engine/documentation/guide");
}
