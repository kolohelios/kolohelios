package project

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
