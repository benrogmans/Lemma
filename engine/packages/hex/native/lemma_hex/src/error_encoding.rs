use lemma::Error;
use lemma::ErrorKind;
use rustler::{Encoder, Env, NifResult, Term};

fn error_kind_string(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Parsing => "parsing",
        ErrorKind::Validation => "validation",
        ErrorKind::Inversion => "inversion",
        ErrorKind::Registry => "registry",
        ErrorKind::MissingRepository => "missing_repository",
        ErrorKind::Request => "request",
        ErrorKind::ResourceLimit => "resource_limit",
    }
}

pub fn encode_error<'a>(env: Env<'a>, err: &Error) -> NifResult<Term<'a>> {
    let mut map = rustler::types::map::map_new(env);
    map = map.map_put(
        rustler::Atom::from_str(env, "kind")?.encode(env),
        error_kind_string(err.kind()).encode(env),
    )?;
    map = map.map_put(
        rustler::Atom::from_str(env, "message")?.encode(env),
        err.message().encode(env),
    )?;

    if let Some(location) = err.location() {
        let mut loc_map = rustler::types::map::map_new(env);
        loc_map = loc_map.map_put(
            rustler::Atom::from_str(env, "file")?.encode(env),
            location.source_type.to_string().encode(env),
        )?;
        loc_map = loc_map.map_put(
            rustler::Atom::from_str(env, "line")?.encode(env),
            location.span.line.encode(env),
        )?;
        loc_map = loc_map.map_put(
            rustler::Atom::from_str(env, "column")?.encode(env),
            location.span.col.encode(env),
        )?;
        map = map.map_put(
            rustler::Atom::from_str(env, "location")?.encode(env),
            loc_map,
        )?;
    }

    if let Some(suggestion) = err.suggestion() {
        map = map.map_put(
            rustler::Atom::from_str(env, "suggestion")?.encode(env),
            suggestion.encode(env),
        )?;
    }

    if let Some(repository) = err.repository() {
        map = map.map_put(
            rustler::Atom::from_str(env, "repository")?.encode(env),
            repository.encode(env),
        )?;
    }

    Ok(map)
}

pub fn encode_errors<'a>(env: Env<'a>, errors: &[Error]) -> NifResult<Term<'a>> {
    let mut terms: Vec<Term<'a>> = Vec::with_capacity(errors.len());
    for e in errors {
        terms.push(encode_error(env, e)?);
    }
    Ok(terms.encode(env))
}
