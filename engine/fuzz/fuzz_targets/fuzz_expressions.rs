#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        
        let code = format!(r#"
doc fuzz_test
fact x = 100
fact y = 50
rule test_expr = {}
"#, s);
        
        let _ = engine.add_lemma_code(&code, "fuzz_expr");
    }
});
