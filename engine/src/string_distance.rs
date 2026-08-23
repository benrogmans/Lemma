//! Edit-distance typo hints — shared by planning (`-> with` errors) and evaluation (missing input keys).

/// Levenshtein distance between two strings.
pub(crate) fn levenshtein(left: &str, right: &str) -> usize {
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let (left_len, right_len) = (left_chars.len(), right_chars.len());
    if left_len == 0 {
        return right_len;
    }
    if right_len == 0 {
        return left_len;
    }
    let mut previous: Vec<usize> = (0..=right_len).collect();
    let mut current = vec![0; right_len + 1];
    for (i, left_char) in left_chars.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right_chars.iter().enumerate() {
            let substitution = usize::from(left_char != right_char);
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right_len]
}

/// Nearest candidate within edit-distance threshold, if any.
/// Comparison is case-insensitive; returned spelling is the candidate's original.
pub(crate) fn closest_name(needed: &str, candidates: &[String]) -> Option<String> {
    let max_distance = if needed.len() <= 3 { 1 } else { 2 };
    let needed_lower = needed.to_ascii_lowercase();
    let mut best: Option<(usize, String, &String)> = None;
    for candidate in candidates {
        let candidate_lower = candidate.to_ascii_lowercase();
        let distance = levenshtein(&needed_lower, &candidate_lower);
        if distance == 0 || distance > max_distance {
            continue;
        }
        let dominated = best
            .as_ref()
            .map(|(best_distance, best_lower, _)| {
                distance < *best_distance
                    || (distance == *best_distance && candidate_lower < *best_lower)
            })
            .unwrap_or(true);
        if dominated {
            best = Some((distance, candidate_lower, candidate));
        }
    }
    best.map(|(_, _, key)| key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_name_typo_duration() {
        let candidates = ["duration", "length", "mass"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        assert_eq!(
            closest_name("durration", &candidates),
            Some("duration".to_string())
        );
    }

    #[test]
    fn closest_name_no_match_when_far() {
        let candidates = ["duration", "length"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        assert_eq!(closest_name("non_existing_data", &candidates), None);
    }
}
