//! Library surface for workspace tooling (LSP) that shares CLI filesystem helpers.
pub mod deps {
    pub use lemma::deps::*;
}

pub mod install;
pub mod workspace;
