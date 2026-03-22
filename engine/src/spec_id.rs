//! Parse spec identifier (name or name~hash) with request-level error handling.
//! Not Lemma source validation — invalid spec id is an API/request error.

use crate::error::Error;
use crate::limits::MAX_SPEC_NAME_LENGTH;

/// Parse a spec identifier into (name, optional hash).
///
/// - `"pricing"` → `("pricing", None)`
/// - `"pricing~a1b2c3d4"` → `("pricing", Some("a1b2c3d4"))`
///
/// Returns `Error::Request` for: empty/whitespace input; `~` present but suffix not exactly 8 hex chars; name empty after trim; name exceeds `MAX_SPEC_NAME_LENGTH`.
pub fn parse_spec_id(s: &str) -> Result<(String, Option<String>), Error> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::request(
            "Spec identifier cannot be empty",
            Some("Provide a spec name or spec~hash (e.g. pricing or pricing~a1b2c3d4)"),
        ));
    }
    let (name, hash) = if let Some(tilde_pos) = s.rfind('~') {
        let hash_part = s[tilde_pos + 1..].trim();
        if hash_part.len() != 8 || !hash_part.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::request(
                format!(
                    "Invalid spec identifier: hash part must be exactly 8 hex characters, got '{}'",
                    hash_part
                ),
                Some("Use spec~a1b2c3d4 (8 hex digits after the tilde) or omit the hash"),
            ));
        }
        let name = s[..tilde_pos].trim();
        if name.is_empty() {
            return Err(Error::request(
                "Spec identifier has empty name (e.g. ~a1b2c3d4)",
                Some("Use spec_name~hash (e.g. pricing~a1b2c3d4)"),
            ));
        }
        (name.to_string(), Some(hash_part.to_lowercase()))
    } else {
        (s.to_string(), None)
    };
    if name.len() > MAX_SPEC_NAME_LENGTH {
        return Err(Error::request(
            format!(
                "Spec name exceeds maximum length ({} characters)",
                MAX_SPEC_NAME_LENGTH
            ),
            Some("Shorten the spec name"),
        ));
    }
    Ok((name, hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_spec_id_name_only() {
        assert_eq!(
            parse_spec_id("pricing").unwrap(),
            ("pricing".to_string(), None)
        );
        assert_eq!(
            parse_spec_id("  pricing  ").unwrap(),
            ("pricing".to_string(), None)
        );
    }

    #[test]
    fn parse_spec_id_with_hash() {
        let (name, hash) = parse_spec_id("pricing~a1b2c3d4").unwrap();
        assert_eq!(name, "pricing");
        assert_eq!(hash, Some("a1b2c3d4".to_string()));
        let (_name, hash) = parse_spec_id("pricing~A1B2C3D4").unwrap();
        assert_eq!(hash, Some("a1b2c3d4".to_string()));
    }

    #[test]
    fn parse_spec_id_empty_err() {
        assert!(parse_spec_id("").is_err());
        assert!(parse_spec_id("   ").is_err());
    }

    #[test]
    fn parse_spec_id_malformed_hash_err() {
        assert!(parse_spec_id("pricing~bad").is_err());
        assert!(parse_spec_id("pricing~1234567").is_err());
        assert!(parse_spec_id("pricing~123456789").is_err());
    }

    #[test]
    fn parse_spec_id_empty_name_err() {
        assert!(parse_spec_id("~a1b2c3d4").is_err());
    }
}
