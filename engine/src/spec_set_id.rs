//! Parse SpecSet identifier: a spec `name`.
//!
//! Effective datetime is never embedded in the id string; pass it separately (e.g. CLI `--effective`).

use crate::error::Error;
use crate::limits::MAX_SPEC_NAME_LENGTH;
use crate::parsing::ast::DateTimeValue;

/// Parse a SpecSet identifier into logical name and optional effective.
pub fn parse_spec_set_id(s: &str) -> Result<(String, Option<DateTimeValue>), Error> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::request(
            "SpecSet identifier cannot be empty",
            Some("Use a spec name"),
        ));
    }

    if s.contains('~') {
        return Err(Error::request(
            "SpecSet identifier cannot contain '~'",
            Some("Use a plain spec name"),
        ));
    }

    if s.contains('^') {
        return Err(Error::request(
            "SpecSet identifier cannot contain '^'",
            Some("Use a plain spec name"),
        ));
    }

    let name = s.to_string();

    if name.len() > MAX_SPEC_NAME_LENGTH {
        return Err(Error::request(
            format!(
                "Spec name exceeds maximum length ({} characters)",
                MAX_SPEC_NAME_LENGTH
            ),
            Some("Shorten the spec name"),
        ));
    }

    Ok((name, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_name_only() {
        assert_eq!(
            parse_spec_set_id("pricing").unwrap(),
            ("pricing".to_string(), None)
        );
        assert_eq!(
            parse_spec_set_id("  pricing  ").unwrap(),
            ("pricing".to_string(), None)
        );
    }

    #[test]
    fn tilde_rejected() {
        assert!(parse_spec_set_id("pricing~a1b2c3d4").is_err());
    }

    #[test]
    fn caret_rejected() {
        assert!(parse_spec_set_id("pricing^a1b2c3d4").is_err());
    }

    #[test]
    fn empty_err() {
        assert!(parse_spec_set_id("").is_err());
        assert!(parse_spec_set_id("   ").is_err());
    }
}
