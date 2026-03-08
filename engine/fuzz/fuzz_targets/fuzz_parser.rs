#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        let files: HashMap<String, String> =
            std::iter::once(("fuzz_input".to_string(), s.to_string())).collect();
        let _ = engine.add_lemma_files(files);
    }
});
