//! Free-text round input → normalized tokens. The whole trimmed input is
//! checked against the game's "no cards" aliases first (so `"no cards"`
//! isn't split into two tokens), then split on commas/whitespace and
//! lowercased. Scoring (`score::token_points`) is what rejects an unknown
//! token; tokenizing never fails.

/// Tokenize one player's raw round input. Returns an empty vec for empty
/// input or any whole-input "no cards" alias.
pub fn tokenize(raw: &str, empty_aliases: &[String]) -> Vec<String> {
    let trimmed = raw.trim();
    let lowered = trimmed.to_lowercase();
    if trimmed.is_empty() || empty_aliases.iter().any(|a| a.to_lowercase() == lowered) {
        return Vec::new();
    }
    trimmed
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases() -> Vec<String> {
        vec!["no cards".to_owned(), String::new()]
    }

    #[test]
    fn splits_on_commas_and_whitespace_and_lowercases() {
        assert_eq!(tokenize("1, 2, 3", &aliases()), ["1", "2", "3"]);
        assert_eq!(tokenize("Wild, 5  5", &aliases()), ["wild", "5", "5"]);
        assert_eq!(tokenize("skip", &aliases()), ["skip"]);
    }

    #[test]
    fn empty_and_no_cards_are_zero_tokens() {
        assert!(tokenize("", &aliases()).is_empty());
        assert!(tokenize("   ", &aliases()).is_empty());
        assert!(tokenize("no cards", &aliases()).is_empty());
        assert!(tokenize("No Cards", &aliases()).is_empty());
    }
}
