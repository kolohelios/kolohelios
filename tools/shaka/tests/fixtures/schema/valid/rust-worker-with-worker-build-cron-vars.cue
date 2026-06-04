package project

// Worker app with a custom build, a scheduled cron trigger, non-secret
// vars, and declared secret names (the pollen-alert shape). Exercises
// every optional field of `#Wrangler` at once.
#Project & {
	name: "pollen-alert"
	kind: "rust-worker"
	wrangler: {
		main:               "build/worker/shim.mjs"
		compatibility_date: "2026-05-14"
		build: {
			command: "cargo install -q worker-build --locked && worker-build --release"
		}
		cron: ["0 2 * * *"]
		vars: {
			DRY_RUN:               "true"
			LAT:                   "47.625"
			LON:                   "-122.5"
			MIN_CONSECUTIVE_HOURS: "3"
			THRESHOLD:             "5"
			TZ:                    "America/Los_Angeles"
		}
		secrets: ["PUSHOVER_APP_TOKEN", "PUSHOVER_USER_KEY"]
	}
}
