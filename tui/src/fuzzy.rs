//! fzf-style fuzzy matching for the project filter and the directory finder.

use ratatui::{style::Style, text::Span};

/// fzf-style: every space-separated term must match as a subsequence; smart
/// case. Returns a score (higher is better) and the matched char indices.
// ponytail: greedy leftmost match, can miss the best-scoring alignment.
// Swap in nucleo-matcher if the ranking ever feels off.
pub(crate) fn fuzzy(query: &str, text: &str) -> Option<(i64, Vec<usize>)> {
    let fold = !query.chars().any(char::is_uppercase);
    let norm = |c: char| if fold { c.to_ascii_lowercase() } else { c };
    let chars: Vec<char> = text.chars().collect();
    let (mut score, mut hits) = (0, vec![]);
    for term in query.split_whitespace() {
        let mut from = 0;
        for q in term.chars().map(norm) {
            let j = (from..chars.len()).find(|&j| norm(chars[j]) == q)?;
            score += 1;
            if j > 0 && hits.last() == Some(&(j - 1)) {
                score += 5; // consecutive
            }
            if j == 0 || matches!(chars[j - 1], '/' | '-' | '_' | '.' | ' ') {
                score += 3; // start of a word
            }
            hits.push(j);
            from = j + 1;
        }
    }
    hits.sort_unstable();
    hits.dedup();
    Some((score, hits))
}

/// `text` as spans, matched chars bold yellow on top of `base`.
pub(crate) fn highlight(text: &str, hits: &[usize], base: Style) -> Vec<Span<'static>> {
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            let style = if hits.contains(&i) { base.yellow().bold() } else { base };
            Span::styled(c.to_string(), style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks() {
        assert_eq!(fuzzy("rlf", "~/lua/rollfor.git").unwrap().1, vec![6, 8, 10]);
        assert!(fuzzy("lua roll", "~/lua/rollfor.git").is_some()); // terms in any order
        assert!(fuzzy("roll lua", "~/lua/rollfor.git").is_some());
        assert!(fuzzy("xyz", "~/lua/rollfor.git").is_none());
        assert!(fuzzy("Lua", "~/lua/rollfor.git").is_none()); // smart case
        let score = |q, t| fuzzy(q, t).unwrap().0;
        assert!(score("roll", "~/rollfor") > score("roll", "~/r-o-l-l")); // consecutive wins
    }
}
