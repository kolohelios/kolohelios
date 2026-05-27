//! Detect GitHub-autoclose references to issues in a commit body.
//!
//! Only the keywords that GitHub actually treats as autoclose on PR
//! merge count: `close`/`closes`/`closed`, `fix`/`fixes`/`fixed`,
//! `resolve`/`resolves`/`resolved`. `Refs #N` and bare `#N` mentions
//! do *not* trigger autoclose, so they don't satisfy the "tip commit
//! must close the issue on merge" rule — see #146.
//!
//! Used by `shaka commit lint` to gate the tip of an issue-tied
//! branch. The same extractor would let a future `shaka commit
//! validate-issue` (the noted #146 follow-up) check that the
//! referenced issue actually exists.

use std::collections::BTreeSet;

const KEYWORDS: &[&str] = &[
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];

/// Return every issue number `N` for which `body` contains a
/// GitHub-autoclose reference (`<keyword> #N`, case-insensitive on
/// the keyword). Order-preserving by first occurrence; deduplicated.
pub fn extract_autoclose_refs(body: &str) -> Vec<u64> {
    let lower = body.to_ascii_lowercase();
    let mut out: Vec<u64> = Vec::new();
    let mut seen: BTreeSet<u64> = BTreeSet::new();
    for keyword in KEYWORDS {
        let needle = format!("{keyword} #");
        let mut search_from = 0usize;
        while let Some(idx) = lower[search_from..].find(&needle) {
            let abs = search_from + idx + needle.len();
            // Reject matches that aren't on a word boundary on the left
            // (e.g. `unfixes #N` shouldn't count).
            let left_ok =
                idx == 0 || !lower.as_bytes()[search_from + idx - 1].is_ascii_alphanumeric();
            let digits: String = lower[abs..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if left_ok && !digits.is_empty() {
                if let Ok(n) = digits.parse::<u64>() {
                    if seen.insert(n) {
                        out.push(n);
                    }
                }
                search_from = abs + digits.len();
            } else {
                search_from = abs;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closes_with_number() {
        assert_eq!(extract_autoclose_refs("Closes #42."), vec![42]);
    }

    #[test]
    fn case_insensitive_keyword() {
        assert_eq!(extract_autoclose_refs("closes #42"), vec![42]);
        assert_eq!(extract_autoclose_refs("CLOSES #42"), vec![42]);
    }

    #[test]
    fn all_autoclose_keywords() {
        for kw in [
            "Close", "Closes", "Closed", "Fix", "Fixes", "Fixed", "Resolve", "Resolves", "Resolved",
        ] {
            let body = format!("{kw} #1");
            assert_eq!(extract_autoclose_refs(&body), vec![1], "keyword {kw}");
        }
    }

    #[test]
    fn refs_keyword_excluded() {
        // Refs #N is intentionally not an autoclose keyword on GitHub,
        // so it doesn't satisfy the lint rule.
        assert!(extract_autoclose_refs("Refs #42").is_empty());
    }

    #[test]
    fn bare_hash_number_excluded() {
        assert!(extract_autoclose_refs("see #42").is_empty());
    }

    #[test]
    fn multiple_distinct_refs() {
        let refs = extract_autoclose_refs("Closes #1. Fixes #2. Resolves #3.");
        assert_eq!(refs, vec![1, 2, 3]);
    }

    #[test]
    fn dedupes_same_number() {
        let refs = extract_autoclose_refs("Closes #42. Also fixes #42.");
        assert_eq!(refs, vec![42]);
    }

    #[test]
    fn rejects_word_boundary_on_left() {
        // `unfixes` shouldn't match `fixes` — autoclose requires the
        // keyword to start on a word boundary.
        assert!(extract_autoclose_refs("unfixes #42").is_empty());
    }

    #[test]
    fn ignores_phrase_without_number() {
        assert!(extract_autoclose_refs("Closes #notanumber").is_empty());
    }

    #[test]
    fn empty_body() {
        assert!(extract_autoclose_refs("").is_empty());
    }
}
