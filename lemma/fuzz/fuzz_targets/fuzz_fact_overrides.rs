#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        
        let code = r#"
doc fuzz_test
fact x = [number]
rule doubled = x * 2
"#;
        
        if engine.add_lemma_code(code, "fuzz_override").is_ok() {
            if let Ok(facts) = lemma::parse_facts(&[s]) {
                let _ = engine.evaluate("fuzz_test", None, Some(facts));
            }
        }
    }
});
