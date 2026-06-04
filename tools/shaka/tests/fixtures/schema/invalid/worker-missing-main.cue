package project

// `main` is required on `#Wrangler` — wrangler needs an entry point.
#Project & {
	name: "pollen-alert"
	kind: "rust-worker"
	wrangler: {
		compatibility_date: "2026-05-14"
	}
}
