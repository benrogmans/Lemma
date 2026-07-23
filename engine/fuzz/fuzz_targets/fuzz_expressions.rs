#![no_main]

use lemma::DateTimeValue;
use lemma::Engine;
use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        let code = format!(
            r#"
spec fuzz_test
data x: 100
data y: 50
rule test_expr: {}
"#,
            s
        );
        let loaded = engine.load([(lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("fuzz_expr"))), &code.to_string())]);

        // Property: load Ok => evaluation must not panic.
        if loaded.is_ok() {
            let now = DateTimeValue::now();
            let _ = engine.run(None, "fuzz_test", Some(&now), HashMap::new(), None, false);
        }
    }
});
