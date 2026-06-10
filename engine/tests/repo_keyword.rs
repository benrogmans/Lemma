//! Repository (`repo …`) scope integration tests.
//!
//! Failures are intentional signal until semantics are fully implemented.

use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;
use std::sync::Arc;

fn path_source(path: &str) -> SourceType {
    SourceType::Path(Arc::new(std::path::PathBuf::from(path)))
}

#[test]
fn engine_run_resolves_unambiguous_spec_from_any_repo() {
    let mut engine = Engine::new();
    engine
        .load(
            "repo scoped\nspec tax\ndata rate: 21%\ndata price: 100\nrule total: price * (1 + rate)",
            SourceType::Volatile,
        )
        .expect("parse and plan must succeed for named-repo specs");

    let now = DateTimeValue::now();
    let outcome = engine.run(None, "tax", Some(&now), HashMap::new(), false, None);
    assert!(
        outcome.is_err(),
        "run(None, name) targets workspace; spec in named repo should not be found"
    );
    let outcome = engine.run(
        Some("scoped"),
        "tax",
        Some(&now),
        HashMap::new(),
        false,
        None,
    );
    assert!(
        outcome.is_ok(),
        "run(Some(repo), name) should find spec in named repo"
    );
}

#[test]
fn engine_two_repositories_same_spec_name_both_loaded_planning_behavior() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo a
spec s
data x: 1
rule r: x

repo b
spec s
data x: 2
rule r: x"#,
            SourceType::Volatile,
        )
        .expect("duplicate names across repos should load");

    let now = DateTimeValue::now();
    let main_run = engine.run(None, "s", Some(&now), HashMap::new(), false, None);
    assert!(
        main_run.is_err(),
        "bare spec name `s` is not in main repository; run must not silently pick one repo"
    );
}

#[test]
fn engine_cross_repo_uses_from_workspace_qualifier_in_main_spec() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo prov
spec provider
data x: 1
rule val: x"#,
            path_source("prov.lemma"),
        )
        .expect("provider repo");

    engine
        .load(
            r#"spec consumer
uses dep: prov provider
rule out: dep.val"#,
            path_source("main.lemma"),
        )
        .expect("consumer in main");

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "consumer", Some(&now), HashMap::new(), false, None)
        .expect("consumer should resolve cross-repo uses");
    assert!(
        response.results.contains_key("out"),
        "expected rule `out` in response"
    );
}

#[test]
fn engine_temporal_versions_inside_named_repository_evaluate() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo t
spec v
data x: 1
rule r: x

spec v 2020-01-01
data x: 2
rule r: x"#,
            SourceType::Volatile,
        )
        .expect("temporal rows in named repo");

    let v_spec_count: usize = engine
        .list()
        .iter()
        .flat_map(|r| r.specs.iter())
        .filter(|ss| ss.name == "v")
        .map(|ss| ss.iter_specs().count())
        .sum();
    assert_eq!(
        v_spec_count, 2,
        "both temporal slices of `v` must appear in context"
    );
}

#[test]
fn engine_list_specs_includes_specs_declared_under_named_repo() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo z
spec one
data a: 1
rule x: a

spec two
data b: 2
rule y: b"#,
            SourceType::Volatile,
        )
        .unwrap();

    let repos = engine.list();
    let names: std::collections::HashSet<_> = repos
        .iter()
        .flat_map(|r| r.specs.iter())
        .map(|ss| ss.name.as_str())
        .collect();
    assert!(
        names.contains("one") && names.contains("two"),
        "list must enumerate specs from named repositories: got {:?}",
        names
    );
}

#[test]
fn engine_schema_resolves_unambiguous_spec_from_named_repo() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo z
spec listed
data n: number -> minimum 0
rule r: n"#,
            SourceType::Volatile,
        )
        .unwrap();

    let now = DateTimeValue::now();
    let sch = engine.schema(Some("z"), "listed", Some(&now));
    assert!(
        sch.is_ok(),
        "schema(Some(repo), name) should find spec in named repo"
    );
    let sch_none = engine.schema(None, "listed", Some(&now));
    assert!(
        sch_none.is_err(),
        "schema(None, name) targets workspace; spec in named repo should not be found"
    );
}

