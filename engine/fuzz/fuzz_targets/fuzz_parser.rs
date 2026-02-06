#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = Engine::new();
        
        let _ = engine.add_lemma_code(s, "fuzz_input");
    }
});
