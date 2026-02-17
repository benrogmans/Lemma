#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        
        let code = r#"
doc fuzz_test
fact x = [number]
rule doubled = x * 2
"#;
        
        let files: HashMap<String, String> =
            std::iter::once(("fuzz_binding".to_string(), code.to_string())).collect();
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        if rt.block_on(engine.add_lemma_files(files)).is_ok() {
            if let Ok(facts) = lemma::parse_facts(&[s]) {
                let _ = engine.evaluate("fuzz_test", None, Some(facts));
            }
        }
    }
});
