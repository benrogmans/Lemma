use crate::registry::Registry;
use tower_lsp::lsp_types::{DocumentLink, Position, Range, Url};

/// Scan a document's text for `@`-prefixed Registry references and return document links.
///
/// This uses a text-based scan to detect patterns like:
/// - `doc @user/workspace/somedoc`
/// - `type ... from @lemma/std/finance`
///
/// For each `@identifier` found, the Registry is consulted for a URL.
/// If the Registry returns a URL, a DocumentLink is created.
///
/// The text-based approach works regardless of whether the file parses successfully,
/// which is important because files being edited may have transient parse errors.
pub fn find_registry_links(text: &str, registry: &dyn Registry) -> Vec<DocumentLink> {
    let mut links = Vec::new();

    // Scan for `@` characters and extract the identifier that follows.
    // Base name: ASCII_ALPHA then (ASCII_ALPHANUMERIC | "_" | "-" | "/")*  (no dot)
    // Optional version suffix: `.` then ASCII_ALPHANUMERIC then (ASCII_ALPHANUMERIC | "_" | "-" | ".")*
    let bytes = text.as_bytes();
    let mut byte_index = 0;

    while byte_index < bytes.len() {
        if bytes[byte_index] == b'@' {
            let at_byte_start = byte_index;
            byte_index += 1; // skip the '@'

            if byte_index < bytes.len() && bytes[byte_index].is_ascii_alphabetic() {
                let identifier_start = byte_index;

                // Consume base name characters (no dot — dot starts version)
                while byte_index < bytes.len() {
                    let byte = bytes[byte_index];
                    if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'/'
                    {
                        byte_index += 1;
                    } else {
                        break;
                    }
                }

                let base_name = &text[identifier_start..byte_index];

                // Check for version suffix: `.` followed by alphanumeric start
                let mut version: Option<&str> = None;
                let mut link_end_byte = byte_index;

                if byte_index < bytes.len()
                    && bytes[byte_index] == b'.'
                    && byte_index + 1 < bytes.len()
                    && bytes[byte_index + 1].is_ascii_alphanumeric()
                {
                    let version_start = byte_index + 1;
                    let mut version_end = version_start;
                    while version_end < bytes.len() {
                        let byte = bytes[version_end];
                        if byte.is_ascii_alphanumeric()
                            || byte == b'_'
                            || byte == b'-'
                            || byte == b'.'
                        {
                            version_end += 1;
                        } else {
                            break;
                        }
                    }
                    version = Some(&text[version_start..version_end]);
                    link_end_byte = version_end;
                    byte_index = version_end;
                }

                if !base_name.is_empty() {
                    if let Some(url_string) = registry.url_for_id(base_name, version) {
                        if let Ok(target_url) = Url::parse(&url_string) {
                            let start_position = byte_offset_to_position(text, at_byte_start);
                            let end_position = byte_offset_to_position(text, link_end_byte);

                            let display = match version {
                                Some(v) => format!("@{}.{}", base_name, v),
                                None => format!("@{}", base_name),
                            };

                            links.push(DocumentLink {
                                range: Range {
                                    start: start_position,
                                    end: end_position,
                                },
                                target: Some(target_url),
                                tooltip: Some(format!("Open {} in Registry", display)),
                                data: None,
                            });
                        }
                    }
                }
            }
        } else {
            byte_index += 1;
        }
    }

    links
}

