# `scoretracker`

A small, phone-friendly, **public** card-game score tracker, served as a
Cloudflare Worker at [`scoretracker.kolohelios.com`](https://scoretracker.kolohelios.com).
No login — anyone with the link can keep score at the table.

## Games

The engine is generic over a **scoring model**; each game is a static
definition in [`games/`](./games) (CUE, embedded at build). Launch games:

| Game     | Model         | Rules |
| -------- | ------------- | ----- |
| Phase 10 | `roundPoints` | Lowest total wins. `1–9 → 5`, `10–12 → 10`, `skip → 15`, `wild → 25`. |
| tic      | `roundPoints` | Lowest total wins. Standard deck: `ace → 20`, face cards → `10`, `2–10 → face value`, `wild → 50`. Going out first "tics" — each tic is worth `-5` at game end. |
| Skip-Bo  | `matchWins`   | No rounds — each finished game records a winner; most wins leads. |

Round input is free text: `1, 2, 3` · `wild, 5, 5` · `skip` · `no cards` ·
empty (no cards). Players default to **Jon** and **Jessica** and are editable
per game.

### Adding a game

Add `games/<id>.cue` (validated against `games/schema.cue`), then redeploy —
no engine code. `build.rs` runs `cue export` over `games/` into the JSON that
`engine::config` embeds.

## URLs

- `GET /` — the app (`?game=<type>&id=<instance>`, default `phase10` / `home`).
- `GET /api/games` — the embedded game registry.
- `GET /api/game/:type/:id` — current state.
- `POST /api/game/:type/:id/round` — add a round (`{entries, award?}`) or
  record a game (`{winner}`).
- `POST /api/game/:type/:id/undo` — drop the last round/game.
- `POST /api/game/:type/:id/reset` — start fresh (optional `{players}`).

Each `(type, id)` is one `GameState` Durable Object; totals are recomputed
server-side on every mutation.

## Local development

Everything runs inside the project devshell (entered automatically by
`direnv`, or `nix develop`):

```sh
just validate          # fmt, clippy, tests, flake check — the CI gate
cargo test             # scoring/parsing + config vet
worker-build --release # compile to wasm
wrangler dev           # run locally at http://localhost:8787
```

## Deploy

Deploy is automated by CI (`.github/workflows/scoretracker-deploy.yml`): PRs
get a `*.workers.dev` preview; merging to `main` deploys to
`scoretracker.kolohelios.com`. The custom-domain binding lives in
`infra/cloudflare-deploy` (Terraform generated from this project's `deploy:`
block). To deploy by hand from the devshell:

```sh
op run --env-file=.env -- wrangler deploy   # .env from .env.example
```
