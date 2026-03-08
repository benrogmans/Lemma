#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;
use lemma::parsing::ast::DateTimeValue;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();

        let code = r#"
spec fuzz_test
fact x: [number]
rule doubled: x * 2
"#;

        let files: HashMap<String, String> =
            std::iter::once(("fuzz_binding".to_string(), code.to_string())).collect();
        if engine.add_lemma_files(files).is_ok() {
            if let Ok(facts) = lemma::parse_facts(&[s]) {
                let now = DateTimeValue::now();
                let _ = engine.evaluate("fuzz_test", None, &now, vec![], facts);
            }
        }
    }
});
