use lemma::Error;
use rustler::{Encoder, Env, Term};

pub fn encode_error<'a>(env: Env<'a>, err: &Error) -> Term<'a> {
    let mut map = rustler::types::map::map_new(env);
    map = map
        .map_put(
            rustler::Atom::from_str(env, "message").unwrap().encode(env),
            err.message().encode(env),
        )
        .unwrap();

    if let Some(location) = err.location() {
        let mut loc_map = rustler::types::map::map_new(env);
        loc_map = loc_map
            .map_put(
                rustler::Atom::from_str(env, "file").unwrap().encode(env),
                location.attribute.as_str().encode(env),
            )
            .unwrap();
        loc_map = loc_map
            .map_put(
                rustler::Atom::from_str(env, "line").unwrap().encode(env),
                location.span.line.encode(env),
            )
            .unwrap();
        loc_map = loc_map
            .map_put(
                rustler::Atom::from_str(env, "column").unwrap().encode(env),
                location.span.col.encode(env),
            )
            .unwrap();
        map = map
            .map_put(
                rustler::Atom::from_str(env, "location")
                    .unwrap()
                    .encode(env),
                loc_map,
            )
            .unwrap();
    }

    if let Some(suggestion) = err.suggestion() {
        map = map
            .map_put(
                rustler::Atom::from_str(env, "suggestion")
                    .unwrap()
                    .encode(env),
                suggestion.encode(env),
            )
            .unwrap();
    }

    map
}

pub fn encode_errors<'a>(env: Env<'a>, errors: &[Error]) -> Term<'a> {
    let terms: Vec<Term<'a>> = errors.iter().map(|e| encode_error(env, e)).collect();
    terms.encode(env)
}
