//! Cost of one editor keystroke: re-applying a single source file into an engine
//! that already holds a loaded workspace.
//!
//! Planning is scoped to the spec sets an edit can change, so the two axes that
//! matter are corpus width (spec sets the edit cannot reach) and dependency depth
//! (spec sets the edit does reach). Width must not affect the measurement; depth
//! must. A regression back to whole-context replanning shows up as `unrelated_width`
//! growing with the corpus size.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use lemma::{Engine, SourceType};
use std::path::PathBuf;
use std::sync::Arc;

const CORPUS_WIDTHS: [usize; 3] = [16, 64, 256];
const CHAIN_DEPTHS: [usize; 3] = [2, 8, 32];

/// The spec set every keystroke benchmark edits.
const EDITED_SPEC: &str = "invoice_total";
const EDITED_PATH: &str = "engine/benches/keystroke/invoice_total.lemma";

fn source_path(label: &str) -> SourceType {
    SourceType::Path(Arc::new(PathBuf::from(label)))
}

/// The edited spec's source with `threshold` as its volume discount cut-off, so each
/// iteration submits genuinely different text.
fn invoice_total_source(threshold: u32) -> String {
    format!(
        "spec {EDITED_SPEC}\n\
         data quantity: number\n\
         data unit_price: number\n\
         rule net_amount: quantity * unit_price\n\
         rule volume_discount: 0%\n\
         \x20 unless quantity > {threshold} then 5%\n\
         rule discount_amount: net_amount * volume_discount\n\
         rule total: net_amount - discount_amount\n"
    )
}

/// A workspace grown wide: `width` VAT spec sets that share no dependency edges with
/// each other or with the edited spec, each carrying three temporal versions.
fn wide_workspace(width: usize) -> Engine {
    let mut engine = Engine::new();
    let mut sources: Vec<(SourceType, String)> = Vec::with_capacity(width * 3 + 1);
    for index in 0..width {
        for (effective, rate) in [("", "21%"), (" 2025-07-01", "19%"), (" 2026-01-01", "20%")] {
            let label = effective.trim();
            sources.push((
                source_path(&format!(
                    "engine/benches/keystroke/vat_rate_{index}_{}.lemma",
                    if label.is_empty() { "origin" } else { label }
                )),
                format!(
                    "spec vat_rate_{index}{effective}\n\
                     data net_amount: number\n\
                     rule vat_due: net_amount * {rate}\n\
                     rule gross_amount: net_amount + vat_due\n"
                ),
            ));
        }
    }
    sources.push((source_path(EDITED_PATH), invoice_total_source(10)));
    engine
        .load(sources)
        .expect("BUG: wide keystroke corpus must load");
    engine
}

/// A dependency chain of `depth` spec sets ending at the edited spec, so editing the
/// chain's base invalidates every link above it.
fn dependency_chain(depth: usize) -> Engine {
    assert!(depth >= 2, "BUG: a chain needs a base and a consumer");
    let mut engine = Engine::new();
    let mut sources: Vec<(SourceType, String)> = Vec::with_capacity(depth);
    sources.push((
        source_path("engine/benches/keystroke/chain_base.lemma"),
        "spec chain_base\n\
         data quantity: number\n\
         rule handling_fee: quantity * 2\n"
            .to_string(),
    ));
    for link in 1..depth - 1 {
        let previous = if link == 1 {
            "chain_base".to_string()
        } else {
            format!("chain_link_{}", link - 1)
        };
        sources.push((
            source_path(&format!("engine/benches/keystroke/chain_link_{link}.lemma")),
            format!(
                "spec chain_link_{link}\n\
                 uses upstream: {previous}\n\
                 rule handling_fee: upstream.handling_fee + 1\n"
            ),
        ));
    }
    let last = if depth == 2 {
        "chain_base".to_string()
    } else {
        format!("chain_link_{}", depth - 2)
    };
    sources.push((
        source_path(EDITED_PATH),
        format!(
            "spec {EDITED_SPEC}\n\
             uses upstream: {last}\n\
             data quantity: number\n\
             data unit_price: number\n\
             rule net_amount: quantity * unit_price\n\
             rule total: net_amount + upstream.handling_fee\n"
        ),
    ));
    engine
        .load(sources)
        .expect("BUG: keystroke dependency chain must load");
    engine
}