/// Convert a byte offset to an LSP Position (0-based line and UTF-16 code unit column).
///
/// This is a local copy of the same logic in diagnostics.rs, kept here to avoid
/// a circular dependency between modules.
fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let clamped_offset = byte_offset.min(text.len());
    let mut line: u32 = 0;
    let mut line_start_byte: usize = 0;

    for (index, byte) in text.bytes().enumerate() {
        if index == clamped_offset {
            break;
        }
        if byte == b'\n' {
            line += 1;
            line_start_byte = index + 1;
        }
    }

    let line_slice = &text[line_start_byte..clamped_offset];
    let utf16_column: u32 = line_slice.encode_utf16().count() as u32;

    Position {
        line,
        character: utf16_column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lemma::registry::{Registry, RegistryBundle, RegistryError, RegistryErrorKind};

    /// Test-only Registry: predictable URLs for document link tests (no resolution).
    struct TestLinkRegistry;

    #[async_trait::async_trait]
    impl Registry for TestLinkRegistry {
        async fn resolve_doc(
            &self,
            name: &str,
            _version: Option<&str>,
        ) -> Result<RegistryBundle, RegistryError> {
            Err(RegistryError {
                message: format!("TestLinkRegistry does not resolve documents: '{}'", name),
                kind: RegistryErrorKind::Other,
            })
        }

        async fn resolve_type(
            &self,
            name: &str,
            _version: Option<&str>,
        ) -> Result<RegistryBundle, RegistryError> {
            Err(RegistryError {
                message: format!("TestLinkRegistry does not resolve type imports: '{}'", name),
                kind: RegistryErrorKind::Other,
            })
        }

        fn url_for_id(&self, name: &str, version: Option<&str>) -> Option<String> {
            match version {
                Some(v) => Some(format!("https://test.lemma.dev/{}.{}", name, v)),
                None => Some(format!("https://test.lemma.dev/{}", name)),
            }
        }
    }

    #[test]
    fn finds_doc_reference_with_at_prefix() {
        let text = "doc example\nfact ext = doc @user/workspace/somedoc";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().map(|u| u.as_str()),
            Some("https://test.lemma.dev/user/workspace/somedoc")
        );
        // The link should span from '@' to the end of the identifier.
        assert_eq!(links[0].range.start.line, 1);
        assert_eq!(links[0].range.end.line, 1);
    }

    #[test]
    fn finds_type_import_with_at_prefix() {
        let text = "doc example\ntype money from @lemma/std/finance";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().map(|u| u.as_str()),
            Some("https://test.lemma.dev/lemma/std/finance")
        );
    }

    #[test]
    fn finds_multiple_at_references() {
        // Doc declarations don't use @, so only the two references produce links.
        let text =
            "doc org/proj/main\nfact other = doc @org/proj/helper\ntype t from @org/proj/types";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn no_links_when_no_at_references() {
        let text = "doc example\nfact x = 10\nrule y = x + 1";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert!(links.is_empty());
    }

    #[test]
    fn at_sign_without_valid_identifier_is_ignored() {
        let text = "doc example\nfact x = @123invalid";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert!(
            links.is_empty(),
            "@ followed by digit should not produce a link"
        );
    }

    #[test]
    fn at_sign_at_end_of_text_is_ignored() {
        let text = "doc example\nfact x = @";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert!(links.is_empty());
    }

    #[test]
    fn trailing_dot_is_stripped_from_identifier() {
        let text = "See doc @user/workspace/somedoc.";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().map(|u| u.as_str()),
            Some("https://test.lemma.dev/user/workspace/somedoc")
        );
    }

    // =====================================================================
    // Versioned document link tests (section 6.6)
    // =====================================================================

    #[test]
    fn versioned_doc_reference_produces_link_with_version() {
        let text = "fact x = doc @owner/repo/doc.v2";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().map(|u| u.as_str()),
            Some("https://test.lemma.dev/owner/repo/doc.v2")
        );
    }

    #[test]
    fn trailing_dot_without_version_excludes_dot_from_link() {
        let text = "fact x = doc @owner/repo/doc.";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().map(|u| u.as_str()),
            Some("https://test.lemma.dev/owner/repo/doc")
        );
    }

    #[test]
    fn unversioned_doc_reference_has_no_version_param() {
        let text = "fact x = doc @owner/repo/doc";
        let registry = TestLinkRegistry;
        let links = find_registry_links(text, &registry);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().map(|u| u.as_str()),
            Some("https://test.lemma.dev/owner/repo/doc")
        );
    }
}
