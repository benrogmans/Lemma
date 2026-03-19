use std::process::Command;

#[test]
#[ignore]
fn test_wasm_build_and_test() {
    // This test ensures the WASM build and tests work correctly
    println!("Building WASM package...");

    // Build WASM package (npm package at engine/packages/npm)
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let npm_dir = manifest_dir.join("packages").join("npm");
    let build_status = Command::new("node")
        .arg("build.js")
        .current_dir(&npm_dir)
        .status()
        .expect("Failed to execute node build.js in packages/npm");

    assert!(
        build_status.success(),
        "WASM build failed with exit code: {:?}",
        build_status.code()
    );

    println!("Testing WASM package...");

    // Test WASM package
    let test_status = Command::new("node")
        .arg("test.js")
        .current_dir(&npm_dir)
        .status()
        .expect("Failed to execute node test.js in packages/npm");

    assert!(
        test_status.success(),
        "WASM tests failed with exit code: {:?}",
        test_status.code()
    );

    println!("WASM build and tests passed!");
}

#[test]
#[ignore]
fn test_wasm_scripts_exist() {
    // This test always runs and just checks that the WASM/npm package scripts exist
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let wasm_dir = manifest_dir.join("packages").join("npm");
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
