// Shared fuzzy-suggestion helper for mistyped lookups

/// Calculate the Levenshtein distance between two strings
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let len_a = a.len();
    let len_b = b.len();

    if len_a == 0 {
        return len_b;
    }
    if len_b == 0 {
        return len_a;
    }

    let mut matrix = vec![vec![0; len_b + 1]; len_a + 1];

    for i in 0..=len_a {
        matrix[i][0] = i;
    }
    for j in 0..=len_b {
        matrix[0][j] = j;
    }

    for i in 1..=len_a {
        for j in 1..=len_b {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[len_a][len_b]
}

/// Find the best match for the input from a list of candidates.
/// Returns a formatted string if a suitable suggestion is found.
pub fn did_you_mean(input: &str, candidates: &[&str]) -> Option<String> {
    let mut best_match: Option<&str> = None;
    let mut best_dist = usize::MAX;

    for &candidate in candidates {
        let dist = levenshtein(input, candidate);
        if dist < best_dist {
            best_dist = dist;
            best_match = Some(candidate);
        }
    }

    // Heuristic for acceptable suggestion:
    // Distance should be reasonable based on the length of the input.
    if let Some(match_str) = best_match {
        let max_dist = if input.len() <= 3 {
            1
        } else if input.len() <= 6 {
            2
        } else {
            3
        };

        if best_dist <= max_dist {
            return Some(format!(" did you mean '{}'?", match_str));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("deployr", "deployer"), 1);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("flitten", "flitting"), 2);
    }

    #[test]
    fn test_did_you_mean() {
        let candidates = vec!["deployer", "alice", "bob", "testnet", "mainnet"];

        assert_eq!(
            did_you_mean("deployr", &candidates),
            Some(" did you mean 'deployer'?".to_string())
        );

        assert_eq!(
            did_you_mean("alice1", &candidates),
            Some(" did you mean 'alice'?".to_string())
        );

        assert_eq!(
            did_you_mean("test", &candidates),
            None // 'test' -> 'testnet' dist is 3, length 4. max_dist is 2.
        );

        assert_eq!(
            did_you_mean("testnt", &candidates),
            Some(" did you mean 'testnet'?".to_string())
        );
    }
}
