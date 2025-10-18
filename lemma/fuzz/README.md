# Fuzzing Lemma Parser

This directory contains fuzz targets for testing the Lemma engine's robustness against malformed input.

## Setup

Fuzzing requires Rust nightly:

```bash
rustup install nightly
cargo install cargo-fuzz
```

## Running Fuzz Tests

### Quick Test (30 seconds)
```bash
cargo +nightly fuzz run fuzz_parser -- -max_total_time=30
```

### Full Fuzzing Session (1 hour)
```bash
cargo +nightly fuzz run fuzz_parser -- -max_total_time=3600
```

### Available Targets

1. **fuzz_parser** - Full document parsing with random input
2. **fuzz_expressions** - Expression parsing within valid document context
3. **fuzz_literals** - Literal value parsing (numbers, strings, units)
4. **fuzz_deeply_nested** - Nested expressions to test stack limits
5. **fuzz_fact_overrides** - Fact override parsing and evaluation

### Running Specific Targets

```bash
cargo +nightly fuzz run fuzz_expressions -- -max_total_time=60
cargo +nightly fuzz run fuzz_literals -- -max_total_time=60
cargo +nightly fuzz run fuzz_deeply_nested -- -max_total_time=60
cargo +nightly fuzz run fuzz_fact_overrides -- -max_total_time=60
```

### Reproducing Crashes

When a crash is found, it's saved to `artifacts/<target>/crash-*`:

```bash
cargo +nightly fuzz run fuzz_parser artifacts/fuzz_parser/crash-abc123
```

### Minimizing Test Cases

To find the smallest input that reproduces a crash:

```bash
cargo +nightly fuzz tmin fuzz_parser artifacts/fuzz_parser/crash-abc123
```

## Known Issues

### Memory Leaks

The fuzzer may report memory leaks from global initialization. These are typically **expected and harmless**:

- Global static storage for atom tables and arenas
- These are one-time allocations, not per-operation leaks

To run without leak detection:

```bash
ASAN_OPTIONS="detect_leaks=0" cargo +nightly fuzz run fuzz_parser
```

## Interpreting Results

- **Crashes (exit code 1)**: Parser panic or assertion failure - **needs fixing**
- **Timeouts**: Input causes excessive computation - review for DOS potential
- **Leaks (exit code 77)**: Usually from global initialization - **can ignore**

## Adding New Targets

1. Create `fuzz_targets/fuzz_your_target.rs`
2. Add a `[[bin]]` entry in `Cargo.toml`
3. Follow the pattern from existing targets

Example:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use lemma::LemmaEngine;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut engine = LemmaEngine::new();
        let _ = engine.add_lemma_code(s, "fuzz");
    }
});
```

## Continuous Fuzzing

For long-running fuzzing campaigns, use a corpus directory:

```bash
mkdir -p corpus/fuzz_parser
cargo +nightly fuzz run fuzz_parser corpus/fuzz_parser -- -max_total_time=86400
```

This will save interesting inputs and gradually improve coverage over time.
