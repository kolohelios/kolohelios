package project

// Worker app with a Cloudflare Worker deploy block. Coverage stays
// optional even when deploy is declared.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {}
	deploy: {
		target:       "cloudflare-worker"
		customDomain: "kolohelios.com"
		zone:         "kolohelios.com"
	}
}
