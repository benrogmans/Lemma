#![no_main]

use lemma::DateTimeValue;
use lemma::Engine;
use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();

        let code = r#"
spec fuzz_test
data x: number
rule doubled: x * 2
"#;

        engine
            .load([(lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "fuzz_binding",
                ))), code.to_string())])
            .expect("BUG: static fuzz spec must load");

        // Property: valid spec loaded => arbitrary data input must not panic.
        let mut data = HashMap::new();
        data.insert("x".to_string(), s.to_string());
        let now = DateTimeValue::now();
        let _ = engine.run(None, "fuzz_test", Some(&now), data, None, false);
    }
});
