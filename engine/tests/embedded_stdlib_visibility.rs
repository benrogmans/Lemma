use lemma::engine::EMBEDDED_STDLIB_REPOSITORY;
use lemma::{Engine, ErrorKind};

#[test]
fn list_includes_embedded_lemma_repository_with_si() {
    let engine = Engine::new();
    let repos = engine.list();
    let lemma = repos
        .iter()
        .find(|r| r.repository.name.as_deref() == Some(EMBEDDED_STDLIB_REPOSITORY))
        .expect("embedded lemma repository must appear in list");
    assert!(
        lemma
            .specs
            .iter()
            .flat_map(|ss| ss.iter_specs())
            .any(|s| s.name == "si"),
        "expected spec si in lemma repo"
    );
}

#[test]
fn format_repository_lemma_contains_si_duration() {
    let engine = Engine::new();
    let text = engine
        .format_repository(EMBEDDED_STDLIB_REPOSITORY)
        .expect("format embedded lemma repo");
    assert!(
        text.contains("repo lemma"),
        "expected repo header, got:\n{text}"
    );
    assert!(
        text.contains("spec si"),
        "expected spec si in formatted output, got:\n{text}"
    );
    assert!(
        text.contains("duration") && text.contains("trait duration"),
        "expected duration typedef in formatted output, got:\n{text}"
    );
}

#[test]
fn format_repository_unknown_qualifier_errors() {
    let engine = Engine::new();
    let err = engine
        .format_repository("workspace")
        .expect_err("workspace is not a repository qualifier");
    assert!(
        err.kind() == ErrorKind::Request,
        "expected request error, got: {err:?}"
    );
}
