#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        
        let code = format!(r#"
doc fuzz_test
fact test_value = {}
"#, s);
        
        let files: HashMap<String, String> =
            std::iter::once(("fuzz_literal".to_string(), code)).collect();
        let _ = tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(engine.add_lemma_files(files));
    }
});
