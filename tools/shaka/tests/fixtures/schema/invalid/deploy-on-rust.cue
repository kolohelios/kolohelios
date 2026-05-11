package project

// deploy lives on rust-worker, not rust — the only target today is
// cloudflare-worker, which only makes sense for wasm builds.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust"
	coverage: {
		line: {
			fail: 30
		}
		branch: {
			fail: 50
		}
	}
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "kolohelios.com"
		zone:         "kolohelios.com"
	}
}
