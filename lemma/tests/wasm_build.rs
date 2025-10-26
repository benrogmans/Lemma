use std::process::Command;

#[test]
#[ignore] // Run with: cargo test --ignored or cargo test wasm_build -- --ignored
fn test_wasm_build_and_test() {
    // This test ensures the WASM build and tests work correctly
    // It's ignored by default because it requires Node.js and is slower

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
fn test_wasm_scripts_exist() {
    // This test always runs and just checks that the WASM scripts exist
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let build_script = std::path::Path::new(manifest_dir).join("wasm/build.js");
    let test_script = std::path::Path::new(manifest_dir).join("wasm/test.js");

    assert!(
        build_script.exists(),
        "WASM build script not found at: {}",
        build_script.display()
    );

    assert!(
        test_script.exists(),
        "WASM test script not found at: {}",
        test_script.display()
    );
}
