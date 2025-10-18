#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;

fuzz_target!(|depth: u8| {
    let mut engine = Engine::new();
    
    let depth = (depth as usize % 50) + 1;
    
    let mut expr = String::from("1");
    for _ in 0..depth {
        expr = format!("({} + 1)", expr);
    }
    
    let code = format!(r#"
doc fuzz_nested
fact x = 1
rule deeply_nested = {}
"#, expr);
    
    let _ = engine.add_lemma_code(&code, "fuzz_nested");
});
