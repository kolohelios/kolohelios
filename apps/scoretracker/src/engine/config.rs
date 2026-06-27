//! Game definitions, embedded at build time. `build.rs` runs `cue export`
//! over `games/*.cue` into `$OUT_DIR/games.json`; this module deserializes
//! that into the in-memory registry. The serde shapes mirror
//! `games/schema.cue` exactly.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The exported `games:` map (id -> game), produced by `build.rs`.
const GAMES_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/games.json"));

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub players: Vec<String>,
    pub model: Model,
}

/// Internally tagged on `kind` so it round-trips the CUE/JSON shape
/// (`{"kind": "roundPoints", ...}`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind")]
pub enum Model {
    #[serde(rename = "roundPoints")]
    RoundPoints(RoundPoints),
    #[serde(rename = "matchWins")]
    MatchWins(MatchWins),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Winner {
    Lowest,
    Highest,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundPoints {
    pub winner: Winner,
    pub scoring: Scoring,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_award: Option<RoundAward>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scoring {
    #[serde(default)]
    pub ranges: Vec<Range>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_value: Option<FaceValue>,
    #[serde(default)]
    pub named: BTreeMap<String, i64>,
    #[serde(default)]
    pub empty_aliases: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Range {
    pub from: i64,
    pub to: i64,
    pub points: i64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct FaceValue {
    pub from: i64,
    pub to: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundAward {
    pub label: String,
    pub points_per_award: i64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MatchWins {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<i64>,
}

/// The parsed registry of every embedded game, keyed by id.
pub fn registry() -> &'static BTreeMap<String, Game> {
    static REG: OnceLock<BTreeMap<String, Game>> = OnceLock::new();
    REG.get_or_init(|| serde_json::from_str(GAMES_JSON).expect("embedded games.json deserializes"))
}

/// Look up one game by id.
pub fn game(id: &str) -> Option<&'static Game> {
    registry().get(id)
}
