package project

// Worker app using Workers Static Assets (the portfolio shape): a
// `wrangler` block with `assets.directory` and no triggers/vars/build.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	wrangler: {
		main:               "build/worker/shim.mjs"
		compatibility_date: "2026-05-01"
		assets: {
			directory: "./dist"
		}
	}
}
