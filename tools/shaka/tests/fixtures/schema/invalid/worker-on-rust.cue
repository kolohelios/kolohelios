package project

// `wrangler` lives on `rust-worker`, not `rust-cli` — wrangler config
// only makes sense for a wasm worker build.
#Project & {
	name: "pollen-alert"
	kind: "rust-cli"
	cli: {
		binaryName: "pollen-alert"
	}
	wrangler: {
		main:               "build/worker/shim.mjs"
		compatibility_date: "2026-05-14"
	}
}
