/// Trait for resolving Registry identifiers to URLs.
///
/// The LSP uses this to construct clickable links for `@`-prefixed references
/// (e.g. `doc @user/workspace/somedoc` or `type money from @lemma/std/finance`).
///
/// Input to all methods is the identifier *without* the leading `@`
/// (e.g. "user/workspace/somedoc").
///
/// Implementations must be Send + Sync so they can be shared across async tasks.
pub trait Registry: Send + Sync {
    /// Map a Registry identifier to a human-facing URL for navigation.
    ///
    /// Returns `None` if no URL is available for this identifier.
    fn url_for_id(&self, identifier: &str) -> Option<String>;
}

/// Stub Registry implementation for the MVP.
///
/// Returns a placeholder URL constructed from a configurable base URL and the identifier.
/// This allows the LSP to provide clickable links from day one, even before
/// a real Registry backend exists.
pub struct StubRegistry {
    base_url: String,
}

impl StubRegistry {
    /// Create a new StubRegistry with a default base URL.
    pub fn new() -> Self {
        Self {
            base_url: "https://registry.lemma.dev".to_string(),
        }
    }
}

impl Registry for StubRegistry {
    fn url_for_id(&self, identifier: &str) -> Option<String> {
        Some(format!("{}/{}", self.base_url, identifier))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_registry_returns_url_for_any_identifier() {
        let registry = StubRegistry::new();
        let url = registry.url_for_id("user/workspace/somedoc");
        assert_eq!(
            url,
            Some("https://registry.lemma.dev/user/workspace/somedoc".to_string())
        );
    }

    #[test]
    fn stub_registry_handles_nested_paths() {
        let registry = StubRegistry::new();
        let url = registry.url_for_id("lemma/std/finance");
        assert_eq!(
            url,
            Some("https://registry.lemma.dev/lemma/std/finance".to_string())
        );
    }
}
