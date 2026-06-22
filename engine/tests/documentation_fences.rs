//! Load and run every ```lemma fence in repo markdown and text files.

use lemma::{
    parse, resolve_registry_references, Context, DateTimeValue, Engine, LemmaBase, ResourceLimits,
    SourceType, EMBEDDED_STDLIB_REPOSITORY,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules"];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn should_skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

fn collect_markdown_and_text_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
            panic!("read_dir {}: {e}", dir.display());
        }) {
            let entry = entry.unwrap_or_else(|e| panic!("dir entry in {}: {e}", dir.display()));
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if should_skip_dir(name) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let is_md = path.extension().is_some_and(|e| e == "md");
            let is_txt = path.extension().is_some_and(|e| e == "txt");
            if is_md || is_txt {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

struct LemmaFence {
    /// 1-based line number of the opening `` ```lemma `` marker in the source file.
    line: usize,
    content: String,
}

fn extract_lemma_fences(content: &str) -> Vec<LemmaFence> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut fence_line = 0usize;
    let mut current = String::new();
    for (line_index, line) in content.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();
        if trimmed == "```lemma" {
            in_fence = true;
            fence_line = line_number;
            current.clear();
            continue;
        }
        if in_fence && trimmed == "```" {
            in_fence = false;
            let block = current.trim().to_string();
            if !block.is_empty() {
                blocks.push(LemmaFence {
                    line: fence_line,
                    content: block,
                });
            }
            continue;
        }
        if in_fence {
            current.push_str(line);
            current.push('\n');
        }
    }
    blocks
}

fn fence_label(path: &Path, line: usize) -> String {
    format!("{}:{line}", path.display())
}

fn specs_declared_in_fence(content: &str, source: &SourceType) -> Vec<(Option<String>, String)> {
    let limits = ResourceLimits::default();
    let parsed = parse(content, source.clone(), &limits).unwrap_or_else(|e| {
        panic!("parse fence source failed: {e}");
    });
    parsed
        .repositories
        .into_iter()
        .flat_map(|(repo, specs)| {
            let qualifier = repo.name.clone();
            specs
                .into_iter()
                .map(move |spec| (qualifier.clone(), spec.name))
        })
        .collect()
}

fn fence_needs_registry(content: &str) -> bool {
    content.contains('@')
}

async fn load_fence_engine(path: &Path, fence: &LemmaFence) -> Engine {
    let label = fence_label(path, fence.line);
    let workspace_source = SourceType::Path(Arc::new(PathBuf::from(&label)));
    let limits = ResourceLimits::default();

    let parsed = parse(&fence.content, workspace_source.clone(), &limits).unwrap_or_else(|e| {
        panic!("parse ```lemma fence at {label} failed: {e}");
    });

    let mut ctx = Context::new();
    for (parsed_repo, specs) in &parsed.repositories {
        for spec in specs {
            ctx.insert_spec(Arc::clone(parsed_repo), Arc::new(spec.clone()))
                .unwrap_or_else(|e| panic!("insert spec for fence at {label} failed: {e}"));
        }
    }

    let mut sources = HashMap::from([(workspace_source.clone(), fence.content.clone())]);

    if fence_needs_registry(&fence.content) {
        let registry = LemmaBase::test();
        resolve_registry_references(&mut ctx, &mut sources, &registry, &limits)
            .await
            .unwrap_or_else(|errs| {
                panic!(
                    "resolve registry for ```lemma fence at {label} failed: {}",
                    errs.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            });
    }

    let mut engine = Engine::new();
    let local: HashSet<_> = HashSet::from([workspace_source.clone()]);

    for (source_id, code) in &sources {
        if local.contains(source_id) {
            continue;
        }
        let dep_id = match source_id {
            SourceType::Registry(repo) => repo.name.as_deref(),
            _ => None,
        };
        engine
            .load_batch(HashMap::from([(source_id.clone(), code.clone())]), dep_id)
            .unwrap_or_else(|errs| {
                panic!(
                    "load registry dep for ```lemma fence at {label} failed: {}",
                    errs.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            });
    }

    engine
        .load(fence.content.clone(), workspace_source)
        .unwrap_or_else(|errs| {
            panic!(
                "load ```lemma fence at {label} failed: {}",
                errs.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        });

    engine
}

async fn run_fence(path: &Path, fence: &LemmaFence) {
    let label = fence_label(path, fence.line);
    let workspace_source = SourceType::Path(Arc::new(PathBuf::from(&label)));
    let target_specs = specs_declared_in_fence(&fence.content, &workspace_source);
    let engine = load_fence_engine(path, fence).await;
    let now = DateTimeValue::now();
    for (repo, spec_name) in target_specs {
        if repo.as_deref() == Some(EMBEDDED_STDLIB_REPOSITORY) {
            continue;
        }
        engine
            .run(
                repo.as_deref(),
                &spec_name,
                Some(&now),
                HashMap::new(),
                false,
                None,
            )
            .unwrap_or_else(|e| {
                panic!("run ```lemma fence at {label} spec {spec_name} failed: {e}")
            });
    }
}

#[tokio::test]
async fn all_lemma_fences_load_and_run() {
    let root = workspace_root();
    let files = collect_markdown_and_text_files(&root);
    let mut ran = 0usize;
    for path in files {
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for fence in extract_lemma_fences(&content) {
            run_fence(&path, &fence).await;
            ran += 1;
        }
    }
    assert!(ran > 0, "must run at least one ```lemma fence");
}
