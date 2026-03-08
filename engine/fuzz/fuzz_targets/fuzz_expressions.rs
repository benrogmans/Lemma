#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        
        let code = format!(r#"
spec fuzz_test
fact x: 100
fact y: 50
rule test_expr: {}
"#, s);
        
        let files: HashMap<String, String> =
            std::iter::once(("fuzz_expr".to_string(), code)).collect();
        let _ = engine.add_lemma_files(files);
    }
});
