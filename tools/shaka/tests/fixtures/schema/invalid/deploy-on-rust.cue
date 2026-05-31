package project

// deploy lives on rust-worker, not rust-cli — the only target today
// is cloudflare-worker, which only makes sense for wasm builds.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-cli"
	cli: {
		binaryName: "kolohelios-portfolio"
	}
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
