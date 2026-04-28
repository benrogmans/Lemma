#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec fuzz_test
data test_value: {}
"#, s);
        let _ = engine.load(&code, lemma::SourceType::Labeled("fuzz_literal"));
    }
});
