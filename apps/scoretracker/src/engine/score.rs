//! Token → points, per a game's `Scoring`. Lookup order: a named token
//! (e.g. `skip`, `wild`, `ace`) wins; otherwise parse as an integer and
//! match `ranges` (first hit) then `faceValue` (value == points).

use crate::engine::config::Scoring;
use crate::engine::error::EngineError;

/// Points for a single already-lowercased token.
pub fn token_points(token: &str, scoring: &Scoring) -> Result<i64, EngineError> {
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
pub fn entry_points(tokens: &[String], scoring: &Scoring) -> Result<i64, EngineError> {
    let mut total = 0;
    for t in tokens {
        total += token_points(t, scoring)?;
    }
    Ok(total)
}
