package project

#Project & {
	name: "pollen-alert"
	kind: "rust-worker"
	worker: {}

	// Source of truth for apps/pollen-alert/wrangler.toml, which
	// `shaka deploy generate-wrangler` emits and `shaka preflight`
	// drift-checks. Never hand-edit the generated wrangler.toml — change
	// this block and regenerate.
	wrangler: {
		main:               "build/worker/shim.mjs"
		compatibility_date: "2026-05-14"

		// `worker-build` compiles the crate to wasm32-unknown-unknown and
		// emits `build/worker/shim.mjs` + the WASM module wrangler uploads.
		// The `cargo install` is a no-op on subsequent runs and matches the
		// workers-rs documented idiom; the devshell adds `~/.cargo/bin` to
		// PATH so the installed binary is visible to the build step.
		build: {
			command: "cargo install -q worker-build --locked && worker-build --release"
		}

		// Nightly cron firing the scheduled handler. `0 2 * * *` UTC
		// translates to 7 PM PDT / 6 PM PST locally — DST drift accepted
		// (v1 keeps the UTC value stable rather than tracking local time).
		cron: ["0 2 * * *"]

		// Non-secret runtime config. Latitude/longitude pin Bainbridge
		// Island, WA; `TZ` is what Open-Meteo uses to return local-time
		// hours. `THRESHOLD` and `MIN_CONSECUTIVE_HOURS` match the v1 rules
		// in `scoring`/`alert`. `DRY_RUN=true` swaps the Pushover notifier
		// for `LogNotifier` so the pipeline can be exercised end-to-end
		// without sending real notifications — useful for staging before
		// secrets are wired or after a config change.
		vars: {
			DRY_RUN:               "true"
			LAT:                   "47.625"
			LON:                   "-122.5"
			MIN_CONSECUTIVE_HOURS: "3"
			THRESHOLD:             "5"
			TZ:                    "America/Los_Angeles"
		}

		// Declared secret names; values never live in the repo. Set each
		// out-of-band via `wrangler secret put <NAME>`. The generator
		// surfaces these as a comment in wrangler.toml.
		secrets: ["PUSHOVER_APP_TOKEN", "PUSHOVER_USER_KEY"]
	}
}
