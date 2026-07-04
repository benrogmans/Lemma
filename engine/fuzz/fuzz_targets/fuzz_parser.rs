#![no_main]

use lemma::DateTimeValue;
use lemma::Engine;
use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        let loaded = engine.load(
            s,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("fuzz_input"))),
        );

        // Property: load Ok => every loaded spec must evaluate without panic.
        if loaded.is_ok() {
            let now = DateTimeValue::now();
            let spec_names: Vec<String> = engine
                .get_workspace()
                .specs
                .iter()
                .map(|ss| ss.name.clone())
                .collect();
            for name in spec_names {
                let _ = engine.run(None, &name, Some(&now), HashMap::new(), false, None);
            }
        }
    }
});
