use std::process::Command;

#[test]
#[ignore]
fn test_wasm_build_and_test() {
    // This test ensures the WASM build and tests work correctly
    println!("Building WASM package...");

    // Build WASM package
    let build_status = Command::new("node")
        .arg("wasm/build.js")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to execute node wasm/build.js");

    assert!(
        build_status.success(),
        "WASM build failed with exit code: {:?}",
        build_status.code()
    );

    println!("Testing WASM package...");

    // Test WASM package
    let test_status = Command::new("node")
        .arg("wasm/test.js")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to execute node wasm/test.js");

    assert!(
        test_status.success(),
        "WASM tests failed with exit code: {:?}",
        test_status.code()
    );

    println!("✅ WASM build and tests passed!");
}

#[test]
#[ignore]
fn test_wasm_scripts_exist() {
    // This test always runs and just checks that the WASM scripts exist
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let wasm_dir = std::path::Path::new(manifest_dir).join("wasm");
    for name in [
        "build.js",
        "test.js",
        "lemma-entry.js",
        "lsp-entry.js",
        "lemma.d.ts",
        "lsp.d.ts",
    ] {
        let p = wasm_dir.join(name);
        assert!(p.exists(), "WASM file missing: {}", p.display());
    }
}
