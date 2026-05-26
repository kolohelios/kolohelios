# pollen-alert

A Cloudflare Workers `cron` that fires nightly, scores overnight
pollen-trap risk for the configured location, and sends a `Pushover`
notification when conditions cross threshold — prompting closing the
windows before sleeping.

## How it works

Once a night the worker hits Open-Meteo for the next 12-ish hours of
hourly forecast (humidity, wind, precipitation probability, cloud cover),
filters to the overnight window (`19:00`–`07:00` local), scores each
hour against the v1 rules in `src/scoring.rs`, and asks
`src/alert.rs` for the first qualifying run of `≥ THRESHOLD` points
across `≥ MIN_CONSECUTIVE_HOURS` consecutive hours. On a hit, the
notifier sends a `Pushover` push; otherwise it logs and exits. With
`DRY_RUN = true`, the notifier is swapped for `LogNotifier`, which
writes the would-be message to `wrangler tail` instead of sending.

## Local development

```sh
cd apps/pollen-alert
direnv allow                     # one-time; loads the per-project flake
just test                        # native unit + integration tests
just build                       # cargo check + wasm-build (release)
nix develop . --command wrangler dev
```

`just validate` runs the same checks `shaka preflight` runs in CI
(`fmt`, `clippy`, `cargo deny`, `machete`, `test`, `coverage`,
`wasm-check`, `worker-build-check`, `nix fmt --check`,
`nix flake check`, whitespace).

The native test suite exercises every pure module
(`scoring`, `alert`, `pipeline`, the `forecast` parser) and reads
the captured Open-Meteo response in
`tests/fixtures/open-meteo/bainbridge-overnight.json` for the
forecast integration test. The Worker runtime (`env` reader, HTTP
adapters, scheduled handler) is cfg-gated to `wasm32` so it doesn't
need to compile under native `cargo test`.

## Configuration

Non-secret `config` lives in `wrangler.toml` under `[vars]`:

| Key                     | Default                 | Meaning                                           |
| ----------------------- | ----------------------- | ------------------------------------------------- |
| `LAT`                   | `47.625`                | Latitude (`Bainbridge` Island).                   |
| `LON`                   | `-122.5`                | Longitude.                                        |
| `TZ`                    | `America/Los_Angeles`   | Open-Meteo timezone (drives local-time hours).    |
| `THRESHOLD`             | `5`                     | Minimum per-hour score to count as risky.         |
| `MIN_CONSECUTIVE_HOURS` | `3`                     | Minimum consecutive risky hours to trigger.       |
| `DRY_RUN`               | `true`                  | When `true`, log instead of sending to Pushover.  |

Tuning the threshold or the consecutive-hour requirement is a
`wrangler.toml` edit and a `wrangler deploy` away.

## Secrets

Set once per worker environment:

```sh
nix develop . --command wrangler secret put PUSHOVER_APP_TOKEN
nix develop . --command wrangler secret put PUSHOVER_USER_KEY
```

Then flip `DRY_RUN = false` in `wrangler.toml` and re-deploy.

## Deploy

Today: laptop `wrangler deploy`.

```sh
nix develop . --command wrangler deploy
```

Switches to CI when the worker becomes a consumer of the reusable
`cf-deploy.yml` workflow (kolohelios/kolohelios#114) — at that point a
`ci.deploy` block in `project.cue` generates
`.github/workflows/pollen-alert-deploy.yml` and the laptop step goes
away. See kolohelios/kolohelios#411 for the tracker.

## Schedule

`0 2 * * *` UTC (configured in `wrangler.toml [triggers]`). In PDT
this is `7 PM` local; in PST it's `6 PM` local. DST drift is
accepted — v1 keeps the UTC time stable rather than tracking local
time. Adjust by editing `[triggers].crons` and re-deploying.
