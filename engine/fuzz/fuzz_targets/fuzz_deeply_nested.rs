#![no_main]

use libfuzzer_sys::fuzz_target;
use lemma::Engine;

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
    let _ = engine.load(&code, lemma::SourceType::Labeled("fuzz_nested"));
});
