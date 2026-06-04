package project

// `compatibility_date` must be `YYYY-MM-DD`; `2026-5-1` (unpadded) does
// not match the pattern.
#Project & {
	name: "pollen-alert"
	kind: "rust-worker"
	wrangler: {
		main:               "build/worker/shim.mjs"
		compatibility_date: "2026-5-1"
	}
}
