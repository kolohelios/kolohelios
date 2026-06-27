//! The persisted game state and the pure mutations over it. The Durable
//! Object stores `GameData` as one JSON blob and calls these functions;
//! totals are always recomputed from the round/match history, never trusted
//! from the client.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::engine::config::{Game, Model, RoundPoints, Winner};
use crate::engine::error::EngineError;
use crate::engine::{parse, score};

/// A player's entry in one `roundPoints` round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub raw: String,
    pub tokens: Vec<String>,
    pub points: i64,
}

/// One `roundPoints` round: an entry per player, plus an optional award
/// recipient (e.g. who "tic'd" by going out first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    pub n: u32,
    pub entries: BTreeMap<String, Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub award: Option<String>,
}

/// One finished `matchWins` game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchResult {
    pub n: u32,
    pub winner: String,
}

/// The full state of one game instance.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameData {
    pub game_type: String,
    pub players: Vec<String>,
    #[serde(default)]
    pub rounds: Vec<Round>,
    #[serde(default)]
    pub matches: Vec<MatchResult>,
    pub totals: BTreeMap<String, i64>,
    /// The current leader(s) — empty until at least one round/match exists;
    /// more than one entry means a tie.
    #[serde(default)]
    pub leaders: Vec<String>,
}

impl GameData {
    /// A fresh game from its config defaults.
    pub fn new(game: &Game) -> Self {
        let mut data = GameData {
            game_type: game.id.clone(),
            players: game.players.clone(),
            ..Default::default()
        };
        recompute(&mut data, game);
        data
    }

    fn has_player(&self, player: &str) -> bool {
        self.players.iter().any(|p| p == player)
    }
}

/// Add a `roundPoints` round. `raws` maps player → free-text entry; players
/// absent from the map score zero (no cards). `award` is the recipient of
/// the round award, only valid when the game defines one.
pub fn apply_round(
    data: &mut GameData,
    game: &Game,
    raws: &BTreeMap<String, String>,
    award: Option<String>,
) -> Result<(), EngineError> {
    let Model::RoundPoints(rp) = &game.model else {
        return Err(EngineError::WrongModel);
    };

    let mut entries = BTreeMap::new();
    for player in &data.players {
        let raw = raws.get(player).cloned().unwrap_or_default();
        let tokens = parse::tokenize(&raw, &rp.scoring.empty_aliases);
        let points = score::entry_points(&tokens, &rp.scoring)?;
        entries.insert(
            player.clone(),
            Entry {
                raw,
                tokens,
                points,
            },
        );
    }

    if let Some(recipient) = &award {
        if rp.round_award.is_none() {
            return Err(EngineError::AwardNotSupported);
        }
        if !data.has_player(recipient) {
            return Err(EngineError::UnknownPlayer {
                player: recipient.clone(),
            });
        }
    }

    let n = data.rounds.len() as u32 + 1;
    data.rounds.push(Round { n, entries, award });
    recompute(data, game);
    Ok(())
}

/// Record a `matchWins` game result.
pub fn apply_match(data: &mut GameData, game: &Game, winner: &str) -> Result<(), EngineError> {
    if !matches!(game.model, Model::MatchWins(_)) {
        return Err(EngineError::WrongModel);
    }
    if !data.has_player(winner) {
        return Err(EngineError::UnknownPlayer {
            player: winner.to_owned(),
        });
    }
    let n = data.matches.len() as u32 + 1;
    data.matches.push(MatchResult {
        n,
        winner: winner.to_owned(),
    });
    recompute(data, game);
    Ok(())
}

/// Remove the last round (`roundPoints`) or match (`matchWins`).
pub fn undo(data: &mut GameData, game: &Game) -> Result<(), EngineError> {
    let removed = match game.model {
        Model::RoundPoints(_) => data.rounds.pop().is_some(),
        Model::MatchWins(_) => data.matches.pop().is_some(),
    };
    if !removed {
        return Err(EngineError::NothingToUndo);
    }
    recompute(data, game);
    Ok(())
}

/// Start a fresh game, optionally with a new roster. An empty/omitted
/// roster keeps the current players.
pub fn reset(data: &mut GameData, game: &Game, players: Option<Vec<String>>) {
    if let Some(roster) = players {
        if !roster.is_empty() {
            data.players = roster;
        }
    }
    data.rounds.clear();
    data.matches.clear();
    recompute(data, game);
}

/// Recompute `totals` and `leaders` from the round/match history. The
/// per-round award adjustment is folded into the running total (so live
/// standings already reflect it; the final result is identical to applying
/// it once at game end).
fn recompute(data: &mut GameData, game: &Game) {
    let mut totals: BTreeMap<String, i64> = data.players.iter().map(|p| (p.clone(), 0)).collect();

    match &game.model {
        Model::RoundPoints(rp) => {
            for round in &data.rounds {
                for (player, entry) in &round.entries {
                    *totals.entry(player.clone()).or_insert(0) += entry.points;
                }
                apply_award(&mut totals, round.award.as_deref(), rp);
            }
            data.leaders = if data.rounds.is_empty() {
                Vec::new()
            } else {
                leaders_by(&totals, rp.winner)
            };
        }
        Model::MatchWins(_) => {
            for m in &data.matches {
                *totals.entry(m.winner.clone()).or_insert(0) += 1;
            }
            data.leaders = if data.matches.is_empty() {
                Vec::new()
            } else {
                leaders_by(&totals, Winner::Highest)
            };
        }
    }

    data.totals = totals;
}

fn apply_award(totals: &mut BTreeMap<String, i64>, recipient: Option<&str>, rp: &RoundPoints) {
    if let (Some(recipient), Some(award)) = (recipient, &rp.round_award) {
        *totals.entry(recipient.to_owned()).or_insert(0) += award.points_per_award;
    }
}

fn leaders_by(totals: &BTreeMap<String, i64>, winner: Winner) -> Vec<String> {
    let best = match winner {
        Winner::Lowest => totals.values().min().copied(),
        Winner::Highest => totals.values().max().copied(),
    };
    match best {
        Some(best) => totals
            .iter()
            .filter(|(_, v)| **v == best)
            .map(|(k, _)| k.clone())
            .collect(),
        None => Vec::new(),
    }
}
