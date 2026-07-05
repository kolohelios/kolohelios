//! Wasm-only Worker runtime: the `fetch` entrypoint that serves the
//! frontend and routes `/api/game/<type>/<id>/...` to the right Durable
//! Object, and the `GameState` object itself. Cfg-gated to `wasm32` because
//! everything here needs `worker` runtime types; native `cargo test`
//! exercises the pure `engine` modules instead.

use std::collections::BTreeMap;

use serde::Deserialize;
// `wasm_bindgen` is re-exported from `worker` and must be in scope by name:
// the `#[durable_object]` macro expands to JS glue that references it.
use worker::{
    durable_object, event, wasm_bindgen, Context, DurableObject, Env, Error, Headers, Method,
    Request, Response, ResponseBuilder, Result, State,
};

use crate::engine::config::{self, Model};
use crate::engine::error::EngineError;
use crate::engine::state::{self, GameData};

/// The single-page frontend, embedded so the Worker is self-contained.
const INDEX_HTML: &str = include_str!("../public/index.html");

/// Body for `POST /round` — `entries`/`award` for `roundPoints`, `winner`
/// for `matchWins`. Unused fields are simply absent.
#[derive(Debug, Default, Deserialize)]
struct RoundRequest {
    #[serde(default)]
    entries: BTreeMap<String, String>,
    #[serde(default)]
    award: Option<String>,
    #[serde(default)]
    winner: Option<String>,
}

/// Body for `POST /reset` — an optional new roster and/or round-1 dealer.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetRequest {
    #[serde(default)]
    players: Option<Vec<String>>,
    #[serde(default)]
    first_dealer: Option<String>,
}

#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let path = req.path();

    // The embedded game registry, so the frontend renders generically.
    if path == "/api/games" {
        return json_ok(200, config::registry());
    }

    // `/api/game/<type>/<id>[/<action>]` → that game instance's DO.
    if path.starts_with("/api/game/") {
        let segments = path_segments(&path);
        let (Some(game_type), Some(id)) = (segments.first(), segments.get(1)) else {
            return json_err(404, "not found", "expected /api/game/<type>/<id>");
        };
        if config::game(game_type).is_none() {
            return json_err(404, "unknown game", "no game type with that id");
        }
        let key = format!("{game_type}:{id}");
        let stub = env.durable_object("GAME")?.id_from_name(&key)?.get_stub()?;
        return stub.fetch_with_request(req).await;
    }

    // Everything else (GET) is the single-page app.
    if matches!(req.method(), Method::Get) && !path.starts_with("/api/") {
        return html_response();
    }

    json_err(404, "not found", "no such route")
}

/// Split the part after `/api/game/` into non-empty path segments
/// (`[type, id, action?]`).
fn path_segments(path: &str) -> Vec<&str> {
    path.strip_prefix("/api/game/")
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// One Durable Object per game instance (`idFromName("<type>:<id>")`). The
/// whole game is a single JSON blob under the `data` key; every mutation
/// recomputes totals from history so client-supplied totals are never
/// trusted.
#[durable_object]
pub struct GameState {
    state: State,
    #[allow(dead_code)]
    env: Env,
}

impl DurableObject for GameState {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let path = req.path();
        let segments = path_segments(&path);
        let Some(game_type) = segments.first() else {
            return json_err(404, "not found", "missing game type");
        };
        let Some(game) = config::game(game_type) else {
            return json_err(404, "unknown game", "no game type with that id");
        };
        let action = segments.get(2).copied().unwrap_or("");

        let mut data = self
            .state
            .storage()
            .get::<GameData>("data")
            .await?
            .unwrap_or_else(|| GameData::new(game));

        let method = req.method();
        let mutated;
        let outcome: std::result::Result<(), EngineError> = match (&method, action) {
            (Method::Get, "") => {
                mutated = false;
                Ok(())
            }
            (Method::Post, "round") => {
                mutated = true;
                let body: RoundRequest = match req.json().await {
                    Ok(b) => b,
                    Err(_) => return json_err(400, "invalid body", "expected a JSON object"),
                };
                match &game.model {
                    Model::RoundPoints(_) => {
                        state::apply_round(&mut data, game, &body.entries, body.award)
                    }
                    Model::MatchWins(_) => match body.winner {
                        Some(winner) => state::apply_match(&mut data, game, &winner),
                        None => Err(EngineError::BadRequest {
                            detail: "recording a game needs a 'winner'".to_owned(),
                        }),
                    },
                }
            }
            (Method::Post, "undo") => {
                mutated = true;
                state::undo(&mut data, game)
            }
            (Method::Post, "reset") => {
                mutated = true;
                let body = req.json::<ResetRequest>().await.unwrap_or_default();
                state::reset(&mut data, game, body.players, body.first_dealer);
                Ok(())
            }
            _ => return json_err(404, "not found", "no such action"),
        };

        match outcome {
            Ok(()) => {
                if mutated {
                    self.state.storage().put("data", &data).await?;
                }
                json_ok(200, &data)
            }
            Err(e) => json_err(400, "invalid", &e.to_string()),
        }
    }
}

fn html_response() -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", "text/html; charset=utf-8")?;
    Ok(ResponseBuilder::new()
        .with_status(200)
        .with_headers(headers)
        .fixed(INDEX_HTML.as_bytes().to_vec()))
}

fn json_ok<T: serde::Serialize>(status: u16, body: &T) -> Result<Response> {
    let bytes = serde_json::to_vec(body).map_err(|e| Error::RustError(e.to_string()))?;
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    Ok(ResponseBuilder::new()
        .with_status(status)
        .with_headers(headers)
        .fixed(bytes))
}

fn json_err(status: u16, error: &str, detail: &str) -> Result<Response> {
    json_ok(
        status,
        &serde_json::json!({ "error": error, "detail": detail }),
    )
}