#[test]
fn engine_dependency_bundle_with_repo_keyword_loads() {
    let mut engine = Engine::new();
    engine
        .load_batch(
            HashMap::from([(
                SourceType::Volatile,
                "repo @pkg/lib\nspec s\ndata x: 1\nrule r: x".to_string(),
            )]),
            Some("@pkg/lib"),
        )
        .expect("dependency bundles with `repo` keyword should load");
    let specs = engine.list();
    assert!(
        specs
            .iter()
            .flat_map(|r| r.specs.iter())
            .any(|ss| ss.name == "s"),
        "spec `s` must be loaded from dependency bundle"
    );
}

#[test]
fn anonymous_dependency_bundle_does_not_collide_with_workspace_main() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec workspace_only\ndata a: 1",
            path_source("workspace.lemma"),
        )
        .expect("workspace (main) must load");

    engine
        .load_batch(
            HashMap::from([(
                SourceType::Volatile,
                "spec dep_spec\ndata b: 2\nrule r: b".to_string(),
            )]),
            Some("@lemma/std"),
        )
        .expect("dependency without `repo` uses dependency id as repository name; no (main) clash");

    let repos = engine.list();
    let dep_repo = repos.iter().find(|r| {
        r.repository.name.as_deref() == Some("@lemma/std")
            && r.repository.dependency.as_deref() == Some("@lemma/std")
    });
    assert!(
        dep_repo.is_some(),
        "expected specs from anonymous dep file under repo @lemma/std, got: {:?}",
        repos
            .iter()
            .map(|r| (r.repository.name.clone(), r.repository.dependency.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn engine_empty_repo_section_then_specs_still_loads() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo empty

repo real
spec s
data x: 1
rule r: x"#,
            SourceType::Volatile,
        )
        .expect("empty repo section should not invalidate file");

    let s_count: usize = engine
        .list()
        .iter()
        .flat_map(|r| r.specs.iter())
        .filter(|ss| ss.name == "s")
        .count();
    assert_eq!(
        s_count, 1,
        "single spec `s` must remain visible after an empty `repo` section"
    );
}

#[test]
fn engine_bare_file_then_repo_file_coexist() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec main_only\ndata q: 1\nrule z: q",
            path_source("m.lemma"),
        )
        .unwrap();
    engine
        .load(
            r#"repo r
spec named_only
data q: 2
rule z: q"#,
            path_source("r.lemma"),
        )
        .unwrap();

    let now = DateTimeValue::now();
    assert!(engine
        .run(None, "main_only", Some(&now), HashMap::new(), false, None,)
        .is_ok());
    assert!(
        engine
            .run(
                None,
                "named_only",
                Some(&now),
                HashMap::new(),
                false,
                Some(&["z".to_string()]),
            )
            .is_err(),
        "run(None, name) targets workspace; spec in named repo `r` should not be found"
    );
    assert!(
        engine
            .run(
                Some("r"),
                "named_only",
                Some(&now),
                HashMap::new(),
                false,
                None,
            )
            .is_ok(),
        "run(Some(repo), name) should find spec in named repo"
    );
}

#[test]
fn engine_duplicate_spec_same_name_different_repositories_allowed() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo r1
spec dup
data x: 1
rule m: x"#,
            path_source("a.lemma"),
        )
        .unwrap();
    engine
        .load(
            r#"repo r2
spec dup
data x: 2
rule m: x"#,
            path_source("b.lemma"),
        )
        .unwrap();

    let dup_count: usize = engine
        .list()
        .iter()
        .flat_map(|r| r.specs.iter())
        .filter(|ss| ss.name == "dup")
        .map(|ss| ss.iter_specs().count())
        .sum();
    assert_eq!(dup_count, 2);
}

#[test]
fn engine_duplicate_spec_same_repository_across_second_load_errors() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo shared
spec dup
data x: 1
rule m: x"#,
            path_source("one.lemma"),
        )
        .unwrap();
    let second = engine.load(
        r#"repo shared
spec dup
data x: 9
rule m: x"#,
        path_source("two.lemma"),
    );
    assert!(
        second.is_err(),
        "duplicate (repository, name, effective) must surface as load error"
    );
}

/// Bare `uses` resolves against the consumer spec's owning repository only — same as qualifying `repo`.
#[test]
fn engine_bare_uses_in_named_repo_errors_when_target_only_in_main_repo() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"spec foundation
data k: 10
rule v: k"#,
            path_source("base.lemma"),
        )
        .unwrap();
    let layered = engine.load(
        r#"repo ext
spec layered
uses u: foundation
rule w: u.v"#,
        path_source("layer.lemma"),
    );
    assert!(
        layered.is_err(),
        "bare `uses foundation` from `repo ext` must not find `foundation` in main repo"
    );
}

