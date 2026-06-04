package project

// `main` is required on `#Worker` — wrangler needs an entry point.
#Project & {
	name: "pollen-alert"
	kind: "rust-worker"
	worker: {
		compatibility_date: "2026-05-14"
	}
}
