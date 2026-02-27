use std::cmp::Ordering;

/// A chunk in a version string: either a run of digits (compared numerically)
/// or a run of non-digits (compared lexicographically).
#[derive(Debug, PartialEq, Eq)]
enum Chunk<'a> {
    Numeric(u128),
    Text(&'a str),
}

impl<'a> PartialOrd for Chunk<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> Ord for Chunk<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Chunk::Numeric(a), Chunk::Numeric(b)) => a.cmp(b),
            (Chunk::Text(a), Chunk::Text(b)) => a.cmp(b),
            (Chunk::Numeric(_), Chunk::Text(_)) => Ordering::Less,
            (Chunk::Text(_), Chunk::Numeric(_)) => Ordering::Greater,
        }
    }
}

/// Split a version string into alternating chunks of digit runs and non-digit runs.
fn split_into_chunks(s: &str) -> Vec<Chunk<'_>> {
    let mut chunks = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // Safe: version tags are validated at parse time to be at most
            // MAX_VERSION_TAG_LENGTH (8) characters, so digit runs cannot
            // overflow u128 (which supports up to 39 digits).
            let num: u128 = s[start..i]
                .parse()
                .expect("Version tag should have been validated to be 8 chars or less");
            chunks.push(Chunk::Numeric(num));
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_digit() {
                i += 1;
            }
            chunks.push(Chunk::Text(&s[start..i]));
        }
    }

    chunks
}

/// Compare two optional version tags using natural sort order.
///
/// `None` (unversioned) always sorts before any `Some`.
/// Within `Some` values, the version string is split into alternating runs of
/// digits and non-digits. Digit runs are compared numerically (so `v10 > v2`),
/// non-digit runs lexicographically. When one string runs out of chunks before
/// the other, it sorts first.
pub fn compare_versions(a: &Option<String>, b: &Option<String>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(va), Some(vb)) => {
            let chunks_a = split_into_chunks(va);
            let chunks_b = split_into_chunks(vb);
            chunks_a.cmp(&chunks_b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn none_sorts_before_any_some() {
        assert_eq!(compare_versions(&None, &None), Ordering::Equal);
        assert_eq!(compare_versions(&None, &v("v1")), Ordering::Less);
        assert_eq!(compare_versions(&v("v1"), &None), Ordering::Greater);
    }

    #[test]
    fn bare_digits_sort_numerically() {
        assert_eq!(compare_versions(&v("3"), &v("12")), Ordering::Less);
        assert_eq!(compare_versions(&v("12"), &v("3")), Ordering::Greater);
    }

    #[test]
    fn digit_chunks_sort_before_text_chunks() {
        assert_eq!(compare_versions(&v("3"), &v("v1")), Ordering::Less);
        assert_eq!(compare_versions(&v("12"), &v("v1")), Ordering::Less);
    }

    #[test]
    fn v_prefixed_sort_numerically_within_prefix() {
        assert_eq!(compare_versions(&v("v1"), &v("v2")), Ordering::Less);
        assert_eq!(compare_versions(&v("v2"), &v("v10")), Ordering::Less);
        assert_eq!(compare_versions(&v("v2"), &v("v12")), Ordering::Less);
        assert_eq!(compare_versions(&v("v12"), &v("v12")), Ordering::Equal);
    }

    #[test]
    fn dotted_versions() {
        assert_eq!(compare_versions(&v("v2.0"), &v("v2.1")), Ordering::Less);
        assert_eq!(compare_versions(&v("v2.1"), &v("v2.9b")), Ordering::Less);
        assert_eq!(compare_versions(&v("v2.9b"), &v("v2.10")), Ordering::Less);
    }

    #[test]
    fn hotfix_suffix_sorts_after_bare_version() {
        assert_eq!(
            compare_versions(&v("v2.10"), &v("v2.10-hotfix")),
            Ordering::Less
        );
    }

    #[test]
    fn complex_version_tags() {
        assert_eq!(
            compare_versions(&v("v12"), &v("v12.3a-b.c3c")),
            Ordering::Less
        );
        assert_eq!(
            compare_versions(&v("v12.3a-b.c3c"), &v("v12.4")),
            Ordering::Less
        );
    }

    #[test]
    fn full_ordering_from_plan_section_6_2() {
        let tags: Vec<Option<String>> = vec![
            None,
            v("3"),
            v("12"),
            v("v1"),
            v("v2"),
            v("v2.0"),
            v("v2.1"),
            v("v2.9b"),
            v("v2.10"),
            v("v2.10-hotfix"),
            v("v12"),
            v("v12.3a-b.c3c"),
            v("v12.4"),
        ];

        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_eq!(
                    compare_versions(&tags[i], &tags[j]),
                    Ordering::Less,
                    "Expected {:?} < {:?}",
                    tags[i],
                    tags[j]
                );
                assert_eq!(
                    compare_versions(&tags[j], &tags[i]),
                    Ordering::Greater,
                    "Expected {:?} > {:?}",
                    tags[j],
                    tags[i]
                );
            }
        }
    }
}
