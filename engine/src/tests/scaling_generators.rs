//! Test-only source generators for scaling gate tests.
//!
//! Each generator produces a Lemma source string parameterised by one dimension.
//! Generators are used by both planning-side and evaluation-side gate tests, so they live
//! in a shared module rather than inside any single test block.

/// Produce a spec with one text declaration that has `count` option rows and one trivial rule.
pub(crate) fn options_per_data_declaration(count: usize) -> String {
    let mut source = String::from("spec scale_test\n\ndata code: text\n");
    for i in 0..count {
        source.push_str(&format!("  -> option \"{}\"\n", nth_code(i)));
    }
    source.push_str("\nrule output: code\n");
    source
}

/// Produce a spec with one text declaration and `count` unless arms all testing the same path.
pub(crate) fn unless_arms_on_shared_path(count: usize) -> String {
    let mut source =
        String::from("spec scale_test\n\ndata code: text\n\nrule output: veto \"no match\"\n");
    for i in 0..count {
        let code = nth_code(i);
        source.push_str(&format!("  unless code is \"{code}\" then \"{code}\"\n"));
    }
    source
}

/// Produce a spec with `count` text declarations and `count` unless arms each testing a distinct path.
pub(crate) fn unless_arms_on_distinct_paths(count: usize) -> String {
    let mut source = String::from("spec scale_test\n\n");
    for i in 0..count {
        source.push_str(&format!("data code_{i}: text\n"));
    }
    source.push_str("\nrule output: veto \"no match\"\n");
    for i in 0..count {
        let code = nth_code(i);
        source.push_str(&format!(
            "  unless code_{i} is \"{code}\" then \"{code}\"\n"
        ));
    }
    source
}

/// Produce a spec with `length` text declarations and one unless arm whose condition is a
/// `length`-way and-chain (one clause per declaration).
pub(crate) fn conjunction_chain(length: usize) -> String {
    let mut source = String::from("spec scale_test\n\n");
    for i in 0..length {
        source.push_str(&format!("data code_{i}: text\n"));
    }
    source.push_str("\nrule output: veto \"no match\"\n  unless ");
    for i in 0..length {
        if i > 0 {
            source.push_str(" and ");
        }
        source.push_str(&format!("code_{i} is \"X\""));
    }
    source.push_str(" then \"hit\"\n");
    source
}

/// Produce `count` temporal versions of a single spec name, starting undated (Origin)
/// and then at 2001-01-01, 2002-01-01, … up to the `count`-th version.
pub(crate) fn temporal_versions_of_one_spec(count: usize) -> String {
    assert!(
        count >= 1,
        "temporal_versions_of_one_spec requires at least one version"
    );
    let mut source = String::from("spec scale_test\n\ndata code: text\n\nrule output: code\n");
    for i in 1..count {
        let year = 2000 + i as u32;
        source.push_str(&format!(
            "\nspec scale_test {year}-01-01\n\ndata code: text\n\nrule output: code\n"
        ));
    }
    source
}

/// Produce `count` independent specs each at a distinct yearly effective date
/// (2001-01-01 through 2000+count-01-01), no shared dependencies.
pub(crate) fn specs_at_distinct_effective_dates(count: usize) -> String {
    let mut source = String::new();
    for i in 0..count {
        let year = 2001 + i as u32;
        source.push_str(&format!(
            "spec scale_test_{i} {year}-01-01\n\ndata code: text\n\nrule output: code\n\n"
        ));
    }
    source
}

/// Same specs as `specs_at_distinct_effective_dates` but all undated (Origin).
/// Used as a control: the undated variant must also yield exactly `count` slices.
pub(crate) fn specs_all_undated(count: usize) -> String {
    let mut source = String::new();
    for i in 0..count {
        source.push_str(&format!(
            "spec scale_test_{i}\n\ndata code: text\n\nrule output: code\n\n"
        ));
    }
    source
}

/// Three-letter uppercase code for the `n`-th distinct value (AAA, AAB, …, ZZZ).
/// Supports up to 26³ = 17 576 distinct values.
fn nth_code(n: usize) -> String {
    let a = (n / (26 * 26)) % 26;
    let b = (n / 26) % 26;
    let c = n % 26;
    format!(
        "{}{}{}",
        (b'A' + a as u8) as char,
        (b'A' + b as u8) as char,
        (b'A' + c as u8) as char
    )
}
