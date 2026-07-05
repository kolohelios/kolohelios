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

/// One `roundPoints` round: an entry per player, plus optional per-round
/// context — the award recipient (who "tic'd"), the wild rank in effect, and
/// who dealt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    pub n: u32,
    pub entries: BTreeMap<String, Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub award: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wild: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dealer: Option<String>,
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
    /// Index into `players` of the round-1 dealer (rotating-dealer games).
    #[serde(default)]
    pub dealer_start: usize,
    /// Who deals the upcoming round (rotating-dealer games, when not
    /// complete).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_dealer: Option<String>,
    /// The wild rank for the upcoming round (wild-progression games, when not
    /// complete).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_wild: Option<String>,
    /// True once a wild-progression game has played all its rounds.
    #[serde(default)]
    pub complete: bool,
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

    // 0-based index of the round being added; its wild rank (if the game has
    // a progression), which also caps the number of rounds.
    let idx = data.rounds.len();
    let wild = match &rp.wild_progression {
        Some(wp) => match wp.ranks.get(idx) {
            Some(rank) => Some((rank.clone(), wp.points)),
            None => return Err(EngineError::GameComplete),
        },
        None => None,
    };
    let wild_ctx = wild.as_ref().map(|(rank, points)| (rank.as_str(), *points));

    let mut entries = BTreeMap::new();
    for player in &data.players {
        let raw = raws.get(player).cloned().unwrap_or_default();
        let tokens = parse::tokenize(&raw, &rp.scoring.empty_aliases);
        let points = score::entry_points(&tokens, &rp.scoring, wild_ctx)?;
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

    let dealer = dealer_for_round(&data.players, rp, data.dealer_start, idx);
    data.rounds.push(Round {
        n: idx as u32 + 1,
        entries,
        award,
        wild: wild.map(|(rank, _)| rank),
        dealer,
    });
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

/// Start a fresh game, optionally with a new roster and/or a round-1 dealer.
/// An empty/omitted roster keeps the current players; a `first_dealer` that
/// isn't a current player is ignored.
pub fn reset(
    data: &mut GameData,
    game: &Game,
    players: Option<Vec<String>>,
    first_dealer: Option<String>,
) {
    if let Some(roster) = players {
        if !roster.is_empty() {
            data.players = roster;
        }
    }
    if let Some(dealer) = first_dealer {
        if let Some(i) = data.players.iter().position(|p| *p == dealer) {
            data.dealer_start = i;
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

            // Upcoming round (0-based index = rounds played so far).
            let next = data.rounds.len();
            data.complete = rp
                .wild_progression
                .as_ref()
                .is_some_and(|wp| next >= wp.ranks.len());
            data.next_wild = if data.complete {
                None
            } else {
                rp.wild_progression
                    .as_ref()
                    .and_then(|wp| wp.ranks.get(next).cloned())
            };
            data.next_dealer = if data.complete {
                None
            } else {
                dealer_for_round(&data.players, rp, data.dealer_start, next)
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
            data.complete = false;
            data.next_wild = None;
            data.next_dealer = None;
        }
    }

    data.totals = totals;
}

/// Who deals round `round_index` (0-based) for a rotating-dealer game:
/// `players[(dealer_start + round_index) % n]`. `None` if the game has no
/// rotating dealer or no players.
fn dealer_for_round(
    players: &[String],
    rp: &RoundPoints,
    dealer_start: usize,
    round_index: usize,
) -> Option<String> {
    if !rp.dealer.as_ref().is_some_and(|d| d.rotates) || players.is_empty() {
        return None;
    }
    let i = (dealer_start + round_index) % players.len();
    Some(players[i].clone())
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