#[test]
fn engine_bare_uses_in_named_repo_succeeds_when_target_in_same_repo() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo ext
spec foundation
data k: 10
rule v: k

spec layered
uses u: foundation
rule w: u.v"#,
            path_source("bundle.lemma"),
        )
        .expect("bare uses within same named repo must resolve");
}

// ── Dependency isolation tests ────────────────────────────────────

#[test]
fn dependency_cannot_merge_with_other_dependency() {
    let mut engine = Engine::new();
    engine
        .load_batch(
            HashMap::from([(
                SourceType::Volatile,
                "repo utils\nspec shared\ndata x: 1".to_string(),
            )]),
            Some("@dep/a"),
        )
        .expect("first dep loads");

    let result = engine.load_batch(
        HashMap::from([(
            SourceType::Volatile,
            "repo utils\nspec other\ndata y: 2".to_string(),
        )]),
        Some("@dep/b"),
    );
    assert!(
        result.is_err(),
        "two different dependencies declaring same repo name must conflict"
    );
    let msg = result
        .unwrap_err()
        .errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        msg.contains("utils") && msg.contains("@dep/a"),
        "error should mention repo name and first dependency, got: {msg}"
    );
}

#[test]
fn dependency_can_have_multiple_repos() {
    let mut engine = Engine::new();
    engine
        .load_batch(
            HashMap::from([(
                SourceType::Volatile,
                "repo @pkg/core\nspec core_spec\ndata x: 1\n\nrepo @pkg/utils\nspec util_spec\ndata y: 2"
                    .to_string(),
            )]),
            Some("@pkg/bundle"),
        )
        .expect("single dependency with multiple repos should load");

    let specs = engine.list();
    assert!(specs
        .iter()
        .flat_map(|r| r.specs.iter())
        .any(|ss| ss.name == "core_spec"));
    assert!(specs
        .iter()
        .flat_map(|r| r.specs.iter())
        .any(|ss| ss.name == "util_spec"));
}

#[test]
fn workspace_files_can_share_repo() {
    let mut engine = Engine::new();
    engine
        .load(
            "repo billing\nspec invoices\ndata x: 1",
            path_source("a.lemma"),
        )
        .expect("first workspace file");
    engine
        .load(
            "repo billing\nspec payments\ndata y: 2",
            path_source("b.lemma"),
        )
        .expect("second workspace file merges into same repo");

    let repos = engine.list();
    let billing_repos: Vec<_> = repos
        .iter()
        .filter(|r| r.repository.name.as_deref() == Some("billing"))
        .collect();
    assert_eq!(
        billing_repos.len(),
        1,
        "both workspace files should merge into one billing repo, got repos: {:?}",
        repos
            .iter()
            .map(|r| r.repository.name.as_deref())
            .collect::<Vec<_>>()
    );
    let billing_repo = billing_repos[0];
    assert_eq!(
        billing_repo.specs.len(),
        2,
        "billing repo should have two specs"
    );
    assert!(billing_repo.specs.iter().any(|ss| ss.name == "invoices"));
    assert!(billing_repo.specs.iter().any(|ss| ss.name == "payments"));
}

#[test]
fn dependency_and_workspace_different_names_coexist() {
    let mut engine = Engine::new();
    engine
        .load(
            "repo billing\nspec finance_spec\ndata x: 1",
            path_source("local.lemma"),
        )
        .expect("workspace load");
    engine
        .load_batch(
            HashMap::from([(
                SourceType::Volatile,
                "repo @jack/finance\nspec finance_spec\ndata y: 2".to_string(),
            )]),
            Some("@jack/finance"),
        )
        .expect("dependency with different repo name should coexist");

    let repos = engine.list();
    assert!(
        repos
            .iter()
            .any(|r| r.repository.name.as_deref() == Some("billing")),
        "workspace finance_spec should live in billing repo"
    );
    assert!(
        repos
            .iter()
            .any(|r| r.repository.name.as_deref() == Some("@jack/finance")),
        "dependency finance_spec should live in @jack/finance repo"
    );
    assert!(
        repos
            .iter()
            .flat_map(|r| r.specs.iter())
            .any(|ss| ss.name == "finance_spec"),
        "finance_spec should be loaded from workspace or dependency"
    );
    let finance_spec_count = repos
        .iter()
        .flat_map(|r| r.specs.iter())
        .filter(|ss| ss.name == "finance_spec")
        .count();
    assert_eq!(
        finance_spec_count, 2,
        "should have finance_spec in both billing (workspace) and @jack/finance (dependency)"
    );
}
