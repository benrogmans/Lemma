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

        if engine
            .load(code, lemma::LoadSource::Labeled("fuzz_binding"))
            .is_ok()
        {
            let mut facts = HashMap::new();
            facts.insert("x".to_string(), s.to_string());
            let now = DateTimeValue::now();
            let _ = engine.run("fuzz_test", Some(&now), facts);
        }
    }
});