/// Base source of the chain, with `fee` as its per-unit handling fee.
fn chain_base_source(fee: u32) -> String {
    format!(
        "spec chain_base\n\
         data quantity: number\n\
         rule handling_fee: quantity * {fee}\n"
    )
}

/// Re-applying one file whose spec set has no consumers, over a growing corpus of
/// spec sets that the edit cannot reach.
fn bench_unrelated_width(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("keystroke/unrelated_width");
    for width in CORPUS_WIDTHS {
        let mut engine = wide_workspace(width);
        let mut threshold = 10_u32;
        group.bench_with_input(BenchmarkId::from_parameter(width), &width, |bencher, _| {
            bencher.iter(|| {
                threshold += 1;
                engine
                    .update(
                        None,
                        invoice_total_source(threshold),
                        source_path(EDITED_PATH),
                    )
                    .expect("BUG: keystroke edit must apply");
            });
        });
    }
    group.finish();
}

/// Re-applying the base of a dependency chain, which must replan every link above it.
fn bench_dependency_depth(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("keystroke/dependency_depth");
    for depth in CHAIN_DEPTHS {
        let mut engine = dependency_chain(depth);
        let mut fee = 2_u32;
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |bencher, _| {
            bencher.iter(|| {
                fee += 1;
                engine
                    .update(
                        None,
                        chain_base_source(fee),
                        source_path("engine/benches/keystroke/chain_base.lemma"),
                    )
                    .expect("BUG: chain base edit must apply");
            });
        });
    }
    group.finish();
}

/// Re-applying a file whose text did not change: the save-with-no-edit cycle.
fn bench_reload_unchanged(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("keystroke/reload_unchanged");
    for width in CORPUS_WIDTHS {
        let mut engine = wide_workspace(width);
        group.bench_with_input(BenchmarkId::from_parameter(width), &width, |bencher, _| {
            bencher.iter(|| {
                engine
                    .update(None, invoice_total_source(10), source_path(EDITED_PATH))
                    .expect("BUG: unchanged reload must apply");
            });
        });
    }
    group.finish();
}

const ISO_COUNTRIES: &str = include_str!("specs/iso_countries.lemma");
const ISO_PATH: &str = "engine/benches/specs/iso_countries.lemma";

/// Cold load of the large temporal unless-chain fixture: parse + plan every slice.
fn bench_plan_cold_iso(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("plan_cold/iso_countries");
    group.sample_size(20);
    group.bench_function("load", |bencher| {
        bencher.iter(|| {
            let mut engine = Engine::new();
            engine
                .load([(source_path(ISO_PATH), ISO_COUNTRIES.to_string())])
                .expect("BUG: iso_countries fixture must load");
            std::hint::black_box(engine);
        });
    });
    group.finish();
}

/// First version of alpha2 only: fold-eligible (`code is "XX"`) vs a same-size
/// chain whose conditions are not constant-keyed (`code is code`), so the
/// OrderedDispatch fold declines. Fold must not plan slower than the decline path.
fn alpha2_2011_source() -> &'static str {
    const MARKER: &str = "spec alpha2 2010-01-01";
    let end = ISO_COUNTRIES
        .find(MARKER)
        .expect("BUG: iso_countries must contain the 2010 alpha2 version boundary");
    &ISO_COUNTRIES[..end]
}

fn bench_fold_vs_nonfold(criterion: &mut Criterion) {
    let foldable = alpha2_2011_source().to_string();
    // Same arm count and literal results; the last unless is `true` so the
    // OrderedDispatch scan declines after walking every prior comparison arm.
    // That is the fair decline path: full scan, no table build.
    let mut nonfoldable = foldable.clone();
    if let Some(last) = nonfoldable.rfind("\n  unless code is \"") {
        let after_unless = nonfoldable[last + 1..]
            .find('\n')
            .map(|i| last + 1 + i)
            .expect("BUG: unless line must be followed by then");
        nonfoldable.replace_range(last + 1..after_unless, "  unless true");
    } else {
        panic!("BUG: foldable alpha2 must contain an unless code is arm");
    }
    let mut group = criterion.benchmark_group("plan_cold/fold_vs_nonfold");
    group.sample_size(40);
    group.bench_function("foldable_is", |bencher| {
        bencher.iter(|| {
            let mut engine = Engine::new();
            engine
                .load([(
                    source_path("engine/benches/keystroke/foldable.lemma"),
                    foldable.clone(),
                )])
                .expect("BUG: foldable alpha2 must load");
            std::hint::black_box(engine);
        });
    });
    group.bench_function("nonfold_declined", |bencher| {
        bencher.iter(|| {
            let mut engine = Engine::new();
            engine
                .load([(
                    source_path("engine/benches/keystroke/nonfoldable.lemma"),
                    nonfoldable.clone(),
                )])
                .expect("BUG: nonfoldable alpha2 must load");
            std::hint::black_box(engine);
        });
    });
    group.finish();
}

