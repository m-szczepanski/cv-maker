//! "Did you mean" suggestions for user-supplied names (fonts, templates, keys).

/// Closest known name by case-insensitive edit distance. `None` when nothing is close
/// enough (distance > 2) — then callers should list the available options instead.
pub fn closest_match(target: &str, known: &[String]) -> Option<String> {
    let target = target.to_lowercase();
    known
        .iter()
        .map(|k| (edit_distance(&target, &k.to_lowercase()), k))
        .min_by_key(|(distance, _)| *distance)
        .filter(|(distance, _)| *distance <= 2)
        .map(|(_, k)| k.clone())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_match_finds_a_typo() {
        let known = vec!["Inter".to_owned(), "JetBrains Mono".to_owned()];
        assert_eq!(closest_match("Inte", &known).as_deref(), Some("Inter"));
    }

    #[test]
    fn closest_match_is_case_insensitive() {
        let known = vec!["compact".to_owned()];
        assert_eq!(closest_match("COMPACT", &known).as_deref(), Some("compact"));
    }

    #[test]
    fn closest_match_gives_up_on_unrelated_names() {
        let known = vec!["Inter".to_owned()];
        assert_eq!(closest_match("zzzzzzz", &known), None);
    }

    #[test]
    fn closest_match_picks_the_best_of_several_candidates() {
        let known = vec![
            "Ibiza".to_owned(),
            "Inter".to_owned(),
            "Internal".to_owned(),
        ];
        assert_eq!(closest_match("Intre", &known).as_deref(), Some("Inter"));
    }
}
