#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let depth = (data[0] as usize % 6) + 1;
    let variant = data[1] % 4;

    let mut expr = String::from("1");
    for i in 0..depth {
        expr = match variant {
            0 => format!("({} + 1)", expr),
            1 => format!("({} * 2)", expr),
            2 => format!("({} - {})", expr, i),
            _ => format!("({})", expr),
        };
    }

    let code = format!(
        "spec fuzz_nested\nfact x: 1\nrule deeply_nested: {}\n",
        expr
    );

    let mut engine = Engine::new();
    let files: HashMap<String, String> =
        std::iter::once(("fuzz_nested".to_string(), code)).collect();
    let _ = engine.add_lemma_files(files);
});
