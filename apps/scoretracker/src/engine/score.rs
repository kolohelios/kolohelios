//! Token → points, per a game's `Scoring`. Lookup order: the round's wild
//! rank (if any) scores as wild and wins; then a named token (e.g. `skip`,
//! `ace`); otherwise parse as an integer and match `ranges` (first hit)
//! then `faceValue` (value == points).

use crate::engine::config::Scoring;
use crate::engine::error::EngineError;

/// The round's wild: its canonical rank plus the points a matching card
/// scores. `None` for games without a per-round wild.
pub type Wild<'a> = Option<(&'a str, i64)>;

/// Canonical rank key for wild-matching. Face cards and aces collapse to a
/// single letter (so `jack`/`j` both match wild rank `j`); number cards are
/// their own value. `None` for tokens that aren't a card rank (e.g. `skip`,
/// `wild`) — those can never be the round's wild.
pub fn canonical_rank(token: &str) -> Option<String> {
    match token {
        "a" | "ace" => Some("a".to_owned()),
        "k" | "king" => Some("k".to_owned()),
        "q" | "queen" => Some("q".to_owned()),
        "j" | "jack" => Some("j".to_owned()),
        _ => token
            .parse::<i64>()
            .ok()
            .filter(|n| (2..=10).contains(n))
            .map(|n| n.to_string()),
    }
}

/// Points for a single already-lowercased token, given the round's wild.
pub fn token_points(token: &str, scoring: &Scoring, wild: Wild) -> Result<i64, EngineError> {
    // The round's wild rank overrides that rank's normal value.
    if let Some((wild_rank, wild_points)) = wild {
        if canonical_rank(token).as_deref() == Some(wild_rank) {
            return Ok(wild_points);
        }
    }
    if let Some(points) = scoring.named.get(token) {
        return Ok(*points);
    }
    if let Ok(n) = token.parse::<i64>() {
        for r in &scoring.ranges {
            if n >= r.from && n <= r.to {
                return Ok(r.points);
            }
        }
        if let Some(fv) = &scoring.face_value {
            if n >= fv.from && n <= fv.to {
                return Ok(n);
            }
        }
    }
    Err(EngineError::UnknownToken {
        token: token.to_owned(),
    })
}

/// Sum the points of one player's tokens. Empty (no cards) is zero.
pub fn entry_points(tokens: &[String], scoring: &Scoring, wild: Wild) -> Result<i64, EngineError> {
    let mut total = 0;
    for t in tokens {
        total += token_points(t, scoring, wild)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::canonical_rank;

    #[test]
    fn canonical_rank_collapses_aliases() {
        assert_eq!(canonical_rank("jack").as_deref(), Some("j"));
        assert_eq!(canonical_rank("j").as_deref(), Some("j"));
        assert_eq!(canonical_rank("KING"), None); // caller lowercases first
        assert_eq!(canonical_rank("king").as_deref(), Some("k"));
        assert_eq!(canonical_rank("10").as_deref(), Some("10"));
        assert_eq!(canonical_rank("5").as_deref(), Some("5"));
        assert_eq!(canonical_rank("wild"), None);
        assert_eq!(canonical_rank("skip"), None);
    }
}
