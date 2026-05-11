package project

#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "kolohelios.com"
	}
}
