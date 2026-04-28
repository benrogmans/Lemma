#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        let code = format!(r#"
spec fuzz_test
data x: 100
data y: 50
rule test_expr: {}
"#, s);
        let _ = engine.load(&code, lemma::SourceType::Labeled("fuzz_expr"));
    }
});