fn iso_loaded_engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .load([(source_path(ISO_PATH), ISO_COUNTRIES.to_string())])
        .expect("BUG: iso_countries fixture must load");
    engine
}

/// Keystroke near the end of the iso buffer: earlier version spans stay equal, so
/// slice replan can reuse them.
fn bench_iso_keystroke_bottom(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("keystroke/iso_bottom");
    group.sample_size(20);
    let mut engine = iso_loaded_engine();
    let mut flip = false;
    group.bench_function("edit_tail", |bencher| {
        bencher.iter(|| {
            flip = !flip;
            let mut code = ISO_COUNTRIES.to_string();
            let needle = "then \"Zambia\"";
            let replacement = if flip { "then \"Zambia!\"" } else { needle };
            let start = code
                .rfind(needle)
                .expect("BUG: iso_countries must contain Zambia arm");
            code.replace_range(start..start + needle.len(), replacement);
            engine
                .update(None, code, source_path(ISO_PATH))
                .expect("BUG: iso bottom edit must apply");
        });
    });
    group.finish();
}

/// Keystroke near the start of the iso buffer: span shifts invalidate later
/// PartialEq skips across the rest of the file.
fn bench_iso_keystroke_top(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("keystroke/iso_top");
    group.sample_size(20);
    let mut engine = iso_loaded_engine();
    let mut flip = false;
    group.bench_function("edit_head", |bencher| {
        bencher.iter(|| {
            flip = !flip;
            let mut code = ISO_COUNTRIES.to_string();
            let needle = "SS: South Sudan";
            let replacement = if flip { "SS: South Sudan!" } else { needle };
            let start = code
                .find(needle)
                .expect("BUG: iso_countries must contain South Sudan docstring");
            code.replace_range(start..start + needle.len(), replacement);
            engine
                .update(None, code, source_path(ISO_PATH))
                .expect("BUG: iso top edit must apply");
        });
    });
    group.finish();
}

/// Identical bytes against a long-lived engine: PartialEq skip, no replan work.
fn bench_iso_identical_noop(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("keystroke/iso_identical");
    group.sample_size(20);
    let mut engine = iso_loaded_engine();
    group.bench_function("noop", |bencher| {
        bencher.iter(|| {
            engine
                .update(None, ISO_COUNTRIES.to_string(), source_path(ISO_PATH))
                .expect("BUG: identical iso update must apply");
        });
    });
    group.finish();
}

/// Drop one temporal version from the buffer: Path prune + whole-set replan.
fn bench_iso_prune_version(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("keystroke/iso_prune_version");
    group.sample_size(20);
    const DROP_MARKER: &str = "spec alpha2 2010-01-01";
    const NEXT_MARKER: &str = "spec alpha2 2007-01-01";
    let pruned: String = {
        let start = ISO_COUNTRIES
            .find(DROP_MARKER)
            .expect("BUG: iso_countries must contain 2010 alpha2");
        let end = ISO_COUNTRIES
            .find(NEXT_MARKER)
            .expect("BUG: iso_countries must contain 2007 alpha2");
        let mut s = String::with_capacity(ISO_COUNTRIES.len() - (end - start));
        s.push_str(&ISO_COUNTRIES[..start]);
        s.push_str(&ISO_COUNTRIES[end..]);
        s
    };
    group.bench_function("prune_2010", |bencher| {
        bencher.iter_batched(
            iso_loaded_engine,
            |mut engine| {
                engine
                    .update(None, pruned.clone(), source_path(ISO_PATH))
                    .expect("BUG: iso prune update must apply");
                std::hint::black_box(engine);
            },
            criterion::BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(
    group,
    bench_unrelated_width,
    bench_dependency_depth,
    bench_reload_unchanged,
    bench_plan_cold_iso,
    bench_fold_vs_nonfold,
    bench_iso_keystroke_bottom,
    bench_iso_keystroke_top,
    bench_iso_identical_noop,
    bench_iso_prune_version
);
criterion_main!(group);
